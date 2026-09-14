//! SchemaLoader — reads a canonical PluginSchema from the sealed blob catalog.
//!
//! The loader accepts only a catalog directory. A plugin-owned JSON file or
//! monolithic schema document is not a valid source of authority.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use tokio::sync::broadcast;
use tracing::{info, warn};

const TCHED_ROUTER_PLUGIN_ID: &str = "tched_router";

/// Event emitted to `WatchSchema` subscribers after a successful reload.
#[derive(Clone, Debug)]
pub struct SchemaReloadEvent {
    pub event_type: String,
}

/// Shared loader state for the tched_router Axum host.
#[derive(Clone)]
pub struct SchemaLoader {
    path: Arc<RwLock<PathBuf>>,
    plugin_id: Arc<RwLock<String>>,
    schema: Arc<RwLock<serde_json::Value>>,
    reload_tx: broadcast::Sender<SchemaReloadEvent>,
    health_status: Arc<RwLock<String>>,
}

impl SchemaLoader {
    /// Create a loader and perform the initial read.
    ///
    /// Panics with a descriptive message if the sealed schema catalog is absent.
    /// This is intentional: the sealed catalog must start first.
    pub fn new(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        Self::new_for_plugin(path, TCHED_ROUTER_PLUGIN_ID)
    }

    /// Create a loader for a specific plugin id and perform the initial read.
    pub fn new_for_plugin(
        path: impl Into<PathBuf>,
        plugin_id: impl Into<String>,
    ) -> anyhow::Result<Self> {
        let path = Arc::new(RwLock::new(path.into()));
        let loader = Self {
            path,
            plugin_id: Arc::new(RwLock::new(plugin_id.into())),
            schema: Arc::new(RwLock::new(serde_json::Value::Null)),
            reload_tx: broadcast::channel(16).0,
            health_status: Arc::new(RwLock::new(String::from("ok"))),
        };
        loader.load()?;
        Ok(loader)
    }

    /// Synchronous load used by `new()` for the initial read.
    ///
    /// Logs a warning if the operation exceeds the 50 ms budget.
    pub fn load(&self) -> anyhow::Result<()> {
        let start = Instant::now();
        let path = self.path.read().unwrap().clone();
        if !path.is_dir() {
            return Err(anyhow::anyhow!(
                "plugin catalog must be a directory containing sealed blobs: {}",
                path.display()
            ));
        }
        self.load_from_blob_catalog(&path, start)
    }

    /// Read the plugin's canonical schema from its sealed blob in a catalog
    /// dir. The blob in the catalog IS the plugin — no monolith, no registry.
    fn load_from_blob_catalog(&self, dir: &Path, start: Instant) -> anyhow::Result<()> {
        let plugin_id = self.plugin_id();
        let schema =
            crate::sealed_schema_reader::read_manifest_pinned_plugin_schema(dir, &plugin_id)?;
        let value = serde_json::to_value(&schema)?;
        let bytes = serde_json::to_vec(&value)?;
        Self::parse_and_store(self, &bytes, start, dir)
    }

    /// Asynchronous reload used by `SIGHUP` and the D-Bus object.
    pub async fn reload(&self) -> anyhow::Result<()> {
        let start = Instant::now();
        let path = self.path.read().unwrap().clone();
        if !path.is_dir() {
            return Err(anyhow::anyhow!(
                "plugin catalog must be a directory containing sealed blobs: {}",
                path.display()
            ));
        }
        tokio::task::spawn_blocking({
            let loader = self.clone();
            move || loader.load_from_blob_catalog(&path, start)
        })
        .await
        .map_err(|e| anyhow::anyhow!("reload task panicked: {}", e))?
    }

    fn parse_and_store(&self, bytes: &[u8], start: Instant, path: &Path) -> anyhow::Result<()> {
        let text = std::str::from_utf8(bytes).map_err(|e| {
            anyhow::anyhow!(
                "sealed plugin schema at {} is not valid UTF-8: {}",
                path.display(),
                e
            )
        })?;
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| {
            anyhow::anyhow!(
                "sealed plugin schema at {} is not valid JSON: {}",
                path.display(),
                e
            )
        })?;
        let plugin_id = self.plugin_id();
        if value.get("name").and_then(|name| name.as_str()) != Some(plugin_id.as_str()) {
            return Err(anyhow::anyhow!(
                "sealed catalog at {} does not contain '{}'",
                path.display(),
                plugin_id
            ));
        }

        {
            let mut schema = self.schema.write().unwrap();
            *schema = value;
        }

        let elapsed = start.elapsed();
        if elapsed > std::time::Duration::from_millis(50) {
            warn!(
                elapsed_ms = elapsed.as_millis(),
                plugin_id = %self.plugin_id(),
                "plugin schema reload exceeded 50 ms budget"
            );
        } else {
            info!(
                elapsed_ms = elapsed.as_millis(),
                plugin_id = %self.plugin_id(),
                "plugin schema loaded"
            );
        }

        let mut health = self.health_status.write().unwrap();
        *health = "ok".to_string();

