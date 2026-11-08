//! Identity-bound local D-Bus transport for the new canonical runtime.
//!
//! Current 3tchedFS clients bind at LungsV1, then invoke
//! PluginV1.CallWithOperationId(method, JSON arguments, UUID). The runtime owns
//! admission, execution, state publication and durable operation receipts.
//! Socket negotiation is anonymous; authority comes only from kernel peer
//! credentials, the committed identity term and the active principal registry.

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use op_state_store::method_receipt::{MAX_METHOD_ARGUMENT_BYTES, MAX_METHOD_RECEIPT_BYTES, MethodExecutionReceipt};
use tokio::sync::{Semaphore, oneshot};
use tokio::task::{JoinHandle, JoinSet};

use crate::runtime::{Runtime, RuntimeError};

#[path = "runtime_lungs_identity.rs"]
mod identity;
use identity::{LungsIdentity, ProcessIdentity};

pub(crate) const DEFAULT_LUNGS_SOCKET: &str = "/run/opdbus/lungs/lungs.sock";
const LUNGS_OBJECT_PATH: &str = "/org/opdbus/v1/lungs";
const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONNECTIONS: usize = 64;
const MAX_IN_FLIGHT_CALLS: usize = 64;

struct LungsBootstrap {
    identity: Arc<LungsIdentity>,
}

#[zbus::interface(name = "org.opdbus.v1.LungsV1")]
impl LungsBootstrap {
    async fn bind(&self, session_id: &str) -> zbus::fdo::Result<bool> {
        self.identity.bind(session_id).await.map_err(binding_denial)?;
        Ok(true)
    }

    async fn bind_current_container(&self) -> zbus::fdo::Result<bool> {
        self.identity.bind_current_container().await.map_err(binding_denial)?;
        Ok(true)
    }
}

fn binding_denial(error: anyhow::Error) -> zbus::fdo::Error {
    tracing::warn!(%error, "native lungs identity binding refused");
    zbus::fdo::Error::AccessDenied("lungs identity binding refused".into())
}

struct LungsPlugin {
    plugin_id: String,
    runtime: Arc<Runtime>,
    identity: Arc<LungsIdentity>,
    calls: Arc<Semaphore>,
}

#[zbus::interface(name = "org.opdbus.v1.PluginV1")]
impl LungsPlugin {
    async fn call_with_operation_id(
        &self,
        method: &str,
        arguments: &str,
        operation_id: &str,
    ) -> zbus::fdo::Result<String> {
        let operation = canonical_operation_id(operation_id)?;
        if arguments.len() > MAX_METHOD_ARGUMENT_BYTES {
            return Err(zbus::fdo::Error::InvalidArgs("method arguments exceed byte bound".into()));
        }
        let arguments: serde_json::Value = serde_json::from_str(arguments)
            .map_err(|_| zbus::fdo::Error::InvalidArgs("method arguments are not JSON".into()))?;
        let caller = self.identity.caller().await.map_err(binding_denial)?;
        let permit = self.calls.clone().try_acquire_owned()
            .map_err(|_| zbus::fdo::Error::LimitsExceeded("lungs runtime is busy or stopping".into()))?;
        let runtime = self.runtime.clone();
        let plugin_id = self.plugin_id.clone();
        let method_owned = method.to_owned();
        // Dropping a D-Bus request or its connection must not drop an admitted
        // backend future. The runtime's durable pending record survives process
        // shutdown; no timeout/cancellation is certified as before-effect.
        let execution = tokio::spawn(async move {
            let _permit = permit;
            runtime.call(caller, &plugin_id, &method_owned, arguments, operation).await
        });
        let completed = execution.await
            .map_err(|_| unknown_outcome(operation, "runtime worker ended"))?
            .map_err(|error| runtime_error(error, operation))?;
        completed.receipt.validate_attribution(&self.plugin_id, method)
            .map_err(|_| unknown_outcome(operation, "runtime receipt is invalid"))?;
        if completed.receipt.operation_id != operation_id {
            return Err(unknown_outcome(operation, "runtime receipt operation mismatch"));
        }
        // Includes the original native peer proof, an exact committed refresh
        // transition when applicable, and the current sealed-method grant.
        self.runtime.revalidate_result(&completed, &self.plugin_id, method).await
            .map_err(|error| runtime_error(error, operation))?;
        receipt_wire(&completed.receipt)
            .map_err(|_| unknown_outcome(operation, "runtime receipt cannot be encoded within its byte bound"))
    }
}

