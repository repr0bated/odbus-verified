//! Agents MCP Server - D-Bus First Architecture
//!
//! Discovers agents via D-Bus introspection and exposes them as MCP tools.
//! This is the proper architecture for Project D-Bus.
//!
//! ## How It Works
//!
//! 1. Agent Manager starts agents as D-Bus services
//!    - org.dbusmcp.Agent.RustPro
//!    - org.dbusmcp.Agent.PythonPro
//!    - etc.
//!
//! 2. This server uses introspection to discover running agents
//!    - Lists services matching org.dbusmcp.Agent.*
//!    - Introspects each to get methods/properties
//!
//! 3. Exposes discovered agents as MCP tools
//!    - rust_pro_check, rust_pro_build, etc.
//!
//! 4. Tool calls are proxied to D-Bus
//!    - MCP tool call -> D-Bus method call -> Agent execution

use crate::protocol::{JsonRpcError, McpRequest, McpResponse};
use anyhow::{Context, Result};
use op_core::BusType;
use op_introspection::ServiceScanner;
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use zbus::Connection;

/// Agent discovered via D-Bus introspection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredAgent {
    pub id: String,
    pub name: String,
    pub description: String,
    pub agent_type: String,
    pub service_name: String,
    pub object_path: String,
    pub operations: Vec<String>,
    pub available: bool,
}

/// MCP tool derived from a D-Bus agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTool {
    pub name: String,
    pub description: String,
    pub agent_id: String,
    pub operation: String,
    pub input_schema: Value,
}

/// Agents MCP Server - discovers and exposes D-Bus agents
pub struct AgentsServer {
    scanner: ServiceScanner,
    connection: Arc<RwLock<Option<Connection>>>,
    discovered_agents: Arc<RwLock<HashMap<String, DiscoveredAgent>>>,
    tools: Arc<RwLock<Vec<AgentTool>>>,
    bus_type: BusType,
}

impl AgentsServer {
    /// Create a new agents server
    pub fn new(bus_type: BusType) -> Self {
        Self {
            scanner: ServiceScanner::new(),
            connection: Arc::new(RwLock::new(None)),
            discovered_agents: Arc::new(RwLock::new(HashMap::new())),
            tools: Arc::new(RwLock::new(Vec::new())),
            bus_type,
        }
    }

    /// Initialize - connect to D-Bus and discover agents
    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing Agents MCP Server (D-Bus first)");

        // Connect to D-Bus
        let conn = match self.bus_type {
            BusType::System => Connection::system().await?,
            BusType::Session => Connection::session().await?,
        };

        {
            let mut connection = self.connection.write().await;
            *connection = Some(conn);
        }

        // Discover agents
        self.discover_agents().await?;

