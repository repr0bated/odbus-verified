//! Configuration for MCP Aggregator
//!
//! Supports loading from JSON/YAML files or environment variables.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;
use tracing::info;

/// Main configuration for the aggregator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatorConfig {
    /// Upstream MCP servers to aggregate
    #[serde(default)]
    pub servers: Vec<UpstreamServer>,

    /// Named profiles that select subsets of tools
    #[serde(default)]
    pub profiles: HashMap<String, ProfileConfig>,

    /// Cache settings
    #[serde(default)]
    pub cache: CacheConfig,

    /// Default profile to use if none specified
    #[serde(default = "default_profile")]
    pub default_profile: String,

    /// Maximum tools to expose per profile (Cursor limit is 40)
    #[serde(default = "default_max_tools")]
    pub max_tools_per_profile: usize,

    /// Compact mode settings
    #[serde(default)]
    pub compact_mode: crate::compact::CompactModeConfig,

    /// Client auto-detection settings
    #[serde(default)]
    pub client_detection: ClientDetectionConfig,

    /// Default tool mode (compact/full/hybrid)
    #[serde(default)]
    pub default_mode: ToolMode,
}

fn default_profile() -> String {
    "default".to_string()
}

fn default_max_tools() -> usize {
    40
}

impl Default for AggregatorConfig {
    fn default() -> Self {
        Self {
            servers: vec![],
            profiles: HashMap::new(),
            cache: CacheConfig::default(),
            default_profile: default_profile(),
            max_tools_per_profile: default_max_tools(),
            compact_mode: crate::compact::CompactModeConfig::default(),
            client_detection: ClientDetectionConfig::default(),
            default_mode: ToolMode::default(),
        }
    }
}

impl AggregatorConfig {
    /// Load configuration from a JSON file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;

        let config: Self = if path
            .extension()
            .map(|e| e == "yaml" || e == "yml")
            .unwrap_or(false)
        {
            serde_yaml::from_str(&content).with_context(|| "Failed to parse YAML config")?
        } else {
            let mut content = content;
            let content_bytes = unsafe { content.as_bytes_mut() };
            simd_json::from_slice(content_bytes).with_context(|| "Failed to parse JSON config")?
        };

        info!("Loaded aggregator config from {}", path.display());
        Ok(config)
    }

    /// Load from default paths, with fallbacks
    pub fn load_default() -> Result<Self> {
        let paths = [
            "/etc/mcp/mcp-servers.json",
            "/etc/op-dbus/aggregator.json",
            "/etc/op-dbus/mcp-aggregator.json",
            "aggregator.json",
        ];

        for path in paths {
            if Path::new(path).exists() {
                return Self::load(path);
            }
        }

        // Return default config if no file found
        info!("No aggregator config found, using defaults");
        Ok(Self::default())
    }

    /// Create a builder for programmatic configuration
    pub fn builder() -> AggregatorConfigBuilder {
        AggregatorConfigBuilder::default()
    }
}

/// Builder for AggregatorConfig
#[derive(Default)]
pub struct AggregatorConfigBuilder {
    config: AggregatorConfig,
}

impl AggregatorConfigBuilder {
    pub fn server(mut self, server: UpstreamServer) -> Self {
        self.config.servers.push(server);
        self
    }

    pub fn profile(mut self, name: &str, profile: ProfileConfig) -> Self {
        self.config.profiles.insert(name.to_string(), profile);
        self
    }

    pub fn max_tools(mut self, max: usize) -> Self {
        self.config.max_tools_per_profile = max;
        self
    }

    pub fn default_profile(mut self, name: &str) -> Self {
        self.config.default_profile = name.to_string();
        self
    }

    pub fn build(self) -> AggregatorConfig {
        self.config
    }
}

/// Configuration for an upstream MCP server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamServer {
    /// Unique identifier for this server
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// Server URL (http://host:port for SSE, or command for stdio)
    pub url: String,

    /// Transport type
    #[serde(default)]
    pub transport: TransportType,

    /// Whether this server is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Tool name prefix (e.g., "github_" for github server)
    #[serde(default)]
    pub tool_prefix: Option<String>,

    /// Only include these tools (empty = all)
    #[serde(default)]
    pub include_tools: Vec<String>,

    /// Exclude these tools
    #[serde(default)]
    pub exclude_tools: Vec<String>,

    /// Priority (higher = preferred when tools conflict)
    #[serde(default)]
    pub priority: i32,

    /// Connection timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,

    /// Optional authentication
    #[serde(default)]
    pub auth: Option<ServerAuth>,
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    30
}

