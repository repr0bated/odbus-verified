//! Request Handler - Processes MCP requests with per-request tool loading
//!
//! Each request gets its own RequestContext with all tools loaded.
//! Tools are unloaded when the request completes.
//!
//! Security (audit item #7): `meta_execute_tool` is the compact-mode entry
//! point for arbitrary tool invocation. The blocklist itself is enforced in
//! `RequestContext::execute_tool` (single choke point covering both this path
//! and the verbose `tools/call` path). The hardening here is purely
//! input-shape validation plus a guard against meta-tool reflection.

use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::compact::{ToolDefinition, CompactServerConfig};
use crate::protocol::{McpRequest, McpResponse, JsonRpcError};
use crate::request_context::{RequestContext, RequestConfig};
use crate::tool_registry::Tool;
use crate::tools;
use crate::{PROTOCOL_VERSION, SERVER_NAME, SERVER_VERSION};

struct OpToolsAdapter {
    inner: op_tools::BoxedTool,
}

#[async_trait]
impl Tool for OpToolsAdapter {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn input_schema(&self) -> Value {
        self.inner.input_schema()
    }

    fn category(&self) -> &str {
        self.inner.category()
    }

    fn namespace(&self) -> &str {
        self.inner.namespace()
    }

    fn tags(&self) -> Vec<String> {
        self.inner.tags()
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        self.inner.execute(input).await
    }
}

/// Meta-tool names. `meta_execute_tool` must reject these as targets to
/// prevent trivial recursion / reflection from the compact path.
const META_TOOL_NAMES: &[&str] = &[
    "execute_tool",
    "list_tools",
    "search_tools",
    "get_tool_schema",
    "respond",
];

/// Request handler that creates per-request contexts
pub struct RequestHandler {
    config: CompactServerConfig,
}

impl RequestHandler {
    pub fn new(config: CompactServerConfig) -> Self {
        Self { config }
    }

    /// Handle an MCP request
    /// 
    /// This creates a RequestContext, loads all tools, processes the request,
    /// then drops the context (unloading tools).
    pub async fn handle(&self, request: McpRequest) -> McpResponse {
        let request_id = uuid::Uuid::new_v4().to_string();
        
        info!(
            request_id = %request_id,
            method = %request.method,
            "Handling MCP request"
        );

        match request.method.as_str() {
            "initialize" => self.handle_initialize(&request),
            "initialized" => McpResponse::success(request.id, json!({})),
            "tools/list" => self.handle_tools_list(&request, &request_id).await,
            "tools/call" => self.handle_tools_call(&request, &request_id).await,
            "ping" => McpResponse::success(request.id, json!({})),
            _ => McpResponse::error(
                request.id,
                JsonRpcError::method_not_found(&request.method),
            ),
        }
    }

