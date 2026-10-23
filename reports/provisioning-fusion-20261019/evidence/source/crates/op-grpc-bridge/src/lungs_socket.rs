//! One local D-Bus lungs transport with kernel-authenticated container callers.
//!
//! Host root in the bridge's own user and PID namespaces may `Bind(session_id)`
//! for the host FUSE mounts. Guests instead call `BindCurrentContainer()` with
//! no identity input. Their kernel peer PID must belong to exactly one running,
//! committed identity container. Every operation revalidates that native
//! instance UUID, init process, peer process, namespaces and current principal.
//! No credential is copied into a guest, and UID alone never selects identity.

use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::{FileTypeExt as _, MetadataExt as _, PermissionsExt as _};
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{ensure, Context, Result};
use op_plugins::state_plugins::identity_sled::ContainerIdentitySled;
use op_plugins::state_plugins::incus::{IncusInstance, IncusPlugin, IncusRootRepairBackend};
use zbus::connection::AuthMechanism;

use crate::dbus_identity::{authoritative_session_identity, DbusIdentityError};
use crate::interceptor::GhostbridgeIdentity;
use crate::mutation_engine::MutationEngine;
use crate::schema_router::SchemaRouter;

pub const DEFAULT_LUNGS_SOCKET: &str = "/run/opdbus/lungs/lungs.sock";
pub const LUNGS_OBJECT_PATH: &str = "/org/opdbus/v1/lungs";
const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONNECTIONS: usize = 64;

/// This fd refers to the socket's original peer, even after its numeric PID is
/// recycled. Opening a pidfd by that numeric PID would already be too late.
struct PeerLifetime(OwnedFd);

impl PeerLifetime {
    fn from_socket(socket: &impl AsRawFd) -> Result<Self> {
        let mut fd: libc::c_int = -1;
        let mut length = std::mem::size_of_val(&fd) as libc::socklen_t;
        // SAFETY: both output pointers refer to live, correctly sized values;
        // the borrowed socket remains open throughout this call.
        let result = unsafe {
            libc::getsockopt(
                socket.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERPIDFD,
                (&mut fd as *mut libc::c_int).cast(),
                &mut length,
            )
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error())
                .context("socket peer lifetime unavailable");
        }
        ensure!(fd >= 0, "socket peer lifetime unavailable");
        // SAFETY: successful SO_PEERPIDFD returns a new owned, close-on-exec fd.
        let lifetime = Self(unsafe { OwnedFd::from_raw_fd(fd) });
        ensure!(
            length as usize == std::mem::size_of_val(&fd),
            "invalid socket peer lifetime"
        );
        lifetime.ensure_alive()?;
        Ok(lifetime)
    }

    fn ensure_alive(&self) -> Result<()> {
        let mut descriptor = libc::pollfd {
            fd: self.0.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: descriptor is a live pollfd; timeout zero cannot block.
        let result = unsafe { libc::poll(&mut descriptor, 1, 0) };
        if result < 0 {
            return Err(std::io::Error::last_os_error())
                .context("socket peer lifetime unavailable");
        }
        ensure!(result == 0 && descriptor.revents == 0, "socket peer exited");
        Ok(())
    }

    fn read_process(&self, pid: u32) -> Result<ProcessIdentity> {
        self.ensure_alive()?;
        let process = ProcessIdentity::read(pid)?;
        self.ensure_alive()?;
        Ok(process)
    }

    fn revalidate(&self, process: &ProcessIdentity) -> Result<()> {
        self.ensure_alive()?;
        process.revalidate()?;
        self.ensure_alive()
    }
}

/// Holding the namespace open prevents its inode being recycled while bound.
#[derive(Clone, Debug)]
struct NamespaceIdentity {
    dev: u64,
    ino: u64,
    _handle: Arc<File>,
}

impl PartialEq for NamespaceIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.dev == other.dev && self.ino == other.ino
    }
}
impl Eq for NamespaceIdentity {}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProcessIdentity {
    pid: u32,
    start_ticks: u64,
    user_namespace: NamespaceIdentity,
    pid_namespace: NamespaceIdentity,
}

