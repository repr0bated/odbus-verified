//! Tool API Handlers

use axum::{
    extract::{Extension, Path},
    response::Json,
};
use serde::{Deserialize, Serialize};
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tracing::info;

use crate::state::AppState;

/// GET /api/tools - List all available tools
pub async fn list_tools_handler(Extension(state): Extension<Arc<AppState>>) -> Json<Value> {
    let tools = state.tool_registry.list().await;

    let tool_list: Vec<Value> = tools
        .iter()
        .map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "category": categorize_tool(&t.name),
            })
        })
        .collect();

    Json(json!({
        "tools": tool_list,
        "count": tool_list.len()
    }))
}

fn categorize_tool(name: &str) -> &'static str {
    if name.starts_with("ovs_") {
        "ovs"
    } else if name.starts_with("systemd_") || name.starts_with("dinit_") || name.starts_with("sv_")
    {
        "service"
    } else if name.starts_with("nm_") || name.starts_with("connman.") {
        "network"
    } else if name.starts_with("file_") {
        "file"
    } else if name.starts_with("system_") {
        "system"
    } else if name.starts_with("plugin_") {
        "plugin"
    } else if name.starts_with("dbus_")
        || name.starts_with("DBus.")
        || name.starts_with("org.")
        || name.contains('.')
    {
        "dbus"
    } else if name.starts_with("agent_") {
        "agent"
    } else {
        "other"
    }
}

/// GET /api/tools/:name - Get tool details
pub async fn get_tool_handler(
    Extension(state): Extension<Arc<AppState>>,
    Path(name): Path<String>,
) -> Json<Value> {
    let tools = state.tool_registry.list().await;

    if let Some(tool) = tools.iter().find(|t| t.name == name) {
        Json(json!({
            "found": true,
            "tool": {
                "name": tool.name,
                "description": tool.description,
                "category": categorize_tool(&tool.name),
                "input_schema": tool.input_schema
            }
        }))
    } else {
        Json(json!({
            "found": false,
            "error": format!("Tool '{}' not found", name)
        }))
    }
}

#[derive(Debug, Deserialize)]
pub struct DirectToolRequest {
    pub tool_name: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Serialize)]
pub struct DirectToolResponse {
    pub success: bool,
    pub tool_name: String,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub execution_time_ms: u64,
}

/// POST /api/tool - Execute a tool directly
pub async fn execute_tool_handler(
    Extension(state): Extension<Arc<AppState>>,
    Json(request): Json<DirectToolRequest>,
) -> Json<DirectToolResponse> {
    info!("Direct tool execution: {}", request.tool_name);
    execute_tool_internal(state, &request.tool_name, request.arguments).await
}

/// POST /api/tools/:name/execute - Execute a named tool
pub async fn execute_named_tool_handler(
    Extension(state): Extension<Arc<AppState>>,
    Path(name): Path<String>,
    Json(arguments): Json<Value>,
) -> Json<DirectToolResponse> {
    info!("Named tool execution: {}", name);
    execute_tool_internal(state, &name, arguments).await
}

async fn execute_tool_internal(
    state: Arc<AppState>,
    tool_name: &str,
    arguments: Value,
) -> Json<DirectToolResponse> {
    let start = std::time::Instant::now();

    let tool = match state.tool_registry.get(tool_name).await {
        Some(t) => t,
        None => {
            return Json(DirectToolResponse {
                success: false,
                tool_name: tool_name.to_string(),
                result: None,
                error: Some("Tool not found".to_string()),
                execution_time_ms: start.elapsed().as_millis() as u64,
            });
        }
    };

    match tool.execute(arguments).await {
        Ok(result) => Json(DirectToolResponse {
            success: true,
            tool_name: tool_name.to_string(),
            result: Some(result),
            error: None,
            execution_time_ms: start.elapsed().as_millis() as u64,
        }),
        Err(e) => Json(DirectToolResponse {
            success: false,
            tool_name: tool_name.to_string(),
            result: None,
            error: Some(e.to_string()),
            execution_time_ms: start.elapsed().as_millis() as u64,
        }),
    }
}
