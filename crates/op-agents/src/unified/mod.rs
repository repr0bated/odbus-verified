//! Unified Agent Architecture
//!
//! This module implements the recommended architecture that:
//! 1. Merges markdown prompts INTO Rust agents (single source of truth)
//! 2. Clearly separates EXECUTION agents from PERSONA agents
//! 3. Uses consistent naming: {type}-executor vs {type}-expert
//!
//! ## Agent Categories
//!
//! - **Execution Agents**: Can run code/commands with sandboxing
//! - **Persona Agents**: LLM-only, provide expertise without code execution
//! - **Orchestration Agents**: Coordinate other agents for complex workflows

pub mod adapter;
pub mod agent_trait;
pub mod execution;
pub mod legacy_catalog;
pub mod orchestration;
pub mod persona;
pub mod prompts;
pub mod registry;

pub use adapter::UnifiedAgentAdapter;
pub use agent_trait::{AgentCapability, AgentCategory, UnifiedAgent};
pub use execution::ExecutionAgent;
pub use orchestration::OrchestrationAgent;
pub use persona::PersonaAgent;
pub use registry::{UnifiedAgentRegistry, GLOBAL_REGISTRY};
