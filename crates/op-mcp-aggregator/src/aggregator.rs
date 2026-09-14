//! Main Aggregator - ties together clients, cache, and profiles
//!
//! This is the primary interface for the MCP aggregator.

use crate::cache::{cache_maintenance_loop, ToolCache};
use crate::client::{ClientManager, McpClient, ToolDefinition};
use crate::config::AggregatorConfig;
use crate::profile::ProfileManager;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

// Re-export ToolMode from config
pub use crate::config::ToolMode;

/// The main MCP Aggregator
pub struct Aggregator {
    /// Configuration
    config: AggregatorConfig,
    /// Client manager
    clients: Arc<ClientManager>,
    /// Tool cache
    cache: Arc<ToolCache>,
    /// Profile manager
    profiles: Arc<ProfileManager>,
    /// Whether the aggregator is initialized
    initialized: RwLock<bool>,
    /// Current client info (set during initialize)
    client_info: RwLock<Option<ClientInfo>>,
    /// Detected tool mode for current client
    detected_mode: RwLock<Option<ToolMode>>,
}

/// Client information from MCP initialize request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: Option<String>,
}

impl Aggregator {
    /// Create a new aggregator from configuration
    pub async fn new(config: AggregatorConfig) -> Result<Self> {
        let cache = Arc::new(ToolCache::new(
            config.cache.max_entries,
            config.cache.schema_ttl(),
        ));

        let clients = Arc::new(ClientManager::new());
        let profiles = Arc::new(ProfileManager::new(&config, cache.clone()));

        let aggregator = Self {
            config,
            clients,
            cache,
            profiles,
            initialized: RwLock::new(false),
            client_info: RwLock::new(None),
            detected_mode: RwLock::new(None),
        };

        Ok(aggregator)
    }

    /// Create from default configuration
    pub async fn from_default_config() -> Result<Self> {
        let config = AggregatorConfig::load_default()?;
        Self::new(config).await
    }

