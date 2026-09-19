//! Oracle decoy assertion issuer.
//!
//! Runs on the decoy — the sole incoming WireGuard termination point. A human's
//! tunnel is verified by the kernel at handshake; this service turns that
//! kernel-level fact into a short-lived Ed25519-signed `OracleIdentityAssertion`
//! that the client carries INNER as gRPC metadata
//! (`x-oracle-identity-assertion-bin`) to `op-grpc-bridge`, the sole validator.
//!
//! Arrival-triggered: peer state is read when a request lands, never polled.
//!
//! Two doors for the same kernel-verified fact:
//! - `GET /assertion` — the base64 OIA1 envelope as `text/plain`, consumed by
//!   the bridge's Rust acquisition client.
//! - `GET /identity/self` — the same envelope as JSON in the shape the 3tched
//!   UI already parses from op-web (`{door, identity:{kind:"oia1",
//!   assertion}, expires_at}`), with CORS for the configured UI origin. This
//!   is the client pickup: the browser fetches it over the human's tunnel on
//!   page load and before expiry, and forwards it unchanged as
//!   `x-oracle-identity-assertion-bin` over WARP to the `:8090` door.
//!
//! The listener MUST be bound to a WireGuard inner address so that reaching it
//! already implies a completed handshake. Binding it to a public address would
//! let anyone mint an assertion for any inner IP they can name.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use ed25519_dalek::SigningKey;
use op_identity::oracle_assertion::DecoyIssuer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const DEFAULT_LISTEN: &str = "10.10.0.1:51888";
const DEFAULT_IFACE: &str = "wg0";
const DEFAULT_KEY_PATH: &str = "/etc/opdbus/decoy-signing-key";
const DEFAULT_KEY_ID_PATH: &str = "/etc/opdbus/decoy-key-id";
const DEFAULT_TTL_SECS: u64 = 300;

struct Config {
    listen: SocketAddr,
    iface: String,
    ttl: Duration,
    /// Exact browser origins allowed to fetch `/identity/self` cross-origin
    /// (the 3tched UI pages). Empty means no CORS headers are emitted.
    allowed_origins: Vec<String>,
    /// The `:8090` door the UI should dial with the issued assertion.
    door: String,
}

fn arg(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

/// Load the 32-byte Ed25519 seed. Base64 or hex, whitespace tolerated.
fn load_signing_key(path: &PathBuf) -> Result<SigningKey> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading signing key {}", path.display()))?;
    let raw = raw.trim();
    let bytes = match base64::engine::general_purpose::STANDARD.decode(raw) {
        Ok(b) => b,
        Err(_) => hex::decode(raw).context("signing key is neither base64 nor hex")?,
    };
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow!("signing key must be exactly 32 bytes"))?;
    Ok(SigningKey::from_bytes(&seed))
}

/// Exact peer lookup: source IP -> WireGuard pubkey.
///
/// Deliberately NOT `op_identity::wireguard::get_pubkey_for_ip`, which does a
/// substring match (`10.10.0.1` matches `10.10.0.11`) — unsound for identity.
///
/// Only a single-host (`/32`, `/128`) allowed-ip authorizes an identity. A peer
/// holding a wide route (e.g. `10.200.0.0/24`) must not thereby be able to claim
/// every address inside it. Ambiguous matches are refused rather than guessed.
fn pubkey_for_ip(iface: &str, peer_ip: IpAddr) -> Result<String> {
    let out = Command::new("wg")
        .args(["show", iface, "allowed-ips"])
        .output()
        .context("running `wg show`")?;
    if !out.status.success() {
        bail!("`wg show {iface} allowed-ips` failed");
    }
    let stdout = String::from_utf8_lossy(&out.stdout);

    let mut found: Option<String> = None;
    for line in stdout.lines() {
        let mut parts = line.split('\t');
        let (Some(pubkey), Some(ips)) = (parts.next(), parts.next()) else {
            continue;
        };
        for entry in ips
            .split(',')
            .map(str::trim)
            .filter(|e| !e.is_empty() && *e != "(none)")
        {
            let Some((addr, prefix)) = entry.rsplit_once('/') else {
                continue;
            };
            let Ok(addr) = addr.parse::<IpAddr>() else {
                continue;
            };
            let host_prefix = match addr {
                IpAddr::V4(_) => "32",
                IpAddr::V6(_) => "128",
            };
            if prefix != host_prefix || addr != peer_ip {
                continue;
            }
            match &found {
                Some(prev) if prev != pubkey => {
                    bail!("ambiguous: {peer_ip} is a host route on more than one peer")
                }
                Some(_) => {}
                None => found = Some(pubkey.to_string()),
            }
        }
    }
    found.ok_or_else(|| anyhow!("no peer holds {peer_ip} as a host route on {iface}"))
}

