//! `datastore` indexes the canonical committed public plugin projections.
//!
//! The five retained methods are read-only views over manifest-pinned sealed
//! schemas and the matching SHM projection generation. This plugin owns the
//! method contract; it does not create a database, read private runtime
//! documents, or aggregate independently owned audit/execution stores.

use anyhow::{Result, bail};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

pub const MAX_DATASTORE_SELECTION: usize = 256;
pub const MAX_DATASTORE_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.datastore.schema@v2"))]
#[schemars(extend("x-oscal-category" = "data"))]
pub struct DataStoreState {
    pub generation: u64,
    pub catalog_hash: String,
    pub object_count: usize,
    pub present_count: usize,
    pub withheld_count: usize,
    pub unobserved_count: usize,
    pub state_bytes: u64,
    pub objects: Vec<DataStoreObjectRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PublicObjectExposure {
    /// A committed observation exists and was filtered by the sealed schema's
    /// explicit public-state policy.
    Present,
    /// The sealed schema deliberately declares no public-state paths.
    Withheld,
    /// The schema exposes public paths, but no observation has been committed.
    Unobserved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DataStoreObjectRef {
    pub plugin_id: String,
    pub object_path: String,
    pub schema_hash: String,
    pub exposure: PublicObjectExposure,
    pub state_bytes: u64,
}

/// Schema-owned dynamic content inside a typed, generation-bound envelope.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DataStoreObject {
    pub reference: DataStoreObjectRef,
    /// Structurally exact JSON parsed from the manifest-pinned sealed plugin
    /// schema. `reference.schema_hash` binds it to the original sealed bytes.
    pub schema: serde_json::Value,
    /// Policy-filtered committed state. `None` is explained by `exposure`.
    pub state: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SnapshotBinding {
    pub generation: u64,
    pub catalog_hash: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PluginSelection {
    /// Exact sealed plugin ids. Empty selects the whole sealed catalog.
    #[serde(default)]
    #[schemars(length(max = 256), inner(length(min = 1, max = 128)))]
    pub plugin_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetObjectInput {
    #[schemars(length(min = 1, max = 128))]
    pub plugin_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryInput {
    /// Exact sealed plugin ids. Empty selects the whole sealed catalog.
    #[serde(default)]
    #[schemars(length(max = 256), inner(length(min = 1, max = 128)))]
    pub plugin_ids: Vec<String>,
    /// Optional exact exposure class; no arbitrary query language is accepted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exposure: Option<PublicObjectExposure>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetStatsInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListObjectsOutput {
    pub binding: SnapshotBinding,
    pub objects: Vec<DataStoreObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetObjectOutput {
    pub binding: SnapshotBinding,
    pub object: DataStoreObject,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QueryOutput {
    pub binding: SnapshotBinding,
    pub results: Vec<DataStoreObject>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetStatsOutput {
    pub binding: SnapshotBinding,
    pub object_count: usize,
    pub present_count: usize,
    pub withheld_count: usize,
    pub unobserved_count: usize,
    pub state_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExportOutput {
    pub binding: SnapshotBinding,
    pub objects: Vec<DataStoreObject>,
}

pub struct DataStorePlugin;

impl DataStorePlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DataStorePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatePlugin for DataStorePlugin {
    fn name(&self) -> &str {
        "datastore"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = datastore_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        bail!("datastore is a read-only committed public projection index")
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        bail!("datastore has no mutation or apply operation")
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        bail!("datastore has no desired-state verification operation")
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        bail!("datastore does not own the SHM projection transaction")
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        bail!("datastore does not own rollback")
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: false,
            atomic_operations: false,
        }
    }
}

pub(crate) fn datastore_schema() -> PluginSchema {
    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
    use op_state_store::SideEffect;

    let root = serde_json::to_value(schemars::schema_for!(DataStoreState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "datastore",
        "2.0.0",
        "Read-only index of manifest-pinned, policy-filtered committed public plugin state",
        &root,
    );
    schema.example = Some(simd_json::json!({
        "generation": 7,
        "catalog_hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "object_count": 1,
        "present_count": 1,
        "withheld_count": 0,
        "unobserved_count": 0,
        "state_bytes": 18,
        "objects": [{
            "plugin_id": "service",
            "object_path": "/org/opdbus/v1/plugins/service",
            "schema_hash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "exposure": "present",
            "state_bytes": 18
        }]
    }));

    schema.methods.insert(
        "list_objects".into(),
        method_decl_from_schemars_with_output::<PluginSelection, ListObjectsOutput>(
            "list_objects",
            SideEffect::Read,
            true,
            "datastore.read",
            "obs.service.datastore.object.list@v2",
        ),
    );
    schema.methods.insert(
        "get_object".into(),
        method_decl_from_schemars_with_output::<GetObjectInput, GetObjectOutput>(
            "get_object",
            SideEffect::Read,
            true,
            "datastore.read",
            "obs.service.datastore.object.get@v2",
        ),
    );
    schema.methods.insert(
        "query".into(),
        method_decl_from_schemars_with_output::<QueryInput, QueryOutput>(
            "query",
            SideEffect::Read,
            true,
            "datastore.read",
            "obs.service.datastore.query@v2",
        ),
    );
    schema.methods.insert(
        "get_stats".into(),
        method_decl_from_schemars_with_output::<GetStatsInput, GetStatsOutput>(
            "get_stats",
            SideEffect::Read,
            true,
            "datastore.read",
            "obs.service.datastore.stats@v2",
        ),
    );
    schema.methods.insert(
        "export".into(),
        method_decl_from_schemars_with_output::<PluginSelection, ExportOutput>(
            "export",
            SideEffect::Read,
            true,
            "datastore.read",
            "obs.service.datastore.export@v2",
        ),
    );
    schema.capabilities.insert(
        "datastore.read".into(),
        op_state_store::CapabilityDecl {
            id: "datastore.read".into(),
            description: "Grants read-only access to sealed-schema-filtered public SHM projections through list_objects, get_object, query, get_stats and export.".into(),
        },
    );
    schema
}

inventory::submit! {
    crate::default_registry::PluginReg::new("datastore", |_ctx| std::sync::Arc::new(DataStorePlugin::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_method_contract_is_typed_and_read_only() {
        let schema = datastore_schema();
        assert_eq!(schema.version, "2.0.0");
        assert_eq!(schema.methods.len(), 5);
        for (name, method) in &schema.methods {
            assert_eq!(name, &method.name);
            assert_eq!(method.side_effect, op_state_store::SideEffect::Read);
            assert_eq!(
                serde_json::to_value(&method.args).unwrap()["type"],
                "object"
            );
            assert_eq!(
                serde_json::to_value(method.returns.as_ref().unwrap()).unwrap()["type"],
                "object"
            );
        }
    }

    #[tokio::test]
    async fn lifecycle_hooks_never_fabricate_success() {
        let plugin = DataStorePlugin::new();
        let empty = simd_json::json!({});
        assert!(plugin.calculate_diff(&empty, &empty).await.is_err());
        assert!(plugin.verify_state(&empty).await.is_err());
        assert!(plugin.create_checkpoint().await.is_err());
    }
}
