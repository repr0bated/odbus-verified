//! op-assistant-grpc — gRPC gateway for the self-hosted Assistant.
//!
//! Routes gRPC calls through the on-host operation.v1 gRPC server served by
//! `op-dbus`. The Assistant listener itself defaults to loopback. Tonic is
//! `10.0.0.2` on `rtly-lungs`. There is no `svc0` port and no `10.200.0.0/24`
//! fabric.

use anyhow::Result;
use op_assistant_grpc::{ServerConfig, run_grpc_server};
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("op_assistant_grpc=info,info")
            }),
        )
        .init();

    let cfg = ServerConfig::default();
    info!(
        host = %cfg.host,
        port = cfg.port,
        endpoint = %cfg.transport.rpc_endpoint,
        "op-assistant-grpc starting"
    );

    run_grpc_server(cfg).await
}
