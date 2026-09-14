//! Compact Mode - Reduces 750+ tools to 4-5 meta-tools
//!
//! Instead of exposing every tool directly (which consumes massive context window),
//! compact mode exposes just a few meta-tools:
//!
//! 1. `list_tools` - List available tools with filtering
//! 2. `execute_tool` - Execute any tool by name
//! 3. `get_tool_schema` - Get schema for a specific tool
//! 4. `search_tools` - Search tools by keyword
//!
//! This design:
//! - Saves ~95% of context tokens
//! - Bypasses Cursor's 40-tool limit entirely
//! - Keeps all tools accessible via execute_tool
//! - Improves LLM reasoning (fewer choices = better decisions)

use crate::aggregator::Aggregator;
use crate::client::ToolDefinition;
use anyhow::Result;
use async_trait::async_trait;
use op_tools::tool::{SecurityLevel, Tool};
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tracing::{debug, info};

/// Compact mode configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactModeConfig {
    /// Whether compact mode is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Include list_tools meta-tool
    #[serde(default = "default_true")]
    pub include_list: bool,

    /// Include execute_tool meta-tool
    #[serde(default = "default_true")]
    pub include_execute: bool,

    /// Include get_tool_schema meta-tool
    #[serde(default = "default_true")]
    pub include_schema: bool,

    /// Include search_tools meta-tool
    #[serde(default = "default_true")]
    pub include_search: bool,

    /// Include batch_execute meta-tool
    #[serde(default)]
    pub include_batch: bool,

    /// Maximum tools to return in list_tools (for context savings)
    #[serde(default = "default_max_list")]
    pub max_list_results: usize,

    /// Default profile for tool execution
    #[serde(default)]
    pub default_profile: Option<String>,
}

fn default_true() -> bool {
    true
}
fn default_max_list() -> usize {
    50
}

impl Default for CompactModeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            include_list: true,
            include_execute: true,
            include_schema: true,
            include_search: true,
            include_batch: false,
            max_list_results: 50,
            default_profile: None,
        }
    }
}

/// Create compact mode tools
pub fn create_compact_tools(
    aggregator: Arc<Aggregator>,
    config: &CompactModeConfig,
) -> Vec<Arc<dyn Tool>> {
    let mut tools: Vec<Arc<dyn Tool>> = Vec::new();

    if config.include_list {
        tools.push(Arc::new(ListToolsTool::new(
            aggregator.clone(),
            config.max_list_results,
        )));
    }

    if config.include_execute {
        tools.push(Arc::new(ExecuteToolTool::new(aggregator.clone())));
    }

    if config.include_schema {
        tools.push(Arc::new(GetToolSchemaTool::new(aggregator.clone())));
    }

    if config.include_search {
        tools.push(Arc::new(SearchToolsTool::new(
            aggregator.clone(),
            config.max_list_results,
        )));
    }

    if config.include_batch {
        tools.push(Arc::new(BatchExecuteTool::new(aggregator.clone())));
    }

    info!("Created {} compact mode meta-tools", tools.len());
    tools
}

// ============================================================================
// META-TOOL 1: list_tools
// ============================================================================

/// Lists available tools with optional filtering
pub struct ListToolsTool {
    aggregator: Arc<Aggregator>,
    max_results: usize,
}

impl ListToolsTool {
    pub fn new(aggregator: Arc<Aggregator>, max_results: usize) -> Self {
        Self {
            aggregator,
            max_results,
        }
    }
}

#[async_trait]
impl Tool for ListToolsTool {
    fn name(&self) -> &str {
        "list_tools"
    }

