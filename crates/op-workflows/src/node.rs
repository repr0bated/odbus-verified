//! Workflow Node - Plugin/Service as a workflow node
//!
//! Nodes are the fundamental building blocks of workflows.
//! Each node represents a plugin, service, or D-Bus method that can:
//! - Receive data through input ports
//! - Execute some operation
//! - Produce data through output ports

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::collections::HashMap;

/// State of a workflow node
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    /// Node is idle, waiting to be executed
    #[default]
    Idle,
    /// Node is waiting for input data
    WaitingForInput,
    /// Node is currently executing
    Running,
    /// Node completed successfully
    Completed,
    /// Node failed
    Failed,
    /// Node was skipped (condition not met)
    Skipped,
}

/// Result of node execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeResult {
    /// Whether execution succeeded
    pub success: bool,
    /// Output data keyed by port name
    pub outputs: HashMap<String, Value>,
    /// Error message if failed
    pub error: Option<String>,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
    /// Additional metadata
    pub metadata: HashMap<String, Value>,
}

impl NodeResult {
    /// Create a successful result
    pub fn success(outputs: HashMap<String, Value>) -> Self {
        Self {
            success: true,
            outputs,
            error: None,
            duration_ms: 0,
            metadata: HashMap::new(),
        }
    }

    /// Create a failed result
    pub fn failure(error: impl Into<String>) -> Self {
        Self {
            success: false,
            outputs: HashMap::new(),
            error: Some(error.into()),
            duration_ms: 0,
            metadata: HashMap::new(),
        }
    }

    /// Set duration
    pub fn with_duration(mut self, duration_ms: u64) -> Self {
        self.duration_ms = duration_ms;
        self
    }
}

/// A port on a workflow node (input or output)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePort {
    /// Port identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// Data type (e.g., "string", "number", "object", "state")
    pub data_type: String,
    /// Whether this port is required
    pub required: bool,
    /// Description
    pub description: Option<String>,
    /// Default value if not connected
    pub default_value: Option<Value>,
}

impl NodePort {
    /// Create a new required port
    pub fn required(id: &str, name: &str, data_type: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            data_type: data_type.to_string(),
            required: true,
            description: None,
            default_value: None,
        }
    }

    /// Create a new optional port
    pub fn optional(id: &str, name: &str, data_type: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            data_type: data_type.to_string(),
            required: false,
            description: None,
            default_value: None,
        }
    }

    /// Add description
    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = Some(desc.to_string());
        self
    }

    /// Add default value
    pub fn with_default(mut self, value: Value) -> Self {
        self.default_value = Some(value);
        self
    }
}

/// Trait for workflow nodes
#[async_trait]
pub trait WorkflowNode: Send + Sync {
    /// Get the node's unique identifier
    fn id(&self) -> &str;

    /// Get the node's display name
    fn name(&self) -> &str;

    /// Get the node type (plugin, service, dbus-method, etc.)
    fn node_type(&self) -> &str;

    /// Get input ports
    fn inputs(&self) -> Vec<NodePort>;

    /// Get output ports
    fn outputs(&self) -> Vec<NodePort>;

    /// Get current state
    fn state(&self) -> NodeState;

    /// Set state
    fn set_state(&mut self, state: NodeState);

    /// Execute the node with given inputs
    async fn execute(&mut self, inputs: HashMap<String, Value>) -> Result<NodeResult>;

    /// Validate inputs before execution
    fn validate_inputs(&self, inputs: &HashMap<String, Value>) -> Result<()> {
        for port in self.inputs() {
            if port.required && !inputs.contains_key(&port.id) && port.default_value.is_none() {
                return Err(anyhow::anyhow!(
                    "Required input '{}' not provided for node '{}'",
                    port.id,
                    self.id()
                ));
            }
        }
        Ok(())
    }

    /// Get configuration schema (JSON Schema)
    fn config_schema(&self) -> Value {
        simd_json::json!({
            "type": "object",
            "properties": {}
        })
    }
}

/// A connection between two nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConnection {
    /// Source node ID
    pub from_node: String,
    /// Source port ID
    pub from_port: String,
    /// Target node ID
    pub to_node: String,
    /// Target port ID
    pub to_port: String,
}

impl NodeConnection {
    /// Create a new connection
    pub fn new(from_node: &str, from_port: &str, to_node: &str, to_port: &str) -> Self {
        Self {
            from_node: from_node.to_string(),
            from_port: from_port.to_string(),
            to_node: to_node.to_string(),
            to_port: to_port.to_string(),
        }
    }
}
