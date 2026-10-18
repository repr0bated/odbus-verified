//! CozoDB relational-graph-vector database plugin.
//!
//! Schema derived from CozoDB official documentation (docs.cozodb.org/en/latest/).
//! CozoDB is a Datalog database with dynamic schema — relations are created with
//! `:create` specifying key/value columns and types. The system exposes metadata
//! via `::relations`, `::columns`, `::indices` system ops.
//!
//! Column types (from docs.cozodb.org/en/latest/datatypes.html):
//!   Atomic: Int, Float, Bool, String, Bytes, Uuid, Json, Validity
//!   Composite: \[Type\] (list), (T1, T2) (tuple), <F32; N> (vector)
//!   Special: Any, Any? (nullable)
//!
//! The bridge supplies its existing cognitive Cozo handle; methods do not open
//! another database. Portable archives never invoke SQLite backup APIs.
//! Access levels: normal, protected, read_only, hidden

use schemars::JsonSchema;

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

#[path = "cozo_methods.rs"]
pub mod methods;
pub use methods::dispatch_cozo_method;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunQueryInput {
    pub query: String,
    #[serde(default)]
    pub params: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportRelationsInput {
    /// Native Cozo JSON headers/rows. Existing relations must already exist.
    /// Transactional puts maintain indexes and enable declared triggers.
    pub data: std::collections::BTreeMap<String, methods::RelationRows>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExportRelationsInput {
    pub relations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupInput {
    /// A short archive label, not a filesystem path. The receipt names a unique
    /// immutable artifact beneath OP_COZO_ARCHIVE_DIR.
    pub path: String,
    /// Empty selects all base relations. Archives reject defaults, triggers,
    /// advanced indexes, hidden relations, and nonempty descriptions because
    /// Cozo 0.7.6 cannot expose/recreate that metadata faithfully here.
    #[serde(default)]
    pub relations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreInput {
    /// Exact artifact basename from backup's receipt. Existing relation
    /// metadata must match; missing relations are created atomically.
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListRelationsInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DescribeRelationInput {
    pub name: String,
}

/// Replace one named system inventory; revision zero requires a new namespace.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceSystemGraphInput {
    pub snapshot: op_cozo_store::GraphSnapshot,
    pub expected_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SystemGraphStatusInput {
    pub namespace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SystemGraphStatusOutput {
    pub snapshot: Option<op_cozo_store::GraphSnapshotReceipt>,
}

/// Refresh only the shared, public topology from committed plugin projections.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefreshSystemGraphInput {
    pub expected_revision: u64,
}

/// CozoDB column definition — mirrors `::columns` system op output.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CozoColumn {
    /// Column name.
    #[schemars(extend("x-oscal-subid" = "exp.software.cozo.column.name@v1"))]
    pub name: String,
    /// Column type (Int, Float, Bool, String, Bytes, Uuid, Json, Validity, Any, Any?).
    #[schemars(extend("x-oscal-subid" = "obs.software.cozo.column.type@v1"))]
    pub r#type: String,
    /// Whether this column is a key in the :create specification.
    #[schemars(extend("x-oscal-subid" = "obs.software.cozo.column.is-key@v1"))]
    pub is_key: bool,
    /// Default value for the column.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("x-oscal-subid" = "obs.software.cozo.column.default@v1"))]
    pub default: Option<String>,
    /// Column description.
    #[schemars(extend("x-oscal-subid" = "exp.software.cozo.column.description@v1"))]
    pub description: String,
}

/// CozoDB index definition — mirrors `::indices` system op output.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CozoIndex {
    pub name: String, // e.g. "relation_name:index_name"
    pub relation: String,
    pub columns: Vec<String>,
}

/// CozoDB trigger — mirrors `::show_triggers` system op output.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CozoTrigger {
    pub relation: String,
    pub event: String, // "put", "rm", "replace"
    pub query: String,
}

/// CozoDB stored relation — mirrors `::relations` + `::columns` system op output.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CozoRelation {
    pub name: String,
    pub description: String,
    pub keys: Vec<CozoColumn>,
    pub values: Vec<CozoColumn>,
    pub access_level: String, // normal, protected, read_only, hidden
    pub row_count: Option<u64>,
}

/// CozoDB HNSW vector index — from docs.cozodb.org/en/latest/vector.html
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CozoHnswIndex {
    pub name: String,
    pub relation: String,
    pub vector_column: String,
    pub dim: u32,
    pub dtype: String, // "f32" or "f64"
    pub m: u32,
    pub ef_construction: u32,
    pub ef_search: u32,
    pub distance: String, // "Cosine", "Euclid", "Dot"
}

/// Top-level CozoDB plugin state — the shape the projection tree exposes.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.cozo.schema@v1"))]
#[schemars(extend("x-oscal-category" = "data"))]
pub struct CozoState {
    /// CozoDB engine type.
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.cozo.engine@v1"))]
    pub engine: String,
    /// Database path (empty for mem).
    #[schemars(extend("x-oscal-subid" = "src.software.plugin.cozo.path@v1"))]
    pub path: String,
    /// CozoDB version.
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.cozo.version@v1"))]
    pub version: String,
    /// Registered database relations.
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.cozo.relations@v1"))]
    pub relations: Vec<CozoRelation>,
    /// Database indices.
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.cozo.indices@v1"))]
    pub indices: Vec<CozoIndex>,
    /// HNSW vector indices.
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.cozo.hnsw-indices@v1"))]
    pub hnsw_indices: Vec<CozoHnswIndex>,
    /// Database triggers.
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.cozo.triggers@v1"))]
    pub triggers: Vec<CozoTrigger>,
    /// Currently running queries count.
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.cozo.running-queries@v1"))]
    pub running_queries: u32,
}

pub struct CozoPlugin;

impl Default for CozoPlugin {
    fn default() -> Self {
        Self
    }
}

impl CozoPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for CozoPlugin {
    fn name(&self) -> &str {
        "cozo"
    }

    fn version(&self) -> &str {
        "2.1.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = cozo_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::json!(null),
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

pub(crate) fn cozo_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(CozoState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "cozo",
        "2.1.0",
        "CozoDB relational-graph-vector database — relations, indices, HNSW, triggers, Datalog",
        &root,
    );
    schema.category = "data".to_string();
    schema.subids.insert(
        "__schema__".to_string(),
        "sch.software.plugin.cozo.schema@v1".to_string(),
    );
    schema.subids.insert(
        "engine".to_string(),
        "src.software.plugin.cozo.engine@v1".to_string(),
    );
    schema.subids.insert(
        "path".to_string(),
        "src.software.plugin.cozo.path@v1".to_string(),
    );
    schema.subids.insert(
        "version".to_string(),
        "obs.software.plugin.cozo.version@v1".to_string(),
    );
    schema.subids.insert(
        "relations".to_string(),
        "obs.software.plugin.cozo.relations@v1".to_string(),
    );
    schema.subids.insert(
        "indices".to_string(),
        "obs.software.plugin.cozo.indices@v1".to_string(),
    );
    schema.subids.insert(
        "hnsw_indices".to_string(),
        "obs.software.plugin.cozo.hnsw-indices@v1".to_string(),
    );
    schema.subids.insert(
        "triggers".to_string(),
        "obs.software.plugin.cozo.triggers@v1".to_string(),
    );
    schema.subids.insert(
        "running_queries".to_string(),
        "obs.software.plugin.cozo.running-queries@v1".to_string(),
    );

    schema.methods.insert(
        "run_query".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            RunQueryInput,
            methods::QueryOutput,
        >(
            "run_query",
            op_state_store::SideEffect::Mutation, // Assuming it could mutate data
            false,
            "cap.data.cozo.query.run@v1",
            "mut.data.cozo.query.run@v1",
        ),
    );
    schema.methods.insert(
        "import_relations".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ImportRelationsInput,
            methods::ImportOutput,
        >(
            "import_relations",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.data.cozo.relations.import@v1",
            "mut.data.cozo.relations.import@v1",
        ),
    );
    schema.methods.insert(
        "export_relations".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ExportRelationsInput,
            methods::ExportOutput,
        >(
            "export_relations",
            op_state_store::SideEffect::Read,
            true,
            "cap.data.cozo.relations.export@v1",
            "obs.data.cozo.relations.export@v1",
        ),
    );
    schema.methods.insert(
        "backup".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            BackupInput,
            methods::ArchiveReceipt,
        >(
            "backup",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.data.cozo.backup.create@v1",
            "mut.data.cozo.backup.create@v1",
        ),
    );
    schema.methods.insert(
        "restore".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            RestoreInput,
            methods::RestoreReceipt,
        >(
            "restore",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.data.cozo.backup.restore@v1",
            "mut.data.cozo.backup.restore@v1",
        ),
    );
    schema.methods.insert(
        "list_relations".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ListRelationsInput,
            methods::ListRelationsOutput,
        >(
            "list_relations",
            op_state_store::SideEffect::Read,
            true,
            "cap.data.cozo.relations.list@v1",
            "obs.data.cozo.relations.list@v1",
        ),
    );
    schema.methods.insert(
        "describe_relation".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            DescribeRelationInput,
            methods::RelationDescription,
        >(
            "describe_relation",
            op_state_store::SideEffect::Read,
            true,
            "cap.data.cozo.relation.describe@v1",
            "obs.data.cozo.relation.describe@v1",
        ),
    );

    schema.capabilities.insert(
        "cap.data.cozo.query.run@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.cozo.query.run@v1".to_string(),
            description: "Grants: run_query.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.data.cozo.relations.import@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.cozo.relations.import@v1".to_string(),
            description: "Grants: import_relations.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.data.cozo.relations.export@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.cozo.relations.export@v1".to_string(),
            description: "Grants: export_relations.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.data.cozo.backup.create@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.cozo.backup.create@v1".to_string(),
            description: "Grants: backup.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.data.cozo.backup.restore@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.cozo.backup.restore@v1".to_string(),
            description: "Grants: restore.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.data.cozo.relations.list@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.cozo.relations.list@v1".to_string(),
            description: "Grants: list_relations.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.data.cozo.relation.describe@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.cozo.relation.describe@v1".to_string(),
            description: "Grants: describe_relation.".to_string(),
        },
    );

    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output as graph_method;
    use op_cozo_store::{
        GraphEdgePage, GraphEdgeQuery, GraphNamespacePage, GraphNamespaceQuery, GraphNodePage,
        GraphNodeQuery, GraphSnapshotReceipt, GraphTraversal, GraphTraversalQuery,
    };
    use op_state_store::{CapabilityDecl, SideEffect};

    let graph_methods = [
        graph_method::<GraphNamespaceQuery, GraphNamespacePage>(
            "list_system_graphs",
            SideEffect::Read,
            true,
            "cap.data.cozo.system-graph.read@v1",
            "obs.data.cozo.system-graph.list@v1",
        ),
        graph_method::<ReplaceSystemGraphInput, GraphSnapshotReceipt>(
            "replace_system_graph",
            SideEffect::Mutation,
            false,
            "cap.data.cozo.system-graph.write@v1",
            "mut.data.cozo.system-graph.replace@v1",
        ),
        graph_method::<RefreshSystemGraphInput, GraphSnapshotReceipt>(
            "refresh_system_graph",
            SideEffect::Mutation,
            false,
            "cap.data.cozo.system-graph.refresh@v1",
            "mut.data.cozo.system-graph.refresh@v1",
        ),
        graph_method::<SystemGraphStatusInput, SystemGraphStatusOutput>(
            "system_graph_status",
            SideEffect::Read,
            true,
            "cap.data.cozo.system-graph.read@v1",
            "obs.data.cozo.system-graph.status@v1",
        ),
        graph_method::<GraphNodeQuery, GraphNodePage>(
            "query_system_graph_nodes",
            SideEffect::Read,
            true,
            "cap.data.cozo.system-graph.read@v1",
            "obs.data.cozo.system-graph.nodes@v1",
        ),
        graph_method::<GraphEdgeQuery, GraphEdgePage>(
            "query_system_graph_edges",
            SideEffect::Read,
            true,
            "cap.data.cozo.system-graph.read@v1",
            "obs.data.cozo.system-graph.edges@v1",
        ),
        graph_method::<GraphTraversalQuery, GraphTraversal>(
            "traverse_system_graph",
            SideEffect::Read,
            true,
            "cap.data.cozo.system-graph.read@v1",
            "obs.data.cozo.system-graph.traverse@v1",
        ),
    ];
    for method in graph_methods {
        schema.methods.insert(method.name.clone(), method);
    }
    for (id, description) in [
        (
            "cap.data.cozo.system-graph.write@v1",
            "Replace a named system inventory with revision conflict detection.",
        ),
        (
            "cap.data.cozo.system-graph.refresh@v1",
            "Refresh shared system topology from committed public plugin state.",
        ),
        (
            "cap.data.cozo.system-graph.read@v1",
            "Read bounded system graph pages and dependency traversals; no arbitrary queries.",
        ),
    ] {
        schema.capabilities.insert(
            id.into(),
            CapabilityDecl {
                id: id.into(),
                description: description.into(),
            },
        );
    }
    schema
}

inventory::submit! {
    crate::default_registry::PluginReg::new("cozo", |_ctx| std::sync::Arc::new(CozoPlugin::new()))
}
