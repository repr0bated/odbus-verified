//! Fresh bridge binary: serves `StateSync`, `PluginService` and
//! `EventChainService` (plus reflection) via [`op_grpc_bridge::serve`].
//!
//! Listen address: `OP_GRPC_BRIDGE_LISTEN`, else `OP_DBUS_GRPC_LISTEN`,
//! else `0.0.0.0:50051`. Catalog source: `OP_BRIDGE_CATALOG` (path to a
//! catalog JSON file), else an empty catalog.

use std::net::SocketAddr;

use op_grpc_bridge::core::BridgeCore;
use op_grpc_bridge::Catalog;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let addr: SocketAddr = std::env::var("OP_GRPC_BRIDGE_LISTEN")
        .or_else(|_| std::env::var("OP_DBUS_GRPC_LISTEN"))
        .unwrap_or_else(|_| "0.0.0.0:50051".to_string())
        .parse()?;

    let catalog = match std::env::var("OP_BRIDGE_CATALOG") {
        Ok(path) => Catalog::from_json_str(&std::fs::read_to_string(&path)?)?,
        Err(_) => Catalog::empty(),
    };

    tracing::info!(%addr, plugins = catalog.plugins().len(), "op-grpc-bridge starting");
    op_grpc_bridge::serve(addr, BridgeCore::new(catalog)).await
}
