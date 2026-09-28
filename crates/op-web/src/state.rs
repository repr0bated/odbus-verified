//! Application State
//!
//! Central state management for the web server.
//! Simple, direct tool access - no MCP complexity.

use anyhow::{anyhow, bail, Context};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tokio_stream::StreamExt;
use tracing::{debug, info, warn};

use op_agents::agent_registry::AgentRegistry;
use op_grpc_bridge::{GrpcClientPool, RemoteOperationClient};
use op_llm::chat::ChatManager;
use op_state_store::{MemoryStore, StateStore};
use op_tools::registry::ToolDefinition;
use op_tools::tool::{BoxedTool, Tool};
use op_tools::ToolRegistry;

use crate::email::{EmailConfig, EmailSender};
use crate::orchestrator::UnifiedOrchestrator;
use crate::sse::SseEventBroadcaster;
use crate::users::UserStore;
use crate::wireguard::WgServerConfig;

/// The bridge's `:8090` door, shared by op-web's own client and the page's
/// door connector. The door is TLS-only; its trust anchor is
/// `OP_DBUS_GRPC_CA_FILE`.
pub fn door_grpc_addr() -> String {
    std::env::var("OP_DBUS_GRPC_ADDR")
        .or_else(|_| std::env::var("OP_DBUS_ADDR"))
        .or_else(|_| std::env::var("FABRIC_GRPC_ADDR"))
        .or_else(|_| std::env::var("OP_FABRIC_GRPC_ADDR"))
        .unwrap_or_else(|_| "https://10.0.0.2:8090".to_string())
}

/// Google OAuth configuration
#[derive(Debug, Clone)]
pub struct GoogleOAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_url: String,
}

/// User-specific API credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserApiCredentials {
    pub token: String,
    pub gemini_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub preferred_provider: Option<String>,
}

impl GoogleOAuthConfig {
    /// Load configuration from environment variables
    pub fn from_env() -> Option<Self> {
        let client_id = std::env::var("GOOGLE_OAUTH_CLIENT_ID").ok()?;
        let client_secret = std::env::var("GOOGLE_OAUTH_CLIENT_SECRET").ok()?;
        let redirect_url = std::env::var("GOOGLE_OAUTH_REDIRECT_URL")
            .unwrap_or_else(|_| "http://localhost:8080/api/privacy/google/callback".to_string());

        Some(Self {
            client_id,
            client_secret,
            redirect_url,
        })
    }
}

/// Application state shared across all handlers
pub struct AppState {
    /// Unified orchestrator - direct tool access
    pub orchestrator: Arc<UnifiedOrchestrator>,
    /// Tool registry
    pub tool_registry: Arc<ToolRegistry>,
    /// Agent registry
    pub agent_registry: Arc<RwLock<AgentRegistry>>,
    /// Chat manager for LLM access
    pub chat_manager: Arc<ChatManager>,
    /// Default model
    pub default_model: String,
    /// Provider name
    pub provider_name: String,
    /// Broadcast channel for WebSocket messages
    pub broadcast_tx: broadcast::Sender<String>,
    /// SSE event broadcaster
    pub sse_broadcaster: Arc<SseEventBroadcaster>,
    /// Server start time
    pub start_time: std::time::Instant,
    /// Conversation history — JSON flat file, no SQLite, no drift.
    pub conversations: Arc<crate::chat_store::ChatSessionStore>,
    /// Privacy router user store
    pub user_store: Arc<UserStore>,
    /// Email sender for magic links
    pub email_sender: Arc<EmailSender>,
    /// WireGuard server configuration
    pub server_config: WgServerConfig,
    /// Persistent state store (audit log)
    pub state_store: Arc<dyn StateStore>,
    /// Google OAuth configuration (optional)
    pub google_oauth_config: Option<GoogleOAuthConfig>,
    /// CSRF tokens for Google OAuth
    pub csrf_tokens: Arc<RwLock<HashMap<String, String>>>,
    /// Remote operation client (gRPC)
    pub grpc_client: Arc<RemoteOperationClient>,
    /// Recent activity feed from projection/D-Bus events
    pub activity_log: Arc<RwLock<Vec<simd_json::OwnedValue>>>,
}

