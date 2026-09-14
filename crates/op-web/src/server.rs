//! Web server implementation

use axum::{routing::get, Router};
use std::net::SocketAddr;
use std::path::PathBuf;
use tower::ServiceBuilder;
use tower_governor::{governor::GovernorConfigBuilder, GovernorLayer};
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    services::ServeDir,
    trace::TraceLayer,
};
use tracing::info;

use crate::routes::api_router;
use crate::state::AppState;
use crate::websocket::ws_handler;

// Re-export for main.rs
pub use crate::websocket::ws_handler as websocket_handler;

/// Web server configuration
#[derive(Debug, Clone)]
pub struct WebServerConfig {
    /// Address to bind to
    pub addr: SocketAddr,
    /// Path to static files (optional)
    pub static_dir: Option<PathBuf>,
    /// Enable CORS
    pub cors_enabled: bool,
    /// Rate limiting configuration
    pub rate_limit: RateLimitConfig,
}

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Requests per minute per IP
    pub requests_per_minute: u64,
    /// Burst size (additional requests allowed)
    pub burst_size: u64,
    /// Enable rate limiting
    pub enabled: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: 100, // Matches Caddy config
            burst_size: 20,
            enabled: true,
        }
    }
}

impl Default for WebServerConfig {
    fn default() -> Self {
        Self {
            addr: SocketAddr::from(([127, 0, 0, 1], 3000)),
            static_dir: None,
            cors_enabled: true,
            rate_limit: Default::default(),
        }
    }
}

impl WebServerConfig {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            ..Default::default()
        }
    }

    pub fn with_static_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.static_dir = Some(path.into());
        self
    }
}

/// Web server
pub struct WebServer {
    config: WebServerConfig,
    state: AppState,
}

impl WebServer {
    /// Create a new web server
    pub fn new(config: WebServerConfig, state: AppState) -> Self {
        Self { config, state }
    }

    /// Build the router
    pub fn router(&self) -> Router {
        let mut app = Router::new()
            // API routes
            .nest("/api", api_router())
            // WebSocket endpoint for streaming
            .route("/ws", get(ws_handler))
            .with_state(self.state.clone());

        // Build middleware stack with consistent types
        let middleware = ServiceBuilder::new()
            .layer(TraceLayer::new_for_http())
            .layer(CompressionLayer::new()); // Automatic gzip/brotli compression

        let app = app.layer(middleware);

        // Add rate limiting if enabled (as separate layer)
        let app = if self.config.rate_limit.enabled {
            let rate_limit_per_sec = self.config.rate_limit.requests_per_minute / 60;
            info!(
                "Rate limiting enabled: {} requests/second, burst: {}",
                rate_limit_per_sec, self.config.rate_limit.burst_size
            );

            let governor_conf = GovernorConfigBuilder::default()
                .per_second(rate_limit_per_sec)
                .burst_size(self.config.rate_limit.burst_size as u32)
                .finish()
                .expect("rate limit configuration should be valid");

            app.layer(GovernorLayer {
                config: governor_conf.into(),
            })
        } else {
            app
        };

        // Add CORS if enabled (as separate layer)
        let app = if self.config.cors_enabled {
            let cors = CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any);
            app.layer(cors)
        } else {
            app
        };

        app
    }

    /// Run the server
    pub async fn run(self) -> Result<(), std::io::Error> {
        let router = self.router();
        let listener = tokio::net::TcpListener::bind(self.config.addr).await?;

        info!("Web server listening on http://{}", self.config.addr);

        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
    }
}
