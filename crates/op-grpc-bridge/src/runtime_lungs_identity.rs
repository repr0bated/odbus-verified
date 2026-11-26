//! Native peer proof for the new bridge's local lungs transport.
//! No credential is accepted from the peer, minted, or copied into a guest.

use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::identity_sled::ContainerIdentitySled;
use op_plugins::state_plugins::incus::{IncusInstance, IncusPlugin, IncusRootRepairBackend};

use crate::identity_receiver::VerifiedSid1;
use crate::runtime::{CallerAuthority, VerifiedCaller};
use super::NEGOTIATION_TIMEOUT;

/// SO_PEERPIDFD refers to the original connector, not a reusable numeric PID.
struct PeerLifetime(OwnedFd);

impl PeerLifetime {
    fn from_socket(socket: &impl AsRawFd) -> Result<Self> {
        let mut fd: libc::c_int = -1;
        let mut length = std::mem::size_of_val(&fd) as libc::socklen_t;
        // SAFETY: the borrowed socket and both correctly sized outputs live
        // through getsockopt. Successful SO_PEERPIDFD transfers one owned fd.
        let result = unsafe {
            libc::getsockopt(
                socket.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERPIDFD,
                (&mut fd as *mut libc::c_int).cast(), &mut length,
            )
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error()).context("socket peer lifetime unavailable");
        }
        ensure!(fd >= 0, "socket peer lifetime unavailable");
        // SAFETY: the successful getsockopt returned a new close-on-exec fd.
        let lifetime = Self(unsafe { OwnedFd::from_raw_fd(fd) });
        ensure!(length as usize == std::mem::size_of_val(&fd), "invalid socket peer lifetime");
        lifetime.ensure_alive()?;
        Ok(lifetime)
    }

    fn ensure_alive(&self) -> Result<()> {
        let mut descriptor = libc::pollfd {
            fd: self.0.as_raw_fd(), events: libc::POLLIN, revents: 0,
        };
        // SAFETY: descriptor is writable and timeout zero cannot block.
        let result = unsafe { libc::poll(&mut descriptor, 1, 0) };
        if result < 0 {
            return Err(std::io::Error::last_os_error()).context("socket peer lifetime unavailable");
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

/// Holding the namespace open prevents inode recycling for a live binding.
#[derive(Clone, Debug)]
struct NamespaceIdentity {
    dev: u64,
    ino: u64,
    _handle: Arc<File>,
}

impl PartialEq for NamespaceIdentity {
    fn eq(&self, other: &Self) -> bool { self.dev == other.dev && self.ino == other.ino }
}
impl Eq for NamespaceIdentity {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProcessIdentity {
    pid: u32,
    start_ticks: u64,
    user_namespace: NamespaceIdentity,
    pid_namespace: NamespaceIdentity,
}

impl ProcessIdentity {
    pub(super) fn current() -> Result<Self> { Self::read(std::process::id()) }

    pub(super) fn read(pid: u32) -> Result<Self> {
        ensure!(pid > 0, "peer process is unavailable");
        let start_ticks = read_start_ticks(pid)?;
        let namespace = |name| -> Result<NamespaceIdentity> {
            let handle = File::open(format!("/proc/{pid}/ns/{name}"))?;
            let metadata = handle.metadata()?;
            Ok(NamespaceIdentity { dev: metadata.dev(), ino: metadata.ino(), _handle: Arc::new(handle) })
        };
        let identity = Self {
            pid, start_ticks,
            user_namespace: namespace("user")?,
            pid_namespace: namespace("pid")?,
        };
        ensure!(read_start_ticks(pid)? == start_ticks, "peer process changed");
        Ok(identity)
    }

    fn same_namespaces(&self, other: &Self) -> bool {
        self.user_namespace == other.user_namespace && self.pid_namespace == other.pid_namespace
    }

    pub(super) fn revalidate(&self) -> Result<()> {
        ensure!(Self::read(self.pid)? == *self, "peer process changed");
        Ok(())
    }
}

fn read_start_ticks(pid: u32) -> Result<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    parse_start_ticks(&stat).context("invalid process identity")
}

fn parse_start_ticks(stat: &str) -> Option<u64> {
    // Names may contain spaces and ')'; the suffix begins at field 3.
    stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()
}

#[derive(Debug, PartialEq, Eq)]
struct ContainerBinding {
    session_id: String,
    session_genesis: String,
    native_name: String,
    native_uuid: String,
    init: ProcessIdentity,
}

enum Binding {
    Host,
    Container(ContainerBinding),
}

struct PinnedBinding {
    target: Binding,
    identity: RwLock<VerifiedSid1>,
}

/// One connection binds once. Every operation rechecks the pinned native term,
/// private identity projection and unrevoked principal registry, including at
/// Runtime's effect boundary through CallerAuthority.
pub(super) struct LungsIdentity {
    peer_uid: u32,
    peer: ProcessIdentity,
    peer_lifetime: PeerLifetime,
    host: Arc<ProcessIdentity>,
    catalog_dir: PathBuf,
    state_dir: String,
    /// The Cozo index derived from identity sled volumes.
    identity_store: Arc<op_cozo_store::CozoGraphShuttle>,
    binding: OnceLock<PinnedBinding>,
    bootstrap: tokio::sync::Mutex<()>,
    bound: tokio::sync::Notify,
}

/// Browser Accept connections use the same kernel and committed-volume proof as lungs,
/// but never publish a D-Bus binding or select the configured host identity.
pub(crate) struct BrowserWaiterIdentity {
    host: Arc<ProcessIdentity>,
    catalog_dir: PathBuf,
    state_dir: String,
    identity_store: Arc<op_cozo_store::CozoGraphShuttle>,
}

impl BrowserWaiterIdentity {
    pub(crate) fn new(catalog_dir: PathBuf, state_dir: String,
        identity_store: Arc<op_cozo_store::CozoGraphShuttle>) -> Result<Self> {
        Ok(Self { host: Arc::new(ProcessIdentity::current()?), catalog_dir, state_dir, identity_store })
    }
}

#[async_trait::async_trait]
impl op_tunnel_mux::WaiterIdentity<String> for BrowserWaiterIdentity {
    async fn identify(&self, stream: &tokio::net::UnixStream) -> Result<op_tunnel_mux::WaiterProof<String>> {
        let peer = LungsIdentity::capture(stream, self.host.clone(), self.catalog_dir.clone(),
            self.state_dir.clone(), self.identity_store.clone())?;
        let binding = tokio::time::timeout(NEGOTIATION_TIMEOUT, peer.current_binding(true))
            .await.context("browser peer identification timed out")??;
        let container = require_browser_container(binding)?;
        peer.revalidate_peer()?;
        Ok(op_tunnel_mux::WaiterProof { key: container.session_id,
            revalidate: Box::new(move || peer.revalidate_peer().is_ok() && container.init.revalidate().is_ok()) })
    }
}

fn require_browser_container(binding: Binding) -> Result<ContainerBinding> {
    match binding {
        Binding::Host => anyhow::bail!("host process has no container binding"),
        Binding::Container(container) => Ok(container),
    }
}

fn host_binding(cmdline: &[u8]) -> Result<Binding> {
    ensure!(!is_incus_forkproxy(cmdline), "host process has no container binding");
    Ok(Binding::Host)
}

fn committed_binding(candidate: Option<ContainerBinding>) -> Result<Binding> {
    candidate.map(Binding::Container).context("peer has no committed live identity container")
}

impl LungsIdentity {
    pub(super) fn capture(
        stream: &tokio::net::UnixStream,
        host: Arc<ProcessIdentity>,
        catalog_dir: PathBuf,
        state_dir: String,
        identity_store: Arc<op_cozo_store::CozoGraphShuttle>,
    ) -> Result<Arc<Self>> {
        // Capture the kernel's original-peer lifetime before any /proc lookup.
        let peer_lifetime = PeerLifetime::from_socket(stream)?;
        let credentials = stream.peer_cred()?;
        let pid = credentials.pid().and_then(|pid| u32::try_from(pid).ok())
            .context("socket peer PID unavailable")?;
        let peer = peer_lifetime.read_process(pid)?;
        Ok(Arc::new(Self {
            peer_uid: credentials.uid(), peer, peer_lifetime, host, catalog_dir, state_dir, identity_store,
            binding: OnceLock::new(), bootstrap: tokio::sync::Mutex::new(()),
            bound: tokio::sync::Notify::new(),
        }))
    }

    fn host_root(&self) -> bool {
        self.peer_uid == 0 && self.peer.same_namespaces(&self.host)
    }

    fn revalidate_peer(&self) -> Result<()> { self.peer_lifetime.revalidate(&self.peer) }

    async fn sessions(&self) -> Result<Vec<ContainerIdentitySled>> {
        let catalog_dir = self.catalog_dir.clone();
        let state_dir = self.state_dir.clone();
        tokio::task::spawn_blocking(move || {
            crate::identity_receiver::committed_sessions_in(&catalog_dir, &state_dir)
        }).await?
    }

    async fn authoritative_identity(&self, session_id: &str) -> Result<VerifiedSid1> {
        let catalog_dir = self.catalog_dir.clone();
        let state_dir = self.state_dir.clone();
        let session_id = session_id.to_owned();
        let bound = tokio::task::spawn_blocking(move || {
            crate::identity_receiver::bound_session_for_id_in(
                &catalog_dir, &state_dir, &session_id, chrono::Utc::now().timestamp(),
            )
        }).await??;
        crate::identity_receiver::verify_bound_registry(bound).await
    }

    async fn container_binding(&self, sled: &ContainerIdentitySled) -> Result<ContainerBinding> {
        self.revalidate_peer()?;
        ensure!(sled.active && sled.is_anchored(), "identity is not active");
        ensure!(sled.expires_at.is_none_or(|expiry| expiry == 0 || expiry > chrono::Utc::now().timestamp()),
            "identity has expired");
        let stored = sled.instance.as_ref().context("identity container unavailable")?;
        ensure!(crate::identity_binding::identity_instance_matches(sled, stored),
            "identity container does not match");
        let native_uuid = instance_uuid(stored)?;
        let pin = |pid| {
            let init = ProcessIdentity::read(pid)?;
            ensure!(self.peer.same_namespaces(&init), "peer is outside identity container");
            self.revalidate_peer()?;
            init.revalidate()?;
            Ok(init)
        };
        let init = if sled.org_relation.as_ref().is_some_and(|r|
            r.kind == op_plugins::state_plugins::identity_sled::OrgRelationKind::Subsidiary)
        {
            crate::runtime_identity_owner::lxd_subsidiary::pin_browser_init(
                &self.identity_store, sled, pin,
            ).await?
        } else {
            let native = IncusRootRepairBackend::local();
            let (metadata, _) = native.instance(&stored.name).await?;
            let current = IncusPlugin::instance_from_native_metadata(metadata)?;
            ensure!(current.name == stored.name && current.instance_type == "container"
                && current.status == "Running" && current.project.as_deref() == Some("default")
                && current.location.as_deref() == Some("none")
                && instance_uuid(&current)? == native_uuid
                && crate::identity_binding::identity_instance_matches(sled, &current),
                "native identity container does not match");
            let pid = native.instance_init_pid(&stored.name).await?;
            let init = pin(pid)?;
            ensure!(native.instance_init_pid(&stored.name).await? == pid, "identity container restarted");
            init
        };
        init.revalidate()?;
        self.revalidate_peer()?;
        Ok(ContainerBinding {
            session_id: sled.session_id.clone(),
            session_genesis: sled.genesis.clone().context("identity anchor unavailable")?,
            native_name: stored.name.clone(), native_uuid, init,
        })
    }

    pub(super) async fn bind(&self, session_id: &str) -> Result<()> {
        tokio::time::timeout(NEGOTIATION_TIMEOUT, async {
            let _bootstrap = self.bootstrap.lock().await;
            self.revalidate_peer()?;
            ensure!(self.host_root(), "host binding requires root in both host namespaces");
            let session = uuid::Uuid::parse_str(session_id)?;
            ensure!(session.to_string() == session_id, "session id is not canonical");
            ensure!(self.binding.get().is_none(), "lungs connection is already bound");
            // An LXD subsidiary is projected by its host mount: index its
            // sled volume before binding, as guest binding does for Incus.
            if self.host_root() && crate::runtime_identity_owner::lxd_subsidiary::adopt(
                self.identity_store.clone(), session_id).await? {
                crate::runtime_identity::recover_startup(self.identity_store.clone(), self.catalog_dir.clone()).await?;
                self.revalidate_peer()?;
            }
            self.publish_binding(Binding::Host, session_id).await
        }).await.context("host binding timed out")?
    }

    pub(super) async fn bind_current_container(&self) -> Result<()> {
        tokio::time::timeout(NEGOTIATION_TIMEOUT, async {
            let _bootstrap = self.bootstrap.lock().await;
            ensure!(self.binding.get().is_none(), "lungs connection is already bound");
            let binding = self.current_binding(false).await?;
            let session_id = match &binding {
                Binding::Host => host_peer_session(self.host_root(), |name| std::env::var(name).ok())?,
                Binding::Container(container) => container.session_id.clone(),
            };
            self.publish_binding(binding, &session_id).await
        }).await.context("container binding timed out")?
    }

    async fn current_binding(&self, browser: bool) -> Result<Binding> {
        self.revalidate_peer()?;
        if self.peer.pid_namespace == self.host.pid_namespace {
            let cmdline = std::fs::read(format!("/proc/{}/cmdline", self.peer.pid))
                .context("host peer command line unavailable")?;
            self.revalidate_peer()?;
            return host_binding(&cmdline);
        }
        let mut candidate = self.find_container_binding().await?;
        if candidate.is_none() && !browser {
            // The sled volume is authoritative: adopt unindexed volumes once.
            let adopted = crate::runtime_identity_owner::adopt_identity_volumes(self.identity_store.clone()).await?;
            if adopted > 0 {
                crate::runtime_identity::recover_startup(self.identity_store.clone(), self.catalog_dir.clone()).await?;
                self.revalidate_peer()?;
                candidate = self.find_container_binding().await?;
            }
        }
        committed_binding(candidate)
    }

    async fn find_container_binding(&self) -> Result<Option<ContainerBinding>> {
        let sessions = self.sessions().await?;
        let mut candidate = None;
        for sled in sessions.iter().filter(|sled| sled.active && sled.is_anchored()) {
            if let Ok(binding) = self.container_binding(sled).await {
                ensure!(candidate.is_none(), "peer has no unique identity container");
                candidate = Some(binding);
            }
        }
        Ok(candidate)
    }

    async fn publish_binding(&self, target: Binding, session_id: &str) -> Result<()> {
        let identity = self.authoritative_identity(session_id).await?;
        self.revalidate_peer()?;
        if let Binding::Container(binding) = &target {
            ensure!(binding.session_genesis == identity.claims().session_genesis,
                "identity anchor changed during binding");
        }
        self.binding.set(PinnedBinding { target, identity: RwLock::new(identity) })
            .map_err(|_| anyhow::anyhow!("lungs connection is already bound"))?;
        self.bound.notify_one();
        Ok(())
    }

    pub(super) async fn wait_bound(&self) -> Result<()> {
        tokio::time::timeout(NEGOTIATION_TIMEOUT, async {
            while self.binding.get().is_none() { self.bound.notified().await; }
        }).await.context("lungs identity bootstrap timed out")
    }

    pub(super) async fn caller(self: &Arc<Self>) -> Result<VerifiedCaller> {
        let pinned = self.binding.get().context("lungs identity is not bound")?;
        let identity = pinned.identity.read().map_err(|_| anyhow::anyhow!("bound identity poisoned"))?.clone();
        let authority = Arc::new(LungsAuthority { peer: self.clone(), identity: identity.clone() });
        authority.revalidate().await?;
        Ok(VerifiedCaller::new(&identity, authority))
    }

    async fn revalidate_native_binding(&self) -> Result<()> {
        let pinned = self.binding.get().context("lungs identity is not bound")?;
        self.revalidate_peer()?;
        match &pinned.target {
            Binding::Host => ensure!(self.host_root(), "host peer changed"),
            Binding::Container(binding) => {
                let sled = self.sessions().await?.into_iter()
                    .find(|sled| sled.session_id == binding.session_id)
                    .context("identity container disappeared")?;
                let current = self.container_binding(&sled).await?;
                ensure!(*binding == current, "identity container binding changed");
            }
        }
        self.revalidate_peer()
    }

    async fn revalidate_binding(&self, expected: &VerifiedSid1) -> Result<()> {
        let pinned = self.binding.get().context("lungs identity is not bound")?;
        ensure!(pinned.identity.read().map_err(|_| anyhow::anyhow!("bound identity poisoned"))?.claims()
            == expected.claims(), "bound caller authority changed");
        self.revalidate_native_binding().await?;
        let current = self.authoritative_identity(&expected.claims().session_id).await?;
        ensure!(current.claims() == expected.claims(), "bound identity term changed");
        self.revalidate_peer()
    }

    async fn accept_refresh(&self, refresh: &crate::runtime_identity::CredentialRefresh) -> Result<()> {
        let pinned = self.binding.get().context("lungs identity is not bound")?;
        ensure!(pinned.identity.read().map_err(|_| anyhow::anyhow!("bound identity poisoned"))?.claims()
            == refresh.previous(), "refresh original native authority differs");
        // Refresh never skips the pidfd, namespace, host-root or exact native
        // container proof established for this same accepted connection.
        self.revalidate_native_binding().await?;
        let current = self.authoritative_identity(&refresh.previous().session_id).await?;
        ensure!(current.claims() == refresh.identity().claims(), "native refresh replacement changed");
        self.revalidate_peer()?;
        let mut identity = pinned.identity.write().map_err(|_| anyhow::anyhow!("bound identity poisoned"))?;
        ensure!(identity.claims() == refresh.previous(), "native identity changed during refresh");
        *identity = current;
        Ok(())
    }
}

struct LungsAuthority {
    peer: Arc<LungsIdentity>,
    identity: VerifiedSid1,
}

#[async_trait::async_trait]
impl CallerAuthority for LungsAuthority {
    async fn revalidate(&self) -> Result<()> {
        tokio::time::timeout(NEGOTIATION_TIMEOUT, self.peer.revalidate_binding(&self.identity))
            .await.context("native caller revalidation timed out")?
    }

    async fn refreshed(self: Arc<Self>, refresh: &crate::runtime_identity::CredentialRefresh)
        -> Result<Arc<dyn CallerAuthority>> {
        ensure!(self.identity.claims() == refresh.previous(), "refresh original lungs authority differs");
        tokio::time::timeout(NEGOTIATION_TIMEOUT, self.peer.accept_refresh(refresh))
            .await.context("native refresh revalidation timed out")??;
        Ok(Arc::new(Self { peer: self.peer.clone(), identity: refresh.identity().clone() }))
    }
}

/// Host identity selectors already configured for this bridge, in order.
const HOST_SESSION_SELECTORS: [&str; 2] = ["OP_LOCAL_HUMAN_SESSION_ID", "IDENTITY_SLED_HOST_SESSION_ID"];

/// A host-namespace peer calling `BindCurrentContainer` is bound by the
/// kernel proof it already has: only host root, and only to the configured
/// host identity. Anyone else in the host namespaces stays refused, and the
/// selected session must still resolve to a committed, registered identity.
fn host_peer_session(host_root: bool, lookup: impl Fn(&str) -> Option<String>) -> Result<String> {
    ensure!(host_root, "host process has no container binding");
    let session_id = HOST_SESSION_SELECTORS.iter()
        .find_map(|name| lookup(name).map(|value| value.trim().to_owned()).filter(|value| !value.is_empty()))
        .context("host root has no configured host identity")?;
    ensure!(uuid::Uuid::parse_str(&session_id).is_ok_and(|uuid| uuid.to_string() == session_id),
        "configured host identity is not canonical");
    Ok(session_id)
}

/// `incusd forkproxy ...` (or LXD's) relays guest connections as host root.
fn is_incus_forkproxy(cmdline: &[u8]) -> bool {
    let mut args = cmdline.split(|byte| *byte == 0).filter(|arg| !arg.is_empty());
    let daemon = args.next().and_then(|arg| arg.rsplit(|byte| *byte == b'/').next())
        .is_some_and(|name| matches!(name, b"incusd" | b"incus" | b"lxd"));
    daemon && args.next() == Some(b"forkproxy".as_slice())
}

fn instance_uuid(instance: &IncusInstance) -> Result<String> {
    let value = instance.config.as_ref().and_then(|config| config.get("volatile.uuid"))
        .context("native instance UUID unavailable")?;
    ensure!(uuid::Uuid::parse_str(value).is_ok_and(|uuid| uuid.to_string() == *value),
        "native instance UUID is invalid");
    Ok(value.clone())
}

pub(super) fn require_peer_pidfd() -> Result<()> {
    let (socket, _peer) = std::os::unix::net::UnixStream::pair()?;
    PeerLifetime::from_socket(&socket)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_waiter_refuses_host_peers() {
        assert!(host_binding(b"/usr/bin/host-client\0").and_then(require_browser_container).is_err());
    }

    #[test]
    fn browser_waiter_refuses_host_forkproxy() {
        assert!(host_binding(b"/usr/bin/lxd\0forkproxy\0").is_err());
    }

    #[test]
    fn browser_waiter_refuses_unmatched_container_peer() {
        assert!(committed_binding(None).and_then(require_browser_container).is_err());
    }

    #[test]
    fn process_names_cannot_shift_the_start_time_field() {
        let fields = (3..=21).map(|value| value.to_string()).collect::<Vec<_>>().join(" ");
        assert_eq!(parse_start_ticks(&format!("123 (a ) process) {fields} 987 23")), Some(987));
        assert_eq!(parse_start_ticks("123 (short) 1 2"), None);
        assert_eq!(parse_start_ticks("invalid"), None);
    }

    #[test]
    fn host_peers_reuse_their_kernel_binding_and_never_gain_an_identity() {
        let configured = "414fbc56-03fb-20ab-758a-bcc7a7cbdc0e";
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| pairs.iter().find(|(key, _)| *key == name).map(|(_, value)| value.to_string())
        };
        // Non-root host processes keep the original refusal.
        assert!(host_peer_session(false, env(&[("OP_LOCAL_HUMAN_SESSION_ID", "414fbc56-03fb-20ab-758a-bcc7a7cbdc0e")]))
            .unwrap_err().to_string().contains("host process has no container binding"));
        // Host root without a configured host identity has nothing to bind.
        assert!(host_peer_session(true, env(&[])).is_err());
        assert!(host_peer_session(true, env(&[("OP_LOCAL_HUMAN_SESSION_ID", "  ")])).is_err());
        // Selectors must be canonical sessions, never paths or names.
        for bad in ["../escape", "human", "414FBC56-03FB-20AB-758A-BCC7A7CBDC0E", "414fbc5603fb20ab758abcc7a7cbdc0e"] {
            let lookup = move |name: &str| (name == "OP_LOCAL_HUMAN_SESSION_ID").then(|| bad.to_owned());
            assert!(host_peer_session(true, lookup).is_err(), "{bad}");
        }
        assert_eq!(host_peer_session(true, env(&[("OP_LOCAL_HUMAN_SESSION_ID", "414fbc56-03fb-20ab-758a-bcc7a7cbdc0e")])).unwrap(), configured);
        assert_eq!(host_peer_session(true, env(&[("IDENTITY_SLED_HOST_SESSION_ID", " 414fbc56-03fb-20ab-758a-bcc7a7cbdc0e\n")])).unwrap(), configured);
        // The local human selector wins over the legacy sled selector.
        assert_eq!(host_peer_session(true, env(&[
            ("IDENTITY_SLED_HOST_SESSION_ID", "b0c777e3-0955-cd8b-3d01-ca12998cae28"),
            ("OP_LOCAL_HUMAN_SESSION_ID", "414fbc56-03fb-20ab-758a-bcc7a7cbdc0e"),
        ])).unwrap(), configured);
    }

    #[test]
    fn incus_proxy_relays_are_not_the_host_identity() {
        assert!(is_incus_forkproxy(b"/usr/libexec/incus/incusd\0forkproxy\0--\0123\0"));
        assert!(is_incus_forkproxy(b"incusd\0forkproxy\0"));
        assert!(is_incus_forkproxy(b"/snap/lxd/current/bin/lxd\0forkproxy\0"));
        assert!(!is_incus_forkproxy(b"/usr/local/libexec/op-dbus/bin/3tched-fs\0mount\0"));
        assert!(!is_incus_forkproxy(b"/usr/bin/forkproxy\0incusd\0"));
        assert!(!is_incus_forkproxy(b"incusd\0"));
        assert!(!is_incus_forkproxy(b""));
    }

    #[test]
    fn namespace_identity_requires_device_and_inode() {
        let handle = Arc::new(File::open("/dev/null").unwrap());
        let namespace = NamespaceIdentity { dev: 1, ino: 2, _handle: handle };
        assert_eq!(namespace, namespace.clone());
        assert_ne!(namespace, NamespaceIdentity { dev: 3, ..namespace.clone() });
        assert_ne!(namespace, NamespaceIdentity { ino: 3, ..namespace.clone() });
    }
}
