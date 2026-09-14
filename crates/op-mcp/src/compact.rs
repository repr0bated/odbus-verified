//! Compact Mode
//!
//! Provides 5 meta-tools for discovering and executing system tools:
//! - list_tools: Browse available tools with filtering
//! - search_tools: Search tools by keyword
//! - get_tool_schema: Get input schema for a specific tool
//! - execute_tool: Execute any tool by name
//! - respond: Send the final user response
//!
//! This mode saves ~95% of context tokens compared to exposing all tools.

use crate::{JsonRpcError, McpRequest, McpResponse, ToolExecutor, ToolInfo};
use anyhow::Result;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

pub struct PrewarmedOpToolsExecutor {
    registry: Arc<op_tools::ToolRegistry>,
}

impl PrewarmedOpToolsExecutor {
    pub async fn new() -> Result<Self> {
        let registry = Arc::new(op_tools::ToolRegistry::new());
        op_tools::register_builtin_tools(&registry).await?;
        Ok(Self { registry })
    }
}

pub type LazyOpToolsExecutor = PrewarmedOpToolsExecutor;

#[async_trait::async_trait]
impl ToolExecutor for PrewarmedOpToolsExecutor {
    async fn list_tools(&self) -> Result<Vec<ToolInfo>> {
        Ok(self
            .registry
            .list()
            .await
            .into_iter()
            .map(|tool| ToolInfo {
                name: tool.name,
                description: tool.description,
                input_schema: tool.input_schema,
                annotations: None,
            })
            .collect())
    }

    async fn execute_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        let tool = self
            .registry
            .get(name)
            .await
            .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", name))?;
        tool.execute(arguments).await
    }

    async fn get_tool_schema(&self, name: &str) -> Result<Option<Value>> {
        Ok(self
            .registry
            .get_definition(name)
            .await
            .map(|d| d.input_schema))
    }

    async fn search_tools(&self, query: &str, limit: usize) -> Result<Vec<ToolInfo>> {
        let needle = query.to_lowercase();
        Ok(self
            .list_tools()
            .await?
            .into_iter()
            .filter(|tool| {
                tool.name.to_lowercase().contains(&needle)
                    || tool.description.to_lowercase().contains(&needle)
            })
            .take(limit)
            .collect())
    }
}

/// Session context passed through from gateway
#[derive(Debug, Clone, Default)]
pub struct SessionContext {
    pub session_id: Option<String>,
    pub is_controller: bool,
    pub peer_pubkey: Option<String>,
}

/// Compact server wraps a tool executor and exposes 5 meta-tools
pub struct CompactServer {
    executor: Arc<dyn ToolExecutor>,
    server_name: String,
    session: RwLock<SessionContext>,
}

