//! Schema-Driven D-Bus Passthrough Integration
//!
//! Wires the `SchemaRouter` into the live `DbusPassthrough` gRPC service path.
//! When a request arrives with an empty `destination` field, the schema router
//! resolves the canonical D-Bus coordinates from the plugin_id extracted from
//! the path. The schema router validates that the method/property exists before
//! making the D-Bus call — unknown methods are rejected.
//!
//! This module provides:
//! 1. `resolve_from_schema_or_explicit()` — backward-compatible route resolution
//! 2. `SchemaPassthroughService` — wraps SchemaRouter for direct plugin_id calls

use std::sync::Arc;

use crate::schema_router::SchemaRouter;
use tonic::Status;
use tracing::debug;

/// Route resolution result.
///
/// Either the caller provided explicit D-Bus coordinates (legacy path),
/// or the route was resolved from the schema catalog via plugin_id.
#[derive(Debug, Clone)]
pub enum ResolvedRoute {
    /// Caller provided explicit D-Bus coordinates — use as-is.
    Explicit {
        destination: String,
        path: String,
        interface: String,
    },
    /// Route resolved from schema catalog via plugin_id extracted from path.
    SchemaResolved { plugin_id: String, sub_path: String },
}

/// Resolve D-Bus routing from either explicit coordinates or schema lookup.
///
/// Rules:
/// - If `destination` is non-empty → `Explicit` (backward compatible).
/// - If `destination` is empty and `path` starts with the canonical plugin
///   base path → extract plugin_id → `SchemaResolved`.
/// - Otherwise → `Explicit` with whatever was provided (will likely fail
///   downstream, which is correct behavior for invalid input).
pub fn resolve_from_schema_or_explicit(
    destination: &str,
    path: &str,
    interface: &str,
) -> ResolvedRoute {
    if !destination.is_empty() {
        return ResolvedRoute::Explicit {
            destination: destination.to_string(),
            path: path.to_string(),
            interface: interface.to_string(),
        };
    }

    // Try canonical path extraction using op_plugins::canonical
    if let Some(plugin_id) = op_plugins::canonical::extract_plugin_name(path) {
        let base = op_plugins::canonical::plugin_path(&plugin_id);
        let sub_path = if path.len() > base.len() {
            path[base.len()..].to_string()
        } else {
            String::new()
        };
        return ResolvedRoute::SchemaResolved {
            plugin_id,
            sub_path,
        };
    }

    // Fallback: treat as explicit with whatever was provided.
    ResolvedRoute::Explicit {
        destination: destination.to_string(),
        path: path.to_string(),
        interface: interface.to_string(),
    }
}

/// Schema-driven passthrough service.
///
/// Wraps the `SchemaRouter` and provides direct plugin_id-based calls that
/// validate against the schema before hitting D-Bus.
pub struct SchemaPassthroughService {
    router: Arc<SchemaRouter>,
}

impl SchemaPassthroughService {
    pub fn new(router: Arc<SchemaRouter>) -> Self {
        Self { router }
    }

    /// Call a method on a plugin by plugin_id.
    ///
    /// The schema router validates the method exists before making the D-Bus call.
    pub async fn call(
        &self,
        plugin_id: &str,
        method_name: &str,
        json_args: &str,
    ) -> Result<String, Status> {
        debug!(plugin_id, method_name, "schema passthrough: routing call");
        self.router
            .call_method(plugin_id, method_name, json_args)
            .await
            .map_err(Status::from)
    }

    /// Get a property from a plugin by plugin_id.
    ///
    /// The schema router validates the property exists before reading.
    pub async fn get_property(
        &self,
        plugin_id: &str,
        property_name: &str,
    ) -> Result<String, Status> {
        debug!(plugin_id, property_name, "schema passthrough: get property");
        self.router
            .get_property(plugin_id, property_name)
            .await
            .map_err(Status::from)
    }

    /// Set a property on a plugin by plugin_id.
    ///
    /// The schema router validates the property exists before writing.
    pub async fn set_property(
        &self,
        plugin_id: &str,
        property_name: &str,
        json_value: &str,
    ) -> Result<(), Status> {
        debug!(plugin_id, property_name, "schema passthrough: set property");
        self.router
            .set_property(plugin_id, property_name, json_value)
            .await
            .map_err(Status::from)
    }

    /// List all routable plugins from the schema catalog.
    pub async fn list_plugins(&self) -> Vec<String> {
        self.router.list_plugin_ids().await
    }

    /// Build the read-only `TchedRouterProjection` proto for the `"tched_router"`
    /// plugin from the in-memory `TchedRouterState` (never a `/dev/shm` re-read).
    /// Returns `None` for any other plugin name.
    /// subid: `exp.service.3tched-router-bridge.grpc-stream@v1`.
    pub fn tched_router_projection(plugin_name: &str) -> Option<crate::proto::TchedRouterProjection> {
        if plugin_name != "tched_router" {
            return None;
        }
        let state = op_plugins::state_plugins::tched_router::TchedRouterPlugin::current_state();
        let schema_json = serde_json::to_string(&state).unwrap_or_else(|_| "{}".to_string());
        let model_routes = state
            .catalog
            .model_routes
            .iter()
            .map(|r| crate::proto::ModelRouteProto {
                hint: r.hint.clone(),
                model: r.model.clone(),
                provider: r.provider.clone(),
                available: r.available,
                effort_level: r.effort_level.clone(),
                privacy_tier: r.privacy_tier.clone(),
                context_window: r.context_window,
            })
            .collect();
        let providers = state
            .catalog
            .providers
            .iter()
            .map(|p| crate::proto::ProviderProto {
                id: p.id.clone(),
                route: p.route.clone(),
                kind: p.kind.clone(),
            })
            .collect();
        Some(crate::proto::TchedRouterProjection {
            schema_json,
            model_routes,
            providers,
        })
    }

    /// Trigger a schema reload (called reactively on mutation, not polled).
    pub async fn reload(&self) {
        let _ = self.router.reload().await;
    }
}
