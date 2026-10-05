//! Workstack Executor - Production Implementation
//!
//! Executes multi-phase workstacks that combine agents and tools.
//! Features:
//! - Phase dependency resolution
//! - Parallel execution where dependencies allow
//! - Rollback on failure
//! - Progress streaming
//! - Variable interpolation
//! - Comprehensive error handling

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use simd_json::prelude::ValueAsScalar;
use tokio::sync::{RwLock, mpsc};
use tracing::{debug, error, info, instrument, warn};

use super::error::{ErrorCode, OrchestrationError, OrchestrationResult};
use super::grpc_pool::{GrpcAgentPool, StreamChunk};
use super::{ExecutionControl, ScopeVerifier, VerifiedScope, WorkstackEventSink};

// ============================================================================
// WORKSTACK TYPES
// ============================================================================

/// A workstack definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workstack {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: Option<String>,
    pub phases: Vec<WorkstackPhase>,
    pub variables: HashMap<String, String>,
    pub required_agents: Vec<String>,
    pub timeout_secs: Option<u64>,
}

impl Workstack {
    pub fn new(id: &str, name: &str, description: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            category: None,
            phases: Vec::new(),
            variables: HashMap::new(),
            required_agents: Vec::new(),
            timeout_secs: None,
        }
    }

    pub fn with_phase(mut self, phase: WorkstackPhase) -> Self {
        // Track required agents
        for agent in &phase.agents {
            if !self.required_agents.contains(agent) {
                self.required_agents.push(agent.clone());
            }
        }
        self.phases.push(phase);
        self
    }

    pub fn with_variable(mut self, key: &str, value: &str) -> Self {
        self.variables.insert(key.to_string(), value.to_string());
        self
    }

    pub fn with_category(mut self, category: &str) -> Self {
        self.category = Some(category.to_string());
        self
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }
}

/// A phase in a workstack
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkstackPhase {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Tools to execute in this phase
    pub tools: Vec<PhaseToolCall>,
    /// Agents to invoke in this phase (via gRPC)
    pub agents: Vec<String>,
    /// Agent operation to invoke (if agents specified)
    pub agent_operation: Option<String>,
    /// Agent arguments (JSON)
    pub agent_arguments: Option<Value>,
    /// IDs of phases this depends on
    pub depends_on: Vec<String>,
    /// Condition for execution (SpEL-like expression)
    pub condition: Option<String>,
    /// Rollback steps if this phase fails
    pub rollback: Vec<PhaseToolCall>,
    /// Continue to next phase even if this fails
    pub continue_on_failure: bool,
    /// Phase timeout in seconds
    pub timeout_secs: u64,
}

impl Default for WorkstackPhase {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            description: String::new(),
            tools: Vec::new(),
            agents: Vec::new(),
            agent_operation: None,
            agent_arguments: None,
            depends_on: Vec::new(),
            condition: None,
            rollback: Vec::new(),
            continue_on_failure: false,
            timeout_secs: 60,
        }
    }
}

/// A tool call within a phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseToolCall {
    pub tool: String,
    pub arguments: Value,
    /// Store result in this variable
    pub store_as: Option<String>,
    /// Number of retries on failure
    pub retries: u32,
}

/// Status of a phase
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhaseStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
    RolledBack,
}

impl std::fmt::Display for PhaseStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PhaseStatus::Pending => write!(f, "pending"),
            PhaseStatus::Running => write!(f, "running"),
            PhaseStatus::Completed => write!(f, "completed"),
            PhaseStatus::Failed => write!(f, "failed"),
            PhaseStatus::Skipped => write!(f, "skipped"),
            PhaseStatus::RolledBack => write!(f, "rolled_back"),
        }
    }
}

// ============================================================================
// EXECUTION TYPES
// ============================================================================

/// Result of a phase execution
#[derive(Debug, Clone)]
pub struct PhaseResult {
    pub phase_id: String,
    pub status: PhaseStatus,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub duration: Duration,
    pub tool_results: Vec<ToolResult>,
    pub agent_results: Vec<AgentResult>,
}

/// Result of a tool call
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool: String,
    pub success: bool,
    pub result: Value,
    pub duration: Duration,
}

/// Result of an agent call
#[derive(Debug, Clone)]
pub struct AgentResult {
    pub agent_id: String,
    pub operation: String,
    pub success: bool,
    pub result: Value,
    pub duration: Duration,
}

/// Result of workstack execution
#[derive(Debug, Clone)]
pub struct WorkstackResult {
    pub workstack_id: String,
    pub execution_id: String,
    pub success: bool,
    pub phases: Vec<PhaseResult>,
    pub duration: Duration,
    pub variables: HashMap<String, Value>,
    pub error: Option<String>,
}

/// Events emitted during execution
#[derive(Debug, Clone)]
pub enum WorkstackEvent {
    Started {
        workstack_id: String,
        execution_id: String,
        total_phases: usize,
    },
    PhaseStarted {
        phase_id: String,
        phase_name: String,
        index: usize,
        total: usize,
    },
    PhaseProgress {
        phase_id: String,
        message: String,
        progress: f32,
    },
    PhaseCompleted {
        phase_id: String,
        status: PhaseStatus,
        duration: Duration,
    },
    ToolOutput {
        phase_id: String,
        tool: String,
        chunk: StreamChunk,
    },
    AgentOutput {
        phase_id: String,
        agent_id: String,
        chunk: StreamChunk,
    },
    Completed {
        workstack_id: String,
        execution_id: String,
        success: bool,
        duration: Duration,
    },
    RollbackStarted {
        phase_id: String,
    },
    RollbackCompleted {
        phase_id: String,
        success: bool,
    },
    Error {
        phase_id: Option<String>,
        error: String,
    },
}

