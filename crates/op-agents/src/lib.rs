//! op-agents: Agent implementations for op-dbus
//!
//! This crate provides agent types and the factory function to create them.
//! Agents are domain-specific AI assistants that can be invoked via D-Bus or MCP.

pub mod agent_catalog;
pub mod agent_registry;
pub mod agent_schema;
pub mod agents;
pub mod dbus_service;
pub mod router;
pub mod security;
pub mod unified;

// Re-export key types
pub use agent_catalog::{builtin_agent_descriptors, AgentDescriptor};
pub use agent_registry::{load_default_specs, AgentRegistry, AgentStatus};
pub use agent_schema::AgentOperationSchema;
pub use agents::base::{AgentContext, AgentTask, AgentTrait, TaskResult};
pub use router::{create_router, AgentsServiceRouter, AgentsState};
pub use unified::{AgentCategory, UnifiedAgentRegistry, GLOBAL_REGISTRY};

/// Create an agent by type name
///
/// Dispatches dynamically through `unified::GLOBAL_REGISTRY`, which lazily builds
/// agents from shared trait components (PersonaAgent, ExecutionAgent, OrchestrationAgent)
/// and wraps them in `UnifiedAgentAdapter` to satisfy `AgentTrait`.
///
/// # Arguments
/// * `agent_type` - The type of agent (e.g., "rust-pro", "memory", "sequential-thinking")
/// * `agent_id` - Unique identifier for this agent instance
///
/// # Returns
/// A boxed agent trait object, or error if type is unknown
pub fn create_agent(
    agent_type: &str,
    agent_id: String,
) -> Result<Box<dyn AgentTrait + Send + Sync>, String> {
    let catalog_type = if agent_type.contains('_') {
        agent_type.replace('_', "-")
    } else {
        agent_type.to_string()
    };

    let lookup = catalog_type.as_str();
    if let Some(unified_agent) = unified::GLOBAL_REGISTRY.get_normalized(lookup) {
        return Ok(Box::new(unified::UnifiedAgentAdapter::new(
            unified_agent,
            lookup.to_string(),
            agent_id,
        )));
    }

    Err(format!("Unknown agent type: {agent_type}"))
}

/// List all available agent types
pub fn list_agent_types() -> Vec<&'static str> {
    let mut ids = unified::GLOBAL_REGISTRY.list_ids();
    ids.sort();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_agent() {
        let agent = create_agent("memory", "test-1".to_string());
        assert!(agent.is_ok());
        let agent = agent.unwrap();
        assert_eq!(agent.agent_type(), "memory");
    }

    #[test]
    fn test_create_agent_underscore_variant() {
        let agent = create_agent("rust_pro", "test-2".to_string());
        assert!(agent.is_ok());
    }

    #[test]
    fn test_unknown_agent() {
        let agent = create_agent("unknown-agent", "test-3".to_string());
        assert!(agent.is_err());
    }

    #[test]
    fn test_list_agent_types() {
        let types = list_agent_types();
        assert!(types.contains(&"memory"));
        assert!(types.contains(&"rust-pro"));
        assert!(types.contains(&"sequential-thinking"));
        assert!(types.len() > 50); // We have many agents
    }

    #[test]
    fn test_create_agent_unified_fallback() {
        let agent = create_agent("python-executor", "test-4".to_string());
        assert!(agent.is_ok());
        assert_eq!(agent.unwrap().agent_type(), "python-executor");
    }
}
