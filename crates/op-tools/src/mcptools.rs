//! MCP Tools integration via the mcptools CLI.
//!
//! Configuration is provided via environment variables or a JSON config file:
//! - OP_MCPTOOLS_CONFIG: Path to JSON config (default: "mcptools.json")
//! - OP_MCPTOOLS_BIN: Path to mcptools binary (default: "mcp")
//! - OP_MCPTOOLS_SERVERS: JSON array of server configs
//!   Example:
//! ```
//! [
//! {
//! "name": "github",
//! "args": ["https://api.example.com/mcp"],
//! "transport": "http",
//! "auth_header": "Bearer TOKEN",
//! "tool_prefix": "mcp_github_"
//! }
//! ]
//! ```
//! - OP_MCPTOOLS_SERVER: Single server command (space-separated) as a fallback
//! - OP_MCPTOOLS_SERVER_NAME: Optional name for OP_MCPTOOLS_SERVER (default: "default")
//! - OP_MCPTOOLS_ALLOW_UNPREFIXED: "true" to allow raw tool names (fallback to prefixed on conflict)

use anyhow::{Context, Result};
use serde::Deserialize;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::sync::Arc;
use tokio::process::Command;
use tracing::{debug, info, warn};

use crate::registry::{ToolDefinition, ToolRegistry};
use crate::tool::Tool;

#[derive(Debug, Clone, Deserialize)]
struct McpToolsServerConfig {
    name: String,
    args: Vec<String>,
    #[serde(default)]
    transport: Option<String>,
    #[serde(default)]
    auth_header: Option<String>,
    #[serde(default)]
    auth_user: Option<String>,
    #[serde(default)]
    tool_prefix: Option<String>,
    #[serde(default)]
    env: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone)]
struct McpToolSpec {
    name: String,
    description: String,
    input_schema: Value,
}

#[derive(Debug, Clone, Deserialize)]
struct McpToolsConfig {
    #[serde(default)]
    allow_unprefixed_names: bool,
    #[serde(default)]
    servers: Vec<McpToolsServerConfig>,
}

pub async fn register_mcp_tools(registry: &ToolRegistry) -> Result<usize> {
    let config = load_mcp_config()?;
    if config.servers.is_empty() {
        return Ok(0);
    }

    let mcp_bin = env::var("OP_MCPTOOLS_BIN").unwrap_or_else(|_| "mcp".to_string());
    let mut registered = 0usize;

    for server in config.servers {
        let tools = match list_mcp_tools(&mcp_bin, &server).await {
            Ok(tools) => tools,
            Err(err) => {
                warn!(
                    "Skipping MCP server '{}' due to list error: {}",
                    server.name, err
                );
                continue;
            }
        };

        for tool in tools {
            let desired_name = select_tool_name(&server, &tool.name, config.allow_unprefixed_names);
            let tool_name =
                match resolve_tool_name_conflict(registry, &server, &tool.name, desired_name).await
                {
                    Some(name) => name,
                    None => continue,
                };

            let description = if tool.description.is_empty() {
                format!("MCP tool from {}", server.name)
            } else {
                format!("{} (MCP server: {})", tool.description, server.name)
            };

            let tool = Arc::new(McpTool {
                name: tool_name.clone(),
                description: description.clone(),
                input_schema: tool.input_schema.clone(),
                namespace: format!("mcp.{}", sanitize_name(&server.name)),
                server: Arc::new(server.clone()),
                remote_tool_name: tool.name.clone(),
                mcp_bin: mcp_bin.clone(),
            });

            let definition = ToolDefinition {
                name: tool_name.clone(),
                description: description.clone(),
                input_schema: tool.input_schema.clone(),
                schema_version: "https://json-schema.org/draft/next/schema".to_string(),
                category: "mcp".to_string(),
                tags: vec!["mcp".to_string(), server.name.clone()],
                namespace: format!("mcp.{}", sanitize_name(&server.name)),
            };

            registry
                .register(Arc::from(tool_name.as_str()), tool, definition)
                .await?;
            registered += 1;
        }
    }

    info!("Registered {} MCP tools via mcptools", registered);
    Ok(registered)
}

