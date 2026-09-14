//! Tool Discovery System
//!
//! Provides a catalog of all available tools without loading them.
//! Tools are loaded on-demand via the ToolRegistry.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::registry::ToolDefinition;

pub mod projection_engine;
pub mod sources;

pub use projection_engine::ProjectionEngine;
pub use sources::{AgentDiscoverySource, DbusDiscoverySource, PluginDiscoverySource};

/// Source type for tool discovery
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceType {
    /// Built-in tools compiled into the binary
    Builtin,
    /// D-Bus services discovered at runtime
    Dbus,
    /// Plugins loaded dynamically
    Plugin,
    /// Agent-based tools
    Agent,
    /// External MCP servers
    Mcp,
}

/// Information about a tool source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSourceInfo {
    pub source_type: SourceType,
    pub name: String,
    pub description: String,
    pub tool_count: usize,
    pub last_refresh: Option<chrono::DateTime<chrono::Utc>>,
}

/// Cache policy for discovery
#[derive(Debug, Clone)]
pub enum DiscoveryCachePolicy {
    /// Always use cached data if available
    PreferCache,
    /// Refresh if cache is older than duration
    RefreshAfter(Duration),
    /// Always refresh from source
    AlwaysRefresh,
}

impl Default for DiscoveryCachePolicy {
    fn default() -> Self {
        DiscoveryCachePolicy::RefreshAfter(Duration::from_secs(300))
    }
}

/// Trait for tool discovery sources
#[async_trait]
pub trait ToolDiscoverySource: Send + Sync {
    /// Get the source type
    fn source_type(&self) -> SourceType;

    /// Get source name
    fn name(&self) -> &str;

    /// Get source description
    fn description(&self) -> &str;

    /// Discover all tools from this source
    async fn discover(&self) -> anyhow::Result<Vec<ToolDefinition>>;

    /// Check if source is available
    async fn is_available(&self) -> bool {
        true
    }
}

/// Built-in tool source for statically defined tools
pub struct BuiltinToolSource {
    tools: Vec<ToolDefinition>,
}

impl BuiltinToolSource {
    pub fn new(tools: Vec<ToolDefinition>) -> Self {
        Self { tools }
    }
}

#[async_trait]
impl ToolDiscoverySource for BuiltinToolSource {
    fn source_type(&self) -> SourceType {
        SourceType::Builtin
    }

    fn name(&self) -> &str {
        "builtin"
    }

    fn description(&self) -> &str {
        "Built-in tools compiled into the binary"
    }

    async fn discover(&self) -> anyhow::Result<Vec<ToolDefinition>> {
        Ok(self.tools.clone())
    }
}