// ============================================================================
// EXECUTION CONTEXT
// ============================================================================

/// Context for workstack execution
struct ExecutionContext {
    scope: VerifiedScope,
    event_sink: Option<Arc<dyn WorkstackEventSink>>,
    execution_id: String,
    workstack: Workstack,
    variables: HashMap<String, Value>,
    phase_results: HashMap<String, PhaseResult>,
    completed_phases: HashSet<String>,
    event_tx: Option<mpsc::Sender<WorkstackEvent>>,
    started_at: Instant,
}

impl ExecutionContext {
    fn new(
        scope: VerifiedScope,
        workstack: Workstack,
        input_variables: HashMap<String, String>,
        event_tx: Option<mpsc::Sender<WorkstackEvent>>,
    ) -> Self {
        let execution_id = format!("{}-{}", workstack.id, uuid::Uuid::new_v4());

        // Merge workstack variables with input variables
        let mut variables: HashMap<String, Value> = workstack
            .variables
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();

        for (k, v) in input_variables {
            variables.insert(k, Value::String(v));
        }

        Self {
            scope,
            event_sink: None,
            execution_id,
            workstack,
            variables,
            phase_results: HashMap::new(),
            completed_phases: HashSet::new(),
            event_tx,
            started_at: Instant::now(),
        }
    }

    /// Store a variable
    fn set_variable(&mut self, key: &str, value: Value) {
        self.variables.insert(key.to_string(), value);
    }

    /// Get a variable
    #[allow(dead_code)]
    fn get_variable(&self, key: &str) -> Option<&Value> {
        self.variables.get(key)
    }

    /// Interpolate variables in a string
    fn interpolate(&self, s: &str) -> String {
        let mut result = s.to_string();

        for (key, value) in &self.variables {
            let placeholder = format!("${{{}}}", key);
            let value_str = if let Some(s) = value.as_str() {
                s.to_string()
            } else if let Some(b) = value.as_bool() {
                b.to_string()
            } else if let Some(n) = value.as_i64() {
                n.to_string()
            } else if let Some(n) = value.as_u64() {
                n.to_string()
            } else if let Some(n) = value.as_f64() {
                n.to_string()
            } else {
                simd_json::to_string(value).unwrap_or_default()
            };
            result = result.replace(&placeholder, &value_str);
        }

        result
    }

    /// Interpolate variables in a JSON value
    fn interpolate_value(&self, value: &Value) -> Value {
        match value {
            Value::String(s) => Value::String(self.interpolate(s)),
            Value::Array(arr) => {
                Value::Array(arr.iter().map(|v| self.interpolate_value(v)).collect())
            }
            Value::Object(obj) => {
                let interpolated: simd_json::value::owned::Object = obj
                    .iter()
                    .map(|(k, v): (&String, &Value)| (k.clone(), self.interpolate_value(v)))
                    .collect();
                Value::Object(Box::new(interpolated))
            }
            other => other.clone(),
        }
    }

    /// Send an event
    async fn emit(&self, event: WorkstackEvent) {
        if let Some(sink) = &self.event_sink {
            sink.on_event(&self.scope, &self.execution_id, &event);
        }
        if let Some(tx) = &self.event_tx {
            let _ = tx.send(event).await;
        }
    }

    /// Check if a phase's dependencies are satisfied
    fn dependencies_satisfied(&self, phase: &WorkstackPhase) -> bool {
        phase
            .depends_on
            .iter()
            .all(|dep| self.completed_phases.contains(dep))
    }

    /// Check if a phase's condition is met
    fn condition_met(&self, phase: &WorkstackPhase) -> bool {
        // Simple condition evaluation
        // In a real implementation, use a proper expression evaluator
        if let Some(condition) = &phase.condition {
            // Check for simple variable existence: "${var}"
            if condition.starts_with("${") && condition.ends_with("}") {
                let var_name = &condition[2..condition.len() - 1];
                return self.variables.contains_key(var_name);
            }

            // Check for "phase_id.success" pattern
            if condition.ends_with(".success") {
                let phase_id = &condition[..condition.len() - 8];
                return self
                    .phase_results
                    .get(phase_id)
                    .map(|r| r.status == PhaseStatus::Completed)
                    .unwrap_or(false);
            }

            // Default: condition not understood, assume true
            true
        } else {
            true
        }
    }
}

// ============================================================================
// WORKSTACK EXECUTOR
// ============================================================================

/// Trait for tool execution (to be implemented by the caller)
pub trait ToolExecutor: Send + Sync {
    /// Implement through the canonical dispatcher with the sealed per-operation
    /// capability. The unscoped method is never a fallback for workstacks.
    fn execute_scoped<'a>(
        &'a self,
        _scope: &'a VerifiedScope,
        _tool: &'a str,
        _arguments: &'a Value,
    ) -> super::ScopeFuture<'a, Value> {
        Box::pin(async {
            Err(OrchestrationError::new(
                ErrorCode::OperationNotSupported,
                "scoped tool dispatch is not wired",
            ))
        })
    }

    fn execute<'a>(
        &'a self,
        tool: &'a str,
        arguments: &'a Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = OrchestrationResult<Value>> + Send + 'a>>;
}

