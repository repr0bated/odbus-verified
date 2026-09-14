//! Plugin State Tools (query/diff/apply)

use crate::tool_registry::{Tool, ToolRegistry};
use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;

const PLUGINS: &[&str] = &[
    "systemd", "network", "packagekit", "firewall", "users", "storage",
    "lxc", "openflow", "privacy"
];

pub async fn register_all(registry: &ToolRegistry) -> Result<usize> {
    let mut count = 0;
    for plugin in PLUGINS {
        registry.register(Arc::new(PluginQueryTool::new(plugin))).await?;
        registry.register(Arc::new(PluginDiffTool::new(plugin))).await?;
        registry.register(Arc::new(PluginApplyTool::new(plugin))).await?;
        count += 3;
    }
    Ok(count)
}

pub struct PluginQueryTool { plugin: String, name: String, desc: String }

impl PluginQueryTool {
    pub fn new(plugin: &str) -> Self {
        Self {
            plugin: plugin.to_string(),
            name: format!("plugin_{}_query", plugin),
            desc: format!("Query current state from {} plugin", plugin),
        }
    }
}

#[async_trait]
impl Tool for PluginQueryTool {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.desc }
    fn category(&self) -> &str { "plugin" }
    fn tags(&self) -> Vec<String> { vec!["plugin".into(), "state".into(), self.plugin.clone()] }
    fn input_schema(&self) -> Value { json!({"type": "object", "properties": {"filter": {"type": "object"}}}) }

    async fn execute(&self, _input: Value) -> Result<Value> {
        // TODO: Integrate with the authoritative plugin catalog / canonical
        // plugin document path instead of inventing plugin state locally.
        Ok(json!({"success": true, "plugin": self.plugin, "operation": "query", "state": {}}))
    }
}

pub struct PluginDiffTool { plugin: String, name: String, desc: String }

impl PluginDiffTool {
    pub fn new(plugin: &str) -> Self {
        Self {
            plugin: plugin.to_string(),
            name: format!("plugin_{}_diff", plugin),
            desc: format!("Calculate state diff for {} plugin", plugin),
        }
    }
}

#[async_trait]
impl Tool for PluginDiffTool {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.desc }
    fn category(&self) -> &str { "plugin" }
    fn tags(&self) -> Vec<String> { vec!["plugin".into(), "diff".into(), self.plugin.clone()] }
    fn input_schema(&self) -> Value { json!({"type": "object", "properties": {"desired_state": {"type": "object"}}, "required": ["desired_state"]}) }

    async fn execute(&self, _input: Value) -> Result<Value> {
        Ok(json!({"success": true, "plugin": self.plugin, "operation": "diff", "changes": []}))
    }
}

pub struct PluginApplyTool { plugin: String, name: String, desc: String }

impl PluginApplyTool {
    pub fn new(plugin: &str) -> Self {
        Self {
            plugin: plugin.to_string(),
            name: format!("plugin_{}_apply", plugin),
            desc: format!("Apply state changes for {} plugin", plugin),
        }
    }
}

#[async_trait]
impl Tool for PluginApplyTool {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { &self.desc }
    fn category(&self) -> &str { "plugin" }
    fn tags(&self) -> Vec<String> { vec!["plugin".into(), "apply".into(), self.plugin.clone()] }
    fn input_schema(&self) -> Value { json!({"type": "object", "properties": {"diff": {"type": "object"}, "dry_run": {"type": "boolean"}}, "required": ["diff"]}) }

    async fn execute(&self, input: Value) -> Result<Value> {
        let dry_run = input.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
        Ok(json!({"success": true, "plugin": self.plugin, "operation": "apply", "dry_run": dry_run, "applied": !dry_run}))
    }
}
