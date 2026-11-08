//! Private EMQX ExHook and content-free notifications of committed bridge state.
//!
//! Binding is independent of broker startup: EMQX itself waits for bridge
//! readiness. The owner must supervise both `serve` and `WakePublisher::run` and
//! withdraw readiness when either required task exits. MQTT never carries state.

use std::future::Future;
use std::io::{BufReader, Read};
use std::net::{Ipv4Addr, SocketAddr};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use futures::StreamExt;
use tokio::net::TcpListener;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
use tonic::{Request, Status};

#[path = "runtime_mqtt_auth.rs"]
mod auth;
#[path = "runtime_mqtt_hooks.rs"]
mod hooks;
#[path = "runtime_mqtt_wake.rs"]
mod wake;

pub use wake::WakePublisher;
pub(crate) use auth::owner_key_for_claims;

mod protocol {
    tonic::include_proto!("emqx.exhook.v3");
}

const EXHOOK_ADDR: SocketAddr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 9000);
const MAX_PEM_BYTES: u64 = 1024 * 1024;
const SHUTDOWN_LIMIT: Duration = Duration::from_secs(30);

/// A successfully configured and bound listener, not a claim about broker health.
/// Dropping this before `serve` releases the listener immediately.
pub struct ExHookServer {
    listener: TcpListener,
    addr: SocketAddr,
    server: Server,
    peer: PeerPin,
    sessions: Arc<auth::Sessions>,
}

impl ExHookServer {
    /// Load the same protected door PEM pair, authenticate broker clients against
    /// the configured CA AND exact leaf certificate, then bind the private port.
    /// No connection to EMQX and no credential minting occurs here.
    pub async fn bind_from_env(
        runtime: Arc<crate::runtime::Runtime>,
        oracle: Arc<crate::oracle_assertion::AssertionValidator>,
    ) -> Result<Self> {
        if let Ok(enabled) = std::env::var("OP_EMQX_EXHOOK_ENABLE") {
            ensure!(enabled == "1", "the production ExHook listener cannot be disabled");
        }
        let addr = std::env::var("OP_EMQX_EXHOOK_BIND_ADDR")
            .unwrap_or_else(|_| EXHOOK_ADDR.to_string())
            .parse::<SocketAddr>()
            .context("invalid private ExHook bind address")?;
        ensure!(addr == EXHOOK_ADDR, "ExHook must bind exactly {EXHOOK_ADDR}");
        let (tls, peer) = tokio::task::spawn_blocking(load_tls).await??;
        // Other bridge consumers enable multiple rustls providers. Tonic 0.12
        // uses rustls's process default; select it explicitly before its builder.
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        }
        let server = Server::builder()
            .tls_config(tls)?
            .concurrency_limit_per_connection(64)
            .max_concurrent_streams(64)
            .timeout(Duration::from_secs(15));
        let sessions = Arc::new(auth::Sessions::new(runtime, oracle));
        let listener = TcpListener::bind(addr).await.context("binding private ExHook listener")?;
        let addr = listener.local_addr()?;
        Ok(Self { listener, addr, server, peer, sessions })
    }

    pub fn local_addr(&self) -> SocketAddr { self.addr }

    pub fn wake_publisher(&self) -> WakePublisher {
        WakePublisher::new(self.sessions.clone())
    }

    /// Run the bound native gRPC service. Accept failures and unexpected server
    /// termination are errors; a requested shutdown closes admission then drains
    /// existing RPCs for at most thirty seconds.
    pub async fn serve<F>(self, shutdown: F) -> Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let Self { listener, mut server, peer, sessions, .. } = self;
        let service = protocol::hook_provider_server::HookProviderServer::new(
            hooks::HookProviderService::new(sessions.clone()),
        )
        .max_decoding_message_size(64 * 1024)
        .max_encoding_message_size(64 * 1024);
        let service = tonic::service::interceptor::InterceptedService::new(service, peer);
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
        let incoming = async_stream::try_stream! {
            loop {
                let (socket, _) = listener.accept().await?;
                yield socket;
            }
        };
        // Give the incoming error type explicitly: try_stream's `?` otherwise
        // has several possible conversions in tonic's generic incoming API.
        let incoming = incoming.map(|item: std::io::Result<tokio::net::TcpStream>| item);
        let serving = server.add_service(service).serve_with_incoming_shutdown(incoming, async {
            let _ = stop_rx.await;
        });
        tokio::pin!(serving);
        tokio::pin!(shutdown);
        tokio::select! {
            result = &mut serving => {
                sessions.clear().await;
                result.context("private ExHook service failed")?;
                bail!("private ExHook listener stopped without a shutdown request");
            }
            () = &mut shutdown => {}
        }
        let _ = stop_tx.send(());
        sessions.clear().await;
        tokio::time::timeout(SHUTDOWN_LIMIT, &mut serving)
            .await
            .context("private ExHook shutdown exceeded thirty seconds")?
            .context("private ExHook shutdown failed")
    }
}

