//! D-Bus interface for individual plugin objects.
//!
//! StateManagerDBus and the StateManager D-Bus surface have been excised.
//! Mutations go through the MutationEngine (gRPC, the single write door);
//! the projected tree reads 1:1 from shm. This module retains only
//! `PluginDbusHost`, the per-plugin D-Bus interface used by the plugin
//! registry to expose individual plugin properties on the bus.

use crate::plugin::StatePlugin;
use op_state_store::{SchemaCatalog, SchemaRegistry};
use parking_lot::RwLock;
use std::sync::Arc;

/// D-Bus interface for an individual plugin
pub struct PluginDbusHost {
    pub plugin: Arc<dyn StatePlugin>,
    /// Compatibility name kept on the host shape for older call sites. This is
    /// the shared schema catalog used to resolve the canonical plugin document.
    pub schema_registry: Arc<RwLock<SchemaRegistry>>,
}

#[zbus::interface(name = "org.opdbus.v1.PluginV1")]
impl PluginDbusHost {
    #[zbus(property)]
    async fn name(&self) -> String {
        self.plugin.name().to_string()
    }

    #[zbus(property)]
    async fn version(&self) -> String {
        self.plugin.version().to_string()
    }

    #[zbus(property)]
    async fn description(&self) -> String {
        self.plugin.metadata().description
    }

    async fn get_state(&self) -> zbus::fdo::Result<String> {
        Ok(simd_json::to_string(&simd_json::json!(null)).unwrap_or_default())
    }

    async fn get_schema(&self) -> zbus::fdo::Result<String> {
        let plugin_name = self.plugin.name();
        let catalog = self.schema_registry.read();
        let schema = catalog
            .get_copies(plugin_name)
            .map(|copies| copies.json_schema.clone())
            .ok_or_else(|| {
                zbus::fdo::Error::Failed(format!(
                    "Schema '{}' not found in shared catalog",
                    plugin_name
                ))
            })?;

        Ok(simd_json::to_string(&schema).unwrap_or_default())
    }
}

/// Preferred architectural name for `PluginDbusHost` schema lookup state.
pub type SharedSchemaCatalog = Arc<RwLock<SchemaCatalog>>;

/// Compatibility alias for older call sites that still say `registry`.
pub type SharedSchemaRegistry = SharedSchemaCatalog;
