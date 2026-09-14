//! Runtime plugin catalog.
//!
//! The catalog indexes live plugin instances and mirrors plugin-owned
//! `PluginSchema` documents into the shared schema catalog. Runtime truth stays
//! with the plugin schema; persisted catalog documents are compatibility
//! snapshots for consumers that still hydrate from disk.

use anyhow::Result;
use op_core::state_publisher::{ChangeType, StatePublisher};
use op_dbus_model::{CatalogDocument, PluginCatalog};
use op_state::StatePlugin;
use op_state_store::SchemaCatalog;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock as AsyncRwLock;
use tracing::{debug, warn};

#[derive(Clone)]
pub struct PluginRecord {
    pub name: String,
    pub plugin: Arc<dyn StatePlugin>,
    pub storage_path: PathBuf,
    pub dbus_path: String,
}

pub struct PluginRegistry {
    plugins: AsyncRwLock<HashMap<String, PluginRecord>>,
    base_path: PathBuf,
    schema_catalog: Arc<RwLock<SchemaCatalog>>,
    schema_catalog_store: Option<Arc<PluginCatalog>>,
    publisher: AsyncRwLock<Option<Arc<dyn StatePublisher>>>,
    dbus_connection: AsyncRwLock<Option<zbus::Connection>>,
}

impl PluginRegistry {
    pub fn new(base_path: impl AsRef<Path>) -> Self {
        Self::with_schema_catalog_and_store(
            base_path,
            Arc::new(RwLock::new(SchemaCatalog::empty())),
            None,
        )
    }

    pub fn with_schema_catalog_and_store(
        base_path: impl AsRef<Path>,
        schema_catalog: Arc<RwLock<SchemaCatalog>>,
        schema_catalog_store: Option<Arc<PluginCatalog>>,
    ) -> Self {
        Self {
            plugins: AsyncRwLock::new(HashMap::new()),
            base_path: base_path.as_ref().to_path_buf(),
            schema_catalog,
            schema_catalog_store,
            publisher: AsyncRwLock::new(None),
            dbus_connection: AsyncRwLock::new(None),
        }
    }

    pub async fn set_publisher(&self, publisher: Arc<dyn StatePublisher>) {
        *self.publisher.write().await = Some(publisher);
    }

    pub async fn set_dbus_connection(&self, connection: zbus::Connection) {
        *self.dbus_connection.write().await = Some(connection);
    }

    pub async fn hydrate_catalog_from_store(&self) -> Result<()> {
        let Some(store) = &self.schema_catalog_store else {
            return Ok(());
        };

        for document in store.list_documents().await? {
            self.schema_catalog.write().register(document.schema);
        }

        Ok(())
    }

    pub async fn register(&self, plugin: Arc<dyn StatePlugin>) -> Result<()> {
        let name = plugin.name().to_string();
        let storage_path = self.plugin_storage_path(&name);
        tokio::fs::create_dir_all(&storage_path).await?;

        let dbus_path = Self::plugin_dbus_path(&name);

        if let Some(schema) = plugin.schema() {
            self.schema_catalog.write().register(schema.clone());

            if let Some(store) = &self.schema_catalog_store {
                let document = CatalogDocument {
                    schema: schema.clone(),
                    dbus_path: dbus_path.clone(),
                    service_name: crate::canonical::BASE_SERVICE_NAME.to_string(),
                    storage_path: storage_path.to_string_lossy().into_owned(),
                    source: "plugin".to_string(),
                };
                store.upsert_document(&document).await?;
            }

            if let Some(publisher) = &*self.publisher.read().await {
                let _ = publisher
                    .publish_change(
                        name.clone(),
                        format!("schema/{}", name),
                        ChangeType::PropertySet,
                        Some("definition".to_string()),
                        None,
                        schema.to_json_schema(),
                        vec!["schema".to_string(), "plugin".to_string()],
                        "PluginSchema".to_string(),
                    )
                    .await;
            }
        } else {
            warn!(
                "Plugin {} has no PluginSchema; it will not enter the schema catalog",
                name
            );
        }

        if let Some(connection) = &*self.dbus_connection.read().await {
            let host = op_state::dbus_server::PluginDbusHost {
                plugin: plugin.clone(),
                schema_registry: self.schema_catalog.clone(),
            };
            if let Err(error) = connection
                .object_server()
                .at(dbus_path.as_str(), host)
                .await
            {
                debug!("Plugin {} D-Bus host export skipped: {}", name, error);
            }
        }

        self.plugins.write().await.insert(
            name.clone(),
            PluginRecord {
                name,
                plugin,
                storage_path,
                dbus_path,
            },
        );

        Ok(())
    }

    pub async fn get(&self, name: &str) -> Option<Arc<dyn StatePlugin>> {
        self.plugins
            .read()
            .await
            .get(name)
            .map(|record| record.plugin.clone())
    }

    pub async fn records(&self) -> Vec<PluginRecord> {
        self.plugins.read().await.values().cloned().collect()
    }

    fn plugin_storage_path(&self, name: &str) -> PathBuf {
        self.base_path.join(Self::sanitize_path_segment(name))
    }

    fn plugin_dbus_path(name: &str) -> String {
        crate::canonical::plugin_path(name)
    }

    fn sanitize_path_segment(segment: &str) -> String {
        let mut out = String::with_capacity(segment.len());
        for ch in segment.chars() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                out.push(ch);
            } else {
                out.push('_');
            }
        }

        if out.is_empty() {
            "_".to_string()
        } else {
            out
        }
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new("/var/lib/op-dbus/plugins")
    }
}

#[cfg(test)]
mod tests {
    use super::PluginRegistry;

    #[test]
    fn should_generate_only_canonical_plugin_paths() {
        assert_eq!(
            PluginRegistry::plugin_dbus_path("cognitive-mcp"),
            "/org/opdbus/v1/plugins/cognitive_mcp"
        );
    }
}
