//! Workflows - Step-by-step execution with conditions
//!
//! Workflows provide a simpler alternative to workstacks for linear
//! sequences of tool calls with conditional branching.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Variable definition for workflows
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowVariable {
    /// Variable name
    pub name: String,
    /// Description
    #[allow(dead_code)]
    pub description: String,
    /// Default value
    #[serde(default)]
    #[allow(dead_code)]
    pub default: Option<Value>,
    /// Whether this variable is required
    #[serde(default)]
    #[allow(dead_code)]
    pub required: bool,
}

/// A single step in a workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    /// Step identifier
    pub id: String,
    /// Human-readable name
    #[allow(dead_code)]
    pub name: String,
    /// Tool to execute
    pub tool: String,
    /// Arguments (supports ${variable} interpolation)
    pub arguments: Value,
    /// Condition for execution (simple expression)
    #[serde(default)]
    pub condition: Option<String>,
    /// Store result in this variable
    #[serde(default)]
    pub store_as: Option<String>,
    /// On success, go to this step (default: next)
    #[serde(default)]
    pub on_success: Option<String>,
    /// On failure, go to this step
    #[serde(default)]
    pub on_failure: Option<String>,
    /// Retry count
    #[serde(default)]
    pub retries: u32,
    /// Timeout in seconds
    #[serde(default = "default_step_timeout")]
    #[allow(dead_code)]
    pub timeout_secs: u64,
}

fn default_step_timeout() -> u64 {
    60
}

/// Complete workflow definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    /// Unique identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description
    #[allow(dead_code)]
    pub description: String,
    /// Input variables
    #[serde(default)]
    #[allow(dead_code)]
    pub variables: Vec<WorkflowVariable>,
    /// Steps in order
    pub steps: Vec<WorkflowStep>,
    /// Category
    #[serde(default)]
    #[allow(dead_code)]
    pub category: String,
    /// Tags
    #[serde(default)]
    #[allow(dead_code)]
    pub tags: Vec<String>,
}

impl Workflow {
    /// Create a new workflow
    #[allow(dead_code)]
    pub fn new(id: &str, name: &str, description: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            variables: Vec::new(),
            steps: Vec::new(),
            category: "general".to_string(),
            tags: Vec::new(),
        }
    }

    /// Add a variable
    #[allow(dead_code)]
    pub fn with_variable(mut self, var: WorkflowVariable) -> Self {
        self.variables.push(var);
        self
    }

    /// Add a step
    #[allow(dead_code)]
    pub fn with_step(mut self, step: WorkflowStep) -> Self {
        self.steps.push(step);
        self
    }

    /// Set category
    #[allow(dead_code)]
    pub fn with_category(mut self, category: &str) -> Self {
        self.category = category.to_string();
        self
    }
}

/// Workflow execution context
#[derive(Debug, Clone, Default)]
pub struct WorkflowContext {
    /// Variables
    pub variables: HashMap<String, Value>,
    /// Step results
    pub step_results: HashMap<String, Value>,
    /// Current step index
    #[allow(dead_code)]
    pub current_step: usize,
    /// Completed steps
    pub completed_steps: Vec<String>,
    /// Failed steps
    pub failed_steps: Vec<String>,
}

impl WorkflowContext {
    /// Create from input
    pub fn new(input: Value) -> Self {
        let mut variables = HashMap::new();
        if let Some(obj) = input.as_object() {
            for (k, v) in obj {
                variables.insert(k.clone(), v.clone());
            }
        }
        Self {
            variables,
            step_results: HashMap::new(),
            current_step: 0,
            completed_steps: Vec::new(),
            failed_steps: Vec::new(),
        }
    }

    /// Interpolate variables
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

