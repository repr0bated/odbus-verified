//! Profile Manager for tool selection
//!
//! Manages named profiles that select subsets of tools from the aggregated pool.

use crate::cache::ToolCache;
use crate::client::ToolDefinition;
use crate::config::{AggregatorConfig, ProfileConfig};
use simd_json::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Manages tool profiles
pub struct ProfileManager {
    /// Profile configurations
    profiles: RwLock<HashMap<String, ProfileConfig>>,
    /// Default profile name
    default_profile: String,
    /// Maximum tools per profile
    max_tools: usize,
    /// Reference to tool cache
    cache: Arc<ToolCache>,
}

impl ProfileManager {
    /// Create a new profile manager
    pub fn new(config: &AggregatorConfig, cache: Arc<ToolCache>) -> Self {
        let mut profiles = config.profiles.clone();

        // Ensure we have a default profile
        if !profiles.contains_key(&config.default_profile) {
            profiles.insert(
                config.default_profile.clone(),
                ProfileConfig::new("Default profile - all tools"),
            );
        }

        Self {
            profiles: RwLock::new(profiles),
            default_profile: config.default_profile.clone(),
            max_tools: config.max_tools_per_profile,
            cache,
        }
    }

    /// Get available profile names
    pub async fn list_profiles(&self) -> Vec<String> {
        self.profiles.read().await.keys().cloned().collect()
    }

    /// Get profile configuration
    pub async fn get_profile(&self, name: &str) -> Option<ProfileConfig> {
        self.profiles.read().await.get(name).cloned()
    }

    /// Add or update a profile
    pub async fn set_profile(&self, name: &str, config: ProfileConfig) {
        self.profiles.write().await.insert(name.to_string(), config);
        info!("Updated profile: {}", name);
    }

    /// Remove a profile
    pub async fn remove_profile(&self, name: &str) -> bool {
        if name == self.default_profile {
            warn!("Cannot remove default profile: {}", name);
            return false;
        }
        self.profiles.write().await.remove(name).is_some()
    }

    /// Get the default profile name
    pub fn default_profile(&self) -> &str {
        &self.default_profile
    }

    /// Get tools for a specific profile
    pub async fn get_tools_for_profile(&self, profile_name: &str) -> Vec<ToolDefinition> {
        let profiles = self.profiles.read().await;
        let profile = profiles.get(profile_name).cloned();
        drop(profiles);

        let profile = match profile {
            Some(p) => p,
            None => {
                warn!("Profile '{}' not found, using default", profile_name);
                self.profiles
                    .read()
                    .await
                    .get(&self.default_profile)
                    .cloned()
                    .unwrap_or_default()
            }
        };

        self.filter_tools(&profile).await
    }

    /// Filter tools based on profile configuration
    async fn filter_tools(&self, profile: &ProfileConfig) -> Vec<ToolDefinition> {
        let all_tools = self.cache.list_all().await;
        let max = profile.max_tools.unwrap_or(self.max_tools);

        let mut filtered: Vec<ToolDefinition> = all_tools
            .into_iter()
            .filter(|(tool, server_id)| self.matches_profile(tool, server_id, profile))
            .map(|(tool, _)| tool)
            .collect();

        // Sort by priority/relevance (for now, just alphabetically)
        filtered.sort_by(|a, b| a.name.cmp(&b.name));

        // Apply max limit
        if filtered.len() > max {
            debug!("Profile has {} tools, limiting to {}", filtered.len(), max);
            filtered.truncate(max);
        }

        filtered
    }

    /// Check if a tool matches the profile criteria
    fn matches_profile(
        &self,
        tool: &ToolDefinition,
        server_id: &str,
        profile: &ProfileConfig,
    ) -> bool {
        // Check server filter
        if !profile.servers.is_empty() && !profile.servers.contains(&server_id.to_string()) {
            return false;
        }

        // Check tool name include filter
        if !profile.include_tools.is_empty()
            && !profile.include_tools.iter().any(|t| {
                // Support wildcards like "github_*"
                if t.ends_with('*') {
                    tool.name.starts_with(&t[..t.len() - 1])
                } else {
                    &tool.name == t
                }
            })
        {
            return false;
        }

        // Check tool name exclude filter
        if profile.exclude_tools.iter().any(|t| {
            if t.ends_with('*') {
                tool.name.starts_with(&t[..t.len() - 1])
            } else {
                &tool.name == t
            }
        }) {
            return false;
        }

        // Check category filter
        if !profile.include_categories.is_empty() {
            let category = tool
                .annotations
                .as_ref()
                .and_then(|a| a.as_object())
                .and_then(|obj| obj.get("category"))
                .and_then(|c| c.as_str())
                .unwrap_or("general");

            if !profile.include_categories.contains(&category.to_string()) {
                return false;
            }
        }

        // Check namespace filter
        if !profile.include_namespaces.is_empty() {
            let namespace = tool
                .annotations
                .as_ref()
                .and_then(|a| a.as_object())
                .and_then(|obj| obj.get("namespace"))
                .and_then(|n| n.as_str())
                .unwrap_or("system");

            if !profile.include_namespaces.contains(&namespace.to_string()) {
                return false;
            }
        }

        true
    }

    /// Check if a tool is available in a profile
    pub async fn tool_available_in_profile(&self, tool_name: &str, profile_name: &str) -> bool {
        let tools = self.get_tools_for_profile(profile_name).await;
        let result = tools.iter().any(|t| t.name == tool_name);
        result
    }