    /// Initialize the aggregator (connects to all servers)
    pub async fn initialize(&self) -> Result<()> {
        if *self.initialized.read().await {
            return Ok(());
        }

        info!(
            "Initializing MCP aggregator with {} servers",
            self.config.servers.len()
        );

        // Create clients for each configured server
        for server_config in &self.config.servers {
            if !server_config.enabled {
                info!("Skipping disabled server: {}", server_config.name);
                continue;
            }

            match McpClient::new(server_config.clone()) {
                Ok(client) => {
                    let client = Arc::new(client);

                    // Try to connect and fetch tools
                    match client.list_tools().await {
                        Ok(tools) => {
                            info!(
                                "Connected to {} with {} tools",
                                server_config.name,
                                tools.len()
                            );

                            // Cache the tools
                            self.cache.insert_batch(tools, &server_config.id).await;
                            self.clients.add_client(client).await;
                        }
                        Err(e) => {
                            error!("Failed to connect to {}: {}", server_config.name, e);
                            // Continue with other servers
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to create client for {}: {}", server_config.name, e);
                }
            }
        }

        // Start background cache maintenance if configured
        if self.config.cache.background_refresh {
            let cache = self.cache.clone();
            tokio::spawn(async move {
                cache_maintenance_loop(cache, Duration::from_secs(60)).await;
            });
        }

        *self.initialized.write().await = true;

        let stats = self.stats().await;
        info!(
            "Aggregator initialized: {} servers, {} tools cached",
            stats.connected_servers, stats.total_tools
        );

        Ok(())
    }

    /// List tools for a specific profile
    pub async fn list_tools(&self, profile_name: &str) -> Result<Vec<ToolDefinition>> {
        self.ensure_initialized().await?;
        Ok(self.profiles.get_tools_for_profile(profile_name).await)
    }

    /// List tools for the default profile
    pub async fn list_default_tools(&self) -> Result<Vec<ToolDefinition>> {
        self.list_tools(self.profiles.default_profile()).await
    }

    /// Call a tool by name
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<ToolCallResult> {
        self.ensure_initialized().await?;

        debug!("Calling tool: {}", name);

        // Find which server owns this tool
        let server_id = self
            .cache
            .get_server_id(name)
            .await
            .ok_or_else(|| anyhow!("Tool '{}' not found in any server", name))?;

        let client = self
            .clients
            .get_client(&server_id)
            .await
            .ok_or_else(|| anyhow!("Server '{}' not connected", server_id))?;

        // Call the tool
        let result = client
            .call_tool(name, arguments.clone())
            .await
            .with_context(|| format!("Failed to call tool '{}' on server '{}'", name, server_id))?;

        Ok(ToolCallResult {
            tool_name: name.to_string(),
            server_id,
            result,
            is_error: false,
        })
    }

    /// Call a tool with profile validation
    pub async fn call_tool_in_profile(
        &self,
        name: &str,
        arguments: Value,
        profile_name: &str,
    ) -> Result<ToolCallResult> {
        // Validate tool is available in profile
        if !self
            .profiles
            .tool_available_in_profile(name, profile_name)
            .await
        {
            return Err(anyhow!(
                "Tool '{}' not available in profile '{}'",
                name,
                profile_name
            ));
        }

        self.call_tool(name, arguments).await
    }

    /// Get available profiles
    pub async fn list_profiles(&self) -> Vec<String> {
        self.profiles.list_profiles().await
    }

    /// Get the default profile name
    pub fn default_profile(&self) -> &str {
        self.profiles.default_profile()
    }

    /// Refresh tools from all servers
    pub async fn refresh(&self) -> Result<()> {
        self.ensure_initialized().await?;

        info!("Refreshing tools from all servers");

        for client in self.clients.clients().await {
            match client.list_tools().await {
                Ok(tools) => {
                    self.cache.insert_batch(tools, client.server_id()).await;
                }
                Err(e) => {
                    warn!("Failed to refresh tools from {}: {}", client.server_id(), e);
                }
            }
        }

        Ok(())
    }

    /// Get aggregator statistics
    pub async fn stats(&self) -> AggregatorStats {
        let clients = self.clients.clients().await;
        let cache_stats = self.cache.stats().await;

        AggregatorStats {
            connected_servers: clients.len(),
            total_tools: self.cache.len().await,
            cache_hits: cache_stats.hits,
            cache_misses: cache_stats.misses,
            profiles: self.profiles.list_profiles().await,
        }
    }

    /// Health check
    pub async fn health_check(&self) -> HealthStatus {
        let mut server_status = Vec::new();

        for client in self.clients.clients().await {
            let healthy = client.health_check().await;
            server_status.push(ServerHealth {
                id: client.server_id().to_string(),
                name: client.config().name.clone(),
                healthy,
            });
        }

        let all_healthy = server_status.iter().all(|s| s.healthy);

        HealthStatus {
            healthy: all_healthy,
            servers: server_status,
        }
    }

    /// Add a server dynamically
    pub async fn add_server(&self, config: crate::config::UpstreamServer) -> Result<()> {
        let client = Arc::new(McpClient::new(config.clone())?);

        let tools = client
            .list_tools()
            .await
            .with_context(|| format!("Failed to connect to {}", config.name))?;

        self.cache.insert_batch(tools, &config.id).await;
        self.clients.add_client(client).await;

        info!("Added server: {}", config.name);
        Ok(())
    }

    // =========================================================================
    // CLIENT DETECTION & TOOL MODE
    // =========================================================================

    /// Set client info from MCP initialize request (auto-detects mode)
    pub async fn set_client_info(&self, name: &str, version: Option<&str>) {
        let client_info = ClientInfo {
            name: name.to_string(),
            version: version.map(String::from),
        };

        // Auto-detect mode based on client
        let mode = self.config.client_detection.detect_mode(name);

        info!(
            "Client connected: {} (v{}) -> {:?} mode",
            name,
            version.unwrap_or("unknown"),
            mode
        );

        *self.client_info.write().await = Some(client_info);
        *self.detected_mode.write().await = Some(mode);
    }

    /// Get the current tool mode (detected or default)
    pub async fn get_tool_mode(&self) -> ToolMode {
        self.detected_mode
            .read()
            .await
            .unwrap_or(self.config.default_mode)
    }

    /// Override the tool mode manually
    pub async fn set_tool_mode(&self, mode: ToolMode) {
        *self.detected_mode.write().await = Some(mode);
        info!("Tool mode set to: {:?}", mode);
    }

    /// Get current client info
    pub async fn get_client_info(&self) -> Option<ClientInfo> {
        self.client_info.read().await.clone()
    }

    /// Check if running in compact mode
    pub async fn is_compact_mode(&self) -> bool {
        matches!(self.get_tool_mode().await, ToolMode::Compact)
    }

    /// Get MCP tools based on current mode (for tools/list response)
    ///
    /// In Compact mode: Returns 4-5 meta-tools
    /// In Full mode: Returns all tools from the profile
    /// In Hybrid mode: Returns essential tools + meta-tools
    pub async fn get_mcp_tools(&self, mode: Option<ToolMode>) -> Result<Vec<McpToolDefinition>> {
        self.ensure_initialized().await?;

        let mode = mode.unwrap_or(self.get_tool_mode().await);

        match mode {
            ToolMode::Compact => self.get_compact_tools().await,
            ToolMode::Full => self.get_full_tools().await,
            ToolMode::Hybrid => self.get_hybrid_tools().await,
        }
    }

    /// Get compact mode meta-tools
    async fn get_compact_tools(&self) -> Result<Vec<McpToolDefinition>> {
        // We need Arc<Self> for the compact tools, so we return static definitions
        // The actual execution happens via execute_tool which has aggregator access
        Ok(vec![
            McpToolDefinition {
                name: "list_tools".to_string(),
                description: "List available tools. Use 'category' or 'namespace' to filter. Returns tool names and descriptions. Call 'get_tool_schema' to get full input schema before executing.".to_string(),
                input_schema: json!({
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
                        "limit": {
                            "type": "integer",
                            "description": "Maximum tools to return (default: 20)",
                            "default": 20
                        }
                    }
                }),
            },
            McpToolDefinition {
                name: "search_tools".to_string(),
                description: "Search for tools by keyword. Searches tool names and descriptions.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum results (default: 10)",
                            "default": 10
                        }
                    },
                    "required": ["query"]
                }),
            },
            McpToolDefinition {
                name: "get_tool_schema".to_string(),
                description: "Get the full input schema for a tool. Use this before calling execute_tool to understand required arguments.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "tool_name": {
                            "type": "string",
                            "description": "Name of the tool to get schema for"
                        }
                    },
                    "required": ["tool_name"]
                }),
            },
            McpToolDefinition {
                name: "execute_tool".to_string(),
                description: "Execute any available tool by name. First use list_tools/search_tools to find tools, then get_tool_schema to see required arguments.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "tool_name": {
                            "type": "string",
                            "description": "Name of the tool to execute"
                        },
                        "arguments": {
                            "type": "object",
                            "description": "Arguments to pass to the tool"
                        }
                    },
                    "required": ["tool_name"]
                }),
            },
        ])
    }

    /// Get all tools in full mode
    async fn get_full_tools(&self) -> Result<Vec<McpToolDefinition>> {
        let profile = self.profiles.default_profile();
        let tools = self.profiles.get_tools_for_profile(profile).await;

        Ok(tools
            .into_iter()
            .map(|t| McpToolDefinition {
                name: t.name,
                description: t.description.clone(),
                input_schema: t.input_schema,
            })
            .collect())
    }

    /// Get hybrid tools (essential + meta-tools)
    async fn get_hybrid_tools(&self) -> Result<Vec<McpToolDefinition>> {
        let mut tools = Vec::new();

        // Add essential tools (respond, system_info, etc.)
        let essential = ["respond", "respond_to_user", "system_info", "shell_exec"];
        let all_tools = self.list_default_tools().await?;

        for tool in all_tools {
            if essential.contains(&tool.name.as_str()) {
                tools.push(McpToolDefinition {
                    name: tool.name,
                    description: tool.description.clone(),
                    input_schema: tool.input_schema,
                });
            }
        }

        // Add compact meta-tools for everything else
        tools.extend(self.get_compact_tools().await?);

        Ok(tools)
    }

    /// Handle compact mode tool execution (called from MCP tools/call)
    pub async fn handle_compact_tool_call(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value> {
        match tool_name {
            "list_tools" => self.compact_list_tools(arguments).await,
            "search_tools" => self.compact_search_tools(arguments).await,
            "get_tool_schema" => self.compact_get_schema(arguments).await,
            "execute_tool" => self.compact_execute_tool(arguments).await,
            _ => {
                // Not a meta-tool, try direct execution
                let result = self.call_tool(tool_name, arguments).await?;
                Ok(result.result)
            }
        }
    }

    async fn compact_list_tools(&self, args: Value) -> Result<Value> {
        let category = args
            .as_object()
            .and_then(|obj| obj.get("category"))
            .and_then(|v| v.as_str());
        let namespace = args
            .as_object()
            .and_then(|obj| obj.get("namespace"))
            .and_then(|v| v.as_str());
        let limit = args
            .as_object()
            .and_then(|obj| obj.get("limit"))
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as usize;

        let all_tools = self.list_default_tools().await?;

        let filtered: Vec<Value> = all_tools
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
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description.as_str(),
                    "category": t.annotations.as_ref()
                        .and_then(|a| a.get("category"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("general")
                })
            })
            .collect();

        Ok(json!({
            "tools": filtered,
            "count": filtered.len(),
            "total_available": all_tools.len(),
            "hint": "Use get_tool_schema to see arguments, then execute_tool to run"
        }))
    }

    async fn compact_search_tools(&self, args: Value) -> Result<Value> {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("query is required"))?
            .to_lowercase();
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;

        let all_tools = self.list_default_tools().await?;

        let mut scored: Vec<(i32, &ToolDefinition)> = all_tools
            .iter()
            .filter_map(|t| {
                let name_lower = t.name.to_lowercase();
                let desc_lower = t.description.as_str().to_lowercase();

                let mut score = 0;
                if name_lower == query {
                    score += 100;
                } else if name_lower.contains(&query) {
                    score += 50;
                }
                if desc_lower.contains(&query) {
                    score += 20;
                }

                if score > 0 {
                    Some((score, t))
                } else {
                    None
                }
            })
            .collect();

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
            "count": results.len()
        }))
    }

    async fn compact_get_schema(&self, args: Value) -> Result<Value> {
        let tool_name = args
            .get("tool_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("tool_name is required"))?;

        let (tool_def, server_id) = self
            .cache
            .get(tool_name)
            .await
            .ok_or_else(|| anyhow!("Tool '{}' not found", tool_name))?;

        Ok(json!({
            "tool": tool_name,
            "description": tool_def.description,
            "input_schema": tool_def.input_schema,
            "server": server_id
        }))
    }

    async fn compact_execute_tool(&self, args: Value) -> Result<Value> {
        let tool_name = args
            .get("tool_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("tool_name is required"))?;

        let arguments = args.get("arguments").cloned().unwrap_or(json!({}));

        let result = self.call_tool(tool_name, arguments).await?;

        Ok(json!({
            "tool": tool_name,
            "result": result.result,
            "success": !result.is_error
        }))
    }

    /// Remove a server
    pub async fn remove_server(&self, server_id: &str) -> Result<()> {
        self.cache.remove_server(server_id).await;
        info!("Removed server: {}", server_id);
        Ok(())
    }

    async fn ensure_initialized(&self) -> Result<()> {
        if !*self.initialized.read().await {
            return Err(anyhow!(
                "Aggregator not initialized. Call initialize() first."
            ));
        }
        Ok(())
    }

    /// Get the profile manager
    pub fn profiles(&self) -> &Arc<ProfileManager> {
        &self.profiles
    }

    /// Get the tool cache
    pub fn cache(&self) -> &Arc<ToolCache> {
        &self.cache
    }
}

/// Result of a tool call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub tool_name: String,
    pub server_id: String,
    pub result: Value,
    pub is_error: bool,
}

