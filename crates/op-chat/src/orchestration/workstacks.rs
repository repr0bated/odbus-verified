//! Workstacks - Multi-phase execution plans
//!
//! Workstacks define complex, multi-phase workflows that coordinate
//! multiple tools and agents to accomplish larger tasks.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Phase execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
    #[allow(dead_code)]
    RolledBack,
}

impl Default for PhaseStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// A single phase in a workstack
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkstackPhase {
    /// Phase identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description
    pub description: String,
    /// Tools to execute in this phase
    pub tools: Vec<PhaseToolCall>,
    /// Agents to involve in this phase
    #[serde(default)]
    pub agents: Vec<String>,
    /// Dependencies (phase IDs that must complete first)
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Condition for execution (JSON expression)
    #[serde(default)]
    pub condition: Option<String>,
    /// Rollback tools if this phase fails
    #[serde(default)]
    pub rollback: Vec<PhaseToolCall>,
    /// Whether to continue on failure
    #[serde(default)]
    pub continue_on_failure: bool,
    /// Timeout in seconds
    #[serde(default = "default_phase_timeout")]
    pub timeout_secs: u64,
    /// Current status
    #[serde(default)]
    pub status: PhaseStatus,
    /// Execution result
    #[serde(default)]
    pub result: Option<Value>,
    /// Error message if failed
    #[serde(default)]
    pub error: Option<String>,
}

fn default_phase_timeout() -> u64 {
    300
}

/// A tool call within a phase
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseToolCall {
    /// Tool name
    pub tool: String,
    /// Arguments (can reference variables with ${var})
    pub arguments: Value,
    /// Store result in this variable
    #[serde(default)]
    pub store_as: Option<String>,
    /// Retry count on failure
    #[serde(default)]
    pub retries: u32,
}

/// A complete workstack definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workstack {
    /// Unique identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description
    pub description: String,
    /// Category
    #[serde(default)]
    pub category: String,
    /// Phases in execution order
    pub phases: Vec<WorkstackPhase>,
    /// Input parameters schema
    #[serde(default)]
    pub input_schema: Value,
    /// Output schema
    #[serde(default)]
    pub output_schema: Value,
    /// Skills to activate during execution
    #[serde(default)]
    pub required_skills: Vec<String>,
    /// Tags for discovery
    #[serde(default)]
    pub tags: Vec<String>,
    /// Version
    #[serde(default = "default_version")]
    pub version: String,
}

fn default_version() -> String {
    "1.0.0".to_string()
}

impl Workstack {
    /// Create a new workstack
    pub fn new(id: &str, name: &str, description: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            category: "general".to_string(),
            phases: Vec::new(),
            input_schema: json!({}),
            output_schema: json!({}),
            required_skills: Vec::new(),
            tags: Vec::new(),
            version: default_version(),
        }
    }

    /// Add a phase
    pub fn with_phase(mut self, phase: WorkstackPhase) -> Self {
        self.phases.push(phase);
        self
    }

    /// Add a required skill
    #[allow(dead_code)]
    pub fn with_skill(mut self, skill: &str) -> Self {
        self.required_skills.push(skill.to_string());
        self
    }

    /// Set category
    #[allow(dead_code)]
    pub fn with_category(mut self, category: &str) -> Self {
        self.category = category.to_string();
        self
    }

    /// Get phases in dependency order
    pub fn ordered_phases(&self) -> Vec<&WorkstackPhase> {
        // Simple topological sort
        let mut result = Vec::new();
        let mut completed: std::collections::HashSet<&str> = std::collections::HashSet::new();
        let mut remaining: Vec<_> = self.phases.iter().collect();

        while !remaining.is_empty() {
            let mut made_progress = false;
            remaining.retain(|phase| {
                let deps_satisfied = phase.depends_on.iter().all(|dep| completed.contains(dep.as_str()));
                if deps_satisfied {
                    result.push(*phase);
                    completed.insert(&phase.id);
                    made_progress = true;
                    false // Remove from remaining
                } else {
                    true // Keep in remaining
                }
            });

            if !made_progress && !remaining.is_empty() {
                // Circular dependency - just add remaining in order
                warn!("Circular dependency detected in workstack phases");
                result.extend(remaining.drain(..));
                break;
            }
        }

        result
    }
}

/// Workstack execution context
#[derive(Debug, Clone, Default)]
pub struct WorkstackContext {
    /// Variables available during execution
    pub variables: HashMap<String, Value>,
    /// Completed phase IDs
    pub completed_phases: Vec<String>,
    /// Failed phase IDs
    #[allow(dead_code)]
    pub failed_phases: Vec<String>,
    /// Phase results
    #[allow(dead_code)]
    pub phase_results: HashMap<String, Value>,
}

impl WorkstackContext {
    /// Create new context with input arguments
    pub fn new(input: Value) -> Self {
        let mut variables = HashMap::new();
        if let Some(obj) = input.as_object() {
            for (k, v) in obj {
                variables.insert(k.clone(), v.clone());
            }
        }
        variables.insert("input".to_string(), input);

        Self {
            variables,
            completed_phases: Vec::new(),
            failed_phases: Vec::new(),
            phase_results: HashMap::new(),
        }
    }

