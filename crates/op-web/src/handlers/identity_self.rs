//! `GET /api/identity/self` — the SID1-aware client injection for the browser.
//!
//! A human who arrives over WireGuard has already been authenticated by the
//! kernel handshake at the decoy; what reaches this host is their inner source
//! address. This handler turns that arrival into the credential the 3tched UI
//! forwards unchanged to the `:8090` door: it maps the request's TCP source
//! address (never a forwarded header) to a projected identity session, reads
//! that session's MutationEngine-authored SID1 from the private credential
//! projection, and returns it together with the door address.
//!
//! Which source addresses may resolve which session is an operator decision
//! carried in `OP_WEB_HUMAN_PEERS`: a comma-separated list of `ip` (resolve the
//! single current human session, the same implicit rule local host clients use)
//! or `ip=session_id` (resolve exactly that session). Anything else is 404 —
//! the UI then falls back to its connect gate. The UI never mints, derives or
//! stores the credential; this endpoint never rebuilds one either — it forwards
//! the sealed bytes exactly as the sled holds them.

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{header, StatusCode},
    response::Response,
};
use base64::Engine as _;
use serde_json::json;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use tracing::{info, warn};

/// Comma-separated `ip` or `ip=session_id` entries allowed to resolve a SID1.
pub const HUMAN_PEERS_ENV: &str = "OP_WEB_HUMAN_PEERS";
/// Door origin handed to the UI; defaults to the svc0 tonic TLS door.
pub const DOOR_URL_ENV: &str = "OP_WEB_DOOR_URL";
const DEFAULT_DOOR_URL: &str = "https://10.0.0.3:8090";

/// Transport scope the returned SID1 must be sealed for: the UI speaks gRPC-Web
/// to the door, and the bridge authenticates it on the `grpc` scope.
const TRANSPORT: &str = "grpc";

/// Parse `OP_WEB_HUMAN_PEERS` into `source ip -> optional session selector`.
/// Malformed entries are dropped, which fails closed for that address.
pub fn parse_human_peers(raw: &str) -> HashMap<IpAddr, Option<String>> {
    raw.split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let (ip, session) = match entry.split_once('=') {
                Some((ip, session)) => (ip.trim(), Some(session.trim().to_string())),
                None => (entry, None),
            };
            let session = session.filter(|value| !value.is_empty());
            ip.parse::<IpAddr>().ok().map(|ip| (ip, session))
        })
        .collect()
}

fn configured_human_peers() -> HashMap<IpAddr, Option<String>> {
    parse_human_peers(&std::env::var(HUMAN_PEERS_ENV).unwrap_or_default())
}

fn configured_door_url() -> String {
    std::env::var(DOOR_URL_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_DOOR_URL.to_string())
}

/// Convert the sled's unpadded base64url SID1 payload to the standard base64
/// gRPC-Web carries in `-bin` metadata. Bytes are not interpreted.
pub fn grpc_web_sealed_id(header_payload: &str) -> Result<String, base64::DecodeError> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(header_payload)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn json_response(status: StatusCode, body: serde_json::Value) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body.to_string()))
        .expect("response with valid body should not fail")
}

fn not_found() -> Response {
    json_response(
        StatusCode::NOT_FOUND,
        json!({ "error": "no identity is projected for this source" }),
    )
}

/// Resolve the requester's SID1 by TCP source address.
pub async fn identity_self_handler(ConnectInfo(peer): ConnectInfo<SocketAddr>) -> Response {
    let source = match peer.ip() {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        v4 => v4,
    };
    let peers = configured_human_peers();
    let Some(selector) = peers.get(&source) else {
        info!(%source, "identity/self: source not in {HUMAN_PEERS_ENV}");
        return not_found();
    };

    let session = match op_identity::resolve_identity_credential_session(selector.as_deref()) {
        Ok(session) => session,
        Err(error) => {
            warn!(%source, %error, "identity/self: no credential session for source");
            return not_found();
        }
    };
    let payload = match session.sealed_id_header_for_transport(TRANSPORT) {
        Ok(payload) => payload,
        Err(error) => {
            warn!(%source, %error, "identity/self: sled has no SID1 valid for {TRANSPORT}");
            return not_found();
        }
    };
    let sealed_id = match grpc_web_sealed_id(payload) {
        Ok(sealed_id) => sealed_id,
        Err(error) => {
            warn!(%source, %error, "identity/self: sled SID1 payload is not base64url");
            return not_found();
        }
    };

    info!(%source, session_id = %session.session_id, "identity/self: served SID1 for WireGuard arrival");
    json_response(
        StatusCode::OK,
        json!({
            "door": configured_door_url(),
            "identity": { "kind": "sid1", "sealedId": sealed_id },
            "session_id": session.session_id,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_and_selector_entries_and_drops_garbage() {
        let peers = parse_human_peers(" 127.0.0.1, 10.0.0.5=f036f8d8-aabb ,::1, nope, 10.0.0.6= ");
        assert_eq!(peers.len(), 4);
        assert_eq!(peers[&"127.0.0.1".parse::<IpAddr>().unwrap()], None);
        assert_eq!(
            peers[&"10.0.0.5".parse::<IpAddr>().unwrap()],
            Some("f036f8d8-aabb".to_string())
        );
        assert_eq!(peers[&"::1".parse::<IpAddr>().unwrap()], None);
        assert_eq!(peers[&"10.0.0.6".parse::<IpAddr>().unwrap()], None);
    }

    #[test]
    fn empty_list_allows_nobody() {
        assert!(parse_human_peers("").is_empty());
    }

    #[test]
    fn sealed_id_is_re_encoded_not_reinterpreted() {
        let wire = b"SID1\x00\x01\xfe\xff arbitrary bytes";
        let url = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(wire);
        let standard = grpc_web_sealed_id(&url).unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(&standard)
                .unwrap(),
            wire
        );
        assert!(grpc_web_sealed_id("not base64!").is_err());
    }
}
