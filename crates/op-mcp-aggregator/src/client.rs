//! MCP Client for connecting to upstream servers
//!
//! Supports SSE and stdio transports for communicating with MCP servers.

use crate::config::{ServerAuth, TransportType, UpstreamServer};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

fn transport_root(url: &str) -> String {
    let trimmed = url.trim_end_matches('/');
    for suffix in ["/mcp", "/message", "/sse"] {
        if let Some(root) = trimmed.strip_suffix(suffix) {
            return root.trim_end_matches('/').to_string();
        }
    }
    trimmed.to_string()
}

fn canonical_mcp_endpoint(url: &str) -> String {
    format!("{}/mcp", transport_root(url))
}

fn legacy_message_endpoint(url: &str) -> String {
    format!("{}/message", transport_root(url))
}

/// MCP JSON-RPC request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl McpRequest {
    pub fn new(method: &str, params: Option<Value>) -> Self {
        static REQUEST_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            jsonrpc: "2.0".to_string(),
            id: json!(REQUEST_ID.fetch_add(1, Ordering::SeqCst)),
            method: method.to_string(),
            params,
        }
    }
}

/// MCP JSON-RPC response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpRpcError>,
}

/// MCP RPC Error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Tool definition (local to avoid cycle)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(default)]
    pub schema_version: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub namespace: String,
    #[serde(default)]
    pub annotations: Option<Value>,
}

/// Client for communicating with an upstream MCP server
pub struct McpClient {
    /// Server configuration
    config: UpstreamServer,
    /// HTTP client (for SSE transport)
    http_client: reqwest::Client,
    /// Cached tools from this server
    cached_tools: RwLock<Vec<ToolDefinition>>,
    /// Whether the client is initialized
    initialized: RwLock<bool>,
}

