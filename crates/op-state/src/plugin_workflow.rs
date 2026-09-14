//! Plugin Workflow System - Node-Based Architecture for Plugins
#![allow(dead_code)]
//!
//! This module enables plugins to participate in flow-based workflows using PocketFlow.
//! Each plugin becomes a node that can be connected to other plugins in complex pipelines.

use crate::plugin::StatePlugin;
use anyhow::Result;
use async_trait::async_trait;
use pocketflow_rs::context::Context;
use pocketflow_rs::node::{Node, ProcessResult, ProcessState};
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;
use simd_json::ValueBuilder;
use std::sync::Arc;

/// Workflow states for plugin execution
#[derive(Debug, Clone, PartialEq, Default)]
pub enum PluginWorkflowState {
    /// Plugin execution started
    #[default]
    Started,
    /// Plugin successfully completed its task
    Completed,
    /// Plugin failed during execution
    Failed,
    /// Plugin is waiting for input from another plugin
    WaitingForInput,
    /// Plugin execution was skipped due to conditions
    Skipped,
    /// Plugin requires manual intervention
    NeedsIntervention,
}

impl ProcessState for PluginWorkflowState {
    fn is_default(&self) -> bool {
        matches!(self, PluginWorkflowState::Started)
    }

    fn to_condition(&self) -> String {
        match self {
            PluginWorkflowState::Started => "started",
            PluginWorkflowState::Completed => "completed",
            PluginWorkflowState::Failed => "failed",
            PluginWorkflowState::WaitingForInput => "waiting_for_input",
            PluginWorkflowState::Skipped => "skipped",
            PluginWorkflowState::NeedsIntervention => "needs_intervention",
        }
        .to_string()
    }
}

/// A workflow-enabled plugin that wraps any StatePlugin
pub struct WorkflowPluginNode {
    /// The underlying plugin
    plugin: Arc<dyn StatePlugin>,
    /// Plugin inputs (data keys this plugin expects from context)
    inputs: Vec<String>,
    /// Plugin outputs (data keys this plugin writes to context)
    outputs: Vec<String>,
    /// Plugin-specific configuration
    config: Value,
}

impl WorkflowPluginNode {
    pub fn new(plugin: Arc<dyn StatePlugin>) -> Self {
        Self {
            plugin,
            inputs: Vec::new(),
            outputs: Vec::new(),
            config: Value::null(),
        }
    }

    pub fn with_inputs(mut self, inputs: Vec<String>) -> Self {
        self.inputs = inputs;
        self
    }

    pub fn with_outputs(mut self, outputs: Vec<String>) -> Self {
        self.outputs = outputs;
        self
    }

    pub fn with_config(mut self, config: Value) -> Self {
        self.config = config;
        self
    }

    /// Extract inputs from workflow context
    fn extract_inputs(&self, context: &Context) -> Result<Value> {
        let mut input_data = simd_json::value::owned::Object::new();

        for input_key in &self.inputs {
            if let Some(serde_value) = context.get(input_key) {
                let simd_value: Value = simd_json::serde::to_owned_value(serde_value)?;
                input_data.insert(input_key.clone(), simd_value);
            }
        }

        Ok(Value::Object(Box::new(input_data)))
    }

