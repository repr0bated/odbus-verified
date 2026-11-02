//! Method router: `(plugin_id, method)` -> synchronous handler.
//!
//! Handlers are pure value-in/value-out functions over JSON values. Async or
//! D-Bus-backed behavior plugs in behind [`BridgeCore`](crate::core::BridgeCore)
//! call sites, not inside the router.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// A synchronous value-in/value-out method handler.
#[derive(Clone)]
pub struct Handler(
    Arc<dyn Fn(&[serde_json::Value]) -> Result<serde_json::Value, RouterError> + Send + Sync>,
);

impl Handler {
    /// Wrap a plain closure as a handler.
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(&[serde_json::Value]) -> Result<serde_json::Value, RouterError>
            + Send
            + Sync
            + 'static,
    {
        Self(Arc::new(f))
    }

    /// Run the handler.
    pub fn call(&self, args: &[serde_json::Value]) -> Result<serde_json::Value, RouterError> {
        (self.0)(args)
    }
}

impl<F> From<F> for Handler
where
    F: Fn(&[serde_json::Value]) -> Result<serde_json::Value, RouterError> + Send + Sync + 'static,
{
    fn from(f: F) -> Self {
        Self::new(f)
    }
}

impl std::fmt::Debug for Handler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Handler(..)")
    }
}

/// Router failure: unknown route, or a handler that returned an error.
#[derive(Debug, thiserror::Error)]
pub enum RouterError {
    /// No route table entry for this plugin at all.
    #[error("unknown plugin: {0}")]
    UnknownPlugin(String),
    /// Plugin is routed, but this method is not.
    #[error("unknown method '{method}' for plugin '{plugin_id}'")]
    UnknownMethod { plugin_id: String, method: String },
    /// The handler ran and failed.
    #[error("handler for '{plugin_id}.{method}' failed: {message}")]
    HandlerFailed {
        plugin_id: String,
        method: String,
        message: String,
    },
}

/// Synchronous `(plugin_id, method)` dispatch table. Cheap to clone.
#[derive(Clone, Debug, Default)]
pub struct Router {
    routes: Arc<RwLock<HashMap<String, HashMap<String, Handler>>>>,
}

impl Router {
    /// An empty router.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) the handler for one route.
    pub fn register(
        &self,
        plugin_id: impl Into<String>,
        method: impl Into<String>,
        handler: Handler,
    ) {
        self.routes
            .write()
            .expect("router lock poisoned")
            .entry(plugin_id.into())
            .or_default()
            .insert(method.into(), handler);
    }

    /// Is a handler registered for this route?
    pub fn has(&self, plugin_id: &str, method: &str) -> bool {
        self.routes
            .read()
            .expect("router lock poisoned")
            .get(plugin_id)
            .is_some_and(|methods| methods.contains_key(method))
    }

    /// Dispatch one call. The table lock is released before the handler runs.
    pub fn call(
        &self,
        plugin_id: &str,
        method: &str,
        args: &[serde_json::Value],
    ) -> Result<serde_json::Value, RouterError> {
        let handler = {
            let routes = self.routes.read().expect("router lock poisoned");
            let methods = routes
                .get(plugin_id)
                .ok_or_else(|| RouterError::UnknownPlugin(plugin_id.to_string()))?;
            methods
                .get(method)
                .cloned()
                .ok_or_else(|| RouterError::UnknownMethod {
                    plugin_id: plugin_id.to_string(),
                    method: method.to_string(),
                })?
        };
        handler.call(args).map_err(|err| match err {
            // A handler that returns routing errors is confused; normalize.
            RouterError::HandlerFailed { .. } => err,
            other => RouterError::HandlerFailed {
                plugin_id: plugin_id.to_string(),
                method: method.to_string(),
                message: other.to_string(),
            },
        })
    }
}
