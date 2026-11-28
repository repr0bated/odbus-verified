//! Bridge-hosted FUSE mounts: the `_3tched-fs` library instantiated inside
//! the bridge, dispatching in-process through `Runtime::call`.
//!
//! The container is the identity. A mount's caller is derived from the
//! committed identity sled whose native Incus container is running under the
//! recorded UUID, exactly as the lungs socket's `BindCurrentContainer` derives
//! it. No credential is read from a file, accepted from a caller, or copied
//! into a guest; the authored SID1 is resolved bridge-side from the sled
//! registry and pinned in the executor. Every call revalidates the native
//! container binding through `CallerAuthority`, and every call is executed —
//! and therefore tracked by the shared execution tracker — through
//! `Runtime::call`. Grants are enforced there, never in this module.
//!
//! Each identity gets two separate mounts, as everywhere else: `3tchedFS`
//! (`Surface::Our`) and `threetched` (`Surface::Binary`), never merged.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU8, Ordering};
use std::collections::HashMap;

use anyhow::{Context as _, Result, ensure};
use op_plugins::state_plugins::identity_sled::ContainerIdentitySled;
use op_plugins::state_plugins::incus::{IncusInstance, IncusPlugin, IncusRootRepairBackend};
use serde_json::Value;

use _3tched_fs::dispatch::LungsError;
use _3tched_fs::fuse_fs::{BoundLocalCall, LocalExecutor, TypedFuse};
use _3tched_fs::model::MethodDecl;
use _3tched_fs::namespace::{ProjectionTarget, Surface};
use _3tched_fs::source::SourceCatalog;
use _3tched_fs::store::ObjectStore;

use crate::identity_receiver::VerifiedSid1;
use crate::runtime::{CallerAuthority, Runtime, RuntimeError, RuntimeResult, CheckpointTarget, VerifiedCaller};

const MAX_PREPARED_CALLS: usize = 1024;
const REGISTERED: u8 = 0;
const BOUND: u8 = 1;
const EXECUTING: u8 = 2;
const ABORTED: u8 = 3;

struct CallState(AtomicU8);
impl CallState {
    fn abort_before_dispatch(&self) -> bool {
        let mut phase = self.0.load(Ordering::Acquire);
        loop {
            if phase == EXECUTING { return false; }
            if phase == ABORTED { return true; }
            match self.0.compare_exchange(phase, ABORTED, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return true, Err(current) => phase = current,
            }
        }
    }
}

#[derive(Clone)]
struct MountRecord {
    key: uuid::Uuid, instance: uuid::Uuid, path: PathBuf, surface: Surface,
    binding: ContainerBinding,
}
struct PreparedCall {
    mount: uuid::Uuid, caller: VerifiedCaller, target: CheckpointTarget,
    state: Arc<CallState>,
    completed: tokio::sync::oneshot::Sender<Result<RuntimeResult, RuntimeError>>,
}
pub(crate) struct FuseCalls {
    mounts: Mutex<Vec<MountRecord>>,
    pending: Mutex<HashMap<uuid::Uuid, PreparedCall>>,
}
impl FuseCalls {
    pub(crate) fn new() -> Self { Self { mounts: Mutex::new(Vec::new()), pending: Mutex::new(HashMap::new()) } }
    fn register(&self, record: MountRecord) -> Result<()> {
        let mut mounts = self.mounts.lock().map_err(|_| anyhow::anyhow!("mounted owner poisoned"))?;
        ensure!(mounts.len() < 128 && !mounts.iter().any(|old| old.path == record.path || old.key == record.key
            || (old.binding == record.binding && old.surface == record.surface)), "duplicate mounted native binding");
        mounts.try_reserve(1)?;
        mounts.push(record);
        Ok(())
    }
    fn unregister(&self, key: uuid::Uuid) {
        if let Ok(mut mounts) = self.mounts.lock() { mounts.retain(|mount| mount.key != key); }
    }
    fn select(&self, binding: &ContainerBinding) -> Result<MountRecord> {
        let mounts = self.mounts.lock().map_err(|_| anyhow::anyhow!("mounted owner poisoned"))?;
        let mut pair = mounts.iter().filter(|mount| &mount.binding == binding);
        let first = pair.next().context("verified container has no registered mounted pair")?;
        let second = pair.next().context("verified container mounted pair is incomplete")?;
        ensure!(pair.next().is_none() && first.surface != second.surface
            && first.instance != second.instance && first.path != second.path, "ambiguous native mounted pair");
        [first, second].into_iter().find(|mount| mount.surface == Surface::Our).cloned()
            .context("verified container has no Our operation namespace")
    }
}

struct Registration { calls: Arc<FuseCalls>, token: uuid::Uuid }
impl Drop for Registration {
    fn drop(&mut self) { if let Ok(mut pending) = self.calls.pending.lock() { pending.remove(&self.token); } }
}
pub(crate) struct FuseInvocationFailure {
    pub error: RuntimeError,
    pub before_dispatch: bool,
}
fn invocation_failure(operation: uuid::Uuid, state: &CallState) -> FuseInvocationFailure {
    let before_dispatch = state.abort_before_dispatch();
    let error = if before_dispatch { RuntimeError::denied("mounted operation did not enter its private callback") }
        else { RuntimeError::unknown(anyhow::anyhow!("mounted descriptor outcome unresolved; preserve original operation, never retry")) };
    FuseInvocationFailure { error: error.at_operation(operation), before_dispatch }
}

