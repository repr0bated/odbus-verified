//! op-http: Central HTTP/TLS Server
//!
//! This is the SINGLE source of truth for all HTTP/TLS handling in op-dbus.
//! All other crates export routers that get composed here.
//!
//! Architecture:
//! ```text
//! unified-server binary
//!     └── op-http (this crate)
//!         ├── TLS termination (rustls)
//!         ├── Middleware stack (CORS, tracing, compression)
//!         └── Router composition
//!             ├── /api/mcp/*    → op_mcp::create_router()
//!             ├── /api/chat/*   → op_chat::create_router()
//!             ├── /api/web/*    → op_web::create_router()
//!             ├── /api/tools/*  → op_tools::create_router()
//!             ├── /api/agents/* → op_agents::create_router()
//!             ├── /ws/*         → websocket handlers
//!             └── /*            → static files
//! ```

pub mod middleware;
pub mod router;
pub mod server;
pub mod tls;

// Re-export main types
pub use middleware::{MiddlewareConfig, MiddlewareStack};
pub use router::{RouterBuilder, ServiceRouter};
pub use server::{HttpServer, HttpServerBuilder, ServerConfig};
pub use tls::{TlsConfig, TlsMode};

// Re-export axum for convenience - other crates use this
pub use axum;
pub use tower;
pub use tower_http;

/// Error types for the HTTP server
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("TLS configuration error: {0}")]
    TlsError(String),

    #[error("Server binding error: {0}")]
    BindError(#[from] std::io::Error),

    #[error("Router configuration error: {0}")]
    RouterError(String),

    #[error("Certificate error: {0}")]
    CertificateError(String),
}

pub type Result<T> = std::result::Result<T, ServerError>;

/// Prelude for convenient imports by other crates
pub mod prelude {
    pub use super::axum::{
        extract::{Json, Path, Query, State},
        response::{IntoResponse, Response},
        routing::{delete, get, post, put},
        Router,
    };
    pub use super::middleware::{MiddlewareConfig, MiddlewareStack};
    pub use super::router::{RouterBuilder, ServiceRouter};
    pub use super::server::{HttpServer, HttpServerBuilder, ServerConfig};
    pub use super::tls::{TlsConfig, TlsMode};
    pub use super::Result;
}
