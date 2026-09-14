//! Plugin State Tool - Creates tools from StatePlugin operations
//!
//! Provides query, diff, and apply tools for each registered StatePlugin.

use anyhow::Result;
use async_trait::async_trait;
use simd_json::OwnedValue as Value;
use simd_json::prelude::*;
use std::sync::Arc;
use tokio::sync::RwLock;

use op_state::PluginCapabilities;

/// Operation supported by the plugin tool
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginOperation {
    Query,
    Diff,
    Apply,
}
use crate::tool::{BoxedTool, Tool};

/// Plugin state tool that wraps a StatePlugin operation
pub struct PluginStateTool {
    name: String,
    description: String,
    operation: PluginOperation,
    plugin_name: String,
    capabilities: PluginCapabilities,
    /// Reference to the plugin catalog executor for executing operations
    plugin_executor: Arc<dyn PluginExecutor + Send + Sync>,
}

/// Trait for executing plugin operations
#[async_trait]
pub trait PluginExecutor: Send + Sync {
    /// Query current state from a plugin
    async fn query_state(&self, plugin_name: &str, filter: Option<Value>) -> Result<Value>;
    
    /// Calculate diff between current and desired state
    async fn calculate_diff(&self, plugin_name: &str, desired_state: Value) -> Result<Value>;
    
    /// Apply a state diff
    async fn apply_diff(&self, plugin_name: &str, diff: Value, dry_run: bool) -> Result<Value>;
}

impl PluginStateTool {
    pub fn new(
        plugin_name: &str,
        description: &str,
        operation: PluginOperation,
        capabilities: &PluginCapabilities,
        executor: Arc<dyn PluginExecutor + Send + Sync>,
    ) -> Self {
        let op_suffix = match operation {
            PluginOperation::Query => "query",
            PluginOperation::Diff => "diff",
            PluginOperation::Apply => "apply",
        };
        
        let name = format!("{}_{}", plugin_name, op_suffix);
        let description = match operation {
            PluginOperation::Query => format!("Query current state from {} plugin", plugin_name),
            PluginOperation::Diff => format!("Calculate state diff for {} plugin", plugin_name),
            PluginOperation::Apply => format!("Apply state changes for {} plugin", plugin_name),
        };
        
        Self {
            name,
            description,
            operation,
            plugin_name: plugin_name.to_string(),
            capabilities: capabilities.clone(),
            plugin_executor: executor,
        }
    }
}

#[async_trait]
impl Tool for PluginStateTool {
    fn name(&self) -> &str {
        &self.name
    }
    
    fn description(&self) -> &str {
        &self.description
    }
    
    fn input_schema(&self) -> Value {
        match self.operation {
            PluginOperation::Query => simd_json::json!({
                "type": "object",
                "properties": {
                    "filter": {
                        "type": "object",
                        "description": "Optional filter for state query"
                    }
                }
            }),
            PluginOperation::Diff => simd_json::json!({
                "type": "object",
                "properties": {
                    "desired_state": {
                        "type": "object",
                        "description": "Desired state configuration"
                    }
                },
                "required": ["desired_state"]
            }),
            PluginOperation::Apply => simd_json::json!({
                "type": "object",
                "properties": {
                    "diff": {
                        "type": "object",
                        "description": "State diff to apply"
                    },
                    "dry_run": {
                        "type": "boolean",
                        "description": "If true, only simulate changes",
                        "default": false
                    }
                },
                "required": ["diff"]
            }),
        }
    }
    
    async fn execute(&self, input: Value) -> Result<Value> {
        match self.operation {
            PluginOperation::Query => {
                let filter = input.get("filter").cloned();
                self.plugin_executor.query_state(&self.plugin_name, filter).await
            }
            PluginOperation::Diff => {
                let desired_state = input.get("desired_state")
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("Missing required field: desired_state"))?;
                self.plugin_executor.calculate_diff(&self.plugin_name, desired_state).await
            }
            PluginOperation::Apply => {
                let diff = input.get("diff")
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("Missing required field: diff"))?;
                let dry_run = input.get("dry_run")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                self.plugin_executor.apply_diff(&self.plugin_name, diff, dry_run).await
            }
        }
    }
}

/// Create a plugin state tool
pub fn create_plugin_state_tool(
    plugin_name: &str,
    description: &str,
    operation: PluginOperation,
    capabilities: &PluginCapabilities,
) -> Result<BoxedTool> {
    // Create a default executor that returns an error.
    // In production, this should be replaced with the shared plugin catalog
    // path rather than a local ad hoc plugin map.
    let executor = Arc::new(DefaultPluginExecutor::new());
    
    Ok(Arc::new(PluginStateTool::new(
        plugin_name,
        description,
        operation,
        capabilities,
        executor,
    )))
}

