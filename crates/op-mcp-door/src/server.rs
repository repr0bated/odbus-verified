//! TLS-only listener (Requirement 1).
//!
//! The only way in is a rustls handshake. A client that speaks plaintext
//! HTTP fails the handshake and the connection is dropped without any HTTP
//! response. There is no plaintext code path to enable.

use crate::mcp::ClientAddr;
use anyhow::{Context, Result, bail};
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::future::Future;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// Load the door certificate and key. Errors name the file, never its bytes.
pub fn load_tls(cert_path: &Path, key_path: &Path) -> Result<Arc<ServerConfig>> {
    let key_meta = std::fs::metadata(key_path)
        .with_context(|| format!("TLS key {} is not readable", key_path.display()))?;
    if key_meta.mode() & 0o077 != 0 {
        bail!(
            "TLS key {} must not be readable by group or other",
            key_path.display()
        );
    }
    let cert_pem = std::fs::read(cert_path)
        .with_context(|| format!("TLS certificate {} is not readable", cert_path.display()))?;
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_pem.as_slice())
        .collect::<std::result::Result<_, _>>()
        .map_err(|_| anyhow::anyhow!("TLS certificate {} is not valid PEM", cert_path.display()))?;
    if certs.is_empty() {
        bail!(
            "TLS certificate {} holds no certificate",
            cert_path.display()
        );
    }
    let key_pem = std::fs::read(key_path)
        .with_context(|| format!("TLS key {} is not readable", key_path.display()))?;
    let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut key_pem.as_slice())
        .ok()
        .flatten()
        .ok_or_else(|| {
            anyhow::anyhow!("TLS key {} holds no usable private key", key_path.display())
        })?;
    let mut config =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .context("TLS protocol versions")?
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|_| anyhow::anyhow!("TLS certificate and key do not form a usable pair"))?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// Serve `app` over TLS on `listener` until `shutdown` resolves.
pub async fn serve(
    listener: TcpListener,
    tls: Arc<ServerConfig>,
    app: axum::Router,
    shutdown: impl Future<Output = ()>,
) -> Result<()> {
    let acceptor = TlsAcceptor::from(tls);
    tokio::pin!(shutdown);
    loop {
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(pair) => pair,
                Err(_) => continue,
            },
            _ = &mut shutdown => return Ok(()),
        };
        let acceptor = acceptor.clone();
        let app = app.clone().layer(axum::Extension(ClientAddr(peer.ip())));
        tokio::spawn(async move {
            // Plaintext or any failed handshake: drop the TCP connection.
            let Ok(Ok(tls_stream)) =
                tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(stream)).await
            else {
                return;
            };
            let service = TowerToHyperService::new(app);
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(tls_stream), service)
                .await;
        });
    }
}
