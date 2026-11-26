//! The door tunnel owner: the host bridge runs `op-tunnel-mux` and serves the
//! sealed `tunnel` plugin. The host door (OP_TUNNEL_DOOR, default
//! 10.0.0.2:8090) is spliced over the shared door socket (OP_TUNNEL_SOCKET,
//! default /run/opdbus/tunnel/tunnel.sock) into the door-owning container's
//! own tonic door, where TLS ends. Methods carry only status and open/close;
//! bytes never enter state.
//!
//! Only the host bridge owns the tunnel: it starts only when OP_TUNNEL_DOOR is
//! set explicitly (deploy/runit/op-grpc-bridge/run). A container bridge leaves
//! it unset; its door is its own TLS listener on lo, reached through
//! `op-tunnel-door`, and it must never rebind the shared door socket.
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{Context, Result};
use op_plugins::state_plugins::tunnel::{
    CloseConnectionsInput, CloseConnectionsOutput, Door, SetOpenInput, TunnelState,
};
use op_tunnel_mux::{BrowserMux, ListenerMeta, Mux, Selector};
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};

use crate::{
    core::{BackendStateUpdate, StateScope},
    runtime::{BackendContext, BackendOutcome, RuntimeError},
};

struct Owner {
    mux: Mux,
    door: String,
    socket: String,
}

static OWNER: OnceLock<Owner> = OnceLock::new();
struct BrowserOwner {
    mux: BrowserMux<String>,
    host: String,
}
static BROWSER_OWNER: OnceLock<BrowserOwner> = OnceLock::new();

pub(crate) fn browser_host_from_env() -> Option<String> {
    tunnel_door(std::env::var("OP_BROWSER_DOOR").ok())
}

pub(crate) fn supports(plugin: &str, method: &str) -> bool {
    plugin == "tunnel" && matches!(method, "status" | "set_open" | "close_connections")
}

/// The host door this bridge splices, when it owns the tunnel. Only an
/// explicit, non-empty OP_TUNNEL_DOOR makes this bridge the tunnel owner.
fn tunnel_door(value: Option<String>) -> Option<String> {
    value
        .map(|door| door.trim().to_string())
        .filter(|door| !door.is_empty())
}

/// Whether this bridge owns the door tunnel (host only).
pub(crate) fn owns_tunnel_from_env() -> bool {
    tunnel_door(std::env::var("OP_TUNNEL_DOOR").ok()).is_some()
}

/// Bind the door socket and the host door, then serve until stopped.
pub(crate) async fn serve_from_env() -> Result<()> {
    let socket = PathBuf::from(
        std::env::var("OP_TUNNEL_SOCKET")
            .unwrap_or_else(|_| "/run/opdbus/tunnel/tunnel.sock".into()),
    );
    let door = tunnel_door(std::env::var("OP_TUNNEL_DOOR").ok())
        .context("door tunnel owner requires an explicit OP_TUNNEL_DOOR")?;
    let unix = bind_socket(&socket)?;
    let listener = tokio::net::TcpListener::bind(&door)
        .await
        .with_context(|| format!("bind host door {door}"))?;
    let mux = Mux::new();
    OWNER
        .set(Owner {
            mux: mux.clone(),
            door: door.clone(),
            socket: socket.display().to_string(),
        })
        .map_err(|_| anyhow::anyhow!("door tunnel owner started twice"))?;
    tracing::info!(%door, socket = %socket.display(), "door tunnel serving");
    op_tunnel_mux::serve(unix, Some(listener), mux).await
}

fn bind_socket(socket: &std::path::Path) -> Result<tokio::net::UnixListener> {
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    match std::fs::symlink_metadata(&socket) {
        Ok(meta) if std::os::unix::fs::FileTypeExt::is_socket(&meta.file_type()) => {
            std::fs::remove_file(&socket)?
        }
        Ok(_) => anyhow::bail!("{} exists and is not a socket", socket.display()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    let unix = tokio::net::UnixListener::bind(&socket)
        .with_context(|| format!("bind door socket {}", socket.display()))?;
    // Only the door-owning container is given this directory; container roots
    // are unprivileged host uids. The peer credential names the caller.
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o666))?;
    Ok(unix)
}

