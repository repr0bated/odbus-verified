//! Agent API Handlers

use axum::{
    extract::{Extension, Path},
    response::Json,
};
use op_agents::{load_default_specs, AgentTask};
use serde::Deserialize;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tracing::info;

use crate::state::AppState;

/// GET /api/agents - List running agent instances
pub async fn list_agents_handler(Extension(state): Extension<Arc<AppState>>) -> Json<Value> {
    let registry = state.agent_registry.read().await;
    let agents = registry.list_instances().await;
    Json(json!({ "agents": agents }))
}

/// GET /api/agents/types - List available agent types
pub async fn list_agent_types_handler(Extension(state): Extension<Arc<AppState>>) -> Json<Value> {
    let registry = state.agent_registry.read().await;
    let mut types = registry.list_agent_types().await;
    if types.is_empty() {
        types = op_agents::list_agent_types()
            .into_iter()
            .map(str::to_string)
            .collect();
    }
    Json(json!({ "types": types }))
}

/// GET /api/agents/:id - Get agent status
pub async fn get_agent_handler(
    Extension(state): Extension<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<Value> {
    let registry = state.agent_registry.read().await;
    match registry.get_instance_status(&id).await {
        Ok(status) => Json(json!({ "agent": status })),
        Err(_) => Json(json!({ "error": "Agent not found" })),
    }
}

#[derive(Debug, Deserialize)]
pub struct SpawnAgentRequest {
    #[serde(rename = "type")]
    pub agent_type: String,
    #[serde(default)]
    pub config: Option<Value>,
}

/// POST /api/agents - Spawn a new agent
pub async fn spawn_agent_handler(
    Extension(state): Extension<Arc<AppState>>,
    Json(request): Json<SpawnAgentRequest>,
) -> Json<Value> {
    info!("Spawning agent of type: {}", request.agent_type);

    {
        let registry = state.agent_registry.read().await;
        if registry.list_agent_types().await.is_empty() {
            drop(registry);
            let registry = state.agent_registry.write().await;
            if let Err(e) = load_default_specs(&registry).await {
                return Json(json!({ "error": e.to_string(), "success": false }));
            }
        }
    }

    let registry = state.agent_registry.write().await;
    match registry
        .spawn_agent(&request.agent_type, request.config)
        .await
    {
        Ok(id) => Json(json!({ "agent_id": id, "success": true })),
        Err(e) => Json(json!({ "error": e.to_string(), "success": false })),
    }
}

/// DELETE /api/agents/:id - Kill an agent
pub async fn kill_agent_handler(
    Extension(state): Extension<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<Value> {
    info!("Killing agent: {}", id);

    let registry = state.agent_registry.write().await;
    match registry.kill_agent(&id).await {
        Ok(_) => Json(json!({ "killed": true })),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

#[derive(Debug, Deserialize)]
pub struct AgentTaskRequest {
    pub operation: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub args: Option<String>,
}

/// POST /api/agents/:id/task - Execute a task on a spawned in-process agent
pub async fn agent_task_handler(
    Extension(state): Extension<Arc<AppState>>,
    Path(id): Path<String>,
    Json(request): Json<AgentTaskRequest>,
) -> Json<Value> {
    let registry = state.agent_registry.read().await;
    let agent_type = match registry.get_instance_status(&id).await {
        Ok(inst) => inst.agent_type.clone(),
        Err(e) => return Json(json!({ "error": e.to_string(), "success": false })),
    };

    let mut task = AgentTask::new(&agent_type, &request.operation);
    if let Some(path) = request.path {
        task = task.with_path(&path);
    }
    if let Some(args) = request.args {
        task = task.with_args(&args);
    }

    match registry.execute_instance_task(&id, task).await {
        Ok(result) => Json(json!({
            "success": result.success,
            "operation": result.operation,
            "data": result.data,
            "metadata": result.metadata,
        })),
        Err(e) => Json(json!({ "error": e.to_string(), "success": false })),
    }
}