#[derive(Clone)]
struct PeerPin(Arc<[u8]>);

impl tonic::service::Interceptor for PeerPin {
    fn call(&mut self, request: Request<()>) -> std::result::Result<Request<()>, Status> {
        let certificates = request.peer_certs()
            .ok_or_else(|| Status::unauthenticated("broker client certificate required"))?;
        let leaf = certificates.first()
            .ok_or_else(|| Status::unauthenticated("broker client certificate required"))?;
        if leaf.as_ref() != self.0.as_ref() {
            return Err(Status::unauthenticated("broker client certificate does not match its pin"));
        }
        if !request.remote_addr().is_some_and(|peer| peer.ip().is_loopback()) {
            return Err(Status::permission_denied("ExHook is a private loopback service"));
        }
        Ok(request)
    }
}

fn configured_path(variable: &str, default: &str) -> PathBuf {
    std::env::var_os(variable).map(PathBuf::from).unwrap_or_else(|| default.into())
}

fn load_tls() -> Result<(ServerTlsConfig, PeerPin)> {
    let cert = protected_file(&configured_path("OP_EMQX_EXHOOK_SERVER_CERT_FILE",
        "/etc/op-dbus/tls/tonic-svc0.crt"), false, MAX_PEM_BYTES)?;
    let key = zeroize::Zeroizing::new(protected_file(&configured_path(
        "OP_EMQX_EXHOOK_SERVER_KEY_FILE", "/etc/op-dbus/tls/tonic-svc0.key"), true, MAX_PEM_BYTES)?);
    let ca = protected_file(&configured_path("OP_EMQX_EXHOOK_CLIENT_CA_FILE",
        "/etc/emqx/certs/opdbus-ca.crt"), false, MAX_PEM_BYTES)?;
    let pin = protected_file(&configured_path("OP_EMQX_EXHOOK_CLIENT_CERT_FILE",
        "/etc/emqx/certs/emqx-exhook-client.crt"), false, MAX_PEM_BYTES)?;
    let certificates = rustls_pemfile::certs(&mut BufReader::new(pin.as_slice()))
        .collect::<std::io::Result<Vec<_>>>()?;
    ensure!(certificates.len() == 1, "ExHook client pin must contain exactly one certificate");
    let peer = PeerPin(Arc::from(certificates[0].as_ref()));
    let tls = ServerTlsConfig::new()
        .identity(Identity::from_pem(cert, key.as_slice()))
        .client_ca_root(Certificate::from_pem(ca))
        .client_auth_optional(false);
    Ok((tls, peer))
}

/// Authority/configuration files are opened without following the final symlink
/// and checked on the opened descriptor, never by an earlier pathname stat.
fn protected_file(path: &Path, private: bool, maximum: u64) -> Result<Vec<u8>> {
    let file = std::fs::OpenOptions::new().read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path).with_context(|| format!("opening protected file {}", path.display()))?;
    let metadata = file.metadata()?;
    let forbidden = if private { 0o077 } else { 0o022 };
    ensure!(metadata.is_file() && metadata.uid() == 0 && metadata.mode() & forbidden == 0,
        "file must be a protected root-owned regular file: {}", path.display());
    ensure!(metadata.len() <= maximum, "protected file exceeds its size bound");
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(maximum + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= maximum, "protected file exceeds its size bound");
    Ok(bytes)
}