    /// Interpolate variables in a value
    pub fn interpolate(&self, value: &Value) -> Value {
        match value {
            Value::String(s) => {
                let mut result = s.clone();
                for (name, val) in &self.variables {
                    let pattern = format!("${{{}}}", name);
                    if result.contains(&pattern) {
                        let replacement = match val {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        result = result.replace(&pattern, &replacement);
                    }
                }
                Value::String(result)
            }
            Value::Object(obj) => {
                let mut new_obj = simd_json::value::owned::Object::new();
                for (k, v) in obj {
                    new_obj.insert(k.clone(), self.interpolate(v));
                }
                Value::Object(new_obj)
            }
            Value::Array(arr) => {
                Value::Array(arr.iter().map(|v| self.interpolate(v)).collect())
            }
            other => other.clone(),
        }
    }

    /// Set a variable
    pub fn set_variable(&mut self, name: &str, value: Value) {
        self.variables.insert(name.to_string(), value);
    }

    /// Mark phase as completed
    pub fn complete_phase(&mut self, phase_id: &str, result: Value) {
        self.completed_phases.push(phase_id.to_string());
        self.phase_results.insert(phase_id.to_string(), result);
    }

    /// Mark phase as failed
    pub fn fail_phase(&mut self, phase_id: &str, error: &str) {
        self.failed_phases.push(phase_id.to_string());
        self.phase_results.insert(
            phase_id.to_string(),
            json!({ "error": error }),
        );
    }
}

/// Workstack registry
pub struct WorkstackRegistry {
    workstacks: HashMap<String, Workstack>,
}

impl WorkstackRegistry {
    /// Create empty registry
    pub fn new() -> Self {
        Self {
            workstacks: HashMap::new(),
        }
    }

    /// Create with default workstacks
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register_default_workstacks();
        registry
    }

    /// Register a workstack
    pub fn register(&mut self, workstack: Workstack) {
        info!(id = %workstack.id, name = %workstack.name, "Registering workstack");
        self.workstacks.insert(workstack.id.clone(), workstack);
    }

    /// Get workstack by ID
    pub fn get(&self, id: &str) -> Option<&Workstack> {
        self.workstacks.get(id)
    }

    /// List all workstacks
    #[allow(dead_code)]
    pub fn list(&self) -> Vec<&Workstack> {
        self.workstacks.values().collect()
    }

    /// List by category
    #[allow(dead_code)]
    pub fn list_by_category(&self, category: &str) -> Vec<&Workstack> {
        self.workstacks
            .values()
            .filter(|w| w.category == category)
            .collect()
    }

    /// Register default workstacks
    fn register_default_workstacks(&mut self) {
        // OVS Network Setup workstack
        self.register(
            Workstack::new(
                "ovs_network_setup",
                "OVS Network Setup",
                "Set up OVS bridge with ports and flows",
            )
            .with_phase(WorkstackPhase {
                id: "verify".to_string(),
                name: "Verify Setup".to_string(),
                description: "Verify the bridge configuration".to_string(),
                tools: vec![
                    PhaseToolCall {
                        tool: "ovs_list_bridges".to_string(),
                        arguments: json!({}),
                        store_as: Some("final_bridges".to_string()),
                        retries: 0,
                    },
                ],
                agents: vec![],
                depends_on: vec![],
                condition: None,
                rollback: vec![],
                continue_on_failure: false,
                timeout_secs: 30,
                status: PhaseStatus::Pending,
                result: None,
                error: None,
            })
        );
    }
}

impl Default for WorkstackRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Workstack executor - runs workstacks
pub struct WorkstackExecutor {
    registry: Arc<RwLock<WorkstackRegistry>>,
}

impl WorkstackExecutor {
    /// Create new executor
    pub fn new(registry: Arc<RwLock<WorkstackRegistry>>) -> Self {
        Self { registry }
    }

