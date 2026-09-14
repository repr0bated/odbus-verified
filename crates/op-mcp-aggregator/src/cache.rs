//! Tool Schema Cache with TTL and LRU eviction
//!
//! Caches tool definitions from upstream servers to reduce latency.

use crate::client::ToolDefinition;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info};

/// Cached tool entry with TTL
#[derive(Debug, Clone)]
struct CachedTool {
    definition: ToolDefinition,
    /// Which server this tool came from
    server_id: String,
    /// When this entry was cached
    cached_at: Instant,
    /// How many times this tool was accessed
    access_count: u64,
}

impl CachedTool {
    fn new(definition: ToolDefinition, server_id: String) -> Self {
        Self {
            definition,
            server_id,
            cached_at: Instant::now(),
            access_count: 0,
        }
    }

    fn is_expired(&self, ttl: Duration) -> bool {
        self.cached_at.elapsed() > ttl
    }

    fn touch(&mut self) {
        self.access_count += 1;
    }
}

/// Tool cache with TTL and LRU eviction
pub struct ToolCache {
    /// Cached tools by name
    cache: RwLock<LruCache<String, CachedTool>>,
    /// Time-to-live for cached entries
    ttl: Duration,
    /// Statistics
    stats: RwLock<CacheStats>,
}

/// Cache statistics
#[derive(Debug, Clone, Default)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub refreshes: u64,
}

impl ToolCache {
    /// Create a new tool cache
    pub fn new(max_entries: usize, ttl: Duration) -> Self {
        let capacity = NonZeroUsize::new(max_entries).unwrap_or(NonZeroUsize::new(1000).unwrap());
        Self {
            cache: RwLock::new(LruCache::new(capacity)),
            ttl,
            stats: RwLock::new(CacheStats::default()),
        }
    }

    /// Get a tool definition from cache
    pub async fn get(&self, name: &str) -> Option<(ToolDefinition, String)> {
        let mut cache = self.cache.write().await;

        if let Some(entry) = cache.get_mut(name) {
            if entry.is_expired(self.ttl) {
                // Entry expired, remove it
                cache.pop(name);
                let mut stats = self.stats.write().await;
                stats.misses += 1;
                return None;
            }

            entry.touch();
            let mut stats = self.stats.write().await;
            stats.hits += 1;

            return Some((entry.definition.clone(), entry.server_id.clone()));
        }

        let mut stats = self.stats.write().await;
        stats.misses += 1;
        None
    }

    /// Insert or update a tool in the cache
    pub async fn insert(&self, tool: ToolDefinition, server_id: &str) {
        let name = tool.name.clone();
        let entry = CachedTool::new(tool, server_id.to_string());

        let mut cache = self.cache.write().await;
        cache.put(name, entry);
    }

    /// Insert multiple tools from a server
    pub async fn insert_batch(&self, tools: Vec<ToolDefinition>, server_id: &str) {
        let mut cache = self.cache.write().await;

        for tool in tools {
            let name = tool.name.clone();
            let entry = CachedTool::new(tool, server_id.to_string());
            cache.put(name, entry);
        }

        debug!("Cached {} tools from server {}", cache.len(), server_id);
    }

    /// Remove a tool from cache
    pub async fn remove(&self, name: &str) -> bool {
        let mut cache = self.cache.write().await;
        cache.pop(name).is_some()
    }

    /// Remove all tools from a specific server
    pub async fn remove_server(&self, server_id: &str) {
        let mut cache = self.cache.write().await;
        let mut stats = self.stats.write().await;

        // Collect keys to remove (can't remove while iterating)
        let to_remove: Vec<String> = cache
            .iter()
            .filter(|(_, entry)| entry.server_id == server_id)
            .map(|(name, _)| name.clone())
            .collect();

        for name in to_remove {
            cache.pop(&name);
            stats.evictions += 1;
        }
    }

