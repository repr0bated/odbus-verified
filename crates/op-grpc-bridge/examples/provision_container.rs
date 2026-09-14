//! Provision a real NIC-less identity container via the real
//! PluginService.CallMethod gRPC path (identity_sled.provision_container) —
//! creates the Incus container, then commits its identity record through the
//! durable Cozo/protected-SHM transaction owner. A typed pending error means
//! the native effect may require recovery before retrying.
use op_grpc_bridge::grpc_client::{GrpcClientPool, RemoteOperationClient};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pubkey = std::env::args()
        .nth(1)
        .expect("usage: provision_container <wireguard_pubkey>");

    let pool = Arc::new(GrpcClientPool::new());
    let client =
        RemoteOperationClient::new(pool, "http://127.0.0.1:8090", "provision-container-tool");

    let args = simd_json::json!({
        "wireguard_pubkey": pubkey,
        "instance": {
            "image": "raccoon",
            "profiles": ["identity"]
        }
    });

    let result = client
        .call_method(
            "identity_sled",
            "",
            "",
            "provision_container",
            vec![args],
            "provision-container-tool",
            "identity_sled.write",
        )
        .await?;

    println!("{}", simd_json::to_string_pretty(&result)?);
    Ok(())
}
