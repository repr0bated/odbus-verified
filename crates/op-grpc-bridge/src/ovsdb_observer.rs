//! Commit complete native topology at the owning plugin's canonical root.
//! Intermediate monitor changes may coalesce: each value replaces the entire
//! observed tree, including removed bridge/port/interface descendants.

use crate::{
    method_contract_admission::admit_compiled_state_contract,
    mutation_engine::MutationEngine,
    sealed_schema_reader::{blob_catalog_dir_from_env, read_manifest_pinned_schema_document},
};
use anyhow::{ensure, Context, Result};
use op_network::rovs_proxy::{
    monitor::{MonitorObservation, OvsdbSnapshot},
    OvsdbDbusClient,
};
use std::{path::Path, sync::Arc};

const PLUGIN: &str = "ovsdb_bridge";

/// Root owns task lifetime; dropping this future drops the watch receiver and
/// terminates its dedicated native monitor connection. No reconnect/poll loop.
pub(crate) async fn observe(engine: Arc<MutationEngine>, client: OvsdbDbusClient) -> Result<()> {
    let catalog = blob_catalog_dir_from_env();
    // A compiled observer grants neither plugin existence nor startup state.
    let pinned = read_manifest_pinned_schema_document(&catalog, PLUGIN)?;
    admit_compiled_state_contract(&pinned, PLUGIN)?;
    let mut receiver = client.monitor_db("Open_vSwitch").await?;
    loop {
        let observation = receiver.borrow_and_update().clone();
        match observation {
            MonitorObservation::Snapshot(snapshot) => {
                match prepare_observation(&catalog, &snapshot) {
                    Ok(prepared) => {
                        // Root replacement preserves the schema's array tree;
                        // raw table deltas and legacy /ovsdb paths never enter
                        // SHM. Audit carries only these selected topology data.
                        let bridges = prepared.state["bridges"].clone();
                        if let Err(error) = engine
                            .process_observed_child(
                                PLUGIN,
                                format!("{}/bridges", op_plugins::canonical::plugin_path(PLUGIN)),
                                bridges,
                                prepared.expected_schema_hash,
                            )
                            .await
                        {
                            tracing::warn!(%error, "Committed OVS topology publication failed");
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "Native OVS topology rejected; committed state retained")
                    }
                }
            }
            MonitorObservation::Unavailable(reason) => {
                anyhow::bail!("OVSDB topology observation ended: {reason}");
            }
        }
        receiver
            .changed()
            .await
            .context("OVSDB topology monitor closed")?;
    }
}

pub(crate) struct PreparedObservation {
    pub state: simd_json::OwnedValue,
    pub expected_schema_hash: String,
}

fn prepare_observation(catalog: &Path, snapshot: &OvsdbSnapshot) -> Result<PreparedObservation> {
    let pinned = read_manifest_pinned_schema_document(catalog, PLUGIN)?;
    admit_compiled_state_contract(&pinned, PLUGIN)?;
    let topology = op_plugins::state_plugins::ovsdb_bridge::project_topology(snapshot)?;
    let state = simd_json::serde::to_owned_value(&topology)?;
    ensure!(
        pinned.schema.validate(&state).valid,
        "native OVS topology does not conform to the pinned plugin state schema"
    );
    pinned.schema.validate_public_state_policy()?;
    // Validate the configured public projection now. Publication still commits
    // selected raw topology; readers apply this exact seal's exposure policy.
    pinned.schema.project_public_state(&state)?;
    let after = read_manifest_pinned_schema_document(catalog, PLUGIN)?;
    ensure!(
        after.blob.schema_json == pinned.blob.schema_json
            && after.schema_hash == pinned.schema_hash,
        "OVS plugin seal changed during observation validation"
    );
    Ok(PreparedObservation {
        state,
        expected_schema_hash: pinned.schema_hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_state::StatePlugin;
    use std::collections::BTreeMap;

    fn empty() -> OvsdbSnapshot {
        OvsdbSnapshot {
            sequence: 1,
            tables: ["Open_vSwitch", "Bridge", "Port", "Interface"]
                .into_iter()
                .map(|name| (name.to_owned(), BTreeMap::new()))
                .collect(),
        }
    }
    #[test]
    fn only_present_compatible_seals_allow_complete_topology_publication() {
        let directory = tempfile::tempdir().unwrap();
        assert!(prepare_observation(directory.path(), &empty()).is_err());
        let mut store = op_blob::BlobStore::open(directory.path()).unwrap();
        let schema = op_plugins::state_plugins::ovsdb_bridge::OvsBridgePlugin::new()
            .schema()
            .unwrap();
        store
            .write(&op_blob::blobify_plugin_schema(PLUGIN, schema.clone()))
            .unwrap();
        let prepared = prepare_observation(directory.path(), &empty()).unwrap();
        assert_eq!(prepared.state, simd_json::json!({"bridges":[]}));
        assert_eq!(prepared.expected_schema_hash.len(), 64);
        let mut drifted = schema.clone();
        let bridges = drifted.fields.get_mut("bridges").unwrap();
        bridges.required = !bridges.required;
        assert!(drifted.validate(&prepared.state).valid);
        store
            .write(&op_blob::blobify_plugin_schema(PLUGIN, drifted))
            .unwrap();
        assert!(prepare_observation(directory.path(), &empty()).is_err());
        let mut stale = schema.clone();
        stale.version = "1.0.0".into();
        store
            .write(&op_blob::blobify_plugin_schema(PLUGIN, stale))
            .unwrap();
        assert!(prepare_observation(directory.path(), &empty()).is_err());
        store
            .write(&op_blob::blobify_plugin_schema(PLUGIN, schema))
            .unwrap();
        let mut incomplete = empty();
        incomplete.tables.remove("Port");
        assert!(prepare_observation(directory.path(), &incomplete).is_err());
    }
}