/// Statistics about the discovery system
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiscoveryStats {
    pub total_tools: usize,
    pub source_count: usize,
    pub last_full_refresh: Option<chrono::DateTime<chrono::Utc>>,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

/// Central tool discovery system
pub struct ToolDiscoverySystem {
    sources: RwLock<Vec<Arc<dyn ToolDiscoverySource>>>,
    cache: RwLock<HashMap<String, ToolDefinition>>,
    cache_timestamp: RwLock<Option<Instant>>,
    cache_policy: DiscoveryCachePolicy,
    stats: RwLock<DiscoveryStats>,
}

impl ToolDiscoverySystem {
    pub fn new() -> Self {
        Self {
            sources: RwLock::new(Vec::new()),
            cache: RwLock::new(HashMap::new()),
            cache_timestamp: RwLock::new(None),
            cache_policy: DiscoveryCachePolicy::default(),
            stats: RwLock::new(DiscoveryStats::default()),
        }
    }

    pub fn with_cache_policy(mut self, policy: DiscoveryCachePolicy) -> Self {
        self.cache_policy = policy;
        self
    }

    /// Register a discovery source
    pub async fn register_source(&self, source: Arc<dyn ToolDiscoverySource>) {
        let mut sources = self.sources.write().await;
        info!(
            "Registering discovery source: {} ({})",
            source.name(),
            source.description()
        );
        sources.push(source);
    }

    /// Get all tool definitions (from cache or refresh)
    pub async fn get_all_tool_definitions(&self) -> anyhow::Result<Vec<ToolDefinition>> {
        let should_refresh = self.should_refresh().await;

        if should_refresh {
            self.refresh_cache().await?;
        } else {
            let mut stats = self.stats.write().await;
            stats.cache_hits += 1;
        }

        let cache = self.cache.read().await;
        Ok(cache.values().cloned().collect())
    }

    /// Get a specific tool definition by name
    pub async fn get_tool_definition(&self, name: &str) -> Option<ToolDefinition> {
        // First check cache
        {
            let cache = self.cache.read().await;
            if let Some(def) = cache.get(name) {
                return Some(def.clone());
            }
        }

        // If not in cache and cache might be stale, refresh
        if self.should_refresh().await {
            if let Err(e) = self.refresh_cache().await {
                warn!("Failed to refresh cache: {}", e);
            }
        }

        let cache = self.cache.read().await;
        cache.get(name).cloned()
    }

    /// Search for tools matching criteria
    pub async fn search_tools(
        &self,
        query: &str,
        category: Option<&str>,
        tags: Option<&[String]>,
    ) -> Vec<ToolDefinition> {
        let cache = self.cache.read().await;
        let query_lower = query.to_lowercase();

        cache
            .values()
            .filter(|def| {
                // Match query against name or description
                let matches_query = query.is_empty()
                    || def.name.to_lowercase().contains(&query_lower)
                    || def.description.to_lowercase().contains(&query_lower);

                // Match category if specified
                let matches_category = category.map(|c| def.category == c).unwrap_or(true);

                // Match tags if specified
                let matches_tags = tags
                    .map(|t| t.iter().any(|tag| def.tags.contains(tag)))
                    .unwrap_or(true);

                matches_query && matches_category && matches_tags
            })
            .cloned()
            .collect()
    }

    /// Get statistics
    pub async fn stats(&self) -> DiscoveryStats {
        self.stats.read().await.clone()
    }

    /// Get information about all sources
    pub async fn get_sources(&self) -> Vec<ToolSourceInfo> {
        let sources = self.sources.read().await;
        let mut infos = Vec::new();

        for source in sources.iter() {
            let tool_count = source.discover().await.map(|t| t.len()).unwrap_or(0);
            infos.push(ToolSourceInfo {
                source_type: source.source_type(),
                name: source.name().to_string(),
                description: source.description().to_string(),
                tool_count,
                last_refresh: None,
            });
        }

        infos
    }

    /// Start background refresh task
    pub async fn start_background_refresh(&self) {
        // Initial refresh
        if let Err(e) = self.refresh_cache().await {
            warn!("Initial cache refresh failed: {}", e);
        }
    }

    /// Force refresh the cache
    pub async fn force_refresh(&self) -> anyhow::Result<()> {
        self.refresh_cache().await
    }

    /// Check if cache should be refreshed
    async fn should_refresh(&self) -> bool {
        match &self.cache_policy {
            DiscoveryCachePolicy::PreferCache => {
                let timestamp = self.cache_timestamp.read().await;
                timestamp.is_none()
            }
            DiscoveryCachePolicy::RefreshAfter(duration) => {
                let timestamp = self.cache_timestamp.read().await;
                match *timestamp {
                    None => true,
                    Some(ts) => ts.elapsed() > *duration,
                }
            }
            DiscoveryCachePolicy::AlwaysRefresh => true,
        }
    }

    /// Refresh the cache from all sources
    async fn refresh_cache(&self) -> anyhow::Result<()> {
        debug!("Refreshing tool discovery cache");

        let sources = self.sources.read().await;
        let mut new_cache = HashMap::new();

        for source in sources.iter() {
            if !source.is_available().await {
                debug!("Source {} is not available, skipping", source.name());
                continue;
            }

            match source.discover().await {
                Ok(tools) => {
                    debug!(
                        "Discovered {} tools from source {}",
                        tools.len(),
                        source.name()
                    );
                    for tool in tools {
                        new_cache.insert(tool.name.clone(), tool);
                    }
                }
                Err(e) => {
                    warn!("Failed to discover tools from {}: {}", source.name(), e);
                }
            }
        }

        // Update cache
        {
            let mut cache = self.cache.write().await;
            *cache = new_cache;
        }

        // Update timestamp
        {
            let mut timestamp = self.cache_timestamp.write().await;
            *timestamp = Some(Instant::now());
        }

        // Update stats
        {
            let cache = self.cache.read().await;
            let mut stats = self.stats.write().await;
            stats.total_tools = cache.len();
            stats.source_count = sources.len();
            stats.last_full_refresh = Some(chrono::Utc::now());
            stats.cache_misses += 1;
        }

        info!(
            "Tool discovery cache refreshed: {} tools from {} sources",
            self.cache.read().await.len(),
            sources.len()
        );

        Ok(())
    }
}

impl Default for ToolDiscoverySystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_builtin_source() {
        let tools = vec![ToolDefinition {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            input_schema: simd_json::json!({}),
            schema_version: "1.0.0".to_string(),
            category: "test".to_string(),
            tags: vec!["test".to_string()],
            namespace: "test".to_string(),
        }];

        let source = BuiltinToolSource::new(tools);
        let discovered = source.discover().await.unwrap();

        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].name, "test_tool");
    }

    #[tokio::test]
    async fn test_discovery_system() {
        let system = ToolDiscoverySystem::new();

        let tools = vec![ToolDefinition {
            name: "builtin_tool".to_string(),
            description: "Built-in tool".to_string(),
            input_schema: simd_json::json!({}),
            schema_version: "https://json-schema.org/draft/next/schema".to_string(),
            category: "builtin".to_string(),
            tags: vec![],
            namespace: "builtin".to_string(),
        }];

        system
            .register_source(Arc::new(BuiltinToolSource::new(tools)))
            .await;

        let all_tools = system.get_all_tool_definitions().await.unwrap();
        assert_eq!(all_tools.len(), 1);

        let found = system.get_tool_definition("builtin_tool").await;
        assert!(found.is_some());
    }

    #[tokio::test]
    async fn test_search_tools() {
        let system = ToolDiscoverySystem::new();

        let tools = vec![
            ToolDefinition {
                name: "systemd_start".to_string(),
                description: "Start a systemd unit".to_string(),
                input_schema: simd_json::json!({}),
                schema_version: "1.0.0".to_string(),
                category: "dbus".to_string(),
                tags: vec!["systemd".to_string()],
                namespace: "system".to_string(),
            },
            ToolDefinition {
                name: "network_status".to_string(),
                description: "Get network status".to_string(),
                input_schema: simd_json::json!({}),
                schema_version: "1.0.0".to_string(),
                category: "dbus".to_string(),
                tags: vec!["network".to_string()],
                namespace: "system".to_string(),
            },
        ];

        system
            .register_source(Arc::new(BuiltinToolSource::new(tools)))
            .await;
        system.refresh_cache().await.unwrap();

        // Search by query
        let results = system.search_tools("systemd", None, None).await;
        assert_eq!(results.len(), 1);

        // Search by tag
        let results = system
            .search_tools("", None, Some(&["network".to_string()]))
            .await;
        assert_eq!(results.len(), 1);
    }
}