async fn respond(stream: &mut TcpStream, status: &str, body: &str) -> Result<()> {
    respond_with(stream, status, "text/plain", None, body).await
}

async fn respond_with(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    cors_origin: Option<&str>,
    body: &str,
) -> Result<()> {
    let cors = cors_origin
        .map(|origin| {
            format!("Access-Control-Allow-Origin: {origin}\r\nVary: Origin\r\nCache-Control: no-store\r\n")
        })
        .unwrap_or_default();
    let msg = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\n{cors}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(msg.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

/// The request's `Origin` header when it is one of the configured UI origins.
/// Exact string match; an unlisted origin gets no CORS header and the browser
/// refuses the response.
fn allowed_origin<'a>(req: &str, cfg: &'a Config) -> Option<&'a str> {
    let origin = req.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("origin").then(|| value.trim())
    })?;
    cfg.allowed_origins
        .iter()
        .find(|allowed| allowed.as_str() == origin)
        .map(String::as_str)
}

async fn handle(mut stream: TcpStream, peer: SocketAddr, issuer: &DecoyIssuer, cfg: &Config) {
    let mut buf = [0u8; 1024];
    let n = match stream.read(&mut buf).await {
        Ok(n) => n,
        Err(e) => {
            eprintln!("read error from {peer}: {e}");
            return;
        }
    };
    let req = String::from_utf8_lossy(&buf[..n]);
    let path = req
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("/");

    if path.starts_with("/healthz") {
        let _ = respond(&mut stream, "200 OK", "ok\n").await;
        return;
    }
    let json_pickup = path.starts_with("/identity/self");
    if !path.starts_with("/assertion") && !json_pickup {
        let _ = respond(&mut stream, "404 Not Found", "not found\n").await;
        return;
    }
    let cors_origin = json_pickup.then(|| allowed_origin(&req, cfg)).flatten();

    // The source address of a connection that arrived on the WG inner address
    // is the kernel's own verdict about which peer this is.
    let inner_ip = peer.ip();
    let pubkey = match pubkey_for_ip(&cfg.iface, inner_ip) {
        Ok(pk) => pk,
        Err(e) => {
            eprintln!("deny {inner_ip}: {e}");
            let _ = respond(
                &mut stream,
                "403 Forbidden",
                "no verified peer for source\n",
            )
            .await;
            return;
        }
    };

    match issuer.issue(&pubkey, inner_ip, cfg.ttl) {
        Ok(signed) => {
            let wire = base64::engine::general_purpose::STANDARD.encode(signed.to_wire());
            println!(
                "issued: ip={inner_ip} pubkey={pubkey} ttl={}s key_id={} pickup={}",
                cfg.ttl.as_secs(),
                issuer.key_id(),
                if json_pickup {
                    "identity/self"
                } else {
                    "assertion"
                }
            );
            if json_pickup {
                let body = pickup_json(&cfg.door, &wire, signed.assertion.expires_at);
                let _ = respond_with(
                    &mut stream,
                    "200 OK",
                    "application/json",
                    cors_origin,
                    &body,
                )
                .await;
            } else {
                let _ = respond(&mut stream, "200 OK", &wire).await;
            }
        }
        Err(e) => {
            eprintln!("issue failed for {inner_ip}: {e}");
            let _ = respond(&mut stream, "500 Internal Server Error", "issue failed\n").await;
        }
    }
}

/// The `/identity/self` body: the shape `3tched.com` `door-config.ts` parses
/// from op-web's same-origin endpoint, so one client path serves both the
/// local SID1 and the remote OIA1 pickup.
fn pickup_json(door: &str, assertion_b64: &str, expires_at: i64) -> String {
    serde_json::json!({
        "door": door,
        "identity": { "kind": "oia1", "assertion": assertion_b64 },
        "expires_at": expires_at,
    })
    .to_string()
}

