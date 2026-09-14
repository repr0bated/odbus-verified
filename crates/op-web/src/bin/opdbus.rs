//! op-dbus: gRPC server for all Operation services
//!
//! Binds to the ovsbr0 interface IPv4 address (discovered at runtime via
//! rtnetlink) so all Incus containers can reach it through the single OVS
//! choke point. No hardcoded IP — the address follows whatever ovsbr0 is
//! assigned.
//!
//! Listen address override: `OP_DBUS_GRPC_LISTEN` (e.g. "0.0.0.0:50051" for
//! dev, or an explicit `ip:port`). When unset, op-dbus resolves the first
//! IPv4 on `OP_DBUS_GRPC_IFACE` (default `ovsbr0`) and binds `<that-ip>:50051`;
//! if the interface has no IPv4 it falls back to `0.0.0.0:50051`.

use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, FmtSubscriber};

use op_grpc_bridge::run_grpc_server;
use op_grpc_bridge::MutationEngine;

use op_network::rovs_proxy::OvsdbDbusClient;
use op_network::rtnetlink::list_interfaces;
use op_state_store::{ChainConfig, EventChain};
use tokio::sync::RwLock;

/// Default port when no explicit listen address is supplied.
const DEFAULT_GRPC_PORT: u16 = 50051;
/// Interface whose IPv4 is used for the default bind address.
const DEFAULT_GRPC_IFACE: &str = "ovsbr0";

/// Resolve the bind address: explicit `OP_DBUS_GRPC_LISTEN` wins; otherwise
/// discover the first IPv4 on the configured interface and append the port;
/// otherwise bind all interfaces.
async fn resolve_listen_addr() -> SocketAddr {
    if let Ok(listen) = std::env::var("OP_DBUS_GRPC_LISTEN") {
        if let Ok(addr) = listen.parse::<SocketAddr>() {
            return addr;
        }
        warn!(%listen, "OP_DBUS_GRPC_LISTEN did not parse as SocketAddr; falling back to discovery");
    }

    let port: u16 = std::env::var("OP_DBUS_GRPC_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_GRPC_PORT);
    let iface =
        std::env::var("OP_DBUS_GRPC_IFACE").unwrap_or_else(|_| DEFAULT_GRPC_IFACE.to_string());

    if let Ok(interfaces) = list_interfaces().await {
        if let Some(ni) = interfaces.iter().find(|i| i.name == iface) {
            if let Some(ip) = ni
                .addresses
                .iter()
                .find(|a| a.family == "AF_INET" || a.address.parse::<std::net::Ipv4Addr>().is_ok())
                .and_then(|a| a.address.split('/').next().map(|s| s.to_string()))
            {
                if let Ok(addr) = format!("{}:{}", ip, port).parse::<SocketAddr>() {
                    info!(iface = %iface, %ip, port, "discovered ovsbr0 bind address");
                    return addr;
                }
            }
        }
    }

    warn!(iface = %iface, "no IPv4 on interface; binding 0.0.0.0");
    format!("0.0.0.0:{}", port)
        .parse()
        .expect("0.0.0.0:port always parses")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = FmtSubscriber::builder()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .try_init();

    let addr = resolve_listen_addr().await;

    let chain = Arc::new(RwLock::new(EventChain::new(ChainConfig::default())));
    let ovsdb = Arc::new(OvsdbDbusClient::new());
    let engine = Arc::new(MutationEngine::new(chain, ovsdb));

    info!(addr = %addr, "op-dbus starting");
    run_grpc_server(addr, engine, None, None).await?;

    Ok(())
}