/// Workstack executor
pub struct WorkstackExecutor {
    agent_pool: Arc<GrpcAgentPool>,
    tool_executor: Arc<dyn ToolExecutor>,
    workstacks: RwLock<HashMap<String, Workstack>>,
    executions: RwLock<HashMap<String, ExecutionState>>,
    scope_verifier: Option<Arc<dyn ScopeVerifier>>,
    event_sink: Option<Arc<dyn WorkstackEventSink>>,
}

/// State of an execution
#[derive(Debug)]
struct ExecutionState {
    owner: VerifiedScope,
    cancel_tx: tokio::sync::watch::Sender<bool>,
    #[allow(dead_code)]
    execution_id: String,
    #[allow(dead_code)]
    workstack_id: String,
    status: ExecutionStatus,
    #[allow(dead_code)]
    current_phase: Option<String>,
    #[allow(dead_code)]
    started_at: Instant,
    completed_at: Option<Instant>,
}

/// Execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    RollingBack,
    RolledBack,
}

impl WorkstackExecutor {
    /// Create a new workstack executor
    pub fn new(agent_pool: Arc<GrpcAgentPool>, tool_executor: Arc<dyn ToolExecutor>) -> Self {
        Self {
            agent_pool,
            tool_executor,
            workstacks: RwLock::new(HashMap::new()),
            executions: RwLock::new(HashMap::new()),
            scope_verifier: None,
            event_sink: None,
        }
    }

    pub fn with_scope_verifier(mut self, verifier: Arc<dyn ScopeVerifier>) -> Self {
        self.scope_verifier = Some(verifier);
        self
    }

    pub fn with_event_sink(mut self, sink: Arc<dyn WorkstackEventSink>) -> Self {
        self.event_sink = Some(sink);
        self
    }

    async fn verified_scope(&self, session_id: &str) -> OrchestrationResult<VerifiedScope> {
        let verifier = self.scope_verifier.as_ref().ok_or_else(|| {
            OrchestrationError::new(
                ErrorCode::OperationNotSupported,
                "scope verifier is required",
            )
        })?;
        let scope = verifier.verify(session_id).await?;
        if scope.session_id != session_id
            || scope.principal_id.is_empty()
            || scope.session_id.is_empty()
            || scope.session_genesis.is_empty()
        {
            return Err(OrchestrationError::new(
                ErrorCode::SessionInvalid,
                "invalid verified session scope",
            ));
        }
        Ok(scope)
    }

    /// Register a workstack
    pub async fn register(&self, workstack: Workstack) {
        let id = workstack.id.clone();
        self.workstacks.write().await.insert(id.clone(), workstack);
        info!(workstack = %id, "Registered workstack");
    }

    /// Register multiple workstacks
    pub async fn register_all(&self, workstacks: Vec<Workstack>) {
        for ws in workstacks {
            self.register(ws).await;
        }
    }

    /// Get a workstack by ID
    pub async fn get(&self, workstack_id: &str) -> Option<Workstack> {
        self.workstacks.read().await.get(workstack_id).cloned()
    }

    /// List all workstacks
    pub async fn list(&self) -> Vec<WorkstackInfo> {
        self.workstacks
            .read()
            .await
            .values()
            .map(|ws| WorkstackInfo {
                id: ws.id.clone(),
                name: ws.name.clone(),
                description: ws.description.clone(),
                category: ws.category.clone(),
                phase_count: ws.phases.len(),
                required_agents: ws.required_agents.clone(),
            })
            .collect()
    }

