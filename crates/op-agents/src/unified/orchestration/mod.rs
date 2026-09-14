//! Orchestration Agents
//!
//! Meta-agents that coordinate other agents for complex workflows.

mod base;
mod code_review_orchestrator;
mod tdd_orchestrator;

pub use base::OrchestrationAgent;
pub use code_review_orchestrator::CodeReviewOrchestrator;
pub use tdd_orchestrator::TddOrchestrator;

use std::collections::HashMap;
use std::sync::LazyLock;

/// All available orchestration agents
#[allow(clippy::type_complexity)]
pub static ORCHESTRATION_AGENTS: LazyLock<
    HashMap<&'static str, fn() -> Box<dyn super::UnifiedAgent>>,
> = LazyLock::new(|| {
    let mut m: HashMap<&'static str, fn() -> Box<dyn super::UnifiedAgent>> = HashMap::new();
    m.insert("tdd-orchestrator", || Box::new(TddOrchestrator::new()));
    m.insert("code-review-orchestrator", || {
        Box::new(CodeReviewOrchestrator::new())
    });
    m
});
