//! SID1 receiver over the committed private identity projection.
//! This does not admit an RPC, evaluate grants, or handle OIA1 assertions.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use op_identity::sealed_id::SealedId;
use op_plugins::state_plugins::identity_sled::{ContainerIdentitySled, IdentitySledState};
use op_state_store::FieldSchema;
use serde::Deserialize;

use crate::catalog::Catalog;

#[derive(Deserialize)]
struct CompiledIdentityContract {
    version: String,
    fields: HashMap<String, FieldSchema>,
}

fn pinned_identity(dir: &Path) -> anyhow::Result<crate::catalog::PluginEntry> {
    let pinned = Catalog::sealed_plugin(dir, "identity_sled")?;
    let schema: op_state_store::PluginSchema = serde_json::from_value(
        pinned
            .schema
            .clone()
            .ok_or_else(|| anyhow::anyhow!("sealed identity schema absent"))?,
    )?;
    let compiled: CompiledIdentityContract = serde_json::from_str(include_str!(concat!(
        env!("OUT_DIR"),
        "/identity_state_contract.json"
    )))?;
    anyhow::ensure!(
        schema.version == compiled.version && schema.fields == compiled.fields,
        "compiled and sealed identity state contracts differ"
    );
    Ok(pinned)
}

fn committed_session(
    catalog_dir: &Path,
    state_dir: &str,
    session_id: &str,
) -> anyhow::Result<Option<ContainerIdentitySled>> {
    let before = pinned_identity(catalog_dir)?;
    let bytes =
        op_core::projection_shm::read_credential_projection_bytes_in(state_dir, "identity_sled")?
            .ok_or_else(|| anyhow::anyhow!("committed identity credential projection is absent"))?;
    let after = pinned_identity(catalog_dir)?;
    anyhow::ensure!(
        before.schema_hash == after.schema_hash
            && before.sealed_schema_json == after.sealed_schema_json,
        "identity plugin seal changed during credential state access"
    );
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        value.get("sleds").is_some_and(serde_json::Value::is_array),
        "identity sleds missing"
    );
    anyhow::ensure!(
        value.get("events").is_some_and(serde_json::Value::is_array),
        "identity events missing"
    );
    let state: IdentitySledState = serde_json::from_value(value)?;
    let mut ids = HashSet::new();
    for sled in &state.sleds {
        anyhow::ensure!(
            ids.insert(sled.session_id.as_str()),
            "duplicate identity in SHM"
        );
        crate::identity_binding::validate_sled(sled)?;
    }
    let mut events = HashSet::new();
    for event in &state.events {
        anyhow::ensure!(
            events.insert((event.session_id.as_str(), event.seq)),
            "duplicate identity event in SHM"
        );
    }
    Ok(state
        .sleds
        .into_iter()
        .find(|sled| sled.session_id == session_id))
}

fn verify_in(
    catalog_dir: &Path,
    state_dir: &str,
    wire: &[u8],
    transport: &str,
    now: i64,
) -> anyhow::Result<SealedId> {
    let claims =
        crate::sid1_checks::parse_for_transport(wire, transport).map_err(anyhow::Error::msg)?;
    let sled = committed_session(catalog_dir, state_dir, &claims.session_id)?
        .ok_or_else(|| anyhow::anyhow!("sealed identity session was not found"))?;
    crate::sid1_checks::match_bound_session(wire, &sled, transport, now).map_err(anyhow::Error::msg)
}

