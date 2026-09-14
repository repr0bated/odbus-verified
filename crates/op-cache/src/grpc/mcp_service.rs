//! MCP (Model Context Protocol) service implementation
//!
//! Bridges the MCP JSON-RPC protocol to the agent registry and orchestrator.
//! - HandleRequest: Dispatches MCP JSON-RPC requests to agents
//! - ListTools: Exposes registered agents as MCP tools

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tonic::{Request, Response, Status};
use tracing::{debug, info, warn};

use super::agent_service::AgentServiceImpl;
use super::orchestrator_service::OrchestratorServiceImpl;
use super::proto::{
    agent_service_server::AgentService, mcp_service_server::McpService, ListAgentsRequest,
    ListToolsRequest, ListToolsResponse, McpError, McpRequest, McpResponse, McpTool,
};

/// MCP service implementation backed by the agent registry and orchestrator.
pub struct McpServiceImpl {
    agent_service: Arc<AgentServiceImpl>,
    /// Reserved for capability-based MCP routing in future methods.
    #[allow(dead_code)]
    orchestrator_service: Arc<OrchestratorServiceImpl>,
}

impl McpServiceImpl {
    /// Create a new MCP service.
    pub fn new(
        agent_service: Arc<AgentServiceImpl>,
        orchestrator_service: Arc<OrchestratorServiceImpl>,
    ) -> Self {
        Self {
            agent_service,
            orchestrator_service,
        }
    }

    /// Dispatch a `tools/call` request to the appropriate agent.
    async fn handle_tools_call(&self, id: &str, params: &[u8]) -> Result<McpResponse, Status> {
        // Parse the params to extract tool name and arguments.
        // Expected JSON: { "name": "<agent_id>", "arguments": { ... } }
        let mut params_buf = params.to_vec();
        let parsed: Result<ToolCallParams, _> = simd_json::from_slice(&mut params_buf);

        let tool_call = match parsed {
            Ok(tc) => tc,
            Err(e) => {
                return Ok(McpResponse {
                    jsonrpc: "2.0".to_string(),
                    id: id.to_string(),
                    result: Vec::new(),
                    error: Some(McpError {
                        code: -32602, // Invalid params
                        message: format!("Invalid tools/call params: {}", e),
                        data: Vec::new(),
                    }),
                });
            }
        };

        let start = Instant::now();
        debug!("MCP tools/call: agent={}", tool_call.name);

        // Serialize the arguments back to bytes for the agent input
        let input = match simd_json::to_vec(&tool_call.arguments) {
            Ok(bytes) => bytes,
            Err(e) => {
                return Ok(McpResponse {
                    jsonrpc: "2.0".to_string(),
                    id: id.to_string(),
                    result: Vec::new(),
                    error: Some(McpError {
                        code: -32603, // Internal error
                        message: format!("Failed to serialize arguments: {}", e),
                        data: Vec::new(),
                    }),
                });
            }
        };

        // Execute through the agent service
        let exec_req = Request::new(super::proto::ExecuteAgentRequest {
            agent_id: tool_call.name.clone(),
            input,
            context: HashMap::new(),
            timeout_ms: 0,
        });

        let exec_response = self.agent_service.execute(exec_req).await?;
        let result = exec_response.into_inner();
        let latency_ms = start.elapsed().as_millis() as u64;

        if result.success {
            // Wrap agent output in MCP content format:
            // { "content": [{ "type": "text", "text": "<output>" }] }
            let content_response = McpContentResponse {
                content: vec![McpContent {
                    r#type: "text".to_string(),
                    text: String::from_utf8_lossy(&result.output).to_string(),
                }],
            };

            let response_bytes = simd_json::to_vec(&content_response).unwrap_or_default();

            debug!(
                "MCP tools/call completed: agent={} latency_ms={}",
                tool_call.name, latency_ms
            );

            Ok(McpResponse {
                jsonrpc: "2.0".to_string(),
                id: id.to_string(),
                result: response_bytes,
                error: None,
            })
        } else {
            warn!(
                "MCP tools/call failed: agent={} error={}",
                tool_call.name, result.error
            );

            Ok(McpResponse {
                jsonrpc: "2.0".to_string(),
                id: id.to_string(),
                result: Vec::new(),
                error: Some(McpError {
                    code: -32603, // Internal error
                    message: result.error,
                    data: Vec::new(),
                }),
            })
        }
    }

    /// Handle a `tools/list` request by delegating to ListTools.
    async fn handle_tools_list(&self, id: &str) -> Result<McpResponse, Status> {
        let tools_response = self.list_tools_internal().await?;

        let response_bytes = simd_json::to_vec(&McpToolsListResult {
            tools: tools_response
                .tools
                .iter()
                .map(|t| McpToolJson {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    input_schema: {
                        let mut buf = t.input_schema.clone();
                        if buf.is_empty() {
                            serde_json::Value::Object(serde_json::Map::new())
                        } else {
                            simd_json::from_slice(&mut buf)
                                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
                        }
                    },
                })
                .collect(),
        })
        .unwrap_or_default();

        Ok(McpResponse {
            jsonrpc: "2.0".to_string(),
            id: id.to_string(),
            result: response_bytes,
            error: None,
        })
    }

