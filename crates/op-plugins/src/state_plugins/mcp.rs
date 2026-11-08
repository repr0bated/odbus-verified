//! MCP plugin — RETIRED stub.
//!
//! MCP was part of the initial setup in error (2026-10-28 correction).
//! The interface is replaced by 3tchedFS. This stub exists only to satisfy
//! orphan imports that have not yet been removed.

// --- Types consumed by mcp_policy.rs and mcp_config_dispatch.rs ----------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConfigureToolGroupsInput {
    pub expected_generation: u64,
    pub toolsets: Vec<ToolsetConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConfigureToolGroupsOutput {
    pub generation: u64,
    pub toolsets: Vec<ToolsetConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolsetConfig {
    pub id: String,
    pub temperature: ToolsetTemperature,
    pub provider: String,
    pub requires_provider_health: bool,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolsetTemperature {
    Hot,
    Warm,
    Cold,
}

// --- Types consumed by mcp_frontend.rs / mcp_config_dispatch.rs ----------

use serde_json::Value;

pub mod methods {
    use super::{ToolsetConfig, ToolsetTemperature};
    use super::Value;

    /// Stub: complete a planning chain for an MCP tool call.
    pub fn complete_method(
        _plan: &ToolsetConfig,
        _arguments: &Value,
    ) -> Result<Value, serde_json::Error> {
        Err(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "MCP retired",
        )))
    }

    /// Stub: plan arguments for an MCP tool call.
    pub fn plan_method(_tool_name: &str, _schema: &serde_json::Value) -> Result<Value, serde_json::Error> {
        Err(serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "MCP retired",
        )))
    }
}

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff};
use op_state_store::MemoryStore;
use simd_json::OwnedValue;

/// RETIRED stub — satisfies test fixtures that still instantiate an MCP schema.
#[derive(Clone)]
pub struct McpStatePlugin {
    schema: Option<op_state_store::PluginSchema>,
}

impl McpStatePlugin {
    pub fn new(_store: Arc<MemoryStore>, _name: &str) -> Self {
        Self { schema: None }
    }
}

#[async_trait::async_trait]
impl op_state::StatePlugin for McpStatePlugin {
    fn name(&self) -> &str { "mcp" }
    fn version(&self) -> &str { "0.0.0" }
    fn schema(&self) -> Option<op_state_store::PluginSchema> { self.schema.clone() }
    async fn calculate_diff(&self, _current: &OwnedValue, _desired: &OwnedValue) -> Result<StateDiff> {
        Err(anyhow::anyhow!("MCP retired"))
    }
    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Err(anyhow::anyhow!("MCP retired"))
    }
    async fn verify_state(&self, _desired: &OwnedValue) -> Result<bool> {
        Err(anyhow::anyhow!("MCP retired"))
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Err(anyhow::anyhow!("MCP retired"))
    }
    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Err(anyhow::anyhow!("MCP retired"))
    }
    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities::default()
    }
}