    /// Handle initialize - no tools loaded yet
    fn handle_initialize(&self, request: &McpRequest) -> McpResponse {
        let server_name = self.config.name.as_deref().unwrap_or(SERVER_NAME);
        
        McpResponse::success(
            request.id.clone(),
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "serverInfo": {
                    "name": server_name,
                    "version": SERVER_VERSION
                },
                "capabilities": {
                    "tools": {
                        "listChanged": false
                    }
                },
                "_meta": {
                    "mode": "compact",
                    "max_turns_per_request": self.config.max_turns,
                    "description": "Compact mode: 5 meta-tools, per-request tool loading"
                }
            }),
        )
    }

    /// Handle tools/list - load tools, return meta-tools, unload
    async fn handle_tools_list(&self, request: &McpRequest, request_id: &str) -> McpResponse {
        // Create context and load tools
        let mut ctx = self.create_context(request_id);
        
        if let Err(e) = self.load_tools(&mut ctx).await {
            error!("Failed to load tools: {}", e);
            return McpResponse::error(
                request.id.clone(),
                JsonRpcError::new(-32000, format!("Failed to load tools: {}", e), None),
            );
        }

        // Return meta-tools (compact mode)
        let meta_tools = self.meta_tool_definitions();
        let underlying_count = ctx.tool_count();
        
        // Context is dropped here, unloading tools
        McpResponse::success(
            request.id.clone(),
            json!({
                "tools": meta_tools,
                "_meta": {
                    "mode": "compact",
                    "meta_tools": meta_tools.len(),
                    "underlying_tools": underlying_count,
                    "max_turns_per_request": self.config.max_turns
                }
            }),
        )
    }

    /// Handle tools/call - load tools, execute, unload
    async fn handle_tools_call(&self, request: &McpRequest, request_id: &str) -> McpResponse {
        // Create context and load tools
        let mut ctx = self.create_context(request_id);
        
        if let Err(e) = self.load_tools(&mut ctx).await {
            error!("Failed to load tools: {}", e);
            return McpResponse::error(
                request.id.clone(),
                JsonRpcError::new(-32000, format!("Failed to load tools: {}", e), None),
            );
        }

        let params = request.params.as_ref();
        
        let tool_name = params
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("");
        
        let arguments = params
            .and_then(|p| p.get("arguments"))
            .cloned()
            .unwrap_or(json!({}));

        info!(
            request_id = %request_id,
            tool = %tool_name,
            turn = ctx.turn_count() + 1,
            max_turns = self.config.max_turns,
            "Executing tool"
        );

        // Execute based on meta-tool name
        let result = match tool_name {
            "execute_tool" => self.meta_execute_tool(&ctx, arguments).await,
            "list_tools" => self.meta_list_tools(&ctx, arguments),
            "search_tools" => self.meta_search_tools(&ctx, arguments),
            "get_tool_schema" => self.meta_get_tool_schema(&ctx, arguments),
            "respond" => self.meta_respond(arguments),
            _ => Err(anyhow::anyhow!("Unknown meta-tool: {}", tool_name)),
        };

        let summary = ctx.summary();
        
        // Context is dropped here, unloading tools
        match result {
            Ok(value) => McpResponse::success(
                request.id.clone(),
                json!({
                    "content": [{
                        "type": "text",
                        "text": simd_json::to_string_pretty(&value).unwrap_or_default()
                    }],
                    "_meta": {
                        "request_id": summary.request_id,
                        "turn": summary.turns_used,
                        "max_turns": summary.max_turns,
                        "remaining": summary.max_turns - summary.turns_used,
                        "elapsed_secs": summary.elapsed_secs
                    }
                }),
            ),
            Err(e) => McpResponse::error(
                request.id.clone(),
                JsonRpcError::new(-32000, e.to_string(), None),
            ),
        }
    }

    /// Create a new request context
    fn create_context(&self, request_id: &str) -> RequestContext {
        let config = RequestConfig {
            max_turns: self.config.max_turns as u32,
            timeout_secs: 300,
            preload_all: true,
        };
        RequestContext::new(request_id.to_string(), config)
    }

    /// Load all tools into context
    async fn load_tools(&self, ctx: &mut RequestContext) -> Result<()> {
        // Load the authoritative op-tools registry per request. The chatbot
        // only sees five meta-tools, while execute_tool can reach every live
        // system, D-Bus, OVS, and PluginSchema projection tool.
        let registry = op_tools::ToolRegistry::new();
        op_tools::register_builtin_tools(&registry).await?;
        for definition in registry.list().await {
            if let Some(tool) = registry.get(&definition.name).await {
                ctx.load_tool(Arc::new(OpToolsAdapter { inner: tool }));
            }
        }

        // Response tools
        ctx.load_tool(Arc::new(tools::response::RespondToUserTool));
        ctx.load_tool(Arc::new(tools::response::CannotPerformTool));
        ctx.load_tool(Arc::new(tools::response::RequestClarificationTool));
        
        // Filesystem tools
        ctx.load_tool(Arc::new(tools::filesystem::ReadFileTool));
        ctx.load_tool(Arc::new(tools::filesystem::WriteFileTool));
        ctx.load_tool(Arc::new(tools::filesystem::ListDirectoryTool));
        
        // Shell tools
        ctx.load_tool(Arc::new(tools::shell::ShellExecuteTool::new()));
        
        // System tools
        ctx.load_tool(Arc::new(tools::system::ListNetworkInterfacesTool));
        
        // Systemd tools
        ctx.load_tool(Arc::new(tools::systemd::SystemdUnitStatusTool));
        ctx.load_tool(Arc::new(tools::systemd::SystemdListUnitsTool));
        ctx.load_tool(Arc::new(tools::systemd::SystemdStartUnitTool));
        ctx.load_tool(Arc::new(tools::systemd::SystemdStopUnitTool));
        ctx.load_tool(Arc::new(tools::systemd::SystemdRestartUnitTool));
        ctx.load_tool(Arc::new(tools::systemd::SystemdEnableUnitTool));
        ctx.load_tool(Arc::new(tools::systemd::SystemdDisableUnitTool));
        ctx.load_tool(Arc::new(tools::systemd::SystemdReloadDaemonTool));
        
        // OVS tools
        ctx.load_tool(Arc::new(tools::ovs::OvsListBridgesTool));
        ctx.load_tool(Arc::new(tools::ovs::OvsShowBridgeTool));
        ctx.load_tool(Arc::new(tools::ovs::OvsListPortsTool));
        ctx.load_tool(Arc::new(tools::ovs::OvsDumpFlowsTool));
        ctx.load_tool(Arc::new(tools::ovs::OvsAddBridgeTool));
        ctx.load_tool(Arc::new(tools::ovs::OvsDelBridgeTool));
        ctx.load_tool(Arc::new(tools::ovs::OvsAddPortTool));
        ctx.load_tool(Arc::new(tools::ovs::OvsDelPortTool));
        ctx.load_tool(Arc::new(tools::ovs::OvsAddFlowTool));
        ctx.load_tool(Arc::new(tools::ovs::OvsDelFlowsTool));
        
        // Plugin state tools (9 plugins \u00d7 3 ops = 27 tools)
        for plugin in &["systemd", "network", "packagekit", "firewall", "users", "storage", "lxc", "openflow", "privacy"] {
            ctx.load_tool(Arc::new(tools::plugin::PluginQueryTool::new(plugin)));
            ctx.load_tool(Arc::new(tools::plugin::PluginDiffTool::new(plugin)));
            ctx.load_tool(Arc::new(tools::plugin::PluginApplyTool::new(plugin)));
        }
        
        info!(
            request_id = %ctx.request_id,
            count = ctx.tool_count(),
            "Loaded all tools for request"
        );
        
        Ok(())
    }

    /// Meta-tool definitions (the 5 tools LLM sees)
    fn meta_tool_definitions(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "execute_tool".to_string(),
                description: "Execute any available tool by name. Use list_tools or search_tools to discover tools first.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "tool_name": {"type": "string", "description": "Name of the tool to execute"},
                        "arguments": {"type": "object", "description": "Arguments to pass to the tool"}
                    },
                    "required": ["tool_name"]
                }),
                category: "meta".to_string(),
                tags: vec!["meta".to_string()],
            },
            ToolDefinition {
                name: "list_tools".to_string(),
                description: "List available tools, optionally by category. Categories: response, filesystem, shell, system, systemd, ovs, network, plugin.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "category": {"type": "string"},
                        "offset": {"type": "integer", "default": 0},
                        "limit": {"type": "integer", "default": 50}
                    }
                }),
                category: "meta".to_string(),
                tags: vec!["meta".to_string()],
            },
            ToolDefinition {
                name: "search_tools".to_string(),
                description: "Search for tools by keyword.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"}
                    },
                    "required": ["query"]
                }),
                category: "meta".to_string(),
                tags: vec!["meta".to_string()],
            },
            ToolDefinition {
                name: "get_tool_schema".to_string(),
                description: "Get the input schema for a specific tool.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "tool_name": {"type": "string"}
                    },
                    "required": ["tool_name"]
                }),
                category: "meta".to_string(),
                tags: vec!["meta".to_string()],
            },
            ToolDefinition {
                name: "respond".to_string(),
                description: "Send a response to the user.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "message": {"type": "string"}
                    },
                    "required": ["message"]
                }),
                category: "meta".to_string(),
                tags: vec!["meta".to_string()],
            },
        ]
    }

    // Meta-tool implementations

    /// Compact-mode `execute_tool` meta-tool.
    ///
    /// Security (audit item #7):
    /// 1. `tool_name` MUST be a non-empty string. Reject everything else
    ///    explicitly rather than silently falling through to a registry miss.
    /// 2. `arguments` MUST be a JSON object (or absent). Strings, arrays, and
    ///    nulls are rejected.
    /// 3. Meta-tool reflection (e.g. `execute_tool("execute_tool", \u2026)`) is
    ///    rejected outright.
    /// 4. The actual blocklist (shell_execute, systemd_*, ovs_create, etc.)
    ///    is enforced inside `ctx.execute_tool` so that the verbose
    ///    `tools/call` path is covered by the same check.
    async fn meta_execute_tool(&self, ctx: &RequestContext, args: Value) -> Result<Value> {
        let obj = args
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("execute_tool: arguments must be a JSON object"))?;

        let tool_name = obj
            .get("tool_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("execute_tool: tool_name must be a non-empty string"))?
            .trim();

        if tool_name.is_empty() {
            anyhow::bail!("execute_tool: tool_name must be a non-empty string");
        }

        if META_TOOL_NAMES.contains(&tool_name) {
            warn!(
                request_id = %ctx.request_id,
                tool = %tool_name,
                "Rejected meta-tool reflection via execute_tool"
            );
            anyhow::bail!(
                "execute_tool cannot target meta-tool '{}'; call it directly via tools/call",
                tool_name
            );
        }

        // arguments defaults to {} but, if present, must be an object.
        let arguments = match obj.get("arguments") {
            None => json!({}),
            Some(v) if v.is_object() => v.clone(),
            Some(_) => anyhow::bail!("execute_tool: arguments must be a JSON object"),
        };

        // The blocklist check happens here, inside ctx.execute_tool.
        ctx.execute_tool(tool_name, arguments).await
    }

    fn meta_list_tools(&self, ctx: &RequestContext, args: Value) -> Result<Value> {
        let category = args.as_object().and_then(|o| o.get("category")).and_then(|v| v.as_str());
        let offset = args.as_object().and_then(|o| o.get("offset")).and_then(|v| v.as_u64()).unwrap_or(0) as usize;
        let limit = args.as_object().and_then(|o| o.get("limit")).and_then(|v| v.as_u64()).unwrap_or(50) as usize;
        
        let tools = ctx.list_tools(offset, limit, category);
        let total = ctx.tool_count();
        
        Ok(json!({
            "tools": tools.iter().map(|t| json!({
                "name": t.name,
                "description": t.description,
                "category": t.category
            })).collect::<Vec<_>>(),
            "total": total,
            "offset": offset,
            "limit": limit,
            "has_more": offset + tools.len() < total
        }))
    }

    fn meta_search_tools(&self, ctx: &RequestContext, args: Value) -> Result<Value> {
        let query = args.as_object().and_then(|o| o.get("query")).and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing query"))?;
        
        let results = ctx.search_tools(query);
        
        Ok(json!({
            "query": query,
            "results": results.iter().map(|t| json!({
                "name": t.name,
                "description": t.description,
                "category": t.category
            })).collect::<Vec<_>>(),
            "count": results.len()
        }))
    }

    fn meta_get_tool_schema(&self, ctx: &RequestContext, args: Value) -> Result<Value> {
        let tool_name = args.as_object().and_then(|o| o.get("tool_name")).and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing tool_name"))?;
        
        let def = ctx.get_definition(tool_name)
            .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", tool_name))?;
        
        Ok(json!({
            "name": def.name,
            "description": def.description,
            "inputSchema": def.input_schema,
            "category": def.category
        }))
    }

    fn meta_respond(&self, args: Value) -> Result<Value> {
        let message = args.as_object().and_then(|o| o.get("message")).and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing message"))?;
        
        Ok(json!({
            "type": "response",
            "message": message,
            "delivered": true
        }))
    }
}
