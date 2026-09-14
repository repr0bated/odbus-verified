//! Workflow Flow - Graph of connected nodes
//!
//! A Workflow is a directed graph of nodes connected by edges.
//! Data flows from output ports to input ports.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::collections::HashMap;

use crate::node::{NodeConnection, NodePort, NodeState};

/// Workflow definition (serializable)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    /// Unique identifier
    pub id: String,
    /// Display name
    pub name: String,
    /// Description
    pub description: String,
    /// Category for organization
    pub category: String,
    /// Node definitions
    pub nodes: Vec<WorkflowNodeDef>,
    /// Connections between nodes
    pub connections: Vec<NodeConnection>,
    /// Input parameters for the workflow
    pub inputs: Vec<NodePort>,
    /// Output parameters from the workflow
    pub outputs: Vec<NodePort>,
    /// Tags for discovery
    pub tags: Vec<String>,
    /// Version
    pub version: String,
}

/// Node definition within a workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowNodeDef {
    /// Node ID (unique within workflow)
    pub id: String,
    /// Node type (e.g., "plugin:systemd", "dbus:org.freedesktop.systemd1")
    pub node_type: String,
    /// Display name
    pub name: String,
    /// Configuration for this node instance
    pub config: Value,
    /// Position for visual layout (optional)
    pub position: Option<(f32, f32)>,
}

/// Runtime workflow instance
pub struct Workflow {
    /// Definition
    pub definition: WorkflowDefinition,
    /// Current state of each node
    pub node_states: HashMap<String, NodeState>,
    /// Collected outputs from completed nodes
    pub node_outputs: HashMap<String, HashMap<String, Value>>,
    /// Workflow-level variables
    pub variables: HashMap<String, Value>,
    /// Overall workflow state
    pub state: WorkflowState,
}

/// Overall workflow state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowState {
    /// Not started
    #[default]
    Idle,
    /// Currently executing
    Running,
    /// Paused
    Paused,
    /// Completed successfully
    Completed,
    /// Failed
    Failed,
    /// Cancelled
    Cancelled,
}

impl Workflow {
    /// Create a new workflow from definition
    pub fn new(definition: WorkflowDefinition) -> Self {
        let mut node_states = HashMap::new();
        for node in &definition.nodes {
            node_states.insert(node.id.clone(), NodeState::Idle);
        }

        Self {
            definition,
            node_states,
            node_outputs: HashMap::new(),
            variables: HashMap::new(),
            state: WorkflowState::Idle,
        }
    }

    /// Get nodes that are ready to execute (all inputs satisfied)
    pub fn get_ready_nodes(&self) -> Vec<String> {
        let mut ready = Vec::new();

        for node_def in &self.definition.nodes {
            // Skip if not idle
            if self.node_states.get(&node_def.id) != Some(&NodeState::Idle) {
                continue;
            }

            // Check if all input connections are satisfied
            let inputs_satisfied = self.are_inputs_satisfied(&node_def.id);
            if inputs_satisfied {
                ready.push(node_def.id.clone());
            }
        }

        ready
    }

    /// Check if all inputs for a node are satisfied
    fn are_inputs_satisfied(&self, node_id: &str) -> bool {
        // Find all connections targeting this node
        for conn in &self.definition.connections {
            if conn.to_node == node_id {
                // Check if source node has completed
                if self.node_states.get(&conn.from_node) != Some(&NodeState::Completed) {
                    return false;
                }
                // Check if source output exists
                if let Some(outputs) = self.node_outputs.get(&conn.from_node) {
                    if !outputs.contains_key(&conn.from_port) {
                        return false;
                    }
                } else {
                    return false;
                }
            }
        }
        true
    }

    /// Get inputs for a node from connected outputs
    pub fn get_node_inputs(&self, node_id: &str) -> HashMap<String, Value> {
        let mut inputs = HashMap::new();

        for conn in &self.definition.connections {
            if conn.to_node == node_id {
                if let Some(outputs) = self.node_outputs.get(&conn.from_node) {
                    if let Some(value) = outputs.get(&conn.from_port) {
                        inputs.insert(conn.to_port.clone(), value.clone());
                    }
                }
            }
        }

        inputs
    }

    /// Mark a node as completed with outputs
    pub fn complete_node(&mut self, node_id: &str, outputs: HashMap<String, Value>) {
        self.node_states
            .insert(node_id.to_string(), NodeState::Completed);
        self.node_outputs.insert(node_id.to_string(), outputs);
    }

    /// Mark a node as failed
    pub fn fail_node(&mut self, node_id: &str, _error: &str) {
        self.node_states
            .insert(node_id.to_string(), NodeState::Failed);
    }

    /// Check if workflow is complete
    pub fn is_complete(&self) -> bool {
        self.definition.nodes.iter().all(|n| {
            matches!(
                self.node_states.get(&n.id),
                Some(NodeState::Completed) | Some(NodeState::Skipped)
            )
        })
    }

    /// Check if workflow has failed
    pub fn has_failed(&self) -> bool {
        self.definition
            .nodes
            .iter()
            .any(|n| self.node_states.get(&n.id) == Some(&NodeState::Failed))
    }

    /// Get workflow outputs (from designated output nodes)
    pub fn get_outputs(&self) -> HashMap<String, Value> {
        // Collect outputs from nodes that connect to workflow outputs
        let mut result = HashMap::new();

        // For now, collect all outputs from all completed nodes
        for (node_id, outputs) in &self.node_outputs {
            for (port_id, value) in outputs {
                result.insert(format!("{}.{}", node_id, port_id), value.clone());
            }
        }

        result
    }
}

impl WorkflowDefinition {
    /// Create a new workflow definition
    pub fn new(id: &str, name: &str, description: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            category: "general".to_string(),
            nodes: Vec::new(),
            connections: Vec::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            tags: Vec::new(),
            version: "1.0.0".to_string(),
        }
    }

    /// Add a node
    pub fn with_node(mut self, node: WorkflowNodeDef) -> Self {
        self.nodes.push(node);
        self
    }

    /// Add a connection
    pub fn with_connection(mut self, conn: NodeConnection) -> Self {
        self.connections.push(conn);
        self
    }

    /// Validate the workflow definition
    pub fn validate(&self) -> Result<()> {
        // Check for duplicate node IDs
        let mut seen_ids = std::collections::HashSet::new();
        for node in &self.nodes {
            if !seen_ids.insert(&node.id) {
                return Err(anyhow::anyhow!("Duplicate node ID: {}", node.id));
            }
        }

        // Check connections reference valid nodes
        for conn in &self.connections {
            if !seen_ids.contains(&conn.from_node) {
                return Err(anyhow::anyhow!(
                    "Connection references unknown source node: {}",
                    conn.from_node
                ));
            }
            if !seen_ids.contains(&conn.to_node) {
                return Err(anyhow::anyhow!(
                    "Connection references unknown target node: {}",
                    conn.to_node
                ));
            }
        }

        // Check for cycles (simple DFS)
        // TODO: Implement proper cycle detection

        Ok(())
    }
}
