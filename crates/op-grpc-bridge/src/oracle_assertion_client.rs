//! Client-side acquisition of one OIA1 assertion from the authenticated decoy.
//!
//! This is deliberately only a transport helper. It does not verify the
//! decoy signature, register a principal, provision an identity container, or
//! mint SID1. Those operations remain owned by the bridge and MutationEngine.

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use futures::StreamExt;
use reqwest::redirect::Policy;

use crate::grpc_client::GhostbridgeCallMetadata;

/// Maximum decoded OIA1 envelope accepted by the bridge.
pub const MAX_ASSERTION_WIRE_BYTES: usize = 16 * 1024;
/// Maximum response body, including the base64 representation and line ending.
pub const MAX_ASSERTION_RESPONSE_BYTES: usize = 4 * MAX_ASSERTION_WIRE_BYTES.div_ceil(3) + 2;
/// The issuer is a local/inner-network service; this is a deliberately short
/// bound for one acquisition request.
pub const DEFAULT_ASSERTION_TIMEOUT: Duration = Duration::from_secs(8);

/// Acquires one signed OIA1 envelope from an explicitly selected decoy.
///
/// The caller must supply the local WireGuard inner address. Binding the HTTP
/// socket to it selects the intended route; the caller is responsible for
/// ensuring that address is already present on its authenticated WireGuard
/// interface. Signature and trust validation are intentionally left to
/// `AssertionValidator` on the bridge.
pub struct DecoyAssertionClient {
    endpoint: SocketAddr,
    source: IpAddr,
    response_limit: usize,
    http: reqwest::Client,
}

impl DecoyAssertionClient {
    /// Construct a client with no environment, DNS, proxy, or default endpoint.
    pub fn new(endpoint: SocketAddr, source: IpAddr) -> Result<Self> {
        Self::with_limits(
            endpoint,
            source,
            DEFAULT_ASSERTION_TIMEOUT,
            MAX_ASSERTION_RESPONSE_BYTES,
        )
    }

    /// Private constructor used by fixture tests for bounded limits.
    fn with_limits(
        endpoint: SocketAddr,
        source: IpAddr,
        timeout: Duration,
        response_limit: usize,
    ) -> Result<Self> {
        if timeout.is_zero() || timeout > DEFAULT_ASSERTION_TIMEOUT {
            bail!("OIA1 assertion timeout must be positive and at most 8s");
        }
        if response_limit == 0 || response_limit > MAX_ASSERTION_RESPONSE_BYTES {
            bail!("OIA1 assertion response limit is outside the supported bound");
        }
        if endpoint.port() == 0 {
            bail!("OIA1 assertion endpoint port must be nonzero");
        }
        if endpoint.ip().is_unspecified() || endpoint.ip().is_multicast() {
            bail!("OIA1 assertion endpoint must be a specific unicast address");
        }
        if source.is_unspecified() || source.is_multicast() {
            bail!("OIA1 assertion source must be a specific unicast address");
        }
        if endpoint.ip().is_ipv4() != source.is_ipv4() {
            bail!("OIA1 assertion endpoint and source address families differ");
        }
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .local_address(Some(source))
            .timeout(timeout)
            .build()
            .context("building bounded OIA1 assertion client")?;
        Ok(Self {
            endpoint,
            source,
            response_limit,
            http,
        })
    }

    /// Fetch and frame-check exactly one OIA1 assertion.
    ///
    /// The returned bytes are the original decoded signed envelope. No retry
    /// or reuse is performed; callers should attach the result to one RPC.
    pub async fn acquire(&self) -> Result<GhostbridgeCallMetadata> {
        let url = format!("http://{}/assertion", self.endpoint);
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .with_context(|| format!("requesting OIA1 assertion from {url}"))?;
        if response.status() != reqwest::StatusCode::OK {
            bail!("OIA1 assertion issuer returned HTTP {}", response.status());
        }

        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("reading OIA1 assertion response")?;
            if body.len().saturating_add(chunk.len()) > self.response_limit {
                bail!(
                    "OIA1 assertion response exceeds {} bytes",
                    self.response_limit
                );
            }
            body.extend_from_slice(&chunk);
        }

