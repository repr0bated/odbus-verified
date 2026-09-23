//! Chat Actor - Central message processor
//!
//! The ChatActor is the "brain" of op-dbus-v2. It:
//! - Receives RPC requests from various frontends (web, MCP, CLI)
//! - Routes requests to appropriate handlers (tools, D-Bus, LLM)
//! - Manages sessions and conversation state
//! - **Executes tools with full tracking and accountability**
//! - Uses ForcedToolPipeline for anti-hallucination chat

use anyhow::Result;
use op_execution_tracker::ExecutionTracker;
use op_llm::provider::{ChatMessage as LlmChatMessage, LlmProvider};
use op_tools::ToolRegistry;
use serde::{Deserialize, Serialize};
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

use crate::forced_tool_pipeline::ForcedToolPipeline;
use crate::memory_loop::{MemoryLoop, CHATBOT_SOUL_ID};
use crate::session::SessionManager;
use crate::system_prompt::generate_system_prompt;
use crate::tool_executor::TrackedToolExecutor;

const DEFAULT_MAX_PROMPT_TOKENS: usize = 200_000;
const DEFAULT_MAX_MESSAGE_CHARS: usize = 80_000;

/// Configuration for ChatActor
#[derive(Debug, Clone)]
pub struct ChatActorConfig {
    /// Maximum concurrent requests
    pub max_concurrent: usize,
    /// Request timeout in seconds
    pub request_timeout_secs: u64,
    /// Enable execution tracking
    pub enable_tracking: bool,
    /// Maximum execution history to keep
    pub max_history: usize,
    /// Default LLM model to use
    pub default_model: String,
}

impl Default for ChatActorConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 10,
            request_timeout_secs: 300,
            enable_tracking: true,
            max_history: 1000,
            default_model: "default".to_string(),
        }
    }
}

/// RPC Request types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcRequest {
    /// List available tools
    ListTools {
        #[serde(default)]
        offset: Option<usize>,
        #[serde(default)]
        limit: Option<usize>,
    },

    /// Execute a tool
    ExecuteTool {
        name: String,
        arguments: Value,
        #[serde(default)]
        session_id: Option<String>,
    },

    /// Get tool definition
    GetTool { name: String },

    /// Chat with LLM
    Chat {
        message: String,
        session_id: String,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        container_id: Option<String>,
    },

    /// Get execution history
    GetHistory {
        #[serde(default)]
        limit: Option<usize>,
    },

    /// Get execution statistics
    GetStats,

    /// Health check
    Health,

    /// Introspect D-Bus service
    Introspect {
        service: String,
        #[serde(default)]
        bus_type: Option<String>,
    },

    /// Call D-Bus method
    DbusCall {
        service: String,
        path: String,
        interface: String,
        method: String,
        #[serde(default)]
        args: Value,
        #[serde(default)]
        bus_type: Option<String>,
    },
}

/// RPC Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
}

impl RpcResponse {
    pub fn success(result: Value) -> Self {
        Self {
            success: true,
            result: Some(result),
            error: None,
            execution_id: None,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            result: None,
            error: Some(msg.into()),
            execution_id: None,
        }
    }

    pub fn with_execution_id(mut self, id: &str) -> Self {
        self.execution_id = Some(id.to_string());
        self
    }
}