    /// Execute a workstack
    #[instrument(skip(self, event_tx), fields(
        session_id = %session_id,
        workstack_id = %workstack_id
    ))]
    pub async fn execute(
        &self,
        session_id: &str,
        workstack_id: &str,
        variables: HashMap<String, String>,
        event_tx: Option<mpsc::Sender<WorkstackEvent>>,
    ) -> OrchestrationResult<WorkstackResult> {
        let scope = self.verified_scope(session_id).await?;
        // Get workstack
        let workstack = self
            .workstacks
            .read()
            .await
            .get(workstack_id)
            .cloned()
            .ok_or_else(|| OrchestrationError::workstack_not_found(workstack_id))?;

        // Validate dependencies (detect cycles)
        self.validate_dependencies(&workstack)?;

        // Create execution context
        let mut ctx = ExecutionContext::new(scope.clone(), workstack.clone(), variables, event_tx);
        ctx.event_sink = self.event_sink.clone();
        let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);

        // Track execution
        let execution_state = ExecutionState {
            owner: scope,
            cancel_tx,
            execution_id: ctx.execution_id.clone(),
            workstack_id: workstack_id.to_string(),
            status: ExecutionStatus::Running,
            current_phase: None,
            started_at: Instant::now(),
            completed_at: None,
        };

        self.executions
            .write()
            .await
            .insert(ctx.execution_id.clone(), execution_state);

        info!(
            execution_id = %ctx.execution_id,
            phases = workstack.phases.len(),
            "Starting workstack execution"
        );

        // Emit started event
        ctx.emit(WorkstackEvent::Started {
            workstack_id: workstack_id.to_string(),
            execution_id: ctx.execution_id.clone(),
            total_phases: workstack.phases.len(),
        })
        .await;

        // Execute phases
        let mut result = tokio::select! {
            biased;
            _ = cancel_rx.changed() => Err(OrchestrationError::new(
                ErrorCode::ExecutionCancelled, "Workstack execution cancelled",
            )),
            result = self.execute_phases(&mut ctx) => result,
        };

        // Update execution state
        {
            let mut executions = self.executions.write().await;
            if let Some(state) = executions.get_mut(&ctx.execution_id) {
                state.status = if state.status == ExecutionStatus::Cancelled {
                    result = Err(OrchestrationError::new(
                        ErrorCode::ExecutionCancelled,
                        "Workstack execution cancelled",
                    ));
                    ExecutionStatus::Cancelled
                } else if result.is_ok() {
                    ExecutionStatus::Completed
                } else {
                    ExecutionStatus::Failed
                };
                state.completed_at = Some(Instant::now());
            }
        }

        // Build result
        let duration = ctx.started_at.elapsed();
        let phases: Vec<PhaseResult> = ctx.phase_results.values().cloned().collect();
        let success = result.is_ok()
            && phases
                .iter()
                .all(|p| p.status == PhaseStatus::Completed || p.status == PhaseStatus::Skipped);

        if let Some(state) = self.executions.write().await.get_mut(&ctx.execution_id)
            && state.status != ExecutionStatus::Cancelled
        {
            state.status = if success {
                ExecutionStatus::Completed
            } else {
                ExecutionStatus::Failed
            };
        }
        if let Err(error) = &result {
            ctx.emit(WorkstackEvent::Error {
                phase_id: None,
                error: error.to_string(),
            })
            .await;
        }

        // Emit completed event
        ctx.emit(WorkstackEvent::Completed {
            workstack_id: workstack_id.to_string(),
            execution_id: ctx.execution_id.clone(),
            success,
            duration,
        })
        .await;

        Ok(WorkstackResult {
            workstack_id: workstack_id.to_string(),
            execution_id: ctx.execution_id.clone(),
            success,
            phases,
            duration,
            variables: ctx.variables,
            error: result.err().map(|e| e.to_string()),
        })
    }

    /// Execute phases in dependency order
    async fn execute_phases(&self, ctx: &mut ExecutionContext) -> OrchestrationResult<()> {
        let phase_ids: Vec<String> = ctx.workstack.phases.iter().map(|p| p.id.clone()).collect();
        let total_phases = phase_ids.len();

        // Execute phases in order (respecting dependencies)
        for (index, phase_id) in phase_ids.iter().enumerate() {
            // Find the phase
            let phase = ctx
                .workstack
                .phases
                .iter()
                .find(|p| &p.id == phase_id)
                .cloned()
                .ok_or_else(|| {
                    OrchestrationError::new(
                        ErrorCode::PhaseNotFound,
                        format!("Phase not found: {}", phase_id),
                    )
                })?;

            // Check dependencies
            if !ctx.dependencies_satisfied(&phase) {
                warn!(phase = %phase_id, "Dependencies not satisfied, skipping");

                let result = PhaseResult {
                    phase_id: phase_id.clone(),
                    status: PhaseStatus::Skipped,
                    result: None,
                    error: Some("Dependencies not satisfied".to_string()),
                    duration: Duration::ZERO,
                    tool_results: vec![],
                    agent_results: vec![],
                };

                ctx.phase_results.insert(phase_id.clone(), result);
                continue;
            }

            // Check condition
            if !ctx.condition_met(&phase) {
                info!(phase = %phase_id, "Condition not met, skipping");

                let result = PhaseResult {
                    phase_id: phase_id.clone(),
                    status: PhaseStatus::Skipped,
                    result: None,
                    error: Some("Condition not met".to_string()),
                    duration: Duration::ZERO,
                    tool_results: vec![],
                    agent_results: vec![],
                };

                ctx.phase_results.insert(phase_id.clone(), result);
                ctx.completed_phases.insert(phase_id.clone());
                continue;
            }

            // Emit phase started
            ctx.emit(WorkstackEvent::PhaseStarted {
                phase_id: phase_id.clone(),
                phase_name: phase.name.clone(),
                index,
                total: total_phases,
            })
            .await;

            // Execute the phase
            let phase_result = self.execute_phase(ctx, &phase).await;

            // Handle result
            match phase_result {
                Ok(result) => {
                    ctx.emit(WorkstackEvent::PhaseCompleted {
                        phase_id: phase_id.clone(),
                        status: result.status,
                        duration: result.duration,
                    })
                    .await;

                    ctx.phase_results.insert(phase_id.clone(), result);
                    ctx.completed_phases.insert(phase_id.clone());
                }
                Err(e) => {
                    error!(phase = %phase_id, error = %e, "Phase failed");

                    let result = PhaseResult {
                        phase_id: phase_id.clone(),
                        status: PhaseStatus::Failed,
                        result: None,
                        error: Some(e.to_string()),
                        duration: Duration::ZERO,
                        tool_results: vec![],
                        agent_results: vec![],
                    };

                    ctx.emit(WorkstackEvent::PhaseCompleted {
                        phase_id: phase_id.clone(),
                        status: PhaseStatus::Failed,
                        duration: Duration::ZERO,
                    })
                    .await;

                    ctx.phase_results.insert(phase_id.clone(), result);

                    // Execute rollback for this phase
                    if !phase.rollback.is_empty() {
                        self.execute_rollback(ctx, &phase).await;
                    }

                    // Check if we should continue
                    if !phase.continue_on_failure {
                        return Err(OrchestrationError::phase_failed(phase_id, &e.to_string()));
                    }
                }
            }
        }

        Ok(())
    }

    /// Execute a single phase
    async fn execute_phase(
        &self,
        ctx: &mut ExecutionContext,
        phase: &WorkstackPhase,
    ) -> OrchestrationResult<PhaseResult> {
        let start = Instant::now();

        info!(
            phase = %phase.id,
            name = %phase.name,
            tools = phase.tools.len(),
            agents = phase.agents.len(),
            "Executing phase"
        );

        let mut tool_results = Vec::new();
        let mut agent_results = Vec::new();

        // Execute tools first
        for tool_call in &phase.tools {
            let args = ctx.interpolate_value(&tool_call.arguments);

            debug!(tool = %tool_call.tool, "Executing tool");

            let tool_start = Instant::now();
            let result = self
                .tool_executor
                .execute_scoped(&ctx.scope, &tool_call.tool, &args)
                .await;
            let tool_duration = tool_start.elapsed();

            match result {
                Ok(value) => {
                    // Store result if requested
                    if let Some(store_as) = &tool_call.store_as {
                        ctx.set_variable(store_as, value.clone());
                    }

                    tool_results.push(ToolResult {
                        tool: tool_call.tool.clone(),
                        success: true,
                        result: value,
                        duration: tool_duration,
                    });
                }
                Err(e) => {
                    warn!(tool = %tool_call.tool, error = %e, "Tool failed");

                    tool_results.push(ToolResult {
                        tool: tool_call.tool.clone(),
                        success: false,
                        result: simd_json::json!({ "error": e.to_string() }),
                        duration: tool_duration,
                    });

                    return Err(OrchestrationError::execution_failed(
                        "tool",
                        &tool_call.tool,
                        &e.to_string(),
                    ));
                }
            }
        }

        // Execute agents
        for agent_id in &phase.agents {
            let operation = phase.agent_operation.as_deref().unwrap_or("execute");

            let args = phase
                .agent_arguments
                .as_ref()
                .map(|a| ctx.interpolate_value(a))
                .unwrap_or(simd_json::json!({}));

            debug!(agent = %agent_id, operation = %operation, "Executing agent");

            let agent_start = Instant::now();

            // Use streaming for known streaming agents
            let result: OrchestrationResult<Value> =
                if Self::is_streaming_agent(agent_id, operation) {
                    let phase_id = phase.id.clone();
                    let agent_id_clone = agent_id.clone();
                    let event_tx = ctx.event_tx.clone();
                    let event_sink = ctx.event_sink.clone();
                    let scope = ctx.scope.clone();
                    let execution_id = ctx.execution_id.clone();

                    self.agent_pool
                        .execute_streaming_scoped(
                            &ctx.scope,
                            agent_id,
                            operation,
                            args,
                            move |chunk| {
                                let event = WorkstackEvent::AgentOutput {
                                    phase_id: phase_id.clone(),
                                    agent_id: agent_id_clone.clone(),
                                    chunk,
                                };
                                if let Some(sink) = &event_sink {
                                    sink.on_event(&scope, &execution_id, &event);
                                }
                                if let Some(tx) = &event_tx {
                                    let _ = tx.try_send(event);
                                }
                            },
                        )
                        .await
                } else {
                    self.agent_pool
                        .execute_scoped(&ctx.scope, agent_id, operation, args)
                        .await
                };

            let agent_duration = agent_start.elapsed();

            match result {
                Ok(value) => {
                    agent_results.push(AgentResult {
                        agent_id: agent_id.clone(),
                        operation: operation.to_string(),
                        success: true,
                        result: value,
                        duration: agent_duration,
                    });
                }
                Err(e) => {
                    warn!(agent = %agent_id, error = %e, "Agent failed");

                    agent_results.push(AgentResult {
                        agent_id: agent_id.clone(),
                        operation: operation.to_string(),
                        success: false,
                        result: simd_json::json!({ "error": e.to_string() }),
                        duration: agent_duration,
                    });

                    return Err(OrchestrationError::execution_failed(
                        agent_id,
                        operation,
                        &e.to_string(),
                    ));
                }
            }
        }

        let duration = start.elapsed();

        Ok(PhaseResult {
            phase_id: phase.id.clone(),
            status: PhaseStatus::Completed,
            result: Some(simd_json::json!({
                "tools": tool_results.len(),
                "agents": agent_results.len(),
            })),
            error: None,
            duration,
            tool_results,
            agent_results,
        })
    }

    /// Execute rollback for a failed phase
    async fn execute_rollback(&self, ctx: &mut ExecutionContext, phase: &WorkstackPhase) {
        let admitted = self
            .authorize_execution_control(
                &ctx.scope.session_id,
                &ctx.execution_id,
                ExecutionControl::Rollback,
            )
            .await
            .and_then(|caller| {
                if caller == ctx.scope {
                    Ok(())
                } else {
                    Err(OrchestrationError::new(
                        ErrorCode::PermissionDenied,
                        "rollback owner scope changed",
                    ))
                }
            });
        if let Err(error) = admitted {
            ctx.emit(WorkstackEvent::Error {
                phase_id: Some(phase.id.clone()),
                error: error.to_string(),
            })
            .await;
            return;
        }
        info!(phase = %phase.id, "Executing rollback");

        ctx.emit(WorkstackEvent::RollbackStarted {
            phase_id: phase.id.clone(),
        })
        .await;

        let mut success = true;

        for rollback_call in &phase.rollback {
            let args = ctx.interpolate_value(&rollback_call.arguments);

            debug!(tool = %rollback_call.tool, "Executing rollback tool");

            if let Err(e) = self
                .tool_executor
                .execute_scoped(&ctx.scope, &rollback_call.tool, &args)
                .await
            {
                error!(
                    tool = %rollback_call.tool,
                    error = %e,
                    "Rollback tool failed"
                );
                success = false;
            }
        }

        ctx.emit(WorkstackEvent::RollbackCompleted {
            phase_id: phase.id.clone(),
            success,
        })
        .await;

        // Update phase status
        if let Some(result) = ctx.phase_results.get_mut(&phase.id)
            && success
        {
            result.status = PhaseStatus::RolledBack;
        }
    }

    /// Check if an agent operation should use streaming
    fn is_streaming_agent(agent_id: &str, operation: &str) -> bool {
        // Convention: operations with "stream" in name are streaming
        if operation.contains("stream") {
            return true;
        }

        // Legacy/Specific overrides
        matches!(
            (agent_id, operation),
            ("rust_pro", "build")
                | ("rust_pro", "test")
                | ("rust_pro", "clippy")
                | ("rust_pro", "doc")
                | ("sequential_thinking", _)
        )
    }

    /// Validate workstack dependencies (detect cycles)
    #[allow(clippy::result_large_err)]
    fn validate_dependencies(&self, workstack: &Workstack) -> OrchestrationResult<()> {
        let phase_ids: HashSet<String> = workstack.phases.iter().map(|p| p.id.clone()).collect();

        // Check that all dependencies exist
        for phase in &workstack.phases {
            for dep in &phase.depends_on {
                if !phase_ids.contains(dep) {
                    return Err(OrchestrationError::new(
                        ErrorCode::DependencyFailed,
                        format!("Phase {} depends on unknown phase {}", phase.id, dep),
                    ));
                }
            }
        }

        // Detect cycles using DFS
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        for phase in &workstack.phases {
            if self.has_cycle(phase, workstack, &mut visited, &mut rec_stack) {
                return Err(OrchestrationError::new(
                    ErrorCode::CircularDependency,
                    format!("Circular dependency detected involving phase {}", phase.id),
                ));
            }
        }

        Ok(())
    }

    fn has_cycle(
        &self,
        phase: &WorkstackPhase,
        workstack: &Workstack,
        visited: &mut HashSet<String>,
        rec_stack: &mut HashSet<String>,
    ) -> bool {
        if rec_stack.contains(&phase.id) {
            return true;
        }

        if visited.contains(&phase.id) {
            return false;
        }

        visited.insert(phase.id.clone());
        rec_stack.insert(phase.id.clone());

        for dep_id in &phase.depends_on {
            if let Some(dep_phase) = workstack.phases.iter().find(|p| &p.id == dep_id)
                && self.has_cycle(dep_phase, workstack, visited, rec_stack)
            {
                return true;
            }
        }

        rec_stack.remove(&phase.id);
        false
    }

    /// Cancel a running execution
    pub async fn cancel(&self, execution_id: &str) -> OrchestrationResult<bool> {
        let _ = execution_id;
        Err(OrchestrationError::new(
            ErrorCode::OperationNotSupported,
            "cancel requires verified caller scope",
        ))
    }

    /// Work C uses this for Cancel/Rollback admission. Caller fields are resolved
    /// again through the verifier; equality includes the session genesis.
    pub async fn authorize_execution_control(
        &self,
        caller_session_id: &str,
        execution_id: &str,
        operation: ExecutionControl,
    ) -> OrchestrationResult<VerifiedScope> {
        let caller = self.verified_scope(caller_session_id).await?;
        let owner = self
            .executions
            .read()
            .await
            .get(execution_id)
            .map(|state| state.owner.clone())
            .ok_or_else(|| OrchestrationError::invalid_arguments("execution not found"))?;
        if caller != owner {
            self.scope_verifier
                .as_ref()
                .ok_or_else(|| OrchestrationError::invalid_arguments("scope verifier missing"))?
                .authorize_control(&caller, &owner, operation)
                .await?;
        }
        Ok(caller)
    }

    pub async fn cancel_scoped(
        &self,
        caller_session_id: &str,
        execution_id: &str,
    ) -> OrchestrationResult<bool> {
        self.authorize_execution_control(caller_session_id, execution_id, ExecutionControl::Cancel)
            .await?;
        let mut executions = self.executions.write().await;

        if let Some(state) = executions.get_mut(execution_id)
            && state.status == ExecutionStatus::Running
        {
            state.status = ExecutionStatus::Cancelled;
            state.cancel_tx.send_replace(true);
            info!(execution_id = %execution_id, "Execution cancelled");
            return Ok(true);
        }

        Ok(false)
    }

    /// Get execution status
    pub async fn get_status(&self, execution_id: &str) -> Option<ExecutionStatus> {
        self.executions
            .read()
            .await
            .get(execution_id)
            .map(|s| s.status)
    }
}

