//! Built-in workflow nodes
//!
//! Provides standard nodes for common operations:
//! - Plugin nodes (wrap StatePlugins)
//! - D-Bus nodes (call D-Bus methods)
//! - Tool nodes (execute tools)
//! - Control flow nodes (conditions, loops)

pub mod dbus_node;
pub mod definitions;
pub mod plugin_node;
pub mod tool_node;

pub use dbus_node::DbusMethodNode;
pub use definitions::builtin_workflows;
pub use plugin_node::PluginNode;
pub use tool_node::ToolNode;

use anyhow::Result;
use async_trait::async_trait;
use simd_json::OwnedValue as Value;
use std::collections::HashMap;

use crate::node::{NodePort, NodeResult, NodeState, WorkflowNode};

/// Log node - logs a message
pub struct LogNode {
    id: String,
    name: String,
    message: String,
    state: NodeState,
}

impl LogNode {
    pub fn new(id: &str, message: &str) -> Self {
        Self {
            id: id.to_string(),
            name: id.to_string(),
            message: message.to_string(),
            state: NodeState::Idle,
        }
    }
}

#[async_trait]
impl WorkflowNode for LogNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }
    fn node_type(&self) -> &str {
        "log"
    }

    fn inputs(&self) -> Vec<NodePort> {
        Vec::new()
    }

    fn outputs(&self) -> Vec<NodePort> {
        vec![
            NodePort::optional("logged", "Logged", "boolean"),
            NodePort::optional("message", "Message", "string"),
        ]
    }

    fn state(&self) -> NodeState {
        self.state
    }

    fn set_state(&mut self, state: NodeState) {
        self.state = state;
    }

    async fn execute(&mut self, _inputs: HashMap<String, Value>) -> Result<NodeResult> {
        tracing::info!("{}", self.message);
        let mut outputs = HashMap::new();
        outputs.insert("logged".to_string(), Value::from(true));
        outputs.insert("message".to_string(), Value::from(self.message.clone()));
        Ok(NodeResult::success(outputs))
    }
}

/// Delay node - waits for a duration
pub struct DelayNode {
    id: String,
    name: String,
    duration_ms: u64,
    state: NodeState,
}

impl DelayNode {
    pub fn new(id: &str, duration_ms: u64) -> Self {
        Self {
            id: id.to_string(),
            name: id.to_string(),
            duration_ms,
            state: NodeState::Idle,
        }
    }
}

#[async_trait]
impl WorkflowNode for DelayNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }
    fn node_type(&self) -> &str {
        "delay"
    }

    fn inputs(&self) -> Vec<NodePort> {
        Vec::new()
    }

    fn outputs(&self) -> Vec<NodePort> {
        vec![NodePort::optional("delayed_ms", "Delayed Ms", "number")]
    }

    fn state(&self) -> NodeState {
        self.state
    }

    fn set_state(&mut self, state: NodeState) {
        self.state = state;
    }

    async fn execute(&mut self, _inputs: HashMap<String, Value>) -> Result<NodeResult> {
        tokio::time::sleep(std::time::Duration::from_millis(self.duration_ms)).await;
        let mut outputs = HashMap::new();
        outputs.insert("slept".to_string(), Value::from(self.duration_ms));
        Ok(NodeResult::success(outputs))
    }
}
