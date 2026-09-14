use op_grpc_bridge::grpc_client::{GrpcClientPool, RemoteOperationClient};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pool = Arc::new(GrpcClientPool::new());
    let client = RemoteOperationClient::new(pool, "http://127.0.0.1:8090", "test-tool");
    let result = client
        .call_method(
            "rovs_commands",
            "",
            "",
            "list_dbs",
            vec![simd_json::json!({})],
            "test-tool",
            "cap.network.ovsdb.db.list@v1",
        )
        .await?;
    println!("{}", simd_json::to_string_pretty(&result)?);
    Ok(())
}