    /// Execute a workstack by ID
    pub async fn execute(
        &self,
        workstack_id: &str,
        input: Value,
        tool_executor: &dyn ToolExecutorTrait,
    ) -> Result<WorkstackResult> {
        let registry = self.registry.read().await;
        let workstack = registry
            .get(workstack_id)
            .ok_or_else(|| anyhow::anyhow!("Workstack not found: {}", workstack_id))?;

        info!(
            workstack_id = %workstack_id,
            name = %workstack.name,
            phases = %workstack.phases.len(),
            "Starting workstack execution"
        );

        let mut context = WorkstackContext::new(input);
        let mut phase_results = Vec::new();
        let ordered_phases = workstack.ordered_phases();

        for phase in ordered_phases {
            // Check if dependencies are satisfied
            let deps_ok = phase.depends_on.iter().all(|dep| {
                context.completed_phases.contains(dep)
            });

            if !deps_ok {
                warn!(phase_id = %phase.id, "Skipping phase - dependencies not met");
                phase_results.push(PhaseResult {
                    phase_id: phase.id.clone(),
                    status: PhaseStatus::Skipped,
                    result: None,
                    error: Some("Dependencies not met".to_string()),
                    duration_ms: 0,
                });
                continue;
            }

            // Execute phase
            let phase_result = self
                .execute_phase(phase, &mut context, tool_executor)
                .await;

            match phase_result {
                Ok(result) => {
                    context.complete_phase(&phase.id, result.result.clone().unwrap_or(Value::Null));
                    phase_results.push(result);
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    context.fail_phase(&phase.id, &error_msg);
                    
                    phase_results.push(PhaseResult {
                        phase_id: phase.id.clone(),
                        status: PhaseStatus::Failed,
                        result: None,
                        error: Some(error_msg.clone()),
                        duration_ms: 0,
                    });

                    if !phase.continue_on_failure {
                        error!(phase_id = %phase.id, error = %error_msg, "Phase failed, stopping workstack");
                        
                        // Execute rollbacks
                        self.execute_rollbacks(&phase_results, &context, tool_executor).await;
                        
                        return Ok(WorkstackResult {
                            workstack_id: workstack_id.to_string(),
                            success: false,
                            phases: phase_results,
                            context: context.variables,
                            error: Some(error_msg),
                        });
                    }
                }
            }
        }

        let all_success = phase_results.iter().all(|r| {
            r.status == PhaseStatus::Completed || r.status == PhaseStatus::Skipped
        });

        Ok(WorkstackResult {
            workstack_id: workstack_id.to_string(),
            success: all_success,
            phases: phase_results,
            context: context.variables,
            error: None,
        })
    }

    /// Execute a single phase
    async fn execute_phase(
        &self,
        phase: &WorkstackPhase,
        context: &mut WorkstackContext,
        tool_executor: &dyn ToolExecutorTrait,
    ) -> Result<PhaseResult> {
        info!(phase_id = %phase.id, name = %phase.name, "Executing phase");
        let start = std::time::Instant::now();

        let mut results = Vec::new();

        for tool_call in &phase.tools {
            let args = context.interpolate(&tool_call.arguments);
            
            debug!(tool = %tool_call.tool, args = %args, "Executing phase tool");
            
            let mut last_error = None;
            let mut success = false;
            
            for attempt in 0..=tool_call.retries {
                match tool_executor.execute_tool(&tool_call.tool, args.clone()).await {
                    Ok(result) => {
                        if let Some(ref var_name) = tool_call.store_as {
                            context.set_variable(var_name, result.clone());
                        }
                        results.push(result);
                        success = true;
                        break;
                    }
                    Err(e) => {
                        last_error = Some(e.to_string());
                        if attempt < tool_call.retries {
                            warn!(
                                tool = %tool_call.tool,
                                attempt = attempt + 1,
                                max_retries = tool_call.retries,
                                "Tool execution failed, retrying"
                            );
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        }
                    }
                }
            }
            
            if !success {
                return Err(anyhow::anyhow!(
                    "Tool '{}' failed after {} attempts: {}",
                    tool_call.tool,
                    tool_call.retries + 1,
                    last_error.unwrap_or_default()
                ));
            }
        }

        let duration = start.elapsed();

        Ok(PhaseResult {
            phase_id: phase.id.clone(),
            status: PhaseStatus::Completed,
            result: Some(json!(results)),
            error: None,
            duration_ms: duration.as_millis() as u64,
        })
    }

    /// Execute rollbacks for failed phases
    async fn execute_rollbacks(
        &self,
        _phase_results: &[PhaseResult],
        _context: &WorkstackContext,
        _tool_executor: &dyn ToolExecutorTrait,
    ) {
        // TODO: Implement rollback logic
        warn!("Rollback execution not yet implemented");
    }
}

/// Result of a single phase execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseResult {
    pub phase_id: String,
    pub status: PhaseStatus,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub duration_ms: u64,
}

/// Result of workstack execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkstackResult {
    pub workstack_id: String,
    pub success: bool,
    pub phases: Vec<PhaseResult>,
    pub context: HashMap<String, Value>,
    pub error: Option<String>,
}

/// Trait for tool execution (to avoid circular dependencies)
#[async_trait]
pub trait ToolExecutorTrait: Send + Sync {
    async fn execute_tool(&self, name: &str, args: Value) -> Result<Value>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workstack_creation() {
        let ws = Workstack::new("test", "Test Workstack", "A test")
            .with_category("testing")
            .with_skill("test_skill");

        assert_eq!(ws.id, "test");
        assert_eq!(ws.category, "testing");
        assert!(ws.required_skills.contains(&"test_skill".to_string()));
    }

    #[test]
    fn test_context_interpolation() {
        let mut ctx = WorkstackContext::new(json!({ "name": "test_bridge" }));
        ctx.set_variable("bridge_name", json!("br0"));

        let value = json!({ "bridge": "${bridge_name}", "other": "static" });
        let result = ctx.interpolate(&value);

        assert_eq!(result.get("bridge").unwrap(), "br0");
        assert_eq!(result.get("other").unwrap(), "static");
    }

    #[test]
    fn test_registry_defaults() {
        let registry = WorkstackRegistry::with_defaults();
        
        assert!(registry.get("ovs_network_setup").is_some());
    }
}
