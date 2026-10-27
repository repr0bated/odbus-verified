//! Native chat_router MCP server facade plus the bridge-owned public gateway
//! toolset policy.

use crate::state_plugins::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{PluginSchema, SideEffect, StateStore};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::collections::HashMap;
use std::sync::Arc;

#[path = "mcp_methods.rs"]
pub mod methods;

/// MCP configuration schema - mirrors the state JSON structure
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct McpConfig {
    /// External MCP servers indexed by name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub servers: Option<HashMap<String, McpServerConfig>>,

    /// Tool groups configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_groups: Option<ToolGroupsConfig>,

    /// Compact mode settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compact_mode: Option<CompactModeConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct McpServerConfig {
    /// Server command to execute
    pub command: String,

    /// Command arguments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,

    /// Environment variables
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,

    /// Whether server is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Transport type (stdio, sse, http)
    #[serde(default = "default_stdio")]
    pub transport: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct ToolGroupsConfig {
    /// Enabled group IDs
    pub enabled: Vec<String>,

    /// Maximum tools limit
    #[serde(default = "default_max_tools")]
    pub max_tools: usize,

    /// Access zone (localhost, trusted_mesh, private_network, public)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_zone: Option<String>,

    /// Trusted network prefixes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trusted_networks: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolsetTemperature {
    Hot,
    Warm,
    Cold,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolsetConfig {
    pub id: String,
    pub temperature: ToolsetTemperature,
    pub provider: String,
    pub requires_provider_health: bool,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigureToolGroupsInput {
    pub expected_generation: u64,
    pub toolsets: Vec<ToolsetConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConfigureToolGroupsOutput {
    pub generation: u64,
    pub toolsets: Vec<ToolsetConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
pub struct CompactModeConfig {
    /// Whether compact mode is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Meta-tools to expose
    #[serde(default = "default_meta_tools")]
    pub meta_tools: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_stdio() -> String {
    "stdio".to_string()
}

fn default_max_tools() -> usize {
    40
}

fn default_meta_tools() -> Vec<String> {
    vec![
        "list_tools".to_string(),
        "search_tools".to_string(),
        "get_tool_schema".to_string(),
        "execute_tool".to_string(),
        "respond".to_string(),
    ]
}

/// Tool definition - canonical schema for all tools
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(default = "default_schema_version")]
    pub schema_version: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_namespace")]
    pub namespace: String,
}

fn default_schema_version() -> String {
    "https://json-schema.org/draft/next/schema".to_string()
}

fn default_namespace() -> String {
    "system.v1".to_string()
}

/// Tool execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub output: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// MCP state plugin
pub struct McpStatePlugin {
    // Retained for constructor compatibility. Native ownership belongs to
    // chat_router; this facade never reads or writes either value.
    _state_store: Arc<dyn op_state_store::StateStore>,
    _config_path: String,
}

impl McpStatePlugin {
    pub fn new(state_store: Arc<dyn StateStore>, config_path: impl Into<String>) -> Self {
        Self {
            _state_store: state_store,
            _config_path: config_path.into(),
        }
    }
}

// ── Schema-only borrowed view of the native chat_router MCP section ──

fn default_empty_object() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.mcp.schema@v1"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct McpState {
    #[serde(default = "default_empty_object")]
    #[schemars(
        description = "Native chat_router mcp configuration",
        extend("x-oscal-subid" = "exp.software.plugin.mcp.config@v2")
    )]
    config: serde_json::Value,
    /// Canonical gateway projection policy, updated only by
    /// configure_tool_groups after durable readback and live owner swap.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    toolsets: Option<ConfigureToolGroupsOutput>,
}

pub(crate) fn mcp_schema() -> PluginSchema {
    if !super::threetched::is_schema_capture() {
        return super::threetched::sealed_schema("mcp")
            .expect("MCP runtime requires its sealed schema");
    }
    let root = serde_json::to_value(schemars::schema_for!(McpState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "mcp",
        "3.0.0",
        "MCP server and tool-group configuration",
        &root,
    );
    schema
        .fields
        .get_mut("config")
        .expect("MCP config field exists")
        .field_type =
        op_state_store::FieldType::exact_json_schema(super::chat_router_contract::section("mcp"))
            .expect("native MCP section is a self-contained exact schema");
    schema.dependencies = vec!["chat_router".to_string()];

    let native = super::chat_router::chat_router_schema();
    let mut get_config = native.methods["GetMcpConfig"].clone();
    get_config.name = "get_config".to_string();
    get_config.required_capability = Some("mcp.read".to_string());
    get_config.subid = "obs.software.plugin.mcp.config.get@v2".to_string();
    schema.methods.insert("get_config".to_string(), get_config);
    let mut configure_server = native.methods["PatchConfig"].clone();
    configure_server.name = "configure_server".to_string();
    configure_server.required_capability = Some("mcp.write".to_string());
    configure_server.subid = "mut.software.plugin.mcp.server.configure@v2".to_string();
    schema
        .methods
        .insert("configure_server".to_string(), configure_server);
    schema.methods.insert(
        "configure_tool_groups".to_string(),
        method_decl_from_schemars_with_output::<
            ConfigureToolGroupsInput,
            ConfigureToolGroupsOutput,
        >(
            "configure_tool_groups",
            SideEffect::Mutation,
            true,
            "mcp.write",
            "mut.software.plugin.mcp.tool-groups.configure@v1",
        ),
    );

    schema.capabilities.insert(
        "mcp.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "mcp.read".to_string(),
            description: "Grants: get_config.".to_string(),
        },
    );
    schema.capabilities.insert(
        "mcp.write".to_string(),
        op_state_store::CapabilityDecl {
            id: "mcp.write".to_string(),
            description: "Grants: configure_server, configure_tool_groups.".to_string(),
        },
    );

    schema
}

#[async_trait]
impl StatePlugin for McpStatePlugin {
    fn name(&self) -> &str {
        "mcp"
    }

    fn version(&self) -> &str {
        "3.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(mcp_schema())
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        let _ = (current, desired);
        anyhow::bail!("mcp facade has no local diff owner; use chat_router PatchConfig")
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let _ = diff;
        anyhow::bail!("mcp facade has no local apply owner; use chat_router PatchConfig")
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        anyhow::bail!("mcp facade has no local verification owner; use chat_router readback")
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        anyhow::bail!("mcp facade has no local checkpoint owner")
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        anyhow::bail!("mcp facade has no local rollback owner")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::common::oscal::validate_subid;
    use op_state_store::MemoryStore;
    use serde_json::Value as JVal;

    fn collect_subids(node: &JVal, out: &mut Vec<String>) {
        if let Some(subid) = node.get("x-oscal-subid").and_then(JVal::as_str) {
            out.push(subid.to_string());
        }
        if let Some(props) = node.get("properties").and_then(JVal::as_object) {
            for v in props.values() {
                collect_subids(v, out);
            }
        }
        if let Some(defs) = node
            .get("$defs")
            .or_else(|| node.get("definitions"))
            .and_then(JVal::as_object)
        {
            for v in defs.values() {
                collect_subids(v, out);
            }
        }
        if let Some(items) = node.get("items") {
            collect_subids(items, out);
        }
        if let Some(alternatives) = node
            .get("anyOf")
            .or_else(|| node.get("oneOf"))
            .and_then(JVal::as_array)
        {
            for v in alternatives {
                collect_subids(v, out);
            }
        }
    }

    #[test]
    fn should_publish_plugin_owned_mcp_schema() {
        let store = Arc::new(MemoryStore::new());
        let plugin = McpStatePlugin::new(store, "/tmp/test-mcp-schema.json");
        let schema = plugin.schema().expect("mcp schema");

        assert_eq!(schema.name, "mcp");
        assert_eq!(schema.version, "3.0.0");
        assert!(schema.fields.contains_key("config"));
        assert_eq!(schema.dependencies, vec!["chat_router".to_string()]);
    }

    #[test]
    fn native_method_contracts_are_cloned_before_facade_renames() {
        let native = crate::state_plugins::chat_router::chat_router_schema();
        let facade = mcp_schema();
        for (facade_name, native_name) in [
            ("get_config", "GetMcpConfig"),
            ("configure_server", "PatchConfig"),
        ] {
            let lhs = &facade.methods[facade_name];
            let rhs = &native.methods[native_name];
            assert_eq!(lhs.args, rhs.args);
            assert_eq!(lhs.returns, rhs.returns);
            assert_eq!(lhs.side_effect, rhs.side_effect);
            assert_eq!(lhs.idempotent, rhs.idempotent);
        }
    }

    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(McpState)).unwrap();
        let mut subids = Vec::new();
        collect_subids(&raw, &mut subids);
        assert!(
            !subids.is_empty(),
            "expected at least one x-oscal-subid in the derived schema"
        );
        for subid in subids {
            validate_subid(&subid).expect("invalid subid: {subid}");
        }
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("mcp", |ctx| std::sync::Arc::new(McpStatePlugin::new(ctx.state_store(), ctx.config_path("mcp", "/etc/op-dbus/mcp-config.json"))))
}
