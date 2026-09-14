use super::plugin_scaffold_helpers::{method_decl_from_schemars_with_output, EmptyInput};
use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use simd_json::OwnedValue as Value;

/// Public plugin state contains only durable-backend semantics. Private memory
/// contents remain in Cozo and are returned only by context-scoped methods.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(extend("x-oscal-category" = "data"))]
pub struct MemoryState {
    pub backend: String,
    pub isolation: String,
}

pub struct MemoryPlugin;

impl Default for MemoryPlugin {
    fn default() -> Self {
        Self
    }
}

impl MemoryPlugin {
    pub fn new() -> Self {
        Self
    }

    pub(crate) fn current_state() -> MemoryState {
        MemoryState {
            backend: "cozo".to_string(),
            isolation: "principal_session".to_string(),
        }
    }
}

#[async_trait]
impl StatePlugin for MemoryPlugin {
    fn name(&self) -> &str {
        "memory"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(memory_schema())
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: Vec::new(),
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct NamespaceInfo {
    pub id: String,
    pub name: String,
    /// `None` means unbounded. Zero is rejected at creation.
    pub max_entries: Option<u64>,
    pub read_only: bool,
    pub entry_count: u64,
    pub config: JsonValue,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct EntryInfo {
    pub id: String,
    pub namespace: String,
    pub key: String,
    pub value: JsonValue,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateNamespaceInput {
    pub name: String,
    #[serde(default)]
    pub max_entries: Option<u64>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub config: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateNamespaceOutput {
    pub namespace: NamespaceInfo,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteNamespaceInput {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeleteNamespaceOutput {
    pub deleted: bool,
    pub deleted_entries: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryNamespaceInput {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryNamespaceOutput {
    pub namespace: Option<NamespaceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InsertEntryInput {
    pub namespace: String,
    pub key: String,
    pub value: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InsertEntryOutput {
    pub entry: EntryInfo,
    pub entry_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateEntryInput {
    pub namespace: String,
    pub key: String,
    pub value: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateEntryOutput {
    pub entry: EntryInfo,
    pub previous_value: JsonValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetEntryInput {
    pub namespace: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetEntryOutput {
    pub entry: Option<EntryInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteEntryInput {
    pub namespace: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeleteEntryOutput {
    pub deleted_entry_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClearNamespaceInput {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ClearNamespaceOutput {
    pub deleted_entries: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NamespaceStats {
    pub name: String,
    pub entries: u64,
    pub max_entries: Option<u64>,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetStatsOutput {
    pub total_namespaces: u64,
    pub total_entries: u64,
    pub namespaces: Vec<NamespaceStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CompactOutput {
    pub scope: String,
    pub compacted_at: String,
    /// Cozo does not report a reclaimed-byte delta.
    pub reclaimed_bytes: Option<u64>,
}

pub(crate) fn memory_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(MemoryState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "memory",
        "2.0.0",
        "Private principal-and-session memory stored durably in Cozo",
        &root,
    );

    let methods = [
        (
            "create_namespace",
            method_decl_from_schemars_with_output::<CreateNamespaceInput, CreateNamespaceOutput>(
                "create_namespace",
                SideEffect::Mutation,
                true,
                "memory.namespace.create",
                "mut.data.memory.create-namespace@v1",
            ),
        ),
        (
            "delete_namespace",
            method_decl_from_schemars_with_output::<DeleteNamespaceInput, DeleteNamespaceOutput>(
                "delete_namespace",
                SideEffect::Mutation,
                true,
                "memory.namespace.delete",
                "mut.data.memory.delete-namespace@v1",
            ),
        ),
        (
            "query_namespace",
            method_decl_from_schemars_with_output::<QueryNamespaceInput, QueryNamespaceOutput>(
                "query_namespace",
                SideEffect::Read,
                true,
                "memory.namespace.read",
                "obs.data.memory.query-namespace@v1",
            ),
        ),
        (
            "insert_entry",
            method_decl_from_schemars_with_output::<InsertEntryInput, InsertEntryOutput>(
                "insert_entry",
                SideEffect::Mutation,
                false,
                "memory.entry.create",
                "mut.data.memory.insert-entry@v1",
            ),
        ),
        (
            "update_entry",
            method_decl_from_schemars_with_output::<UpdateEntryInput, UpdateEntryOutput>(
                "update_entry",
                SideEffect::Mutation,
                false,
                "memory.entry.write",
                "mut.data.memory.update-entry@v1",
            ),
        ),
        (
            "get_entry",
            method_decl_from_schemars_with_output::<GetEntryInput, GetEntryOutput>(
                "get_entry",
                SideEffect::Read,
                true,
                "memory.entry.read",
                "obs.data.memory.get-entry@v1",
            ),
        ),
        (
            "delete_entry",
            method_decl_from_schemars_with_output::<DeleteEntryInput, DeleteEntryOutput>(
                "delete_entry",
                SideEffect::Mutation,
                true,
                "memory.entry.delete",
                "mut.data.memory.delete-entry@v1",
            ),
        ),
        (
            "clear_namespace",
            method_decl_from_schemars_with_output::<ClearNamespaceInput, ClearNamespaceOutput>(
                "clear_namespace",
                SideEffect::Mutation,
                true,
                "memory.namespace.clear",
                "mut.data.memory.clear-namespace@v1",
            ),
        ),
        (
            "get_stats",
            method_decl_from_schemars_with_output::<EmptyInput, GetStatsOutput>(
                "get_stats",
                SideEffect::Read,
                true,
                "memory.stats.read",
                "obs.data.memory.get-stats@v1",
            ),
        ),
        (
            "compact",
            method_decl_from_schemars_with_output::<EmptyInput, CompactOutput>(
                "compact",
                SideEffect::Mutation,
                true,
                "memory.memory.compact",
                "mut.data.memory.compact@v1",
            ),
        ),
    ];
    schema.methods.extend(
        methods
            .into_iter()
            .map(|(name, declaration)| (name.to_string(), declaration)),
    );

    for (id, methods) in [
        ("memory.namespace.create", "create_namespace"),
        ("memory.namespace.delete", "delete_namespace"),
        ("memory.namespace.read", "query_namespace"),
        ("memory.entry.create", "insert_entry"),
        ("memory.entry.write", "update_entry"),
        ("memory.entry.read", "get_entry"),
        ("memory.entry.delete", "delete_entry"),
        ("memory.namespace.clear", "clear_namespace"),
        ("memory.stats.read", "get_stats"),
        ("memory.memory.compact", "compact"),
    ] {
        schema.capabilities.insert(
            id.to_string(),
            CapabilityDecl {
                id: id.to_string(),
                description: format!("Grants: {methods}."),
            },
        );
    }
    schema
        .capabilities
        .get_mut("memory.memory.compact")
        .expect("compact capability inserted")
        .description = "Grants only whole-store Cozo physical maintenance via compact; it does not grant namespace or entry access."
        .to_string();

    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_has_typed_name_matched_methods() {
        let schema = memory_schema();
        assert_eq!(schema.version, "2.0.0");
        assert_eq!(schema.methods.len(), 10);
        for (key, method) in &schema.methods {
            assert_eq!(key, &method.name);
            assert!(method.returns.is_some(), "{key} needs a typed output");
        }
    }
}

inventory::submit! {
    crate::default_registry::PluginReg::new("memory", |_ctx| std::sync::Arc::new(MemoryPlugin::new()))
}
