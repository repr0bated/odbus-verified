//! TLS door for HTTP callers (used by `op-web`): PEM identity plus an
//! upgrade-capable TCP server. Ported from the legacy bridge `server.rs`;
//! tonic's `Identity` hides the key and its HTTP/1 transport drops upgraded
//! connections, so the TCP door configures rustls from the same PEM pair.

use std::convert::Infallible;
use std::fs::OpenOptions;
use std::io::{BufReader, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::connect_info::ConnectInfo;
use axum::Router;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio_rustls::rustls;
use tokio_rustls::TlsAcceptor;
use tonic::transport::server::TcpConnectInfo;
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

    /// Load the deployed identity without following a symlink or accepting
    /// an identity that another account can replace through file writes.
    pub fn from_root_owned_files(
        cert: impl AsRef<Path>,
        key: impl AsRef<Path>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            cert: read_identity_file(cert.as_ref(), false)?,
            key: read_identity_file(key.as_ref(), true)?,
        })
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

fn read_identity_file(path: &Path, private: bool) -> anyhow::Result<Vec<u8>> {
    use anyhow::Context;

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .with_context(|| format!("opening TLS identity file {}", path.display()))?;
    let metadata = file.metadata()?;
    let forbidden_mode = if private { 0o077 } else { 0o022 };
    anyhow::ensure!(
        metadata.is_file() && metadata.uid() == 0 && metadata.mode() & forbidden_mode == 0,
        "TLS identity file must be a protected root-owned regular file: {}",
        path.display(),
    );
    const MAX_PEM_BYTES: u64 = 1024 * 1024;
    anyhow::ensure!(metadata.len() <= MAX_PEM_BYTES, "TLS identity file exceeds its size bound");
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_PEM_BYTES + 1).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() as u64 <= MAX_PEM_BYTES, "TLS identity file exceeds its size bound");
    Ok(bytes)
}

/// Passed to upgraded connections, which outlive Hyper's HTTP connection.
#[derive(Clone)]
pub(crate) struct ConnectionShutdown(watch::Receiver<bool>);

impl ConnectionShutdown {
    pub(crate) async fn cancelled(&mut self) {
        let already_stopping = *self.0.borrow_and_update();
        if !already_stopping {
            let _ = self.0.changed().await;
        }
    }
}

/// Serve `app` over TLS, retaining the existing indefinitely-running API.
pub async fn serve_upgradeable_tcp(
    listener: tokio::net::TcpListener,
    tls: Arc<rustls::ServerConfig>,
    app: Router,
) -> anyhow::Result<()> {
    serve_upgradeable_tcp_with_shutdown(listener, tls, app, std::future::pending()).await
}