impl ProcessIdentity {
    fn read(pid: u32) -> Result<Self> {
        ensure!(pid > 0, "peer process is unavailable");
        let start_ticks = read_start_ticks(pid)?;
        let namespace = |name| -> Result<NamespaceIdentity> {
            let handle = File::open(format!("/proc/{pid}/ns/{name}"))?;
            let metadata = handle.metadata()?;
            Ok(NamespaceIdentity {
                dev: metadata.dev(),
                ino: metadata.ino(),
                _handle: Arc::new(handle),
            })
        };
        let identity = Self {
            pid,
            start_ticks,
            user_namespace: namespace("user")?,
            pid_namespace: namespace("pid")?,
        };
        ensure!(
            read_start_ticks(pid)? == start_ticks,
            "peer process changed"
        );
        Ok(identity)
    }

    fn same_namespaces(&self, other: &Self) -> bool {
        self.user_namespace == other.user_namespace && self.pid_namespace == other.pid_namespace
    }

    fn revalidate(&self) -> Result<()> {
        ensure!(Self::read(self.pid)? == *self, "peer process changed");
        Ok(())
    }
}

fn read_start_ticks(pid: u32) -> Result<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    // The process name can contain spaces and ')'. Fields after its final
    // closing ')' start at field 3; field 22 is the process start time.
    parse_start_ticks(&stat).context("invalid process identity")
}