    /// Handle an `initialize` request.
    fn handle_initialize(&self, id: &str) -> McpResponse {
        let init_result = McpInitializeResult {
            protocol_version: "2024-11-05".to_string(),
            capabilities: McpServerCapabilities {
                tools: Some(McpToolCapability { list_changed: true }),
            },
            server_info: McpServerInfo {
                name: "op-cache".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };

        let result_bytes = simd_json::to_vec(&init_result).unwrap_or_default();

        McpResponse {
            jsonrpc: "2.0".to_string(),
            id: id.to_string(),
            result: result_bytes,
            error: None,
        }
    }

    /// Handle a `ping` request.
    fn handle_ping(&self, id: &str) -> McpResponse {
        McpResponse {
            jsonrpc: "2.0".to_string(),
            id: id.to_string(),
            result: b"{}".to_vec(),
            error: None,
        }
    }

    /// Build the list of tools from the agent registry.
    async fn list_tools_internal(&self) -> Result<ListToolsResponse, Status> {
        let agents_response = self
            .agent_service
            .list_agents(Request::new(ListAgentsRequest { enabled_only: true }))
            .await?
            .into_inner();

        let tools: Vec<McpTool> = agents_response
            .agents
            .into_iter()
            .map(|agent| {
                // Build a JSON Schema describing the agent's input.
                // Each agent accepts arbitrary JSON via the "input" field.
                let input_schema = build_agent_input_schema(&agent.name, &agent.description);

                McpTool {
                    name: agent.id,
                    description: if agent.description.is_empty() {
                        agent.name
                    } else {
                        agent.description
                    },
                    input_schema,
                }
            })
            .collect();

        Ok(ListToolsResponse { tools })
    }
}

/// Build a minimal JSON Schema for an agent's input.
fn build_agent_input_schema(_name: &str, _description: &str) -> Vec<u8> {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "input": {
                "type": "string",
                "description": "Input data for the agent"
            }
        }
    });

    simd_json::to_vec(&schema).unwrap_or_default()
}

// --- Internal serde types for MCP JSON-RPC ---

#[derive(serde::Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: serde_json::Value,
}

#[derive(serde::Serialize)]
struct McpContentResponse {
    content: Vec<McpContent>,
}

#[derive(serde::Serialize)]
struct McpContent {
    r#type: String,
    text: String,
}

#[derive(serde::Serialize)]
struct McpToolsListResult {
    tools: Vec<McpToolJson>,
}

#[derive(serde::Serialize)]
struct McpToolJson {
    name: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: serde_json::Value,
}

#[derive(serde::Serialize)]
struct McpInitializeResult {
    #[serde(rename = "protocolVersion")]
    protocol_version: String,
    capabilities: McpServerCapabilities,
    #[serde(rename = "serverInfo")]
    server_info: McpServerInfo,
}

#[derive(serde::Serialize)]
struct McpServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<McpToolCapability>,
}

#[derive(serde::Serialize)]
struct McpToolCapability {
    #[serde(rename = "listChanged")]
    list_changed: bool,
}

#[derive(serde::Serialize)]
struct McpServerInfo {
    name: String,
    version: String,
}

#[tonic::async_trait]
impl McpService for McpServiceImpl {
    async fn handle_request(
        &self,
        request: Request<McpRequest>,
    ) -> Result<Response<McpResponse>, Status> {
        let req = request.into_inner();

        // Validate JSON-RPC version
        if !req.jsonrpc.is_empty() && req.jsonrpc != "2.0" {
            return Ok(Response::new(McpResponse {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Vec::new(),
                error: Some(McpError {
                    code: -32600, // Invalid Request
                    message: format!("Unsupported JSON-RPC version: {}", req.jsonrpc),
                    data: Vec::new(),
                }),
            }));
        }

        info!("MCP request: method={} id={}", req.method, req.id);

        let response = match req.method.as_str() {
            "initialize" => self.handle_initialize(&req.id),
            "ping" => self.handle_ping(&req.id),
            "tools/list" => self.handle_tools_list(&req.id).await?,
            "tools/call" => self.handle_tools_call(&req.id, &req.params).await?,
            _ => {
                warn!("MCP unknown method: {}", req.method);
                McpResponse {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: Vec::new(),
                    error: Some(McpError {
                        code: -32601, // Method not found
                        message: format!("Method not found: {}", req.method),
                        data: Vec::new(),
                    }),
                }
            }
        };

        Ok(Response::new(response))
    }

    async fn list_tools(
        &self,
        _request: Request<ListToolsRequest>,
    ) -> Result<Response<ListToolsResponse>, Status> {
        let response = self.list_tools_internal().await?;

        info!("MCP list_tools: returning {} tools", response.tools.len());

        Ok(Response::new(response))
    }
}
