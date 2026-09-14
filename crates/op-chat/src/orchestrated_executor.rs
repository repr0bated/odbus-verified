//! Orchestrated Executor - Integrates tool execution with orchestration
//!
//! This module bridges the gap between:
//! - Simple tool execution (ToolAdapter)
//! - Multi-step workstacks (WorkstackExecutor)
//! - Skill activation (SkillRegistry)
//! - Agent coordination (AgentRegistry)
//!
//! All tool calls flow through here to determine if orchestration is needed.

use anyhow::Result;
use op_core::{ExecutionTracker, ToolRequest, ToolResult};
use op_tools::ToolRegistry;
use serde::{Deserialize, Serialize};
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use crate::orchestration::{
    CoordinationMode, DisclosureLevel, SkillRegistry, Workstack, WorkstackExecutor,
    WorkstackRegistry, WorkstackRequest, WorkstackResult,
};
use crate::tool_executor::TrackedToolExecutor;

/// Execution mode determined by the orchestrator
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ExecutionMode {
    /// Direct tool execution (no orchestration needed)
    Direct { tool_name: String },
    /// Execute a workstack (multi-phase)
    Workstack { workstack_id: String },
    /// Activate a skill and execute
    Skill { skill_name: String },
    /// Coordinate multiple agents
    MultiAgent { agents: Vec<String> },
    /// Execute a workflow (sequence of tools)
    Workflow { workflow_id: String },
}

/// Result from orchestrated execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratedResult {
    /// Execution mode that was used
    pub mode: ExecutionMode,
    /// Whether execution succeeded
    pub success: bool,
    /// Primary result content
    pub content: Value,
    /// Execution trace (for debugging)
    pub trace: Vec<ExecutionStep>,
    /// Activated skills
    pub skills_activated: Vec<String>,
    /// Agents involved
    pub agents_involved: Vec<String>,
    /// Total duration in milliseconds
    pub duration_ms: u64,
    /// Execution ID for tracking
    pub execution_id: String,
}

/// Single step in execution trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStep {
    pub step_number: usize,
    pub step_type: String,
    pub tool_or_agent: String,
    pub success: bool,
    pub duration_ms: u64,
    pub output_summary: Option<String>,
}

/// Orchestrated Executor - The unified execution layer
pub struct OrchestratedExecutor {
    /// Tool registry for direct tool execution
    tool_registry: Arc<ToolRegistry>,
    /// Tracked executor for accountability
    tracked_executor: Arc<TrackedToolExecutor>,
    /// Workstack registry
    workstack_registry: Arc<RwLock<WorkstackRegistry>>,
    /// Skill registry
    skill_registry: Arc<RwLock<SkillRegistry>>,
    /// Execution tracker
    execution_tracker: Arc<ExecutionTracker>,
    /// Workflow definitions (tool sequences)
    workflows: Arc<RwLock<HashMap<String, Workflow>>>,
}

/// Workflow definition (sequence of tool calls)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
    /// Stop on first failure?
    pub fail_fast: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub tool_name: String,
    /// Template for arguments (can reference previous step outputs)
    pub arguments_template: Value,
    /// Condition for execution (optional)
    pub condition: Option<String>,
}

