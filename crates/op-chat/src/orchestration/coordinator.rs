//! Multi-Agent Coordinator - Parallel agent execution and communication
//!
//! Manages multiple agents working together on complex tasks.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

/// Coordination strategy for multi-agent tasks
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CoordinationStrategy {
    /// Execute agents sequentially, passing results
    Sequential,
    /// Execute agents in parallel, aggregate results
    Parallel,
    /// Execute agents in parallel, use first successful result
    RaceFirstSuccess,
    /// Pipeline: each agent's output is next agent's input
    Pipeline,
    /// Voting: multiple agents vote on a decision
    Voting { threshold: f32 },
    /// Consensus: all agents must agree
    Consensus,
}

impl Default for CoordinationStrategy {
    fn default() -> Self {
        Self::Sequential
    }
}

/// Task for an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTask {
    /// Unique task ID
    pub id: String,
    /// Target agent
    pub agent: String,
    /// Task description/prompt
    pub prompt: String,
    /// Input data
    pub input: Value,
    /// Timeout in seconds
    #[serde(default = "default_task_timeout")]
    pub timeout_secs: u64,
    /// Priority (higher = more urgent)
    #[serde(default)]
    #[allow(dead_code)]
    pub priority: i32,
}

fn default_task_timeout() -> u64 {
    300
}

impl AgentTask {
    pub fn new(agent: &str, prompt: &str, input: Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            agent: agent.to_string(),
            prompt: prompt.to_string(),
            input,
            timeout_secs: default_task_timeout(),
            priority: 0,
        }
    }

    #[allow(dead_code)]
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    #[allow(dead_code)]
    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }
}

/// Result from an agent task
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    /// Task ID
    pub task_id: String,
    /// Agent that executed
    pub agent: String,
    /// Whether task succeeded
    pub success: bool,
    /// Result data
    pub result: Value,
    /// Error if failed
    pub error: Option<String>,
    /// Execution time in ms
    pub duration_ms: u64,
}

/// Message between coordinator and agents
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum CoordinatorMessage {
    /// Assign task to agent
    AssignTask(AgentTask),
    /// Task completed
    TaskComplete(TaskResult),
    /// Cancel task
    CancelTask(String),
    /// Shutdown
    Shutdown,
}

