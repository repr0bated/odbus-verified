use anyhow::Result;
use async_trait::async_trait;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::DynamicLoaderError;
use crate::loading_strategy::LoadingStrategy;
use op_execution_tracker::{ExecutionContext, ExecutionTracker};
use op_tools::{BoxedTool, ToolRegistry};

/// Dynamic tool registry that wraps existing registry with caching
pub struct DynamicToolRegistry {
    /// Underlying tool registry (existing functionality)
    base_registry: Arc<ToolRegistry>,

    /// Execution tracker for load decisions
    execution_tracker: Arc<ExecutionTracker>,

    /// Loading strategy
    loading_strategy: Arc<dyn LoadingStrategy>,

    /// LRU cache for loaded tools
    tool_cache: Arc<RwLock<LruCache<String, BoxedTool>>>,

    /// Cache statistics
    cache_hits: Arc<RwLock<u64>>,
    cache_misses: Arc<RwLock<u64>>,
}

impl DynamicToolRegistry {
    /// Create new dynamic registry
    pub fn new(
        base_registry: Arc<ToolRegistry>,
        execution_tracker: Arc<ExecutionTracker>,
        loading_strategy: Arc<dyn LoadingStrategy>,
        max_cache_size: usize,
    ) -> Self {
        Self {
            base_registry,
            execution_tracker,
            loading_strategy,
            tool_cache: Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(max_cache_size).unwrap(),
            ))),
            cache_hits: Arc::new(RwLock::new(0)),
            cache_misses: Arc::new(RwLock::new(0)),
        }
    }

    /// Get tool with dynamic loading
    pub async fn get_tool(&self, name: &str, context: &ExecutionContext) -> Result<BoxedTool> {
        // Check cache first
        {
            // LruCache::get requires &mut self to update LRU order
            let mut cache = self.tool_cache.write().await;
            if let Some(tool) = cache.get(name) {
                *self.cache_hits.write().await += 1;
                return Ok(Arc::clone(tool));
            }
        }

        // Tool not in cache - check if we should load it
        if self.loading_strategy.should_load(name, context).await {
            // Load from base registry
            if let Some(tool) = self.base_registry.get(name).await {
                // Cache the tool
                let mut cache = self.tool_cache.write().await;
                cache.put(name.to_string(), tool.clone());

                *self.cache_misses.write().await += 1;
                return Ok(tool);
            }
        }

        Err(DynamicLoaderError::ToolNotFound(name.to_string()).into())
    }

    /// Get cache statistics
    pub async fn get_cache_stats(&self) -> (u64, u64) {
        let hits = *self.cache_hits.read().await;
        let misses = *self.cache_misses.read().await;
        (hits, misses)
    }

    /// Get current cache size
    pub async fn get_cache_size(&self) -> usize {
        let cache = self.tool_cache.read().await;
        cache.len()
    }

    /// Clear cache (for testing or memory management)
    pub async fn clear_cache(&self) {
        let mut cache = self.tool_cache.write().await;
        cache.clear();
    }

    /// Get base registry (for compatibility)
    pub fn base_registry(&self) -> Arc<ToolRegistry> {
        Arc::clone(&self.base_registry)
    }

    /// Get execution tracker (for compatibility)
    pub fn execution_tracker(&self) -> Arc<ExecutionTracker> {
        Arc::clone(&self.execution_tracker)
    }
}

/// Enhanced tool registry trait
#[async_trait]
pub trait EnhancedToolRegistry: Send + Sync {
    /// Get tool with dynamic loading
    async fn get_tool(&self, name: &str, context: &ExecutionContext) -> Result<BoxedTool>;

    /// Get cache statistics
    async fn get_cache_stats(&self) -> (u64, u64);

    /// Get current cache size
    async fn get_cache_size(&self) -> usize;
}

#[async_trait]
impl EnhancedToolRegistry for DynamicToolRegistry {
    async fn get_tool(&self, name: &str, context: &ExecutionContext) -> Result<BoxedTool> {
        self.get_tool(name, context).await
    }

    async fn get_cache_stats(&self) -> (u64, u64) {
        self.get_cache_stats().await
    }

    async fn get_cache_size(&self) -> usize {
        self.get_cache_size().await
    }
}