impl McpClient {
    /// Create a new MCP client for the given server
    pub fn new(config: UpstreamServer) -> Result<Self> {
        let mut client_builder = reqwest::Client::builder().timeout(config.timeout());

        // Add auth if configured
        if let Some(auth) = &config.auth {
            let resolved = auth.resolve();
            match resolved {
                ServerAuth::Bearer { token } => {
                    let mut headers = reqwest::header::HeaderMap::new();
                    headers.insert(
                        reqwest::header::AUTHORIZATION,
                        format!("Bearer {}", token)
                            .parse()
                            .map_err(|_| anyhow!("Invalid bearer token"))?,
                    );
                    client_builder = client_builder.default_headers(headers);
                }
                ServerAuth::Header { name, value } => {
                    let mut headers = reqwest::header::HeaderMap::new();
                    headers.insert(
                        reqwest::header::HeaderName::from_bytes(name.as_bytes())
                            .map_err(|_| anyhow!("Invalid header name"))?,
                        value.parse().map_err(|_| anyhow!("Invalid header value"))?,
                    );
                    client_builder = client_builder.default_headers(headers);
                }
                ServerAuth::Basic { username, password } => {
                    let mut headers = reqwest::header::HeaderMap::new();
                    use base64::Engine;
                    let credentials = base64::engine::general_purpose::STANDARD
                        .encode(format!("{}:{}", username, password));
                    headers.insert(
                        reqwest::header::AUTHORIZATION,
                        format!("Basic {}", credentials)
                            .parse()
                            .map_err(|_| anyhow!("Invalid basic auth"))?,
                    );
                    client_builder = client_builder.default_headers(headers);
                }
            }
        }

        let http_client = client_builder
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self {
            config,
            http_client,
            cached_tools: RwLock::new(vec![]),
            initialized: RwLock::new(false),
        })
    }

    /// Get the server ID
    pub fn server_id(&self) -> &str {
        &self.config.id
    }

    /// Get the server config
    pub fn config(&self) -> &UpstreamServer {
        &self.config
    }

    /// Initialize the connection to the upstream server
    pub async fn initialize(&self) -> Result<()> {
        if *self.initialized.read().await {
            return Ok(());
        }

        info!("Initializing MCP client for server: {}", self.config.name);

        match self.config.transport {
            TransportType::Sse => self.initialize_sse().await?,
            TransportType::Stdio => self.initialize_stdio().await?,
            TransportType::Websocket => {
                return Err(anyhow!("WebSocket transport not yet implemented"));
            }
        }

        *self.initialized.write().await = true;
        Ok(())
    }

    async fn initialize_sse(&self) -> Result<()> {
        let request = McpRequest::new(
            "initialize",
            Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "op-mcp-aggregator",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
        );

        let response = self.send_request(&request).await?;

        if let Some(error) = response.error {
            return Err(anyhow!("Initialize failed: {}", error.message));
        }

        debug!(
            "Initialized connection to {}: {:?}",
            self.config.name, response.result
        );
        Ok(())
    }

    async fn initialize_stdio(&self) -> Result<()> {
        // For stdio, we'd spawn a child process
        // This is a simplified implementation
        warn!("Stdio transport initialization not fully implemented");
        Ok(())
    }

    /// Send a request to the upstream server
    async fn send_request(&self, request: &McpRequest) -> Result<McpResponse> {
        match self.config.transport {
            TransportType::Sse => self.send_sse_request(request).await,
            TransportType::Stdio => self.send_stdio_request(request).await,
            TransportType::Websocket => Err(anyhow!("WebSocket not implemented")),
        }
    }

    async fn send_sse_request(&self, request: &McpRequest) -> Result<McpResponse> {
        let url = canonical_mcp_endpoint(&self.config.url);
        let legacy_url = legacy_message_endpoint(&self.config.url);

        debug!("Sending MCP request to {}: {}", url, request.method);

        let mut response = self
            .http_client
            .post(&url)
            .json(request)
            .send()
            .await
            .with_context(|| format!("Failed to send request to {}", self.config.name))?;

        if matches!(
            response.status(),
            reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED
        ) && url != legacy_url
        {
            warn!(
                "Upstream {} does not expose /mcp, retrying legacy /message endpoint",
                self.config.name
            );
            response = self
                .http_client
                .post(&legacy_url)
                .json(request)
                .send()
                .await
                .with_context(|| format!("Failed legacy request to {}", self.config.name))?;
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP error {}: {}", status, body));
        }

        let mcp_response: McpResponse = response
            .json()
            .await
            .with_context(|| "Failed to parse MCP response")?;

        Ok(mcp_response)
    }

    async fn send_stdio_request(&self, _request: &McpRequest) -> Result<McpResponse> {
        // Stdio implementation would write to child process stdin
        // and read from stdout
        Err(anyhow!("Stdio transport not fully implemented"))
    }

    /// List tools from this server
    pub async fn list_tools(&self) -> Result<Vec<ToolDefinition>> {
        self.initialize().await?;

        let request = McpRequest::new("tools/list", None);
        let response = self.send_request(&request).await?;

        if let Some(error) = response.error {
            return Err(anyhow!("tools/list failed: {}", error.message));
        }

        let result = response.result.unwrap_or(json!({}));
        let tools: Vec<ToolDefinition> = result
            .as_object()
            .and_then(|obj| obj.get("tools"))
            .and_then(|t| simd_json::serde::from_owned_value(t.clone()).ok())
            .unwrap_or_default();

        // Filter tools based on server config
        let filtered: Vec<ToolDefinition> = tools
            .into_iter()
            .filter(|t| self.config.should_include_tool(&t.name))
            .map(|mut t| {
                // Apply prefix if configured
                t.name = self.config.prefixed_name(&t.name);
                t
            })
            .collect();

        // Cache the tools
        *self.cached_tools.write().await = filtered.clone();

        info!("Loaded {} tools from {}", filtered.len(), self.config.name);
        Ok(filtered)
    }

    /// Get cached tools (without refreshing)
    pub async fn get_cached_tools(&self) -> Vec<ToolDefinition> {
        self.cached_tools.read().await.clone()
    }

    /// Call a tool on this server
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        self.initialize().await?;

        // Remove prefix if we added one
        let actual_name = if let Some(prefix) = &self.config.tool_prefix {
            let prefix_with_underscore = format!("{}_", prefix);
            name.strip_prefix(&prefix_with_underscore)
                .unwrap_or(name)
                .to_string()
        } else {
            name.to_string()
        };

        debug!(
            "Calling tool {} (actual: {}) on {}",
            name, actual_name, self.config.name
        );

        let request = McpRequest::new(
            "tools/call",
            Some(json!({
                "name": actual_name,
                "arguments": arguments
            })),
        );

        let response = self.send_request(&request).await?;

        if let Some(error) = response.error {
            return Err(anyhow!("Tool call failed: {}", error.message));
        }

        Ok(response.result.unwrap_or(json!(null)))
    }

    /// Check if this server has a tool (by prefixed name)
    pub async fn has_tool(&self, name: &str) -> bool {
        let tools = self.cached_tools.read().await;
        let result = tools.iter().any(|t| t.name == name);
        result
    }

    /// Health check
    pub async fn health_check(&self) -> bool {
        match self.config.transport {
            TransportType::Sse => {
                let url = format!("{}/health", transport_root(&self.config.url));
                self.http_client
                    .get(&url)
                    .send()
                    .await
                    .map(|r| r.status().is_success())
                    .unwrap_or(false)
            }
            _ => true, // Assume healthy for other transports
        }
    }
}

