//! Status Handler - Comprehensive system status

use axum::{extract::Extension, response::Json};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use sysinfo::System;

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
    pub system: SystemInfo,
    pub tools: ToolsInfo,
    pub llm: LlmInfo,
    pub agents: AgentsInfo,
    pub services: Vec<ServiceStatus>,
    pub network: NetworkInfo,
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
    // Get system info
    let mut sys = System::new_all();
    sys.refresh_all();

    let hostname = gethostname::gethostname().to_string_lossy().to_string();
    let kernel = System::kernel_version().unwrap_or_else(|| "unknown".to_string());

    let load_avg = System::load_average();
    let memory_total_mb = sys.total_memory() / 1024 / 1024;
    let memory_used_mb = sys.used_memory() / 1024 / 1024;
    let memory_percent = if memory_total_mb > 0 {
        (memory_used_mb as f64 / memory_total_mb as f64) * 100.0
    } else {
        0.0
    };

    let cpu_usage = sys.global_cpu_info().cpu_usage();

    let system = SystemInfo {
        hostname,
        kernel,
        uptime_secs: state.uptime_secs(),
        load_average: [load_avg.one, load_avg.five, load_avg.fifteen],
        memory_total_mb,
        memory_used_mb,
        memory_percent,
        cpu_count: sys.cpus().len(),
        cpu_usage,
    };

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

    // Get key services status
    let services = get_key_services(&state.grpc_client).await;

    // Get network interfaces
    let network = get_network_info().await;

    Json(StatusResponse {
        system,
        tools,
        llm,
        agents,
        services,
        network,
    })
}

async fn get_key_services(_client: &op_grpc_bridge::RemoteOperationClient) -> Vec<ServiceStatus> {
    let mut services = Vec::new();

    let key_services = [
        "nginx",
        "docker",
        "sshd",
        "openvswitch-switch",
        "ovsdb-server",
        "ovs-vswitchd",
        "op-dbus",
        "op-web",
    ];

    for name in key_services {
        let status = if is_sv_service_running(name).await {
            "active".to_string()
        } else {
            "inactive".to_string()
        };
        services.push(ServiceStatus {
            name: name.to_string(),
            status,
        });
    }

    services
}

async fn is_sv_service_running(name: &str) -> bool {
    let stat_path = op_core::runit::supervise_stat_path(name);
    if let Ok(content) = tokio::fs::read_to_string(&stat_path).await {
        content.trim() == "run"
    } else {
        // Fallback: check /proc for process with the service name
        false
    }
}

async fn get_network_info() -> NetworkInfo {
    let mut interfaces = Vec::new();

    if let Ok(mut entries) = tokio::fs::read_dir("/sys/class/net").await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();

            let state_path = format!("/sys/class/net/{}/operstate", name);
            let state = tokio::fs::read_to_string(&state_path)
                .await
                .unwrap_or_else(|_| "unknown".to_string())
                .trim()
                .to_string();

            let mac_path = format!("/sys/class/net/{}/address", name);
            let mac_address = tokio::fs::read_to_string(&mac_path)
                .await
                .ok()
                .map(|m| m.trim().to_string())
                .filter(|m| m != "00:00:00:00:00:00");

            interfaces.push(InterfaceInfo {
                name,
                state,
                mac_address,
            });
        }
    }

    NetworkInfo { interfaces }
}
