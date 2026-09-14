//! MCP tool registry adapter for built-in agents.
//!
//! The agent catalog is the local schema source for exposing agents as tools.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use op_agents::{builtin_agent_descriptors, create_agent, AgentDescriptor, AgentTask};
use op_mcp::tool_registry::{BoxedTool, Tool, ToolRegistry};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
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
        json!({
            "type": "object",
            "properties": {
                "args": {
                    "type": "string",
                    "description": "Natural-language task or operation-specific arguments"
                },
                "path": {
                    "type": "string",
                    "description": "Optional working path"
                },
                "config": {
                    "type": "object",
                    "description": "Optional agent-specific configuration"
                }
            }
        })
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
        if let Some(args) = input.get("args").and_then(|v| v.as_str()) {
            task = task.with_args(args);
        }
        if let Some(config) = input.get("config").and_then(|v| v.as_object()) {
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
