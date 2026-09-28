//! MCP tool registry adapter for built-in agents.
//!
//! Contracts come from the cognitive plugin. Authenticated persona and workflow
//! execution lives in the bridge; this registry is its implementation inventory.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use op_agents::{AgentDescriptor, AgentTask, builtin_agent_descriptors, create_agent};
use op_mcp::tool_registry::{BoxedTool, Tool, ToolRegistry};
use op_plugins::state_plugins::cognitive_agents;
use simd_json::prelude::*;
use simd_json::{OwnedValue as Value, json};
use std::collections::HashMap;
use std::sync::Arc;

pub async fn register_agent_tools(registry: &ToolRegistry) -> Result<usize> {
    let mut count = 0;

    for descriptor in builtin_agent_descriptors() {
        for operation in &descriptor.operations {
            registry
                .register(Arc::new(AgentCatalogTool {
                    descriptor: descriptor.clone(),
                    operation: operation.clone(),
                    tool_name: format!(
                        "agent_{}_{}",
                        sanitize_tool_name(&descriptor.agent_type),
                        sanitize_tool_name(operation)
                    ),
                }) as BoxedTool)
                .await?;
            count += 1;
        }
    }

    Ok(count)
}

struct AgentCatalogTool {
    descriptor: AgentDescriptor,
    operation: String,
    tool_name: String,
}

#[async_trait]
impl Tool for AgentCatalogTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.descriptor.description
    }

    fn category(&self) -> &str {
        "agent"
    }

    fn namespace(&self) -> &str {
        "agents"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "agent".to_string(),
            self.descriptor.agent_type.clone(),
            self.operation.clone(),
        ]
    }

    fn input_schema(&self) -> Value {
        cognitive_agents::method_by_name(&self.tool_name)
            .expect("registered agent must have a plugin contract")
            .declaration
            .args
            .clone()
    }

    fn required_capability(&self) -> Option<&str> {
        cognitive_agents::method_by_name(&self.tool_name)?
            .declaration
            .required_capability
            .as_deref()
    }

    fn subid(&self) -> Option<&str> {
        Some(
            &cognitive_agents::method_by_name(&self.tool_name)?
                .declaration
                .subid,
        )
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let agent_id = format!(
            "tool-registry-{}",
            sanitize_tool_name(&self.descriptor.agent_type)
        );
        let agent =
            create_agent(&self.descriptor.agent_type, agent_id).map_err(|err| anyhow!(err))?;

        let mut task = AgentTask::new(&self.descriptor.agent_type, &self.operation);
        if let Some(path) = input.get("path").and_then(|v| v.as_str()) {
            task = task.with_path(path);
        }
        if let Some(config) = input.as_object() {
            task.config = config
                .iter()
                .map(|(key, value)| (key.to_string(), value.clone()))
                .collect::<HashMap<_, _>>();
        }

        let result = agent.execute(task).await.map_err(|err| anyhow!(err))?;
        Ok(json!({
            "success": result.success,
            "agent_type": self.descriptor.agent_type,
            "agent_name": self.descriptor.name,
            "operation": result.operation,
            "data": result.data,
            "metadata": result.metadata,
        }))
    }
}

fn sanitize_tool_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}