const VOLUME_POOLS: &[&str] = &[
    "/var/lib/incus/storage-pools/3tched-pool/custom",
    "/var/lib/lxd/storage-pools/lxd-pool/custom",
];

// Only committed, active identities are inputs. Files left by a removed
// container are not authority, and a duplicate port has no unique target.
fn assigned_ports(
    ids: impl Iterator<Item = String>,
    pools: &[&str],
) -> Result<HashMap<u16, String>> {
    let mut ports = HashMap::new();
    let mut duplicates = HashSet::new();
    for id in ids {
        if std::path::Path::new(&id).file_name() != Some(std::ffi::OsStr::new(&id)) {
            continue;
        }
        let name = format!("default_identity-{id}");
        let mut volume = None;
        for pool in pools {
            let candidate = std::path::Path::new(pool).join(&name);
            if candidate.try_exists()? {
                anyhow::ensure!(volume.is_none(), "ambiguous identity volume");
                volume = Some(candidate);
            }
        }
        let Some(volume) = volume else {
            continue;
        };
        let value = match std::fs::read_to_string(volume.join("browser_port")) {
            Ok(value) => value,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err.into()),
        };
        anyhow::ensure!(
            std::fs::read_to_string(volume.join("session_id"))?.trim() == id,
            "volume identity mismatch"
        );
        let port: u16 = value.trim().parse()?;
        anyhow::ensure!(
            (20000..30000).contains(&port),
            "invalid browser port assignment"
        );
        if ports.insert(port, id).is_some() {
            duplicates.insert(port);
        }
    }
    for port in duplicates {
        ports.remove(&port);
    }
    Ok(ports)
}

struct PortSelector {
    catalog: PathBuf,
    state: String,
}

impl PortSelector {
    async fn assignments(self: &Arc<Self>) -> Result<HashMap<u16, String>> {
        let source = self.clone();
        tokio::task::spawn_blocking(move || source.read_assignments()).await?
    }

    fn read_assignments(&self) -> Result<HashMap<u16, String>> {
        let records = crate::identity_receiver::committed_sessions_in(&self.catalog, &self.state)?;
        let now = chrono::Utc::now().timestamp();
        let sessions = records
            .into_iter()
            .filter(|record| {
                record.active
                    && record.is_anchored()
                    && record
                        .expires_at
                        .is_none_or(|expiry| expiry == 0 || expiry > now)
            })
            .map(|record| record.session_id);
        assigned_ports(sessions, VOLUME_POOLS)
    }
}

struct BrowserSelector(Arc<PortSelector>);

#[tonic::async_trait]
impl Selector<String> for BrowserSelector {
    async fn select(&self, listener: &ListenerMeta, _: &TcpStream) -> Option<String> {
        // Check the committed binding at accept, even between reconciliation ticks.
        self.0.assignments().await.ok()?.remove(&listener.port)
    }
}

fn prepare_browser_directory(socket: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    use std::path::Component;
    let parent = socket
        .parent()
        .context("browser socket requires a parent")?;
    anyhow::ensure!(
        parent.is_absolute() && parent.parent().is_some() && socket.file_name().is_some(),
        "browser socket requires its own absolute directory"
    );
    let mut path = PathBuf::new();
    for component in parent.components() {
        anyhow::ensure!(
            matches!(component, Component::RootDir | Component::Normal(_)),
            "invalid browser directory component"
        );
        path.push(component);
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&path)?;
                std::fs::symlink_metadata(&path)?
            }
            Err(err) => return Err(err.into()),
        };
        anyhow::ensure!(
            meta.is_dir() && meta.uid() == 0 && meta.mode() & 0o022 == 0,
            "browser socket requires root-owned non-writable directory ancestors"
        );
    }
    std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

