//! GCloud CLI introspection tools.
//!
//! These tools provide access to the gcloud CLI command hierarchy,
//! allowing agents to discover and understand gcloud commands, flags,
//! and arguments.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use op_inspector::{GCloudCommand, GCloudParser, GCloudSchema};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{Tool, ToolRegistry};

/// Cached gcloud schema to avoid re-introspection
struct GCloudCache {
    schema: Option<GCloudSchema>,
}

impl GCloudCache {
    fn new() -> Self {
        Self { schema: None }
    }
}

pub async fn register_gcloud_tools(registry: &ToolRegistry) -> Result<()> {
    let parser = Arc::new(GCloudParser::new());
    let cache = Arc::new(RwLock::new(GCloudCache::new()));

    registry
        .register_tool(Arc::new(GCloudIntrospectTool::new(
            parser.clone(),
            cache.clone(),
        )))
        .await?;
    registry
        .register_tool(Arc::new(GCloudListGroupsTool::new(
            parser.clone(),
            cache.clone(),
        )))
        .await?;
    registry
        .register_tool(Arc::new(GCloudGetCommandTool::new(
            parser.clone(),
            cache.clone(),
        )))
        .await?;
    registry
        .register_tool(Arc::new(GCloudSearchTool::new(
            parser.clone(),
            cache.clone(),
        )))
        .await?;

    tracing::info!("Registered 4 gcloud introspection tools");
    Ok(())
}

// Helper to get or populate the cache
async fn get_cached_schema(
    parser: &GCloudParser,
    cache: &RwLock<GCloudCache>,
    max_depth: usize,
) -> Result<GCloudSchema> {
    // Check cache first
    {
        let cache_read = cache.read().await;
        if let Some(ref schema) = cache_read.schema {
            return Ok(schema.clone());
        }
    }

    // Introspect and cache
    let schema = parser.introspect_full(max_depth).await?;
    {
        let mut cache_write = cache.write().await;
        cache_write.schema = Some(schema.clone());
    }

    Ok(schema)
}

// Helper to find a command by path
fn find_command<'a>(root: &'a GCloudCommand, path: &[String]) -> Option<&'a GCloudCommand> {
    let mut current = root;
    for part in path {
        current = current.subcommands.get(part)?;
    }
    Some(current)
}

// Helper to collect all commands matching a pattern
fn search_commands(
    cmd: &GCloudCommand,
    pattern: &str,
    results: &mut Vec<Value>,
    max_results: usize,
) {
    if results.len() >= max_results {
        return;
    }

    let pattern_lower = pattern.to_lowercase();

    // Check if this command matches
    if cmd.name.to_lowercase().contains(&pattern_lower)
        || cmd.description.to_lowercase().contains(&pattern_lower)
        || cmd.full_path.to_lowercase().contains(&pattern_lower)
    {
        results.push(json!({
            "name": cmd.name,
            "full_path": cmd.full_path,
            "description": cmd.description,
            "is_group": cmd.is_group,
            "flag_count": cmd.flags.len(),
        }));
    }

    // Search in flags
    for flag in &cmd.flags {
        if results.len() >= max_results {
            return;
        }
        if flag.name.to_lowercase().contains(&pattern_lower)
            || flag.description.to_lowercase().contains(&pattern_lower)
        {
            results.push(json!({
                "type": "flag",
                "command": cmd.full_path,
                "flag": flag.name,
                "description": flag.description,
                "required": flag.required,
                "value_type": flag.value_type,
            }));
        }
    }

    // Recurse into subcommands
    for subcmd in cmd.subcommands.values() {
        search_commands(subcmd, pattern, results, max_results);
    }
}

// =============================================================================
// TOOLS
// =============================================================================

struct GCloudIntrospectTool {
    parser: Arc<GCloudParser>,
    cache: Arc<RwLock<GCloudCache>>,
}

impl GCloudIntrospectTool {
    fn new(parser: Arc<GCloudParser>, cache: Arc<RwLock<GCloudCache>>) -> Self {
        Self { parser, cache }
    }
}

#[async_trait]
impl Tool for GCloudIntrospectTool {
    fn name(&self) -> &str {
        "gcloud_introspect"
    }

    fn description(&self) -> &str {
        "Introspect the gcloud CLI command hierarchy. Returns full schema with all commands, groups, flags, and arguments."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "max_depth": {
                    "type": "integer",
                    "description": "Maximum depth for recursive introspection (default: 3)",
                    "default": 3,
                    "minimum": 1,
                    "maximum": 10
                },
                "refresh": {
                    "type": "boolean",
                    "description": "Force re-introspection even if cached (default: false)",
                    "default": false
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let max_depth = input.get("max_depth").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
        let refresh = input
            .get("refresh")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Clear cache if refresh requested
        if refresh {
            let mut cache_write = self.cache.write().await;
            cache_write.schema = None;
        }

        let schema = get_cached_schema(&self.parser, &self.cache, max_depth).await?;

        Ok(json!({
            "schema_version": schema.schema_version,
            "gcloud_version": schema.gcloud_version,
            "account": schema.account,
            "statistics": {
                "total_groups": schema.statistics.total_groups,
                "total_commands": schema.statistics.total_commands,
                "total_flags": schema.statistics.total_flags,
                "introspection_time_ms": schema.statistics.introspection_time_ms,
            },
            "hierarchy": simd_json::serde::to_owned_value(&schema.hierarchy)?
        }))
    }

    fn category(&self) -> &str {
        "gcloud"
    }

    fn tags(&self) -> Vec<String> {
        vec!["gcloud".into(), "introspection".into(), "cli".into()]
    }
}

struct GCloudListGroupsTool {
    parser: Arc<GCloudParser>,
    cache: Arc<RwLock<GCloudCache>>,
}

impl GCloudListGroupsTool {
    fn new(parser: Arc<GCloudParser>, cache: Arc<RwLock<GCloudCache>>) -> Self {
        Self { parser, cache }
    }
}

#[async_trait]
impl Tool for GCloudListGroupsTool {
    fn name(&self) -> &str {
        "gcloud_list_groups"
    }