/// Verify SID1 using the sealed identity contract and committed private SHM.
/// Callers must still check the grant for each requested method and revalidate
/// long-lived subscriptions as appropriate.
pub async fn verify_sid1(wire: &[u8], transport: &str) -> anyhow::Result<SealedId> {
    let catalog_dir = std::env::var_os("OP_BLOB_CATALOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(op_blob::catalog::DEFAULT_SHM_DIR));
    let state_dir = op_core::projection_shm::shm_state_dir();
    let wire = wire.to_vec();
    let transport = transport.to_owned();
    tokio::task::spawn_blocking(move || {
        verify_in(
            &catalog_dir,
            &state_dir,
            &wire,
            &transport,
            chrono::Utc::now().timestamp(),
        )
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use op_state::StatePlugin;

    #[test]
    fn receiver_requires_pinned_contract_and_committed_bound_identity() {
        let dir = tempfile::tempdir().unwrap();
        let catalog_dir = dir.path().join("catalog");
        let state_dir = dir.path().join("state");
        let state_dir = state_dir.to_str().unwrap();
        let key = "fixture-key".to_string();
        let claims = SealedId {
            principal_id: op_identity::session::derive_principal_id(&key),
            principal_kind: "wireguard-principal".into(),
            session_id: op_identity::session::derive_session_id(&key),
            wireguard_pubkey: key,
            session_genesis: "genesis".into(),
            trace_id: "trace".into(),
            schema_version: 1,
            issued_at: 100,
            expires_at: 200,
            arrival_timestamp: 100,
            chain_head_at_arrival: "chain".into(),
            catalog_hash_at_arrival: "catalog".into(),
            head_timestamp_at_arrival: 99,
            transport_scope: "grpc".into(),
            nextdns: None,
        };
        let wire = claims.seal().unwrap();
        assert!(verify_in(&catalog_dir, state_dir, &wire, "grpc", 150).is_err());
        let mut schema = op_plugins::state_plugins::identity_sled::IdentitySledPlugin::new()
            .schema()
            .unwrap();
        let mut catalog = op_blob::BlobStore::open(&catalog_dir).unwrap();
        catalog
            .write(&op_blob::blobify_plugin_schema(
                "identity_sled",
                schema.clone(),
            ))
            .unwrap();
        assert!(verify_in(&catalog_dir, state_dir, &wire, "grpc", 150).is_err());
        let mut state = serde_json::json!({"sleds": [{
            "session_id": claims.session_id, "wireguard_pubkey": claims.wireguard_pubkey,
            "principal_kind": "human", "genesis": claims.session_genesis, "trace_id": claims.trace_id,
            "schema_version": claims.schema_version, "expires_at": claims.expires_at,
            "arrival_timestamp": claims.arrival_timestamp, "chain_head_at_arrival": claims.chain_head_at_arrival,
            "catalog_hash_at_arrival": claims.catalog_hash_at_arrival, "head_timestamp_at_arrival": claims.head_timestamp_at_arrival,
            "sealed_id": format!("{}{}", op_identity::sealed_id::INLINE_PREFIX,
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&wire))
        }], "events": []});
        op_core::projection_shm::update_projection_with_credentials_in(
            state_dir,
            "identity_sled",
            nix::unistd::getgid().as_raw(),
            |_| {
                Ok(Some((
                    br#"{"sleds":[],"events":[]}"#.to_vec(),
                    serde_json::to_vec(&state)?,
                )))
            },
        )
        .unwrap();
        assert!(verify_in(&catalog_dir, state_dir, &wire, "grpc", 150).is_ok());
        assert!(verify_in(&catalog_dir, state_dir, &wire, "grpc", 200).is_err());
        assert!(verify_in(&catalog_dir, state_dir, &wire, "mcp", 150).is_err());
        state["sleds"][0]["principal_kind"] = serde_json::json!("service");
        op_core::projection_shm::update_projection_with_credentials_in(
            state_dir,
            "identity_sled",
            nix::unistd::getgid().as_raw(),
            |_| {
                Ok(Some((
                    br#"{"sleds":[],"events":[]}"#.to_vec(),
                    serde_json::to_vec(&state)?,
                )))
            },
        )
        .unwrap();
        assert!(verify_in(&catalog_dir, state_dir, &wire, "grpc", 150).is_err());
        state["sleds"][0]["principal_kind"] = serde_json::json!("human");
        op_core::projection_shm::update_projection_with_credentials_in(
            state_dir,
            "identity_sled",
            nix::unistd::getgid().as_raw(),
            |_| {
                Ok(Some((
                    br#"{"sleds":[],"events":[]}"#.to_vec(),
                    serde_json::to_vec(&state)?,
                )))
            },
        )
        .unwrap();
        assert!(verify_in(&catalog_dir, state_dir, &wire, "grpc", 150).is_ok());
        schema.version.push_str("-drift");
        catalog
            .write(&op_blob::blobify_plugin_schema("identity_sled", schema))
            .unwrap();
        assert!(verify_in(&catalog_dir, state_dir, &wire, "grpc", 150).is_err());
    }
}
