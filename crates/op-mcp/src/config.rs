
use anyhow::Result;
use config::{Config, ConfigError, Environment, File};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub name: String,
    pub version: String,
    pub tool_config: ToolConfig,
}

#[derive(Debug, Deserialize)]
pub struct ToolConfig {
    pub max_loaded_tools: usize,
    pub min_idle_secs: u64,
    pub enable_dbus_discovery: bool,
    pub enable_plugin_discovery: bool,
    pub enable_agent_discovery: bool,
    pub preload_essential: bool,
}

impl Settings {
    pub fn new() -> Result<Self, ConfigError> {
        let s = Config::builder()
            .add_source(File::with_name("config/default"))
            .add_source(Environment::with_prefix("MCP").separator("_"))
            .build()?;
        s.try_deserialize()
    }
}