    /// Get all cached tool definitions
    pub async fn list_all(&self) -> Vec<(ToolDefinition, String)> {
        let cache = self.cache.read().await;
        cache
            .iter()
            .filter(|(_, entry)| !entry.is_expired(self.ttl))
            .map(|(_, entry)| (entry.definition.clone(), entry.server_id.clone()))
            .collect()
    }

    /// Get all tool names
    pub async fn tool_names(&self) -> Vec<String> {
        let cache = self.cache.read().await;
        cache
            .iter()
            .filter(|(_, entry)| !entry.is_expired(self.ttl))
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Check which server owns a tool
    pub async fn get_server_id(&self, tool_name: &str) -> Option<String> {
        let cache = self.cache.read().await;
        cache.peek(tool_name).map(|entry| entry.server_id.clone())
    }

    /// Clear all cached entries
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        cache.clear();
        info!("Tool cache cleared");
    }

    /// Get cache statistics
    pub async fn stats(&self) -> CacheStats {
        self.stats.read().await.clone()
    }

    /// Get cache size
    pub async fn len(&self) -> usize {
        self.cache.read().await.len()
    }

    /// Check if cache is empty
    pub async fn is_empty(&self) -> bool {
        self.cache.read().await.is_empty()
    }

    /// Evict expired entries
    pub async fn evict_expired(&self) -> usize {
        let mut cache = self.cache.write().await;
        let mut stats = self.stats.write().await;

        let to_remove: Vec<String> = cache
            .iter()
            .filter(|(_, entry)| entry.is_expired(self.ttl))
            .map(|(name, _)| name.clone())
            .collect();

        let count = to_remove.len();
        for name in to_remove {
            cache.pop(&name);
            stats.evictions += 1;
        }

        if count > 0 {
            debug!("Evicted {} expired cache entries", count);
        }

        count
    }
}

/// Background cache maintenance task
pub async fn cache_maintenance_loop(cache: Arc<ToolCache>, interval: Duration) {
    loop {
        tokio::time::sleep(interval).await;
        let evicted = cache.evict_expired().await;
        if evicted > 0 {
            debug!("Cache maintenance: evicted {} entries", evicted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use simd_json::json;

    fn make_tool(name: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: "Test tool".to_string(),
            input_schema: json!({}),
            schema_version: String::new(),
            category: "test".to_string(),
            tags: vec![],
            namespace: "test".to_string(),
            annotations: None,
        }
    }

    #[tokio::test]
    async fn test_cache_insert_and_get() {
        let cache = ToolCache::new(100, Duration::from_secs(300));
        let tool = make_tool("test_tool");

        cache.insert(tool.clone(), "server1").await;

        let result = cache.get("test_tool").await;
        assert!(result.is_some());
        let (def, server) = result.unwrap();
        assert_eq!(def.name, "test_tool");
        assert_eq!(server, "server1");
    }

    #[tokio::test]
    async fn test_cache_expiry() {
        let cache = ToolCache::new(100, Duration::from_millis(10));
        let tool = make_tool("test_tool");

        cache.insert(tool, "server1").await;

        // Should be found immediately
        assert!(cache.get("test_tool").await.is_some());

        // Wait for expiry
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Should be expired now
        assert!(cache.get("test_tool").await.is_none());
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let cache = ToolCache::new(100, Duration::from_secs(300));
        let tool = make_tool("test_tool");

        cache.insert(tool, "server1").await;

        // Hit
        cache.get("test_tool").await;
        // Miss
        cache.get("nonexistent").await;

        let stats = cache.stats().await;
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[tokio::test]
    async fn test_remove_server() {
        let cache = ToolCache::new(100, Duration::from_secs(300));

        cache.insert(make_tool("tool1"), "server1").await;
        cache.insert(make_tool("tool2"), "server1").await;
        cache.insert(make_tool("tool3"), "server2").await;

        assert_eq!(cache.len().await, 3);

        cache.remove_server("server1").await;

        assert_eq!(cache.len().await, 1);
        assert!(cache.get("tool3").await.is_some());
    }
}