impl CompactServer {
    pub fn new(executor: Arc<dyn ToolExecutor>) -> Self {
        Self {
            executor,
            server_name: "op-mcp-compact".to_string(),
            session: RwLock::new(SessionContext::default()),
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.server_name = name.into();
        self
    }

    /// Set session context (called by gateway after auth)
    pub async fn set_session(&self, ctx: SessionContext) {
        info!(
            session_id = ?ctx.session_id,
            is_controller = %ctx.is_controller,
            "Session context set"
        );
        *self.session.write().await = ctx;
    }

    /// Check if current session can execute controller-only tools
    pub async fn can_execute_controller_tools(&self) -> bool {
        self.session.read().await.is_controller
    }

    /// Handle MCP request
    pub async fn handle_request(&self, request: McpRequest) -> McpResponse {
        debug!(method = %request.method, "Handling compact MCP request");

        match request.method.as_str() {
            "initialize" => self.handle_initialize(request).await,
            "initialized" => McpResponse::success(request.id, json!({})),
            "ping" => McpResponse::success(request.id, json!({})),
            "tools/list" => self.handle_tools_list(request).await,
            "tools/call" => self.handle_tools_call(request).await,
            "notifications/initialized" => McpResponse::success(request.id, json!({})),
            _ => McpResponse::error(request.id, JsonRpcError::method_not_found(&request.method)),
        }
    }

    async fn handle_initialize(&self, request: McpRequest) -> McpResponse {
        info!("Compact MCP initialized");

        McpResponse::success(
            request.id,
            json!({
                "protocolVersion": crate::PROTOCOL_VERSION,
                "capabilities": {
                    "tools": { "listChanged": false }
                },
                "serverInfo": {
                    "name": self.server_name,
                    "version": crate::SERVER_VERSION
                },
                "instructions": "This server uses compact mode with 5 meta-tools. Use list_tools to discover available tools, get_tool_schema to get the input schema, execute_tool to run tools, and respond for the final answer."
            }),
        )
    }

    async fn handle_tools_list(&self, request: McpRequest) -> McpResponse {
        McpResponse::success(
            request.id,
            json!({
                "tools": compact_tools_schema(),
                "_meta": { "compactMode": true }
            }),
        )
    }

    async fn handle_tools_call(&self, request: McpRequest) -> McpResponse {
        let params = match &request.params {
            Some(p) => p,
            None => {
                return McpResponse::error(
                    request.id,
                    JsonRpcError::invalid_params("Missing params"),
                )
            }
        };

        let tool_name = match params
            .as_object()
            .and_then(|o| o.get("name"))
            .and_then(|n| n.as_str())
        {
            Some(n) => n,
            None => {
                return McpResponse::error(
                    request.id,
                    JsonRpcError::invalid_params("Missing tool name"),
                )
            }
        };

        let arguments = params
            .as_object()
            .and_then(|o| o.get("arguments"))
            .cloned()
            .unwrap_or(json!({}));

        // Route to meta-tool handlers
        match tool_name {
            "list_tools" => self.meta_list_tools(request.id, arguments).await,
            "search_tools" => self.meta_search_tools(request.id, arguments).await,
            "get_tool_schema" => self.meta_get_tool_schema(request.id, arguments).await,
            "execute_tool" => self.meta_execute_tool(request.id, arguments).await,
            "respond" => self.meta_respond(request.id, arguments).await,
            _ => McpResponse::error(
                request.id,
                JsonRpcError::new(-32001, format!(
                    "Unknown meta-tool: {}. Use list_tools, search_tools, get_tool_schema, execute_tool, or respond.",
                    tool_name
                )),
            ),
        }
    }

    async fn meta_list_tools(&self, id: Option<Value>, args: Value) -> McpResponse {
        let category = args
            .as_object()
            .and_then(|o| o.get("category"))
            .and_then(|c| c.as_str());
        let limit = args
            .as_object()
            .and_then(|o| o.get("limit"))
            .and_then(|l| l.as_u64())
            .unwrap_or(20) as usize;
        let offset = args
            .as_object()
            .and_then(|o| o.get("offset"))
            .and_then(|o| o.as_u64())
            .unwrap_or(0) as usize;

        match self.executor.list_tools().await {
            Ok(tools) => {
                let filtered: Vec<_> = tools
                    .into_iter()
                    .filter(|t| {
                        category
                            .map(|c| {
                                t.name.contains(c)
                                    || t.description.to_lowercase().contains(&c.to_lowercase())
                            })
                            .unwrap_or(true)
                    })
                    .skip(offset)
                    .take(limit)
                    .map(|t| {
                        json!({
                            "name": t.name,
                            "description": t.description
                        })
                    })
                    .collect();

                let total = filtered.len();

                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": simd_json::to_string_pretty(&json!({
                                "tools": filtered,
                                "count": total,
                                "offset": offset,
                                "limit": limit
                            })).unwrap()
                        }],
                        "isError": false
                    }),
                )
            }
            Err(e) => {
                error!(error = %e, "Failed to list tools");
                McpResponse::success(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": format!("Error: {}", e) }],
                        "isError": true
                    }),
                )
            }
        }
    }

    async fn meta_search_tools(&self, id: Option<Value>, args: Value) -> McpResponse {
        let query = args
            .as_object()
            .and_then(|o| o.get("query"))
            .and_then(|q| q.as_str())
            .unwrap_or("");
        let limit = args
            .as_object()
            .and_then(|o| o.get("limit"))
            .and_then(|l| l.as_u64())
            .unwrap_or(10) as usize;

        match self.executor.search_tools(query, limit).await {
            Ok(tools) => {
                let results: Vec<_> = tools
                    .into_iter()
                    .map(|t| {
                        json!({
                            "name": t.name,
                            "description": t.description
                        })
                    })
                    .collect();

                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": simd_json::to_string_pretty(&json!({
                                "query": query,
                                "results": results,
                                "count": results.len()
                            })).unwrap()
                        }],
                        "isError": false
                    }),
                )
            }
            Err(e) => {
                error!(error = %e, "Failed to search tools");
                McpResponse::success(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": format!("Error: {}", e) }],
                        "isError": true
                    }),
                )
            }
        }
    }

    async fn meta_get_tool_schema(&self, id: Option<Value>, args: Value) -> McpResponse {
        let tool_name = args
            .as_object()
            .and_then(|o| o.get("tool_name"))
            .and_then(|n| n.as_str())
            .unwrap_or("");

        if tool_name.is_empty() {
            return McpResponse::success(
                id,
                json!({
                    "content": [{ "type": "text", "text": "Error: tool_name is required" }],
                    "isError": true
                }),
            );
        }

        match self.executor.get_tool_schema(tool_name).await {
            Ok(Some(schema)) => McpResponse::success(
                id,
                json!({
                    "content": [{
                        "type": "text",
                        "text": simd_json::to_string_pretty(&json!({
                            "tool": tool_name,
                            "schema": schema
                        })).unwrap()
                    }],
                    "isError": false
                }),
            ),
            Ok(None) => McpResponse::success(
                id,
                json!({
                    "content": [{ "type": "text", "text": format!("Tool not found: {}", tool_name) }],
                    "isError": true
                }),
            ),
            Err(e) => {
                error!(error = %e, "Failed to get tool schema");
                McpResponse::success(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": format!("Error: {}", e) }],
                        "isError": true
                    }),
                )
            }
        }
    }

    async fn meta_execute_tool(&self, id: Option<Value>, args: Value) -> McpResponse {
        let tool_name = args
            .as_object()
            .and_then(|o| o.get("tool_name"))
            .and_then(|n| n.as_str())
            .unwrap_or("");
        let arguments = args
            .as_object()
            .and_then(|o| o.get("arguments"))
            .cloned()
            .unwrap_or(json!({}));

        if tool_name.is_empty() {
            return McpResponse::success(
                id,
                json!({
                    "content": [{ "type": "text", "text": "Error: tool_name is required" }],
                    "isError": true
                }),
            );
        }

        info!(tool = %tool_name, "Executing tool via compact mode");

        match self.executor.execute_tool(tool_name, arguments).await {
            Ok(result) => {
                let text = simd_json::to_string_pretty(&result).unwrap_or_default();
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": text
                        }],
                        "isError": false
                    }),
                )
            }
            Err(e) => {
                error!(tool = %tool_name, error = %e, "Tool execution failed");
                McpResponse::success(
                    id,
                    json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Error executing {}: {}", tool_name, e)
                        }],
                        "isError": true
                    }),
                )
            }
        }
    }

    async fn meta_respond(&self, id: Option<Value>, args: Value) -> McpResponse {
        let message = args
            .as_object()
            .and_then(|o| o.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        McpResponse::success(
            id,
            json!({
                "content": [{
                    "type": "text",
                    "text": message
                }],
                "isError": false
            }),
        )
    }
}