/// One identity's pair of bridge-hosted mounts, from bridge configuration.
/// `container` names the native Incus instance; identity is derived from it,
/// never supplied by a caller.
#[derive(Debug, Clone)]
pub(crate) struct FuseMountSpec {
    pub container: String,
    pub mount_3tched: PathBuf,
    pub mount_threetched: PathBuf,
    pub call_owner_uid: u32,
    pub store_root: PathBuf,
}

/// Parse `OP_FUSE_LOCAL_MOUNTS`: `;`-separated entries of
/// `container:3tched_mountpoint:threetched_mountpoint:call_owner_uid[:store_root]`.
/// `off`/`none`/`disabled` (case-insensitive) disables bridge-hosted mounts
/// entirely, including auto-enumerated ones. Unset/empty means no explicit
/// specs; auto-enumeration from committed sleds still applies.
pub(crate) fn specs_from_env() -> Result<Vec<FuseMountSpec>> {
    let Some(raw) = std::env::var_os("OP_FUSE_LOCAL_MOUNTS") else {
        return Ok(Vec::new());
    };
    let raw = raw.to_string_lossy();
    if matches!(raw.trim().to_ascii_lowercase().as_str(), "off" | "none" | "disabled") {
        return Ok(Vec::new());
    }
    let mut specs = Vec::new();
    for entry in raw.split(';').filter(|entry| !entry.trim().is_empty()) {
        let fields: Vec<&str> = entry.trim().split(':').collect();
        ensure!(
            fields.len() == 4 || fields.len() == 5,
            "OP_FUSE_LOCAL_MOUNTS entry must be container:3tched_mount:threetched_mount:uid[:store_root]"
        );
        specs.push(FuseMountSpec {
            container: fields[0].to_owned(),
            mount_3tched: PathBuf::from(fields[1]),
            mount_threetched: PathBuf::from(fields[2]),
            call_owner_uid: fields[3].parse().context("OP_FUSE_LOCAL_MOUNTS uid")?,
            store_root: fields
                .get(4)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/dev/shm/3tchedfs-bridge")),
        });
    }
    Ok(specs)
}

fn local_mounts_disabled() -> bool {
    std::env::var("OP_FUSE_LOCAL_MOUNTS")
        .is_ok_and(|raw| matches!(raw.trim().to_ascii_lowercase().as_str(), "off" | "none" | "disabled"))
}

/// Auto-enumerate one spec per committed identity: active+anchored sled with a
/// native instance. Running state is not assumed here; `resolve_container` /
/// `container_binding` re-verify the native container is Running before any
/// mount is served. Mountpoints are by convention under
/// `/run/mounts/by-container/<native-name>/`, never the standalone-owned
/// `/run/mounts/3tchedFS` or `/run/mounts/threetchedFS`.
async fn auto_specs(runtime: &Runtime) -> Vec<FuseMountSpec> {
    let catalog_dir = runtime.catalog_dir().to_path_buf();
    let state_dir = op_core::projection_shm::shm_state_dir();
    let sessions = match tokio::task::spawn_blocking(move || {
        crate::identity_receiver::committed_sessions_in(&catalog_dir, &state_dir)
    }).await {
        Ok(Ok(sessions)) => sessions,
        Ok(Err(error)) => { tracing::warn!(%error, "FUSE auto-mount: committed sled enumeration failed"); return Vec::new(); }
        Err(error) => { tracing::warn!(%error, "FUSE auto-mount: committed sled enumeration panicked"); return Vec::new(); }
    };
    let mut names: Vec<String> = Vec::new();
    for sled in sessions.iter().filter(|sled| sled.active && sled.is_anchored()) {
        if let Some(instance) = &sled.instance {
            if !names.contains(&instance.name) { names.push(instance.name.clone()); }
        }
    }
    names.sort();
    // SAFETY: getuid always succeeds.
    let uid = unsafe { libc::getuid() };
    let mut specs = Vec::new();
    // The bridge runs with umask 077, so create_dir_all would make /run/mounts
    // 0700 when it wins the boot race; the standalone 3tched-fs/threetched-fs
    // helper requires root:root 0755 there and refuses to mount otherwise.
    if let Err(error) = ensure_shared_mounts_root(Path::new("/run/mounts")) {
        tracing::warn!(%error, "FUSE auto-mount: cannot prepare /run/mounts");
    }
    for name in names {
        let root = PathBuf::from("/run/mounts/by-container").join(&name);
        let mount_3tched = root.join("3tchedFS");
        let mount_threetched = root.join("threetched");
        if let Err(error) = std::fs::create_dir_all(&mount_3tched).and_then(|_| std::fs::create_dir_all(&mount_threetched)) {
            tracing::warn!(%error, container = %name, "FUSE auto-mount: cannot create mountpoints; skipping identity");
            continue;
        }
        specs.push(FuseMountSpec { container: name, mount_3tched, mount_threetched, call_owner_uid: uid, store_root: PathBuf::from("/dev/shm/3tchedfs-bridge") });
    }
    specs
}

