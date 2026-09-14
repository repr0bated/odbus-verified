//! Workflow Engine - Executes workflow graphs
//!
//! The engine manages workflow execution:
//! - Determines execution order based on dependencies
//! - Manages parallel execution of independent nodes
//! - Handles errors and retries
//! - Collects results

use crate::flow::{Workflow, WorkflowDefinition, WorkflowState};
use crate::node::{NodeResult, NodeState, WorkflowNode};
use anyhow::Result;
use simd_json::OwnedValue as Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Workflow execution result
#[derive(Debug, Clone)]
pub struct WorkflowExecutionResult {
    /// Whether workflow completed successfully
    pub success: bool,
    /// Workflow outputs
    pub outputs: HashMap<String, Value>,
    /// Error message if failed
    pub error: Option<String>,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
    /// Node execution results
    pub node_results: HashMap<String, NodeResult>,
}

/// Factory for creating node instances from definitions
pub trait NodeFactory: Send + Sync {
    /// Create a node instance from a node type and config
    fn create_node(
        &self,
        node_type: &str,
        node_id: &str,
        config: &Value,
    ) -> Result<Box<dyn WorkflowNode>>;
}

/// Workflow Engine - Executes workflows
pub struct WorkflowEngine {
    /// Registered workflow definitions
    definitions: Arc<RwLock<HashMap<String, WorkflowDefinition>>>,
    /// Node factory for creating node instances
    node_factory: Arc<dyn NodeFactory>,
    /// Maximum parallel nodes
    max_parallel: usize,
}

impl WorkflowEngine {
    /// Create a new workflow engine
    pub fn new(node_factory: Arc<dyn NodeFactory>) -> Self {
        Self {
            definitions: Arc::new(RwLock::new(HashMap::new())),
            node_factory,
            max_parallel: 10,
        }
    }

    /// Set maximum parallel node executions
    pub fn with_max_parallel(mut self, max: usize) -> Self {
        self.max_parallel = max;
        self
    }

    /// Register a workflow definition
    pub async fn register(&self, definition: WorkflowDefinition) -> Result<()> {
        definition.validate()?;
        let mut defs = self.definitions.write().await;
        info!(workflow_id = %definition.id, "Registering workflow");
        defs.insert(definition.id.clone(), definition);
        Ok(())
    }

    /// Get a workflow definition
    pub async fn get_definition(&self, workflow_id: &str) -> Option<WorkflowDefinition> {
        let defs = self.definitions.read().await;
        defs.get(workflow_id).cloned()
    }

    /// List all workflow definitions
    pub async fn list_definitions(&self) -> Vec<WorkflowDefinition> {
        let defs = self.definitions.read().await;
        defs.values().cloned().collect()
    }

    /// Execute a workflow by ID
    pub async fn execute(
        &self,
        workflow_id: &str,
        inputs: HashMap<String, Value>,
    ) -> Result<WorkflowExecutionResult> {
        let definition = self
            .get_definition(workflow_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("Workflow not found: {}", workflow_id))?;

        self.execute_definition(definition, inputs).await
    }

    /// Execute a workflow definition
    pub async fn execute_definition(
        &self,
        definition: WorkflowDefinition,
        inputs: HashMap<String, Value>,
    ) -> Result<WorkflowExecutionResult> {
        let start = std::time::Instant::now();
        let workflow_id = definition.id.clone();

        info!(workflow_id = %workflow_id, "Starting workflow execution");

        // Create workflow instance
        let mut workflow = Workflow::new(definition.clone());
        workflow.state = WorkflowState::Running;

        // Set initial variables from inputs
        for (key, value) in inputs {
            workflow.variables.insert(key, value);
        }

        // Create node instances
        let mut nodes: HashMap<String, Box<dyn WorkflowNode>> = HashMap::new();
        for node_def in &definition.nodes {
            match self
                .node_factory
                .create_node(&node_def.node_type, &node_def.id, &node_def.config)
            {
                Ok(node) => {
                    nodes.insert(node_def.id.clone(), node);
                }
                Err(e) => {
                    error!(node_id = %node_def.id, error = %e, "Failed to create node");
                    return Ok(WorkflowExecutionResult {
                        success: false,
                        outputs: HashMap::new(),
                        error: Some(format!("Failed to create node '{}': {}", node_def.id, e)),
                        duration_ms: start.elapsed().as_millis() as u64,
                        node_results: HashMap::new(),
                    });
                }
            }
        }

        let mut node_results: HashMap<String, NodeResult> = HashMap::new();

        // Execute nodes in dependency order
        loop {
            // Check for completion
            if workflow.is_complete() {
                workflow.state = WorkflowState::Completed;
                break;
            }

            // Check for failure
            if workflow.has_failed() {
                workflow.state = WorkflowState::Failed;
                break;
            }

            // Get ready nodes
            let ready_nodes = workflow.get_ready_nodes();
            if ready_nodes.is_empty() {
                // No nodes ready but not complete - deadlock or all failed
                warn!(workflow_id = %workflow_id, "No nodes ready to execute");
                workflow.state = WorkflowState::Failed;
                break;
            }

            // Execute ready nodes (in parallel up to max_parallel)
            let batch: Vec<_> = ready_nodes.into_iter().take(self.max_parallel).collect();

            for node_id in batch {
                debug!(workflow_id = %workflow_id, node_id = %node_id, "Executing node");

                // Get inputs for this node
                let node_inputs = workflow.get_node_inputs(&node_id);

                // Get node instance
                if let Some(node) = nodes.get_mut(&node_id) {
                    // Update state
                    node.set_state(NodeState::Running);
                    workflow
                        .node_states
                        .insert(node_id.clone(), NodeState::Running);

                    // Execute
                    match node.execute(node_inputs).await {
                        Ok(result) => {
                            if result.success {
                                workflow.complete_node(&node_id, result.outputs.clone());
                                node.set_state(NodeState::Completed);
                            } else {
                                let error = result.error.clone().unwrap_or_default();
                                workflow.fail_node(&node_id, &error);
                                node.set_state(NodeState::Failed);
                            }
                            node_results.insert(node_id.clone(), result);
                        }
                        Err(e) => {
                            error!(node_id = %node_id, error = %e, "Node execution error");
                            workflow.fail_node(&node_id, &e.to_string());
                            node.set_state(NodeState::Failed);
                            node_results
                                .insert(node_id.clone(), NodeResult::failure(e.to_string()));
                        }
                    }
                }
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;
        let success = workflow.state == WorkflowState::Completed;

        info!(
            workflow_id = %workflow_id,
            success = success,
            duration_ms = duration_ms,
            "Workflow execution complete"
        );

        Ok(WorkflowExecutionResult {
            success,
            outputs: workflow.get_outputs(),
            error: if success {
                None
            } else {
                Some("Workflow execution failed".to_string())
            },
            duration_ms,
            node_results,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockNodeFactory;

    impl NodeFactory for MockNodeFactory {
        fn create_node(
            &self,
            _node_type: &str,
            _node_id: &str,
            _config: &Value,
        ) -> Result<Box<dyn WorkflowNode>> {
            Err(anyhow::anyhow!("Mock factory"))
        }
    }

    #[tokio::test]
    async fn test_workflow_registration() {
        let factory = Arc::new(MockNodeFactory);
        let engine = WorkflowEngine::new(factory);

        let def = WorkflowDefinition::new("test", "Test Workflow", "A test workflow");
        engine.register(def).await.unwrap();

        assert!(engine.get_definition("test").await.is_some());
    }
}