    fn description(&self) -> &str {
        "List available tools. Use 'category' or 'namespace' to filter. Returns tool names and descriptions. Call 'get_tool_schema' to get full input schema before executing a tool."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "description": "Filter by category (e.g., 'systemd', 'network', 'filesystem')"
                },
                "namespace": {
                    "type": "string",
                    "description": "Filter by namespace (e.g., 'system', 'dbus', 'external')"
                },
                "profile": {
                    "type": "string",
                    "description": "Profile to list tools from (default: current profile)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of tools to return",
                    "default": 20
                }
            },
            "additionalProperties": false
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let category = input
            .as_object()
            .and_then(|obj| obj.get("category"))
            .and_then(|v| v.as_str());
        let namespace = input
            .as_object()
            .and_then(|obj| obj.get("namespace"))
            .and_then(|v| v.as_str());
        let profile = input
            .as_object()
            .and_then(|obj| obj.get("profile"))
            .and_then(|v| v.as_str())
            .unwrap_or(self.aggregator.default_profile());
        let limit = input
            .as_object()
            .and_then(|obj| obj.get("limit"))
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as usize;

        let limit = limit.min(self.max_results);

        debug!(
            "list_tools: profile={}, category={:?}, namespace={:?}, limit={}",
            profile, category, namespace, limit
        );

        let all_tools = self.aggregator.list_tools(profile).await?;

        // Filter
        let filtered: Vec<&ToolDefinition> = all_tools
            .iter()
            .filter(|t| {
                if let Some(cat) = category {
                    let tool_cat = t
                        .annotations
                        .as_ref()
                        .and_then(|a| a.as_object())
                        .and_then(|obj| obj.get("category"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("general");
                    if tool_cat != cat {
                        return false;
                    }
                }
                if let Some(ns) = namespace {
                    let tool_ns = t
                        .annotations
                        .as_ref()
                        .and_then(|a| a.as_object())
                        .and_then(|obj| obj.get("namespace"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("system");
                    if tool_ns != ns {
                        return false;
                    }
                }
                true
            })
            .take(limit)
            .collect();

        // Return compact format (name + description only, no schemas)
        let tools_list: Vec<Value> = filtered
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description.as_str(),
                    "category": t.annotations.as_ref()
                        .and_then(|a| a.as_object())
                        .and_then(|obj| obj.get("category"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("general")
                })
            })
            .collect();

        Ok(json!({
            "tools": tools_list,
            "count": filtered.len(),
            "total_available": all_tools.len(),
            "profile": profile,
            "hint": "Use 'get_tool_schema' to get the input schema before calling 'execute_tool'"
        }))
    }

    fn category(&self) -> &str {
        "meta"
    }
    fn namespace(&self) -> &str {
        "compact"
    }
    fn security_level(&self) -> SecurityLevel {
        SecurityLevel::ReadOnly
    }
}

// ============================================================================
// META-TOOL 2: execute_tool
// ============================================================================

/// Executes any tool by name
pub struct ExecuteToolTool {
    aggregator: Arc<Aggregator>,
}

impl ExecuteToolTool {
    pub fn new(aggregator: Arc<Aggregator>) -> Self {
        Self { aggregator }
    }
}

#[async_trait]
impl Tool for ExecuteToolTool {
    fn name(&self) -> &str {
        "execute_tool"
    }

    fn description(&self) -> &str {
        "Execute any available tool by name. First use 'list_tools' to see available tools, then 'get_tool_schema' to see required arguments."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tool_name": {
                    "type": "string",
                    "description": "Name of the tool to execute"
                },
                "arguments": {
                    "type": "object",
                    "description": "Arguments to pass to the tool (use get_tool_schema to see required args)"
                }
            },
            "required": ["tool_name"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let tool_name = input
            .as_object()
            .and_then(|obj| obj.get("tool_name"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("tool_name is required"))?;

        let arguments = input
            .as_object()
            .and_then(|obj| obj.get("arguments"))
            .cloned()
            .unwrap_or(json!({}));

        debug!("execute_tool: {} with args {:?}", tool_name, arguments);

        let result = self.aggregator.call_tool(tool_name, arguments).await?;

        Ok(json!({
            "tool": tool_name,
            "result": result.result,
            "server": result.server_id,
            "success": !result.is_error
        }))
    }

    fn category(&self) -> &str {
        "meta"
    }
    fn namespace(&self) -> &str {
        "compact"
    }
    fn security_level(&self) -> SecurityLevel {
        SecurityLevel::Elevated
    }
}

// ============================================================================
// META-TOOL 3: get_tool_schema
// ============================================================================

/// Gets the full schema for a specific tool
pub struct GetToolSchemaTool {
    aggregator: Arc<Aggregator>,
}

impl GetToolSchemaTool {
    pub fn new(aggregator: Arc<Aggregator>) -> Self {
        Self { aggregator }
    }
}

#[async_trait]
impl Tool for GetToolSchemaTool {
    fn name(&self) -> &str {
        "get_tool_schema"
    }

