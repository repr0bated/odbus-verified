//! Agents Router - HTTP endpoints for agent management
//!
//! This module exports a router that can be mounted by op-http.

use axum::{
    extract::State,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::agent_registry::AgentRegistry;

/// Agents service state
#[derive(Clone)]
pub struct AgentsState {
    pub registry: Arc<RwLock<AgentRegistry>>,
}

impl AgentsState {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(RwLock::new(AgentRegistry::new())),
        }
    }

    pub fn with_registry(registry: AgentRegistry) -> Self {
        Self {
            registry: Arc::new(RwLock::new(registry)),
        }
    }
}

impl Default for AgentsState {
    fn default() -> Self {
        Self::new()
    }
}

/// Create the agents router
///
/// Mount this at `/api/agents` in the unified server:
/// ```ignore
/// use op_http::prelude::*;
/// use op_agents::router::{create_router, AgentsState};
///
/// let state = AgentsState::new();
/// let router = RouterBuilder::new()
///     .nest("/api/agents", "agents", create_router(state))
///     .build();
/// ```
pub fn create_router(state: AgentsState) -> Router {
    Router::new()
        .route("/", get(list_agents_handler))
        .route("/", post(spawn_agent_handler))
        .route("/health", get(health_handler))
        .route("/types", get(list_types_handler))
        .route("/:id", get(get_agent_handler))
        .route("/:id", delete(kill_agent_handler))
        //.route("/:id/task", post(send_task_handler))
        .with_state(state)
}

/// Service info for op-http ServiceRouter trait
pub struct AgentsServiceRouter;

impl op_http::router::ServiceRouter for AgentsServiceRouter {
    fn prefix() -> &'static str {
        "/api/agents"
    }

    fn name() -> &'static str {
        "agents"
    }

    fn description() -> &'static str {
        "Agent management API endpoints"
    }
}

// === Handlers ===

async fn health_handler() -> impl IntoResponse {
    Json(json!({
        "status": "healthy",
        "service": "agents"
    }))
}

async fn list_agents_handler(State(state): State<AgentsState>) -> impl IntoResponse {
    let registry = state.registry.read().await;
    let agents = registry.list_instances().await;
    Json(json!({ "agents": agents }))
}

async fn list_types_handler() -> impl IntoResponse {
    let types = crate::list_agent_types();
    Json(json!({ "types": types }))
}

async fn get_agent_handler(
    State(state): State<AgentsState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let registry = state.registry.read().await;
    match registry.get_instance_status(&id).await {
        Ok(status) => Json(json!({ "agent": status })),
        Err(_) => Json(json!({ "error": "Agent not found" })),
    }
}

async fn spawn_agent_handler(
    State(state): State<AgentsState>,
    Json(request): Json<Value>,
) -> impl IntoResponse {
    let agent_type = request
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("executor");
    let config = request.get("config").cloned();

    let registry = state.registry.write().await;
    match registry.spawn_agent(agent_type, config).await {
        Ok(id) => Json(json!({ "agent_id": id })),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

async fn kill_agent_handler(
    State(state): State<AgentsState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let registry = state.registry.write().await;
    match registry.kill_agent(&id).await {
        Ok(_) => Json(json!({ "killed": true })),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

// TODO: Implement send_task
// async fn send_task_handler(
//     State(state): State<AgentsState>,
//     axum::extract::Path(id): axum::extract::Path<String>,
//     Json(task): Json<Value>,
// ) -> impl IntoResponse {
//     let registry = state.registry.read().await;
//     match registry.send_task(&id, task).await {
//         Ok(result) => Json(json!({ "result": result })),
//         Err(e) => Json(json!({ "error": e.to_string() })),
//     }
// }
