//! External MCP Client - Connect to and introspect other MCP servers

use anyhow::{Context, Result};
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::RwLock;

/// External MCP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalMcpConfig {
    pub name: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,

    /// Provider transport. Stdio remains available for compatibility, while
    /// supervised providers use Streamable HTTP and are never spawned here.
    #[serde(default)]
    pub transport: ExternalMcpTransport,

    /// Streamable HTTP MCP URL, required when `transport=streamable_http`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Environment variables to pass to the server
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// API key (will be set as env var or header based on auth_method)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// API key environment variable name (default: API_KEY)
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,

    /// Authentication method
    #[serde(default)]
    pub auth_method: AuthMethod,

    /// Custom headers for HTTP-based MCP servers
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExternalMcpTransport {
    #[default]
    Stdio,
    StreamableHttp,
}

fn default_api_key_env() -> String {
    "API_KEY".to_string()
}

/// Authentication method for MCP servers
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    /// No authentication
    #[default]
    None,

    /// API key in environment variable
    EnvVar,

    /// Bearer token in Authorization header (for HTTP-based MCP)
    BearerToken,

    /// Custom header (specify in headers field)
    CustomHeader,
}

/// External MCP server tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalTool {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    pub server_name: String,
}

/// External MCP client
pub struct ExternalMcpClient {
    config: ExternalMcpConfig,
    process: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
    http: reqwest::Client,
    http_session_id: Option<String>,
    tools: RwLock<Vec<ExternalTool>>,
    next_id: RwLock<u64>,
}

impl ExternalMcpClient {
    /// Create new external MCP client
    pub fn new(config: ExternalMcpConfig) -> Self {
        Self {
            config,
            process: None,
            stdin: None,
            stdout: None,
            http: reqwest::Client::new(),
            http_session_id: None,
            tools: RwLock::new(Vec::new()),
            next_id: RwLock::new(1),
        }
    }

