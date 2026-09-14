use chrono::{DateTime, Utc};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plugin {
    pub name: String,
    pub service_name: String,
    pub base_object: simd_json::OwnedValue,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    pub id: String,
    pub plugin_name: String,
    pub definition: simd_json::OwnedValue,
    pub discovered_from: Option<String>,
    pub discovered_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Canonical persisted plugin document.
///
/// Architectural rule:
/// - The plugin defines the schema.
/// - That same schema is the footprint and JSON render contract.
/// - This document is the persisted authority that projection layers mirror.
///
/// The document stays intentionally small. We do not create separate runtime
/// "schema", "footprint", or "render" authorities here because that would
/// reintroduce the drift this refactor is removing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCatalogDocument {
    /// Canonical plugin-owned schema. This is the thing every downstream
    /// consumer ultimately resolves.
    pub schema: PluginSchema,
    /// Stable D-Bus projection path for the plugin.
    pub dbus_path: String,
    /// Service identity used by external projections and compatibility layers.
    pub service_name: String,
    /// Durable storage path allocated to the plugin instance.
    pub storage_path: String,
    /// Origin marker for diagnostics; runtime plugin registration should use
    /// `"plugin"` rather than inventing a second authority.
    pub source: String,
}
