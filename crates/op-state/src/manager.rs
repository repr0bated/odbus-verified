//! State manager for coordinating plugins and schemas

use crate::plugin::{ApplyResult, StateDiff, StatePlugin};
use anyhow::{anyhow, Result};
use op_state_store::{SchemaCatalog, SchemaRegistry, StateStore};
use parking_lot::RwLock;
use simd_json::OwnedValue as Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Global state manager
pub struct StateManager {
    plugins: Arc<RwLock<HashMap<String, Arc<dyn StatePlugin>>>>,
    #[allow(dead_code)]
    store: Option<Arc<dyn StateStore>>,
    schema_catalog: Arc<RwLock<SchemaCatalog>>,
    /// Broadcast sender for watch() method
    watch_tx: Option<Arc<tokio::sync::broadcast::Sender<PluginEvent>>>,
}

/// Plugin event for broadcast
#[derive(Debug, Clone)]
pub struct PluginEvent {
    pub plugin_id: String,
    pub operation: PluginOperation,
}

/// Plugin operation type
#[derive(Debug, Clone)]
pub enum PluginOperation {
    Register,
    Deregister,
    Update,
}

impl Default for StateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl StateManager {
    /// Create a new state manager
    pub fn new() -> Self {
        let (watch_tx, _) = tokio::sync::broadcast::channel(100);
        Self {
            plugins: Arc::new(RwLock::new(HashMap::new())),
            store: None,
            schema_catalog: Arc::new(RwLock::new(SchemaCatalog::new())),
            watch_tx: Some(Arc::new(watch_tx)),
        }
    }

    /// Preferred constructor: create with a specific schema catalog.
    pub fn with_schema_catalog(schema_catalog: Arc<RwLock<SchemaCatalog>>) -> Self {
        let (watch_tx, _) = tokio::sync::broadcast::channel(100);
        Self {
            plugins: Arc::new(RwLock::new(HashMap::new())),
            store: None,
            schema_catalog,
            watch_tx: Some(Arc::new(watch_tx)),
        }
    }

    /// Compatibility constructor for older call sites that still pass the
    /// catalog under the `schema_registry` name.
    pub fn with_schema_registry(schema_registry: Arc<RwLock<SchemaRegistry>>) -> Self {
        Self::with_schema_catalog(schema_registry)
    }

    /// Register a plugin
    pub fn register_plugin(&self, name: String, plugin: Arc<dyn StatePlugin>) {
        self.plugins.write().insert(name.clone(), plugin);

        // Fire watch broadcast
        if let Some(tx) = &self.watch_tx {
            let _ = tx.send(PluginEvent {
                plugin_id: name,
                operation: PluginOperation::Register,
            });
        }
    }

    /// Deregister a plugin
    pub fn deregister_plugin(&self, name: &str) -> bool {
        let removed = self.plugins.write().remove(name).is_some();

        // Fire watch broadcast
        if removed {
            if let Some(tx) = &self.watch_tx {
                let _ = tx.send(PluginEvent {
                    plugin_id: name.to_string(),
                    operation: PluginOperation::Deregister,
                });
            }
        }
        removed
    }

    /// Get a plugin by name
    pub fn get_plugin(&self, name: &str) -> Option<Arc<dyn StatePlugin>> {
        self.plugins.read().get(name).cloned()
    }

    /// Watch for plugin state changes
    pub fn watch(&self) -> Option<tokio::sync::broadcast::Receiver<PluginEvent>> {
        self.watch_tx.as_ref().map(|tx| tx.subscribe())
    }

    /// List all registered plugins
    pub fn list_plugins(&self) -> Vec<String> {
        self.plugins.read().keys().cloned().collect()
    }

    /// Compatibility accessor. Architecturally this is the schema catalog used
    /// for lookup and validation, not a second source of truth.
    pub fn schema_registry(&self) -> Arc<RwLock<SchemaRegistry>> {
        self.schema_catalog.clone()
    }

    pub fn schema_catalog(&self) -> Arc<RwLock<SchemaCatalog>> {
        self.schema_catalog.clone()
    }

    /// Validate a desired plugin state against the authoritative schema catalog.
    pub fn validate_plugin_state(&self, plugin_name: &str, desired: &Value) -> Result<()> {
        let validation = self
            .schema_catalog
            .read()
            .validate(plugin_name, desired)
            .ok_or_else(|| anyhow!("Schema '{}' not found in schema catalog", plugin_name))?;

        if validation.valid {
            return Ok(());
        }

        Err(anyhow!(
            "State rejected by schema '{}': {}",
            plugin_name,
            validation.errors.join("; ")
        ))
    }

    /// Apply a full desired state document for one plugin.
    pub async fn apply_plugin_state(
        &self,
        plugin_name: &str,
        desired: Value,
    ) -> Result<ApplyResult> {
        self.validate_plugin_state(plugin_name, &desired)?;

        let plugin = self
            .get_plugin(plugin_name)
            .ok_or_else(|| anyhow!("Plugin '{}' not found", plugin_name))?;
        let current = simd_json::json!(null);
        let diff = plugin.calculate_diff(&current, &desired).await?;

        plugin.apply_state(&diff).await
    }

    /// Apply state to a plugin
    pub async fn apply_state(&self, diff: StateDiff) -> Result<ApplyResult> {
        let plugin = self
            .get_plugin(&diff.plugin)
            .ok_or_else(|| anyhow!("Plugin '{}' not found", diff.plugin))?;
        plugin.apply_state(&diff).await
    }
}
