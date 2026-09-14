//! Tools Router - HTTP endpoints for tool management
//!
//! This module exports a router that can be mounted by op-http.

use axum::{
    extract::State,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;

use crate::registry::ToolRegistry;

/// Tools service state
#[derive(Clone)]
pub struct ToolsState {
    pub registry: Arc<ToolRegistry>,
}

impl ToolsState {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self { registry }
    }
}

/// Create the tools router
///
/// Mount this at `/api/tools` in the unified server:
/// ```ignore
/// use op_http::prelude::*;
/// use op_tools::router::{create_router, ToolsState};
///
/// let registry = Arc::new(ToolRegistry::new());
/// let state = ToolsState::new(registry);
/// let router = RouterBuilder::new()
///     .nest("/api/tools", "tools", create_router(state))
///     .build();
/// ```
pub fn create_router(state: ToolsState) -> Router {
    Router::new()
        .route("/", get(list_tools_handler))
        .route("/health", get(health_handler))
        .route("/:name", get(get_tool_handler))
        .route("/:name/execute", post(execute_tool_handler))
        .with_state(state)
}

/// Service info for op-http ServiceRouter trait
pub struct ToolsServiceRouter;

impl op_http::router::ServiceRouter for ToolsServiceRouter {
    fn prefix() -> &'static str {
        "/api/tools"
    }

    fn name() -> &'static str {
        "tools"
    }

    fn description() -> &'static str {
        "Tool registry API endpoints"
    }
}

// === Handlers ===

async fn health_handler() -> impl IntoResponse {
    Json(json!({
        "status": "healthy",
        "service": "tools"
    }))
}

async fn list_tools_handler(State(state): State<ToolsState>) -> impl IntoResponse {
    let tools = state.registry.list().await;
    let tool_list: Vec<_> = tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description
            })
        })
        .collect();

    Json(json!({
        "tools": tool_list,
        "count": tool_list.len()
    }))
}

async fn get_tool_handler(
    State(state): State<ToolsState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> impl IntoResponse {
    match state.registry.get(&name).await {
        Some(tool) => Json(json!({
            "name": tool.name(),
            "description": tool.description(),
            "inputSchema": tool.input_schema()
        })),
        None => Json(json!({ "error": "Tool not found" })),
    }
}

async fn execute_tool_handler(
    State(state): State<ToolsState>,
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(params): Json<Value>,
) -> impl IntoResponse {
    if let Some(tool) = state.registry.get(&name).await {
        match tool.execute(params).await {
            Ok(result) => Json(json!({
                "success": true,
                "result": result
            })),
            Err(e) => Json(json!({
                "success": false,
                "error": e.to_string()
            })),
        }
    } else {
        Json(json!({
            "success": false,
            "error": "Tool not found"
        }))
    }
}
