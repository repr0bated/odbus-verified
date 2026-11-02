//! TLS door for HTTP callers (used by `op-web`): PEM identity plus an
//! upgrade-capable TCP server. Ported from the legacy bridge `server.rs`;
//! tonic's `Identity` hides the key and its HTTP/1 transport drops upgraded
//! connections, so the TCP door configures rustls from the same PEM pair.

use std::convert::Infallible;
use std::io::BufReader;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::connect_info::ConnectInfo;
use axum::Router;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use tokio_rustls::rustls;
use tokio_rustls::TlsAcceptor;
use tower::Service;

/// Keep the selected PEM pair available to the upgrade-capable TCP server.
#[derive(Clone)]
pub struct PemIdentity {
    cert: Vec<u8>,
    key: Vec<u8>,
}

impl std::fmt::Debug for PemIdentity {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PemIdentity([redacted])")
    }
}

impl PemIdentity {
    pub fn from_pem(cert: impl AsRef<[u8]>, key: impl AsRef<[u8]>) -> Self {
        Self {
            cert: cert.as_ref().to_vec(),
            key: key.as_ref().to_vec(),
        }
    }

    pub fn server_config(&self) -> anyhow::Result<Arc<rustls::ServerConfig>> {
        let certs = rustls_pemfile::certs(&mut BufReader::new(self.cert.as_slice()))
            .collect::<Result<Vec<_>, _>>()?;
        anyhow::ensure!(!certs.is_empty(), "TCP TLS certificate chain is empty");
        let key = rustls_pemfile::private_key(&mut BufReader::new(self.key.as_slice()))?
            .ok_or_else(|| anyhow::anyhow!("TCP TLS private key is missing"))?;
        let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }
}

/// Serve `app` over TLS on `listener`, allowing HTTP/1 upgrades
/// (websockets). Runs until the listener errors.
pub async fn serve_upgradeable_tcp(
    listener: tokio::net::TcpListener,
    tls: Arc<rustls::ServerConfig>,
    app: Router,
) -> anyhow::Result<()> {
    const MAX_CONNECTIONS: usize = 512;
    let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    let acceptor = TlsAcceptor::from(tls);
    loop {
        let permit = permits.clone().acquire_owned().await?;
        let (stream, peer) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::warn!(%error, "TCP door accept failed");
                tokio::time::sleep(Duration::from_secs(1)).await;
                continue;
            }
        };
        let acceptor = acceptor.clone();
        let app = app.clone();
        tokio::spawn(async move {
            let permit = Arc::new(permit);
            let tls =
                match tokio::time::timeout(Duration::from_secs(5), acceptor.accept(stream)).await {
                    Ok(Ok(tls)) => tls,
                    _ => return,
                };
            let service = hyper::service::service_fn(move |request: hyper::Request<Incoming>| {
                let mut app = app.clone();
                let permit = permit.clone();
                async move {
                    let mut request = request.map(Body::new);
                    request.extensions_mut().insert(ConnectInfo(peer));
                    request.extensions_mut().insert(permit);
                    let response = app.call(request).await?;
                    Ok::<_, Infallible>(response)
                }
            });
            let mut builder = Builder::new(TokioExecutor::new());
            builder
                .http1()
                .timer(TokioTimer::new())
                .header_read_timeout(Duration::from_secs(10));
            if let Err(error) = builder
                .serve_connection_with_upgrades(TokioIo::new(tls), service)
                .await
            {
                tracing::debug!(%error, "TCP door connection closed");
            }
        });
    }
}