/// Summary info for a workstack
#[derive(Debug, Clone)]
pub struct WorkstackInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: Option<String>,
    pub phase_count: usize,
    pub required_agents: Vec<String>,
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(session: &str, genesis: &str) -> VerifiedScope {
        VerifiedScope {
            principal_id: "principal".into(),
            session_id: session.into(),
            session_genesis: genesis.into(),
        }
    }

    struct Verifier;
    impl ScopeVerifier for Verifier {
        fn verify<'a>(&'a self, session: &'a str) -> super::super::ScopeFuture<'a, VerifiedScope> {
            Box::pin(async move { Ok(scope(session, "genesis")) })
        }
    }

    struct Sink(std::sync::Mutex<Vec<(VerifiedScope, String, WorkstackEvent)>>);
    impl WorkstackEventSink for Sink {
        fn on_event(&self, scope: &VerifiedScope, id: &str, event: &WorkstackEvent) {
            self.0
                .lock()
                .unwrap()
                .push((scope.clone(), id.into(), event.clone()));
        }
    }

    #[tokio::test]
    async fn unverified_execution_and_unscoped_cancel_are_refused() {
        let executor = WorkstackExecutor::new(
            Arc::new(GrpcAgentPool::with_defaults()),
            Arc::new(MockToolExecutor),
        );
        executor
            .register(Workstack::new("test", "Test", "fixture"))
            .await;
        assert!(
            executor
                .execute("default", "test", HashMap::new(), None)
                .await
                .is_err()
        );
        assert!(executor.executions.read().await.is_empty());
        assert!(executor.cancel("unknown").await.is_err());
    }

    #[tokio::test]
    async fn ownership_includes_session_and_genesis_for_both_controls() {
        let executor = WorkstackExecutor::new(
            Arc::new(GrpcAgentPool::with_defaults()),
            Arc::new(MockToolExecutor),
        )
        .with_scope_verifier(Arc::new(Verifier));
        let (cancel_tx, _rx) = tokio::sync::watch::channel(false);
        executor.executions.write().await.insert(
            "run".into(),
            ExecutionState {
                owner: scope("owner", "old-genesis"),
                cancel_tx,
                execution_id: "run".into(),
                workstack_id: "test".into(),
                status: ExecutionStatus::Running,
                current_phase: None,
                started_at: Instant::now(),
                completed_at: None,
            },
        );
        for operation in [ExecutionControl::Cancel, ExecutionControl::Rollback] {
            for caller in ["other", "owner"] {
                assert!(
                    executor
                        .authorize_execution_control(caller, "run", operation)
                        .await
                        .is_err()
                );
            }
        }
        assert_eq!(
            executor.get_status("run").await,
            Some(ExecutionStatus::Running)
        );
        executor
            .executions
            .write()
            .await
            .get_mut("run")
            .unwrap()
            .owner = scope("owner", "genesis");
        assert!(executor.cancel_scoped("owner", "run").await.unwrap());
        assert_eq!(
            executor.get_status("run").await,
            Some(ExecutionStatus::Cancelled)
        );
    }

    struct BlockingTool(Arc<tokio::sync::Notify>);

    #[tokio::test]
    async fn sink_receives_phase_completion_and_unique_execution_binding() {
        let sink = Arc::new(Sink(std::sync::Mutex::new(Vec::new())));
        let executor = WorkstackExecutor::new(
            Arc::new(GrpcAgentPool::with_defaults()),
            Arc::new(MockToolExecutor),
        )
        .with_scope_verifier(Arc::new(Verifier))
        .with_event_sink(sink.clone());
        executor
            .register(
                Workstack::new("test", "Test", "fixture").with_phase(WorkstackPhase {
                    id: "phase".into(),
                    ..Default::default()
                }),
            )
            .await;
        let first = executor
            .execute("owner", "test", HashMap::new(), None)
            .await
            .unwrap();
        let second = executor
            .execute("owner", "test", HashMap::new(), None)
            .await
            .unwrap();
        assert!(first.success && second.success);
        assert_ne!(first.execution_id, second.execution_id);
        let events = sink.0.lock().unwrap();
        for id in [&first.execution_id, &second.execution_id] {
            let run_events: Vec<_> = events.iter().filter(|(_, run, _)| run == id).collect();
            assert_eq!(run_events.len(), 4);
            assert!(
                run_events
                    .iter()
                    .all(|(caller, _, _)| caller == &scope("owner", "genesis"))
            );
            assert!(matches!(
                run_events[2].2,
                WorkstackEvent::PhaseCompleted {
                    status: PhaseStatus::Completed,
                    ..
                }
            ));
            assert!(matches!(
                run_events[3].2,
                WorkstackEvent::Completed { success: true, .. }
            ));
        }
    }
    impl ToolExecutor for BlockingTool {
        fn execute<'a>(
            &'a self,
            _tool: &'a str,
            _args: &'a Value,
        ) -> super::super::ScopeFuture<'a, Value> {
            Box::pin(async { panic!("unscoped fallback must never run") })
        }
        fn execute_scoped<'a>(
            &'a self,
            caller: &'a VerifiedScope,
            _tool: &'a str,
            _args: &'a Value,
        ) -> super::super::ScopeFuture<'a, Value> {
            Box::pin(async move {
                assert_eq!(caller, &scope("owner", "genesis"));
                self.0.notify_one();
                std::future::pending().await
            })
        }
    }

    #[tokio::test]
    async fn cancel_interrupts_active_tool_and_sink_receives_scoped_terminal() {
        let entered = Arc::new(tokio::sync::Notify::new());
        let sink = Arc::new(Sink(std::sync::Mutex::new(Vec::new())));
        let executor = Arc::new(
            WorkstackExecutor::new(
                Arc::new(GrpcAgentPool::with_defaults()),
                Arc::new(BlockingTool(entered.clone())),
            )
            .with_scope_verifier(Arc::new(Verifier))
            .with_event_sink(sink.clone()),
        );
        executor
            .register(
                Workstack::new("test", "Test", "fixture").with_phase(WorkstackPhase {
                    id: "phase".into(),
                    tools: vec![PhaseToolCall {
                        tool: "blocked".into(),
                        arguments: simd_json::json!({}),
                        store_as: None,
                        retries: 0,
                    }],
                    ..Default::default()
                }),
            )
            .await;
        let runner = executor.clone();
        let task =
            tokio::spawn(
                async move { runner.execute("owner", "test", HashMap::new(), None).await },
            );
        tokio::time::timeout(Duration::from_secs(2), entered.notified())
            .await
            .unwrap();
        let id = sink.0.lock().unwrap()[0].1.clone();
        assert!(executor.cancel_scoped("other", &id).await.is_err());
        assert!(executor.cancel_scoped("owner", &id).await.unwrap());
        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(!result.success);
        assert_eq!(
            executor.get_status(&id).await,
            Some(ExecutionStatus::Cancelled)
        );
        let events = sink.0.lock().unwrap();
        assert!(
            events
                .iter()
                .all(|(caller, run, _)| caller == &scope("owner", "genesis") && run == &id)
        );
        assert!(
            events
                .iter()
                .any(|(_, _, event)| matches!(event, WorkstackEvent::PhaseStarted { .. }))
        );
        assert!(matches!(
            events.last().unwrap().2,
            WorkstackEvent::Completed { success: false, .. }
        ));
    }

    #[tokio::test]
    async fn scope_cannot_fall_back_to_unwired_agent_or_unscoped_tool() {
        let pool = GrpcAgentPool::with_defaults();
        let caller = scope("owner", "genesis");
        assert_eq!(
            pool.execute_scoped(&caller, "rust_pro", "check", simd_json::json!({}))
                .await
                .unwrap_err()
                .code,
            ErrorCode::OperationNotSupported
        );
        assert_eq!(
            MockToolExecutor
                .execute_scoped(&caller, "tool", &simd_json::json!({}))
                .await
                .unwrap_err()
                .code,
            ErrorCode::OperationNotSupported
        );
    }

    struct MockToolExecutor;

    impl ToolExecutor for MockToolExecutor {
        fn execute<'a>(
            &'a self,
            tool: &'a str,
            _arguments: &'a Value,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = OrchestrationResult<Value>> + Send + 'a>,
        > {
            Box::pin(async move {
                Ok(simd_json::json!({
                    "tool": tool,
                    "success": true,
                }))
            })
        }
    }

    #[test]
    fn test_workstack_builder() {
        let ws = Workstack::new("test", "Test", "A test workstack")
            .with_category("testing")
            .with_variable("key", "value")
            .with_timeout(300);

        assert_eq!(ws.id, "test");
        assert_eq!(ws.category, Some("testing".to_string()));
        assert_eq!(ws.variables.get("key"), Some(&"value".to_string()));
        assert_eq!(ws.timeout_secs, Some(300));
    }

    #[test]
    fn test_variable_interpolation() {
        let ws = Workstack::new("test", "Test", "Test");
        let mut ctx = ExecutionContext::new(
            VerifiedScope {
                principal_id: "principal".into(),
                session_id: "session".into(),
                session_genesis: "genesis".into(),
            },
            ws,
            HashMap::from([("name".to_string(), "world".to_string())]),
            None,
        );

        ctx.set_variable("count", Value::from(42_u64));

        assert_eq!(ctx.interpolate("Hello ${name}!"), "Hello world!");
        assert_eq!(ctx.interpolate("Count: ${count}"), "Count: 42");
    }

    #[tokio::test]
    async fn test_executor_register_get() {
        let pool = Arc::new(GrpcAgentPool::with_defaults());
        let tool_exec = Arc::new(MockToolExecutor);
        let executor = WorkstackExecutor::new(pool, tool_exec);

        let ws = Workstack::new("test-ws", "Test", "A test");
        executor.register(ws).await;

        let retrieved = executor.get("test-ws").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, "test-ws");
    }

    #[test]
    fn test_cycle_detection() {
        let pool = Arc::new(GrpcAgentPool::with_defaults());
        let tool_exec = Arc::new(MockToolExecutor);
        let executor = WorkstackExecutor::new(pool, tool_exec);

        // Valid workstack (no cycles)
        let valid = Workstack::new("valid", "Valid", "No cycles")
            .with_phase(WorkstackPhase {
                id: "a".to_string(),
                depends_on: vec![],
                ..Default::default()
            })
            .with_phase(WorkstackPhase {
                id: "b".to_string(),
                depends_on: vec!["a".to_string()],
                ..Default::default()
            });

        assert!(executor.validate_dependencies(&valid).is_ok());

        // Invalid workstack (cycle: a -> b -> a)
        let invalid = Workstack::new("invalid", "Invalid", "Has cycle")
            .with_phase(WorkstackPhase {
                id: "a".to_string(),
                depends_on: vec!["b".to_string()],
                ..Default::default()
            })
            .with_phase(WorkstackPhase {
                id: "b".to_string(),
                depends_on: vec!["a".to_string()],
                ..Default::default()
            });

        assert!(executor.validate_dependencies(&invalid).is_err());
    }
}