/// Create the shared mount root as 0755 (never via the process umask) and
/// repair a 0700 root left by an earlier bridge start. Symlinks are refused.
fn ensure_shared_mounts_root(root: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    match std::fs::DirBuilder::new().mode(0o755).create(root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = std::fs::symlink_metadata(root)?;
    if !metadata.is_dir() {
        return Err(std::io::Error::other("mount root is not a directory"));
    }
    if metadata.permissions().mode() & 0o7777 != 0o755 {
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// Keep namespace handles open, like the lungs ProcessIdentity binding, so
/// namespace inode recycling cannot turn a stale process into authority.
#[derive(Debug, Clone)]
struct NamespaceIdentity { device: u64, inode: u64, _handle: Arc<std::fs::File> }
impl PartialEq for NamespaceIdentity {
    fn eq(&self, other: &Self) -> bool { self.device == other.device && self.inode == other.inode }
}
impl Eq for NamespaceIdentity {}
impl NamespaceIdentity {
    fn read(pid: u32, kind: &str) -> Result<Self> {
        use std::os::unix::fs::MetadataExt;
        ensure!(pid > 0, "native process unavailable");
        let handle = std::fs::File::open(format!("/proc/{pid}/ns/{kind}"))?;
        let metadata = handle.metadata()?;
        Ok(Self { device: metadata.dev(), inode: metadata.ino(), _handle: Arc::new(handle) })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContainerBinding {
    session_id: String,
    session_genesis: String,
    native_name: String,
    native_uuid: String,
    init_pid: u32,
    init_start_ticks: u64,
    init_user_namespace: NamespaceIdentity,
    init_pid_namespace: NamespaceIdentity,
    host_user_namespace: NamespaceIdentity,
    host_pid_namespace: NamespaceIdentity,
}

fn read_start_ticks(pid: u32) -> Result<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    // Names may contain spaces and ')'; the suffix begins at field 3.
    stat.rsplit_once(')')
        .and_then(|(_, suffix)| suffix.split_whitespace().nth(19))
        .and_then(|field| field.parse().ok())
        .context("invalid container init process identity")
}

fn instance_uuid(instance: &IncusInstance) -> Result<String> {
    let value = instance
        .config
        .as_ref()
        .and_then(|config| config.get("volatile.uuid"))
        .context("native instance UUID unavailable")?;
    ensure!(
        uuid::Uuid::parse_str(value).is_ok_and(|parsed| parsed.to_string() == *value),
        "native instance UUID is invalid"
    );
    Ok(value.clone())
}

/// Derive the pinned binding for one sled from its native container, the same
/// checks `LungsIdentity::container_binding` performs (minus the socket peer,
/// which a hosted mount does not have).
async fn container_binding(sled: &ContainerIdentitySled) -> Result<ContainerBinding> {
    ensure!(sled.active && sled.is_anchored(), "identity is not active");
    ensure!(
        sled.expires_at
            .is_none_or(|expiry| expiry == 0 || expiry > chrono::Utc::now().timestamp()),
        "identity has expired"
    );
    let stored = sled.instance.as_ref().context("identity container unavailable")?;
    ensure!(
        crate::identity_binding::identity_instance_matches(sled, stored),
        "identity container does not match"
    );
    let native_uuid = instance_uuid(stored)?;
    let native = IncusRootRepairBackend::local();
    let (metadata, _) = native.instance(&stored.name).await?;
    let current = IncusPlugin::instance_from_native_metadata(metadata)?;
    ensure!(
        current.name == stored.name
            && current.instance_type == "container"
            && current.status == "Running"
            && current.project.as_deref() == Some("default")
            && current.location.as_deref() == Some("none")
            && instance_uuid(&current)? == native_uuid
            && crate::identity_binding::identity_instance_matches(sled, &current),
        "native identity container does not match"
    );
    let init_pid = native.instance_init_pid(&stored.name).await?;
    let init_start_ticks = read_start_ticks(init_pid)?;
    let init_user_namespace = NamespaceIdentity::read(init_pid, "user")?;
    let init_pid_namespace = NamespaceIdentity::read(init_pid, "pid")?;
    let host_user_namespace = NamespaceIdentity::read(std::process::id(), "user")?;
    let host_pid_namespace = NamespaceIdentity::read(std::process::id(), "pid")?;
    ensure!(init_pid_namespace != host_pid_namespace, "host process has no container binding");
    ensure!(
        native.instance_init_pid(&stored.name).await? == init_pid
            && read_start_ticks(init_pid)? == init_start_ticks
            && NamespaceIdentity::read(init_pid, "user")? == init_user_namespace
            && NamespaceIdentity::read(init_pid, "pid")? == init_pid_namespace,
        "identity container restarted"
    );
    Ok(ContainerBinding {
        session_id: sled.session_id.clone(),
        session_genesis: sled
            .genesis
            .clone()
            .context("identity anchor unavailable")?,
        native_name: stored.name.clone(),
        native_uuid,
        init_pid,
        init_start_ticks,
        init_user_namespace, init_pid_namespace, host_user_namespace, host_pid_namespace,
    })
}

async fn committed_sled(catalog_dir: &Path, state_dir: &str, session_id: &str) -> Result<ContainerIdentitySled> {
    let catalog_dir = catalog_dir.to_path_buf();
    let state_dir = state_dir.to_owned();
    let session_id = session_id.to_owned();
    tokio::task::spawn_blocking(move || {
        crate::identity_receiver::committed_sessions_in(&catalog_dir, &state_dir)?
            .into_iter()
            .find(|sled| sled.session_id == session_id)
            .context("identity container disappeared")
    })
    .await?
}

async fn authored_identity(catalog_dir: &Path, state_dir: &str, session_id: &str) -> Result<VerifiedSid1> {
    let catalog_dir = catalog_dir.to_path_buf();
    let state_dir = state_dir.to_owned();
    let session_id = session_id.to_owned();
    let bound = tokio::task::spawn_blocking(move || {
        crate::identity_receiver::bound_session_for_id_in(
            &catalog_dir,
            &state_dir,
            &session_id,
            chrono::Utc::now().timestamp(),
        )
    })
    .await??;
    crate::identity_receiver::verify_bound_registry(bound).await
}

/// Per-call authority: the pinned container binding must still be the live
/// binding, and the authored identity term must be unchanged. This is the
/// `LungsAuthority` contract without a socket peer.
struct ContainerAuthority {
    catalog_dir: PathBuf,
    state_dir: String,
    binding: ContainerBinding,
    identity: VerifiedSid1,
}

#[async_trait::async_trait]
impl CallerAuthority for ContainerAuthority {
    async fn revalidate(&self) -> Result<()> {
        let sled = committed_sled(&self.catalog_dir, &self.state_dir, &self.binding.session_id).await?;
        let current = container_binding(&sled).await?;
        ensure!(current == self.binding, "identity container binding changed");
        let identity = authored_identity(&self.catalog_dir, &self.state_dir, &self.binding.session_id).await?;
        ensure!(
            identity.claims() == self.identity.claims(),
            "bound identity term changed"
        );
        Ok(())
    }
}

/// Resolve a native container name to its pinned verified caller. The name
/// comes from bridge configuration only; exactly one active anchored sled may
/// own that container.
async fn resolve_container(
    catalog_dir: &Path,
    state_dir: &str,
    container: &str,
) -> Result<(VerifiedCaller, ContainerBinding)> {
    let sessions = {
        let catalog_dir = catalog_dir.to_path_buf();
        let state_dir = state_dir.to_owned();
        tokio::task::spawn_blocking(move || {
            crate::identity_receiver::committed_sessions_in(&catalog_dir, &state_dir)
        })
        .await??
    };
    let mut candidate = None;
    for sled in sessions.iter().filter(|sled| sled.active && sled.is_anchored()) {
        let owns = sled.instance.as_ref().is_some_and(|instance| instance.name == container
            || instance_uuid(instance).is_ok_and(|uuid| uuid == container));
        if owns {
            ensure!(candidate.is_none(), "container has no unique identity");
            candidate = Some(sled);
        }
    }
    let sled = candidate.with_context(|| format!("no committed identity owns container {container}"))?;
    let binding = container_binding(sled).await?;
    let identity = authored_identity(catalog_dir, state_dir, &binding.session_id).await?;
    ensure!(
        binding.session_genesis == identity.claims().session_genesis,
        "identity anchor changed during binding"
    );
    let authority = Arc::new(ContainerAuthority {
        catalog_dir: catalog_dir.to_path_buf(),
        state_dir: state_dir.to_owned(),
        binding: binding.clone(),
        identity: identity.clone(),
    });
    authority.revalidate().await?;
    Ok((VerifiedCaller::new(&identity, authority), binding))
}

struct FuseExecutor {
    runtime: Arc<Runtime>,
    caller: VerifiedCaller,
    mount: uuid::Uuid,
    handle: tokio::runtime::Handle,
}

impl LocalExecutor for FuseExecutor {
    fn authorize(&self, plugin: &str, method: &str, declaration: &MethodDecl) -> Result<(), LungsError> {
        self.handle.block_on(async {
            self.caller.authority.revalidate().await.map_err(|_| LungsError::Refused("native mounted identity changed".into()))?;
            let (_, actual) = self.runtime.pinned_method(plugin, method)
                .map_err(|_| LungsError::Refused("sealed mounted declaration unavailable".into()))?;
            if !same_declaration(&actual, declaration) { return Err(LungsError::Refused("sealed mounted declaration changed".into())); }
            self.runtime.authorize_nested(&self.caller, plugin, method).await
                .map_err(|_| LungsError::Refused("current exact mounted grant denied".into()))
        })
    }
    fn bind_call(&self, token: uuid::Uuid, plugin: &str, method: &str, schema_hash: &str,
        declaration: &MethodDecl) -> Result<Box<dyn BoundLocalCall>, LungsError> {
        let (_, actual) = self.runtime.pinned_method(plugin, method)
            .map_err(|_| LungsError::Refused("sealed binding declaration unavailable".into()))?;
        if !same_declaration(&actual, declaration) { return Err(LungsError::Refused("sealed binding declaration changed".into())); }
        let mut calls = self.runtime.fuse_calls().pending.lock().map_err(|_| LungsError::Executor("private binding owner unavailable".into()))?;
        let call = calls.get(&token).ok_or_else(|| LungsError::Refused("private binding absent or already consumed".into()))?;
        if call.mount != self.mount || call.target.plugin_id != plugin || call.target.method != method
            || call.target.schema_hash != schema_hash || call.caller.claims() != self.caller.claims()
            || call.caller.principal_id != self.caller.principal_id || call.caller.session_id != self.caller.session_id
            || call.caller.session_genesis != self.caller.session_genesis {
            return Err(LungsError::Refused("private binding differs from actual mounted identity/target".into()));
        }
        call.state.0.compare_exchange(REGISTERED, BOUND, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| LungsError::Refused("private binding cannot be transferred or replayed".into()))?;
        let call = calls.remove(&token).ok_or_else(|| LungsError::Refused("private binding already consumed".into()))?;
        Ok(Box::new(PreparedExecutor { runtime: self.runtime.clone(), mount_caller: self.caller.clone(), call }))
    }
    fn call<'a>(
        &'a self,
        plugin_id: &'a str,
        method_name: &'a str,
        _method: &'a MethodDecl,
        arguments: Value,
        operation_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Value, LungsError>> + Send + 'a>> {
        Box::pin(async move {
            let operation = uuid::Uuid::parse_str(operation_id)
                .map_err(|_| LungsError::Executor("operation id is not canonical".into()))?;
            let completed = self
                .runtime
                .call(self.caller.clone(), plugin_id, method_name, arguments, operation)
                .await
                .map_err(|error| runtime_error(&error, operation))?;
            completed
                .receipt
                .validate_attribution(plugin_id, method_name)
                .map_err(|_| {
                    LungsError::OutcomeUnknown(format!(
                        "operation_id={operation_id}; runtime receipt is invalid"
                    ))
                })?;
            if completed.receipt.operation_id != operation_id {
                return Err(LungsError::OutcomeUnknown(format!(
                    "operation_id={operation_id}; runtime receipt operation mismatch"
                )));
            }
            self.runtime
                .revalidate_result(&completed, plugin_id, method_name)
                .await
                .map_err(|error| runtime_error(&error, operation))?;
            serde_json::to_value(&completed.receipt).map_err(|error| {
                LungsError::OutcomeUnknown(format!(
                    "operation_id={operation_id}; runtime receipt cannot be encoded: {error}"
                ))
            })
        })
    }
}

/// Same canonical classification as the lungs socket's `runtime_error`: only
/// the class and the operation locator cross to the writer.
fn runtime_error(error: &RuntimeError, operation: uuid::Uuid) -> LungsError {
    if error.outcome_unknown() {
        LungsError::OutcomeUnknown(format!(
            "operation_id={operation}; canonical execution did not establish disclosure; do not retry automatically"
        ))
    } else if error.is_denied() {
        LungsError::Refused(format!("method admission refused; operation_id={operation}"))
    } else if error.failed_before_effect() || error.is_unsupported() {
        LungsError::Unsupported(format!("method not admitted; operation_id={operation}"))
    } else {
        LungsError::OutcomeUnknown(format!(
            "operation_id={operation}; canonical runtime failed; do not retry automatically"
        ))
    }
}

struct PreparedExecutor { runtime: Arc<Runtime>, mount_caller: VerifiedCaller, call: PreparedCall }
impl BoundLocalCall for PreparedExecutor {
    fn operation_id(&self) -> uuid::Uuid { self.call.target.operation_id }
    fn call(self: Box<Self>, plugin: String, method: String, declaration: MethodDecl, arguments: Value)
        -> Pin<Box<dyn Future<Output = Result<Value, LungsError>> + Send>> {
        Box::pin(async move {
            let Self { runtime, mount_caller, call } = *self;
            let operation = call.target.operation_id;
            let checked: Result<(), RuntimeError> = async {
                mount_caller.authority.revalidate().await.map_err(RuntimeError::from)?;
                call.caller.authority.revalidate().await.map_err(RuntimeError::from)?;
                let (schema, actual) = runtime.pinned_method(&plugin, &method)?;
                if call.target.plugin_id != plugin || call.target.method != method || schema.schema_hash != call.target.schema_hash
                    || !same_declaration(&actual, &declaration) || crate::core::BridgeCore::method_value_digest(&arguments)? != call.target.arguments_digest {
                    return Err(RuntimeError::denied("prepared FD arguments/schema/target changed"));
                }
                Ok(())
            }.await;
            if let Err(error) = checked {
                call.state.abort_before_dispatch();
                let diagnostic = runtime_error(&error, operation);
                let _ = call.completed.send(Err(error.at_operation(operation)));
                return Err(diagnostic);
            }
            if call.state.0.compare_exchange(BOUND, EXECUTING, Ordering::AcqRel, Ordering::Acquire).is_err() {
                let error = RuntimeError::denied("prepared descriptor was aborted before dispatch").at_operation(operation);
                let diagnostic = runtime_error(&error, operation);
                let _ = call.completed.send(Err(error));
                return Err(diagnostic);
            }
            // Legitimate private post-FUSE callback, never an invocation fallback.
            let completed = runtime.call(call.caller, &plugin, &method, arguments, operation).await;
            match completed {
                Ok(completed) => {
                    if let Err(error) = runtime.revalidate_result(&completed, &plugin, &method).await {
                        let diagnostic = runtime_error(&error, operation);
                        let _ = call.completed.send(Err(error.at_operation(operation)));
                        return Err(diagnostic);
                    }
                    match serde_json::to_value(&completed.receipt) {
                        Ok(wire) => { let _ = call.completed.send(Ok(completed)); Ok(wire) }
                        Err(_) => {
                            let error = RuntimeError::unknown(anyhow::anyhow!("original receipt encoding failed")).at_operation(operation);
                            let diagnostic = runtime_error(&error, operation);
                            let _ = call.completed.send(Err(error));
                            Err(diagnostic)
                        }
                    }
                }
                Err(error) => {
                    let diagnostic = runtime_error(&error, operation);
                    let _ = call.completed.send(Err(error));
                    Err(diagnostic)
                }
            }
        })
    }
}

/// Live bridge-hosted mounts. Dropping the guard detaches every mountpoint;
/// in-flight calls keep their outcome-unknown semantics in the daemon.
pub(crate) struct LocalMounts {
    mountpoints: Vec<PathBuf>,
    threads: Vec<std::thread::JoinHandle<()>>,
    calls: Arc<FuseCalls>,
    keys: Vec<uuid::Uuid>,
    /// Mounts added after startup by the reconciler, and the reconciler task.
    /// Dropping the startup guard stops the reconciler first, then detaches
    /// everything it mounted.
    late: Option<(tokio::task::JoinHandle<()>, Arc<Mutex<Vec<LocalMounts>>>)>,
}

impl LocalMounts {
    fn empty(calls: Arc<FuseCalls>) -> Self {
        Self { mountpoints: Vec::new(), threads: Vec::new(), calls, keys: Vec::new(), late: None }
    }
}

impl Drop for LocalMounts {
    fn drop(&mut self) {
        if let Some((task, late)) = self.late.take() {
            task.abort();
            let late = late.lock().map(|mut late| std::mem::take(&mut *late)).unwrap_or_default();
            drop(late);
        }
        for key in self.keys.drain(..) { self.calls.unregister(key); }
        for mountpoint in &self.mountpoints {
            // SAFETY: path is a live FUSE mountpoint owned by this guard.
            unsafe {
                libc::umount2(
                    std::ffi::CString::new(mountpoint.as_os_str().as_encoded_bytes())
                        .unwrap_or_default()
                        .as_ptr(),
                    libc::MNT_DETACH,
                );
            }
        }
        // A mount copied into another mount namespace keeps its connection open
        // after MNT_DETACH, so its serving thread may never end. Never let that
        // hold shutdown before the bridge's own drain deadline starts; process
        // exit closes the remaining descriptors.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        for thread in self.threads.drain(..) {
            while !thread.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            if thread.is_finished() {
                let _ = thread.join();
            } else {
                tracing::warn!(thread = ?thread.thread().name(),
                    "FUSE mount is still held by another mount namespace; leaving it to process exit");
            }
        }
    }
}

/// Build both mounts for one identity and serve them on dedicated threads.
/// Refuses (mounts nothing for that identity) when the container identity
/// cannot be derived, per requirements R3.4.
async fn start_identity_mounts(
    runtime: Arc<Runtime>,
    spec: &FuseMountSpec,
) -> Result<LocalMounts> {
    let catalog_dir = runtime.catalog_dir().to_path_buf();
    let state_dir = op_core::projection_shm::shm_state_dir();
    let (caller, binding) = resolve_container(&catalog_dir, &state_dir, &spec.container).await?;
    let mut served = LocalMounts::empty(runtime.fuse_calls().clone());
    for (surface, mountpoint) in [
        (Surface::Our, spec.mount_3tched.clone()),
        (Surface::Binary, spec.mount_threetched.clone()),
    ] {
        ensure!(
            mountpoint.is_dir(),
            "mountpoint {} is not a directory",
            mountpoint.display()
        );
        let store_root = spec.store_root.clone();
        let container = spec.container.clone();
        let catalog = catalog_dir.clone();
        let state = PathBuf::from(&state_dir);
        let mount_key = uuid::Uuid::new_v4();
        let exec: Arc<dyn LocalExecutor> = Arc::new(FuseExecutor { runtime: runtime.clone(), caller: caller.clone(),
            mount: mount_key, handle: tokio::runtime::Handle::current() });
        let uid = spec.call_owner_uid;
        let filesystem = tokio::task::spawn_blocking(move || -> Result<TypedFuse> {
            let store = ObjectStore::open(
                &store_root.join(format!("{}-{}", container, surface.as_str())),
            )?;
            let snapshot = SourceCatalog::new(catalog.clone(), state.clone()).capture_schema_only()?;
            let view = store.capture_snapshot(snapshot, true)?;
            let mut filesystem = TypedFuse::local(
                store,
                ProjectionTarget::View(view.view_id),
                state,
                exec,
                surface,
            )?
            .with_call_owner_uid(uid)?
            .with_catalog_dir(catalog)?;
            if surface == Surface::Binary {
                let binary = std::env::var_os("THREETCHED_BIN")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| {
                        PathBuf::from("/usr/local/libexec/op-dbus/threetched/threetched")
                    });
                ensure!(binary.is_file(), "threetched surface binary is missing: {}", binary.display());
                let captured = _3tched_fs::threetched_surface::ThreetchedSurface::capture(&binary)?;
                filesystem.graft_threetched(captured)?;
            }
            Ok(filesystem)
        })
        .await??;
        let instance = uuid::Uuid::parse_str(filesystem.mount_instance_uuid().context("native mount instance absent")?)?;
        let point = mountpoint.clone();
        let thread = std::thread::Builder::new()
            .name(format!("fuse-{}-{}", spec.container, surface.as_str()))
            .spawn(move || {
                let options = vec![
                    fuser::MountOption::FSName(surface.mount_name().to_string()),
                    fuser::MountOption::Subtype("3tchedFS".to_string()),
                    fuser::MountOption::DefaultPermissions,
                    fuser::MountOption::NoDev,
                    fuser::MountOption::NoSuid,
                    fuser::MountOption::NoExec,
                    fuser::MountOption::NoAtime,
                    fuser::MountOption::RW,
                    fuser::MountOption::AllowOther,
                ];
                if let Err(error) = fuser::mount2(filesystem, &point, &options) {
                    tracing::error!(%error, mount = %point.display(), "bridge-hosted FUSE mount ended");
                }
            })
            .context("spawn bridge-hosted FUSE mount thread")?;
        served.mountpoints.push(mountpoint.clone());
        served.threads.push(thread);
        served.keys.push(mount_key);
        runtime.fuse_calls().register(MountRecord { key: mount_key, instance, path: std::fs::canonicalize(mountpoint)?,
            surface, binding: binding.clone() })?;
    }
    Ok(served)
}

/// Mount every spec whose container is not yet served. Fault-isolating per
/// identity: a container that fails to resolve/mount is logged and mounts
/// nothing. `last_error` keeps a retried failure from logging every pass.
async fn mount_pending(
    runtime: &Arc<Runtime>,
    specs: &[FuseMountSpec],
    guard: &mut LocalMounts,
    points: &mut Vec<PathBuf>,
    served: &mut Vec<String>,
    last_error: &mut HashMap<String, String>,
) {
    for spec in specs {
        if served.contains(&spec.container) { continue; }
        let mut canonical = Vec::with_capacity(2);
        let mut invalid = false;
        for point in [&spec.mount_3tched, &spec.mount_threetched] {
            match std::fs::canonicalize(point) {
                Ok(point) => canonical.push(point),
                Err(error) => { tracing::warn!(%error, container = %spec.container, mount = %point.display(), "bridge-hosted FUSE mountpoint unavailable; skipping identity"); invalid = true; break; }
            }
        }
        if invalid { continue; }
        if canonical.iter().any(|point| points.iter().any(|old: &PathBuf| point.starts_with(old) || old.starts_with(point))) {
            tracing::warn!(container = %spec.container, "bridge-hosted FUSE mount roots overlap an earlier identity; skipping identity");
            continue;
        }
        match start_identity_mounts(runtime.clone(), spec).await {
            Ok(mut mounted) => {
                points.extend(canonical);
                served.push(spec.container.clone());
                last_error.remove(&spec.container);
                guard.mountpoints.append(&mut mounted.mountpoints);
                guard.threads.append(&mut mounted.threads);
                guard.keys.append(&mut mounted.keys);
                tracing::info!(container = %spec.container, "bridge-hosted FUSE mounts serving");
            }
            Err(error) => {
                let text = format!("{error:#}");
                if last_error.get(&spec.container) != Some(&text) {
                    tracing::warn!(error = %text, container = %spec.container, "bridge-hosted FUSE mounts not started for identity; will retry");
                    last_error.insert(spec.container.clone(), text);
                }
            }
        }
    }
}

async fn current_specs(runtime: &Runtime) -> Vec<FuseMountSpec> {
    let mut specs = match specs_from_env() {
        Ok(specs) => specs,
        Err(error) => { tracing::warn!(%error, "invalid OP_FUSE_LOCAL_MOUNTS; continuing with auto-enumerated mounts only"); Vec::new() }
    };
    for auto in auto_specs(runtime).await {
        if !specs.iter().any(|spec| spec.container == auto.container) { specs.push(auto); }
    }
    if specs.len() > 64 { tracing::warn!(count = specs.len(), "mounted identity pair capacity exceeded; truncating"); specs.truncate(64); }
    specs
}

/// How often committed identities are rechecked for containers that were not
/// running (or not yet provisioned) at the previous pass.
const RECONCILE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

/// Start every bridge-hosted mount pair: explicit `OP_FUSE_LOCAL_MOUNTS`
/// specs plus auto-enumerated specs from committed sleds (explicit specs win
/// per container). Bootstrap calls this after `Runtime::open` has run
/// `adopt_identity_volumes`/`recover_startup`, so the committed projection is
/// already indexed. A reconciler then rechecks every `RECONCILE_INTERVAL`, so
/// a container started or provisioned after the bridge gets its mounts
/// without a bridge restart. Known gap: a container that restarts keeps its
/// (now failing-revalidation) mounts until bridge restart.
pub(crate) async fn start_local_mounts(runtime: Arc<Runtime>) -> Result<LocalMounts> {
    let mut guard = LocalMounts::empty(runtime.fuse_calls().clone());
    if local_mounts_disabled() {
        tracing::info!("bridge-hosted FUSE mounts disabled by OP_FUSE_LOCAL_MOUNTS");
        return Ok(guard);
    }
    let mut points = Vec::<PathBuf>::new();
    let mut served = Vec::<String>::new();
    let mut last_error = HashMap::<String, String>::new();
    let specs = current_specs(&runtime).await;
    mount_pending(&runtime, &specs, &mut guard, &mut points, &mut served, &mut last_error).await;
    let late = Arc::new(Mutex::new(Vec::<LocalMounts>::new()));
    let task_late = late.clone();
    let task_runtime = runtime.clone();
    let task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(RECONCILE_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            let specs = current_specs(&task_runtime).await;
            if specs.iter().all(|spec| served.contains(&spec.container)) { continue; }
            let mut added = LocalMounts::empty(task_runtime.fuse_calls().clone());
            mount_pending(&task_runtime, &specs, &mut added, &mut points, &mut served, &mut last_error).await;
            if added.mountpoints.is_empty() { continue; }
            match task_late.lock() {
                Ok(mut late) => late.push(added),
                Err(_) => { tracing::error!("bridge-hosted FUSE reconciler state poisoned; stopping"); return; }
            }
        }
    });
    guard.late = Some((task, late));
    Ok(guard)
}

