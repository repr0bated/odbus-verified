//! Async access to a single shared Cozo-backed D-Bus index handle.

use crate::indexer::{DbusIndexer, IndexStatistics, SearchResult};
use anyhow::Result;
use op_core::types::BusType;
use std::path::PathBuf;
use std::sync::Arc;

pub struct IndexerManager {
    indexer: Arc<DbusIndexer>,
}

impl IndexerManager {
    pub async fn new(db_path: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self {
            indexer: Arc::new(DbusIndexer::new(db_path.into()).await?),
        })
    }

    pub async fn build_index(&self, bus_type: BusType) -> Result<IndexStatistics> {
        self.indexer.build_index(bus_type).await
    }

    pub async fn search_methods(&self, query: String, limit: usize) -> Result<Vec<SearchResult>> {
        let indexer = Arc::clone(&self.indexer);
        tokio::task::spawn_blocking(move || indexer.search_methods(&query, limit)).await?
    }

    pub async fn search_properties(
        &self,
        query: String,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let indexer = Arc::clone(&self.indexer);
        tokio::task::spawn_blocking(move || indexer.search_properties(&query, limit)).await?
    }

    pub async fn search_all(&self, query: String, limit: usize) -> Result<Vec<SearchResult>> {
        let indexer = Arc::clone(&self.indexer);
        tokio::task::spawn_blocking(move || indexer.search_all(&query, limit)).await?
    }

    pub async fn get_statistics(&self) -> Result<Option<IndexStatistics>> {
        let indexer = Arc::clone(&self.indexer);
        tokio::task::spawn_blocking(move || indexer.get_statistics()).await?
    }

    pub async fn clear_index(&self) -> Result<()> {
        let indexer = Arc::clone(&self.indexer);
        tokio::task::spawn_blocking(move || indexer.clear_index()).await?
    }
}