pub(crate) async fn serve_browser_from_env(
    runtime: Arc<crate::runtime::Runtime>,
    host: String,
) -> Result<()> {
    // One or more host addresses, comma-separated (for example the bridge
    // address and the WARP address): every identity port is bound on each.
    let ips: Vec<IpAddr> = host
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            value
                .parse()
                .context("OP_BROWSER_DOOR must list host IP addresses")
        })
        .collect::<Result<_>>()?;
    anyhow::ensure!(!ips.is_empty(), "OP_BROWSER_DOOR lists no address");
    let socket = PathBuf::from(
        std::env::var_os("OP_BROWSER_SOCKET")
            .unwrap_or_else(|| "/run/opdbus/tunnel-browser/browser.sock".into()),
    );
    let control = PathBuf::from(
        std::env::var_os("OP_TUNNEL_SOCKET")
            .unwrap_or_else(|| "/run/opdbus/tunnel/tunnel.sock".into()),
    );
    anyhow::ensure!(
        socket.parent() != control.parent(),
        "browser socket requires a separate directory"
    );
    let catalog = runtime.catalog_dir().to_owned();
    let state = op_core::projection_shm::shm_state_dir();
    let identity = crate::runtime_lungs::BrowserWaiterIdentity::new(
        catalog.clone(),
        state.clone(),
        runtime.identity_store().clone(),
    )?;
    let records = Arc::new(PortSelector { catalog, state });
    let selector = Arc::new(BrowserSelector(records.clone()));
    prepare_browser_directory(&socket)?;
    let unix = bind_socket(&socket)?;
    let mux = BrowserMux::new();
    BROWSER_OWNER
        .set(BrowserOwner {
            mux: mux.clone(),
            host,
        })
        .map_err(|_| anyhow::anyhow!("browser tunnel owner started twice"))?;
    let socket_server = op_tunnel_mux::serve_browser(unix, mux.clone(), Arc::new(identity));
    tokio::pin!(socket_server);
    let mut workers = tokio::task::JoinSet::new();
    let mut listening: HashMap<SocketAddr, tokio::task::AbortHandle> = HashMap::new();
    let mut refused: HashSet<SocketAddr> = HashSet::new();
    let mut tick = tokio::time::interval(Duration::from_secs(2));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            result = &mut socket_server => return result,
            _ = workers.join_next(), if !workers.is_empty() => {},
            _ = tick.tick() => {
                let ports = match records.assignments().await {
                    Ok(ports) => ports,
                    Err(_) => {
                        tracing::warn!("browser assignments unavailable; refusing connections");
                        HashMap::new()
                    }
                };
                listening.retain(|addr, task| {
                    if !ports.contains_key(&addr.port()) { task.abort(); return false; }
                    !task.is_finished()
                });
                refused.retain(|addr| ports.contains_key(&addr.port()));
                for port in ports.into_keys() {
                    for ip in &ips {
                        let addr = SocketAddr::new(*ip, port);
                        if listening.contains_key(&addr) { continue; }
                        match TcpListener::bind(addr).await {
                            Ok(listener) => {
                                refused.remove(&addr);
                                let task = workers.spawn(op_tunnel_mux::serve_browser_listener(listener, mux.clone(), selector.clone()));
                                listening.insert(addr, task);
                            }
                            Err(error) => if refused.insert(addr) {
                                tracing::warn!(%addr, %error, "browser listener bind refused");
                            },
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod port_tests {
    use super::assigned_ports;

    #[test]
    fn only_current_unambiguous_committed_ports_are_selected() {
        let dir = tempfile::tempdir().unwrap();
        let pools = [dir.path().to_str().unwrap()];
        let snapshot =
            |ids: &[&str]| assigned_ports(ids.iter().map(|id| id.to_string()), &pools).unwrap();
        for (id, port) in [("first", "20001\n"), ("second", "20002\n")] {
            let volume = dir.path().join(format!("default_identity-{id}"));
            std::fs::create_dir(&volume).unwrap();
            std::fs::write(volume.join("session_id"), id).unwrap();
            std::fs::write(volume.join("browser_port"), port).unwrap();
        }
        let both = snapshot(&["first", "second"]);
        assert_eq!(both.get(&20001).map(String::as_str), Some("first"));
        assert!(!both.contains_key(&20003));
        // Provisioning file survives removal; the authoritative record does not.
        let removed = snapshot(&["second"]);
        assert!(!removed.contains_key(&20001));
        assert_eq!(removed.get(&20002).map(String::as_str), Some("second"));
        std::fs::write(
            dir.path().join("default_identity-second/browser_port"),
            "20001\n",
        )
        .unwrap();
        assert!(!snapshot(&["first", "second"]).contains_key(&20001));
    }
}

async fn observe() -> TunnelState {
    let mut state = TunnelState::default();
    if let Some(owner) = OWNER.get() {
        let status = owner.mux.status().await;
        state.door = owner.door.clone();
        state.socket = owner.socket.clone();
        state.open = status.open;
        state.waiting_uids = status.waiting_uids;
        state.waiting = count(status.waiting);
        state.active = count(status.active);
        state.oldest_active_since = status.oldest_active_since;
    }
    if let Some(owner) = BROWSER_OWNER.get() {
        let status = owner.mux.status().await;
        state.browser_door = Some(owner.host.clone());
        state.browser_open = status.open;
        state.browser_waiting = count(status.waiting);
        state.browser_active = count(status.active);
    }
    state
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

pub(crate) async fn dispatch(
    ctx: &BackendContext<'_>,
    method: &str,
    args: &Value,
) -> Result<BackendOutcome, RuntimeError> {
    let unavailable =
        || RuntimeError::unsupported_before_effect("requested tunnel owner is not running");
    if OWNER.get().is_none() && BROWSER_OWNER.get().is_none() {
        return Err(unavailable());
    }
    crate::runtime_misc::revalidate(ctx).await?;
    let closed = match method {
        "status" => None,
        "set_open" => {
            let input: SetOpenInput = serde_json::from_value(args.clone())
                .map_err(|error| RuntimeError::before_effect(error.into()))?;
            match input.door {
                Door::Control => OWNER
                    .get()
                    .ok_or_else(unavailable)?
                    .mux
                    .set_open(input.open),
                Door::Browser => BROWSER_OWNER
                    .get()
                    .ok_or_else(unavailable)?
                    .mux
                    .set_open(input.open),
            };
            None
        }
        "close_connections" => {
            let input: CloseConnectionsInput = serde_json::from_value(args.clone())
                .map_err(|error| RuntimeError::before_effect(error.into()))?;
            Some(count(match input.door {
                Door::Control => OWNER.get().ok_or_else(unavailable)?.mux.close_active(),
                Door::Browser => BROWSER_OWNER
                    .get()
                    .ok_or_else(unavailable)?
                    .mux
                    .close_active(),
            }))
        }
        _ => {
            return Err(RuntimeError::unsupported_before_effect(
                "tunnel method has no owner",
            ));
        }
    };
    let snapshot = observe().await;
    let state = serde_json::to_value(&snapshot)?;
    let result = match closed {
        Some(closed) => serde_json::to_value(CloseConnectionsOutput {
            closed,
            state: snapshot,
        })?,
        None => state.clone(),
    };
    Ok(BackendOutcome {
        result,
        state: BackendStateUpdate::Replace(state),
        // The door is host-wide, not one identity's private state.
        scope: StateScope::Public,
        additional: Vec::new(),
        guard: None,
    })
}

#[cfg(test)]
mod tests {
    use super::tunnel_door;

    #[test]
    fn only_an_explicit_door_owns_the_tunnel() {
        assert_eq!(tunnel_door(None), None);
        assert_eq!(tunnel_door(Some(String::new())), None);
        assert_eq!(tunnel_door(Some("  ".into())), None);
        assert_eq!(
            tunnel_door(Some("10.0.0.2:8090".into())).as_deref(),
            Some("10.0.0.2:8090")
        );
        assert_eq!(
            tunnel_door(Some(" 10.0.0.2:8090\n".into())).as_deref(),
            Some("10.0.0.2:8090")
        );
    }
}
