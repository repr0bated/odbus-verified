pub mod models;

use models::PluginCatalogDocument;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, info};

pub use models::{Plugin, PluginCatalogDocument as CatalogDocument, Schema};

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

#[derive(Clone, Default)]
pub struct PluginCatalog {
    documents: Arc<RwLock<HashMap<String, PluginCatalogDocument>>>,
}

impl PluginCatalog {
    pub fn new() -> Self {
        debug!("Creating in-memory PluginCatalog");
        Self::default()
    }

    #[tracing::instrument(skip(self, document))]
    pub async fn upsert_document(
        &self,
        document: &PluginCatalogDocument,
    ) -> std::result::Result<(), ModelError> {
        let plugin_name = document.schema.name.clone();
        self.documents
            .write()
            .expect("plugin catalog lock should not be poisoned")
            .insert(plugin_name.clone(), document.clone());
        info!(%plugin_name, "Plugin catalog document mirrored in memory");
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub async fn get_document(
        &self,
        name: &str,
    ) -> std::result::Result<Option<PluginCatalogDocument>, ModelError> {
        Ok(self
            .documents
            .read()
            .expect("plugin catalog lock should not be poisoned")
            .get(name)
            .cloned())
    }

    #[tracing::instrument(skip(self))]
    pub async fn list_documents(
        &self,
    ) -> std::result::Result<Vec<PluginCatalogDocument>, ModelError> {
        Ok(self
            .documents
            .read()
            .expect("plugin catalog lock should not be poisoned")
            .values()
            .cloned()
            .collect())
    }
}
