//! Status Handler - Comprehensive system status

use axum::{extract::Extension, response::Json};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

use crate::state::AppState;

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
        // D-Bus projected methods generally include bus/interface path segments.
        "dbus"
    } else if name.starts_with("agent_") {
        "agent"
    } else {
        "other"
    }
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub app_uptime_secs: u64,
    pub system: Option<SystemInfo>,
    pub tools: ToolsInfo,
    pub llm: LlmInfo,
    pub agents: AgentsInfo,
    pub services: Option<Vec<ServiceStatus>>,
    pub network: Option<NetworkInfo>,
}

#[derive(Serialize)]
pub struct SystemInfo {
    pub hostname: String,
    pub kernel: String,
    pub uptime_secs: u64,
    pub load_average: [f64; 3],
    pub memory_total_mb: u64,
    pub memory_used_mb: u64,
    pub memory_percent: f64,
    pub cpu_count: usize,
    pub cpu_usage: f32,
}

#[derive(Serialize)]
pub struct ToolsInfo {
    pub total: usize,
    pub by_category: HashMap<String, usize>,
}

#[derive(Serialize)]
pub struct LlmInfo {
    pub provider: String,
    pub model: String,
    pub available: bool,
}

#[derive(Serialize)]
pub struct AgentsInfo {
    pub types_available: usize,
    pub instances_running: usize,
}

#[derive(Serialize)]
pub struct ServiceStatus {
    pub name: String,
    pub status: String,
}

#[derive(Serialize)]
pub struct NetworkInfo {
    pub interfaces: Vec<InterfaceInfo>,
}

#[derive(Serialize)]
pub struct InterfaceInfo {
    pub name: String,
    pub state: String,
    pub mac_address: Option<String>,
}

/// GET /api/status - Comprehensive system status
pub async fn status_handler(Extension(state): Extension<Arc<AppState>>) -> Json<StatusResponse> {
    // Get tools info
    let tools_list = state.tool_registry.list().await;
    let mut by_category: HashMap<String, usize> = HashMap::new();
    for tool in &tools_list {
        *by_category
            .entry(categorize_tool(&tool.name).to_string())
            .or_insert(0) += 1;
    }

    let tools = ToolsInfo {
        total: tools_list.len(),
        by_category,
    };

    // Get LLM info
    let provider = state.chat_manager.current_provider().await;
    let model = state.chat_manager.current_model().await;
    let llm = LlmInfo {
        provider: provider.to_string(),
        model,
        available: true,
    };

    // Get agents info
    let agent_types = op_agents::list_agent_types().len();
    let agent_instances = state
        .agent_registry
        .read()
        .await
        .list_instances()
        .await
        .len();
    let agents = AgentsInfo {
        types_available: agent_types,
        instances_running: agent_instances,
    };

    Json(StatusResponse {
        app_uptime_secs: state.uptime_secs(),
        system: None,
        tools,
        llm,
        agents,
        services: None,
        network: None,
    })
}