/// Aggregator statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatorStats {
    pub connected_servers: usize,
    pub total_tools: usize,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub profiles: Vec<String>,
}

/// Health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub healthy: bool,
    pub servers: Vec<ServerHealth>,
}

/// Individual server health
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerHealth {
    pub id: String,
    pub name: String,
    pub healthy: bool,
}

/// MCP tool definition for tools/list response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Integration with op-tools ToolRegistry
impl Aggregator {
    /// Register aggregated tools with an op-tools ToolRegistry
    pub async fn register_with_tool_registry(
        &self,
        registry: &op_tools::ToolRegistry,
        profile_name: &str,
    ) -> Result<()> {
        let tools = self.list_tools(profile_name).await?;

        for tool_def in tools {
            let aggregator = self.clone_arc();
            let tool_name = tool_def.name.clone();

            // Create a tool that proxies to the aggregator
            let proxy_tool = AggregatorProxyTool {
                name: tool_def.name.clone(),
                description: tool_def.description.clone(),
                input_schema: tool_def.input_schema.clone(),
                aggregator,
            };

            registry.register_tool(Arc::new(proxy_tool)).await?;
            debug!("Registered proxy tool: {}", tool_name);
        }

        Ok(())
    }

    fn clone_arc(&self) -> Arc<Aggregator> {
        // This is a bit awkward - in practice you'd store Arc<Self>
        // For now, return a placeholder
        unimplemented!("Use Arc<Aggregator> directly")
    }
}

/// Proxy tool that delegates to the aggregator
struct AggregatorProxyTool {
    name: String,
    description: String,
    input_schema: Value,
    aggregator: Arc<Aggregator>,
}

#[async_trait::async_trait]
impl op_tools::tool::Tool for AggregatorProxyTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let result = self.aggregator.call_tool(&self.name, input).await?;
        Ok(result.result)
    }

    fn category(&self) -> &str {
        "aggregated"
    }

    fn namespace(&self) -> &str {
        "external"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_aggregator_creation() {
        let config = AggregatorConfig::default();
        let aggregator = Aggregator::new(config).await.unwrap();

        // Should not be initialized yet
        assert!(aggregator.ensure_initialized().await.is_err());
    }

    #[tokio::test]
    async fn test_aggregator_empty_init() {
        let config = AggregatorConfig::default();
        let aggregator = Aggregator::new(config).await.unwrap();

        // Initialize with no servers should work
        aggregator.initialize().await.unwrap();

        let stats = aggregator.stats().await;
        assert_eq!(stats.connected_servers, 0);
        assert_eq!(stats.total_tools, 0);
    }
}