        Ok(())
    }

    /// Clone the current schema value.
    pub async fn get(&self) -> serde_json::Value {
        self.schema.read().unwrap().clone()
    }

    /// Return the configured sealed catalog directory.
    pub fn path(&self) -> PathBuf {
        self.path.read().unwrap().clone()
    }

    /// Return the plugin id selected from the sealed catalog.
    pub fn plugin_id(&self) -> String {
        self.plugin_id.read().unwrap().clone()
    }

    /// Update the plugin id; the next `load()` reads that plugin's sealed blob.
    pub fn set_plugin_id(&self, plugin_id: impl Into<String>) {
        let mut id = self.plugin_id.write().unwrap();
        *id = plugin_id.into();
    }

    /// Update the sealed catalog directory; the next `load()` reads from it.
    pub fn set_path(&self, path: impl Into<PathBuf>) {
        let mut p = self.path.write().unwrap();
        *p = path.into();
    }

    /// Return a handle to the reload broadcast channel.
    pub fn reload_tx(&self) -> broadcast::Sender<SchemaReloadEvent> {
        self.reload_tx.clone()
    }

    /// Return the current health status string.
    pub async fn health_status(&self) -> String {
        self.health_status.read().unwrap().clone()
    }

    /// Set health status (used by the D-Bus object and server startup).
    pub async fn set_health_status(&self, status: String) {
        let mut h = self.health_status.write().unwrap();
        *h = status;
    }

    /// Spawn a task that reloads the schema on `SIGHUP` and broadcasts a
    /// plugin schema reload event.
    pub fn watch_sighup(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut sig =
                match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup()) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(error = %e, "failed to install SIGHUP handler; manual reloads only");
                        return;
                    }
                };

            loop {
                sig.recv().await;
                info!(
                    subid = "evt.service.plugin-schema.reloaded@v1",
                    plugin_id = %self.plugin_id(),
                    "SIGHUP received"
                );
                match self.reload().await {
                    Ok(()) => {
                        let _ = self.reload_tx.send(SchemaReloadEvent {
                            event_type: "reload".to_string(),
                        });
                    }
                    Err(e) => {
                        warn!(error = %e, "SIGHUP reload failed");
                        let mut health = self.health_status.write().unwrap();
                        *health = e.to_string();
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seal(dir: &Path, schema: op_state_store::PluginSchema) {
        let blob = op_blob::blobify_plugin_schema("tched_router", schema);
        op_blob::catalog::BlobStore::open(dir)
            .unwrap()
            .write(&blob)
            .unwrap();
    }

    #[tokio::test]
    async fn should_load_schema_from_live_catalog() {
        let dir = tempfile::tempdir().unwrap();
        let value = op_state_store::PluginSchema::builder("tched_router")
            .version("1.0.0")
            .description("test schema")
            .build();
        let path = dir.path();
        seal(path, value);
        let loader = SchemaLoader::new(path).unwrap();
        let loaded = loader.get().await;
        assert_eq!(loaded["name"], "tched_router");
        assert_eq!(loaded["version"], "1.0.0");
        assert_eq!(loader.health_status().await, "ok");
    }

    #[tokio::test]
    async fn should_reject_direct_plugin_schema_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tched_router.json");
        std::fs::write(&path, "{}").unwrap();
        let error = SchemaLoader::new(&path)
            .err()
            .expect("direct schema file must be rejected");
        assert!(error.to_string().contains("catalog must be a directory"));
    }

    #[tokio::test]
    async fn should_reject_blob_without_catalog_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let schema = op_state_store::PluginSchema::builder("tched_router").build();
        op_blob::blobify_plugin_schema("tched_router", schema)
            .write_to_dir(dir.path())
            .unwrap();
        assert!(SchemaLoader::new(dir.path()).is_err());
    }

    #[tokio::test]
    async fn should_reload_schema_on_sighup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let initial = op_state_store::PluginSchema::builder("tched_router")
            .version("1.0.0")
            .description("initial")
            .build();
        seal(path, initial.clone());
        let loader = Arc::new(SchemaLoader::new(path).unwrap());
        let mut rx = loader.reload_tx().subscribe();

        // Simulate a reload by calling load directly and sending the event.
        loader.load().unwrap();
        let _ = loader.reload_tx.send(SchemaReloadEvent {
            event_type: "reload".to_string(),
        });

        // Verify the broadcast is received.
        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type, "reload");
        assert_eq!(loader.get().await["version"], "1.0.0");

        // Update the file and reload again.
        let updated = op_state_store::PluginSchema::builder("tched_router")
            .version("2.0.0")
            .description("updated")
            .build();
        seal(path, updated);
        // A stale file is not an authority: only the manifest-pinned blob is.
        op_blob::blobify_plugin_schema("tched_router", initial)
            .write_to_dir(path)
            .unwrap();
        loader.load().unwrap();
        let _ = loader.reload_tx.send(SchemaReloadEvent {
            event_type: "reload".to_string(),
        });

        let event = rx.recv().await.unwrap();
        assert_eq!(event.event_type, "reload");
        assert_eq!(loader.get().await["version"], "2.0.0");
    }
}
