//! Look up a real identity sled by WireGuard pubkey via the canonical
//! identity_sled plugin (PluginService.CallMethod), the same canonical path
//! used by identity updates after container provisioning.
use op_grpc_bridge::grpc_client::{GrpcClientPool, RemoteOperationClient};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pubkey = std::env::args()
        .nth(1)
        .expect("usage: get_identity <wireguard_pubkey>");
    let session_id = op_identity::session::derive_session_id(&pubkey);

    let pool = Arc::new(GrpcClientPool::new());
    let client = RemoteOperationClient::new(pool, "http://127.0.0.1:8090", "get-identity-tool");

    let args = simd_json::json!({ "session_id": session_id });
    let result = client
        .call_method(
            "identity_sled",
            "",
            "",
            "get_identity",
            vec![args],
            "get-identity-tool",
            "identity_sled.read",
        )
        .await?;

    println!("{}", simd_json::to_string_pretty(&result)?);
    Ok(())
}
