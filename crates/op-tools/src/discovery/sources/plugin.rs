//! Plugin Discovery Source
//!
//! Discovers tools from state plugins.

use async_trait::async_trait;
use tracing::debug;

use crate::discovery::{SourceType, ToolDiscoverySource};
use crate::registry::ToolDefinition;

/// Plugin discovery source
pub struct PluginDiscoverySource {
    /// Known plugin names
    plugins: Vec<String>,
}

impl Default for PluginDiscoverySource {
    fn default() -> Self {
        Self {
            plugins: default_plugins(),
        }
    }
}

fn default_plugins() -> Vec<String> {
    vec![
        "systemd".to_string(),
        "net".to_string(),
        "packagekit".to_string(),
        "login1".to_string(),
        "keyring".to_string(),
        "openflow".to_string(),
        "systemd_networkd".to_string(),
        "dnsresolver".to_string(),
        "netmaker".to_string(),
        "pcidecl".to_string(),
        "sessdecl".to_string(),
    ]
}

impl PluginDiscoverySource {
    pub fn new(plugins: Vec<String>) -> Self {
        Self { plugins }
    }
}

#[async_trait]
impl ToolDiscoverySource for PluginDiscoverySource {
    fn source_type(&self) -> SourceType {
        SourceType::Plugin
    }

    fn name(&self) -> &str {
        "plugins"
    }

    fn description(&self) -> &str {
        "State plugins with query/diff/apply operations"
    }

    async fn discover(&self) -> anyhow::Result<Vec<ToolDefinition>> {
        let mut tools = Vec::new();

        // Each plugin generates 3 tools: _query, _diff, _apply
        for plugin in &self.plugins {
            // Query tool
            tools.push(ToolDefinition {
                name: format!("plugin_{}_query", plugin),
                description: format!("Query current state from {} plugin", plugin),
                input_schema: simd_json::json!({
                    "type": "object",
                    "properties": {
                        "filter": {
                            "type": "object",
                            "description": "Optional filter for state query"
                        }
                    }
                }),
                schema_version: "https://json-schema.org/draft/next/schema".to_string(),
                category: "state".to_string(),
                tags: vec!["plugin".to_string(), "state".to_string(), plugin.clone()],
                namespace: "system.v1".to_string(),
            });

            // Diff tool
            tools.push(ToolDefinition {
                name: format!("plugin_{}_diff", plugin),
                description: format!(
                    "Calculate diff between current and desired state for {} plugin",
                    plugin
                ),
                input_schema: simd_json::json!({
                    "type": "object",
                    "properties": {
                        "desired_state": {
                            "type": "object",
                            "description": "Desired state configuration"
                        }
                    },
                    "required": ["desired_state"]
                }),
                schema_version: "https://json-schema.org/draft/next/schema".to_string(),
                category: "state".to_string(),
                tags: vec!["plugin".to_string(), "state".to_string(), plugin.clone()],
                namespace: "system.v1".to_string(),
            });

            // Apply tool
            tools.push(ToolDefinition {
                name: format!("plugin_{}_apply", plugin),
                description: format!("Apply state changes for {} plugin", plugin),
                input_schema: simd_json::json!({
                    "type": "object",
                    "properties": {
                        "diff": {
                            "type": "object",
                            "description": "State diff to apply"
                        },
                        "dry_run": {
                            "type": "boolean",
                            "description": "If true, only simulate changes",
                            "default": false
                        }
                    },
                    "required": ["diff"]
                }),
                schema_version: "https://json-schema.org/draft/next/schema".to_string(),
                category: "state".to_string(),
                tags: vec!["plugin".to_string(), "state".to_string(), plugin.clone()],
                namespace: "system.v1".to_string(),
            });
        }

        debug!(
            "Discovered {} plugin tools from {} plugins",
            tools.len(),
            self.plugins.len()
        );
        Ok(tools)
    }
}
