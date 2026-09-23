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

    async fn execute(&mut self, _inputs: HashMap<String, Value>) -> Result<NodeResult> {
        let start = std::time::Instant::now();
        Ok(NodeResult::failure(format!(
            "tool node '{}' has no authorized ToolRegistry executor",
            self.tool_name
        ))
        .with_duration(start.elapsed().as_millis() as u64))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtin::dbus_node::DbusMethodNode;
    use crate::builtin::plugin_node::{PluginNode, PluginOperation};

    #[tokio::test]
    async fn unconfigured_execution_nodes_fail_instead_of_claiming_work() {
        let mut nodes: Vec<Box<dyn WorkflowNode>> = vec![
            Box::new(ToolNode::new("tool", "cargo_build")),
            Box::new(PluginNode::new("plugin", "network", PluginOperation::Apply)),
            Box::new(DbusMethodNode::new(
                "dbus",
                "org.example",
                "/x",
                "org.example.X",
                "Apply",
            )),
        ];
        for node in &mut nodes {
            let result = node.execute(HashMap::new()).await.unwrap();
            assert!(!result.success, "{} claimed success", node.name());
            assert!(result.outputs.is_empty());
            assert!(result
                .error
                .as_deref()
                .is_some_and(|message| message.contains("no authorized")));
        }
    }
}
