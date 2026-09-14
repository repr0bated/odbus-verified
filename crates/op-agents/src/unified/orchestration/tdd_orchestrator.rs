//! TDD Orchestrator Agent
//!
//! Coordinates Test-Driven Development workflow:
//! 1. Write failing test
//! 2. Write minimal code to pass
//! 3. Refactor

use super::base::{OrchestrationAgent, WorkflowStep};
use simd_json::json;

#[allow(dead_code)]
pub struct TddOrchestrator(OrchestrationAgent);

impl TddOrchestrator {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> OrchestrationAgent {
        OrchestrationAgent::new(
            "tdd-orchestrator",
            "TDD Orchestrator",
            "Coordinates Test-Driven Development workflow: Red-Green-Refactor",
            vec!["python-executor", "rust-executor", "code-reviewer"],
        )
        .with_step(WorkflowStep {
            name: "write_test".to_string(),
            agent_id: "code-reviewer".to_string(),
            operation: "consult".to_string(),
            args_template: json!({
                "query": "Generate a failing test for the requested feature"
            }),
            condition: None,
        })
        .with_step(WorkflowStep {
            name: "run_test_red".to_string(),
            agent_id: "python-executor".to_string(),
            operation: "test".to_string(),
            args_template: json!({ "path": "." }),
            condition: Some("expect_failure".to_string()),
        })
        .with_step(WorkflowStep {
            name: "implement".to_string(),
            agent_id: "code-reviewer".to_string(),
            operation: "consult".to_string(),
            args_template: json!({
                "query": "Write minimal code to make the test pass"
            }),
            condition: None,
        })
        .with_step(WorkflowStep {
            name: "run_test_green".to_string(),
            agent_id: "python-executor".to_string(),
            operation: "test".to_string(),
            args_template: json!({ "path": "." }),
            condition: Some("expect_success".to_string()),
        })
        .with_step(WorkflowStep {
            name: "refactor".to_string(),
            agent_id: "code-reviewer".to_string(),
            operation: "review".to_string(),
            args_template: json!({
                "query": "Suggest refactoring improvements while keeping tests green"
            }),
            condition: None,
        })
    }
}

impl Default for TddOrchestrator {
    fn default() -> Self {
        Self(Self::new())
    }
}
