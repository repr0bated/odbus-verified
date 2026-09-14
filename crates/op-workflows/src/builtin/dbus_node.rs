//! D-Bus Method Node - Calls a D-Bus method as a workflow node

use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;

use crate::node::{NodePort, NodeResult, NodeState, WorkflowNode};

/// A workflow node that calls a D-Bus method
pub struct DbusMethodNode {
    id: String,
    name: String,
    service: String,
    path: String,
    interface: String,
    method: String,
    state: NodeState,
}

impl DbusMethodNode {
    /// Create a new D-Bus method node
    pub fn new(id: &str, service: &str, path: &str, interface: &str, method: &str) -> Self {
        Self {
            id: id.to_string(),
            name: format!(
                "{}.{}",
                interface.split('.').next_back().unwrap_or(interface),
                method
            ),
            service: service.to_string(),
            path: path.to_string(),
            interface: interface.to_string(),
            method: method.to_string(),
            state: NodeState::Idle,
        }
    }
}

#[async_trait]
impl WorkflowNode for DbusMethodNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn node_type(&self) -> &str {
        "dbus-method"
    }

    fn inputs(&self) -> Vec<NodePort> {
        vec![NodePort::optional("args", "Arguments", "array")
            .with_description("Arguments to pass to the D-Bus method")]
    }

    fn outputs(&self) -> Vec<NodePort> {
        vec![NodePort::required("result", "Result", "object")
            .with_description("Result from the D-Bus method call")]
    }

    fn state(&self) -> NodeState {
        self.state
    }

    fn set_state(&mut self, state: NodeState) {
        self.state = state;
    }

    async fn execute(&mut self, inputs: HashMap<String, Value>) -> Result<NodeResult> {
        let start = std::time::Instant::now();
        let args = inputs.get("args").cloned().unwrap_or(json!([]));

        // In a real implementation, this would call the D-Bus method
        // For now, return mock data
        let mut outputs = HashMap::new();
        outputs.insert(
            "result".to_string(),
            json!({
                "service": self.service,
                "path": self.path,
                "interface": self.interface,
                "method": self.method,
                "args": args,
                "response": null,
                "success": true
            }),
        );

        Ok(NodeResult::success(outputs).with_duration(start.elapsed().as_millis() as u64))
    }

    fn config_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "service": {
                    "type": "string",
                    "description": "D-Bus service name",
                    "default": self.service
                },
                "path": {
                    "type": "string",
                    "description": "D-Bus object path",
                    "default": self.path
                },
                "interface": {
                    "type": "string",
                    "description": "D-Bus interface name",
                    "default": self.interface
                },
                "method": {
                    "type": "string",
                    "description": "D-Bus method name",
                    "default": self.method
                }
            },
            "required": ["service", "path", "interface", "method"]
        })
    }
}
