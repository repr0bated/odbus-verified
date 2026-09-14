//! Decoy-only native network owner. No host bridge startup path invokes this.
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();
    op_grpc_bridge::decoy_network::run_decoy().await
}