impl UpstreamServer {
    /// Create a new SSE-based upstream server
    pub fn sse(id: &str, name: &str, url: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            url: url.to_string(),
            transport: TransportType::Sse,
            enabled: true,
            tool_prefix: None,
            include_tools: vec![],
            exclude_tools: vec![],
            priority: 0,
            timeout_secs: default_timeout(),
            auth: None,
        }
    }

    /// Create a new stdio-based upstream server
    pub fn stdio(id: &str, name: &str, command: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            url: command.to_string(),
            transport: TransportType::Stdio,
            enabled: true,
            tool_prefix: None,
            include_tools: vec![],
            exclude_tools: vec![],
            priority: 0,
            timeout_secs: default_timeout(),
            auth: None,
        }
    }

    /// Add a tool prefix
    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.tool_prefix = Some(prefix.to_string());
        self
    }

    /// Include only specific tools
    pub fn with_include(mut self, tools: Vec<String>) -> Self {
        self.include_tools = tools;
        self
    }

    /// Exclude specific tools
    pub fn with_exclude(mut self, tools: Vec<String>) -> Self {
        self.exclude_tools = tools;
        self
    }

    /// Get timeout as Duration
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs)
    }

    /// Check if a tool should be included from this server
    pub fn should_include_tool(&self, tool_name: &str) -> bool {
        // Check excludes first
        if self.exclude_tools.iter().any(|t| t == tool_name) {
            return false;
        }

        // If includes specified, tool must be in the list
        if !self.include_tools.is_empty() {
            return self.include_tools.iter().any(|t| t == tool_name);
        }

        true
    }

    /// Apply prefix to a tool name
    pub fn prefixed_name(&self, tool_name: &str) -> String {
        match &self.tool_prefix {
            Some(prefix) => format!("{}_{}", prefix, tool_name),
            None => tool_name.to_string(),
        }
    }
}

/// Transport type for upstream servers
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransportType {
    /// HTTP + Server-Sent Events
    #[default]
    Sse,
    /// Standard I/O (for local processes)
    Stdio,
    /// WebSocket
    Websocket,
}

/// Authentication configuration for upstream servers
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerAuth {
    /// Bearer token authentication
    Bearer {
        /// Token value (can be env var reference like ${GITHUB_TOKEN})
        token: String,
    },
    /// Basic authentication
    Basic { username: String, password: String },
    /// Custom header
    Header { name: String, value: String },
}

impl ServerAuth {
    /// Resolve environment variable references in auth values
    pub fn resolve(&self) -> Self {
        match self {
            Self::Bearer { token } => Self::Bearer {
                token: resolve_env_var(token),
            },
            Self::Basic { username, password } => Self::Basic {
                username: resolve_env_var(username),
                password: resolve_env_var(password),
            },
            Self::Header { name, value } => Self::Header {
                name: name.clone(),
                value: resolve_env_var(value),
            },
        }
    }
}

/// Resolve environment variable references like ${VAR_NAME}
fn resolve_env_var(value: &str) -> String {
    if value.starts_with("${") && value.ends_with('}') {
        let var_name = &value[2..value.len() - 1];
        std::env::var(var_name).unwrap_or_else(|_| value.to_string())
    } else {
        value.to_string()
    }
}

/// Profile configuration - defines which tools are available
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileConfig {
    /// Human-readable description
    #[serde(default)]
    pub description: String,

    /// Which servers to include (empty = all)
    #[serde(default)]
    pub servers: Vec<String>,

    /// Specific tools to include (empty = all from included servers)
    #[serde(default)]
    pub include_tools: Vec<String>,

    /// Tools to exclude
    #[serde(default)]
    pub exclude_tools: Vec<String>,

    /// Tool categories to include
    #[serde(default)]
    pub include_categories: Vec<String>,

    /// Tool namespaces to include
    #[serde(default)]
    pub include_namespaces: Vec<String>,

    /// Maximum tools for this profile (overrides global)
    #[serde(default)]
    pub max_tools: Option<usize>,
}

impl ProfileConfig {
    /// Create a new empty profile
    pub fn new(description: &str) -> Self {
        Self {
            description: description.to_string(),
            ..Default::default()
        }
    }

    /// Include specific servers
    pub fn with_servers(mut self, servers: Vec<&str>) -> Self {
        self.servers = servers.into_iter().map(String::from).collect();
        self
    }

    /// Include specific tools
    pub fn with_tools(mut self, tools: Vec<&str>) -> Self {
        self.include_tools = tools.into_iter().map(String::from).collect();
        self
    }

    /// Exclude specific tools
    pub fn excluding(mut self, tools: Vec<&str>) -> Self {
        self.exclude_tools = tools.into_iter().map(String::from).collect();
        self
    }