    /// Start the external MCP server process
    pub async fn start(&mut self) -> Result<()> {
        let start_time = std::time::Instant::now();
        match self.config.transport {
            ExternalMcpTransport::Stdio => self.start_stdio_process().await?,
            ExternalMcpTransport::StreamableHttp => {
                let url = self
                    .config
                    .url
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .context("streamable HTTP MCP provider has no URL")?;
                tracing::info!(provider = %self.config.name, %url, "Connecting to supervised MCP provider");
            }
        }

        // Initialize the MCP server with timeout and retry logic
        let init_start = std::time::Instant::now();
        // A runit-supervised HTTP provider may deliberately answer 503 and
        // terminate when it detects a stranded singleton session.  Retrying
        // initialization lets runit replace that process transparently; stdio
        // providers retain the existing fail-fast behavior on protocol errors.
        let http_transport = matches!(self.config.transport, ExternalMcpTransport::StreamableHttp);
        let max_retries = if http_transport { 5 } else { 3 };
        let mut retry_count = 0;

        let init_result = loop {
            match tokio::time::timeout(std::time::Duration::from_secs(10), self.initialize()).await
            {
                Ok(Ok(_)) => {
                    let init_duration = init_start.elapsed();
                    tracing::info!(
                        "External MCP server initialized in {:.2}s",
                        init_duration.as_secs_f32()
                    );
                    break Ok(());
                }
                Ok(Err(e)) => {
                    retry_count += 1;
                    if http_transport && retry_count < max_retries {
                        self.http_session_id = None;
                        tracing::warn!(
                            provider = %self.config.name,
                            attempt = retry_count,
                            max_attempts = max_retries,
                            %e,
                            "HTTP MCP provider initialization failed; retrying supervised provider"
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                    tracing::error!(
                        "Failed to initialize external MCP server {}: {}",
                        self.config.name,
                        e
                    );
                    break Err(e);
                }
                Err(_) => {
                    retry_count += 1;
                    if retry_count >= max_retries {
                        tracing::error!(
                            "External MCP server {} initialization timed out after {} attempts",
                            self.config.name,
                            max_retries
                        );
                        break Err(anyhow::anyhow!(
                            "Initialization timeout after {} attempts",
                            max_retries
                        ));
                    }
                    tracing::warn!(
                        "Initialization attempt {} timed out, retrying...",
                        retry_count
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                }
            }
        };

        init_result?;
        self.send_initialized_notification().await?;

        // List available tools with timeout
        let tools_start = std::time::Instant::now();
        let tools_result =
            tokio::time::timeout(std::time::Duration::from_secs(15), self.refresh_tools()).await;

        match tools_result {
            Ok(Ok(_)) => {
                let tools_duration = tools_start.elapsed();
                tracing::info!(
                    "External MCP server tools loaded in {:.2}s",
                    tools_duration.as_secs_f32()
                );
            }
            Ok(Err(e)) => {
                tracing::error!(
                    "Failed to load tools from external MCP server {}: {}",
                    self.config.name,
                    e
                );
                return Err(e);
            }
            Err(_) => {
                tracing::error!(
                    "External MCP server {} tools loading timed out (15s)",
                    self.config.name
                );
                return Err(anyhow::anyhow!("Tools loading timeout"));
            }
        }

        let total_duration = start_time.elapsed();
        tracing::info!(
            "External MCP server started: {} ({} tools) in {:.2}s total",
            self.config.name,
            self.tools.read().await.len(),
            total_duration.as_secs_f32()
        );

        if total_duration.as_secs() > 5 {
            tracing::warn!("External MCP server {} took longer than expected to start (>5s). Consider optimizing or checking for startup issues.", self.config.name);
        }

        Ok(())
    }

    async fn start_stdio_process(&mut self) -> Result<()> {
        if self.config.command.trim().is_empty() {
            anyhow::bail!("stdio MCP provider '{}' has no command", self.config.name);
        }
        tracing::info!(provider = %self.config.name, "Starting stdio MCP provider");

        let mut cmd = Command::new(&self.config.command);
        cmd.args(&self.config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .envs(&self.config.env);

        if let Some(api_key) = &self.config.api_key {
            if matches!(self.config.auth_method, AuthMethod::EnvVar) {
                cmd.env(&self.config.api_key_env, api_key);
            }
        }

        let mut child = cmd
            .spawn()
            .with_context(|| format!("Failed to spawn MCP provider: {}", self.config.name))?;
        self.stdin = Some(child.stdin.take().context("Failed to open stdin")?);
        self.stdout = Some(BufReader::new(
            child.stdout.take().context("Failed to open stdout")?,
        ));
        self.process = Some(child);
        Ok(())
    }

    /// Initialize the MCP server
    async fn initialize(&mut self) -> Result<()> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_id().await,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {
                    "name": "op-dbus-mcp-aggregator",
                    "version": "0.1.0"
                }
            }
        });

        let response = self.send_request(request).await?;

        if response.get("error").is_some() {
            anyhow::bail!("Failed to initialize MCP server: {:?}", response);
        }

        tracing::debug!("MCP server initialized: {}", self.config.name);
        Ok(())
    }