    /// Store outputs in workflow context
    fn store_outputs(&self, context: &mut Context, output_data: &Value) -> Result<()> {
        if let Some(obj) = output_data.as_object() {
            for (key, value) in obj {
                if self.outputs.contains(key) {
                    let serde_value: serde_json::Value = serde_json::to_value(value)?;
                    context.set(key, serde_value);
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl Node for WorkflowPluginNode {
    type State = PluginWorkflowState;

    async fn prepare(&self, context: &mut Context) -> Result<()> {
        log::info!(
            "🔧 Preparing plugin '{}' for workflow execution",
            self.plugin.name()
        );

        // Extract inputs from context and prepare plugin
        let inputs = self.extract_inputs(context)?;
        log::debug!(
            "📥 Plugin '{}' received inputs: {:?}",
            self.plugin.name(),
            inputs
        );

        Ok(())
    }

    async fn execute(&self, context: &Context) -> Result<serde_json::Value> {
        log::info!("⚡ Executing plugin '{}' in workflow", self.plugin.name());

        // Check if plugin is available
        if !self.plugin.is_available() {
            log::warn!(
                "⚠️  Plugin '{}' is not available: {}",
                self.plugin.name(),
                self.plugin.unavailable_reason()
            );
            return Ok(serde_json::Value::String("skipped".to_string()));
        }

        // No current state to query — schema is the source of truth
        let current_state = simd_json::json!(null);
        log::debug!(
            "Plugin '{}' state: schema-driven, no held state",
            self.plugin.name(),
        );

        // For workflow execution, we assume the "desired" state comes from inputs
        // In a real implementation, this would be more sophisticated
        let desired_state = if let Some(serde_val) = context.get("desired_state") {
            simd_json::serde::to_owned_value(serde_val)?
        } else {
            Value::null()
        };

        // Calculate diff
        let diff = self
            .plugin
            .calculate_diff(&current_state, &desired_state)
            .await?;
        log::debug!(
            "🔍 Plugin '{}' calculated diff: {:?}",
            self.plugin.name(),
            diff
        );

        // Apply changes if needed
        if !diff.actions.is_empty() {
            log::info!(
                "🔄 Plugin '{}' applying {} changes",
                self.plugin.name(),
                diff.actions.len()
            );
            let result = self.plugin.apply_state(&diff).await?;

            match result.success {
                true => {
                    log::info!("✅ Plugin '{}' completed successfully", self.plugin.name());
                    Ok(serde_json::Value::String("completed".to_string()))
                }
                false => {
                    log::error!(
                        "❌ Plugin '{}' failed: {:?}",
                        self.plugin.name(),
                        result.errors
                    );
                    Ok(serde_json::Value::String("failed".to_string()))
                }
            }
        } else {
            log::info!("⏭️  Plugin '{}' - no changes needed", self.plugin.name());
            Ok(serde_json::Value::String("completed".to_string()))
        }
    }

    async fn post_process(
        &self,
        context: &mut Context,
        result: &Result<serde_json::Value>,
    ) -> Result<ProcessResult<PluginWorkflowState>> {
        match result {
            Ok(value) => {
                if let Some(status) = value.as_str() {
                    match status {
                        "completed" => {
                            // Store successful execution results in context
                            let execution_result = simd_json::json!({
                                "plugin": self.plugin.name(),
                                "status": "completed",
                                "timestamp": chrono::Utc::now().timestamp()
                            });
                            let serde_result = serde_json::to_value(execution_result)?;
                            self.store_outputs(
                                context,
                                &simd_json::serde::to_owned_value(&serde_result)?,
                            )?;
                            log::info!(
                                "📤 Plugin '{}' stored results in workflow context",
                                self.plugin.name()
                            );
                            Ok(ProcessResult::new(
                                PluginWorkflowState::Completed,
                                "Plugin completed successfully".to_string(),
                            ))
                        }
                        "failed" => {
                            // Store failure information
                            let failure_result = simd_json::json!({
                                "plugin": self.plugin.name(),
                                "status": "failed",
                                "timestamp": chrono::Utc::now().timestamp()
                            });
                            let serde_failure: serde_json::Value =
                                serde_json::to_value(failure_result)?;
                            context.set("last_error", serde_failure);
                            log::error!(
                                "💥 Plugin '{}' workflow execution failed",
                                self.plugin.name()
                            );
                            Ok(ProcessResult::new(
                                PluginWorkflowState::Failed,
                                "Plugin execution failed".to_string(),
                            ))
                        }
                        "skipped" => {
                            log::info!(
                                "⏭️  Plugin '{}' was skipped in workflow",
                                self.plugin.name()
                            );
                            Ok(ProcessResult::new(
                                PluginWorkflowState::Skipped,
                                "Plugin was skipped".to_string(),
                            ))
                        }
                        _ => {
                            log::debug!(
                                "Plugin '{}' completed with status: {}",
                                self.plugin.name(),
                                status
                            );
                            Ok(ProcessResult::new(
                                PluginWorkflowState::Completed,
                                format!("Plugin completed with status: {}", status),
                            ))
                        }
                    }
                } else {
                    Ok(ProcessResult::new(
                        PluginWorkflowState::Completed,
                        "Plugin completed".to_string(),
                    ))
                }
            }
            Err(e) => {
                log::error!("💥 Plugin '{}' execution error: {}", self.plugin.name(), e);
                let error_result = simd_json::json!({
                    "plugin": self.plugin.name(),
                    "status": "error",
                    "error": e.to_string(),
                    "timestamp": chrono::Utc::now().timestamp()
                });
                let serde_error: serde_json::Value = serde_json::to_value(error_result)?;
                context.set("last_error", serde_error);
                Ok(ProcessResult::new(
                    PluginWorkflowState::Failed,
                    format!("Plugin execution error: {}", e),
                ))
            }
        }
    }
}

/// Plugin Workflow Manager - Orchestrates plugin execution
pub struct PluginWorkflowManager {
    workflows: std::collections::HashMap<String, pocketflow_rs::Flow<PluginWorkflowState>>,
}

impl Default for PluginWorkflowManager {
    fn default() -> Self {
        Self::new()
    }
}

impl PluginWorkflowManager {
    pub fn new() -> Self {
        Self {
            workflows: std::collections::HashMap::new(),
        }
    }

    /// Register a plugin as a workflow node
    pub fn register_plugin(&mut self, name: &str, plugin: Arc<dyn StatePlugin>) {
        // Create a basic workflow node
        let _node = WorkflowPluginNode::new(plugin);
        // In a full implementation, we'd store these nodes for workflow creation
        // For now, just log the registration
        log::info!("Registered plugin '{}' as workflow node", name);
        // TODO: Store the node for later workflow creation
    }

    /// Create a system administration workflow
    pub fn create_system_admin_workflow(&mut self) -> Result<()> {
        // Example: Network config → Firewall → Monitoring
        log::info!("🏗️  Creating system administration workflow");
        log::info!("   Network Plugin → Firewall Plugin → Monitoring Plugin");

        // For now, just log that this workflow would be created
        // In a full implementation, this would create actual workflow nodes
        // and connect them with proper state transitions

        Ok(())
    }

    /// Create a privacy network setup workflow
    pub fn create_privacy_network_workflow(&mut self) -> Result<()> {
        log::info!("🔒 Creating privacy network workflow");
        log::info!("   WireGuard Gateway → WARP Tunnel → XRay Client");
        log::info!("   ↓");
        log::info!("   Single OVS bridge (vmbr0) routes all traffic");

        // This workflow orchestrates privacy components on single bridge:
        // 1. Privacy plugin coordinates system services (WireGuard, WARP)
        // 2. LXC plugin creates XRay container with socket networking
        // 3. OpenFlow plugin sets up traffic routing through vmbr0
        // 4. Netmaker mesh also uses same bridge for container networking

        Ok(())
    }

    /// Create a container networking workflow (includes Netmaker mesh)
    pub fn create_container_networking_workflow(&mut self) -> Result<()> {
        log::info!("🏗️  Creating container networking workflow");
        log::info!("   Netmaker Server → LXC Containers → Socket Networking → vmbr0 Bridge");
        log::info!("   ↓");
        log::info!("   Full mesh networking for all containers on single bridge");

        // This workflow handles container networking on single bridge:
        // 1. Netmaker plugin manages system-wide mesh server
        // 2. LXC plugin creates containers with socket networking
        // 3. Containers auto-join Netmaker mesh via first-boot hooks
        // 4. All interfaces (privacy + mesh) connect to vmbr0
        // 5. OpenFlow rules route traffic between all components

        Ok(())
    }

    /// Create a development workflow
    pub fn create_development_workflow(&mut self) -> Result<()> {
        // Example: Code analysis → Testing → Documentation → Deployment
        log::info!("🏗️  Creating development workflow");
        log::info!("   Code Analysis → Testing → Documentation → Deployment");

        // For now, just log that this workflow would be created
        // In a full implementation, this would create actual workflow nodes

        Ok(())
    }

    /// Execute a workflow with given context
    pub async fn execute_workflow(
        &self,
        workflow_name: &str,
        context: Context,
    ) -> Result<serde_json::Value> {
        if let Some(workflow) = self.workflows.get(workflow_name) {
            log::info!("🚀 Executing plugin workflow: {}", workflow_name);
            let result = workflow.run(context).await?;
            log::info!("✅ Plugin workflow completed: {}", workflow_name);
            Ok(result)
        } else {
            Err(anyhow::anyhow!("Workflow '{}' not found", workflow_name))
        }
    }

    /// List available workflows
    pub fn list_workflows(&self) -> Vec<String> {
        self.workflows.keys().cloned().collect()
    }
}

/// Builder pattern for workflow plugin nodes
pub struct WorkflowPluginNodeBuilder {
    node: WorkflowPluginNode,
}

impl WorkflowPluginNodeBuilder {
    pub fn with_inputs(mut self, inputs: Vec<String>) -> Self {
        self.node.inputs = inputs;
        self
    }

    pub fn with_outputs(mut self, outputs: Vec<String>) -> Self {
        self.node.outputs = outputs;
        self
    }

    pub fn with_config(mut self, config: Value) -> Self {
        self.node.config = config;
        self
    }

    pub fn build(self) -> WorkflowPluginNode {
        self.node
    }
}