    /// Set max tools
    pub fn with_max(mut self, max: usize) -> Self {
        self.max_tools = Some(max);
        self
    }
}

/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// How long to cache tool schemas (seconds)
    #[serde(default = "default_schema_ttl")]
    pub schema_ttl_secs: u64,

    /// Maximum cached entries
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,

    /// Whether to refresh cache in background
    #[serde(default = "default_true")]
    pub background_refresh: bool,
}

/// Client auto-detection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientDetectionConfig {
    /// Enable automatic client detection
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Clients that should use compact mode by default
    #[serde(default = "default_compact_clients")]
    pub compact_mode_clients: Vec<String>,

    /// Clients that should use full mode by default
    #[serde(default = "default_full_clients")]
    pub full_mode_clients: Vec<String>,

    /// Default mode when client is unknown
    #[serde(default = "default_mode")]
    pub default_mode: String,
}

fn default_compact_clients() -> Vec<String> {
    vec![
        // Claude/Anthropic clients
        "claude".to_string(),
        "anthropic".to_string(),
        "@anthropic".to_string(),
        // ChatGPT/OpenAI clients
        "chatgpt".to_string(),
        "openai".to_string(),
        "gpt".to_string(),
        // Generic LLM/AI clients
        "llm".to_string(),
        "ai-assistant".to_string(),
        "assistant".to_string(),
        // Chatbot mode
        "chatbot".to_string(),
        "op-chat".to_string(),
        "chat".to_string(),
        // CLI tools that benefit from compact
        "cli".to_string(),
        "terminal".to_string(),
    ]
}

fn default_full_clients() -> Vec<String> {
    vec![
        // Gemini CLI - ALL variations (Google's CLI tool)
        "gemini".to_string(),         // Base match
        "gemini-cli".to_string(),     // Hyphenated
        "gemini_cli".to_string(),     // Underscored
        "gemini cli".to_string(),     // Space
        "@google/gemini".to_string(), // NPM package style
        "google-gemini".to_string(),  // Google prefix
        // Cursor IDE - has 40 tool limit but can use full mode for small sets
        "cursor".to_string(),
        // VS Code extensions
        "vscode".to_string(),
        "code".to_string(),
        // Direct API access
        "api".to_string(),
        "direct".to_string(),
    ]
}

fn default_mode() -> String {
    "compact".to_string() // Default to compact for efficiency
}

impl Default for ClientDetectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            compact_mode_clients: default_compact_clients(),
            full_mode_clients: default_full_clients(),
            default_mode: default_mode(),
        }
    }
}

impl ClientDetectionConfig {
    /// Detect the appropriate mode for a client
    pub fn detect_mode(&self, client_name: &str) -> ToolMode {
        if !self.enabled {
            return self.parse_default_mode();
        }

        let client_lower = client_name.to_lowercase();

        // PRIORITY 1: Explicit Gemini CLI detection (always FULL mode for Gemini 3+)
        if Self::is_gemini_cli(&client_lower) {
            tracing::info!(
                "🔷 Gemini CLI detected: '{}' -> FULL mode (Vertex AI Compact Tool Defs supported)",
                client_name
            );
            return ToolMode::Full;
        }

        // PRIORITY 2: Check for compact mode clients
        for pattern in &self.compact_mode_clients {
            let pattern_lower = pattern.to_lowercase();
            if client_lower.contains(&pattern_lower) || pattern_lower.contains(&client_lower) {
                tracing::info!(
                    "Auto-detected compact mode for client: {} (matched: {})",
                    client_name,
                    pattern
                );
                return ToolMode::Compact;
            }
        }

        // PRIORITY 3: Check for full mode clients
        for pattern in &self.full_mode_clients {
            let pattern_lower = pattern.to_lowercase();
            if client_lower.contains(&pattern_lower) || pattern_lower.contains(&client_lower) {
                tracing::info!(
                    "Auto-detected full mode for client: {} (matched: {})",
                    client_name,
                    pattern
                );
                return ToolMode::Full;
            }
        }

        // Use default (compact for safety/efficiency)
        tracing::info!(
            "Unknown client '{}', using default mode: {}",
            client_name,
            self.default_mode
        );
        self.parse_default_mode()
    }

    /// Explicit check for Gemini CLI (Google's AI CLI tool)
    fn is_gemini_cli(client_name: &str) -> bool {
        let gemini_patterns = [
            "gemini",
            "google-ai",
            "google ai",
            "googleai",
            "@google/",
            "bard", // Old name for Gemini
        ];

        for pattern in gemini_patterns {
            if client_name.contains(pattern) {
                return true;
            }
        }
        false
    }

