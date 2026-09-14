//! Router configuration for op-web

use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;

use crate::routes;
use crate::state::AppState;
use crate::websocket::handle_websocket;

/// Web service router builder
pub struct WebServiceRouter;

impl WebServiceRouter {
    /// Create a new router with all routes
    pub fn new() -> Self {
        Self
    }
}

/// Create the main application router
pub fn create_router(state: Arc<AppState>, static_dir: Option<String>) -> Router {
    let router = Router::new()
        // Health check
        .route("/api/health", get(routes::health_check))
        // LLM routes
        .route("/api/schema/catalog", get(crate::handlers::schema::schema_catalog_raw_handler))
        .route("/api/schema/catalog/detail", get(crate::handlers::schema::schema_catalog_handler))
        .route("/api/llm/status", get(routes::llm::get_llm_status))
        .route(
            "/api/llm/providers",
            get(crate::handlers::llm::list_providers_handler),
        )
        .route("/api/llm/models", get(routes::llm::get_models))
        // Chat routes
        .route("/api/chat", post(routes::chat::chat_message))
        // Tool routes
        .route("/api/tools", get(routes::tools::list_tools))
        .route("/api/tools/:name", get(routes::tools::get_tool))
        .route(
            "/api/tools/:name/execute",
            post(routes::tools::execute_tool),
        )
        // WebSocket
        .route("/ws", get(handle_websocket))
        .with_state(state);

    // No SPA fallback: API/WebSocket only. Unmatched routes return 404.
    let _ = static_dir;
    router
}

/// Create WebSocket-only router (for composition)
pub fn create_websocket_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/ws", get(handle_websocket))
        .with_state(state)
}