/// Create a plugin state tool with a custom executor
pub fn create_plugin_state_tool_with_executor(
    plugin_name: &str,
    description: &str,
    operation: PluginOperation,
    capabilities: &PluginCapabilities,
    executor: Arc<dyn PluginExecutor + Send + Sync>,
) -> Result<BoxedTool> {
    Ok(Arc::new(PluginStateTool::new(
        plugin_name,
        description,
        operation,
        capabilities,
        executor,
    )))
}

/// Default plugin executor that delegates to a local compatibility map.
///
/// This is intentionally not authoritative. Real deployments should delegate
/// to the canonical plugin catalog / plugin document path.
pub struct DefaultPluginExecutor {
    /// Local compatibility map used by older tool plumbing.
    plugins: Arc<RwLock<std::collections::HashMap<String, Arc<dyn StatePluginAdapter + Send + Sync>>>>,
}

impl DefaultPluginExecutor {
    pub fn new() -> Self {
        Self {
            plugins: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }
    
    pub async fn register_plugin(&self, name: &str, plugin: Arc<dyn StatePluginAdapter + Send + Sync>) {
        let mut plugins = self.plugins.write().await;
        plugins.insert(name.to_string(), plugin);
    }
}

impl Default for DefaultPluginExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PluginExecutor for DefaultPluginExecutor {
    async fn query_state(&self, plugin_name: &str, filter: Option<Value>) -> Result<Value> {
        let plugins = self.plugins.read().await;
        match plugins.get(plugin_name) {
            Some(plugin) => plugin.query_state(filter).await,
            None => Err(anyhow::anyhow!("Plugin not found: {}", plugin_name)),
        }
    }
    
    async fn calculate_diff(&self, plugin_name: &str, desired_state: Value) -> Result<Value> {
        let plugins = self.plugins.read().await;
        match plugins.get(plugin_name) {
            Some(plugin) => plugin.calculate_diff(desired_state).await,
            None => Err(anyhow::anyhow!("Plugin not found: {}", plugin_name)),
        }
    }
    
    async fn apply_diff(&self, plugin_name: &str, diff: Value, dry_run: bool) -> Result<Value> {
        let plugins = self.plugins.read().await;
        match plugins.get(plugin_name) {
            Some(plugin) => plugin.apply_diff(diff, dry_run).await,
            None => Err(anyhow::anyhow!("Plugin not found: {}", plugin_name)),
        }
    }
}

/// Adapter trait for StatePlugin to work with the tool system
#[async_trait]
pub trait StatePluginAdapter: Send + Sync {
    async fn query_state(&self, filter: Option<Value>) -> Result<Value>;
    async fn calculate_diff(&self, desired_state: Value) -> Result<Value>;
    async fn apply_diff(&self, diff: Value, dry_run: bool) -> Result<Value>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockPluginAdapter;

    #[async_trait]
    impl StatePluginAdapter for MockPluginAdapter {
        async fn query_state(&self, _filter: Option<Value>) -> Result<Value> {
            Ok(simd_json::json!({"packages": ["vim", "git"]}))
        }

        async fn calculate_diff(&self, desired_state: Value) -> Result<Value> {
            Ok(simd_json::json!({
                "add": desired_state.get("add").cloned().unwrap_or(Value::Null),
                "remove": []
            }))
        }

        async fn apply_diff(&self, diff: Value, dry_run: bool) -> Result<Value> {
            Ok(simd_json::json!({
                "applied": !dry_run,
                "changes": diff
            }))
        }
    }

    #[tokio::test]
    async fn test_plugin_state_tool_query() {
        let executor = Arc::new(DefaultPluginExecutor::new());
        executor.register_plugin("test", Arc::new(MockPluginAdapter)).await;

        let tool = PluginStateTool::new(
            "test",
            "Test plugin",
            PluginOperation::Query,
            &PluginCapabilities::default(),
            executor,
        );

        let result = tool.execute(simd_json::json!({})).await.unwrap();
        assert!(result.get("packages").is_some());
    }

    #[tokio::test]
    async fn test_plugin_state_tool_apply() {
        let executor = Arc::new(DefaultPluginExecutor::new());
        executor.register_plugin("test", Arc::new(MockPluginAdapter)).await;

        let tool = PluginStateTool::new(
            "test",
            "Test plugin",
            PluginOperation::Apply,
            &PluginCapabilities::default(),
            executor,
        );

        let result = tool.execute(simd_json::json!({
            "diff": {"add": ["nginx"]},
            "dry_run": true
        })).await.unwrap();
        
        assert_eq!(result.get("applied").and_then(|v| v.as_bool()), Some(false));
    }
}