        Ok(())
    }

    /// Discover agents via D-Bus introspection
    pub async fn discover_agents(&self) -> Result<()> {
        info!("Discovering D-Bus agents...");

        // List all services on the bus
        let services = self.scanner.list_services(self.bus_type).await?;

        // Filter for agent services
        let agent_services: Vec<_> = services
            .iter()
            .filter(|s| s.name.starts_with("org.dbusmcp.Agent."))
            .collect();

        info!("Found {} agent services on D-Bus", agent_services.len());

        let mut discovered = self.discovered_agents.write().await;
        let mut tools = self.tools.write().await;

        discovered.clear();
        tools.clear();

        // Introspect each agent service
        for service in agent_services {
            match self.introspect_agent(&service.name).await {
                Ok(agent) => {
                    info!("  ✓ {} ({} operations)", agent.name, agent.operations.len());

                    // Create tools for each operation
                    for op in &agent.operations {
                        let tool = AgentTool {
                            name: format!("{}_{}", agent.id, op),
                            description: format!(
                                "[{}] {} - {} operation",
                                agent.name, agent.description, op
                            ),
                            agent_id: agent.id.clone(),
                            operation: op.clone(),
                            input_schema: self.get_operation_schema(&agent.agent_type, op),
                        };
                        tools.push(tool);
                    }

                    discovered.insert(agent.id.clone(), agent);
                }
                Err(e) => {
                    warn!("  ✗ Failed to introspect {}: {}", service.name, e);
                }
            }
        }

        info!(
            "Discovered {} agents with {} total tools",
            discovered.len(),
            tools.len()
        );

        Ok(())
    }

    /// Introspect a single agent service
    async fn introspect_agent(&self, service_name: &str) -> Result<DiscoveredAgent> {
        let connection = self.connection.read().await;
        let conn = connection
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not connected to D-Bus"))?;

        // Extract agent type from service name
        // org.dbusmcp.Agent.RustPro -> rust_pro
        let agent_type_pascal = service_name
            .strip_prefix("org.dbusmcp.Agent.")
            .ok_or_else(|| anyhow::anyhow!("Invalid agent service name"))?;
        let agent_type = pascal_to_snake(agent_type_pascal);
        let agent_id = agent_type.clone();

        // Object path
        let object_path = format!("/org/dbusmcp/Agent/{}", agent_type_pascal);

        // Create proxy to call introspection methods
        let proxy =
            zbus::Proxy::new(conn, service_name, object_path.clone(), "org.dbusmcp.Agent").await?;

        // Get agent metadata
        let name: String = proxy
            .call("name", &())
            .await
            .unwrap_or_else(|_| agent_type_pascal.to_string());

        let description: String = proxy
            .call("description", &())
            .await
            .unwrap_or_else(|_| "D-Bus agent".to_string());

        let operations: Vec<String> = proxy
            .call("operations", &())
            .await
            .unwrap_or_else(|_| vec!["execute".to_string()]);

        Ok(DiscoveredAgent {
            id: agent_id,
            name,
            description,
            agent_type,
            service_name: service_name.to_string(),
            object_path,
            operations,
            available: true,
        })
    }

    /// Get input schema for an operation
    fn get_operation_schema(&self, _agent_type: &str, _operation: &str) -> Value {
        // Default schema - agents can override via D-Bus properties
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to operate on"
                },
                "args": {
                    "type": "string",
                    "description": "Additional arguments"
                }
            }
        })
    }

    /// Execute a tool by calling the D-Bus agent
    pub async fn execute_tool(&self, tool_name: &str, arguments: Value) -> Result<Value> {
        debug!("Executing tool: {} with args: {:?}", tool_name, arguments);

        // Find the tool
        let tools = self.tools.read().await;
        let tool = tools
            .iter()
            .find(|t| t.name == tool_name)
            .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", tool_name))?;

        // Find the agent
        let agents = self.discovered_agents.read().await;
        let agent = agents
            .get(&tool.agent_id)
            .ok_or_else(|| anyhow::anyhow!("Agent not found: {}", tool.agent_id))?;

        if !agent.available {
            return Err(anyhow::anyhow!("Agent {} is not available", agent.id));
        }

        // Get D-Bus connection
        let connection = self.connection.read().await;
        let conn = connection
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not connected to D-Bus"))?;

        // Create proxy
        let proxy = zbus::Proxy::new(
            conn,
            &*agent.service_name,
            &*agent.object_path,
            "org.dbusmcp.Agent",
        )
        .await?;

        // Build task JSON
        let task = json!({
            "task_type": agent.agent_type,
            "operation": tool.operation,
            "path": arguments.get("path").and_then(|v| v.as_str()),
            "args": arguments.get("args").and_then(|v| v.as_str()),
            "config": arguments.get("config").cloned().unwrap_or(json!({}))
        });

        let task_json = simd_json::to_string(&task)?;

        // Call D-Bus method
        let result: String = proxy
            .call("Execute", &(task_json,))
            .await
            .context("D-Bus Execute call failed")?;

        // Execute operation
        let mut result_mut = result.clone();
        let result_value: Value = unsafe { simd_json::from_str(&mut result_mut) }
            .unwrap_or(json!({ "raw_output": result }));

        Ok(result_value)
    }

    /// Get list of available agents
    pub async fn list_agents(&self) -> Vec<DiscoveredAgent> {
        let agents = self.discovered_agents.read().await;
        agents.values().cloned().collect()
    }

    /// Get list of available tools (for MCP tools/list)
    pub async fn list_tools(&self) -> Vec<Value> {
        let tools = self.tools.read().await;
        tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.input_schema
                })
            })
            .collect()
    }

    /// Refresh agent discovery
    pub async fn refresh(&self) -> Result<()> {
        info!("Refreshing agent discovery...");
        self.discover_agents().await
    }

    /// Handle an MCP request
    pub async fn handle_request(&self, request: McpRequest) -> McpResponse {
        match request.method.as_str() {
            "initialize" => McpResponse::success(
                request.id,
                json!({
                    "protocolVersion": crate::PROTOCOL_VERSION,
                    "serverInfo": {
                        "name": "op-mcp-agents",
                        "version": crate::SERVER_VERSION
                    },
                    "capabilities": {
                        "tools": { "listChanged": true }
                    }
                }),
            ),
            "tools/list" => {
                let tools = self.list_tools().await;
                McpResponse::success(request.id, json!({ "tools": tools }))
            }
            "tools/call" => {
                let tool_name = request
                    .params
                    .as_object()
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("");
                let arguments = request
                    .params
                    .as_object()
                    .and_then(|p| p.get("arguments"))
                    .cloned()
                    .unwrap_or(json!({}));
                match self.execute_tool(tool_name, arguments).await {
                    Ok(result) => {
                        let text = simd_json::to_string_pretty(&result).unwrap_or_default();
                        McpResponse::success(
                            request.id,
                            json!({
                                "content": [{
                                    "type": "text",
                                    "text": text
                                }],
                                "isError": false
                            }),
                        )
                    }
                    Err(e) => McpResponse::success(
                        request.id,
                        json!({
                            "content": [{
                                "type": "text",
                                "text": format!("Error executing agent tool {tool_name}: {e}")
                            }],
                            "isError": true
                        }),
                    ),
                }
            }
            _ => McpResponse::error(
                request.id,
                JsonRpcError::new(-32601, format!("Method not found: {}", request.method)),
            ),
        }
    }
}

/// Convert PascalCase to snake_case
fn pascal_to_snake(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pascal_to_snake() {
        assert_eq!(pascal_to_snake("RustPro"), "rust_pro");
        assert_eq!(pascal_to_snake("PythonPro"), "python_pro");
        assert_eq!(pascal_to_snake("SequentialThinking"), "sequential_thinking");
        assert_eq!(pascal_to_snake("BackendArchitect"), "backend_architect");
    }
}