    fn parse_default_mode(&self) -> ToolMode {
        match self.default_mode.to_lowercase().as_str() {
            "full" => ToolMode::Full,
            "hybrid" => ToolMode::Hybrid,
            _ => ToolMode::Compact,
        }
    }

    /// Check if a client name matches Gemini CLI
    pub fn is_gemini(client_name: &str) -> bool {
        Self::is_gemini_cli(&client_name.to_lowercase())
    }
}

/// Tool mode - how tools are exposed to clients
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToolMode {
    /// Compact mode: 4-5 meta-tools (list, search, schema, execute)
    /// Best for: LLMs, chatbots, context-limited clients
    #[default]
    Compact,

    /// Full mode: All tools exposed directly
    /// Best for: IDEs, direct API access, small tool sets
    Full,

    /// Hybrid mode: Essential tools direct + meta-tools for the rest
    /// Best for: When you need a few tools always available
    Hybrid,
}

fn default_schema_ttl() -> u64 {
    300 // 5 minutes
}

fn default_max_entries() -> usize {
    1000
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            schema_ttl_secs: default_schema_ttl(),
            max_entries: default_max_entries(),
            background_refresh: true,
        }
    }
}

impl CacheConfig {
    pub fn schema_ttl(&self) -> Duration {
        Duration::from_secs(self.schema_ttl_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_upstream_server_tool_filtering() {
        let server = UpstreamServer::sse("test", "Test", "http://localhost:3000")
            .with_include(vec!["tool_a".into(), "tool_b".into()])
            .with_exclude(vec!["tool_c".into()]);

        assert!(server.should_include_tool("tool_a"));
        assert!(server.should_include_tool("tool_b"));
        assert!(!server.should_include_tool("tool_c"));
        assert!(!server.should_include_tool("tool_d")); // Not in include list
    }

    #[test]
    fn test_tool_prefix() {
        let server =
            UpstreamServer::sse("gh", "GitHub", "http://localhost:3000").with_prefix("github");

        assert_eq!(server.prefixed_name("search"), "github_search");
    }

    #[test]
    fn test_resolve_env_var() {
        std::env::set_var("TEST_TOKEN", "secret123");
        assert_eq!(resolve_env_var("${TEST_TOKEN}"), "secret123");
        assert_eq!(resolve_env_var("plain_value"), "plain_value");
        std::env::remove_var("TEST_TOKEN");
    }

    #[test]
    fn test_config_builder() {
        let config = AggregatorConfig::builder()
            .server(UpstreamServer::sse(
                "local",
                "Local",
                "http://localhost:3001",
            ))
            .profile("admin", ProfileConfig::new("Admin tools").with_max(30))
            .max_tools(40)
            .default_profile("admin")
            .build();

        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.profiles.len(), 1);
        assert_eq!(config.max_tools_per_profile, 40);
        assert_eq!(config.default_profile, "admin");
    }

    #[test]
    fn test_gemini_cli_detection() {
        let config = ClientDetectionConfig::default();

        // All these should detect as Gemini CLI -> Full mode
        let gemini_clients = [
            "gemini-cli",
            "Gemini CLI",
            "gemini",
            "@google/gemini-cli",
            "google-ai-cli",
            "GoogleAI",
            "bard", // Old Gemini name
        ];

        for client in gemini_clients {
            let mode = config.detect_mode(client);
            assert_eq!(mode, ToolMode::Full, "Failed for client: {}", client);
            assert!(
                ClientDetectionConfig::is_gemini(client),
                "is_gemini failed for: {}",
                client
            );
        }
    }

    #[test]
    fn test_cursor_detection() {
        let config = ClientDetectionConfig::default();

        // Cursor should get Full mode
        let cursor_clients = ["cursor", "Cursor IDE", "cursor-editor"];

        for client in cursor_clients {
            let mode = config.detect_mode(client);
            assert_eq!(mode, ToolMode::Full, "Failed for client: {}", client);
        }
    }

    #[test]
    fn test_claude_detection() {
        let config = ClientDetectionConfig::default();

        // Claude/Anthropic should get Compact mode
        let claude_clients = ["claude", "Claude", "anthropic", "@anthropic/cli"];

        for client in claude_clients {
            let mode = config.detect_mode(client);
            assert_eq!(mode, ToolMode::Compact, "Failed for client: {}", client);
        }
    }

    #[test]
    fn test_unknown_client_default() {
        let config = ClientDetectionConfig::default();

        // Unknown clients should get default (Compact)
        let mode = config.detect_mode("some-random-unknown-client");
        assert_eq!(mode, ToolMode::Compact);
    }
}
