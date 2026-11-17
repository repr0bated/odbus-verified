//! `op-tunnel-mux [--egress PATH] [--inbound-only PATH] [--door ADDR]`
//!
//! Egress socket (default /run/opdbus/tunnel/egress/tunnel.sock): Open and
//! Accept, for identity containers. Inbound-only socket (default
//! /run/opdbus/tunnel/inbound/tunnel.sock): Accept only, for the
//! control-plane chatbot. With --door, connections arriving on ADDR are
//! spliced to the waiting container.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

fn bind_unix(path: &Path) -> Result<tokio::net::UnixListener> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err).with_context(|| format!("remove stale {}", path.display())),
    }
    tokio::net::UnixListener::bind(path).with_context(|| format!("bind {}", path.display()))
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();
    let mut egress = PathBuf::from("/run/opdbus/tunnel/egress/tunnel.sock");
    let mut inbound_only = PathBuf::from("/run/opdbus/tunnel/inbound/tunnel.sock");
    let mut door = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--egress" => egress = args.next().context("--egress needs a path")?.into(),
            "--inbound-only" => inbound_only = args.next().context("--inbound-only needs a path")?.into(),
            "--door" => door = Some(args.next().context("--door needs an address")?),
            other => anyhow::bail!("unknown argument {other}"),
        }
    }
    let egress_unix = bind_unix(&egress)?;
    let only_unix = bind_unix(&inbound_only)?;
    let door = match door {
        Some(addr) => Some(
            tokio::net::TcpListener::bind(&addr).await.with_context(|| format!("bind {addr}"))?,
        ),
        None => None,
    };
    tracing::info!(egress = %egress.display(), inbound_only = %inbound_only.display(), "tunnel mux serving");
    op_tunnel_mux::serve_both(egress_unix, Some(only_unix), door, op_tunnel_mux::Mux::new()).await
}
