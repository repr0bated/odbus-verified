//! Agent type aliases for op-cache.
//!
//! Keeps public API aligned with gRPC naming.

pub use crate::agent_registry::{
    AgentCapability as Capability, AgentDefinition as Agent, AgentPriority as Priority,
    AgentRegistry,
};
