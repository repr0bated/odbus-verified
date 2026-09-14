//! Async-safe indexer manager
//!
//! Wraps the DbusIndexer to provide async-safe access using spawn_blocking

use crate::indexer::{DbusIndexer, IndexStatistics, SearchResult};
use anyhow::Result;
use op_core::types::BusType;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Async-safe wrapper around DbusIndexer
pub struct IndexerManager {
    db_path: PathBuf,
    // Mutex protects against concurrent database access
    // Inner Option allows taking ownership for spawn_blocking
    #[allow(clippy::arc_with_non_send_sync)]
    _indexer: Arc<Mutex<Option<DbusIndexer>>>,
}

impl IndexerManager {
    /// Create new indexer manager
    #[allow(clippy::arc_with_non_send_sync)]
    pub async fn new(db_path: impl Into<PathBuf>) -> Result<Self> {
        let db_path = db_path.into();
        let indexer = DbusIndexer::new(&db_path).await?;

        Ok(Self {
            db_path,
            _indexer: Arc::new(Mutex::new(Some(indexer))),
        })
    }

    /// Build or rebuild the index
    pub async fn build_index(&self, bus_type: BusType) -> Result<IndexStatistics> {
        let db_path = self.db_path.clone();

        tokio::task::spawn_blocking(move || {
            // Create a new indexer in the blocking task
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let indexer = DbusIndexer::new(&db_path).await?;
                indexer.build_index(bus_type).await
            })
        })
        .await?
    }

    /// Search methods
    pub async fn search_methods(&self, query: String, limit: usize) -> Result<Vec<SearchResult>> {
        let db_path = self.db_path.clone();

        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let indexer = DbusIndexer::new(&db_path).await?;
                indexer.search_methods(&query, limit)
            })
        })
        .await?
    }

    /// Search properties
    pub async fn search_properties(
        &self,
        query: String,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let db_path = self.db_path.clone();

        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let indexer = DbusIndexer::new(&db_path).await?;
                indexer.search_properties(&query, limit)
            })
        })
        .await?
    }

    /// Search all (methods, properties, signals)
    pub async fn search_all(&self, query: String, limit: usize) -> Result<Vec<SearchResult>> {
        let db_path = self.db_path.clone();

        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let indexer = DbusIndexer::new(&db_path).await?;
                indexer.search_all(&query, limit)
            })
        })
        .await?
    }

    /// Get statistics
    pub async fn get_statistics(&self) -> Result<Option<IndexStatistics>> {
        let db_path = self.db_path.clone();

        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let indexer = DbusIndexer::new(&db_path).await?;
                indexer.get_statistics()
            })
        })
        .await?
    }

    /// Clear index
    pub async fn clear_index(&self) -> Result<()> {
        let db_path = self.db_path.clone();

        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let indexer = DbusIndexer::new(&db_path).await?;
                indexer.clear_index()
            })
        })
        .await?
    }
}

// IndexerManager is Send + Sync by virtue of using Arc<Mutex<...>>
unsafe impl Send for IndexerManager {}
unsafe impl Sync for IndexerManager {}
