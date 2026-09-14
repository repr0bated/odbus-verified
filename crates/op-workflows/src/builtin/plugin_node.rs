//! Plugin Node - Wraps a StatePlugin as a workflow node
//!
//! Converts StatePlugins into workflow nodes with:
//! - Input: desired_state
//! - Outputs: current_state, diff, apply_result

use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;

use crate::node::{NodePort, NodeResult, NodeState, WorkflowNode};

/// A workflow node that wraps a StatePlugin
pub struct PluginNode {
    id: String,
    name: String,
    plugin_name: String,
    state: NodeState,
    operation: PluginOperation,
}

/// Operation to perform on the plugin
#[derive(Debug, Clone, Copy)]
pub enum PluginOperation {
    /// Query current state
    Query,
    /// Calculate diff between current and desired
    Diff,
    /// Apply desired state
    Apply,
}

impl PluginNode {
    /// Create a new plugin node
    pub fn new(id: &str, plugin_name: &str, operation: PluginOperation) -> Self {
        let op_name = match operation {
            PluginOperation::Query => "Query",
            PluginOperation::Diff => "Diff",
            PluginOperation::Apply => "Apply",
        };

        Self {
            id: id.to_string(),
            name: format!("{} {}", plugin_name, op_name),
            plugin_name: plugin_name.to_string(),
            state: NodeState::Idle,
            operation,
        }
    }
}

#[async_trait]
impl WorkflowNode for PluginNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "plugin"
    }

    fn inputs(&self) -> Vec<NodePort> {
        match self.operation {
            PluginOperation::Query => vec![],
            PluginOperation::Diff | PluginOperation::Apply => {
                vec![
                    NodePort::optional("desired_state", "Desired State", "object")
                        .with_description("The desired state to diff/apply"),
                ]
            }
        }
    }

    fn outputs(&self) -> Vec<NodePort> {
        match self.operation {
            PluginOperation::Query => {
                vec![
                    NodePort::required("current_state", "Current State", "object")
                        .with_description("The current system state"),
                ]
            }
            PluginOperation::Diff => vec![NodePort::required("diff", "State Diff", "object")
                .with_description("Difference between current and desired state")],
            PluginOperation::Apply => vec![NodePort::required("result", "Apply Result", "object")
                .with_description("Result of applying the state")],
        }
    }

    fn state(&self) -> NodeState {
        self.state
    }

    fn set_state(&mut self, state: NodeState) {
        self.state = state;
    }

    async fn execute(&mut self, inputs: HashMap<String, Value>) -> Result<NodeResult> {
        let start = std::time::Instant::now();

        // In a real implementation, this would call the actual plugin
        // For now, return mock data
        let result = match self.operation {
            PluginOperation::Query => {
                let mut outputs = HashMap::new();
                outputs.insert(
                    "current_state".to_string(),
                    json!({
                        "plugin": self.plugin_name,
                        "state": "queried",
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }),
                );
                NodeResult::success(outputs)
            }
            PluginOperation::Diff => {
                let desired = inputs.get("desired_state").cloned().unwrap_or(json!({}));
                let mut outputs = HashMap::new();
                outputs.insert(
                    "diff".to_string(),
                    json!({
                        "plugin": self.plugin_name,
                        "desired": desired,
                        "changes": [],
                        "has_changes": false
                    }),
                );
                NodeResult::success(outputs)
            }
            PluginOperation::Apply => {
                let mut outputs = HashMap::new();
                outputs.insert(
                    "result".to_string(),
                    json!({
                        "plugin": self.plugin_name,
                        "applied": true,
                        "changes_made": 0
                    }),
                );
                NodeResult::success(outputs)
            }
        };

        Ok(result.with_duration(start.elapsed().as_millis() as u64))
    }

    fn config_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "plugin_name": {
                    "type": "string",
                    "description": "Name of the plugin",
                    "default": self.plugin_name
                },
                "operation": {
                    "type": "string",
                    "enum": ["query", "diff", "apply"],
                    "description": "Operation to perform"
                }
            }
        })
    }
}