fn receipt_wire(receipt: &MethodExecutionReceipt) -> Result<String> {
    struct BoundedJson(Vec<u8>);
    impl std::io::Write for BoundedJson {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_METHOD_RECEIPT_BYTES.saturating_sub(self.0.len()) {
                return Err(std::io::Error::other("method receipt byte bound"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    let mut encoded = BoundedJson(Vec::new());
    serde_json::to_writer(&mut encoded, receipt)?;
    Ok(String::from_utf8(encoded.0)?)
}

fn canonical_operation_id(text: &str) -> zbus::fdo::Result<uuid::Uuid> {
    let id = uuid::Uuid::parse_str(text)
        .map_err(|_| zbus::fdo::Error::InvalidArgs("operation ID is not a UUID".into()))?;
    if id.to_string() != text {
        return Err(zbus::fdo::Error::InvalidArgs("operation ID is not canonical".into()));
    }
    Ok(id)
}

fn unknown_outcome(operation: uuid::Uuid, reason: &'static str) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(format!(
        "outcome_unknown; operation_id={operation}; {reason}; do not retry automatically"
    ))
}

fn runtime_error(error: RuntimeError, operation: uuid::Uuid) -> zbus::fdo::Error {
    // Backend diagnostics can contain private arguments or credentials. Only
    // the canonical classification and caller's operation locator cross here.
    if error.outcome_unknown() {
        let related = error.related_operation_ids().join(",");
        zbus::fdo::Error::Failed(format!(
            "outcome_unknown; operation_id={operation}; related_operation_ids={related}; canonical execution did not establish disclosure; do not retry automatically"
        ))
    } else if error.is_denied() {
        zbus::fdo::Error::AccessDenied(format!("method admission refused; operation_id={operation}"))
    } else if error.failed_before_effect() {
        zbus::fdo::Error::NotSupported(format!("method_failed_before_effect; operation_id={operation}"))
    } else if error.is_unsupported() {
        zbus::fdo::Error::NotSupported(format!("method not admitted; operation_id={operation}"))
    } else {
        unknown_outcome(operation, "canonical runtime failed")
    }
}

/// Holds the socket inode and exclusive owner lock; never removes another
/// server's path when a supervisor replaces this instance.
struct SocketOwner {
    path: PathBuf,
    dev: u64,
    ino: u64,
    _lock: File,
}

impl Drop for SocketOwner {
    fn drop(&mut self) {
        if let Ok(metadata) = std::fs::symlink_metadata(&self.path) {
            if metadata.file_type().is_socket() && metadata.dev() == self.dev && metadata.ino() == self.ino {
                if let Err(error) = std::fs::remove_file(&self.path) {
                    tracing::warn!(%error, socket = %self.path.display(), "lungs socket cleanup failed");
                }
            }
        }
    }
}

/// Main owns this guard for the listener's entire lifetime. Creation returns
/// only after bind, socket permissions and kernel authentication prerequisites
/// succeed. Shutdown stops admissions; already dispatched runtime calls finish
/// independently and keep their durable receipt/unknown-outcome semantics.
pub(crate) struct LungsServer {
    socket: Arc<SocketOwner>,
    calls: Arc<Semaphore>,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<()>>>,
}

impl LungsServer {
    pub(crate) fn socket_path(&self) -> &Path { &self.socket.path }

    pub(crate) fn is_finished(&self) -> bool {
        self.task.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Cancellation-safe supervision for the main startup owner's select!.
    /// Keep the handle in the guard while pending; remove it only after its
    /// result is consumed so shutdown never polls a completed handle twice.
    pub(crate) async fn terminated(&mut self) -> Result<()> {
        let outcome = self.task.as_mut()
            .context("lungs server owner has already terminated")?
            .await;
        self.task.take();
        self.calls.close();
        match outcome {
            Ok(Ok(())) => anyhow::bail!("lungs server stopped unexpectedly"),
            Ok(Err(error)) => Err(error).context("lungs server stopped unexpectedly"),
            Err(error) => Err(error).context("lungs server owner failed"),
        }
    }

    pub(crate) fn begin_shutdown(&mut self) {
        self.calls.close();
        if let Some(stop) = self.stop.take() { let _ = stop.send(()); }
    }

    pub(crate) async fn shutdown(mut self) -> Result<()> {
        self.begin_shutdown();
        if let Some(task) = self.task.as_mut() {
            let outcome = task.await;
            self.task.take();
            outcome.context("lungs server worker failed")??;
        }
        Ok(())
    }
}

impl Drop for LungsServer {
    fn drop(&mut self) {
        self.calls.close();
        if let Some(stop) = self.stop.take() { let _ = stop.send(()); }
        if let Some(task) = self.task.take() { task.abort(); }
    }
}

async fn bind_socket(path: &Path) -> Result<(tokio::net::UnixListener, SocketOwner)> {
    ensure!(path.is_absolute(), "lungs socket path must be absolute");
    let parent = path.parent().context("lungs socket has no parent directory")?;
    std::fs::create_dir_all(parent)?;
    let directory = std::fs::symlink_metadata(parent)?;
    ensure!(directory.is_dir() && !directory.file_type().is_symlink()
        && directory.uid() == 0 && directory.mode() & 0o022 == 0,
        "lungs socket directory is not protected");
    let lock_path = path.with_extension("sock.lock");
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(&lock_path)?;
    let metadata = lock.metadata()?;
    ensure!(metadata.is_file() && metadata.uid() == 0 && metadata.mode() & 0o077 == 0,
        "lungs socket owner lock is not protected");
    // SAFETY: lock owns a live descriptor; nonblocking flock cannot stall startup.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(std::io::Error::last_os_error()).context("lungs socket already has an owner");
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(metadata.file_type().is_socket() && metadata.uid() == 0,
                "lungs path exists and is not an owned socket");
            match tokio::time::timeout(Duration::from_secs(1), tokio::net::UnixStream::connect(path)).await {
                Ok(Err(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => {}
                Ok(Err(error)) => return Err(error).context("cannot establish stale lungs socket"),
                _ => anyhow::bail!("lungs socket already has a listener"),
            }
            let current = std::fs::symlink_metadata(path)?;
            ensure!(metadata.dev() == current.dev() && metadata.ino() == current.ino(),
                "lungs socket changed during startup");
            std::fs::remove_file(path)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let listener = tokio::net::UnixListener::bind(path)
        .with_context(|| format!("bind lungs socket {}", path.display()))?;
    let metadata = std::fs::symlink_metadata(path)?;
    let owner = SocketOwner { path: path.to_owned(), dev: metadata.dev(), ino: metadata.ino(), _lock: lock };
    // World access grants negotiation, not identity selection or method access.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666))?;
    Ok((listener, owner))
}

pub(crate) async fn start(runtime: Arc<Runtime>) -> Result<LungsServer> {
    ensure!(nix::unistd::geteuid().is_root(), "native lungs server requires host root");
    identity::require_peer_pidfd().context("SO_PEERPIDFD is required for lungs admission")?;
    let host = Arc::new(ProcessIdentity::current()?);
    let catalog_dir = std::env::var_os("OP_BLOB_CATALOG_DIR").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(op_blob::catalog::DEFAULT_SHM_DIR));
    let state_dir = op_core::projection_shm::shm_state_dir();
    let path = std::env::var_os("OPDBUS_LUNGS_SOCKET").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_LUNGS_SOCKET));
    let objects: Arc<Vec<(String, String)>> = Arc::new(runtime.catalog().plugins().iter().map(|plugin| {
        (format!("/org/opdbus/v1/plugins/{}", plugin.id), plugin.id.clone())
    }).collect());
    // Validate paths before taking socket ownership; no partial listener is ready.
    for (path, _) in objects.iter() { zbus::zvariant::ObjectPath::try_from(path.as_str())?; }
    let (listener, owner) = bind_socket(&path).await?;
    let socket = Arc::new(owner);
    let owned_socket = socket.clone();
    let calls = Arc::new(Semaphore::new(MAX_IN_FLIGHT_CALLS));
    let active_calls = calls.clone();
    let (stop, mut stopped) = oneshot::channel();
    let task = tokio::spawn(async move {
        let _socket = owned_socket;
        let mut connections = JoinSet::new();
        let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
        loop {
            tokio::select! {
                biased;
                _ = &mut stopped => break,
                finished = connections.join_next(), if !connections.is_empty() => {
                    if let Some(Err(error)) = finished {
                        tracing::warn!(%error, "lungs connection worker ended");
                    }
                }
                incoming = listener.accept() => {
                    let (stream, _) = incoming.context("accept lungs connection")?;
                    let Ok(permit) = slots.clone().try_acquire_owned() else { continue; };
                    let identity = match LungsIdentity::capture(&stream, host.clone(), catalog_dir.clone(), state_dir.clone()) {
                        Ok(identity) => identity,
                        Err(error) => {
                            tracing::warn!(%error, "lungs peer proof refused");
                            continue;
                        }
                    };
                    let runtime = runtime.clone();
                    let objects = objects.clone();
                    let calls = active_calls.clone();
                    connections.spawn(async move {
                        let _permit = permit;
                        if let Err(error) = serve_connection(stream, runtime, identity, objects, calls).await {
                            tracing::warn!(%error, "lungs connection ended");
                        }
                    });
                }
            }
        }
        active_calls.close();
        connections.abort_all();
        while connections.join_next().await.is_some() {}
        Ok(())
    });
    Ok(LungsServer { socket, calls, stop: Some(stop), task: Some(task) })
}

async fn serve_connection(
    stream: tokio::net::UnixStream,
    runtime: Arc<Runtime>,
    identity: Arc<LungsIdentity>,
    objects: Arc<Vec<(String, String)>>,
    calls: Arc<Semaphore>,
) -> Result<()> {
    let mut builder = zbus::connection::Builder::unix_stream(stream)
        .server(zbus::Guid::generate())?.p2p()
        .auth_mechanism(zbus::connection::AuthMechanism::Anonymous)
        .serve_at(LUNGS_OBJECT_PATH, LungsBootstrap { identity: identity.clone() })?;
    for (path, plugin_id) in objects.iter() {
        builder = builder.serve_at(path.clone(), LungsPlugin {
            plugin_id: plugin_id.clone(), runtime: runtime.clone(), identity: identity.clone(), calls: calls.clone(),
        })?;
    }
    let connection = tokio::time::timeout(NEGOTIATION_TIMEOUT, builder.build())
        .await.context("lungs D-Bus negotiation timed out")??;
    identity.wait_bound().await?;
    connection.closed().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_locators_are_canonical_without_rewriting_client_input() {
        let id = "12345678-9abc-4def-8123-0123456789ab";
        assert_eq!(canonical_operation_id(id).unwrap().to_string(), id);
        assert!(canonical_operation_id(&id.to_uppercase()).is_err());
        assert!(canonical_operation_id(&id.replace('-', "")).is_err());
        assert!(canonical_operation_id("not-an-operation").is_err());
    }

    #[test]
    fn uncertainty_keeps_correlation_and_never_promises_safe_retry() {
        let id = uuid::Uuid::new_v4();
        let error = unknown_outcome(id, "runtime worker ended");
        assert!(matches!(error, zbus::fdo::Error::Failed(_)));
        let message = error.to_string();
        assert!(message.contains(&format!("operation_id={id}")));
        assert!(message.contains("do not retry automatically"));
    }
}
