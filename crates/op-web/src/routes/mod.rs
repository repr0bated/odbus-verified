//! API routes and route handlers

use axum::{
    extract::Extension,
    routing::{delete, get, post},
    Router,
};
use std::sync::Arc;
use tower_http::compression::CompressionLayer;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

use crate::groups_admin;
use crate::handlers;
use crate::middleware::security;
use crate::sse;
use crate::state::AppState;
use crate::websocket;

pub mod admin;
#[allow(dead_code)]
pub mod chat;
#[allow(dead_code)]
pub mod llm;

/// Create the complete router with all routes
pub fn create_router(state: Arc<AppState>) -> Router {
    // CORS configuration
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // API routes
    let api_routes = Router::new()
        // Health & Status
        .route("/health", get(handlers::health::health_handler))
        .route("/status", get(handlers::status::status_handler))
        // Schema — single source of truth from the sealed blob catalog
        .route("/schema", get(handlers::schema::schema_handler))
        // Compatibility paths used by dashboard builds predating `/api/schema`.
        .route("/schema/catalog", get(handlers::schema::schema_handler))
        .route(
            "/schema/catalog/detail",
            get(handlers::schema::schema_catalog_handler),
        )
        // Identity sled compatibility route — one projected session per request
        .route(
            "/identity/sled",
            get(handlers::identity::identity_sled_handler),
        )
        // SID1-aware client injection: a WireGuard arrival's own credential,
        // resolved by TCP source address (OP_WEB_HUMAN_PEERS), for the 3tched UI
        .route(
            "/identity/self",
            get(handlers::identity_self::identity_self_handler),
        )
        // Dashboard
        .route(
            "/dashboard/metrics",
            get(handlers::dashboard::dashboard_metrics_handler),
        )
        .route(
            "/dashboard/projections",
            get(handlers::dashboard::dashboard_projections_handler),
        )
        // Users
        .route("/users", get(handlers::users::list_users_handler))
        .route("/users/:id", get(handlers::users::get_user_handler))
        // VPN
        .route("/vpn/status", get(handlers::vpn::vpn_status_handler))
        .route(
            "/vpn/connections",
            get(handlers::vpn::vpn_connections_handler),
        )
        .route("/vpn/config", get(handlers::vpn::vpn_config_handler))
        // Mail
        .route("/mail/status", get(handlers::mail::mail_status_handler))
        .route("/mail/queue", get(handlers::mail::mail_queue_handler))
        .route("/mail/accounts", get(handlers::mail::mail_accounts_handler))
        // Logs
        .route("/logs", get(handlers::logs::logs_handler))
        .route("/logs/stream", get(handlers::logs::logs_stream_handler))
        // Chat endpoints
        .route(
            "/chat",
            post(handlers::tched_router::tched_router_chat_handler),
        )
        .route(
            "/chat/stream",
            post(handlers::tched_router::tched_router_chat_stream_handler),
        )
        .route(
            "/tched-router/chat",
            post(handlers::tched_router::tched_router_chat_handler),
        )
        .route(
            "/tched-router/chat/stream",
            post(handlers::tched_router::tched_router_chat_stream_handler),
        )
        .route(
            "/tched-router/schema",
            get(handlers::tched_router::tched_router_schema_handler),
        )
        .route(
            "/tched_router/schema",
            get(handlers::tched_router::tched_router_schema_handler),
        )
        // Plugin identity endpoints — model-agnostic, sealed SHM blob catalog
        // (`/dev/shm/opdbus/`) is the single source of truth for both.
        .route(
            "/plugins",
            get(handlers::plugin_schema::plugin_list_handler),
        )
        .route(
            "/plugin-schema/:plugin_id",
            get(handlers::plugin_schema::plugin_schema_handler),
        )
        // UI-model spec gallery + catalog (render slices of the sealed blob
        // PluginSchema; promote a winning lens into the catalog). Model-agnostic
        // by name: whichever model generates the spec, the surface is the same.
        .route(
            "/ui-model/gallery",
            get(handlers::ui_model::ui_model_gallery_handler),
        )
        .route(
            "/ui-model/gallery/:id",
            delete(handlers::ui_model::ui_model_gallery_delete_handler),
        )
        .route(
            "/ui-model/catalog",
            get(handlers::ui_model::ui_model_catalog_handler),
        )
        .route(
            "/ui-model/catalog/promote/:id",
            post(handlers::ui_model::ui_model_catalog_promote_handler),
        )
        .route(
            "/ui-model/catalog/:id",
            delete(handlers::ui_model::ui_model_catalog_delete_handler),
        )
        .route(
            "/ui-model/plugin-schema/:plugin",
            get(handlers::ui_model::ui_model_plugin_schema_handler),
        )
        .route(
            "/ui-model/plugins",
            get(handlers::ui_model::ui_model_list_plugins_handler),
        )
        .route("/chat/sessions", get(handlers::chat::list_sessions_handler))
        .route(
            "/chat/sessions",
            post(handlers::chat::create_session_handler),
        )
        .route(
            "/chat/sessions/:id",
            delete(handlers::chat::delete_session_handler),
        )
        .route("/chat/message", post(handlers::chat::send_message_handler))
        .route(
            "/chat/history/:session_id",
            get(handlers::chat::get_history_handler),
        )
        .route(
            "/chat/transcript",
            post(handlers::chat::save_transcript_handler),
        )
        .route(
            "/chat/system-prompt",
            get(handlers::chat::get_system_prompt_handler),
        )
        .route(
            "/chat/system-prompt",
            axum::routing::put(handlers::chat::update_system_prompt_handler),
        )
        // Analytics / Accountability
        .route(
            "/analytics/semantic-search",
            get(handlers::analytics::semantic_search_handler),
        )
        // Tool endpoints
        .route("/tools", get(handlers::tools::list_tools_handler))
        .route("/tools/:name", get(handlers::tools::get_tool_handler))
        .route("/tool", post(handlers::tools::execute_tool_handler))
        .route(
            "/tools/:name/execute",
            post(handlers::tools::execute_named_tool_handler),
        )
        // Agent endpoints
        .route("/agents", get(handlers::agents::list_agents_handler))
        .route("/agents", post(handlers::agents::spawn_agent_handler))
        .route(
            "/agents/types",
            get(handlers::agents::list_agent_types_handler),
        )
        .route("/agents/:id", get(handlers::agents::get_agent_handler))
        .route(
            "/agents/:id/task",
            post(handlers::agents::agent_task_handler),
        )
        .route(
            "/agents/:id",
            axum::routing::delete(handlers::agents::kill_agent_handler),
        )
        // LLM endpoints
        .route("/llm/status", get(handlers::llm::llm_status_handler))
        .route("/llm/providers", get(handlers::llm::list_providers_handler))
        .route("/llm/models", get(llm::get_models))
        .route(
            "/llm/models/:provider",
            get(handlers::llm::list_models_for_provider_handler),
        )
        .route("/llm/model", post(handlers::llm::switch_model_handler))
        .route(
            "/llm/chat",
            post(handlers::tched_router::tched_router_chat_handler),
        )
        // OpenClaw endpoints (internal/base layer)
        .route(
            "/openclaw/status",
            get(handlers::openclaw::openclaw_status_handler),
        )
        .route(
            "/openclaw/config",
            get(handlers::openclaw::openclaw_config_handler),
        )
        .route(
            "/openclaw/chat",
            post(handlers::tched_router::tched_router_chat_handler),
        )
        .route(
            "/openclaw/models",
            get(handlers::openclaw::openclaw_models_handler),
        )
        // Assistant endpoints (user-facing aliases — same handlers, branded URLs)
        .route(
            "/assistant/status",
            get(handlers::openclaw::openclaw_status_handler),
        )
        .route(
            "/assistant/config",
            get(handlers::openclaw::openclaw_config_handler),
        )
        .route(
            "/assistant/chat",
            post(handlers::tched_router::tched_router_chat_handler),
        )
        .route(
            "/assistant/models",
            get(handlers::openclaw::openclaw_models_handler),
        )
        // SSE events
        .route("/events", get(sse::sse_handler))
        // Privacy router endpoints
        .route("/privacy/signup", post(handlers::privacy::signup))
        .route("/privacy/verify", get(handlers::privacy::verify))
        .route(
            "/privacy/config/:user_id",
            get(handlers::privacy::get_config),
        )
        .route("/privacy/status", get(handlers::privacy::status))
        .route(
            "/privacy/credentials",
            post(handlers::privacy::set_credentials),
        )
        // Google OAuth endpoints
        .route("/privacy/google/auth", get(handlers::privacy::google_auth))
        .route(
            "/privacy/google/callback",
            get(handlers::privacy::google_callback),
        );

    // WebSocket route
    let ws_route = Router::new().route("/ws", get(websocket::websocket_handler));

    // Main dashboard/REST router. MCP and gRPC are served only by the
    // authenticated bridge on TLS :8090.
    let router = Router::new()
        .nest("/api", api_routes)
        // Ordinary HTTP compatibility lives on op-web :8080. Each handler is
        // an adapter to a schema-declared bridge method on gRPC :8090.
        .route(
            "/v1/models",
            get(handlers::tched_router::openai_models_handler),
        )
        .route(
            "/v1/chat/completions",
            post(handlers::tched_router::tched_router_chat_handler),
        )
        // Human-facing privacy verification flow (magic-link target)
        .route("/privacy/verify", get(handlers::privacy::verify_redirect))
        .route(
            "/privacy/access",
            get(handlers::privacy::privacy_access_message),
        )
        .merge(ws_route)
        .nest("/groups-admin", groups_admin::create_groups_admin_router())
        .nest("/admin", admin::admin_routes());

    // Serve the pinned Lovable dashboard while keeping `/api` and websocket
    // routes on this origin. Client-side routes fall back to index.html.
    let static_dir = std::env::var("OP_WEB_STATIC_DIR")
        .unwrap_or_else(|_| "/usr/local/share/op-dbus/dashboard".to_string());
    let spa =
        ServeDir::new(&static_dir).fallback(ServeFile::new(format!("{static_dir}/index.html")));

    router
        .fallback_service(spa)
        .layer(Extension(state))
        .layer(axum::middleware::from_fn(security::ip_security_middleware))
        .layer(axum::middleware::from_fn(
            crate::middleware::access_log::access_log_middleware,
        ))
        .layer(axum::middleware::from_fn(
            crate::middleware::spa_cache::spa_cache_middleware,
        ))
        .layer(cors)
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http())
}
