//! Shared tonic `Channel` connector.
//!
//! Supports two endpoint forms so the same `ZEROCLAW_GRPC` value works in dev
//! and in production:
//!   - TCP:  `http://host:port`            — local dev / gRPC-Web
//!   - UDS:  `unix:/run/ghostbridge/container.sock`
//!
//! Production is the Unix-socket form: the assistant container has no IP, so the
//! console reaches the op-grpc-bridge over `/run/ghostbridge/container.sock`
//! (native gRPC + reflection). `tonic::transport::Endpoint::from_shared` only
//! dials TCP authorities, so the `unix:` form is wired through
//! `connect_with_connector` with a `UnixStream` dialer.

use anyhow::{Context, Result};
use hyper_util::rt::TokioIo;
use tokio::net::UnixStream;
use tonic::transport::{Channel, Endpoint, Uri};
use tower::service_fn;

/// Parse a `unix:` endpoint into its filesystem path.
///
/// Accepts `unix:/path`, `unix://authority/path` (authority ignored), and
/// `unix:///path`. Returns `None` for non-`unix:` endpoints.
fn unix_socket_path(endpoint: &str) -> Option<String> {
    let rest = endpoint.strip_prefix("unix:")?;
    let path = match rest.strip_prefix("//") {
        // `unix://authority/path` or `unix:///path` — drop the authority.
        Some(after) => match after.find('/') {
            Some(idx) => &after[idx..],
            None => after,
        },
        // `unix:/path`
        None => rest,
    };
    Some(path.to_string())
}

/// Open a tonic `Channel` to either a `unix:` socket or a TCP `http://` endpoint.
pub async fn connect_channel(endpoint: &str) -> Result<Channel> {
    if let Some(path) = unix_socket_path(endpoint) {
        // The HTTP authority below is a placeholder required by tonic; the
        // connector ignores it and dials the Unix socket instead.
        let channel = Endpoint::try_from("http://[::]:50051")
            .context("placeholder URI for Unix-socket endpoint")?
            .connect_with_connector(service_fn(move |_: Uri| {
                let path = path.clone();
                async move { Ok::<_, std::io::Error>(TokioIo::new(UnixStream::connect(path).await?)) }
            }))
            .await
            .with_context(|| format!("connecting gRPC over Unix socket: {endpoint}"))?;
        Ok(channel)
    } else {
        let channel = Endpoint::from_shared(endpoint.to_string())
            .with_context(|| format!("invalid gRPC endpoint: {endpoint}"))?
            .connect()
            .await
            .with_context(|| format!("connecting gRPC over TCP: {endpoint}"))?;
        Ok(channel)
    }
}
