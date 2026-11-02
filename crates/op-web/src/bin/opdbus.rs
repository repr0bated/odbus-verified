//! op-dbus: gRPC server for all Operation services
//!
//! Binds to the ovsbr0 interface IPv4 address (discovered at runtime via
//! rtnetlink) so all Incus containers can reach it through the single OVS
//! choke point. No hardcoded IP — the address follows whatever ovsbr0 is
//! assigned.
//!
//! Listen address override: `OP_DBUS_GRPC_LISTEN` (e.g. "0.0.0.0:50051" for
//! dev, or an explicit `ip:port`). When unset, op-dbus resolves the first
//! IPv4 on `OP_DBUS_GRPC_IFACE` (default `ovsbr0`) and binds `<that-ip>:50051`.
//! Missing interface addressing is fatal unless the operator supplies an
//! explicit listen address.

use anyhow::Context;
use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use op_grpc_bridge::core::BridgeCore;
use op_grpc_bridge::Catalog;

use op_network::rtnetlink::list_interfaces;

/// Default port when no explicit listen address is supplied.
const DEFAULT_GRPC_PORT: u16 = 50051;
/// Interface whose IPv4 is used for the default bind address.
const DEFAULT_GRPC_IFACE: &str = "ovsbr0";

/// Resolve the bind address: explicit `OP_DBUS_GRPC_LISTEN` wins; otherwise
/// discover the first IPv4 on the configured interface and append the port;
/// otherwise fail closed instead of unexpectedly exposing every interface.
async fn resolve_listen_addr() -> anyhow::Result<SocketAddr> {
    if let Ok(listen) = std::env::var("OP_DBUS_GRPC_LISTEN") {
        return listen
            .parse::<SocketAddr>()
            .with_context(|| format!("invalid OP_DBUS_GRPC_LISTEN value: {listen}"));
    }

    let port: u16 = std::env::var("OP_DBUS_GRPC_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_GRPC_PORT);
    let iface =
        std::env::var("OP_DBUS_GRPC_IFACE").unwrap_or_else(|_| DEFAULT_GRPC_IFACE.to_string());

    if let Ok(interfaces) = list_interfaces().await
        && let Some(ni) = interfaces.iter().find(|i| i.name == iface)
        && let Some(ip) = ni
            .addresses
            .iter()
            .find(|a| a.family == "AF_INET" || a.address.parse::<std::net::Ipv4Addr>().is_ok())
            .and_then(|a| a.address.split('/').next().map(|s| s.to_string()))
        && let Ok(addr) = format!("{}:{}", ip, port).parse::<SocketAddr>()
    {
        info!(iface = %iface, %ip, port, "discovered ovsbr0 bind address");
        return Ok(addr);
    }

    anyhow::bail!(
        "no IPv4 address found on {iface}; set OP_DBUS_GRPC_LISTEN explicitly if another bind is intended"
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = FmtSubscriber::builder()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .try_init();

    let addr = resolve_listen_addr().await?;

    let catalog = match std::env::var("OP_BRIDGE_CATALOG") {
        Ok(path) => Catalog::from_json_str(&std::fs::read_to_string(&path)?)?,
        Err(_) => Catalog::empty(),
    };

    info!(addr = %addr, "op-dbus starting");
    op_grpc_bridge::serve(addr, BridgeCore::new(catalog)).await?;

    Ok(())
}