#[tokio::main]
async fn main() -> Result<()> {
    let cfg = Config {
        listen: arg("--listen")
            .unwrap_or_else(|| DEFAULT_LISTEN.to_string())
            .parse()
            .context("parsing --listen")?,
        iface: arg("--iface").unwrap_or_else(|| DEFAULT_IFACE.to_string()),
        ttl: Duration::from_secs(
            arg("--ttl")
                .map(|t| t.parse::<u64>())
                .transpose()
                .context("parsing --ttl")?
                .unwrap_or(DEFAULT_TTL_SECS),
        ),
        allowed_origins: arg("--allow-origin")
            .or_else(|| std::env::var("OP_DECOY_ISSUER_ALLOWED_ORIGINS").ok())
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
        door: arg("--door")
            .or_else(|| std::env::var("OP_DECOY_ISSUER_DOOR").ok())
            .unwrap_or_else(|| "https://10.0.0.3:8090".to_string()),
    };

    if cfg.listen.ip().is_unspecified() {
        bail!(
            "refusing to bind {}: the listener must be on a WireGuard inner address, \
               otherwise anyone reachable can mint assertions",
            cfg.listen
        );
    }

    let key_path = PathBuf::from(arg("--key").unwrap_or_else(|| DEFAULT_KEY_PATH.to_string()));
    let signing_key = load_signing_key(&key_path)?;
    let key_id = match arg("--key-id") {
        Some(id) => id,
        None => std::fs::read_to_string(DEFAULT_KEY_ID_PATH)
            .context("reading key id")?
            .trim()
            .to_string(),
    };

    let issuer = DecoyIssuer::new(signing_key, key_id, cfg.ttl);
    let verifying =
        base64::engine::general_purpose::STANDARD.encode(issuer.verifying_key().to_bytes());

    let listener = TcpListener::bind(cfg.listen)
        .await
        .with_context(|| format!("binding {}", cfg.listen))?;
    println!(
        "op-decoy-issuer listening on {} iface={} key_id={} ttl={}s door={} pickup_origins={:?}",
        cfg.listen,
        cfg.iface,
        issuer.key_id(),
        cfg.ttl.as_secs(),
        cfg.door,
        cfg.allowed_origins
    );
    println!("verifying key (for the bridge trust store): {verifying}");

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => handle(stream, peer, &issuer, &cfg).await,
            Err(e) => eprintln!("accept error: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(origins: &[&str]) -> Config {
        Config {
            listen: "10.10.0.1:51888".parse().unwrap(),
            iface: "wg0".into(),
            ttl: Duration::from_secs(300),
            allowed_origins: origins.iter().map(|s| s.to_string()).collect(),
            door: "https://10.0.0.3:8090".into(),
        }
    }

    #[test]
    fn pickup_json_matches_the_ui_identity_shape() {
        let body = pickup_json("https://10.0.0.3:8090", "QUJDRA==", 1_700_000_000);
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["door"], "https://10.0.0.3:8090");
        assert_eq!(parsed["identity"]["kind"], "oia1");
        assert_eq!(parsed["identity"]["assertion"], "QUJDRA==");
        assert_eq!(parsed["expires_at"], 1_700_000_000);
    }

    #[test]
    fn cors_origin_is_exact_match_only() {
        let config = cfg(&["https://3tched.com", "http://localhost:5173"]);
        let req = "GET /identity/self HTTP/1.1\r\nHost: x\r\nOrigin: https://3tched.com\r\n\r\n";
        assert_eq!(allowed_origin(req, &config), Some("https://3tched.com"));
        let evil = "GET /identity/self HTTP/1.1\r\nOrigin: https://3tched.com.evil\r\n\r\n";
        assert_eq!(allowed_origin(evil, &config), None);
        let none = "GET /identity/self HTTP/1.1\r\nHost: x\r\n\r\n";
        assert_eq!(allowed_origin(none, &config), None);
        assert_eq!(allowed_origin(req, &cfg(&[])), None);
    }
}
