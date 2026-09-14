//! Typed projection of the runit-supervised MongoDB MCP provider.
//!
//! The provider itself is loopback-only and starts read-only. This schema is
//! intentionally limited to read/metadata/knowledge methods: connection
//! strings and Atlas write administration are not accepted through this
//! public typed surface.

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::path::Path;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;

const READY_PATH: &str = "/run/opdbus/runit-ready/mongodb-mcp-server";
const AUTH_READY_PATH: &str = "/run/opdbus/runit-ready/mongodb-mcp-server-authenticated";

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.mongodb-mcp.schema@v1"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct MongoDbMcpState {
    pub status: String,
    pub endpoint: String,
    pub read_only: bool,
    pub credential_source_configured: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EmptyInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInput {
    pub connection_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseInput {
    pub connection_id: String,
    pub database: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CollectionInput {
    pub connection_id: String,
    pub database: String,
    pub collection: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CollectionSchemaInput {
    pub connection_id: String,
    pub database: String,
    pub collection: String,
    #[serde(default)]
    pub sample_size: Option<u64>,
    #[serde(default)]
    pub response_bytes_limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FindInput {
    pub connection_id: String,
    pub database: String,
    pub collection: String,
    #[serde(default)]
    pub filter: Option<serde_json::Value>,
    #[serde(default)]
    pub projection: Option<serde_json::Value>,
    #[serde(default)]
    pub limit: Option<u64>,
    #[serde(default)]
    pub sort: Option<serde_json::Value>,
    #[serde(default)]
    pub response_bytes_limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CountInput {
    pub connection_id: String,
    pub database: String,
    pub collection: String,
    #[serde(default)]
    pub query: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AggregateInput {
    pub connection_id: String,
    pub database: String,
    pub collection: String,
    pub pipeline: Vec<serde_json::Value>,
    #[serde(default)]
    pub response_bytes_limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AggregateDatabaseInput {
    pub connection_id: String,
    pub database: String,
    pub pipeline: Vec<serde_json::Value>,
    #[serde(default)]
    pub response_bytes_limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeSourceFilter {
    pub name: String,
    #[serde(default)]
    pub version_label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchKnowledgeInput {
    pub query: String,
    #[serde(default)]
    pub limit: Option<u64>,
    #[serde(default)]
    pub data_sources: Option<Vec<KnowledgeSourceFilter>>,
}

/// Provider MCP result is retained verbatim; the bridge normalizes it to the
/// single public endpoint's `tools/call` result shape.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProviderResult {
    #[serde(flatten)]
    pub value: serde_json::Map<String, serde_json::Value>,
}

pub struct MongoDbMcpPlugin;

impl MongoDbMcpPlugin {
    pub fn new() -> Self {
        Self
    }

    pub(crate) fn current_state() -> MongoDbMcpState {
        let ready = Path::new(READY_PATH).exists();
        MongoDbMcpState {
            status: if ready { "ready" } else { "unavailable" }.to_string(),
            endpoint: "http://127.0.0.1:3102/mcp".to_string(),
            read_only: true,
            credential_source_configured: Path::new(AUTH_READY_PATH).exists(),
        }
    }
}

impl Default for MongoDbMcpPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatePlugin for MongoDbMcpPlugin {
    fn name(&self) -> &str {
        "mongodb_mcp"
    }

    fn version(&self) -> &str {
        "2.1.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(mongodb_mcp_schema())
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
        Ok(Path::new(READY_PATH).exists())
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(Self::current_state())?,
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

pub(crate) fn mongodb_mcp_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(MongoDbMcpState))
        .expect("MongoDB MCP state schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "mongodb_mcp",
        "2.1.0",
        "Read-only MongoDB MCP data, metadata, and official knowledge provider",
        &root,
    );

    macro_rules! read_method {
        ($name:literal, $input:ty, $subid:literal) => {
            schema.methods.insert(
                $name.to_string(),
                method_decl_from_schemars_with_output::<$input, ProviderResult>(
                    $name,
                    SideEffect::Read,
                    true,
                    "mongodb_mcp.read",
                    $subid,
                ),
            );
        };
    }

    read_method!(
        "list_connections",
        EmptyInput,
        "obs.service.plugin.mongodb-mcp.connection.list@v1"
    );
    read_method!(
        "list_databases",
        ConnectionInput,
        "obs.service.plugin.mongodb-mcp.database.list@v1"
    );
    read_method!(
        "list_collections",
        DatabaseInput,
        "obs.service.plugin.mongodb-mcp.collection.list@v1"
    );
    read_method!(
        "db_stats",
        DatabaseInput,
        "obs.service.plugin.mongodb-mcp.database.stats@v1"
    );
    read_method!(
        "collection_schema",
        CollectionSchemaInput,
        "obs.service.plugin.mongodb-mcp.collection.schema@v1"
    );
    read_method!(
        "collection_indexes",
        CollectionInput,
        "obs.service.mongodb-mcp.collection-index.list@v1"
    );
    read_method!(
        "collection_storage_size",
        CollectionInput,
        "obs.service.plugin.mongodb-mcp.collection.storage@v1"
    );
    read_method!(
        "find",
        FindInput,
        "obs.service.plugin.mongodb-mcp.document.find@v1"
    );
    read_method!(
        "count",
        CountInput,
        "obs.service.plugin.mongodb-mcp.document.count@v1"
    );
    read_method!(
        "aggregate",
        AggregateInput,
        "obs.service.plugin.mongodb-mcp.collection.aggregate@v1"
    );
    read_method!(
        "aggregate_db",
        AggregateDatabaseInput,
        "obs.service.plugin.mongodb-mcp.database.aggregate@v1"
    );
    read_method!(
        "list_knowledge_sources",
        EmptyInput,
        "obs.service.mongodb-mcp.knowledge-source.list@v1"
    );
    read_method!(
        "search_knowledge",
        SearchKnowledgeInput,
        "obs.service.plugin.mongodb-mcp.knowledge.search@v1"
    );

    schema.capabilities.insert(
        "mongodb_mcp.read".to_string(),
        CapabilityDecl {
            id: "mongodb_mcp.read".to_string(),
            description:
                "Read-only MongoDB metadata, queries, aggregations, and official knowledge search"
                    .to_string(),
        },
    );
    schema
}

inventory::submit! {
    crate::default_registry::PluginReg::new("mongodb_mcp", |_ctx| std::sync::Arc::new(MongoDbMcpPlugin::new()))
}