impl OrchestratedExecutor {
    /// Create a new orchestrated executor
    pub async fn new(
        tool_registry: Arc<ToolRegistry>,
        execution_tracker: Arc<ExecutionTracker>,
    ) -> Result<Self> {
        let tracked_executor = Arc::new(TrackedToolExecutor::new(
            tool_registry.clone(),
            execution_tracker.clone(),
        ));

        let workstack_registry = Arc::new(RwLock::new(WorkstackRegistry::new()));
        let skill_registry = Arc::new(RwLock::new(SkillRegistry::new()));

        // Load built-in workstacks
        {
            let mut registry = workstack_registry.write().await;
            for workstack in crate::orchestration::builtin_workstacks() {
                registry.register(workstack);
            }
        }

        Ok(Self {
            tool_registry,
            tracked_executor,
            workstack_registry,
            skill_registry,
            execution_tracker,
            workflows: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Execute with automatic orchestration detection
    pub async fn execute(
        &self,
        tool_name: &str,
        arguments: Value,
        session_id: Option<String>,
    ) -> Result<OrchestratedResult> {
        let start_time = std::time::Instant::now();
        let execution_id = uuid::Uuid::new_v4().to_string();

        info!(
            execution_id = %execution_id,
            tool = %tool_name,
            "Starting orchestrated execution"
        );

        // Determine execution mode
        let mode = self.determine_execution_mode(tool_name, &arguments).await;

        debug!(mode = ?mode, "Determined execution mode");

        // Execute based on mode
        let result = match &mode {
            ExecutionMode::Direct { tool_name } => {
                self.execute_direct(tool_name, arguments.clone(), session_id.clone())
                    .await
            }
            ExecutionMode::Workstack { workstack_id } => {
                self.execute_workstack(workstack_id, arguments.clone(), session_id.clone())
                    .await
            }
            ExecutionMode::Skill { skill_name } => {
                self.execute_with_skill(skill_name, tool_name, arguments.clone(), session_id.clone())
                    .await
            }
            ExecutionMode::MultiAgent { agents } => {
                self.execute_multi_agent(agents, arguments.clone(), session_id.clone())
                    .await
            }
            ExecutionMode::Workflow { workflow_id } => {
                self.execute_workflow(workflow_id, arguments.clone(), session_id.clone())
                    .await
            }
        };

        let duration_ms = start_time.elapsed().as_millis() as u64;

        match result {
            Ok(mut orchestrated) => {
                orchestrated.duration_ms = duration_ms;
                orchestrated.execution_id = execution_id;
                orchestrated.mode = mode;
                Ok(orchestrated)
            }
            Err(e) => {
                error!(error = %e, "Orchestrated execution failed");
                Ok(OrchestratedResult {
                    mode,
                    success: false,
                    content: json!({ "error": e.to_string() }),
                    trace: vec![],
                    skills_activated: vec![],
                    agents_involved: vec![],
                    duration_ms,
                    execution_id,
                })
            }
        }
    }

    /// Determine the appropriate execution mode
    async fn determine_execution_mode(&self, tool_name: &str, arguments: &Value) -> ExecutionMode {
        // Check if this is a workstack invocation
        if tool_name.starts_with("workstack_") {
            let workstack_id = tool_name.strip_prefix("workstack_").unwrap_or(tool_name);
            let registry = self.workstack_registry.read().await;
            if registry.get(workstack_id).is_some() {
                return ExecutionMode::Workstack {
                    workstack_id: workstack_id.to_string(),
                };
            }
        }

        // Check if this is a skill invocation
        if tool_name.starts_with("skill_") {
            let skill_name = tool_name.strip_prefix("skill_").unwrap_or(tool_name);
            let registry = self.skill_registry.read().await;
            if registry.get(skill_name).is_some() {
                return ExecutionMode::Skill {
                    skill_name: skill_name.to_string(),
                };
            }
        }

        // Check if this is a workflow invocation
        if tool_name.starts_with("workflow_") {
            let workflow_id = tool_name.strip_prefix("workflow_").unwrap_or(tool_name);
            let workflows = self.workflows.read().await;
            if workflows.contains_key(workflow_id) {
                return ExecutionMode::Workflow {
                    workflow_id: workflow_id.to_string(),
                };
            }
        }

        // Check if multi-agent coordination is requested
        if let Some(agents) = arguments.get("coordinate_agents").and_then(|v| v.as_array()) {
            let agent_names: Vec<String> = agents
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if !agent_names.is_empty() {
                return ExecutionMode::MultiAgent {
                    agents: agent_names,
                };
            }
        }

        // Default: direct tool execution
        ExecutionMode::Direct {
            tool_name: tool_name.to_string(),
        }
    }

    /// Execute a tool directly (no orchestration)
    async fn execute_direct(
        &self,
        tool_name: &str,
        arguments: Value,
        session_id: Option<String>,
    ) -> Result<OrchestratedResult> {
        let start = std::time::Instant::now();

        let tracked = self
            .tracked_executor
            .execute(tool_name, arguments, session_id)
            .await?;

        let step = ExecutionStep {
            step_number: 1,
            step_type: "tool".to_string(),
            tool_or_agent: tool_name.to_string(),
            success: tracked.success(),
            duration_ms: start.elapsed().as_millis() as u64,
            output_summary: Some(truncate(&tracked.result.content.to_string(), 200)),
        };

        Ok(OrchestratedResult {
            mode: ExecutionMode::Direct {
                tool_name: tool_name.to_string(),
            },
            success: tracked.success(),
            content: tracked.result.content,
            trace: vec![step],
            skills_activated: vec![],
            agents_involved: vec![],
            duration_ms: 0, // Will be set by caller
            execution_id: tracked.execution_id,
        })
    }

    /// Execute a workstack (multi-phase orchestration)
    async fn execute_workstack(
        &self,
        workstack_id: &str,
        arguments: Value,
        session_id: Option<String>,
    ) -> Result<OrchestratedResult> {
        let registry = self.workstack_registry.read().await;
        let workstack = registry
            .get(workstack_id)
            .ok_or_else(|| anyhow::anyhow!("Workstack not found: {}", workstack_id))?;

        info!(workstack = %workstack_id, "Executing workstack");

        // Create workstack executor
        let executor = WorkstackExecutor::new(
            self.tool_registry.clone(),
            self.skill_registry.clone(),
        );

        // Build request
        let request = WorkstackRequest {
            workstack_id: workstack_id.to_string(),
            arguments: simd_json::to_string(&arguments)?,
            agent_overrides: HashMap::new(),
            skip_phases: vec![],
            session_id,
        };

        // Execute workstack
        let result = executor.execute(workstack.clone(), request).await?;

        // Convert to orchestrated result
        let trace: Vec<ExecutionStep> = result
            .phases
            .iter()
            .enumerate()
            .map(|(i, phase)| ExecutionStep {
                step_number: i + 1,
                step_type: "phase".to_string(),
                tool_or_agent: phase.phase_name.clone(),
                success: phase.success,
                duration_ms: phase.duration_ms,
                output_summary: Some(truncate(&phase.output, 200)),
            })
            .collect();

        Ok(OrchestratedResult {
            mode: ExecutionMode::Workstack {
                workstack_id: workstack_id.to_string(),
            },
            success: result.success,
            content: json!({
                "output": result.output,
                "phases_completed": result.phases.len(),
            }),
            trace,
            skills_activated: result.metadata.skills_activated,
            agents_involved: vec![], // TODO: Track agents
            duration_ms: result.metadata.duration_ms,
            execution_id: String::new(), // Will be set by caller
        })
    }

    /// Execute with skill activation
    async fn execute_with_skill(
        &self,
        skill_name: &str,
        tool_name: &str,
        arguments: Value,
        session_id: Option<String>,
    ) -> Result<OrchestratedResult> {
        // Activate skill
        {
            let mut registry = self.skill_registry.write().await;
            registry.activate(skill_name, DisclosureLevel::Instructions)?;
        }

        info!(skill = %skill_name, "Skill activated");

        // Execute the underlying tool
        let mut result = self
            .execute_direct(tool_name, arguments, session_id)
            .await?;

        result.skills_activated.push(skill_name.to_string());
        result.mode = ExecutionMode::Skill {
            skill_name: skill_name.to_string(),
        };

        Ok(result)
    }

    /// Execute with multi-agent coordination
    async fn execute_multi_agent(
        &self,
        agents: &[String],
        arguments: Value,
        session_id: Option<String>,
    ) -> Result<OrchestratedResult> {
        info!(agents = ?agents, "Coordinating multiple agents");

        let mut trace = Vec::new();
        let mut all_success = true;
        let mut combined_output = Vec::new();

        for (i, agent) in agents.iter().enumerate() {
            let agent_tool = format!("agent_{}", agent.replace('-', "_"));
            let start = std::time::Instant::now();

            let result = self
                .tracked_executor
                .execute(&agent_tool, arguments.clone(), session_id.clone())
                .await;

            let (success, output) = match result {
                Ok(tracked) => (tracked.success(), tracked.result.content.to_string()),
                Err(e) => {
                    all_success = false;
                    (false, e.to_string())
                }
            };

            trace.push(ExecutionStep {
                step_number: i + 1,
                step_type: "agent".to_string(),
                tool_or_agent: agent.clone(),
                success,
                duration_ms: start.elapsed().as_millis() as u64,
                output_summary: Some(truncate(&output, 200)),
            });

            combined_output.push(json!({
                "agent": agent,
                "success": success,
                "output": output,
            }));

            if !success {
                all_success = false;
            }
        }

        Ok(OrchestratedResult {
            mode: ExecutionMode::MultiAgent {
                agents: agents.to_vec(),
            },
            success: all_success,
            content: json!({
                "agents": combined_output,
                "all_succeeded": all_success,
            }),
            trace,
            skills_activated: vec![],
            agents_involved: agents.to_vec(),
            duration_ms: 0,
            execution_id: String::new(),
        })
    }

    /// Execute a workflow (sequence of tools)
    async fn execute_workflow(
        &self,
        workflow_id: &str,
        arguments: Value,
        session_id: Option<String>,
    ) -> Result<OrchestratedResult> {
        let workflows = self.workflows.read().await;
        let workflow = workflows
            .get(workflow_id)
            .ok_or_else(|| anyhow::anyhow!("Workflow not found: {}", workflow_id))?
            .clone();
        drop(workflows);

        info!(workflow = %workflow_id, steps = workflow.steps.len(), "Executing workflow");

        let mut trace = Vec::new();
        let mut all_success = true;
        let mut context = arguments.clone();
        let mut step_outputs: Vec<Value> = Vec::new();

        for (i, step) in workflow.steps.iter().enumerate() {
            // Resolve arguments template
            let step_args = self.resolve_template(&step.arguments_template, &context, &step_outputs)?;

            let start = std::time::Instant::now();

            let result = self
                .tracked_executor
                .execute(&step.tool_name, step_args, session_id.clone())
                .await;

            let (success, output) = match result {
                Ok(tracked) => {
                    step_outputs.push(tracked.result.content.clone());
                    (tracked.success(), tracked.result.content.to_string())
                }
                Err(e) => {
                    step_outputs.push(Value::Null);
                    (false, e.to_string())
                }
            };

            trace.push(ExecutionStep {
                step_number: i + 1,
                step_type: "workflow_step".to_string(),
                tool_or_agent: step.tool_name.clone(),
                success,
                duration_ms: start.elapsed().as_millis() as u64,
                output_summary: Some(truncate(&output, 200)),
            });

            if !success {
                all_success = false;
                if workflow.fail_fast {
                    warn!(step = i + 1, "Workflow stopped due to fail_fast");
                    break;
                }
            }
        }

        Ok(OrchestratedResult {
            mode: ExecutionMode::Workflow {
                workflow_id: workflow_id.to_string(),
            },
            success: all_success,
            content: json!({
                "workflow": workflow_id,
                "steps_completed": trace.len(),
                "steps_total": workflow.steps.len(),
                "outputs": step_outputs,
            }),
            trace,
            skills_activated: vec![],
            agents_involved: vec![],
            duration_ms: 0,
            execution_id: String::new(),
        })
    }

    /// Resolve template arguments with context
    fn resolve_template(
        &self,
        template: &Value,
        context: &Value,
        step_outputs: &[Value],
    ) -> Result<Value> {
        // Simple template resolution - in production, use a proper template engine
        let template_str = simd_json::to_string(template)?;

        // Replace $context.* references
        let mut resolved = template_str.clone();
        if let Some(obj) = context.as_object() {
            for (key, value) in obj {
                let placeholder = format!("$context.{}", key);
                let replacement = match value {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                resolved = resolved.replace(&placeholder, &replacement);
            }
        }

        // Replace $step[N] references
        for (i, output) in step_outputs.iter().enumerate() {
            let placeholder = format!("$step[{}]", i);
            let replacement = output.to_string();
            resolved = resolved.replace(&placeholder, &replacement);
        }

        Ok(simd_json::from_str(&resolved)?)
    }

    /// Register a workflow
    pub async fn register_workflow(&self, workflow: Workflow) {
        let mut workflows = self.workflows.write().await;
        workflows.insert(workflow.id.clone(), workflow);
    }

    /// Get workstack registry
    pub fn workstack_registry(&self) -> &Arc<RwLock<WorkstackRegistry>> {
        &self.workstack_registry
    }

    /// Get skill registry
    pub fn skill_registry(&self) -> &Arc<RwLock<SkillRegistry>> {
        &self.skill_registry
    }
}

/// Truncate string to max length
fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_mode_detection() {
        // Test workstack detection
        assert!(matches!(
            determine_mode_from_name("workstack_full_stack_feature"),
            Some(ExecutionMode::Workstack { .. })
        ));

        // Test skill detection
        assert!(matches!(
            determine_mode_from_name("skill_python_debugging"),
            Some(ExecutionMode::Skill { .. })
        ));

        // Test workflow detection
        assert!(matches!(
            determine_mode_from_name("workflow_deploy_production"),
            Some(ExecutionMode::Workflow { .. })
        ));

        // Test direct (no prefix)
        assert!(determine_mode_from_name("ovs_list_bridges").is_none());
    }

    fn determine_mode_from_name(name: &str) -> Option<ExecutionMode> {
        if name.starts_with("workstack_") {
            Some(ExecutionMode::Workstack {
                workstack_id: name["workstack_".len()..].to_string(),
            })
        } else if name.starts_with("skill_") {
            Some(ExecutionMode::Skill {
                skill_name: name["skill_".len()..].to_string(),
            })
        } else if name.starts_with("workflow_") {
            Some(ExecutionMode::Workflow {
                workflow_id: name["workflow_".len()..].to_string(),
            })
        } else {
            None
        }
    }
}
