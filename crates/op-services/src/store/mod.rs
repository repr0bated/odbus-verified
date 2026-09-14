//! JSON flat-file service store.
//!
//! No SQLite, no drift. Desired state = file contents.
//! Every mutation rewrites the entire services file atomically (write+rename).
//! Audit log uses append-only JSON-lines for efficient logging.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::schema::{ServiceDef, ServiceName};

const DEFAULT_SERVICES_PATH: &str = "/var/lib/op-dbus/services.json";
const DEFAULT_AUDIT_PATH: &str = "/var/lib/op-dbus/services-audit.jsonl";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ServicesCatalog {
    services: HashMap<String, ServiceDef>,
}

/// In-memory projection of service definitions with atomic JSON persistence.
pub struct Store {
    services_path: PathBuf,
    audit_path: PathBuf,
    data: RwLock<ServicesCatalog>,
}

impl Store {
    pub async fn new(path: impl AsRef<Path>) -> Result<Self> {
        let services_path = path.as_ref().to_path_buf();
        let audit_path = services_path.with_extension("audit.jsonl");
        Self::with_paths(services_path, audit_path).await
    }

    pub async fn default_store() -> Result<Self> {
        Self::with_paths(DEFAULT_SERVICES_PATH.into(), DEFAULT_AUDIT_PATH.into()).await
    }

    async fn with_paths(services_path: PathBuf, audit_path: PathBuf) -> Result<Self> {
        let catalog = if services_path.exists() {
            match tokio::fs::read_to_string(&services_path).await {
                Ok(contents) => match serde_json::from_str::<ServicesCatalog>(&contents) {
                    Ok(c) => {
                        info!(services = c.services.len(), "Loaded services from JSON");
                        c
                    }
                    Err(e) => {
                        warn!(error = %e, "Corrupt services JSON, starting fresh");
                        ServicesCatalog::default()
                    }
                },
                Err(e) => {
                    warn!(error = %e, "Failed to read services JSON, starting fresh");
                    ServicesCatalog::default()
                }
            }
        } else {
            info!("No services file found, starting fresh");
            ServicesCatalog::default()
        };

        Ok(Self {
            services_path,
            audit_path,
            data: RwLock::new(catalog),
        })
    }

    pub async fn get_service(&self, name: &ServiceName) -> Result<Option<ServiceDef>> {
        let guard = self.data.read().await;
        Ok(guard.services.get(name.as_str()).cloned())
    }

    pub async fn save_service(&self, service: &ServiceDef) -> Result<()> {
        let mut guard = self.data.write().await;
        guard
            .services
            .insert(service.name.as_str().to_string(), service.clone());
        drop(guard);
        self.flush().await?;
        Ok(())
    }

    pub async fn delete_service(&self, name: &ServiceName) -> Result<()> {
        let mut guard = self.data.write().await;
        let removed = guard.services.remove(name.as_str()).is_some();
        drop(guard);
        if removed {
            self.flush().await?;
        }
        Ok(())
    }

    pub async fn list_services(&self) -> Result<Vec<ServiceDef>> {
        let guard = self.data.read().await;
        Ok(guard.services.values().cloned().collect())
    }

    pub async fn audit(
        &self,
        service: Option<&str>,
        action: &str,
        details: Option<&str>,
    ) -> Result<()> {
        let entry = serde_json::json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "service_name": service,
            "action": action,
            "details": details,
        });
        let line = format!("{}\n", serde_json::to_string(&entry)?);
        tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.audit_path)
            .await?
            .write_all(line.as_bytes())
            .await?;
        Ok(())
    }

    /// Atomic flush: write to temp file, then rename.
    async fn flush(&self) -> Result<()> {
        let guard = self.data.read().await;
        let json = serde_json::to_string_pretty(&*guard)?;
        drop(guard);

        let tmp = self.services_path.with_extension("tmp");
        tokio::fs::write(&tmp, json).await?;
        tokio::fs::rename(&tmp, &self.services_path).await?;

        debug!(path = %self.services_path.display(), "Flushed services to JSON");
        Ok(())
    }
}
