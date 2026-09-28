//! The host cannot prove a browser's identity from its TCP source address.
//! Human credential pickup belongs to the decoy's authenticated WireGuard door.

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{HeaderMap, StatusCode, header},
    response::Response,
};
use serde_json::json;
use std::net::SocketAddr;

/// Kept as an explicit compatibility route so clients receive a clear failure.
/// A local peer, Host header, and Origin header are not proof of a principal.
pub async fn identity_self_handler(
    ConnectInfo(_peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let (status, message) =
        if crate::middleware::security::request_site_is_local_same_origin(&headers) {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "identity pickup requires the decoy's authenticated endpoint",
            )
        } else {
            (StatusCode::FORBIDDEN, "cross-site identity access denied")
        };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(json!({ "error": message }).to_string()))
        .expect("response with valid body should not fail")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn loopback_with_claimed_local_host_cannot_receive_credential() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "localhost:8080".parse().unwrap());
        let response =
            identity_self_handler(ConnectInfo("127.0.0.1:1234".parse().unwrap()), headers).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn rebound_host_cannot_receive_credential() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "attacker.example:8080".parse().unwrap());
        let response =
            identity_self_handler(ConnectInfo("127.0.0.1:1234".parse().unwrap()), headers).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