        let encoded = std::str::from_utf8(&body).context("OIA1 assertion response is not UTF-8")?;
        let wire = base64::engine::general_purpose::STANDARD
            .decode(encoded.trim())
            .context("OIA1 assertion response is not standard base64")?;
        if wire.len() > MAX_ASSERTION_WIRE_BYTES {
            bail!("OIA1 assertion exceeds {} bytes", MAX_ASSERTION_WIRE_BYTES);
        }
        let signed = op_identity::oracle_assertion::SignedAssertion::from_wire(&wire)
            .map_err(|error| anyhow::anyhow!("invalid OIA1 assertion envelope: {error}"))?;
        if signed.assertion.tunnel_inner_ip != self.source {
            bail!(
                "OIA1 assertion inner IP {} does not match bound source {}",
                signed.assertion.tunnel_inner_ip,
                self.source
            );
        }

        Ok(GhostbridgeCallMetadata {
            sealed_id: None,
            oracle_assertion: Some(wire),
            session_genesis: None,
            trace_id: None,
            wireguard_pubkey: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::extract::ConnectInfo;
    use axum::http::{HeaderValue, StatusCode, Uri};
    use axum::routing::any;
    use ed25519_dalek::SigningKey;
    use op_identity::oracle_assertion::DecoyIssuer;
    use std::sync::{Arc, Mutex};
    use tokio::task::JoinHandle;

    const SOURCE: IpAddr = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

    async fn serve(app: Router) -> (SocketAddr, JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        (address, task)
    }

    fn fixture_wire_for(source: IpAddr) -> Vec<u8> {
        let issuer = DecoyIssuer::new(
            SigningKey::from_bytes(&[7u8; 32]),
            "fixture-decoy",
            Duration::from_secs(900),
        );
        issuer
            .issue(
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
                source,
                Duration::from_secs(60),
            )
            .unwrap()
            .to_wire()
    }

    fn fixture_wire() -> Vec<u8> {
        fixture_wire_for(SOURCE)
    }

    #[tokio::test]
    async fn acquires_one_assertion_from_exact_path_and_source() {
        let observed = Arc::new(Mutex::new(None));
        let observed_for_handler = Arc::clone(&observed);
        let wire = fixture_wire();
        let encoded = base64::engine::general_purpose::STANDARD.encode(&wire);
        let app = Router::new().fallback(any(
            move |ConnectInfo(peer): ConnectInfo<SocketAddr>, uri: Uri| {
                let observed = Arc::clone(&observed_for_handler);
                let encoded = encoded.clone();
                async move {
                    *observed.lock().unwrap() = Some((peer, uri.path().to_string()));
                    (StatusCode::OK, encoded)
                }
            },
        ));
        let (endpoint, task) = serve(app).await;
        let result = DecoyAssertionClient::new(endpoint, SOURCE)
            .unwrap()
            .acquire()
            .await
            .unwrap();
        task.abort();

        assert_eq!(result.sealed_id, None);
        assert_eq!(result.session_genesis, None);
        assert_eq!(result.oracle_assertion.as_deref(), Some(wire.as_slice()));
        let (peer, path) = observed.lock().unwrap().clone().unwrap();
        assert_eq!(peer.ip(), SOURCE);
        assert_eq!(path, "/assertion");
    }

    #[tokio::test]
    async fn rejects_http_403_denial() {
        let app = Router::new().fallback(any(|| async {
            (StatusCode::FORBIDDEN, "no verified peer for source\n")
        }));
        let (endpoint, task) = serve(app).await;
        let error = match DecoyAssertionClient::new(endpoint, SOURCE)
            .unwrap()
            .acquire()
            .await
        {
            Ok(_) => panic!("HTTP 403 must not produce an assertion"),
            Err(error) => error,
        };
        task.abort();
        assert!(error.to_string().contains("HTTP 403"));
    }

    #[tokio::test]
    async fn rejects_redirects_without_following_them() {
        let app = Router::new().fallback(any(|| async {
            (
                StatusCode::FOUND,
                [(
                    axum::http::header::LOCATION,
                    HeaderValue::from_static("/other"),
                )],
                "redirect",
            )
        }));
        let (endpoint, task) = serve(app).await;
        let error = match DecoyAssertionClient::new(endpoint, SOURCE)
            .unwrap()
            .acquire()
            .await
        {
            Ok(_) => panic!("redirect must not produce an assertion"),
            Err(error) => error,
        };
        task.abort();
        assert!(error.to_string().contains("HTTP 302"));
    }

    #[tokio::test]
    async fn rejects_malformed_and_oversized_responses() {
        let malformed = Router::new().fallback(any(|| async { (StatusCode::OK, "not-base64") }));
        let (endpoint, task) = serve(malformed).await;
        let error = match DecoyAssertionClient::new(endpoint, SOURCE)
            .unwrap()
            .acquire()
            .await
        {
            Ok(_) => panic!("malformed response must be rejected"),
            Err(error) => error,
        };
        task.abort();
        assert!(error.to_string().contains("base64"));

        let oversized = "A".repeat(MAX_ASSERTION_RESPONSE_BYTES + 1);
        let app = Router::new().fallback(any(move || {
            let oversized = oversized.clone();
            async move { (StatusCode::OK, oversized) }
        }));
        let (endpoint, task) = serve(app).await;
        let error = match DecoyAssertionClient::new(endpoint, SOURCE)
            .unwrap()
            .acquire()
            .await
        {
            Ok(_) => panic!("oversized response must be rejected"),
            Err(error) => error,
        };
        task.abort();
        assert!(error.to_string().contains("exceeds"));
    }

    #[tokio::test]
    async fn rejects_valid_base64_with_malformed_or_trailing_wire() {
        let mut trailing = fixture_wire();
        trailing.push(0);
        for body in [
            base64::engine::general_purpose::STANDARD.encode(b"OIA1"),
            base64::engine::general_purpose::STANDARD.encode(trailing),
        ] {
            let app = Router::new().fallback(any({
                let body = body.clone();
                move || {
                    let body = body.clone();
                    async move { (StatusCode::OK, body) }
                }
            }));
            let (endpoint, task) = serve(app).await;
            let error = match DecoyAssertionClient::new(endpoint, SOURCE)
                .unwrap()
                .acquire()
                .await
            {
                Ok(_) => panic!("malformed OIA1 wire must be rejected"),
                Err(error) => error,
            };
            task.abort();
            assert!(error.to_string().contains("envelope"));
        }
    }

    #[tokio::test]
    async fn rejects_signed_inner_ip_mismatch() {
        let other = IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 2));
        let encoded = base64::engine::general_purpose::STANDARD.encode(fixture_wire_for(other));
        let app = Router::new().fallback(any(move || {
            let encoded = encoded.clone();
            async move { (StatusCode::OK, encoded) }
        }));
        let (endpoint, task) = serve(app).await;
        let error = match DecoyAssertionClient::new(endpoint, SOURCE)
            .unwrap()
            .acquire()
            .await
        {
            Ok(_) => panic!("inner IP mismatch must be rejected"),
            Err(error) => error,
        };
        task.abort();
        assert!(error.to_string().contains("does not match"));
    }

    #[tokio::test]
    async fn rejects_slow_response_with_bounded_timeout() {
        let app = Router::new().fallback(any(|| async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            (StatusCode::OK, "anything")
        }));
        let (endpoint, task) = serve(app).await;
        let error = match DecoyAssertionClient::with_limits(
            endpoint,
            SOURCE,
            Duration::from_millis(10),
            MAX_ASSERTION_RESPONSE_BYTES,
        )
        .unwrap()
        .acquire()
        .await
        {
            Ok(_) => panic!("slow response must time out"),
            Err(error) => error,
        };
        task.abort();
        let chain = format!("{error:#}");
        assert!(
            chain.contains("timed out") || chain.contains("deadline") || chain.contains("elapsed")
        );
    }
}