fn parse_start_ticks(stat: &str) -> Option<u64> {
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

#[derive(Clone, Debug)]
struct ContainerBinding {
    session_id: String,
    session_genesis: String,
    native_name: String,
    native_uuid: String,
    init: ProcessIdentity,
}

enum Binding {
    Host(String),
    Container(ContainerBinding),
}

impl Binding {
    fn session_id(&self) -> &str {
        match self {
            Self::Host(session) => session,
            Self::Container(binding) => &binding.session_id,
        }
    }
}

/// The identity one lungs connection breathes for, set once by bootstrap.
pub struct LungsIdentity {
    engine: Arc<MutationEngine>,
    peer_uid: u32,
    peer: ProcessIdentity,
    peer_lifetime: PeerLifetime,
    host: Arc<ProcessIdentity>,
    binding: OnceLock<Binding>,
    bootstrap: tokio::sync::Mutex<()>,
    bound: tokio::sync::Notify,
}

impl LungsIdentity {
    fn new(
        engine: Arc<MutationEngine>,
        peer_uid: u32,
        peer: ProcessIdentity,
        peer_lifetime: PeerLifetime,
        host: Arc<ProcessIdentity>,
    ) -> Self {
        Self {
            engine,
            peer_uid,
            peer,
            peer_lifetime,
            host,
            binding: OnceLock::new(),
            bootstrap: tokio::sync::Mutex::new(()),
            bound: tokio::sync::Notify::new(),
        }
    }

    fn is_host_root(&self) -> bool {
        host_root_allowed(self.peer_uid, &self.peer, &self.host)
    }

    fn revalidate_peer(&self) -> Result<()> {
        self.peer_lifetime.revalidate(&self.peer)
    }

    /// Resolution is performed for every PluginV1.Call. Native restart or
    /// replacement cannot inherit a connection, even with the same UID/name.
    pub async fn resolve(&self) -> Result<GhostbridgeIdentity, DbusIdentityError> {
        let binding = self.binding.get().ok_or(DbusIdentityError::NoBinding)?;
        let outcome = async {
            self.revalidate_peer()?;
            match binding {
                Binding::Host(_) => ensure!(self.is_host_root(), "host peer changed"),
                Binding::Container(pinned) => {
                    let sled = crate::identity_sled_dispatch::stored_session(
                        self.engine.as_ref(),
                        &pinned.session_id,
                    )
                    .await?
                    .context("identity binding unavailable")?;
                    let current = self.container_binding(&sled).await?;
                    ensure!(same_binding(pinned, &current), "container binding changed");
                }
            }
            self.revalidate_peer()?;
            let identity = authoritative_session_identity(
                self.engine.as_ref(),
                None,
                binding.session_id(),
                true,
            )
            .await
            .map_err(anyhow::Error::from)?;
            self.revalidate_peer()?;
            Ok(identity)
        };
        tokio::time::timeout(NEGOTIATION_TIMEOUT, outcome)
            .await
            .map_err(|_| DbusIdentityError::StaleBinding)?
            .map_err(|error| {
                tracing::warn!(%error, "lungs identity revalidation refused");
                DbusIdentityError::StaleBinding
            })
    }

    async fn container_binding(&self, sled: &ContainerIdentitySled) -> Result<ContainerBinding> {
        self.revalidate_peer()?;
        ensure!(sled.active && sled.is_anchored(), "identity is not active");
        let stored = sled
            .instance
            .as_ref()
            .context("identity container unavailable")?;
        ensure!(
            crate::identity_sled_dispatch::identity_instance_matches(sled, stored),
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
                && crate::identity_sled_dispatch::identity_instance_matches(sled, &current),
            "native identity container does not match"
        );
        let init_pid = native.instance_init_pid(&stored.name).await?;
        let init = ProcessIdentity::read(init_pid)?;
        ensure!(
            self.peer.same_namespaces(&init),
            "peer is outside identity container"
        );
        self.revalidate_peer()?;
        init.revalidate()?;
        // Close the native read/proc race before publishing this term.
        ensure!(
            native.instance_init_pid(&stored.name).await? == init_pid,
            "identity container restarted"
        );
        init.revalidate()?;
        self.revalidate_peer()?;
        Ok(ContainerBinding {
            session_id: sled.session_id.clone(),
            session_genesis: sled
                .genesis
                .clone()
                .context("identity anchor unavailable")?,
            native_name: stored.name.clone(),
            native_uuid,
            init,
        })
    }

    async fn publish_binding(&self, binding: Binding) -> Result<()> {
        self.revalidate_peer()?;
        let identity =
            authoritative_session_identity(self.engine.as_ref(), None, binding.session_id(), true)
                .await?;
        let sled = crate::identity_sled_dispatch::stored_session(
            self.engine.as_ref(),
            &identity.session_id,
        )
        .await?
        .context("lungs session disappeared")?;
        self.engine
            .register_session_context(crate::mutation_engine::SessionContext {
                genesis_hex: identity.session_genesis,
                session_id: identity.session_id,
                wireguard_pubkey: sled.wireguard_pubkey.clone(),
                sealed_id_digest: crate::mutation_engine::sealed_id_digest(
                    sled.sealed_id.as_deref(),
                ),
            })
            .await;
        self.revalidate_peer()?;
        self.binding
            .set(binding)
            .map_err(|_| anyhow::anyhow!("this lungs connection is already bound"))?;
        self.bound.notify_one();
        Ok(())
    }

    async fn bind(&self, session_id: &str) -> Result<()> {
        let _bootstrap = self.bootstrap.lock().await;
        self.revalidate_peer()?;
        ensure!(self.is_host_root(), "host binding requires host root");
        ensure!(
            valid_session_id(session_id),
            "session id is not a plain name"
        );
        ensure!(
            self.binding.get().is_none(),
            "this lungs connection is already bound"
        );
        self.publish_binding(Binding::Host(session_id.to_owned()))
            .await
    }

    async fn bind_current_container(&self) -> Result<()> {
        let _bootstrap = self.bootstrap.lock().await;
        self.revalidate_peer()?;
        ensure!(
            self.binding.get().is_none(),
            "this lungs connection is already bound"
        );
        // A host process cannot claim a guest by choosing its shared mapped UID.
        ensure!(
            self.peer.pid_namespace != self.host.pid_namespace,
            "host process has no container binding"
        );
        self.revalidate_peer()?;
        let sessions = crate::identity_sled_dispatch::all_sessions(self.engine.as_ref()).await?;
        let mut candidates = Vec::new();
        for sled in sessions
            .iter()
            .filter(|sled| sled.active && sled.is_anchored())
        {
            if let Ok(binding) = self.container_binding(sled).await {
                candidates.push(binding);
            }
        }
        let binding = exactly_one(candidates)?;
        self.publish_binding(Binding::Container(binding)).await
    }
}

fn instance_uuid(instance: &IncusInstance) -> Result<String> {
    let value = instance
        .config
        .as_ref()
        .and_then(|config| config.get("volatile.uuid"))
        .context("native instance UUID unavailable")?;
    ensure!(
        uuid::Uuid::parse_str(value).is_ok_and(|uuid| uuid.to_string() == *value),
        "native instance UUID is invalid"
    );
    Ok(value.clone())
}

fn same_binding(pinned: &ContainerBinding, current: &ContainerBinding) -> bool {
    pinned.session_id == current.session_id
        && pinned.session_genesis == current.session_genesis
        && pinned.native_name == current.native_name
        && pinned.native_uuid == current.native_uuid
        && pinned.init == current.init
}

fn host_root_allowed(uid: u32, peer: &ProcessIdentity, host: &ProcessIdentity) -> bool {
    uid == 0 && peer.same_namespaces(host)
}

fn exactly_one(mut candidates: Vec<ContainerBinding>) -> Result<ContainerBinding> {
    ensure!(
        candidates.len() == 1,
        "peer has no unique identity container"
    );
    Ok(candidates.remove(0))
}

struct LungsBootstrap {
    identity: Arc<LungsIdentity>,
}

#[zbus::interface(name = "org.opdbus.v1.LungsV1")]
impl LungsBootstrap {
    async fn bind(&self, session_id: String) -> zbus::fdo::Result<bool> {
        self.identity
            .bind(&session_id)
            .await
            .map_err(bootstrap_denial)?;
        Ok(true)
    }

    async fn bind_current_container(&self) -> zbus::fdo::Result<bool> {
        tokio::time::timeout(NEGOTIATION_TIMEOUT, self.identity.bind_current_container())
            .await
            .map_err(|_| bootstrap_denial(anyhow::anyhow!("container binding timed out")))?
            .map_err(bootstrap_denial)?;
        Ok(true)
    }
}

fn bootstrap_denial(error: anyhow::Error) -> zbus::fdo::Error {
    tracing::warn!(%error, "lungs bootstrap refused");
    zbus::fdo::Error::AccessDenied("lungs identity binding refused".into())
}

fn valid_session_id(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id.len() <= 128
        && session_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// Access to the socket grants negotiation only; bootstrap and every operation
/// authenticate kernel/native identity before capability admission.
fn bind_socket(path: &Path) -> Result<tokio::net::UnixListener> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => std::fs::remove_file(path)?,
        Ok(_) => anyhow::bail!("{} exists and is not a socket", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let listener =
        tokio::net::UnixListener::bind(path).with_context(|| format!("bind {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o666))?;
    Ok(listener)
}

pub async fn serve(
    path: &Path,
    router: Arc<SchemaRouter>,
    engine: Arc<MutationEngine>,
) -> Result<()> {
    let host = Arc::new(ProcessIdentity::read(std::process::id())?);
    let listener = bind_socket(path)?;
    tracing::info!(socket = %path.display(), "lungs socket listening");
    let connections = Arc::new(tokio::sync::Semaphore::new(MAX_CONNECTIONS));
    loop {
        let permit = connections.clone().acquire_owned().await?;
        let (stream, _) = listener.accept().await?;
        // Capture the kernel's socket-origin pidfd before consulting /proc.
        // Missing SO_PEERPIDFD support must not fall back to numeric PID auth.
        let peer_lifetime = match PeerLifetime::from_socket(&stream) {
            Ok(lifetime) => lifetime,
            Err(error) => {
                tracing::warn!(%error, "lungs peer lifetime refused");
                continue;
            }
        };
        let credentials = match stream.peer_cred() {
            Ok(credentials) => credentials,
            Err(_) => continue,
        };
        let Some(pid) = credentials.pid().and_then(|pid| u32::try_from(pid).ok()) else {
            continue;
        };
        let peer = match peer_lifetime.read_process(pid) {
            Ok(peer) => peer,
            Err(_) => continue,
        };
        let router = router.clone();
        let identity = Arc::new(LungsIdentity::new(
            engine.clone(),
            credentials.uid(),
            peer,
            peer_lifetime,
            host.clone(),
        ));
        tokio::spawn(async move {
            let _permit = permit;
            let outcome = async {
                let mut builder = zbus::connection::Builder::unix_stream(stream)
                    .server(zbus::Guid::generate())?
                    .p2p()
                    .auth_mechanism(AuthMechanism::Anonymous)
                    .serve_at(
                        LUNGS_OBJECT_PATH,
                        LungsBootstrap {
                            identity: identity.clone(),
                        },
                    )?;
                for (object_path, object) in router.lungs_objects(identity.clone()).await {
                    builder = builder.serve_at(object_path, object)?;
                }
                let connection = tokio::time::timeout(NEGOTIATION_TIMEOUT, builder.build())
                    .await
                    .context("lungs D-Bus negotiation timed out")??;
                // World-connectable guests must not retain all admission slots
                // indefinitely without establishing an authenticated binding.
                tokio::time::timeout(NEGOTIATION_TIMEOUT, async {
                    while identity.binding.get().is_none() {
                        identity.bound.notified().await;
                    }
                })
                .await
                .context("lungs identity bootstrap timed out")?;
                connection.closed().await;
                Ok::<_, anyhow::Error>(())
            }
            .await;
            if let Err(error) = outcome {
                tracing::warn!(%error, "lungs connection ended");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, ticks: u64, user_ns: u64, pid_ns: u64) -> ProcessIdentity {
        let handle = Arc::new(File::open("/dev/null").unwrap());
        ProcessIdentity {
            pid,
            start_ticks: ticks,
            user_namespace: NamespaceIdentity {
                dev: 1,
                ino: user_ns,
                _handle: handle.clone(),
            },
            pid_namespace: NamespaceIdentity {
                dev: 1,
                ino: pid_ns,
                _handle: handle,
            },
        }
    }

    fn candidate() -> ContainerBinding {
        ContainerBinding {
            session_id: "own-session".into(),
            session_genesis: "own-anchor".into(),
            native_name: "Chatbot3tched".into(),
            native_uuid: "4dece3a4-d987-44a0-b8b5-d7cb27211ed0".into(),
            init: process(23, 100, 2, 3),
        }
    }

    #[test]
    fn host_root_requires_both_host_namespaces() {
        let host = process(1, 50, 1, 1);
        let privileged_guest = process(23, 100, 1, 3);
        assert!(!host_root_allowed(0, &privileged_guest, &host));
        let unprivileged_guest = process(23, 100, 2, 3);
        assert!(!host_root_allowed(1_000_000, &unprivileged_guest, &host));
        assert!(host_root_allowed(0, &process(24, 60, 1, 1), &host));
        assert!(!host_root_allowed(1000, &process(24, 60, 1, 1), &host));
    }

    #[test]
    fn shared_uid_or_host_process_cannot_match_guest_namespaces() {
        let first = process(22, 100, 2, 3);
        let second = process(24, 120, 4, 5);
        let host = process(25, 130, 1, 1);
        assert!(!first.same_namespaces(&second));
        assert!(!host.same_namespaces(&first));
        assert!(process(26, 140, 2, 3).same_namespaces(&first));
    }

    #[test]
    fn stale_init_pid_uuid_and_wrong_session_do_not_inherit_binding() {
        let pinned = candidate();
        assert!(same_binding(&pinned, &candidate()));
        let mut changed = candidate();
        changed.init.start_ticks += 1;
        assert!(!same_binding(&pinned, &changed));
        changed = candidate();
        changed.init.pid += 1;
        assert!(!same_binding(&pinned, &changed));
        changed = candidate();
        changed.native_uuid = "different".into();
        assert!(!same_binding(&pinned, &changed));
        changed = candidate();
        changed.session_id = "other-session".into();
        assert!(!same_binding(&pinned, &changed));
        changed = candidate();
        changed.session_genesis = "replacement-anchor".into();
        assert!(!same_binding(&pinned, &changed));
        changed = candidate();
        changed.native_name = "other-container".into();
        assert!(!same_binding(&pinned, &changed));
    }

    #[test]
    fn absent_or_ambiguous_container_identity_is_denied() {
        assert!(exactly_one(Vec::new()).is_err());
        assert!(exactly_one(vec![candidate(), candidate()]).is_err());
        assert_eq!(
            exactly_one(vec![candidate()]).unwrap().session_id,
            "own-session"
        );
    }

    #[test]
    fn process_stat_handles_spaces_and_parentheses() {
        let mut fields = vec!["S"; 20];
        fields[19] = "1234";
        assert_eq!(
            parse_start_ticks(&format!("42 (name ) with spaces) {}", fields.join(" "))),
            Some(1234)
        );
        assert_eq!(parse_start_ticks("invalid"), None);
    }

    #[test]
    fn kernel_process_identity_rejects_stale_start_time() {
        let mut peer = ProcessIdentity::read(std::process::id()).unwrap();
        assert!(peer.revalidate().is_ok());
        peer.start_ticks += 1;
        assert!(peer.revalidate().is_err());
    }

    // Re-exec the test harness so the connecting process can exit without
    // forking inside a multithreaded Rust process. This fixture does nothing
    // when explicitly run without its private socket argument.
    #[test]
    #[ignore]
    fn peer_connector_fixture() {
        use std::io::{Read as _, Write as _};
        let Ok(path) = std::env::var("OPDBUS_LUNGS_PIDFD_FIXTURE_SOCKET") else {
            return;
        };
        let mut stream = std::os::unix::net::UnixStream::connect(path).unwrap();
        stream.write_all(b"x").unwrap();
        let _ = std::io::stdin().read_exact(&mut [0]);
        drop(stream);
    }

    #[test]
    fn exited_socket_peer_cannot_use_another_live_process_identity() {
        use std::io::Read as _;
        use std::process::{Command, Stdio};
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("peer.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "lungs_socket::tests::peer_connector_fixture",
                "--ignored",
            ])
            .env("OPDBUS_LUNGS_PIDFD_FIXTURE_SOCKET", &path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        child.try_wait().unwrap().is_none(),
                        "peer fixture exited early"
                    );
                    assert!(
                        std::time::Instant::now() < deadline,
                        "peer fixture timed out"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("accept peer fixture: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.read_exact(&mut [0]).unwrap();
        let lifetime = PeerLifetime::from_socket(&stream).unwrap();
        let original = lifetime.read_process(child.id()).unwrap();
        assert!(lifetime.revalidate(&original).is_ok());
        drop(child.stdin.take());
        assert!(child.wait().unwrap().success());

        // The unrelated process is live and its /proc data is valid, just as
        // recycled numeric PID data would be. The socket-origin pin rejects it
        // before that data can replace the exited original peer's identity.
        let unrelated = ProcessIdentity::read(std::process::id()).unwrap();
        assert!(unrelated.revalidate().is_ok());
        assert!(lifetime.read_process(unrelated.pid).is_err());
        assert!(lifetime.revalidate(&unrelated).is_err());
        // Also reject a connection first inspected only after its peer exited.
        assert!(PeerLifetime::from_socket(&stream).is_err());
    }

    #[test]
    fn session_ids_are_plain_names() {
        assert!(valid_session_id("b0c777e3-0955-cd8b-3d01-ca12998cae28"));
        for session_id in ["", "../escape", "a/b", "a.b", "x\0y", &"a".repeat(129)] {
            assert!(!valid_session_id(session_id), "{session_id:?}");
        }
    }

    #[tokio::test]
    async fn bind_socket_replaces_stale_socket_without_opening_parent_writes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("opdbus").join("lungs.sock");
        drop(bind_socket(&path).unwrap());
        let _again = bind_socket(&path).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o666
        );
        assert_eq!(
            std::fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o022,
            0
        );
    }

    #[tokio::test]
    async fn bind_socket_refuses_to_replace_a_file_that_is_not_a_socket() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("lungs.sock");
        std::fs::write(&path, b"keep").unwrap();
        assert!(bind_socket(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"keep");
    }
}