    async fn send_initialized_notification(&mut self) -> Result<()> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });
        match self.config.transport {
            ExternalMcpTransport::Stdio => {
                let stdin = self.stdin.as_mut().context("MCP provider not started")?;
                let payload = simd_json::to_string(&notification)?;
                stdin.write_all(payload.as_bytes()).await?;
                stdin.write_all(b"\n").await?;
                stdin.flush().await?;
            }
            ExternalMcpTransport::StreamableHttp => {
                let _ = self.send_http(notification, false).await?;
            }
        }
        Ok(())
    }

    /// Refresh tools list from the MCP server
    pub async fn refresh_tools(&mut self) -> Result<()> {
        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_id().await,
            "method": "tools/list",
            "params": {}
        });

        let response = self.send_request(request).await?;

        if let Some(error) = response.get("error") {
            anyhow::bail!("Failed to list tools: {:?}", error);
        }

        let tools_array = response
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .context("Invalid tools response")?;

        let mut tools = Vec::new();
        for tool in tools_array {
            let name = tool
                .get("name")
                .and_then(|n| n.as_str())
                .context("Tool missing name")?;
            let description = tool
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");
            let input_schema = tool.get("inputSchema").cloned().unwrap_or(json!({}));

            tools.push(ExternalTool {
                name: format!("{}:{}", self.config.name, name),
                description: format!("[{}] {}", self.config.name, description),
                input_schema,
                server_name: self.config.name.clone(),
            });
        }

        *self.tools.write().await = tools;
        Ok(())
    }

    /// Get all tools from this MCP server
    pub async fn get_tools(&self) -> Vec<ExternalTool> {
        self.tools.read().await.clone()
    }

    /// Call a tool on the external MCP server
    pub async fn call_tool(&mut self, tool_name: &str, arguments: Value) -> Result<Value> {
        // Strip server prefix if present
        let tool_name = tool_name
            .strip_prefix(&format!("{}:", self.config.name))
            .unwrap_or(tool_name);

        let request = json!({
            "jsonrpc": "2.0",
            "id": self.next_id().await,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments
            }
        });

        let response = self.send_request(request).await?;

        if let Some(error) = response.get("error") {
            anyhow::bail!("Tool call failed: {:?}", error);
        }

        response
            .get("result")
            .cloned()
            .context("Missing result in response")
    }

    /// Send request to MCP server and get response
    async fn send_request(&mut self, request: Value) -> Result<Value> {
        match self.config.transport {
            ExternalMcpTransport::Stdio => self.send_stdio(request).await,
            ExternalMcpTransport::StreamableHttp => self
                .send_http(request, true)
                .await?
                .context("MCP provider returned no JSON-RPC response"),
        }
    }

    async fn send_stdio(&mut self, request: Value) -> Result<Value> {
        let stdin = self.stdin.as_mut().context("MCP server not started")?;
        let stdout = self.stdout.as_mut().context("MCP server not started")?;

        // Send request
        let request_str = simd_json::to_string(&request)?;
        stdin.write_all(request_str.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;

        tracing::debug!(provider = %self.config.name, "Sent stdio MCP request");

        // Read response
        let mut response_line = String::new();
        stdout.read_line(&mut response_line).await?;

        tracing::debug!(provider = %self.config.name, "Received stdio MCP response");

        let response: Value = unsafe { simd_json::from_str(&mut response_line) }
            .context("Failed to parse MCP response")?;

        Ok(response)
    }

    async fn send_http(&mut self, request: Value, expect_response: bool) -> Result<Option<Value>> {
        let url = self
            .config
            .url
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .context("streamable HTTP MCP provider has no URL")?;
        let mut builder = self
            .http
            .post(url)
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&request);
        if let Some(session_id) = &self.http_session_id {
            builder = builder.header("mcp-session-id", session_id);
        }
        for (name, value) in &self.config.headers {
            builder = builder.header(name, value);
        }
        if let Some(api_key) = &self.config.api_key {
            if matches!(self.config.auth_method, AuthMethod::BearerToken) {
                builder = builder.header(AUTHORIZATION, format!("Bearer {api_key}"));
            }
        }

        let response = builder
            .send()
            .await
            .with_context(|| format!("MCP provider '{}' is unavailable", self.config.name))?;
        if let Some(session_id) = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.trim().is_empty())
        {
            self.http_session_id = Some(session_id.to_string());
        }
        let status = response.status();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let body = response.bytes().await?;
        if !status.is_success() {
            anyhow::bail!(
                "MCP provider '{}' returned HTTP {}",
                self.config.name,
                status
            );
        }
        if !expect_response || body.is_empty() {
            return Ok(None);
        }
        Ok(Some(parse_http_mcp_response(&body, &content_type)?))
    }

    /// Get next request ID
    async fn next_id(&self) -> u64 {
        let mut id = self.next_id.write().await;
        let current = *id;
        *id += 1;
        current
    }

    /// Stop the MCP server
    pub async fn stop(&mut self) -> Result<()> {
        if matches!(self.config.transport, ExternalMcpTransport::StreamableHttp) {
            if let (Some(url), Some(session_id)) =
                (self.config.url.as_deref(), self.http_session_id.take())
            {
                let _ = self
                    .http
                    .delete(url)
                    .header("mcp-session-id", session_id)
                    .send()
                    .await;
            }
            return Ok(());
        }
        if let Some(mut process) = self.process.take() {
            tracing::info!("Stopping external MCP server: {}", self.config.name);
            process.kill().await?;
        }
        Ok(())
    }
}

