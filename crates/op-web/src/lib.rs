//! op-web: Unified Web Server for op-dbus-v2
//!
//! This crate consolidates ALL HTTP services into a single server:
//! - REST API for tools, agents, chat, status
//! - WebSocket for real-time chat
//! - SSE for streaming events
//! - Static file serving for web UI
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      op-web Server (:8080)                       │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  /api/health     - Health check                                 │
//! │  /api/status     - System status                                │
//! │  /api/tools      - Tool registry                                │
//! │  /api/agents     - Agent management                             │
//! │  /api/chat       - Chat API                                     │
//! │  /api/events     - SSE event stream                             │
//! │  /ws             - WebSocket chat                               │
//! │  /               - Static files (WASM frontend)                 │
//! └─────────────────────────────────────────────────────────────────┘
//! ```

/// Select rustls's aws-lc-rs provider for the whole process, as the bridge
/// binary does. tonic's TLS also compiles in ring, so rustls cannot choose a
/// provider itself and panics at the first TLS client or server setup.
pub fn install_tls_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

pub mod chat_store;
pub mod door_proxy;
pub mod email;
pub mod groups_admin;
pub mod handlers;
pub mod middleware;
pub mod orchestrator;
pub mod privacy_container;
pub mod privacy_network;
pub mod privacy_openflow;
pub mod privacy_routes;
pub mod routes;
pub mod sse;
pub mod state;
pub mod state_manager_client;
pub mod state_tree;
pub mod tched_router_routes;
pub mod users;
pub mod websocket;
pub mod wireguard;

pub use orchestrator::UnifiedOrchestrator;
pub use state::AppState;

/// Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub static_dir: Option<String>,
    pub enable_cors: bool,
    pub enable_compression: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8080,
            static_dir: Some("static".to_string()),
            enable_cors: true,
            enable_compression: true,
        }
    }
}
