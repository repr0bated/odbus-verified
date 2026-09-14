//! MCP Router - HTTP endpoints for MCP protocol
//!
//! This module exports a router that can be mounted by op-http.
//! NO server code here - just route definitions.

use axum::{
    extract::State,
    response::{sse::Event, IntoResponse, Sse},
    routing::{get, post},
    Json, Router,
};
use futures::stream::{self, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use simd_json::{json, OwnedValue as Value};
use std::{convert::Infallible, sync::Arc, time::Duration};

use crate::lazy_tools::LazyToolManager;
use crate::server::McpServer;

/// MCP service state
#[derive(Clone)]
pub struct McpState {
    pub server: Arc<McpServer>,
    pub tool_manager: Arc<LazyToolManager>,
}

impl McpState {
    pub async fn new() -> anyhow::Result<Self> {
        let server = Arc::new(McpServer::new(Default::default()).await?);
        let tool_manager = Arc::new(LazyToolManager::new().await?);

        Ok(Self {
            server,
            tool_manager,
        })
    }
}

/// Create the MCP router
///
/// Mount this at `/api/mcp` in the unified server:
/// ```ignore
/// use op_http::prelude::*;
/// use op_mcp::router::{create_router, McpState};
///
/// let state = McpState::new().await?;
/// let router = RouterBuilder::new()
///     .nest("/api/mcp", "mcp", create_router(state))
///     .build();
/// ```
pub fn create_router(state: McpState) -> Router {
    Router::new()
        .route("/", post(mcp_handler))
        .route("/health", get(health_handler))
        .route("/sse", get(sse_handler))
        .route("/tools", get(list_all_tools_handler))
        .route("/tools/:name", post(call_tool_handler))
        .route("/initialize", post(initialize_handler))
        .with_state(state)
}

/// Service info for op-http ServiceRouter trait
pub struct McpServiceRouter;

impl op_http::router::ServiceRouter for McpServiceRouter {
    fn prefix() -> &'static str {
        "/api/mcp"
    }

    fn name() -> &'static str {
        "mcp"
    }

    fn description() -> &'static str {
        "MCP protocol endpoints"
    }
}

// === Handlers ===

#[derive(Deserialize)]
struct McpRequest {
    jsonrpc: String,
    id: Value,
    method: String,
    params: Option<Value>,
}

#[derive(Serialize)]
struct McpResponse {
    jsonrpc: String,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

async fn mcp_handler(
    State(state): State<McpState>,
    Json(request): Json<McpRequest>,
) -> impl IntoResponse {
    let result = match request.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": "2024-11-05",
            "serverInfo": {
                "name": "op-mcp",
                "version": env!("CARGO_PKG_VERSION")
            },
            "capabilities": {
                "tools": { "listChanged": true }
            }
        })),
        "tools/list" => {
            let tools = state.tool_manager.list_all_tools().await;
            Ok(json!({ "tools": tools }))
        }
        "tools/call" => {
            if let Some(params) = request.params {
                let name = params.as_object().and_then(|o| o.get("name")).and_then(|v| v.as_str()).unwrap_or("");
                let args = params.as_object().and_then(|o| o.get("arguments")).cloned().unwrap_or(json!({}));
                if let Some(tool) = state.tool_manager.get_tool(name).await {
                    match tool.execute(args).await {
                        Ok(result) => Ok(result),
                        Err(e) => Err(json!({
                            "code": -32603,
                            "message": e.to_string()
                        })),
                    }
                } else {
                    Err(json!({
                        "code": -32601,
                        "message": format!("Tool not found: {}", name)
                    }))
                }
            }
            else {
                Err(json!({
                    "code": -32602,
                    "message": "Missing params"
                }))
            }
        }
        _ => Err(json!({
            "code": -32601,
            "message": format!("Method not found: {}", request.method)
        })),
    };

    let response = match result {
        Ok(r) => McpResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: Some(r),
            error: None,
        },
        Err(e) => McpResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(e),
        },
    };

    Json(response)
}

async fn health_handler() -> impl IntoResponse {
    Json(json!({
        "status": "healthy",
        "service": "mcp",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn sse_handler(
    State(state): State<McpState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Send initial events then keep alive
    let tools = state.tool_manager.list_all_tools().await;

    let initial_events = vec![
        Event::default()
            .event("endpoint")
            .data("/api/mcp"),
        Event::default()
            .event("tools")
            .data(json!({
                "name": "op-mcp",
                "count": tools.len(),
                "tools": tools
            }).to_string()),
    ];

    let initial_stream = stream::iter(initial_events.into_iter().map(Ok));

    let keepalive_stream = stream::unfold(0u64, |counter| async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let event = Event::default()
            .event("ping")
            .data(json!({ "counter": counter }).to_string());
        Some((Ok(event), counter + 1))
    });

    Sse::new(initial_stream.chain(keepalive_stream)).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(30))
            .text("keepalive"),
    )
}

async fn list_all_tools_handler(State(state): State<McpState>) -> impl IntoResponse {
    let tools = state.tool_manager.list_all_tools().await;
    Json(json!({ "tools": tools }))
}

async fn call_tool_handler(
    State(state): State<McpState>,
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(params): Json<Value>,
) -> impl IntoResponse {
    if let Some(tool) = state.tool_manager.get_tool(&name).await {
        match tool.execute(params).await {
            Ok(result) => Json(json!({ "result": result })),
            Err(e) => Json(json!({ "error": e.to_string() })),
        }
    } else {
        Json(json!({ "error": "Tool not found" }))
    }
}

async fn initialize_handler() -> impl IntoResponse {
    Json(json!({
        "protocolVersion": "2024-11-05",
        "serverInfo": {
            "name": "op-mcp",
            "version": env!("CARGO_PKG_VERSION")
        },
        "capabilities": {
            "tools": { "listChanged": true }
        }
    }))
}
