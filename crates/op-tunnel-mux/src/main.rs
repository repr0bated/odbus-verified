//! `op-tunnel-mux [--socket PATH] [--door ADDR]`
//!
//! Serves the inbound door tunnel on PATH (default
//! /run/opdbus/tunnel/tunnel.sock), bound only into the door-owning
//! container. With --door, connections arriving on ADDR are spliced to it.

use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

use anyhow::{Context, Result};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();
    let mut socket = PathBuf::from("/run/opdbus/tunnel/tunnel.sock");
    let mut door = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => socket = args.next().context("--socket needs a path")?.into(),
            "--door" => door = Some(args.next().context("--door needs an address")?),
            other => anyhow::bail!("unknown argument {other}"),
        }
    }
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    match std::fs::remove_file(&socket) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err).with_context(|| format!("remove stale {}", socket.display())),
    }
    let unix = tokio::net::UnixListener::bind(&socket)
        .with_context(|| format!("bind {}", socket.display()))?;
    // The container's root maps to an unprivileged host uid. Only the
    // door-owning container is given this directory; the peer credential
    // names it.
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o666))?;
    let door = match door {
        Some(addr) => Some(
            tokio::net::TcpListener::bind(&addr).await.with_context(|| format!("bind {addr}"))?,
        ),
        None => None,
    };
    tracing::info!(socket = %socket.display(), "door tunnel serving");
    op_tunnel_mux::serve(unix, door, op_tunnel_mux::Mux::new()).await
}
