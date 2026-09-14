//! Orchestrated Executor - Unified execution with orchestration support
//!
//! Routes execution requests to the appropriate handler:
//! - Direct tool execution
//! - Workstack execution
//! - Skill-augmented execution
//! - Multi-agent coordination
//! - Workflow execution

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use super::{
    coordinator::{AgentCoordinator, AgentTask, CoordinationStrategy},
    dbus_orchestrator::DbusOrchestrator,
    skills::SkillRegistry,
    workflows::WorkflowEngine,
    workstacks::{ToolExecutorTrait, WorkstackExecutor, WorkstackRegistry},
};
use op_core::ExecutionTracker;
use op_tools::ToolRegistry;

/// Execution mode determined by the orchestrator
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ExecutionMode {
    /// Direct tool execution
    Direct { tool_name: String },
    /// Workstack execution
    Workstack { workstack_id: String },
    /// Skill-augmented execution
    #[allow(dead_code)]
    Skill { skill_name: String },
    /// Multi-agent coordination
    MultiAgent { agents: Vec<String> },
    /// Workflow execution
    Workflow { workflow_id: String },
}

/// Result of orchestrated execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratedResult {
    /// Execution mode used
    pub mode: ExecutionMode,
    /// Whether execution succeeded
    pub success: bool,
    /// Result content
    pub content: Value,
    /// Execution ID for tracking
    pub execution_id: String,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Skills that were activated
    #[serde(default)]
    pub skills_activated: Vec<String>,
    /// Agents that were involved
    #[serde(default)]
    pub agents_involved: Vec<String>,
    /// Execution trace for debugging
    #[serde(default)]
    pub trace: Vec<TraceEntry>,
}

/// Trace entry for debugging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    pub timestamp: String,
    pub component: String,
    pub action: String,
    pub details: Option<Value>,
}