/// Agent pool entry
#[derive(Debug, Clone)]
struct AgentEntry {
    agent_type: String,
    status: AgentStatus,
    #[allow(dead_code)]
    current_task: Option<String>,
    #[allow(dead_code)]
    completed_tasks: u32,
    #[allow(dead_code)]
    failed_tasks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum AgentStatus {
    Idle,
    Busy,
    #[allow(dead_code)]
    Error,
    #[allow(dead_code)]
    Offline,
}

/// Multi-agent coordinator
pub struct AgentCoordinator {
    /// Agent pool
    agents: Arc<RwLock<HashMap<String, AgentEntry>>>,
    /// Pending tasks
    #[allow(dead_code)]
    pending_tasks: Arc<RwLock<Vec<AgentTask>>>,
    /// Active tasks
    #[allow(dead_code)]
    active_tasks: Arc<RwLock<HashMap<String, AgentTask>>>,
    /// Task results
    #[allow(dead_code)]
    results: Arc<RwLock<HashMap<String, TaskResult>>>,
    /// Message channel
    #[allow(dead_code)]
    tx: mpsc::Sender<CoordinatorMessage>,
    #[allow(dead_code)]
    rx: Arc<RwLock<mpsc::Receiver<CoordinatorMessage>>>,
}

impl AgentCoordinator {
    /// Create new coordinator
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(100);
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            pending_tasks: Arc::new(RwLock::new(Vec::new())),
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(HashMap::new())),
            tx,
            rx: Arc::new(RwLock::new(rx)),
        }
    }

    /// Register an agent type
    pub async fn register_agent(&self, agent_type: &str) {
        let mut agents = self.agents.write().await;
        agents.insert(
            agent_type.to_string(),
            AgentEntry {
                agent_type: agent_type.to_string(),
                status: AgentStatus::Idle,
                current_task: None,
                completed_tasks: 0,
                failed_tasks: 0,
            },
        );
        info!(agent = %agent_type, "Agent registered with coordinator");
    }

    /// Execute tasks with given strategy
    pub async fn execute(
        &self,
        tasks: Vec<AgentTask>,
        strategy: CoordinationStrategy,
        tool_executor: &dyn super::workstacks::ToolExecutorTrait,
    ) -> Result<Vec<TaskResult>> {
        info!(
            task_count = tasks.len(),
            strategy = ?strategy,
            "Starting coordinated execution"
        );

        match strategy {
            CoordinationStrategy::Sequential => {
                self.execute_sequential(tasks, tool_executor).await
            }
            CoordinationStrategy::Parallel => {
                self.execute_parallel(tasks, tool_executor).await
            }
            CoordinationStrategy::RaceFirstSuccess => {
                self.execute_race(tasks, tool_executor).await
            }
            CoordinationStrategy::Pipeline => {
                self.execute_pipeline(tasks, tool_executor).await
            }
            CoordinationStrategy::Voting { threshold } => {
                self.execute_voting(tasks, threshold, tool_executor).await
            }
            CoordinationStrategy::Consensus => {
                self.execute_consensus(tasks, tool_executor).await
            }
        }
    }

    /// Sequential execution
    async fn execute_sequential(
        &self,
        tasks: Vec<AgentTask>,
        tool_executor: &dyn super::workstacks::ToolExecutorTrait,
    ) -> Result<Vec<TaskResult>> {
        let mut results = Vec::new();
        let mut last_result: Option<Value> = None;

        for mut task in tasks {
            // Pass previous result as context
            if let Some(ref prev) = last_result {
                if let Some(obj) = task.input.as_object_mut() {
                    obj.insert("previous_result".to_string(), prev.clone());
                }
            }

            let result = self.execute_single_task(&task, tool_executor).await;
            last_result = Some(result.result.clone());
            results.push(result);
        }

        Ok(results)
    }

    /// Parallel execution
    async fn execute_parallel(
        &self,
        tasks: Vec<AgentTask>,
        tool_executor: &dyn super::workstacks::ToolExecutorTrait,
    ) -> Result<Vec<TaskResult>> {
        // For now, execute sequentially but mark as parallel
        // True parallel would require spawning tasks
        let mut results = Vec::new();
        for task in tasks {
            results.push(self.execute_single_task(&task, tool_executor).await);
        }
        Ok(results)
    }

    /// Race execution - first success wins
    async fn execute_race(
        &self,
        tasks: Vec<AgentTask>,
        tool_executor: &dyn super::workstacks::ToolExecutorTrait,
    ) -> Result<Vec<TaskResult>> {
        for task in tasks {
            let result = self.execute_single_task(&task, tool_executor).await;
            if result.success {
                return Ok(vec![result]);
            }
        }
        Ok(vec![])
    }

    /// Pipeline execution - output becomes input
    async fn execute_pipeline(
        &self,
        tasks: Vec<AgentTask>,
        tool_executor: &dyn super::workstacks::ToolExecutorTrait,
    ) -> Result<Vec<TaskResult>> {
        let mut results = Vec::new();
        let mut current_input = json!({});

        for mut task in tasks {
            task.input = current_input.clone();
            let result = self.execute_single_task(&task, tool_executor).await;
            current_input = result.result.clone();
            results.push(result);
        }

        Ok(results)
    }

    /// Voting execution
    async fn execute_voting(
        &self,
        tasks: Vec<AgentTask>,
        threshold: f32,
        tool_executor: &dyn super::workstacks::ToolExecutorTrait,
    ) -> Result<Vec<TaskResult>> {
        let results = self.execute_parallel(tasks, tool_executor).await?;
        
        // Count votes (successful results)
        let total = results.len() as f32;
        let successes = results.iter().filter(|r| r.success).count() as f32;
        let vote_ratio = successes / total;

        info!(vote_ratio = %vote_ratio, threshold = %threshold, "Voting result");

        Ok(results)
    }

    /// Consensus execution
    async fn execute_consensus(
        &self,
        tasks: Vec<AgentTask>,
        tool_executor: &dyn super::workstacks::ToolExecutorTrait,
    ) -> Result<Vec<TaskResult>> {
        let results = self.execute_parallel(tasks, tool_executor).await?;
        
        // Check if all succeeded
        let all_success = results.iter().all(|r| r.success);
        if !all_success {
            warn!("Consensus not reached - not all agents succeeded");
        }

        Ok(results)
    }

    /// Execute a single task
    async fn execute_single_task(
        &self,
        task: &AgentTask,
        tool_executor: &dyn super::workstacks::ToolExecutorTrait,
    ) -> TaskResult {
        let start = std::time::Instant::now();
        debug!(task_id = %task.id, agent = %task.agent, "Executing agent task");

        // Build tool call for the agent
        let tool_name = format!("agent_{}", task.agent.replace('-', "_"));
        let args = json!({
            "prompt": task.prompt,
            "input": task.input,
        });

        match tool_executor.execute_tool(&tool_name, args).await {
            Ok(result) => {
                let duration = start.elapsed();
                TaskResult {
                    task_id: task.id.clone(),
                    agent: task.agent.clone(),
                    success: true,
                    result,
                    error: None,
                    duration_ms: duration.as_millis() as u64,
                }
            }
            Err(e) => {
                let duration = start.elapsed();
                error!(task_id = %task.id, error = %e, "Agent task failed");
                TaskResult {
                    task_id: task.id.clone(),
                    agent: task.agent.clone(),
                    success: false,
                    result: json!(null),
                    error: Some(e.to_string()),
                    duration_ms: duration.as_millis() as u64,
                }
            }
        }
    }

    /// Get coordinator statistics
    #[allow(dead_code)]
    pub async fn stats(&self) -> CoordinatorStats {
        let agents = self.agents.read().await;
        let pending = self.pending_tasks.read().await;
        let active = self.active_tasks.read().await;
        let results = self.results.read().await;

        CoordinatorStats {
            registered_agents: agents.len(),
            idle_agents: agents.values().filter(|a| a.status == AgentStatus::Idle).count(),
            busy_agents: agents.values().filter(|a| a.status == AgentStatus::Busy).count(),
            pending_tasks: pending.len(),
            active_tasks: active.len(),
            completed_tasks: results.len(),
        }
    }
}

impl Default for AgentCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// Coordinator statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct CoordinatorStats {
    pub registered_agents: usize,
    pub idle_agents: usize,
    pub busy_agents: usize,
    pub pending_tasks: usize,
    pub active_tasks: usize,
    pub completed_tasks: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_creation() {
        let task = AgentTask::new("python-pro", "Analyze this code", json!({ "code": "print(1)" }))
            .with_timeout(60)
            .with_priority(5);

        assert_eq!(task.agent, "python-pro");
        assert_eq!(task.timeout_secs, 60);
        assert_eq!(task.priority, 5);
    }

    #[tokio::test]
    async fn test_coordinator_registration() {
        let coordinator = AgentCoordinator::new();
        coordinator.register_agent("python-pro").await;
        coordinator.register_agent("rust-pro").await;

        let stats = coordinator.stats().await;
        assert_eq!(stats.registered_agents, 2);
        assert_eq!(stats.idle_agents, 2);
    }
}