    /// Evaluate a simple condition
    pub fn evaluate_condition(&self, condition: &str) -> bool {
        // Simple condition evaluation
        // Supports: ${var}, ${var} == "value", ${var} != "value", !${var}
        let condition = condition.trim();

        if condition.starts_with('!') {
            let var_name = condition[1..].trim().trim_start_matches("${")
                .trim_end_matches('}');
            return self.variables.get(var_name)
                .map(|v| v.is_null() || v == &Value::Bool(false) || v == &json!(""))
                .unwrap_or(true);
        }

        if condition.contains("==") {
            let parts: Vec<&str> = condition.split("==").collect();
            if parts.len() == 2 {
                let var_name = parts[0].trim().trim_start_matches("${")
                    .trim_end_matches('}');
                let expected = parts[1].trim().trim_matches('"');
                return self.variables.get(var_name)
                    .map(|v| v.as_str().map(|s| s == expected).unwrap_or(false))
                    .unwrap_or(false);
            }
        }

        if condition.contains("!=") {
            let parts: Vec<&str> = condition.split("!=").collect();
            if parts.len() == 2 {
                let var_name = parts[0].trim().trim_start_matches("${")
                    .trim_end_matches('}');
                let expected = parts[1].trim().trim_matches('"');
                return self.variables.get(var_name)
                    .map(|v| v.as_str().map(|s| s != expected).unwrap_or(true))
                    .unwrap_or(true);
            }
        }

        // Just check if variable exists and is truthy
        let var_name = condition.trim_start_matches("${")
            .trim_end_matches('}');
        self.variables.get(var_name)
            .map(|v| !v.is_null() && v != &Value::Bool(false) && v != &json!(""))
            .unwrap_or(false)
    }
}

/// Step execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step_id: String,
    pub success: bool,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub skipped: bool,
}

/// Workflow execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowResult {
    pub workflow_id: String,
    pub success: bool,
    pub steps: Vec<StepResult>,
    pub variables: HashMap<String, Value>,
    pub error: Option<String>,
    #[allow(dead_code)]
    pub total_duration_ms: u64,
}

/// Workflow engine - executes workflows
pub struct WorkflowEngine {
    workflows: Arc<RwLock<HashMap<String, Workflow>>>,
}

