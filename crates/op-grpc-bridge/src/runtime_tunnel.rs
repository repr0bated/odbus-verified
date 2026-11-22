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
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use op_plugins::state_plugins::tunnel::{CloseConnectionsOutput, SetOpenInput, TunnelState};
use op_tunnel_mux::Mux;
use serde_json::Value;

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

pub(crate) fn supports(plugin: &str, method: &str) -> bool {
    plugin == "tunnel" && matches!(method, "status" | "set_open" | "close_connections")
}

/// The host door this bridge splices, when it owns the tunnel. Only an
/// explicit, non-empty OP_TUNNEL_DOOR makes this bridge the tunnel owner.
fn tunnel_door(value: Option<String>) -> Option<String> {
    value.map(|door| door.trim().to_string()).filter(|door| !door.is_empty())
}

/// Whether this bridge owns the door tunnel (host only).
pub(crate) fn owns_tunnel_from_env() -> bool {
    tunnel_door(std::env::var("OP_TUNNEL_DOOR").ok()).is_some()
}

/// Bind the door socket and the host door, then serve until stopped.
pub(crate) async fn serve_from_env() -> Result<()> {
    let socket = PathBuf::from(
        std::env::var("OP_TUNNEL_SOCKET").unwrap_or_else(|_| "/run/opdbus/tunnel/tunnel.sock".into()),
    );
    let door = tunnel_door(std::env::var("OP_TUNNEL_DOOR").ok())
        .context("door tunnel owner requires an explicit OP_TUNNEL_DOOR")?;
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
    let listener = tokio::net::TcpListener::bind(&door)
        .await
        .with_context(|| format!("bind host door {door}"))?;
    let mux = Mux::new();
    OWNER
        .set(Owner { mux: mux.clone(), door: door.clone(), socket: socket.display().to_string() })
        .map_err(|_| anyhow::anyhow!("door tunnel owner started twice"))?;
    tracing::info!(%door, socket = %socket.display(), "door tunnel serving");
    op_tunnel_mux::serve(unix, Some(listener), mux).await
}

async fn observe(owner: &Owner) -> TunnelState {
    let status = owner.mux.status().await;
    TunnelState {
        door: owner.door.clone(),
        socket: owner.socket.clone(),
        open: status.open,
        waiting_uids: status.waiting_uids,
        waiting: u32::try_from(status.waiting).unwrap_or(u32::MAX),
        active: u32::try_from(status.active).unwrap_or(u32::MAX),
        oldest_active_since: status.oldest_active_since,
    }
}

pub(crate) async fn dispatch(
    ctx: &BackendContext<'_>, method: &str, args: &Value,
) -> Result<BackendOutcome, RuntimeError> {
    let owner = OWNER
        .get()
        .ok_or_else(|| RuntimeError::unsupported_before_effect("door tunnel owner is not running"))?;
    crate::runtime_misc::revalidate(ctx).await?;
    let result = match method {
        "status" => serde_json::to_value(observe(owner).await)?,
        "set_open" => {
            let input: SetOpenInput = serde_json::from_value(args.clone())
                .map_err(|error| RuntimeError::before_effect(error.into()))?;
            owner.mux.set_open(input.open);
            serde_json::to_value(observe(owner).await)?
        }
        "close_connections" => {
            let closed = owner.mux.close_active();
            serde_json::to_value(CloseConnectionsOutput {
                closed: u32::try_from(closed).unwrap_or(u32::MAX),
                state: observe(owner).await,
            })?
        }
        _ => return Err(RuntimeError::unsupported_before_effect("tunnel method has no owner")),
    };
    let state = serde_json::to_value(observe(owner).await)?;
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
        assert_eq!(tunnel_door(Some("10.0.0.2:8090".into())).as_deref(), Some("10.0.0.2:8090"));
        assert_eq!(tunnel_door(Some(" 10.0.0.2:8090\n".into())).as_deref(), Some("10.0.0.2:8090"));
    }
}