#[derive(Clone)]
struct McpTool {
    name: String,
    description: String,
    input_schema: Value,
    namespace: String,
    server: Arc<McpToolsServerConfig>,
    remote_tool_name: String,
    mcp_bin: String,
}

#[async_trait::async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    fn category(&self) -> &str {
        "mcp"
    }

    fn namespace(&self) -> &str {
        &self.namespace
    }

    fn tags(&self) -> Vec<String> {
        vec!["mcp".to_string(), self.server.name.clone()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let params = simd_json::to_string(&input).context("Failed to serialize MCP params")?;
        let output =
            run_mcp_call(&self.mcp_bin, &self.server, &self.remote_tool_name, &params).await?;

        if output
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            let message = extract_text_content(&output)
                .unwrap_or_else(|| "MCP tool returned an error without text content".to_string());
            anyhow::bail!(message);
        }

        Ok(output)
    }
}

fn load_mcp_config() -> Result<McpToolsConfig> {
    let allow_unprefixed_names = env::var("OP_MCPTOOLS_ALLOW_UNPREFIXED")
        .ok()
        .map(parse_bool)
        .transpose()?
        .unwrap_or(false);

    if let Ok(raw) = env::var("OP_MCPTOOLS_SERVERS") {
        if raw.trim().is_empty() {
            return Ok(McpToolsConfig {
                allow_unprefixed_names,
                servers: Vec::new(),
            });
        }

        if let Ok(list) = serde_json::from_str::<Vec<McpToolsServerConfig>>(&raw) {
            return Ok(McpToolsConfig {
                allow_unprefixed_names,
                servers: list,
            });
        }

        let single = serde_json::from_str::<McpToolsServerConfig>(&raw)
            .context("OP_MCPTOOLS_SERVERS must be JSON (array or object)")?;
        return Ok(McpToolsConfig {
            allow_unprefixed_names,
            servers: vec![single],
        });
    }

    if let Some(config_path) = resolve_config_path() {
        let raw = std::fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read {}", config_path))?;
        let mut config: McpToolsConfig = serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse {}", config_path))?;
        if allow_unprefixed_names {
            config.allow_unprefixed_names = true;
        }
        return Ok(config);
    }

    if let Ok(raw) = env::var("OP_MCPTOOLS_SERVER") {
        let args = split_args(&raw);
        if args.is_empty() {
            return Ok(McpToolsConfig {
                allow_unprefixed_names,
                servers: Vec::new(),
            });
        }

        let name = env::var("OP_MCPTOOLS_SERVER_NAME").unwrap_or_else(|_| "default".to_string());
        return Ok(McpToolsConfig {
            allow_unprefixed_names,
            servers: vec![McpToolsServerConfig {
                name,
                args,
                transport: None,
                auth_header: None,
                auth_user: None,
                tool_prefix: None,
                env: None,
            }],
        });
    }

    Ok(McpToolsConfig {
        allow_unprefixed_names,
        servers: Vec::new(),
    })
}

async fn list_mcp_tools(mcp_bin: &str, server: &McpToolsServerConfig) -> Result<Vec<McpToolSpec>> {
    let mut cmd = Command::new(mcp_bin);
    cmd.arg("tools").arg("--format").arg("json");
    apply_server_args(&mut cmd, server);

    let output = cmd.output().await.context("Failed to run mcptools list")?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("mcptools tools failed: {}", stderr.trim());
    }

    let payload: Value = serde_json::from_str(&stdout)
        .with_context(|| format!("Failed to parse mcptools output: {}", stdout))?;
    let tools = payload
        .get("tools")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let mut parsed = Vec::new();
    for tool in tools {
        let name = tool
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        if name.is_empty() {
            continue;
        }
        let description = tool
            .get("description")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let input_schema = tool
            .get("inputSchema")
            .cloned()
            .or_else(|| tool.get("input_schema").cloned())
            .unwrap_or_else(|| json!({"type": "object"}));

        parsed.push(McpToolSpec {
            name,
            description,
            input_schema,
        });
    }

    Ok(parsed)
}