fn parse_http_mcp_response(body: &[u8], content_type: &str) -> Result<Value> {
    if content_type.contains("text/event-stream") {
        let text = std::str::from_utf8(body).context("MCP SSE response is not UTF-8")?;
        for line in text.lines() {
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            let mut owned = data.to_string();
            if let Ok(value) = unsafe { simd_json::from_str::<Value>(&mut owned) } {
                return Ok(value);
            }
        }
        anyhow::bail!("MCP SSE response contained no JSON-RPC data event");
    }

    let mut bytes = body.to_vec();
    simd_json::from_slice::<Value>(&mut bytes).context("Failed to parse MCP HTTP response")
}

impl Drop for ExternalMcpClient {
    fn drop(&mut self) {
        if let Some(mut process) = self.process.take() {
            let _ = process.start_kill();
        }
    }
}

/// External MCP manager - manages multiple external MCP servers
pub struct ExternalMcpManager {
    clients: RwLock<HashMap<String, ExternalMcpClient>>,
}

impl ExternalMcpManager {
    /// Create new manager
    pub fn new() -> Self {
        Self {
            clients: RwLock::new(HashMap::new()),
        }
    }

    /// Add and start an external MCP server
    pub async fn add_server(&self, config: ExternalMcpConfig) -> Result<()> {
        let name = config.name.clone();
        let mut client = ExternalMcpClient::new(config);

        client.start().await?;

        self.clients.write().await.insert(name, client);
        Ok(())
    }

    /// Load servers from config file
    pub async fn load_from_file(&self, path: &str) -> Result<()> {
        let content = tokio::fs::read_to_string(path)
            .await
            .context("Failed to read MCP config file")?;

        let mut content = content;
        let configs: Vec<ExternalMcpConfig> =
            unsafe { simd_json::from_str(&mut content) }.context("Failed to parse MCP config")?;

        for config in configs {
            if let Err(e) = self.add_server(config.clone()).await {
                tracing::error!("Failed to start MCP server {}: {}", config.name, e);
            }
        }

        Ok(())
    }

    /// Get all tools from all external MCP servers
    pub async fn get_all_tools(&self) -> Vec<ExternalTool> {
        let clients = self.clients.read().await;
        let mut all_tools = Vec::new();

        for client in clients.values() {
            all_tools.extend(client.get_tools().await);
        }

        all_tools
    }

    /// Call a tool (format: "server:tool" or just "tool")
    pub async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<Value> {
        let (server_name, actual_tool_name) = if let Some(idx) = tool_name.find(':') {
            (&tool_name[..idx], &tool_name[idx + 1..])
        } else {
            // Try to find which server has this tool
            return Err(anyhow::anyhow!(
                "Tool name must include server prefix: server:tool"
            ));
        };

        let mut clients = self.clients.write().await;
        let client = clients
            .get_mut(server_name)
            .context(format!("MCP server not found: {}", server_name))?;

        client.call_tool(actual_tool_name, arguments).await
    }

