//! op-workflows: Workflow engine with plugin/service nodes
//!
//! Features:
//! - PocketFlow-style flow-based programming
//! - Plugins and services as workflow nodes
//! - State transitions and event-driven execution
//! - Parallel and sequential execution modes

pub mod builtin;
pub mod context;
pub mod engine;
pub mod flow;
pub mod history;
pub mod node;
pub mod orchestrator;
pub mod workflows;

pub use orchestrator::{
    Orchestrator, OrchestratorConfig, OrchestratorStats, StepResult, WorkflowResult,
};

pub use context::WorkflowContext;
pub use engine::WorkflowEngine;
pub use flow::{Workflow, WorkflowDefinition, WorkflowState};
pub use node::{NodeConnection, NodePort, NodeResult, NodeState, WorkflowNode};

/// Prelude for convenient imports
pub mod prelude {
    pub use super::context::WorkflowContext;
    pub use super::engine::WorkflowEngine;
    pub use super::flow::{Workflow, WorkflowDefinition, WorkflowState};
    pub use super::node::{NodeConnection, NodePort, NodeResult, NodeState, WorkflowNode};
}