    /// Get profile stats
    pub async fn get_profile_stats(&self, profile_name: &str) -> ProfileStats {
        let tools = self.get_tools_for_profile(profile_name).await;

        let mut categories: HashMap<String, usize> = HashMap::new();
        for tool in &tools {
            let category = tool
                .annotations
                .as_ref()
                .and_then(|a| a.as_object())
                .and_then(|obj| obj.get("category"))
                .and_then(|c| c.as_str())
                .unwrap_or("general")
                .to_string();
            *categories.entry(category).or_insert(0) += 1;
        }

        ProfileStats {
            tool_count: tools.len(),
            max_tools: self.max_tools,
            categories,
        }
    }
}

/// Statistics about a profile
#[derive(Debug, Clone)]
pub struct ProfileStats {
    pub tool_count: usize,
    pub max_tools: usize,
    pub categories: HashMap<String, usize>,
}

impl ProfileStats {
    pub fn remaining_capacity(&self) -> usize {
        self.max_tools.saturating_sub(self.tool_count)
    }

    pub fn is_at_capacity(&self) -> bool {
        self.tool_count >= self.max_tools
    }
}

/// Create default profiles for common use cases
pub fn create_default_profiles() -> HashMap<String, ProfileConfig> {
    let mut profiles = HashMap::new();

    // Minimal profile - only essential tools
    profiles.insert(
        "minimal".to_string(),
        ProfileConfig {
            description: "Essential tools only".to_string(),
            max_tools: Some(10),
            include_categories: vec!["response".to_string(), "system".to_string()],
            ..Default::default()
        },
    );

    // Sysadmin profile - system management tools
    profiles.insert(
        "sysadmin".to_string(),
        ProfileConfig {
            description: "System administration tools".to_string(),
            max_tools: Some(35),
            include_namespaces: vec![
                "system".to_string(),
                "systemd".to_string(),
                "network".to_string(),
                "dbus".to_string(),
            ],
            ..Default::default()
        },
    );

    // Developer profile - development tools
    profiles.insert(
        "dev".to_string(),
        ProfileConfig {
            description: "Development tools".to_string(),
            max_tools: Some(35),
            include_categories: vec![
                "filesystem".to_string(),
                "shell".to_string(),
                "git".to_string(),
                "code".to_string(),
            ],
            ..Default::default()
        },
    );

    // Full profile - everything (may exceed limits)
    profiles.insert(
        "full".to_string(),
        ProfileConfig {
            description: "All available tools (may exceed Cursor limits)".to_string(),
            max_tools: Some(100),
            ..Default::default()
        },
    );

    profiles
}

#[cfg(test)]
mod tests {
    use super::*;
    use simd_json::json;
    use std::time::Duration;

    fn make_tool(name: &str, category: &str, namespace: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_string(),
            description: "Test".to_string(),
            input_schema: json!({}),
            schema_version: String::new(),
            category: category.to_string(),
            tags: vec![],
            namespace: namespace.to_string(),
            annotations: Some(json!({
                "category": category,
                "namespace": namespace
            })),
        }
    }

    #[tokio::test]
    async fn test_profile_filtering() {
        let cache = Arc::new(ToolCache::new(100, Duration::from_secs(300)));

        // Add tools to cache
        cache
            .insert(make_tool("tool1", "system", "system"), "server1")
            .await;
        cache
            .insert(make_tool("tool2", "network", "network"), "server1")
            .await;
        cache
            .insert(make_tool("tool3", "dev", "dev"), "server2")
            .await;

        let config = AggregatorConfig::default();
        let manager = ProfileManager::new(&config, cache);

        // Add a restrictive profile
        manager
            .set_profile(
                "system_only",
                ProfileConfig {
                    description: "System tools".to_string(),
                    include_namespaces: vec!["system".to_string()],
                    ..Default::default()
                },
            )
            .await;

        let tools = manager.get_tools_for_profile("system_only").await;
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "tool1");
    }

    #[tokio::test]
    async fn test_wildcard_matching() {
        let cache = Arc::new(ToolCache::new(100, Duration::from_secs(300)));

        cache
            .insert(make_tool("github_search", "git", "git"), "gh")
            .await;
        cache
            .insert(make_tool("github_pr_list", "git", "git"), "gh")
            .await;
        cache
            .insert(make_tool("shell_exec", "shell", "system"), "local")
            .await;

        let config = AggregatorConfig::default();
        let manager = ProfileManager::new(&config, cache);

        manager
            .set_profile(
                "github",
                ProfileConfig {
                    description: "GitHub tools".to_string(),
                    include_tools: vec!["github_*".to_string()],
                    ..Default::default()
                },
            )
            .await;

        let tools = manager.get_tools_for_profile("github").await;
        assert_eq!(tools.len(), 2);
    }

    #[tokio::test]
    async fn test_max_tools_limit() {
        let cache = Arc::new(ToolCache::new(100, Duration::from_secs(300)));

        // Add many tools
        for i in 0..50 {
            cache
                .insert(
                    make_tool(&format!("tool{}", i), "general", "system"),
                    "server",
                )
                .await;
        }

        let config = AggregatorConfig {
            max_tools_per_profile: 20,
            ..Default::default()
        };
        let manager = ProfileManager::new(&config, cache);

        let tools = manager.get_tools_for_profile("default").await;
        assert_eq!(tools.len(), 20);
    }
}