impl AppState {
    /// Create new AppState with an optional shared tool registry
    /// If registry is provided, skips tool discovery (use registry from main binary)
    pub async fn new_with_registry(
        tool_registry: Option<Arc<ToolRegistry>>,
        state_store: Option<Arc<dyn StateStore>>,
    ) -> anyhow::Result<Self> {
        info!("Initializing application state...");

        let tool_registry = if let Some(registry) = tool_registry {
            info!("Using shared tool registry from main binary (projection already complete)");
            registry
        } else {
            if let Some(remote_url) = remote_tool_source_url() {
                info!("Creating tool registry from op-dbus at {}", remote_url);
                let registry = Arc::new(ToolRegistry::new());
                register_remote_tools(&registry, &remote_url).await?;
                registry
            } else {
                info!("Creating new tool registry (standalone mode)");
                let registry = Arc::new(ToolRegistry::new());

                // Register ALL tools (including D-Bus projection) - only in standalone mode
                register_all_tools(&registry).await?;

                registry
            }
        };

        // Log tool count
        let tools = tool_registry.list().await;
        info!("✅ Using registry with {} tools", tools.len());
        log_tool_summary(&tools);

        // Create chat manager for LLM access
        let chat_manager = Arc::new(ChatManager::new());

        // Load persisted provider/model
        if let Some(provider) = read_persisted_provider().await
            && let Ok(provider_type) = provider.parse()
        {
            match chat_manager.switch_provider(provider_type).await {
                Err(e) => {
                    warn!("Failed to load provider '{}': {}", provider, e);
                }
                _ => {
                    info!("Loaded provider: {}", provider);
                }
            }
        }

        if let Some(model) = read_persisted_model().await {
            let non_sandboxed = read_persisted_model_non_sandboxed().await;
            match chat_manager
                .switch_model_with_options(model.clone(), non_sandboxed)
                .await
            {
                Err(e) => {
                    warn!("Failed to load model '{}': {}", model, e);
                }
                _ => {
                    info!("Loaded model: {}", model);
                }
            }
        }

        // Get LLM info
        let provider_type = chat_manager.current_provider().await;
        let default_model = chat_manager.current_model().await;
        let provider_name = provider_type.to_string();

        info!("✅ LLM: {} ({})", provider_name, default_model);

        // Create agent registry
        let agent_registry = Arc::new(RwLock::new(AgentRegistry::new()));

        // Create orchestrator with direct tool access
        let orchestrator = Arc::new(UnifiedOrchestrator::new(
            tool_registry.clone(),
            chat_manager.clone(),
        ));

        // Create broadcast channel for WebSocket
        let (broadcast_tx, _) = broadcast::channel(100);

        // Create SSE broadcaster
        let sse_broadcaster = Arc::new(SseEventBroadcaster::new());

        // Initialize privacy router components
        // User database in CozoDB — graph-native, no JSON drift.
        let user_store = Arc::new(
            UserStore::new("/var/lib/op-dbus/users-cozo")
                .await
                .context("opening durable CozoDB user store")?,
        );

        let email_config = EmailConfig::from_env().unwrap_or_else(|e| {
            warn!("Failed to load email config: {}", e);
            EmailConfig {
                smtp_host: "localhost".to_string(),
                smtp_port: 587,
                smtp_user: String::new(),
                smtp_pass: String::new(),
                from_email: "noreply@example.com".to_string(),
                from_name: "Privacy Router".to_string(),
                base_url: "http://localhost:8080".to_string(),
            }
        });
        let email_sender = Arc::new(EmailSender::new(email_config));

        // Load WireGuard server config (will need to be configured properly)
        let server_config = WgServerConfig::default();

        // Load Google OAuth config
        let google_oauth_config = GoogleOAuthConfig::from_env();
        if google_oauth_config.is_some() {
            info!("✅ Google OAuth configured");
        } else {
            info!("⚠️  Google OAuth not configured (set GOOGLE_OAUTH_CLIENT_ID and GOOGLE_OAUTH_CLIENT_SECRET)");
        }

        // Initialize State Store
        let state_store: Arc<dyn StateStore> = if let Some(store) = state_store {
            info!("Using shared state store from main binary");
            store
        } else {
            // No persistent database — SHM is the single source of truth.
            // All ephemeral state (chat sessions, execution jobs) lives in-memory only.
            info!("Using in-memory state store (no persistent DB — SHM is source of truth)");
            Arc::new(MemoryStore::new())
        };

        info!("✅ Application state initialized");

        // Initialize gRPC client for remote operations.
        // Consolidated control plane is op-grpc-bridge on :8090 (not a separate :50051 opdbus).
        let grpc_addr = door_grpc_addr();
        let pool = Arc::new(GrpcClientPool::new());
        let grpc_client = Arc::new(RemoteOperationClient::new(pool, &grpc_addr, "op-web"));

        Ok(Self {
            orchestrator,
            tool_registry,
            agent_registry,
            chat_manager,
            default_model,
            provider_name,
            broadcast_tx,
            sse_broadcaster,
            start_time: std::time::Instant::now(),
            conversations: Arc::new(crate::chat_store::ChatSessionStore::default_store().await?),
            user_store,
            email_sender,
            server_config,
            state_store,
            google_oauth_config,
            csrf_tokens: Arc::new(RwLock::new(HashMap::new())),
            grpc_client,
            activity_log: Arc::new(RwLock::new(Vec::new())),
        })
    }

