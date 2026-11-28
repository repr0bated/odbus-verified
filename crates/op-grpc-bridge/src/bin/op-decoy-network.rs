//! Explicit decoy-only binary. Normal bridge startup never calls this path.

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt().try_init();
    op_grpc_bridge::run_decoy().await
}
