//! Shared Middleware Stack
//!
//! Centralized middleware configuration applied to all routes.
//! This ensures consistent behavior across all HTTP endpoints.

use axum::{
    body::Body,
    http::Request,
    middleware::{self, Next},
    response::Response,
    Router,
};
use std::time::{Duration, Instant};
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    timeout::TimeoutLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::Level;

/// Middleware configuration
#[derive(Debug, Clone)]
pub struct MiddlewareConfig {
    /// Enable CORS (default: true)
    pub cors_enabled: bool,
    /// CORS allowed origins (None = any)
    pub cors_origins: Option<Vec<String>>,
    /// Enable request tracing (default: true)
    pub tracing_enabled: bool,
    /// Enable response compression (default: true)
    pub compression_enabled: bool,
    /// Request timeout (default: 30s)
    pub timeout: Duration,
    /// Enable security headers (default: true)
    pub security_headers: bool,
    /// Enable request logging (default: true)
    pub request_logging: bool,
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            cors_enabled: true,
            cors_origins: None,
            tracing_enabled: true,
            compression_enabled: true,
            timeout: Duration::from_secs(30),
            security_headers: true,
            request_logging: true,
        }
    }
}

impl MiddlewareConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cors(mut self, enabled: bool) -> Self {
        self.cors_enabled = enabled;
        self
    }

    pub fn cors_origins(mut self, origins: Vec<String>) -> Self {
        self.cors_origins = Some(origins);
        self
    }

    pub fn tracing(mut self, enabled: bool) -> Self {
        self.tracing_enabled = enabled;
        self
    }

    pub fn compression(mut self, enabled: bool) -> Self {
        self.compression_enabled = enabled;
        self
    }

    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = duration;
        self
    }

    pub fn security_headers(mut self, enabled: bool) -> Self {
        self.security_headers = enabled;
        self
    }

    pub fn request_logging(mut self, enabled: bool) -> Self {
        self.request_logging = enabled;
        self
    }
}

/// Middleware stack that can be applied to a router
pub struct MiddlewareStack {
    config: MiddlewareConfig,
}

impl MiddlewareStack {
    pub fn new(config: MiddlewareConfig) -> Self {
        Self { config }
    }

    pub fn default_stack() -> Self {
        Self::new(MiddlewareConfig::default())
    }

    /// Apply the middleware stack to a router
    pub fn apply(self, router: Router) -> Router {
        let mut router = router;

        // Security headers (innermost - runs first on response)
        if self.config.security_headers {
            router = router.layer(middleware::from_fn(security_headers_middleware));
        }

        // Request logging
        if self.config.request_logging {
            router = router.layer(middleware::from_fn(request_logging_middleware));
        }

        // Timeout
        router = router.layer(TimeoutLayer::new(self.config.timeout));

        // Compression
        if self.config.compression_enabled {
            router = router.layer(CompressionLayer::new());
        }

        // Tracing
        if self.config.tracing_enabled {
            router = router.layer(
                TraceLayer::new_for_http()
                    .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                    .on_response(DefaultOnResponse::new().level(Level::INFO)),
            );
        }

        // CORS (outermost - runs first on request)
        if self.config.cors_enabled {
            let cors = if let Some(ref origins) = self.config.cors_origins {
                // Specific origins
                let origins: Vec<_> = origins.iter().filter_map(|o| o.parse().ok()).collect();
                CorsLayer::new()
                    .allow_origin(origins)
                    .allow_methods(Any)
                    .allow_headers(Any)
            } else {
                // Any origin
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods(Any)
                    .allow_headers(Any)
            };
            router = router.layer(cors);
        }

        router
    }
}

/// Security headers middleware
async fn security_headers_middleware(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;

    let headers = response.headers_mut();
    headers.insert("X-Content-Type-Options", "nosniff".parse().unwrap());
    headers.insert("X-Frame-Options", "DENY".parse().unwrap());
    headers.insert("X-XSS-Protection", "1; mode=block".parse().unwrap());
    headers.insert(
        "Referrer-Policy",
        "strict-origin-when-cross-origin".parse().unwrap(),
    );
    headers.insert(
        "Permissions-Policy",
        "geolocation=(), microphone=(), camera=()".parse().unwrap(),
    );

    response
}

/// Request logging middleware
async fn request_logging_middleware(request: Request<Body>, next: Next) -> Response {
    let start = Instant::now();
    let method = request.method().clone();
    let uri = request.uri().clone();
    let version = request.version();

    let response = next.run(request).await;

    let duration = start.elapsed();
    let status = response.status();

    // Log at appropriate level based on status
    if status.is_server_error() {
        tracing::error!(
            "{} {} {:?} {} - {}ms",
            method,
            uri,
            version,
            status.as_u16(),
            duration.as_millis()
        );
    } else if status.is_client_error() {
        tracing::warn!(
            "{} {} {:?} {} - {}ms",
            method,
            uri,
            version,
            status.as_u16(),
            duration.as_millis()
        );
    } else {
        tracing::info!(
            "{} {} {:?} {} - {}ms",
            method,
            uri,
            version,
            status.as_u16(),
            duration.as_millis()
        );
    }

    response
}

/// Convenience function to apply default middleware
pub fn default_middleware_stack(router: Router) -> Router {
    MiddlewareStack::default_stack().apply(router)
}

/// Convenience function to apply middleware with config
pub fn apply_middleware(router: Router, config: MiddlewareConfig) -> Router {
    MiddlewareStack::new(config).apply(router)
}
