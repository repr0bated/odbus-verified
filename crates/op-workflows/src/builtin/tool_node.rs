//! Tool Node - Executes a tool as a workflow node

use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;

use crate::node::{NodePort, NodeResult, NodeState, WorkflowNode};

/// A workflow node that executes a tool
pub struct ToolNode {
    id: String,
    name: String,
    tool_name: String,
    state: NodeState,
}

impl ToolNode {
    /// Create a new tool node
    pub fn new(id: &str, tool_name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: format!("Tool: {}", tool_name),
            tool_name: tool_name.to_string(),
            state: NodeState::Idle,
        }
    }
}

#[async_trait]
impl WorkflowNode for ToolNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "tool"
    }

    fn inputs(&self) -> Vec<NodePort> {
        vec![NodePort::optional("arguments", "Arguments", "object")
            .with_description("Arguments to pass to the tool")]
    }

    fn outputs(&self) -> Vec<NodePort> {
        vec![NodePort::required("result", "Result", "object")
            .with_description("Result from tool execution")]
    }

    fn state(&self) -> NodeState {
        self.state
    }

    fn set_state(&mut self, state: NodeState) {
        self.state = state;
    }

    async fn execute(&mut self, inputs: HashMap<String, Value>) -> Result<NodeResult> {
        let start = std::time::Instant::now();
        let arguments = inputs.get("arguments").cloned().unwrap_or(json!({}));

        // In a real implementation, this would execute the tool via ToolRegistry
        // For now, return mock data
        let mut outputs = HashMap::new();
        outputs.insert(
            "result".to_string(),
            json!({
                "tool": self.tool_name,
                "arguments": arguments,
                "output": null,
                "success": true
            }),
        );

        Ok(NodeResult::success(outputs).with_duration(start.elapsed().as_millis() as u64))
    }

    fn config_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tool_name": {
                    "type": "string",
                    "description": "Name of the tool to execute",
                    "default": self.tool_name
                }
            },
            "required": ["tool_name"]
        })
    }
}