async fn run_mcp_call(
    mcp_bin: &str,
    server: &McpToolsServerConfig,
    tool_name: &str,
    params: &str,
) -> Result<Value> {
    let mut cmd = Command::new(mcp_bin);
    cmd.arg("call")
        .arg(tool_name)
        .arg("--format")
        .arg("json")
        .arg("--params")
        .arg(params);

    apply_server_args(&mut cmd, server);

    let output = cmd.output().await.context("Failed to run mcptools call")?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("mcptools call failed: {}", stderr.trim());
    }

    let payload: Value = serde_json::from_str(&stdout)
        .with_context(|| format!("Failed to parse mcptools output: {}", stdout))?;
    Ok(payload)
}

fn apply_server_args(cmd: &mut Command, server: &McpToolsServerConfig) {
    if let Some(transport) = &server.transport {
        cmd.arg("--transport").arg(transport);
    }

    if let Some(auth_header) = &server.auth_header {
        cmd.arg("--auth-header").arg(auth_header);
    }

    if let Some(auth_user) = &server.auth_user {
        cmd.arg("--auth-user").arg(auth_user);
    }

    if let Some(envs) = &server.env {
        cmd.envs(envs);
    }

    for arg in &server.args {
        cmd.arg(arg);
    }
}

fn resolve_config_path() -> Option<String> {
    let path = env::var("OP_MCPTOOLS_CONFIG").unwrap_or_else(|_| "mcptools.json".to_string());
    if Path::new(&path).is_file() {
        Some(path)
    } else {
        None
    }
}

fn split_args(raw: &str) -> Vec<String> {
    raw.split_whitespace()
        .map(|value| value.to_string())
        .collect()
}

fn sanitize_name(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn build_tool_name(server: &McpToolsServerConfig, tool_name: &str) -> String {
    if let Some(prefix) = &server.tool_prefix {
        format!("{}{}", prefix, sanitize_name(tool_name))
    } else {
        format!(
            "mcp_{}_{}",
            sanitize_name(&server.name),
            sanitize_name(tool_name)
        )
    }
}

fn select_tool_name(
    server: &McpToolsServerConfig,
    tool_name: &str,
    allow_unprefixed: bool,
) -> String {
    if allow_unprefixed {
        tool_name.to_string()
    } else {
        build_tool_name(server, tool_name)
    }
}

async fn resolve_tool_name_conflict(
    registry: &ToolRegistry,
    server: &McpToolsServerConfig,
    tool_name: &str,
    desired: String,
) -> Option<String> {
    if registry.get_definition(&desired).await.is_none() {
        return Some(desired);
    }

    let fallback = build_tool_name(server, tool_name);
    if fallback != desired && registry.get_definition(&fallback).await.is_none() {
        debug!(
            "Using prefixed name '{}' for MCP tool '{}' due to conflict",
            fallback, tool_name
        );
        return Some(fallback);
    }

    debug!(
        "Skipping MCP tool '{}' because names '{}' and '{}' already exist",
        tool_name, desired, fallback
    );
    None
}

fn parse_bool(raw: String) -> Result<bool> {
    let lowered = raw.trim().to_ascii_lowercase();
    match lowered.as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(anyhow::anyhow!("Invalid boolean value: {}", raw)),
    }
}

fn extract_text_content(payload: &Value) -> Option<String> {
    payload
        .get("content")
        .and_then(|value| value.as_array())
        .and_then(|items| items.first())
        .and_then(|item| item.get("text"))
        .and_then(|value| value.as_str())
        .map(|text| text.to_string())
}
