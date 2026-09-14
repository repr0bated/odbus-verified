//! Agent Discovery Source
//!
//! Discovers tools from D-Bus agents and LLM agents.

use async_trait::async_trait;
use std::path::PathBuf;
use tracing::{debug, warn};

use crate::discovery::{SourceType, ToolDiscoverySource};
use crate::registry::ToolDefinition;

/// Agent discovery source
pub struct AgentDiscoverySource {
    /// Path to LLM agents directory
    agents_dir: PathBuf,
    /// Known D-Bus agents
    dbus_agents: Vec<String>,
}

impl Default for AgentDiscoverySource {
    fn default() -> Self {
        Self {
            agents_dir: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/home/jeremy"))
                .join("agents"),
            dbus_agents: default_dbus_agents(),
        }
    }
}

fn default_dbus_agents() -> Vec<String> {
    vec![
        "executor".to_string(),
        "file".to_string(),
        "network".to_string(),
        "systemd".to_string(),
        "monitor".to_string(),
        "packagekit".to_string(),
        "python-pro".to_string(),
        "rust-pro".to_string(),
        "c-pro".to_string(),
        "cpp-pro".to_string(),
        "golang-pro".to_string(),
        "javascript-pro".to_string(),
        "php-pro".to_string(),
        "sql-pro".to_string(),
    ]
}

impl AgentDiscoverySource {
    pub fn new(agents_dir: PathBuf, dbus_agents: Vec<String>) -> Self {
        Self {
            agents_dir,
            dbus_agents,
        }
    }

    pub fn with_agents_dir(mut self, dir: PathBuf) -> Self {
        self.agents_dir = dir;
        self
    }
}

#[async_trait]
impl ToolDiscoverySource for AgentDiscoverySource {
    fn source_type(&self) -> SourceType {
        SourceType::Agent
    }

    fn name(&self) -> &str {
        "agents"
    }

    fn description(&self) -> &str {
        "D-Bus agents and LLM agents"
    }

    async fn discover(&self) -> anyhow::Result<Vec<ToolDefinition>> {
        let mut tools = Vec::new();

        // Discover D-Bus agents
        for agent in &self.dbus_agents {
            tools.push(ToolDefinition {
                name: format!("agent_{}_execute", agent.replace('-', "_")),
                description: format!("Execute task via {} agent", agent),
                input_schema: simd_json::json!({
                    "type": "object",
                    "properties": {
                        "task": {
                            "type": "string",
                            "description": "Task description for the agent"
                        },
                        "parameters": {
                            "type": "object",
                            "description": "Additional parameters"
                        }
                    },
                    "required": ["task"]
                }),
                schema_version: "https://json-schema.org/draft/next/schema".to_string(),
                category: "agent".to_string(),
                tags: vec!["agent".to_string(), "dbus".to_string(), agent.clone()],
                namespace: "system.v1".to_string(),
            });
        }

        // Discover LLM agents from ~/agents/
        if self.agents_dir.exists() {
            match self.discover_llm_agents().await {
                Ok(llm_tools) => {
                    debug!("Discovered {} LLM agent tools", llm_tools.len());
                    tools.extend(llm_tools);
                }
                Err(e) => {
                    warn!("Failed to discover LLM agents: {}", e);
                }
            }
        }

        debug!("Discovered {} total agent tools", tools.len());
        Ok(tools)
    }

    async fn is_available(&self) -> bool {
        // Always available - D-Bus agents are part of the system
        true
    }
}

impl AgentDiscoverySource {
    async fn discover_llm_agents(&self) -> anyhow::Result<Vec<ToolDefinition>> {
        let mut tools = Vec::new();

        // Look for plugins/*/agents/*.md files
        let plugins_dir = self.agents_dir.join("plugins");
        if plugins_dir.exists() {
            let mut entries = tokio::fs::read_dir(&plugins_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                if entry.file_type().await?.is_dir() {
                    let plugin_name = entry.file_name().to_string_lossy().to_string();
                    let agents_subdir = entry.path().join("agents");

                    if agents_subdir.exists() {
                        let mut agent_entries = tokio::fs::read_dir(&agents_subdir).await?;
                        while let Some(agent_entry) = agent_entries.next_entry().await? {
                            let path = agent_entry.path();
                            if path.extension().map(|e| e == "md").unwrap_or(false) {
                                let agent_name = path
                                    .file_stem()
                                    .map(|s| s.to_string_lossy().to_string())
                                    .unwrap_or_default();

                                tools.push(ToolDefinition {
                                    name: format!(
                                        "llm_agent_{}_{}",
                                        plugin_name.replace('-', "_"),
                                        agent_name.replace('-', "_")
                                    ),
                                    description: format!(
                                        "LLM agent: {} / {}",
                                        plugin_name, agent_name
                                    ),
                                    input_schema: simd_json::json!({
                                        "type": "object",
                                        "properties": {
                                            "prompt": {
                                                "type": "string",
                                                "description": "Prompt for the LLM agent"
                                            },
                                            "context": {
                                                "type": "object",
                                                "description": "Additional context"
                                            }
                                        },
                                        "required": ["prompt"]
                                    }),
                                    schema_version: "https://json-schema.org/draft/next/schema"
                                        .to_string(),
                                    category: "llm_agent".to_string(),
                                    tags: vec![
                                        "llm".to_string(),
                                        "agent".to_string(),
                                        plugin_name.clone(),
                                    ],
                                    namespace: "system.v1".to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(tools)
    }
}