impl TraceEntry {
    pub fn new(component: &str, action: &str) -> Self {
        Self {
            timestamp: chrono::Utc::now().to_rfc3339(),
            component: component.to_string(),
            action: action.to_string(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}

/// Orchestrated executor - the main entry point for all execution
pub struct OrchestratedExecutor {
    /// Tool registry for direct execution
    tool_registry: Arc<ToolRegistry>,
    /// Execution tracker
    #[allow(dead_code)]
    tracker: Arc<ExecutionTracker>,
    /// Skill registry
    skill_registry: Arc<RwLock<SkillRegistry>>,
    /// Workstack registry
    workstack_registry: Arc<RwLock<WorkstackRegistry>>,
    /// Workflow engine
    workflow_engine: Arc<WorkflowEngine>,
    /// Agent coordinator
    coordinator: Arc<AgentCoordinator>,
    /// D-Bus orchestrator
    dbus_orchestrator: Arc<DbusOrchestrator>,
}

impl OrchestratedExecutor {
    /// Create new orchestrated executor
    pub async fn new(
        tool_registry: Arc<ToolRegistry>,
        tracker: Arc<ExecutionTracker>,
    ) -> Result<Self> {
        let skill_registry = Arc::new(RwLock::new(SkillRegistry::with_defaults()));
        let workstack_registry = Arc::new(RwLock::new(WorkstackRegistry::with_defaults()));
        let workflow_engine = Arc::new(WorkflowEngine::with_defaults());
        let coordinator = Arc::new(AgentCoordinator::new());
        let dbus_orchestrator = Arc::new(DbusOrchestrator::with_defaults());

        // Try to connect to D-Bus orchestrator
        if let Err(e) = dbus_orchestrator.connect().await {
            warn!("Could not connect to D-Bus orchestrator: {}", e);
        }

        // Register default agents with coordinator
        for agent_type in [
            "python-pro", "rust-pro", "golang-pro", "javascript-pro",
            "code-reviewer", "security-auditor", "docs-architect",
            "systemd", "network", "file", "executor", "monitor",
        ] {
            coordinator.register_agent(agent_type).await;
        }

        info!("Orchestrated executor initialized");

        Ok(Self {
            tool_registry,
            tracker,
            skill_registry,
            workstack_registry,
            workflow_engine,
            coordinator,
            dbus_orchestrator,
        })
    }

    /// Execute with automatic mode detection
    #[allow(dead_code)]
    pub async fn execute(
        &self,
        name: &str,
        arguments: Value,
        _session_id: Option<String>,
    ) -> Result<OrchestratedResult> {
        let start = std::time::Instant::now();
        let execution_id = uuid::Uuid::new_v4().to_string();
        let mut trace = Vec::new();

        trace.push(TraceEntry::new("orchestrator", "execution_start")
            .with_details(json!({ "name": name, "execution_id": &execution_id })));

        // Determine execution mode
        let mode = self.determine_mode(name, &arguments).await;
        trace.push(TraceEntry::new("orchestrator", "mode_determined")
            .with_details(json!({ "mode": format!("{:?}", mode) })));

        // Get active skills
        let active_skills: Vec<String> = self.skill_registry.read().await
            .active_skills()
            .iter()
            .map(|s| s.name.clone())
            .collect();

        // Execute based on mode
        let result = match &mode {
            ExecutionMode::Direct { tool_name } => {
                self.execute_direct(tool_name, arguments, &active_skills, &mut trace).await
            }
            ExecutionMode::Workstack { workstack_id } => {
                self.execute_workstack(workstack_id, arguments, &mut trace).await
            }
            ExecutionMode::Skill { skill_name } => {
                self.execute_with_skill(skill_name, name, arguments, &mut trace).await
            }
            ExecutionMode::MultiAgent { agents } => {
                self.execute_multi_agent(agents, arguments, &mut trace).await
            }
            ExecutionMode::Workflow { workflow_id } => {
                self.execute_workflow(workflow_id, arguments, &mut trace).await
            }
        };

        let duration = start.elapsed();
        trace.push(TraceEntry::new("orchestrator", "execution_complete")
            .with_details(json!({ "duration_ms": duration.as_millis() })));

        match result {
            Ok((content, agents_involved)) => Ok(OrchestratedResult {
                mode,
                success: true,
                content,
                execution_id,
                duration_ms: duration.as_millis() as u64,
                skills_activated: active_skills,
                agents_involved,
                trace,
            }),
            Err(e) => Ok(OrchestratedResult {
                mode,
                success: false,
                content: json!({ "error": e.to_string() }),
                execution_id,
                duration_ms: duration.as_millis() as u64,
                skills_activated: active_skills,
                agents_involved: vec![],
                trace,
            }),
        }
    }

    /// Determine execution mode from name and arguments
    async fn determine_mode(&self, name: &str, arguments: &Value) -> ExecutionMode {
        // Check for workstack prefix
        if name.starts_with("workstack_") {
            return ExecutionMode::Workstack {
                workstack_id: name["workstack_".len()..].to_string(),
            };
        }

        // Check for workflow prefix
        if name.starts_with("workflow_") {
            return ExecutionMode::Workflow {
                workflow_id: name["workflow_".len()..].to_string(),
            };
        }

        // Check for multi-agent request
        if let Some(agents) = arguments.get("agents").and_then(|v| v.as_array()) {
            let agent_list: Vec<String> = agents
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if !agent_list.is_empty() {
                return ExecutionMode::MultiAgent { agents: agent_list };
            }
        }

        // Check if this is a registered workstack
        if self.workstack_registry.read().await.get(name).is_some() {
            return ExecutionMode::Workstack {
                workstack_id: name.to_string(),
            };
        }

        // Check if this is a registered workflow
        if self.workflow_engine.get(name).await.is_some() {
            return ExecutionMode::Workflow {
                workflow_id: name.to_string(),
            };
        }

        // Default to direct execution
        ExecutionMode::Direct {
            tool_name: name.to_string(),
        }
    }

    /// Execute tool directly with skill augmentation
    async fn execute_direct(
        &self,
        tool_name: &str,
        mut arguments: Value,
        active_skills: &[String],
        trace: &mut Vec<TraceEntry>,
    ) -> Result<(Value, Vec<String>)> {
        trace.push(TraceEntry::new("direct", "start")
            .with_details(json!({ "tool": tool_name })));

        // Apply skill transformations
        let skill_registry = self.skill_registry.read().await;
        for skill_name in active_skills {
            if let Some(skill) = skill_registry.get(skill_name) {
                // Check constraints
                skill.check_constraints(tool_name, &arguments)?;
                // Transform input
                arguments = skill.transform_input(tool_name, arguments);
            }
        }

        // Execute via tool registry
        let tool = self.tool_registry.get(tool_name).await
            .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", tool_name))?;

        let result = tool.execute(arguments.clone()).await?;

        // Apply output transformations
        let mut final_result = result;
        for skill_name in active_skills {
            if let Some(skill) = skill_registry.get(skill_name) {
                final_result = skill.transform_output(tool_name, final_result);
            }
        }

        trace.push(TraceEntry::new("direct", "complete"));

        Ok((final_result, vec![]))
    }

    /// Execute a workstack
    async fn execute_workstack(
        &self,
        workstack_id: &str,
        arguments: Value,
        trace: &mut Vec<TraceEntry>,
    ) -> Result<(Value, Vec<String>)> {
        trace.push(TraceEntry::new("workstack", "start")
            .with_details(json!({ "workstack_id": workstack_id })));

        let executor = WorkstackExecutor::new(self.workstack_registry.clone());
        let adapter = ToolRegistryAdapter {
            registry: self.tool_registry.clone(),
        };

        let result = executor.execute(workstack_id, arguments, &adapter).await?;

        // Collect agents involved from phases
        let agents: Vec<String> = self.workstack_registry.read().await
            .get(workstack_id)
            .map(|ws| ws.phases.iter().flat_map(|p| p.agents.clone()).collect())
            .unwrap_or_default();

        trace.push(TraceEntry::new("workstack", "complete")
            .with_details(json!({ "phases": result.phases.len() })));

        Ok((simd_json::serde::to_owned_value(result)?, agents))
    }

    /// Execute with a specific skill activated
    async fn execute_with_skill(
        &self,
        skill_name: &str,
        tool_name: &str,
        arguments: Value,
        trace: &mut Vec<TraceEntry>,
    ) -> Result<(Value, Vec<String>)> {
        trace.push(TraceEntry::new("skill", "activating")
            .with_details(json!({ "skill": skill_name })));

        // Temporarily activate skill
        {
            let mut registry = self.skill_registry.write().await;
            registry.activate(skill_name)?;
        }

        // Get tool from arguments or use provided
        let actual_tool = arguments.get("tool")
            .and_then(|v| v.as_str())
            .unwrap_or(tool_name);
        
        let actual_args = arguments.get("arguments")
            .cloned()
            .unwrap_or(arguments.clone());

        let result = self.execute_direct(
            actual_tool,
            actual_args,
            &[skill_name.to_string()],
            trace,
        ).await;

        // Deactivate skill
        {
            let mut registry = self.skill_registry.write().await;
            let _ = registry.deactivate(skill_name);
        }

        result
    }

    /// Execute with multiple agents
    async fn execute_multi_agent(
        &self,
        agents: &[String],
        arguments: Value,
        trace: &mut Vec<TraceEntry>,
    ) -> Result<(Value, Vec<String>)> {
        trace.push(TraceEntry::new("multi_agent", "start")
            .with_details(json!({ "agents": agents })));

        let prompt = arguments.get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("Execute task");

        let strategy = arguments.get("strategy")
            .and_then(|v| v.as_str())
            .map(|s| match s {
                "parallel" => CoordinationStrategy::Parallel,
                "pipeline" => CoordinationStrategy::Pipeline,
                "race" => CoordinationStrategy::RaceFirstSuccess,
                "voting" => CoordinationStrategy::Voting { threshold: 0.5 },
                "consensus" => CoordinationStrategy::Consensus,
                _ => CoordinationStrategy::Sequential,
            })
            .unwrap_or(CoordinationStrategy::Sequential);

        let tasks: Vec<AgentTask> = agents.iter()
            .map(|agent| AgentTask::new(agent, prompt, arguments.clone()))
            .collect();

        let adapter = ToolRegistryAdapter {
            registry: self.tool_registry.clone(),
        };

        let results = self.coordinator.execute(tasks, strategy, &adapter).await?;

        trace.push(TraceEntry::new("multi_agent", "complete")
            .with_details(json!({ "results": results.len() })));

        Ok((simd_json::serde::to_owned_value(results)?, agents.to_vec()))
    }

    /// Execute a workflow
    async fn execute_workflow(
        &self,
        workflow_id: &str,
        arguments: Value,
        trace: &mut Vec<TraceEntry>,
    ) -> Result<(Value, Vec<String>)> {
        trace.push(TraceEntry::new("workflow", "start")
            .with_details(json!({ "workflow_id": workflow_id })));

        let adapter = ToolRegistryAdapter {
            registry: self.tool_registry.clone(),
        };

        let result = self.workflow_engine.execute(workflow_id, arguments, &adapter).await?;

        trace.push(TraceEntry::new("workflow", "complete")
            .with_details(json!({ "steps": result.steps.len() })));

        Ok((simd_json::serde::to_owned_value(result)?, vec![]))
    }

    /// Register a workflow
    #[allow(dead_code)]
    pub async fn register_workflow(&self, workflow: super::workflows::Workflow) {
        self.workflow_engine.register(workflow).await;
    }

    /// Register a workstack
    #[allow(dead_code)]
    pub async fn register_workstack(&self, workstack: super::workstacks::Workstack) {
        self.workstack_registry.write().await.register(workstack);
    }

    /// Activate a skill
    #[allow(dead_code)]
    pub async fn activate_skill(&self, name: &str) -> Result<()> {
        self.skill_registry.write().await.activate(name)
    }

    /// Deactivate a skill
    #[allow(dead_code)]
    pub async fn deactivate_skill(&self, name: &str) -> Result<()> {
        self.skill_registry.write().await.deactivate(name)
    }

    /// Get skill registry
    #[allow(dead_code)]
    pub fn skill_registry(&self) -> &Arc<RwLock<SkillRegistry>> {
        &self.skill_registry
    }

    /// Get workstack registry
    #[allow(dead_code)]
    pub fn workstack_registry(&self) -> &Arc<RwLock<WorkstackRegistry>> {
        &self.workstack_registry
    }

    /// Get workflow engine
    #[allow(dead_code)]
    pub fn workflow_engine(&self) -> &Arc<WorkflowEngine> {
        &self.workflow_engine
    }

    /// Get agent coordinator
    #[allow(dead_code)]
    pub fn coordinator(&self) -> &Arc<AgentCoordinator> {
        &self.coordinator
    }

    /// Get D-Bus orchestrator
    #[allow(dead_code)]
    pub fn dbus_orchestrator(&self) -> &Arc<DbusOrchestrator> {
        &self.dbus_orchestrator
    }
}

/// Adapter to use ToolRegistry with workstack executor
struct ToolRegistryAdapter {
    registry: Arc<ToolRegistry>,
}

#[async_trait]
impl ToolExecutorTrait for ToolRegistryAdapter {
    async fn execute_tool(&self, name: &str, args: Value) -> Result<Value> {
        let tool = self.registry.get(name).await
            .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", name))?;
        tool.execute(args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mode_detection() {
        let registry = Arc::new(ToolRegistry::new());
        let tracker = Arc::new(ExecutionTracker::new(100));
        let executor = OrchestratedExecutor::new(registry, tracker).await.unwrap();

        // Test workstack prefix
        let mode = executor.determine_mode("workstack_full_stack_feature", &json!({})).await;
        assert!(matches!(mode, ExecutionMode::Workstack { .. }));

        // Test workflow prefix
        let mode = executor.determine_mode("workflow_deploy", &json!({})).await;
        assert!(matches!(mode, ExecutionMode::Workflow { .. }));

        // Test multi-agent
        let mode = executor.determine_mode("task", &json!({ "agents": ["python-pro", "rust-pro"] })).await;
        assert!(matches!(mode, ExecutionMode::MultiAgent { .. }));

        // Test direct
        let mode = executor.determine_mode("ovs_list_bridges", &json!({})).await;
        assert!(matches!(mode, ExecutionMode::Direct { .. }));
    }
}