/// Get the 5 compact meta-tool schemas
pub fn compact_tools_schema() -> Vec<Value> {
    vec![
        json!({
            "name": "list_tools",
            "description": "List available tools. Filter by category. Returns tool names and descriptions.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "category": {
                        "type": "string",
                        "description": "Filter by category (e.g., 'ovs', 'dbus', 'file', 'agent')"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum tools to return",
                        "default": 20
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Offset for pagination",
                        "default": 0
                    }
                }
            }
        }),
        json!({
            "name": "search_tools",
            "description": "Search tools by keyword in name or description.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum results",
                        "default": 10
                    }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "get_tool_schema",
            "description": "Get the input schema for a specific tool. Call this before execute_tool to know the required arguments.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tool_name": {
                        "type": "string",
                        "description": "Name of the tool"
                    }
                },
                "required": ["tool_name"]
            }
        }),
        json!({
            "name": "execute_tool",
            "description": "Execute any tool by name with arguments. First use get_tool_schema to see required arguments.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tool_name": {
                        "type": "string",
                        "description": "Name of the tool to execute"
                    },
                    "arguments": {
                        "type": "object",
                        "description": "Arguments to pass to the tool"
                    }
                },
                "required": ["tool_name"]
            }
        }),
        json!({
            "name": "respond",
            "description": "Send the final response to the user.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Response message"
                    }
                },
                "required": ["message"]
            }
        }),
    ]
}

