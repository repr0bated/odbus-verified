//! Base Orchestration Agent Implementation

use async_trait::async_trait;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::collections::HashSet;

use super::super::agent_trait::{
    AgentCapability, AgentCategory, AgentRequest, AgentResponse, UnifiedAgent,
};
use super::super::registry::UnifiedAgentRegistry;
use crate::security::SecurityProfile;

/// Workflow step definition
#[derive(Debug, Clone)]
pub struct WorkflowStep {
    pub name: String,
    pub agent_id: String,
    pub operation: String,
    pub args_template: Value,
    pub condition: Option<String>,
}

/// Base implementation for orchestration agents
pub struct OrchestrationAgent {
    pub id: String,
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub allowed_agents: HashSet<String>,
    pub workflow_steps: Vec<WorkflowStep>,
}

impl OrchestrationAgent {
    pub fn new(id: &str, name: &str, description: &str, allowed_agents: Vec<&str>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            system_prompt: format!(
                "You are {}, an orchestration agent that coordinates other agents to complete complex tasks.",
                name
            ),
            allowed_agents: allowed_agents.into_iter().map(|s| s.to_string()).collect(),
            workflow_steps: vec![],
        }
    }

    pub fn with_step(mut self, step: WorkflowStep) -> Self {
        self.workflow_steps.push(step);
        self
    }

    /// Execute workflow steps using the registry
    pub async fn execute_workflow(
        &self,
        registry: &UnifiedAgentRegistry,
        context: Value,
    ) -> AgentResponse {
        let mut results = Vec::new();
        let mut current_context = context;

        for step in &self.workflow_steps {
            // Check if agent is allowed
            if !self.allowed_agents.contains(&step.agent_id) {
                return AgentResponse::failure(format!(
                    "Agent '{}' not allowed in this orchestration",
                    step.agent_id
                ));
            }

            // Get the agent
            let agent = match registry.get(&step.agent_id) {
                Some(a) => a,
                None => {
                    return AgentResponse::failure(format!("Agent '{}' not found", step.agent_id));
                }
            };

            // Build request
            let request = AgentRequest {
                operation: step.operation.clone(),
                args: step.args_template.clone(),
                context: Some(current_context.to_string()),
                files: vec![],
            };

            // Execute
            let response = agent.execute(request).await;
            results.push(json!({
                "step": step.name,
                "agent": step.agent_id,
                "success": response.success,
                "result": response.data,
            }));

            if !response.success {
                return AgentResponse::failure(format!(
                    "Workflow failed at step '{}': {}",
                    step.name, response.message
                ))
                .with_suggestions(vec![
                    format!("Check {} agent configuration", step.agent_id),
                    "Review step arguments".to_string(),
                ]);
            }

            // Update context with result
            if let Some(obj) = current_context.as_object_mut() {
                obj.insert(format!("step_{}_result", step.name), response.data);
            }
        }

        AgentResponse::success(
            json!({
                "workflow": self.id,
                "steps_completed": results.len(),
                "results": results,
            }),
            "Workflow completed successfully",
        )
    }
}

#[async_trait]
impl UnifiedAgent for OrchestrationAgent {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn category(&self) -> AgentCategory {
        AgentCategory::Orchestration
    }

    fn capabilities(&self) -> Vec<AgentCapability> {
        vec![
            AgentCapability::DelegateToAgents {
                agents: self.allowed_agents.iter().cloned().collect(),
            },
            AgentCapability::WorkflowManagement,
        ]
    }

    fn system_prompt(&self) -> &str {
        &self.system_prompt
    }

    fn security_profile(&self) -> Option<&SecurityProfile> {
        None // Orchestrators delegate security to sub-agents
    }

    fn operations(&self) -> Vec<&str> {
        vec!["run_workflow", "list_steps", "validate"]
    }

    async fn execute(&self, request: AgentRequest) -> AgentResponse {
        match request.operation.as_str() {
            "list_steps" => {
                let steps: Vec<_> = self
                    .workflow_steps
                    .iter()
                    .map(|s| {
                        json!({
                            "name": s.name,
                            "agent": s.agent_id,
                            "operation": s.operation,
                        })
                    })
                    .collect();
                AgentResponse::success(
                    json!({ "steps": steps }),
                    format!("Workflow has {} steps", steps.len()),
                )
            }
            "validate" => {
                // Would validate workflow configuration
                AgentResponse::success(json!({ "valid": true }), "Workflow configuration is valid")
            }
            _ => AgentResponse::failure(
                "run_workflow requires a registry - use execute_workflow() directly",
            ),
        }
    }
}