    /// Create new AppState (standalone mode with its own tool discovery)
    pub async fn new() -> anyhow::Result<Self> {
        Self::new_with_registry(None, None).await
    }

    /// Get uptime in seconds
    pub fn uptime_secs(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Start the event bridge from gRPC to SSE
    pub fn start_event_bridge(self: Arc<Self>) {
        let state = self.clone();

        // 1. State updates bridge
        let state_clone = state.clone();
        tokio::spawn(async move {
            info!("Starting gRPC -> SSE state updates bridge...");
            loop {
                match state_clone
                    .grpc_client
                    .subscribe(vec![], vec![], vec![])
                    .await
                {
                    Ok(mut stream) => {
                        info!("Subscribed to gRPC state updates");
                        state_clone
                            .sse_broadcaster
                            .broadcast("state_resync", "{\"reason\":\"upstream_connected\"}");
                        while let Some(msg_result) = stream.next().await {
                            match msg_result {
                                Ok(msg) => match crate::sse::state_message_event(&msg) {
                                    Ok(event) => state_clone
                                        .sse_broadcaster
                                        .broadcast(&event.event_type, &event.data),
                                    Err(error) => {
                                        warn!(%error, "Invalid committed state frame; reconnecting");
                                        break;
                                    }
                                },
                                Err(e) => {
                                    warn!("gRPC subscription stream error: {}", e);
                                    break;
                                }
                            }
                        }
                        state_clone.sse_broadcaster.broadcast(
                            "state_unavailable",
                            "{\"reason\":\"upstream_disconnected\"}",
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                    Err(e) => {
                        warn!("Failed to subscribe to gRPC updates: {}. Retrying...", e);
                        state_clone.sse_broadcaster.broadcast(
                            "state_unavailable",
                            "{\"reason\":\"upstream_unavailable\"}",
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
        });

        // Audit history is privileged and is never subscribed with the web
        // service identity for this shared, unauthenticated SSE endpoint.
    }
}

const PERSISTED_MODEL_PATH: &str = "/etc/op-dbus/llm-model";
const PERSISTED_MODEL_NON_SANDBOXED_PATH: &str = "/etc/op-dbus/llm-model-non-sandboxed";
const PERSISTED_PROVIDER_PATH: &str = "/etc/op-dbus/llm-provider";

async fn read_persisted_model() -> Option<String> {
    tokio::fs::read_to_string(PERSISTED_MODEL_PATH)
        .await
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn read_persisted_model_non_sandboxed() -> Option<bool> {
    tokio::fs::read_to_string(PERSISTED_MODEL_NON_SANDBOXED_PATH)
        .await
        .ok()
        .map(|s| {
            let value = s.trim();
            value == "1" || value.eq_ignore_ascii_case("true")
        })
}

async fn read_persisted_provider() -> Option<String> {
    tokio::fs::read_to_string(PERSISTED_PROVIDER_PATH)
        .await
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[derive(Clone)]
struct RemoteToolProxy {
    name: String,
    description: String,
    input_schema: Value,
    base_url: String,
    client: reqwest::Client,
}

#[async_trait]
impl Tool for RemoteToolProxy {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        self.input_schema.clone()
    }

    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let url = format!("{}/api/tool", self.base_url);
        let payload = json!({
            "tool_name": self.name.clone(),
            "arguments": input,
        });
        let body = simd_json::to_string(&payload).context("failed to serialize tool payload")?;

        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .with_context(|| format!("failed to call remote tool endpoint {}", url))?;

        let status = response.status();
        let response_body = response
            .text()
            .await
            .context("failed to read remote tool response body")?;
        if !status.is_success() {
            bail!(
                "remote tool execution failed ({}): {}",
                status,
                response_body
            );
        }

        let mut bytes = response_body.into_bytes();
        let parsed: Value =
            simd_json::to_owned_value(&mut bytes).context("failed to parse remote response")?;
        let parsed_obj = parsed
            .as_object()
            .ok_or_else(|| anyhow!("remote response is not an object"))?;
        let success = parsed_obj
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if success {
            return Ok(parsed_obj
                .get("result")
                .cloned()
                .unwrap_or_else(|| json!(null)));
        }

        let err = parsed_obj
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("remote tool execution failed");
        bail!("{}", err);
    }

    fn namespace(&self) -> &str {
        "op-dbus"
    }

    fn category(&self) -> &str {
        "remote"
    }
}

fn remote_tool_source_url() -> Option<String> {
    let mode = std::env::var("OP_WEB_TOOL_SOURCE")
        .ok()
        .map(|v| v.trim().to_lowercase());
    let use_remote = matches!(mode.as_deref(), Some("op-dbus") | Some("remote"))
        || std::env::var("OP_WEB_PULL_TOOLS_FROM_OP_DBUS")
            .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
    if !use_remote {
        return None;
    }

    if let Ok(url) = std::env::var("OP_WEB_REMOTE_TOOL_URL") {
        let trimmed = url.trim().trim_end_matches('/').to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }

    let host = std::env::var("OP_DBUS_WEB_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("OP_DBUS_WEB_PORT").unwrap_or_else(|_| "8081".to_string());
    Some(format!("http://{}:{}", host, port))
}

async fn register_remote_tools(registry: &Arc<ToolRegistry>, base_url: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/tools", base_url.trim_end_matches('/'));
    let response_body = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to fetch remote tools from {}", url))?
        .error_for_status()
        .with_context(|| format!("remote tools endpoint returned error: {}", url))?
        .text()
        .await
        .context("failed reading remote tools response body")?;

    let mut bytes = response_body.into_bytes();
    let payload: Value = simd_json::to_owned_value(&mut bytes)
        .context("failed to parse remote tools payload as JSON")?;
    let tool_entries = payload
        .as_object()
        .and_then(|o| o.get("tools"))
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("remote tools payload missing 'tools' array"))?;

    let mut registered = 0usize;
    for entry in tool_entries {
        let Some(entry_obj) = entry.as_object() else {
            continue;
        };
        let Some(name) = entry_obj.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }

        let description = entry_obj
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("Remote op-dbus tool")
            .to_string();
        let category = entry_obj
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("remote")
            .to_string();
        let input_schema = json!({
            "type": "object",
            "additionalProperties": true
        });

        let tool: BoxedTool = Arc::new(RemoteToolProxy {
            name: name.to_string(),
            description: description.clone(),
            input_schema: input_schema.clone(),
            base_url: base_url.trim_end_matches('/').to_string(),
            client: client.clone(),
        });
        let definition = ToolDefinition {
            name: name.to_string(),
            description,
            input_schema,
            schema_version: "https://json-schema.org/draft/next/schema".to_string(),
            category,
            tags: vec!["remote".to_string(), "op-dbus".to_string()],
            namespace: "op-dbus".to_string(),
        };
        registry
            .register(Arc::from(name), tool, definition)
            .await
            .with_context(|| format!("failed to register remote tool '{}'", name))?;
        registered += 1;
    }

    info!(
        "Registered {} remote tools from op-dbus ({})",
        registered, base_url
    );
    Ok(())
}

/// Register all tools from all sources
async fn register_all_tools(registry: &Arc<ToolRegistry>) -> anyhow::Result<()> {
    info!("Registering tools...");

    // Canonical tool/agent registration lives in op_tools/op_dbus.
    op_tools::register_builtin_tools(registry).await?;

    // Perform D-Bus tool discovery to populate the registry with 16k+ tools
    let introspection = Arc::new(op_introspection::IntrospectionService::new());
    let projection = op_tools::discovery::projection_engine::ProjectionEngine::new(introspection);

    // Discover from System bus
    info!("Discovering D-Bus tools (System bus)...");
    match projection
        .discover_all(registry, op_core::BusType::System)
        .await
    {
        Ok(count) => info!("Registered {} tools from D-Bus System bus", count),
        Err(e) => warn!("System bus discovery failed: {}", e),
    }

    // Also discover from Session bus for user-level D-Bus services
    info!("Discovering D-Bus tools (Session bus)...");
    let session_introspection = Arc::new(op_introspection::IntrospectionService::new());
    let session_projection =
        op_tools::discovery::projection_engine::ProjectionEngine::new(session_introspection);
    match session_projection
        .discover_all(registry, op_core::BusType::Session)
        .await
    {
        Ok(count) => info!("Registered {} tools from D-Bus Session bus", count),
        Err(e) => warn!("Session bus discovery failed: {}", e),
    }

    Ok(())
}

/// Log tool summary
fn log_tool_summary(tools: &[op_tools::registry::ToolDefinition]) {
    let ovs = tools.iter().filter(|t| t.name.starts_with("ovs_")).count();
    let dbus = tools.iter().filter(|t| t.name.starts_with("dbus_")).count();
    let file = tools.iter().filter(|t| t.name.starts_with("file_")).count();
    let shell = tools
        .iter()
        .filter(|t| t.name.starts_with("shell_"))
        .count();
    let agent = tools
        .iter()
        .filter(|t| t.name.starts_with("agent_"))
        .count();
    let other = tools.len() - ovs - dbus - file - shell - agent;

    debug!(
        "  OVS: {}, D-Bus: {}, File: {}, Shell: {}, Agents: {}, Other: {}",
        ovs, dbus, file, shell, agent, other
    );
}