fn prompt_token_budget() -> usize {
    std::env::var("OP_CHAT_MAX_PROMPT_TOKENS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 1_000)
        .unwrap_or(DEFAULT_MAX_PROMPT_TOKENS)
}

fn max_message_chars() -> usize {
    std::env::var("OP_CHAT_MAX_MESSAGE_CHARS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 1_000)
        .unwrap_or(DEFAULT_MAX_MESSAGE_CHARS)
}

fn estimate_message_tokens(message: &LlmChatMessage) -> usize {
    (message.role.len() + message.content.len()).div_ceil(4) + 8
}

fn truncate_content(content: &str, max_chars: usize) -> String {
    let char_count = content.chars().count();
    if char_count <= max_chars {
        return content.to_string();
    }

    let keep_each_side = max_chars.saturating_sub(80) / 2;
    let head: String = content.chars().take(keep_each_side).collect();
    let tail: String = content
        .chars()
        .rev()
        .take(keep_each_side)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    format!(
        "{}\n\n[truncated {} chars from this message]\n\n{}",
        head,
        char_count.saturating_sub(keep_each_side * 2),
        tail
    )
}

fn core_message_to_llm(message: &op_core::ChatMessage, max_chars: usize) -> LlmChatMessage {
    let role = match message.role {
        op_core::ChatRole::User => "user",
        op_core::ChatRole::Assistant => "assistant",
        op_core::ChatRole::System => "system",
        op_core::ChatRole::Tool => "tool",
    };

    LlmChatMessage {
        role: role.to_string(),
        content: truncate_content(&message.content, max_chars),
        tool_calls: None,
        tool_call_id: None,
    }
}

fn push_recent_history_with_budget(
    messages: &mut Vec<LlmChatMessage>,
    history: &[op_core::ChatMessage],
    current_user_message: &str,
    token_budget: usize,
    max_chars: usize,
) -> usize {
    let current_message = LlmChatMessage::user(truncate_content(current_user_message, max_chars));
    let current_tokens = estimate_message_tokens(&current_message);
    let mut used_tokens: usize = messages.iter().map(estimate_message_tokens).sum();
    let mut selected = Vec::new();

    for hist_msg in history.iter().rev() {
        let llm_msg = core_message_to_llm(hist_msg, max_chars);
        let msg_tokens = estimate_message_tokens(&llm_msg);
        if used_tokens + current_tokens + msg_tokens > token_budget {
            break;
        }
        used_tokens += msg_tokens;
        selected.push(llm_msg);
    }

    let omitted = history.len().saturating_sub(selected.len());
    if omitted > 0 {
        messages.push(LlmChatMessage::system(format!(
            "{} older chat messages omitted to keep the proxy prompt under {} estimated tokens.",
            omitted, token_budget
        )));
    }

    selected.reverse();
    messages.extend(selected);
    messages.push(current_message);
    omitted
}

/// Message sent to the actor
struct ActorMessage {
    request: RpcRequest,
    respond_to: oneshot::Sender<RpcResponse>,
}

/// Handle to interact with ChatActor
#[derive(Clone)]
pub struct ChatActorHandle {
    sender: mpsc::Sender<ActorMessage>,
}

impl ChatActorHandle {
    /// Send a request and wait for response
    #[tracing::instrument(skip(self))]
    pub async fn call(&self, request: RpcRequest) -> Result<RpcResponse> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(ActorMessage {
                request,
                respond_to: tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Actor channel closed"))?;

        rx.await.map_err(|_| anyhow::anyhow!("Actor dropped"))
    }

    /// Fire and forget (for notifications)
    #[tracing::instrument(skip(self))]
    pub async fn notify(&self, request: RpcRequest) -> Result<()> {
        let (tx, _rx) = oneshot::channel();
        self.sender
            .send(ActorMessage {
                request,
                respond_to: tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Actor channel closed"))
    }

    // === Convenience methods ===

    #[tracing::instrument(skip(self))]
    pub async fn health(&self) -> RpcResponse {
        self.call(RpcRequest::Health)
            .await
            .unwrap_or_else(|e| RpcResponse::error(e.to_string()))
    }

    #[tracing::instrument(skip(self))]
    pub async fn list_tools(&self) -> RpcResponse {
        self.call(RpcRequest::ListTools {
            offset: None,
            limit: None,
        })
        .await
        .unwrap_or_else(|e| RpcResponse::error(e.to_string()))
    }

    #[tracing::instrument(skip(self))]
    pub async fn execute_tool(&self, request: op_core::ToolRequest) -> RpcResponse {
        self.call(RpcRequest::ExecuteTool {
            name: request.tool_name.clone(),
            arguments: request.arguments,
            session_id: None,
        })
        .await
        .unwrap_or_else(|e| RpcResponse::error(e.to_string()))
    }

    #[tracing::instrument(skip(self, message))]
    pub async fn chat(&self, session_id: Option<String>, message: &str) -> RpcResponse {
        let session_id = session_id.unwrap_or_else(|| "default".to_string());
        self.call(RpcRequest::Chat {
            message: message.to_string(),
            session_id,
            model: None,
            container_id: None,
        })
        .await
        .unwrap_or_else(|e| RpcResponse::error(e.to_string()))
    }

    #[tracing::instrument(skip(self))]
    pub async fn list_services(&self, _bus_type: op_core::BusType) -> RpcResponse {
        RpcResponse::error("List services not supported via RPC yet")
    }

    #[tracing::instrument(skip(self))]
    pub async fn introspect(
        &self,
        bus_type: op_core::BusType,
        service: &str,
        _path: &str,
    ) -> RpcResponse {
        let bus_str = match bus_type {
            op_core::BusType::Session => "session",
            op_core::BusType::System => "system",
        };

        self.call(RpcRequest::Introspect {
            service: service.to_string(),
            bus_type: Some(bus_str.to_string()),
        })
        .await
        .unwrap_or_else(|e| RpcResponse::error(e.to_string()))
    }
}

/// The Chat Actor - central processing unit
pub struct ChatActor {
    config: ChatActorConfig,
    tool_executor: Arc<TrackedToolExecutor>,
    tool_registry: Arc<ToolRegistry>,
    session_manager: Arc<SessionManager>,
    pipeline: Arc<ForcedToolPipeline>,
    llm_provider: Arc<dyn LlmProvider>,
    receiver: mpsc::Receiver<ActorMessage>,
    memory_loop: Option<Arc<MemoryLoop>>,
}

impl ChatActor {
    /// Create a new ChatActor with an LLM provider
    pub async fn new(
        config: ChatActorConfig,
        llm_provider: Arc<dyn LlmProvider>,
    ) -> Result<(Self, ChatActorHandle)> {
        let tool_registry = Arc::new(ToolRegistry::new());
        Self::build(config, tool_registry, llm_provider).await
    }

    /// Create with existing tool registry
    pub async fn with_registry(
        config: ChatActorConfig,
        tool_registry: Arc<ToolRegistry>,
        llm_provider: Arc<dyn LlmProvider>,
    ) -> Result<(Self, ChatActorHandle)> {
        Self::build(config, tool_registry, llm_provider).await
    }

    /// Internal builder shared by constructors
    async fn build(
        config: ChatActorConfig,
        tool_registry: Arc<ToolRegistry>,
        llm_provider: Arc<dyn LlmProvider>,
    ) -> Result<(Self, ChatActorHandle)> {
        let (sender, receiver) = mpsc::channel(config.max_concurrent);

        let tracker = Arc::new(ExecutionTracker::new(config.max_history));
        let tool_executor = Arc::new(TrackedToolExecutor::new(tool_registry.clone(), tracker));
        let session_manager = Arc::new(SessionManager::new());
        let pipeline = Arc::new(ForcedToolPipeline::new(
            tool_registry.clone(),
            tool_executor.clone(),
        ));

        // Register builtin tools so the LLM has tools to call
        if let Err(e) = op_tools::builtin::register_all_builtin_tools(&tool_registry).await {
            warn!("Failed to register some builtin tools: {}", e);
        }
        if let Err(e) = op_tools::builtin::register_response_tools(&tool_registry).await {
            warn!("Failed to register response tools: {}", e);
        }

        let tool_count = tool_registry.list().await.len();
        info!("ChatActor initialized with {} tools", tool_count);

        let actor = Self {
            config,
            tool_executor,
            tool_registry,
            session_manager,
            pipeline,
            llm_provider,
            receiver,
            memory_loop: None,
        };

        let handle = ChatActorHandle { sender };
        Ok((actor, handle))
    }

    /// Attach a memory loop — enables 4-layer session memory injection + post-turn persistence
    pub fn set_memory_loop(&mut self, loop_: Arc<MemoryLoop>) {
        self.memory_loop = Some(loop_);
    }

    /// Get tool registry for external registration
    pub fn tool_registry(&self) -> &Arc<ToolRegistry> {
        &self.tool_registry
    }

    /// Get tool executor
    pub fn tool_executor(&self) -> &Arc<TrackedToolExecutor> {
        &self.tool_executor
    }

    /// Get session manager
    pub fn session_manager(&self) -> &Arc<SessionManager> {
        &self.session_manager
    }

    /// Run the actor event loop
    pub async fn run(&mut self) {
        info!("ChatActor started");

        while let Some(msg) = self.receiver.recv().await {
            let response = self.handle_request(msg.request).await;
            let _ = msg.respond_to.send(response);
        }

        info!("ChatActor stopped");
    }

    /// Handle a single request
    async fn handle_request(&self, request: RpcRequest) -> RpcResponse {
        debug!(request = ?request, "Handling request");

        match request {
            RpcRequest::ListTools { offset, limit } => self.handle_list_tools(offset, limit).await,

            RpcRequest::ExecuteTool {
                name,
                arguments,
                session_id,
            } => self.handle_execute_tool(&name, arguments, session_id).await,

            RpcRequest::GetTool { name } => self.handle_get_tool(&name).await,

            RpcRequest::Chat {
                message,
                session_id,
                model,
                container_id,
            } => {
                self.handle_chat(&message, &session_id, model, container_id)
                    .await
            }

            RpcRequest::GetHistory { limit } => self.handle_get_history(limit.unwrap_or(50)).await,

            RpcRequest::GetStats => self.handle_get_stats().await,

            RpcRequest::Health => self.handle_health().await,

            RpcRequest::Introspect { service, bus_type } => {
                self.handle_introspect(&service, bus_type).await
            }

            RpcRequest::DbusCall {
                service,
                path,
                interface,
                method,
                args,
                bus_type,
            } => {
                self.handle_dbus_call(&service, &path, &interface, &method, args, bus_type)
                    .await
            }
        }
    }

    async fn handle_list_tools(&self, offset: Option<usize>, limit: Option<usize>) -> RpcResponse {
        let tools = self.tool_registry.list().await;

        let offset = offset.unwrap_or(0);
        let limit = limit.unwrap_or(tools.len());

        let paginated: Vec<_> = tools.into_iter().skip(offset).take(limit).collect();

        RpcResponse::success(json!({
            "tools": paginated,
            "total": paginated.len(),
            "offset": offset,
            "limit": limit
        }))
    }

    async fn handle_execute_tool(
        &self,
        _name: &str,
        _arguments: Value,
        _session_id: Option<String>,
    ) -> RpcResponse {
        // A chat session string is not a verified identity or an operation grant.
        RpcResponse::error("Legacy actor ExecuteTool lacks canonical admission; use PluginService/CallMethod with a verified SID1/OIA1 and declared capability")
    }

    async fn handle_get_tool(&self, name: &str) -> RpcResponse {
        match self.tool_registry.get(name).await {
            Some(tool) => RpcResponse::success(json!({
                "name": tool.name(),
                "description": tool.description(),
                "input_schema": tool.input_schema()
            })),
            None => RpcResponse::error(format!("Tool not found: {}", name)),
        }
    }

    async fn handle_chat(
        &self,
        message: &str,
        session_id: &str,
        model: Option<String>,
        container_id: Option<String>,
    ) -> RpcResponse {
        let model = model.as_deref().unwrap_or(&self.config.default_model);

        info!(
            session_id = %session_id,
            model = %model,
            container_id = ?container_id,
            "Processing chat message"
        );

        // Get or create session, retrieve history
        let session = self.session_manager.get_or_create(session_id).await;

        // Inject session memory if container_id present and memory_loop configured
        let session_memory =
            if let (Some(c_id), Some(mem_loop)) = (&container_id, &self.memory_loop) {
                match mem_loop
                    .inject_session_memory(CHATBOT_SOUL_ID, c_id, message)
                    .await
                {
                    Ok(mem) => {
                        info!(session_id = %session_id, container_id = %c_id, "Memory injected");
                        Some(mem)
                    }
                    Err(e) => {
                        warn!(
                            session_id = %session_id,
                            error = %e,
                            "Failed to inject session memory"
                        );
                        None
                    }
                }
            } else {
                None
            };

        // Build LLM message history
        let mut messages = Vec::new();

        // 1. Base system prompt with optional memory appended
        let system_msg = generate_system_prompt(session_memory.as_ref()).await;
        messages.push(system_msg);

        // 3. Add recent session history plus the new user message within a
        // bounded prompt budget. The full session remains persisted, but the
        // proxy request must not grow without limit.
        let token_budget = prompt_token_budget();
        let max_chars = max_message_chars();
        let omitted_messages = push_recent_history_with_budget(
            &mut messages,
            &session.messages,
            message,
            token_budget,
            max_chars,
        );
        if omitted_messages > 0 {
            warn!(
                session_id = %session_id,
                omitted_messages,
                token_budget,
                max_message_chars = max_chars,
                "Trimmed chat history before sending to LLM proxy"
            );
        }

        // Store user message in session
        self.session_manager
            .add_message(session_id, op_core::ChatMessage::user(message))
            .await;

        // 5. Process through ForcedToolPipeline
        let pipeline_result = self
            .pipeline
            .process_message(
                self.llm_provider.as_ref(),
                model,
                messages,
                Some(session_id.to_string()),
            )
            .await;

        match pipeline_result {
            Ok(result) => {
                // Store assistant response in session
                self.session_manager
                    .add_message(
                        session_id,
                        op_core::ChatMessage::assistant(&result.response),
                    )
                    .await;

                // Spawn non-blocking post-turn memory task
                if let (Some(c_id), Some(mem_loop)) =
                    (container_id.clone(), self.memory_loop.clone())
                {
                    mem_loop.spawn_post_turn_memory_task(
                        c_id,
                        message.to_string(),
                        result.response.clone(),
                        Vec::new(), // TODO: extract tool args from pipeline result
                    );
                }

                if !result.verified {
                    warn!(
                        session_id = %session_id,
                        issues = ?result.hallucination_check.issues,
                        "Response had hallucination issues"
                    );
                }

                RpcResponse::success(json!({
                    "response": result.response,
                    "verified": result.verified,
                    "tools_executed": result.executed_tools,
                    "session_id": session_id,
                }))
            }
            Err(e) => {
                error!(session_id = %session_id, error = %e, "Chat pipeline failed");
                RpcResponse::error(format!("Chat failed: {}", e))
            }
        }
    }

    async fn handle_get_history(&self, limit: usize) -> RpcResponse {
        let history = self.tool_executor.get_history(limit).await;
        RpcResponse::success(json!({
            "executions": history,
            "count": history.len()
        }))
    }

    async fn handle_get_stats(&self) -> RpcResponse {
        let stats = self.tool_executor.get_stats().await;
        RpcResponse::success(stats)
    }

    async fn handle_health(&self) -> RpcResponse {
        let tool_count = self.tool_registry.list().await.len();
        let session_count = self.session_manager.count().await;

        RpcResponse::success(json!({
            "status": "healthy",
            "tools_registered": tool_count,
            "active_sessions": session_count,
            "provider": format!("{:?}", self.llm_provider.provider_type()),
        }))
    }

    async fn handle_introspect(&self, _service: &str, _bus_type: Option<String>) -> RpcResponse {
        RpcResponse::error("Introspection service disabled")
    }

    async fn handle_dbus_call(
        &self,
        _service: &str,
        _path: &str,
        _interface: &str,
        _method: &str,
        _args: Value,
        _bus_type: Option<String>,
    ) -> RpcResponse {
        RpcResponse::error("Generic D-Bus call not implemented - use registered tools")
    }
}
