//! Router Composition
//!
//! Utilities for composing routers from multiple crates into a unified router.
//! Each crate implements ServiceRouter to expose its routes.

use axum::Router;
use std::path::PathBuf;
use tower_http::services::ServeDir;
use tracing::info;

/// Trait for crates that provide HTTP routes
///
/// Implement this trait in your crate to expose routes to the central server:
/// ```ignore
/// pub struct MyServiceRouter;
///
/// impl ServiceRouter for MyServiceRouter {
///     fn prefix() -> &'static str {
///         "/api/myservice"
///     }
///
///     fn name() -> &'static str {
///         "my-service"
///     }
/// }
///
/// // Then provide a create_router function:
/// pub fn create_router(state: MyState) -> Router {
///     Router::new()
///         .route("/health", get(health))
///         .route("/data", post(data))
///         .with_state(state)
/// }
/// ```
pub trait ServiceRouter: Send + Sync {
    /// The URL prefix for this service (e.g., "/api/mcp")
    fn prefix() -> &'static str;

    /// Service name for logging
    fn name() -> &'static str;

    /// Optional: service description
    fn description() -> &'static str {
        ""
    }
}

/// Builder for composing multiple service routers
pub struct RouterBuilder {
    router: Router,
    static_dir: Option<PathBuf>,
    services: Vec<(&'static str, &'static str)>, // (prefix, name)
}

impl RouterBuilder {
    /// Create a new router builder
    pub fn new() -> Self {
        Self {
            router: Router::new(),
            static_dir: None,
            services: Vec::new(),
        }
    }

    /// Add a router at a specific prefix
    pub fn nest(mut self, prefix: &'static str, name: &'static str, router: Router) -> Self {
        info!("Mounting service '{}' at {}", name, prefix);
        self.router = self.router.nest(prefix, router);
        self.services.push((prefix, name));
        self
    }

    /// Add a route directly to the root router
    pub fn route(mut self, path: &str, method_router: axum::routing::MethodRouter) -> Self {
        self.router = self.router.route(path, method_router);
        self
    }

    /// Merge another router (no prefix)
    pub fn merge(mut self, router: Router) -> Self {
        self.router = self.router.merge(router);
        self
    }

    /// Set static file directory (served at root, fallback)
    pub fn static_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.static_dir = Some(path.into());
        self
    }

    /// Get list of mounted services
    pub fn services(&self) -> &[(&'static str, &'static str)] {
        &self.services
    }

    /// Build the final router
    pub fn build(mut self) -> Router {
        // Add static file serving if configured (as fallback)
        if let Some(static_dir) = self.static_dir {
            if static_dir.exists() {
                info!("Serving static files from: {:?}", static_dir);
                self.router = self.router.fallback_service(ServeDir::new(static_dir));
            } else {
                tracing::warn!("Static directory not found: {:?}", static_dir);
            }
        }

        self.router
    }
}

impl Default for RouterBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to create a router builder
pub fn router() -> RouterBuilder {
    RouterBuilder::new()
}
