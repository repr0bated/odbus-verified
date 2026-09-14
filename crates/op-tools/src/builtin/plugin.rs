//! Plugin Tools - State plugin operations

use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use op_core::Tool;

pub struct PluginTool {
    name: String,
    description: String,
    plugin_name: String,
    operation: String,
}

impl PluginTool {
    pub fn new(name: &str, description: &str, plugin_name: &str, operation: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            plugin_name: plugin_name.to_string(),
            operation: operation.to_string(),
        }
    }
}

#[async_trait]
impl Tool for PluginTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        match self.operation.as_str() {
            "query" => json!({
                "type": "object",
                "properties": {
                    "filter": {"type": "object", "description": "Optional filter"}
                }
            }),
            "diff" => json!({
                "type": "object",
                "properties": {
                    "desired_state": {"type": "object", "description": "Desired state"}
                },
                "required": ["desired_state"]
            }),
            "apply" => json!({
                "type": "object",
                "properties": {
                    "diff": {"type": "object", "description": "State diff to apply"},
                    "dry_run": {"type": "boolean", "default": false}
                },
                "required": ["diff"]
            }),
            _ => json!({"type": "object", "properties": {}})
        }
    }

    async fn execute(&self, args: Value) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        // Use op-state's plugin system
        match self.operation.as_str() {
            "query" => {
                match op_state::query_plugin(&self.plugin_name, args).await {
                    Ok(state) => Ok(json!({
                        "plugin": self.plugin_name,
                        "state": state
                    })),
                    Err(e) => Err(format!("Query failed: {}", e).into())
                }
            }
            "diff" => {
                let desired = args.get("desired_state").cloned().unwrap_or(json!({}));
                match op_state::diff_plugin(&self.plugin_name, desired).await {
                    Ok(diff) => Ok(json!({
                        "plugin": self.plugin_name,
                        "diff": diff
                    })),
                    Err(e) => Err(format!("Diff failed: {}", e).into())
                }
            }
            "apply" => {
                let diff = args.get("diff").cloned().unwrap_or(json!({}));
                let dry_run = args.get("dry_run").and_then(|d| d.as_bool()).unwrap_or(false);
                match op_state::apply_plugin(&self.plugin_name, diff, dry_run).await {
                    Ok(result) => Ok(json!({
                        "plugin": self.plugin_name,
                        "applied": !dry_run,
                        "result": result
                    })),
                    Err(e) => Err(format!("Apply failed: {}", e).into())
                }
            }
            _ => Ok(json!({"error": "Unknown operation"}))
        }
    }
}