/// Run compact server in stdio mode
pub async fn run_compact_stdio_server() -> Result<()> {
    use crate::transport::{StdioTransport, Transport};

    // Initialize logging to stderr
    let subscriber = tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .with_writer(std::io::stderr)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let peer_pubkey = if let Ok(pubkey) = std::env::var("WG_PUBKEY") {
        info!(pubkey = %pubkey, "WG_PUBKEY configured; using injected WireGuard identity");
        Some(pubkey)
    } else if let Ok(wg_iface) =
        std::env::var("WG_INTERFACE").or_else(|_| std::env::var("FABRIC_WG_INTERFACE"))
    {
        tracing::warn!(
            interface = %wg_iface,
            "WG_INTERFACE/FABRIC_WG_INTERFACE identity path is deprecated; prefer explicit pubkey injection"
        );
        let wg_id = op_identity::WireGuardIdentity::with_interface(&wg_iface);
        match wg_id.get_local_pubkey() {
            Ok(pubkey) => Some(pubkey),
            Err(e) => {
                tracing::warn!(
                    interface = %wg_iface,
                    error = %e,
                    "Could not read WG public key; set WG_PUBKEY env var to override"
                );
                None
            }
        }
    } else {
        tracing::info!(
            "No WG interface or WG_PUBKEY configured; running compact MCP without peer pubkey"
        );
        None
    };

    let executor: Arc<dyn ToolExecutor> = Arc::new(PrewarmedOpToolsExecutor::new().await?);
    let server = Arc::new(CompactServer::new(executor));

    server
        .set_session(SessionContext {
            peer_pubkey,
            ..Default::default()
        })
        .await;

    info!("Starting compact MCP server (stdio)");

    StdioTransport::new().serve(server).await
}

/// Run compact server on a Unix domain socket
pub async fn run_compact_unix_server(unix_path: &std::path::Path) -> Result<()> {
    use crate::transport::{HttpSseTransport, Transport};

    // ── WireGuard identity ────────────────────────────────────────────────────
    // Read the local WG pubkey and resolve its projected identity session,
    // and stamp peer_pubkey into the session so tools can use it for auth.
    let peer_pubkey = if let Ok(pubkey) = std::env::var("WG_PUBKEY") {
        info!(pubkey = %pubkey, "WG_PUBKEY configured; using injected WireGuard identity");
        Some(pubkey)
    } else if let Ok(wg_iface) =
        std::env::var("WG_INTERFACE").or_else(|_| std::env::var("FABRIC_WG_INTERFACE"))
    {
        tracing::warn!(
            interface = %wg_iface,
            "WG_INTERFACE/FABRIC_WG_INTERFACE identity path is deprecated; prefer explicit pubkey injection"
        );
        let wg_id = op_identity::WireGuardIdentity::with_interface(&wg_iface);
        match wg_id.get_local_pubkey() {
            Ok(pubkey) => {
                info!(interface = %wg_iface, pubkey = %pubkey, "WG identity resolved");
                Some(pubkey)
            }
            Err(e) => {
                tracing::warn!(
                    interface = %wg_iface,
                    error = %e,
                    "Could not read WG public key; set WG_PUBKEY env var to override"
                );
                None
            }
        }
    } else {
        tracing::info!(
            "No WG interface or WG_PUBKEY configured; running compact MCP without peer pubkey"
        );
        None
    };

    // Load the authoritative op-tools registry lazily per request so the
    // chatbot sees five stable meta-tools while retaining access to every
    // live system, D-Bus, OVS, and PluginSchema projection tool.
    let executor: Arc<dyn ToolExecutor> = Arc::new(PrewarmedOpToolsExecutor::new().await?);
    let server = Arc::new(CompactServer::new(executor));

    // Stamp the WG identity into the session context.
    server
        .set_session(SessionContext {
            peer_pubkey,
            ..Default::default()
        })
        .await;

    info!(path = %unix_path.display(), "Starting compact MCP server (unix socket)");

    let transport = HttpSseTransport::new(String::new())
        .with_unix_path(unix_path.to_path_buf())
        .with_base_path("/mcp/compact");
    transport.serve(server).await
}

// Implement McpHandler for CompactServer
#[async_trait::async_trait]
impl crate::transport::McpHandler for CompactServer {
    async fn handle_request(&self, request: McpRequest) -> McpResponse {
        self.handle_request(request).await
    }
}