    fn description(&self) -> &str {
        "List gcloud command groups at a given path. Use empty path for top-level groups."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Command path (e.g., ['compute', 'instances']). Empty for root.",
                    "default": []
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let path: Vec<String> = input
            .get("path")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let schema = get_cached_schema(&self.parser, &self.cache, 3).await?;
        let cmd = find_command(&schema.hierarchy, &path)
            .ok_or_else(|| anyhow!("Command path not found: {:?}", path))?;

        let groups: Vec<Value> = cmd
            .subcommands
            .values()
            .filter(|c| c.is_group)
            .map(|c| {
                json!({
                    "name": c.name,
                    "full_path": c.full_path,
                    "description": c.description,
                    "subcommand_count": c.subcommands.len(),
                })
            })
            .collect();

        let commands: Vec<Value> = cmd
            .subcommands
            .values()
            .filter(|c| !c.is_group)
            .map(|c| {
                json!({
                    "name": c.name,
                    "full_path": c.full_path,
                    "description": c.description,
                    "flag_count": c.flags.len(),
                })
            })
            .collect();

        Ok(json!({
            "path": path,
            "full_path": cmd.full_path,
            "groups": groups,
            "commands": commands,
            "group_count": groups.len(),
            "command_count": commands.len(),
        }))
    }

    fn category(&self) -> &str {
        "gcloud"
    }

    fn tags(&self) -> Vec<String> {
        vec!["gcloud".into(), "discovery".into()]
    }
}

struct GCloudGetCommandTool {
    parser: Arc<GCloudParser>,
    cache: Arc<RwLock<GCloudCache>>,
}

impl GCloudGetCommandTool {
    fn new(parser: Arc<GCloudParser>, cache: Arc<RwLock<GCloudCache>>) -> Self {
        Self { parser, cache }
    }
}

#[async_trait]
impl Tool for GCloudGetCommandTool {
    fn name(&self) -> &str {
        "gcloud_get_command"
    }

    fn description(&self) -> &str {
        "Get detailed information about a specific gcloud command, including all flags and arguments."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Command path (e.g., ['compute', 'instances', 'create'])"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let path: Vec<String> = input
            .get("path")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .ok_or_else(|| anyhow!("Missing required parameter: path"))?;

        let schema = get_cached_schema(&self.parser, &self.cache, 3).await?;
        let cmd = find_command(&schema.hierarchy, &path)
            .ok_or_else(|| anyhow!("Command not found: {:?}", path))?;

        let flags: Vec<Value> = cmd
            .flags
            .iter()
            .map(|f| {
                json!({
                    "name": f.name,
                    "short_name": f.short_name,
                    "description": f.description,
                    "required": f.required,
                    "value_type": f.value_type,
                    "default": f.default,
                    "choices": f.choices,
                })
            })
            .collect();

        let positional_args: Vec<Value> = cmd
            .positional_args
            .iter()
            .map(|a| {
                json!({
                    "name": a.name,
                    "description": a.description,
                    "required": a.required,
                })
            })
            .collect();

        let subcommands: Vec<String> = cmd.subcommands.keys().cloned().collect();

        Ok(json!({
            "name": cmd.name,
            "full_path": cmd.full_path,
            "description": cmd.description,
            "is_group": cmd.is_group,
            "flags": flags,
            "positional_args": positional_args,
            "subcommands": subcommands,
            "flag_count": flags.len(),
            "subcommand_count": subcommands.len(),
        }))
    }

    fn category(&self) -> &str {
        "gcloud"
    }

    fn tags(&self) -> Vec<String> {
        vec!["gcloud".into(), "command".into()]
    }
}

struct GCloudSearchTool {
    parser: Arc<GCloudParser>,
    cache: Arc<RwLock<GCloudCache>>,
}

impl GCloudSearchTool {
    fn new(parser: Arc<GCloudParser>, cache: Arc<RwLock<GCloudCache>>) -> Self {
        Self { parser, cache }
    }
}

#[async_trait]
impl Tool for GCloudSearchTool {
    fn name(&self) -> &str {
        "gcloud_search"
    }

    fn description(&self) -> &str {
        "Search gcloud commands and flags by keyword. Searches names and descriptions."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query (searches command names, descriptions, and flags)"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 20)",
                    "default": 20,
                    "minimum": 1,
                    "maximum": 100
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required parameter: query"))?;

        let max_results = input
            .get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as usize;

        let schema = get_cached_schema(&self.parser, &self.cache, 3).await?;

        let mut results = Vec::new();
        search_commands(&schema.hierarchy, query, &mut results, max_results);

        Ok(json!({
            "query": query,
            "result_count": results.len(),
            "results": results,
        }))
    }

    fn category(&self) -> &str {
        "gcloud"
    }

    fn tags(&self) -> Vec<String> {
        vec!["gcloud".into(), "search".into()]
    }
}
