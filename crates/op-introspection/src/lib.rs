//! op-introspection: DBus introspection capabilities
//!
//! This crate provides:
//! - Service discovery
//! - Interface introspection
//! - XML parsing to JSON-serializable structures
//! - Caching of introspection results
//! - FTS5 full-text search indexer for semantic DBus queries
//!
//! All introspection results are returned as structs that implement
//! Serialize/Deserialize for easy JSON conversion in the RPC layer.

pub mod cache;
pub mod indexer;
pub mod indexer_manager;
pub mod parser;
pub mod projection;
pub mod scanner;

pub use cache::IntrospectionCache;
pub use indexer::{DbusIndexer, IndexStatistics, SearchResult};
pub use indexer_manager::IndexerManager;
pub use parser::IntrospectionParser;
pub use projection::DbusProjection;
pub use scanner::ServiceScanner;

use op_core::error::Result;
use op_core::types::{BusType, ObjectInfo, ServiceInfo};
use simd_json::ValueBuilder;
use std::sync::Arc;

/// High-level introspection service
///
/// Provides DBus introspection with results as JSON-serializable structs.
pub struct IntrospectionService {
    scanner: ServiceScanner,
    cache: Arc<IntrospectionCache>,
}

impl IntrospectionService {
    /// Create a new introspection service
    pub fn new() -> Self {
        Self {
            scanner: ServiceScanner::new(),
            cache: Arc::new(IntrospectionCache::new()),
        }
    }

    /// List all services on a bus (returns JSON-serializable structs)
    pub async fn list_services(&self, bus_type: BusType) -> Result<Vec<ServiceInfo>> {
        self.scanner.list_services(bus_type).await
    }

    /// List all services as JSON
    pub async fn list_services_json(&self, bus_type: BusType) -> Result<simd_json::OwnedValue> {
        let services = self.list_services(bus_type).await?;
        Ok(simd_json::serde::to_owned_value(services).unwrap_or(simd_json::OwnedValue::null()))
    }

    /// Introspect a service (returns JSON-serializable struct)
    pub async fn introspect(
        &self,
        bus_type: BusType,
        service: &str,
        path: &str,
    ) -> Result<ObjectInfo> {
        // Check cache first
        if let Some(cached) = self.cache.get(bus_type, service, path).await {
            return Ok(cached);
        }

        // Perform introspection
        let info = self.scanner.introspect(bus_type, service, path).await?;

        // Cache the result
        self.cache.set(bus_type, service, path, info.clone()).await;

        Ok(info)
    }

    /// Introspect a service and return as JSON
    pub async fn introspect_json(
        &self,
        bus_type: BusType,
        service: &str,
        path: &str,
    ) -> Result<simd_json::OwnedValue> {
        let info = self.introspect(bus_type, service, path).await?;
        Ok(simd_json::serde::to_owned_value(info).unwrap_or(simd_json::OwnedValue::null()))
    }

    /// Get cache reference
    pub fn cache(&self) -> Arc<IntrospectionCache> {
        Arc::clone(&self.cache)
    }
}

impl Default for IntrospectionService {
    fn default() -> Self {
        Self::new()
    }
}

/// Prelude for convenient imports
pub mod prelude {
    pub use super::{
        DbusIndexer, DbusProjection, IndexStatistics, IndexerManager, IntrospectionCache,
        IntrospectionParser, IntrospectionService, SearchResult, ServiceScanner,
    };
}
