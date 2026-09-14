//! Real PluginService.CallMethod("xray", "get_stats", ...) round-trip test.
use op_grpc_bridge::grpc_client::{GrpcClientPool, RemoteOperationClient};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pool = Arc::new(GrpcClientPool::new());
    let client = RemoteOperationClient::new(pool, "http://127.0.0.1:8090", "xray-stats-tool");

    let args = simd_json::json!({ "name": "inbound>>>qdrant-http>>>traffic>>>uplink" });
    let result = client
        .call_method(
            "xray",
            "",
            "",
            "get_stats",
            vec![args],
            "xray-stats-tool",
            "xray.read",
        )
        .await?;

    println!("{}", simd_json::to_string_pretty(&result)?);
    Ok(())
}
