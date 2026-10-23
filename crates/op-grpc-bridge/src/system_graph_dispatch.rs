//! A system inventory is derived from committed public SHM, never method
//! arguments, an event payload, private memory or an independent host scan.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, ensure};
use op_cozo_store::{GraphProvenance, GraphProvenanceKind, GraphSnapshot, GraphSnapshotReceipt};

use crate::system_graph_projection::{GraphPluginProjection, project_system_graph};

const MAX_CATALOG_PLUGINS: usize = 512;
const MAX_STATE_BYTES: u64 = 32 * 1024 * 1024;

// Only subsystem metadata is needed. Private conversation/identity stores are
// deliberately absent; declared schemas still establish their public methods.
const STATE_PLUGINS: &[&str] = &[
    "incus",
    "btrfs",
    "qdrant",
    "mail_server",
    "cognitive",
    "memory",
    "embedding_model",
    "oscal_subid_registry",
    "json_render",
    "model_registry",
    "unix_socket",
    "shared_unix_socket",
    "ovsdb_bridge",
];

pub(crate) fn committed_system_graph(catalog_dir: &Path, state_dir: &str) -> Result<GraphSnapshot> {
    let before = crate::sealed_schema_reader::manifest_plugins(catalog_dir)?;
    ensure!(
        before.len() <= MAX_CATALOG_PLUGINS,
        "system graph catalog exceeds plugin limit"
    );
    let catalog = crate::sealed_schema_reader::read_manifest_pinned_catalog(catalog_dir)?;
    let selected: Vec<String> = STATE_PLUGINS
        .iter()
        .filter(|id| catalog.plugins.contains_key(**id))
        .map(|id| (*id).to_owned())
        .collect();
    let mut snapshot = op_core::projection_shm::read_projection_snapshot_bounded_in(
        state_dir,
        &selected,
        MAX_STATE_BYTES,
    )?;
    let mut plugins = BTreeMap::new();
    for (id, document) in catalog.plugins {
        let state = snapshot
            .states
            .remove(&id)
            .map(|mut bytes| -> Result<_> {
                let value = simd_json::to_owned_value(&mut bytes)?;
                document
                    .schema
                    .validate_exact_schema_fields(&value)
                    .with_context(|| format!("invalid committed {id} projection"))?;
                document
                    .schema
                    .project_public_state(&crate::mutation_engine::public_plugin_value(
                        &id, &value,
                    ))?
                    .map(serde_json::to_value)
                    .transpose()
                    .map_err(Into::into)
            })
            .transpose()?
            .flatten();
        plugins.insert(
            id,
            GraphPluginProjection {
                schema: document.schema,
                schema_hash: document.schema_hash,
                state,
            },
        );
    }
    ensure!(
        crate::sealed_schema_reader::manifest_plugins(catalog_dir)? == before,
        "sealed catalog changed during graph observation; retry"
    );
    let provenance = GraphProvenance {
        kind: GraphProvenanceKind::ObservedRuntime,
        source_commit: "unknown".into(),
        release_id: None,
        generated_at: chrono::Utc::now().to_rfc3339(),
        inputs: BTreeMap::from([
            ("sealed_catalog_sha256".into(), catalog.catalog_hash),
            ("shm_generation".into(), snapshot.generation.to_string()),
        ]),
        coverage: vec!["manifest-pinned plugin contracts".into(), "committed public subsystem state".into()],
        limitations: vec![
            "Capture time is not backend observation time; absent public state is unknown, not absent infrastructure.".into(),
            "This snapshot does not independently probe services or identify the active release; compare the prepared-release inventory separately.".into(),
            "Private memory, conversations, credentials and mail content are excluded.".into(),
        ],
    };
    Ok(project_system_graph(&plugins, provenance))
}

impl crate::mutation_engine::MutationEngine {
    pub(crate) async fn refresh_system_graph(
        &self,
        expected_revision: u64,
    ) -> Result<GraphSnapshotReceipt> {
        let store = self.cognitive().await?.cozo_shuttle();
        let catalog = crate::sealed_schema_reader::blob_catalog_dir_from_env();
        let state = op_core::projection_shm::shm_state_dir();
        tokio::task::spawn_blocking(move || {
            let snapshot = committed_system_graph(&catalog, &state)?;
            store
                .replace_system_graph(&snapshot, expected_revision)
                .map_err(Into::into)
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_capture_obeys_sealed_projection_policy_and_committed_generation() {
        let root = tempfile::tempdir().unwrap();
        let catalog = root.path().join("catalog");
        let state = root.path().join("state");
        let mut schema = op_state_store::PluginSchema::builder("memory")
            .version("1.0.0")
            .string_field("backend", true, "public storage backend")
            .string_field("secret", true, "private fixture content")
            .build();
        // Real memory content is never a public graph input. This fixture also
        // detects accidental bypass of the sealed exposure policy.
        schema.public_state.paths = vec!["/backend".into()];
        let blob = op_blob::blobify_plugin_schema("memory", schema);
        op_blob::BlobStore::open(&catalog)
            .unwrap()
            .write(&blob)
            .unwrap();
        op_core::projection_shm::update_projection_in(state.to_str().unwrap(), "memory", |_| {
            Ok(Some(
                br#"{"backend":"cozo","secret":"never-export-this"}"#.to_vec(),
            ))
        })
        .unwrap();
        let graph = committed_system_graph(&catalog, state.to_str().unwrap()).unwrap();
        let json = serde_json::to_string(&graph).unwrap();
        assert!(!json.contains("never-export-this"));
        assert_eq!(graph.provenance.inputs["shm_generation"], "1");
        assert_eq!(graph.provenance.kind, GraphProvenanceKind::ObservedRuntime);
        op_cozo_store::system_graph_snapshot_digest(&graph).unwrap();
        let manifest = catalog.join(op_blob::catalog::MANIFEST_FILENAME);
        std::fs::remove_file(manifest).unwrap();
        assert!(committed_system_graph(&catalog, state.to_str().unwrap()).is_err());
    }
}
