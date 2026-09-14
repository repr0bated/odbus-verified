//! Code Review Orchestrator Agent
//!
//! Coordinates comprehensive code review:
//! 1. Static analysis
//! 2. Security audit
//! 3. Architecture review
//! 4. Documentation check

use super::base::{OrchestrationAgent, WorkflowStep};
use simd_json::json;

#[allow(dead_code)]
pub struct CodeReviewOrchestrator(OrchestrationAgent);

impl CodeReviewOrchestrator {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> OrchestrationAgent {
        OrchestrationAgent::new(
            "code-review-orchestrator",
            "Code Review Orchestrator",
            "Coordinates comprehensive code review with multiple expert agents",
            vec![
                "python-executor",
                "rust-executor",
                "security-auditor",
                "code-reviewer",
                "backend-architect",
            ],
        )
        .with_step(WorkflowStep {
            name: "lint".to_string(),
            agent_id: "python-executor".to_string(),
            operation: "lint".to_string(),
            args_template: json!({ "path": ".", "fix": false }),
            condition: None,
        })
        .with_step(WorkflowStep {
            name: "typecheck".to_string(),
            agent_id: "python-executor".to_string(),
            operation: "typecheck".to_string(),
            args_template: json!({ "path": "." }),
            condition: None,
        })
        .with_step(WorkflowStep {
            name: "security_audit".to_string(),
            agent_id: "security-auditor".to_string(),
            operation: "review".to_string(),
            args_template: json!({
                "query": "Audit this code for security vulnerabilities"
            }),
            condition: None,
        })
        .with_step(WorkflowStep {
            name: "architecture_review".to_string(),
            agent_id: "backend-architect".to_string(),
            operation: "review".to_string(),
            args_template: json!({
                "query": "Review the architecture and suggest improvements"
            }),
            condition: None,
        })
        .with_step(WorkflowStep {
            name: "final_review".to_string(),
            agent_id: "code-reviewer".to_string(),
            operation: "review".to_string(),
            args_template: json!({
                "query": "Provide final code review summary"
            }),
            condition: None,
        })
    }
}

impl Default for CodeReviewOrchestrator {
    fn default() -> Self {
        Self(Self::new())
    }
}
