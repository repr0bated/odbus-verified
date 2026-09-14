//! Plugin state client — routes reads/writes through the gRPC MutationEngine
//! (the single write door) instead of the excised D-Bus StateManager.
//!
//! Reads go to `RemoteOperationClient::get_state` (state_cache, the mutation
//! fold). Writes go to `RemoteOperationClient::set_state` (ApplyPatch through
//! the MutationEngine → EventChain → shm projection). No second write door,
//! no StateManager, no drift.

use anyhow::{Context, Result};
use op_grpc_bridge::{GhostbridgeCallMetadata, GrpcClientPool, RemoteOperationClient};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::sync::{Arc, OnceLock};

static GRPC_CLIENT: OnceLock<RemoteOperationClient> = OnceLock::new();

/// Lazily initialize a singleton RemoteOperationClient backed by the
/// MutationEngine's gRPC endpoint (env `OP_DBUS_GRPC_ADDR`, default
/// `https://10.0.0.3:8090` — the fabric/TCP TLS-only tonic door; trust anchor
/// via `OP_DBUS_GRPC_CA_FILE`, see GrpcClientPool::configure_endpoint).
fn client() -> &'static RemoteOperationClient {
    GRPC_CLIENT.get_or_init(|| {
        let grpc_addr = std::env::var("OP_DBUS_GRPC_ADDR")
            .or_else(|_| std::env::var("FABRIC_GRPC_ADDR"))
            .or_else(|_| std::env::var("OP_FABRIC_GRPC_ADDR"))
            .unwrap_or_else(|_| "https://10.0.0.3:8090".to_string());
        let pool = Arc::new(GrpcClientPool::new());
        RemoteOperationClient::new(pool, &grpc_addr, "op-web")
    })
}

pub async fn query_plugin_state<T>(plugin_id: &str) -> Result<Option<T>>
where
    T: DeserializeOwned,
{
    match client().get_state(plugin_id, "").await {
        Ok(state) => match simd_json::serde::from_owned_value::<T>(state) {
            Ok(value) => Ok(Some(value)),
            Err(_) => Ok(None),
        },
        Err(_) => Ok(None),
    }
}

pub async fn apply_plugin_state<T>(plugin_id: &str, value: &T) -> Result<()>
where
    T: Serialize,
{
    let state = simd_json::serde::to_owned_value(value)
        .with_context(|| format!("serialize {} plugin state", plugin_id))?;
    client()
        .set_state(plugin_id, "", state, "op-web", "")
        .await
        .map_err(|e| anyhow::anyhow!("apply {} state via gRPC: {}", plugin_id, e))?;
    Ok(())
}

/// Dispatch a schema-declared plugin method through the bridge while
/// preserving the identity supplied by the outer HTTP request.
pub async fn call_plugin_method(
    plugin_id: &str,
    method: &str,
    arguments: simd_json::OwnedValue,
    capability_id: &str,
    identity: &GhostbridgeCallMetadata,
) -> Result<simd_json::OwnedValue> {
    client()
        .call_method_with_metadata(
            plugin_id,
            &format!("/org/opdbus/v1/plugins/{plugin_id}"),
            "org.opdbus.v1.PluginV1",
            method,
            vec![arguments],
            "op-web",
            capability_id,
            identity,
        )
        .await
        .map_err(|e| anyhow::anyhow!("call {plugin_id}.{method} through bridge: {e}"))
}