    fn description(&self) -> &str {
        "Get the full input schema for a tool. Use this before calling execute_tool to understand required and optional arguments."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "tool_name": {
                    "type": "string",
                    "description": "Name of the tool to get schema for"
                }
            },
            "required": ["tool_name"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let tool_name = input
            .as_object()
            .and_then(|obj| obj.get("tool_name"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("tool_name is required"))?;

        debug!("get_tool_schema: {}", tool_name);

        // Search for the tool in cache
        let (tool_def, server_id) = self
            .aggregator
            .cache()
            .get(tool_name)
            .await
            .ok_or_else(|| anyhow::anyhow!("Tool '{}' not found", tool_name))?;

        Ok(json!({
            "tool": tool_name,
            "description": tool_def.description,
            "input_schema": tool_def.input_schema,
            "server": server_id,
            "annotations": tool_def.annotations
        }))
    }

    fn category(&self) -> &str {
        "meta"
    }
    fn namespace(&self) -> &str {
        "compact"
    }
    fn security_level(&self) -> SecurityLevel {
        SecurityLevel::ReadOnly
    }
}

// ============================================================================
// META-TOOL 4: search_tools
// ============================================================================

/// Searches tools by keyword in name or description
pub struct SearchToolsTool {
    aggregator: Arc<Aggregator>,
    max_results: usize,
}

impl SearchToolsTool {
    pub fn new(aggregator: Arc<Aggregator>, max_results: usize) -> Self {
        Self {
            aggregator,
            max_results,
        }
    }
}

#[async_trait]
impl Tool for SearchToolsTool {
    fn name(&self) -> &str {
        "search_tools"
    }

    fn description(&self) -> &str {
        "Search for tools by keyword. Searches tool names and descriptions. Use this to find relevant tools for a task."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (searches in tool names and descriptions)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum results to return",
                    "default": 10
                }
            },
            "required": ["query"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let query = input
            .as_object()
            .and_then(|obj| obj.get("query"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("query is required"))?
            .to_lowercase();

        let limit = input
            .as_object()
            .and_then(|obj| obj.get("limit"))
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as usize;
        let limit = limit.min(self.max_results);

        debug!("search_tools: query='{}', limit={}", query, limit);

        let all_tools = self.aggregator.list_default_tools().await?;

        // Score and filter tools
        let mut scored: Vec<(i32, &ToolDefinition)> = all_tools
            .iter()
            .filter_map(|t| {
                let name_lower = t.name.to_lowercase();
                let desc_lower = t.description.to_lowercase();

                let mut score = 0;

                // Exact name match
                if name_lower == query {
                    score += 100;
                }
                // Name contains query
                else if name_lower.contains(&query) {
                    score += 50;
                }
                // Description contains query
                if desc_lower.contains(&query) {
                    score += 20;
                }
                // Word boundary match in name
                if name_lower.split('_').any(|w| w == query) {
                    score += 30;
                }

                if score > 0 {
                    Some((score, t))
                } else {
                    None
                }
            })
            .collect();

        // Sort by score descending
        scored.sort_by_key(|b| std::cmp::Reverse(b.0));

        let results: Vec<Value> = scored
            .iter()
            .take(limit)
            .map(|(score, t)| {
                json!({
                    "name": t.name,
                    "description": t.description.as_str(),
                    "relevance": score
                })
            })
            .collect();

        Ok(json!({
            "query": query,
            "results": results,
            "count": results.len(),
            "hint": "Use 'get_tool_schema' to see arguments, then 'execute_tool' to run"
        }))
    }

    fn category(&self) -> &str {
        "meta"
    }
    fn namespace(&self) -> &str {
        "compact"
    }
    fn security_level(&self) -> SecurityLevel {
        SecurityLevel::ReadOnly
    }
}

// ============================================================================
// META-TOOL 5: batch_execute (optional)
// ============================================================================

/// Executes multiple tools in sequence
pub struct BatchExecuteTool {
    aggregator: Arc<Aggregator>,
}

impl BatchExecuteTool {
    pub fn new(aggregator: Arc<Aggregator>) -> Self {
        Self { aggregator }
    }
}

#[async_trait]
impl Tool for BatchExecuteTool {
    fn name(&self) -> &str {
        "batch_execute"
    }