/// Manager for multiple MCP clients
pub struct ClientManager {
    clients: RwLock<Vec<Arc<McpClient>>>,
}

impl ClientManager {
    pub fn new() -> Self {
        Self {
            clients: RwLock::new(vec![]),
        }
    }

    /// Add a client
    pub async fn add_client(&self, client: Arc<McpClient>) {
        self.clients.write().await.push(client);
    }

    /// Get all clients
    pub async fn clients(&self) -> Vec<Arc<McpClient>> {
        self.clients.read().await.clone()
    }

    /// Get client by server ID
    pub async fn get_client(&self, server_id: &str) -> Option<Arc<McpClient>> {
        self.clients
            .read()
            .await
            .iter()
            .find(|c| c.server_id() == server_id)
            .cloned()
    }

    /// Find which client owns a tool
    pub async fn find_tool_owner(&self, tool_name: &str) -> Option<Arc<McpClient>> {
        for client in self.clients.read().await.iter() {
            if client.has_tool(tool_name).await {
                return Some(client.clone());
            }
        }
        None
    }

    /// Refresh all clients
    pub async fn refresh_all(&self) -> Result<()> {
        let clients = self.clients.read().await.clone();
        for client in clients {
            if let Err(e) = client.list_tools().await {
                error!("Failed to refresh tools from {}: {}", client.server_id(), e);
            }
        }
        Ok(())
    }
}

impl Default for ClientManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_request_creation() {
        let req = McpRequest::new("tools/list", None);
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.method, "tools/list");
        assert!(req.params.is_none());
    }

    #[test]
    fn test_tool_prefix_stripping() {
        // Test that tool names are properly prefixed/unprefixed
        let config =
            UpstreamServer::sse("gh", "GitHub", "http://localhost:3000").with_prefix("github");

        assert_eq!(config.prefixed_name("search"), "github_search");
    }

    #[test]
    fn should_normalize_transport_urls() {
        assert_eq!(
            canonical_mcp_endpoint("http://localhost:3000"),
            "http://localhost:3000/mcp"
        );
        assert_eq!(
            canonical_mcp_endpoint("http://localhost:3000/sse"),
            "http://localhost:3000/mcp"
        );
        assert_eq!(
            legacy_message_endpoint("http://localhost:3000/mcp"),
            "http://localhost:3000/message"
        );
    }
}
