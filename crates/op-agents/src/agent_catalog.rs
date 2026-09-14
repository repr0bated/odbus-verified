//! Agent catalog for tool registration.
//!
//! Builds descriptors from the unified registry via `create_agent`, including
//! per-operation JSON Schemas.

use crate::agent_schema::{infer_category, schemas_for_operations, AgentOperationSchema};
use crate::unified::{self, AgentCategory};
use crate::{create_agent, list_agent_types, AgentTrait};

/// Minimal descriptor for tool registration + schema export.
#[derive(Debug, Clone)]
pub struct AgentDescriptor {
    pub agent_type: String,
    pub name: String,
    pub description: String,
    pub operations: Vec<String>,
    pub category: AgentCategory,
    /// Typed JSON Schema per operation (MCP / plugin contract).
    pub operation_schemas: Vec<AgentOperationSchema>,
}

impl AgentDescriptor {
    pub fn schema_for(&self, operation: &str) -> Option<&AgentOperationSchema> {
        self.operation_schemas.iter().find(|s| s.name == operation)
    }
}

fn describe_agent(agent: &dyn AgentTrait) -> AgentDescriptor {
    let agent_type = agent.agent_type().to_string();
    let operations = agent.operations();
    let category = unified::GLOBAL_REGISTRY
        .get_normalized(&agent_type)
        .map(|a| a.category())
        .unwrap_or_else(|| infer_category(&agent_type, &operations));
    let description = agent.description().to_string();
    let operation_schemas = schemas_for_operations(category, &operations, &description);

    AgentDescriptor {
        agent_type,
        name: agent.name().to_string(),
        description,
        operations,
        category,
        operation_schemas,
    }
}

/// List built-in agents suitable for MCP/tool exposure.
pub fn builtin_agent_descriptors() -> Vec<AgentDescriptor> {
    let mut out = Vec::new();
    // Deduplicate on the *canonical* agent_type() — list ids and unified ids
    // can alias the same concrete agent (e.g. go-pro / golang-pro).
    let mut seen = std::collections::HashSet::new();

    let mut push_created = |agent: Box<dyn AgentTrait + Send + Sync>| {
        let canonical = agent.agent_type().to_string();
        if seen.insert(canonical) {
            out.push(describe_agent(agent.as_ref()));
        }
    };

    for agent_type in list_agent_types() {
        match create_agent(agent_type, "catalog".to_string()) {
            Ok(agent) => push_created(agent),
            Err(err) => tracing::warn!(agent_type, error = %err, "catalog skip"),
        }
    }

    for id in unified::GLOBAL_REGISTRY.list_ids() {
        match create_agent(id, "catalog".to_string()) {
            Ok(agent) => push_created(agent),
            Err(err) => tracing::warn!(agent = id, error = %err, "unified catalog skip"),
        }
    }

    out.sort_by(|a, b| a.agent_type.cmp(&b.agent_type));
    out
}