pub(crate) async fn invoke_bound(runtime: &Runtime, caller: &VerifiedCaller, target: CheckpointTarget,
    arguments: Value) -> Result<RuntimeResult, FuseInvocationFailure> {
    let operation = target.operation_id;
    let state = Arc::new(CallState(AtomicU8::new(REGISTERED)));
    let state_dir = op_core::projection_shm::shm_state_dir();
    let sled = committed_sled(runtime.catalog_dir(), &state_dir, &caller.session_id).await
        .map_err(|_| invocation_failure(operation, &state))?;
    let binding = container_binding(&sled).await.map_err(|_| invocation_failure(operation, &state))?;
    let identity = authored_identity(runtime.catalog_dir(), &state_dir, &caller.session_id).await
        .map_err(|_| invocation_failure(operation, &state))?;
    if caller.claims() != identity.claims() || caller.session_genesis != binding.session_genesis {
        return Err(invocation_failure(operation, &state));
    }
    let mount = runtime.fuse_calls().select(&binding).map_err(|_| invocation_failure(operation, &state))?;
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let token = uuid::Uuid::new_v4();
    {
        let mut pending = runtime.fuse_calls().pending.lock().map_err(|_| invocation_failure(operation, &state))?;
        if pending.len() >= MAX_PREPARED_CALLS || pending.try_reserve(1).is_err() {
            return Err(invocation_failure(operation, &state));
        }
        pending.insert(token, PreparedCall { mount: mount.key, caller: caller.clone(), target: target.clone(),
            state: state.clone(), completed: sender });
    }
    let registration = Registration { calls: runtime.fuse_calls().clone(), token };
    let plugin = target.plugin_id.clone();
    let method = target.method.clone();
    let io = tokio::task::spawn_blocking(move || -> Result<Value> {
        let namespace = _3tched_fs::mounted::MountedNamespace::open_for_identity_mode(
            &mount.path, _3tched_fs::mounted::IdentityMode::ContainerBound, None)?;
        let path = format!("plugins/{}/operations/{}/call",
            _3tched_fs::namespace::encode_component(&plugin), _3tched_fs::namespace::encode_component(&method));
        namespace.invoke_bound(&path, arguments, token, operation, mount.instance)
    }).await;
    drop(registration);
    let wire = match io {
        Ok(Ok(wire)) => wire,
        // Never convert private completion into FD success. The atomic abort
        // also prevents a queued bound worker from starting after this failure.
        _ => return Err(invocation_failure(operation, &state)),
    };
    let completed = receiver.await.map_err(|_| invocation_failure(operation, &state))?
        .map_err(|error| FuseInvocationFailure { error, before_dispatch: state.abort_before_dispatch() })?;
    let actual = serde_json::to_value(&completed.receipt).map_err(|_| invocation_failure(operation, &state))?;
    if wire.get("result") != Some(&actual) || completed.receipt.operation_id != operation.to_string()
        || completed.receipt.plugin_id != target.plugin_id || completed.receipt.method != target.method {
        return Err(invocation_failure(operation, &state));
    }
    Ok(completed)
}

impl Runtime {
    /// Selection is only by actual committed native name/UUID, never a label,
    /// UID, credential/model session or caller-supplied identity metadata.
    pub(crate) async fn resolve_executor(&self, requestor: &VerifiedCaller, principal: &str, container: &str)
        -> Result<VerifiedCaller, RuntimeError> {
        requestor.authority.revalidate().await.map_err(RuntimeError::from)?;
        let (mut caller, _) = resolve_container(self.catalog_dir(), &op_core::projection_shm::shm_state_dir(), container).await
            .map_err(|_| RuntimeError::denied("executor has no unique current committed native container binding"))?;
        if caller.principal_id != principal {
            return Err(RuntimeError::denied("approved executor principal differs from native container identity"));
        }
        caller.require_delegated_checkpoint();
        Ok(caller)
    }
}

/// The sealed catalog and the mount carry the same declaration as two
/// structs; compare every field, the side effect through its serialized form.
fn same_declaration(sealed: &op_state_store::MethodDecl, mounted: &MethodDecl) -> bool {
    match (serde_json::to_value(sealed), serde_json::to_value(mounted)) {
        (Ok(sealed), Ok(mounted)) => sealed == mounted,
        _ => false,
    }
}