impl WorkflowEngine {
    /// Create new engine
    pub fn new() -> Self {
        Self {
            workflows: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with default workflows
    pub fn with_defaults() -> Self {
        Self::new()
    }

    /// Register a workflow
    pub async fn register(&self, workflow: Workflow) {
        info!(id = %workflow.id, name = %workflow.name, "Registering workflow");
        self.workflows.write().await.insert(workflow.id.clone(), workflow);
    }

    /// Get workflow by ID
    pub async fn get(&self, id: &str) -> Option<Workflow> {
        self.workflows.read().await.get(id).cloned()
    }

    /// List all workflows
    #[allow(dead_code)]
    pub async fn list(&self) -> Vec<Workflow> {
        self.workflows.read().await.values().cloned().collect()
    }

    /// Execute a workflow
    pub async fn execute(
        &self,
        workflow_id: &str,
        input: Value,
        tool_executor: &dyn super::workstacks::ToolExecutorTrait,
    ) -> Result<WorkflowResult> {
        let workflow = self.get(workflow_id).await
            .ok_or_else(|| anyhow::anyhow!("Workflow not found: {}", workflow_id))?;

        info!(
            workflow_id = %workflow_id,
            name = %workflow.name,
            steps = %workflow.steps.len(),
            "Starting workflow execution"
        );

        let start = std::time::Instant::now();
        let mut context = WorkflowContext::new(input);
        let mut step_results = Vec::new();
        let mut current_step_id: Option<String> = workflow.steps.first().map(|s| s.id.clone());

        while let Some(step_id) = current_step_id.take() {
            let step = workflow.steps.iter().find(|s| s.id == step_id);
            let step = match step {
                Some(s) => s,
                None => {
                    error!(step_id = %step_id, "Step not found in workflow");
                    break;
                }
            };

            // Check condition
            if let Some(ref condition) = step.condition {
                if !context.evaluate_condition(condition) {
                    debug!(step_id = %step.id, condition = %condition, "Step skipped - condition not met");
                    step_results.push(StepResult {
                        step_id: step.id.clone(),
                        success: true,
                        result: None,
                        error: None,
                        duration_ms: 0,
                        skipped: true,
                    });
                    // Move to next step
                    current_step_id = self.get_next_step(&workflow, &step.id, true);
                    continue;
                }
            }

            // Execute step
            let step_start = std::time::Instant::now();
            let args = context.interpolate(&step.arguments);

            debug!(step_id = %step.id, tool = %step.tool, "Executing workflow step");

            let mut last_error = None;
            let mut success = false;
            let mut result_value = None;

            for attempt in 0..=step.retries {
                match tool_executor.execute_tool(&step.tool, args.clone()).await {
                    Ok(result) => {
                        if let Some(ref var_name) = step.store_as {
                            context.variables.insert(var_name.clone(), result.clone());
                        }
                        context.step_results.insert(step.id.clone(), result.clone());
                        context.completed_steps.push(step.id.clone());
                        result_value = Some(result);
                        success = true;
                        break;
                    }
                    Err(e) => {
                        last_error = Some(e.to_string());
                        if attempt < step.retries {
                            warn!(
                                step_id = %step.id,
                                attempt = attempt + 1,
                                "Step failed, retrying"
                            );
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        }
                    }
                }
            }

            let step_duration = step_start.elapsed();

            if success {
                step_results.push(StepResult {
                    step_id: step.id.clone(),
                    success: true,
                    result: result_value,
                    error: None,
                    duration_ms: step_duration.as_millis() as u64,
                    skipped: false,
                });
                current_step_id = step.on_success.clone()
                    .or_else(|| self.get_next_step(&workflow, &step.id, true));
            } else {
                context.failed_steps.push(step.id.clone());
                step_results.push(StepResult {
                    step_id: step.id.clone(),
                    success: false,
                    result: None,
                    error: last_error.clone(),
                    duration_ms: step_duration.as_millis() as u64,
                    skipped: false,
                });

                if let Some(ref failure_step) = step.on_failure {
                    current_step_id = Some(failure_step.clone());
                } else {
                    // No failure handler - stop workflow
                    return Ok(WorkflowResult {
                        workflow_id: workflow_id.to_string(),
                        success: false,
                        steps: step_results,
                        variables: context.variables,
                        error: last_error,
                        total_duration_ms: start.elapsed().as_millis() as u64,
                    });
                }
            }
        }

        let all_success = step_results.iter().all(|r| r.success || r.skipped);

        Ok(WorkflowResult {
            workflow_id: workflow_id.to_string(),
            success: all_success,
            steps: step_results,
            variables: context.variables,
            error: None,
            total_duration_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Get next step in workflow
    fn get_next_step(&self, workflow: &Workflow, current_id: &str, _success: bool) -> Option<String> {
        let current_idx = workflow.steps.iter().position(|s| s.id == current_id)?;
        workflow.steps.get(current_idx + 1).map(|s| s.id.clone())
    }
}

impl Default for WorkflowEngine {
    fn default() -> Self {
        Self::with_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_creation() {
        let wf = Workflow::new("test", "Test", "A test workflow")
            .with_category("testing")
            .with_step(WorkflowStep {
                id: "step1".to_string(),
                name: "Step 1".to_string(),
                tool: "test_tool".to_string(),
                arguments: json!({}),
                condition: None,
                store_as: None,
                on_success: None,
                on_failure: None,
                retries: 0,
                timeout_secs: 60,
            });

        assert_eq!(wf.id, "test");
        assert_eq!(wf.steps.len(), 1);
    }

    #[test]
    fn test_condition_evaluation() {
        let mut ctx = WorkflowContext::new(json!({ "enabled": true, "mode": "production" }));
        ctx.variables.insert("empty".to_string(), json!(""));

        assert!(ctx.evaluate_condition("${enabled}"));
        assert!(!ctx.evaluate_condition("!${enabled}"));
        assert!(ctx.evaluate_condition("${mode} == \"production\""));
        assert!(!ctx.evaluate_condition("${mode} == \"development\""));
        assert!(ctx.evaluate_condition("${mode} != \"development\""));
        assert!(!ctx.evaluate_condition("${empty}"));
    }
}
