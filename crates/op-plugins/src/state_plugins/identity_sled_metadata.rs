//! The local identity metadata view, derived from the plugin-owned record.
//!
//! This preserves the scalar fields consumed by op_identity::SessionIdentity.
//! It is NOT the generic wire exposure policy: StateSync/SSE must still apply
//! the exact sealed PluginSchema.public_state policy (currently withheld).
//! The full private identity tree, including SID1 and events, is unchanged.
use super::ContainerIdentitySled;
use anyhow::{ensure, Context, Result};
use simd_json::prelude::ValueAsContainer;
use simd_json::{json, OwnedValue};

/// Build the minimal local metadata document paired with the private tree.
/// Explicit field construction prevents future record additions, arbitrary
/// Incus config/device values and event content from becoming public by default.
/// No credential is authored, reconstructed, rehashed or returned here.
pub fn identity_metadata_projection(state: &OwnedValue) -> Result<OwnedValue> {
    let object = state
        .as_object()
        .context("identity metadata root must be an object")?;
    let sleds = object
        .get("sleds")
        .context("identity metadata requires sleds")?;
    ensure!(
        sleds.as_array().is_some(),
        "identity metadata sleds must be an array"
    );
    // Root events and unrelated fields are deliberately not deserialized.
    // Their validation belongs to the durable producer, not this metadata view.
    let typed: Vec<ContainerIdentitySled> = simd_json::serde::from_owned_value(sleds.clone())?;
    let mut rows = Vec::with_capacity(typed.len());
    for sled in typed {
        ensure!(
            sled.principal_kind
                .as_deref()
                .is_none_or(|kind| matches!(kind, "human" | "service")),
            "identity metadata has an invalid actor class"
        );
        rows.push(json!({
            "session_id": sled.session_id,
            "wireguard_pubkey": sled.wireguard_pubkey,
            "principal_kind": sled.principal_kind,
            "mutation_index": sled.mutation_index,
            "genesis": sled.genesis,
            "trace_id": sled.trace_id,
            "schema_version": sled.schema_version,
            "active": sled.active,
            "expires_at": sled.expires_at,
            "arrival_timestamp": sled.arrival_timestamp,
            "chain_head_at_arrival": sled.chain_head_at_arrival,
            "catalog_hash_at_arrival": sled.catalog_hash_at_arrival,
            "head_timestamp_at_arrival": sled.head_timestamp_at_arrival
        }));
    }
    Ok(json!({"sleds":rows}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use simd_json::prelude::*;

    #[test]
    fn only_existing_scalar_metadata_survives_opaque_fields_and_future_additions() {
        let state = json!({"sleds":[{
            "session_id":"session","wireguard_pubkey":"key","principal_kind":"human",
            "mutation_index":u64::MAX,"genesis":"anchor","trace_id":"trace","schema_version":3,
            "active":false,"expires_at":123,"arrival_timestamp":1,
            "chain_head_at_arrival":"head","catalog_hash_at_arrival":"catalog","head_timestamp_at_arrival":0,
            "sealed_id":"sid1:private","peer_ip":"private-ip","interface":"private-interface",
            "instance":{"name":"session","status":"Stopped","type":"container",
                "config":{"secret":"private-config"},"future":"private-child"},
            "future":"private-new-field"
        }],"events":[{"content":"private-event"}],"future":"private-root"});
        let before = state.clone();
        let public = identity_metadata_projection(&state).unwrap();
        assert_eq!(state, before, "projection never edits the private tree");
        assert_eq!(public.as_object().unwrap().len(), 1);
        let row = &public["sleds"][0];
        assert_eq!(row.as_object().unwrap().len(), 13);
        assert_eq!(row["mutation_index"].as_u64(), Some(u64::MAX));
        assert_eq!(row["principal_kind"], "human");
        assert_eq!(row["active"], false);
        assert_eq!(row["genesis"], "anchor");
        assert!(!simd_json::to_string(&public).unwrap().contains("private"));
    }

    #[test]
    fn missing_malformed_or_unknown_class_inputs_fail_instead_of_defaulting_to_empty() {
        for state in [
            json!({}),
            json!({"sleds":{}}),
            json!({"sleds":[{"session_id":"s"}]}),
            json!({"sleds":[{"session_id":"s","wireguard_pubkey":"k","principal_kind":"wireguard-principal"}]}),
        ] {
            assert!(identity_metadata_projection(&state).is_err());
        }
        assert_eq!(
            identity_metadata_projection(&json!({"sleds":[]})).unwrap(),
            json!({"sleds":[]})
        );
        assert_eq!(
            identity_metadata_projection(&json!({"sleds":[],"events":{"unparsed":"private-root"}}))
                .unwrap(),
            json!({"sleds":[]})
        );
    }

    #[test]
    fn legacy_genesis_alias_is_normalized_without_creating_a_credential() {
        let public = identity_metadata_projection(&json!({"sleds":[{
            "session_id":"s","wireguard_pubkey":"k","hashed_footprint":"legacy-anchor",
            "principal_kind":"service","sealed_id":"sid1:private"
        }]}))
        .unwrap();
        assert_eq!(public["sleds"][0]["genesis"], "legacy-anchor");
        assert!(public["sleds"][0].get("sealed_id").is_none());
        assert!(public["sleds"][0].get("hashed_footprint").is_none());
    }
}