    fn description(&self) -> &str {
        "Execute multiple tools in sequence. Useful for multi-step operations. Each tool runs with its own arguments. If any tool fails, subsequent tools still run."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operations": {
                    "type": "array",
                    "description": "List of tool operations to execute",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool_name": {
                                "type": "string",
                                "description": "Name of tool to execute"
                            },
                            "arguments": {
                                "type": "object",
                                "description": "Arguments for this tool"
                            }
                        },
                        "required": ["tool_name"]
                    }
                },
                "stop_on_error": {
                    "type": "boolean",
                    "description": "Stop execution if a tool fails",
                    "default": false
                }
            },
            "required": ["operations"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let operations = input
            .as_object()
            .and_then(|obj| obj.get("operations"))
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("operations array is required"))?;

        let stop_on_error = input
            .as_object()
            .and_then(|obj| obj.get("stop_on_error"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        debug!(
            "batch_execute: {} operations, stop_on_error={}",
            operations.len(),
            stop_on_error
        );

        let mut results = Vec::new();
        let mut all_succeeded = true;

        for (i, op) in operations.as_slice().iter().enumerate() {
            let tool_name = op
                .as_object()
                .and_then(|obj| obj.get("tool_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            let arguments = op
                .as_object()
                .and_then(|obj| obj.get("arguments"))
                .cloned()
                .unwrap_or(json!({}));

            match self.aggregator.call_tool(tool_name, arguments).await {
                Ok(result) => {
                    results.push(json!({
                        "index": i,
                        "tool": tool_name,
                        "success": true,
                        "result": result.result
                    }));
                }
                Err(e) => {
                    all_succeeded = false;
                    results.push(json!({
                        "index": i,
                        "tool": tool_name,
                        "success": false,
                        "error": e.to_string()
                    }));

                    if stop_on_error {
                        break;
                    }
                }
            }
        }

        Ok(json!({
            "results": results,
            "total": operations.len(),
            "succeeded": results.iter().filter(|r| r.as_object().and_then(|obj| obj.get("success")) == Some(&json!(true))).count(),
            "all_succeeded": all_succeeded
        }))
    }

    fn category(&self) -> &str {
        "meta"
    }
    fn namespace(&self) -> &str {
        "compact"
    }
    fn security_level(&self) -> SecurityLevel {
        SecurityLevel::Elevated
    }
}

/// Summary of compact mode tools for documentation
pub fn compact_mode_summary() -> Value {
    json!({
        "mode": "compact",
        "description": "Reduces 750+ tools to 4-5 meta-tools for context efficiency",
        "tools": [
            {
                "name": "list_tools",
                "purpose": "Browse available tools by category/namespace"
            },
            {
                "name": "search_tools",
                "purpose": "Find tools by keyword search"
            },
            {
                "name": "get_tool_schema",
                "purpose": "Get input schema before executing a tool"
            },
            {
                "name": "execute_tool",
                "purpose": "Execute any tool by name with arguments"
            },
            {
                "name": "batch_execute",
                "purpose": "Run multiple tools in sequence (optional)"
            }
        ],
        "workflow": [
            "1. Use list_tools or search_tools to find relevant tools",
            "2. Use get_tool_schema to see required arguments",
            "3. Use execute_tool to run the tool"
        ],
        "benefits": [
            "~95% context token savings",
            "Bypasses 40-tool limit",
            "Clearer LLM reasoning",
            "All tools still accessible"
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AggregatorConfig;

    #[tokio::test]
    async fn test_compact_mode_config_default() {
        let config = CompactModeConfig::default();
        assert!(config.enabled);
        assert!(config.include_list);
        assert!(config.include_execute);
        assert!(config.include_schema);
        assert!(config.include_search);
        assert!(!config.include_batch);
        assert_eq!(config.max_list_results, 50);
    }

    #[tokio::test]
    async fn test_create_compact_tools() {
        let agg_config = AggregatorConfig::default();
        let aggregator = Arc::new(Aggregator::new(agg_config).await.unwrap());

        let compact_config = CompactModeConfig::default();
        let tools = create_compact_tools(aggregator, &compact_config);

        // Should have 4 tools (batch disabled by default)
        assert_eq!(tools.len(), 4);

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"list_tools"));
        assert!(names.contains(&"execute_tool"));
        assert!(names.contains(&"get_tool_schema"));
        assert!(names.contains(&"search_tools"));
    }

    #[test]
    fn test_compact_mode_summary() {
        let summary = compact_mode_summary();
        assert_eq!(
            summary.as_object().and_then(|obj| obj.get("mode")).unwrap(),
            "compact"
        );
        assert!(
            summary
                .as_object()
                .and_then(|obj| obj.get("tools"))
                .unwrap()
                .as_array()
                .unwrap()
                .len()
                >= 4
        );
    }
}