    /// Stop all MCP servers
    pub async fn stop_all(&self) -> Result<()> {
        let mut clients = self.clients.write().await;
        for (name, client) in clients.iter_mut() {
            if let Err(e) = client.stop().await {
                tracing::error!("Failed to stop MCP server {}: {}", name, e);
            }
        }
        clients.clear();
        Ok(())
    }
}

impl Default for ExternalMcpManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::extract::State;
    use axum::http::{Request, Response, StatusCode};
    use axum::routing::post;
    use axum::Router;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn streamable_http_json_response_parses() {
        let value = parse_http_mcp_response(
            br#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#,
            "application/json",
        )
        .expect("JSON response parses");
        assert_eq!(value["id"], 1);
        assert!(value["result"]["tools"]
            .as_array()
            .is_some_and(Vec::is_empty));
    }

    #[test]
    fn streamable_http_sse_response_parses_first_json_data_event() {
        let value = parse_http_mcp_response(
            b"event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"ok\":true}}\n\n",
            "text/event-stream; charset=utf-8",
        )
        .expect("SSE response parses");
        assert_eq!(value["id"], 2);
        assert_eq!(value["result"]["ok"], true);
    }

    #[test]
    fn streamable_http_sse_without_json_fails_closed() {
        assert!(parse_http_mcp_response(b"data: [DONE]\n\n", "text/event-stream").is_err());
    }

    #[tokio::test]
    async fn streamable_http_start_retries_supervised_provider_restart() {
        async fn handler(
            State(initialize_attempts): State<Arc<AtomicUsize>>,
            request: Request<Body>,
        ) -> Response<Body> {
            let body = axum::body::to_bytes(request.into_body(), 64 * 1024)
                .await
                .expect("request body");
            let mut body = body.to_vec();
            let value: Value = simd_json::from_slice(&mut body).expect("JSON-RPC request");
            match value.get("method").and_then(ValueAsScalar::as_str) {
                Some("initialize")
                    if initialize_attempts.fetch_add(1, Ordering::SeqCst) == 0 =>
                {
                    Response::builder()
                        .status(StatusCode::SERVICE_UNAVAILABLE)
                        .header("content-type", "application/json")
                        .header("retry-after", "1")
                        .body(Body::from(
                            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32003,"message":"restart"}}"#,
                        ))
                        .unwrap()
                }
                Some("initialize") => Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .header("mcp-session-id", "replacement-session")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":2,"result":{"protocolVersion":"2025-03-26","capabilities":{},"serverInfo":{"name":"test","version":"1"}}}"#,
                    ))
                    .unwrap(),
                Some("notifications/initialized") => Response::builder()
                    .status(StatusCode::ACCEPTED)
                    .body(Body::empty())
                    .unwrap(),
                Some("tools/list") => Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":3,"result":{"tools":[]}}"#,
                    ))
                    .unwrap(),
                other => panic!("unexpected method: {other:?}"),
            }
        }

        let initialize_attempts = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test provider");
        let address = listener.local_addr().expect("test provider address");
        let server_attempts = Arc::clone(&initialize_attempts);
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/mcp", post(handler))
                    .with_state(server_attempts),
            )
            .await
            .expect("test provider server");
        });

        let mut client = ExternalMcpClient::new(ExternalMcpConfig {
            name: "retry-test".into(),
            command: String::new(),
            args: vec![],
            transport: ExternalMcpTransport::StreamableHttp,
            url: Some(format!("http://{address}/mcp")),
            env: HashMap::new(),
            api_key: None,
            api_key_env: "API_KEY".into(),
            auth_method: AuthMethod::None,
            headers: HashMap::new(),
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), client.start())
            .await
            .expect("client start timeout")
            .expect("client recovers after supervised restart response");
        assert_eq!(initialize_attempts.load(Ordering::SeqCst), 2);

        server.abort();
    }
}