/// Bound HTTP/1 upgrades and native HTTP/2 connections together, including
/// handshakes. Preserve streaming bodies, original metadata and kernel peer
/// addresses; stop accepting before draining on termination.
pub async fn serve_upgradeable_tcp_with_shutdown<F>(
    listener: tokio::net::TcpListener,
    tls: Arc<rustls::ServerConfig>,
    app: Router,
    shutdown: F,
) -> anyhow::Result<()>
where
    F: Future<Output = ()>,
{
    const MAX_CONNECTIONS: usize = 512;
    const DRAIN_TIMEOUT: Duration = Duration::from_secs(30);
    let local_addr = listener.local_addr()?;
    let permits = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    let acceptor = TlsAcceptor::from(tls);
    let (stop, stopped) = watch::channel(false);
    let mut connections = JoinSet::new();
    tokio::pin!(shutdown);

    let result = loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => break Ok(()),
            finished = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(error)) = finished {
                    break Err(anyhow::anyhow!("TLS connection task failed: {error}"));
                }
            }
            accepted = async {
                let permit = permits.clone().acquire_owned().await?;
                let (stream, peer) = listener.accept().await?;
                Ok::<_, anyhow::Error>((stream, peer, permit))
            } => {
                let (stream, peer, permit) = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => break Err(error),
                };
                let acceptor = acceptor.clone();
                let app = app.clone();
                let mut shutdown = ConnectionShutdown(stopped.clone());
                connections.spawn(async move {
                    let permit = Arc::new(permit);
                    let tls = tokio::select! {
                        biased;
                        _ = shutdown.cancelled() => return,
                        handshake = tokio::time::timeout(
                            Duration::from_secs(5), acceptor.accept(stream),
                        ) => match handshake {
                            Ok(Ok(tls)) => tls,
                            _ => return,
                        },
                    };
                    let request_shutdown = shutdown.clone();
                    let service = hyper::service::service_fn(move |request: hyper::Request<Incoming>| {
                        let mut app = app.clone();
                        let permit = permit.clone();
                        let shutdown = request_shutdown.clone();
                        async move {
                            let mut request = request.map(Body::new);
                            request.extensions_mut().insert(ConnectInfo(peer));
                            // tonic::Request::remote_addr() does not inspect Axum ConnectInfo.
                            request.extensions_mut().insert(TcpConnectInfo {
                                local_addr: Some(local_addr),
                                remote_addr: Some(peer),
                            });
                            request.extensions_mut().insert(permit);
                            request.extensions_mut().insert(shutdown);
                            let response = app.call(request).await?;
                            Ok::<_, Infallible>(response)
                        }
                    });
                    let mut builder = Builder::new(TokioExecutor::new());
                    builder
                        .http1()
                        .timer(TokioTimer::new())
                        .header_read_timeout(Duration::from_secs(10));
                    builder.http2().max_concurrent_streams(128);
                    let connection = builder.serve_connection_with_upgrades(TokioIo::new(tls), service);
                    tokio::pin!(connection);
                    let result = tokio::select! {
                        result = &mut connection => result,
                        _ = shutdown.cancelled() => {
                            connection.as_mut().graceful_shutdown();
                            connection.await
                        },
                    };
                    if let Err(error) = result {
                        tracing::debug!(%error, "TCP door connection closed");
                    }
                });
            }
        }
    };

    drop(listener);
    stop.send_replace(true);
    let drained = tokio::time::timeout(DRAIN_TIMEOUT, async {
        while let Some(finished) = connections.join_next().await {
            if let Err(error) = finished {
                tracing::warn!(%error, "TLS connection task failed during shutdown");
            }
        }
        // Upgraded MQTT sockets retain their permit until their close completes.
        let _all = permits.acquire_many(MAX_CONNECTIONS as u32).await?;
        Ok::<_, anyhow::Error>(())
    }).await;
    if drained.is_err() {
        connections.abort_all();
        while connections.join_next().await.is_some() {}
        anyhow::bail!("TLS door connections exceeded the graceful shutdown deadline");
    }
    drained??;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tls_preserves_tonic_peer_and_identity_metadata_and_drains_on_shutdown() {
        let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let identity = PemIdentity::from_pem(
            certificate.cert.pem(),
            certificate.key_pair.serialize_pem(),
        );
        let app = Router::new().route(
            "/peer",
            axum::routing::get(|request: axum::extract::Request| async move {
                let request = tonic::Request::from_http(request);
                let assertion = request
                    .metadata()
                    .get_bin("x-oracle-identity-assertion-bin")
                    .unwrap()
                    .to_bytes()
                    .unwrap();
                assert_eq!(assertion.as_ref(), b"OIA1");
                request.remote_addr().unwrap().to_string()
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let tls = identity.server_config().unwrap();
        let server = tokio::spawn(serve_upgradeable_tcp_with_shutdown(
            listener, tls, app, async { let _ = stopped.await; },
        ));
        let mut roots = rustls::RootCertStore::empty();
        roots.add(certificate.cert.der().clone()).unwrap();
        let client_tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
        let actual_peer = tcp.local_addr().unwrap();
        let tls = tokio_rustls::TlsConnector::from(Arc::new(client_tls))
            .connect(rustls::pki_types::ServerName::try_from("localhost").unwrap(), tcp)
            .await
            .unwrap();
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(TokioIo::new(tls)).await.unwrap();
        let client = tokio::spawn(connection);
        let response = sender.send_request(
            hyper::Request::builder()
                .uri("/peer")
                .header("host", "localhost")
                .header("x-oracle-identity-assertion-bin", "T0lBMQ==")
                .header("x-forwarded-for", "203.0.113.1")
                .body(tonic::body::empty_body())
                .unwrap(),
        ).await.unwrap();
        let body = axum::body::to_bytes(Body::new(response.into_body()), 128).await.unwrap();
        assert_eq!(body.as_ref(), actual_peer.to_string().as_bytes());
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), server).await.unwrap().unwrap().unwrap();
        client.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn shutdown_does_not_wait_for_unfinished_tls_handshake() {
        let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let identity = PemIdentity::from_pem(
            certificate.cert.pem(), certificate.key_pair.serialize_pem(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(serve_upgradeable_tcp_with_shutdown(
            listener, identity.server_config().unwrap(), Router::new(),
            async { let _ = stopped.await; },
        ));
        let _unfinished = tokio::net::TcpStream::connect(addr).await.unwrap();
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), server).await.unwrap().unwrap().unwrap();
    }
}
