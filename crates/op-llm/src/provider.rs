//! LLM Provider Traits and Types
//!
//! This module defines the common interface for all LLM providers
//! including tool calling support with REQUIRED tool_choice.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::fmt;
use std::str::FromStr;

/// Provider types
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProviderType {
    Anthropic,
    Antigravity,
    Assistant,
    Factory,
    Gemini,
    GeminiCli,
    HuggingFace,
    OpenAI,
    OpenClaw,
    Perplexity,
    Salad,
    Custom(String),
}

impl fmt::Display for ProviderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderType::Anthropic => write!(f, "anthropic"),
            ProviderType::Antigravity => write!(f, "antigravity"),
            ProviderType::Assistant => write!(f, "assistant"),
            ProviderType::Factory => write!(f, "factory"),
            ProviderType::Gemini => write!(f, "gemini"),
            ProviderType::GeminiCli => write!(f, "gemini-cli"),
            ProviderType::HuggingFace => write!(f, "huggingface"),
            ProviderType::OpenAI => write!(f, "openai"),
            ProviderType::OpenClaw => write!(f, "openclaw"),
            ProviderType::Perplexity => write!(f, "perplexity"),
            ProviderType::Salad => write!(f, "salad"),
            ProviderType::Custom(name) => write!(f, "{}", name),
        }
    }
}

impl FromStr for ProviderType {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "anthropic" => Ok(ProviderType::Anthropic),
            "antigravity" => Ok(ProviderType::Antigravity),
            "assistant" => Ok(ProviderType::Assistant),
            "factory" | "codex" => Ok(ProviderType::Factory),
            "google" | "google-ai" | "google_ai" => Ok(ProviderType::Antigravity),
            "gemini" => Ok(ProviderType::Gemini),
            "gemini-cli" | "gemini_cli" | "geminicli" => Ok(ProviderType::GeminiCli),
            "huggingface" | "hugging_face" | "hf" => Ok(ProviderType::HuggingFace),
            "opencode" | "kilocode" | "kilo-code" | "kilo_code" | "openrouter" | "open-router"
            | "open_router" => Ok(ProviderType::Custom(s.to_lowercase())),
            "openai" | "open_ai" => Ok(ProviderType::OpenAI),
            "openclaw" | "open_claw" => Ok(ProviderType::OpenClaw),
            "perplexity" => Ok(ProviderType::Perplexity),
            "salad" | "salad-ai" | "salad_ai" => Ok(ProviderType::Salad),
            other => Err(format!("Unknown provider type: {}", other)),
        }
    }
}

/// Chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

/// Tool call information from LLM response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// Tool definition for LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: simd_json::OwnedValue,
    #[serde(default)]
    pub schema_version: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub namespace: String,
}

impl ToolDefinition {
    pub fn to_anthropic_format(&self) -> simd_json::OwnedValue {
        simd_json::json!({
            "name": self.name,
            "description": self.description,
            "input_schema": self.input_schema
        })
    }

    pub fn to_openai_format(&self) -> simd_json::OwnedValue {
        simd_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.input_schema
            }
        })
    }
}

/// Tool choice for LLM request
///
/// IMPORTANT: Use `Required` to force the LLM to use tools.
/// This is essential for the anti-hallucination architecture.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoice {
    /// Let LLM decide whether to use tools (NOT RECOMMENDED)
    #[default]
    Auto,
    /// FORCE LLM to use a tool (REQUIRED for anti-hallucination)
    Required,
    /// Disable tool usage
    None,
    /// Force specific tool
    Tool(String),
}

impl ToolChoice {
    /// Convert to OpenAI/HuggingFace format
    pub fn to_api_format(&self) -> Value {
        match self {
            ToolChoice::Auto => simd_json::json!("auto"),
            ToolChoice::Required => simd_json::json!("required"),
            ToolChoice::None => simd_json::json!("none"),
            ToolChoice::Tool(name) => simd_json::json!({
                "type": "function",
                "function": {"name": name}
            }),
        }
    }

    pub fn to_anthropic_format(&self) -> Value {
        match self {
            ToolChoice::Auto => simd_json::json!({"type": "auto"}),
            ToolChoice::Required => simd_json::json!({"type": "any"}),
            ToolChoice::None => simd_json::json!({"type": "none"}),
            ToolChoice::Tool(name) => simd_json::json!({
                "type": "tool",
                "name": name
            }),
        }
    }
}

/// Full chat request with tools
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: ToolChoice,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
}

impl ChatRequest {
    pub fn new(messages: Vec<ChatMessage>) -> Self {
        Self {
            messages,
            tools: Vec::new(),
            tool_choice: ToolChoice::Auto,
            max_tokens: None,
            temperature: None,
            top_p: None,
        }
    }

    /// Create request with FORCED tool usage
    pub fn with_forced_tools(messages: Vec<ChatMessage>, tools: Vec<ToolDefinition>) -> Self {
        Self {
            messages,
            tools,
            tool_choice: ToolChoice::Required, // ◄── FORCE TOOL USE
            max_tokens: None,
            temperature: Some(0.7),
            top_p: None,
        }
    }

    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = choice;
        self
    }

    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = Some(tokens);
        self
    }

    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = Some(temp);
        self
    }
}

/// Token usage information
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Provider configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub organization_id: Option<String>,
    pub project_id: Option<String>,
}

/// Chat response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub message: ChatMessage,
    pub model: String,
    pub provider: String,
    pub finish_reason: Option<String>,
    pub usage: Option<TokenUsage>,
    pub tool_calls: Option<Vec<ToolCallInfo>>,
}

/// Model information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub parameters: Option<String>,
    pub available: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    pub downloads: Option<u64>,
    pub updated_at: Option<String>,
}

/// Boxed provider for dynamic dispatch
pub type BoxedProvider = Box<dyn LlmProvider + Send + Sync>;

/// Provider capabilities
#[derive(Debug, Clone)]
pub struct ProviderCapabilities {
    pub streaming: bool,
    pub tool_use: bool,
    pub vision: bool,
    pub embeddings: bool,
    pub max_context_length: usize,
}

/// Streaming chunk for real-time responses
#[derive(Debug, Clone)]
pub struct StreamChunk {
    pub content: String,
    pub finish_reason: Option<String>,
    pub usage: Option<TokenUsage>,
}

/// LLM Provider trait
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Get provider type
    fn provider_type(&self) -> ProviderType;

    /// List available models
    async fn list_models(&self) -> Result<Vec<ModelInfo>>;

    /// Search models
    async fn search_models(&self, query: &str, limit: usize) -> Result<Vec<ModelInfo>>;

    /// Get model info
    async fn get_model(&self, model_id: &str) -> Result<Option<ModelInfo>>;

    /// Check if model is available
    async fn is_model_available(&self, model_id: &str) -> Result<bool>;

    /// Basic chat (no tools) - AVOID USING THIS
    async fn chat(&self, model: &str, messages: Vec<ChatMessage>) -> Result<ChatResponse>;

    /// Chat with full request including tools - USE THIS
    ///
    /// Implementations MUST:
    /// 1. Pass tools to the API
    /// 2. Set tool_choice according to request
    /// 3. Parse tool_calls from response
    async fn chat_with_request(&self, model: &str, request: ChatRequest) -> Result<ChatResponse> {
        // Default implementation ignores tools - OVERRIDE THIS!
        tracing::warn!("chat_with_request using default implementation - tools ignored!");
        self.chat(model, request.messages).await
    }

    /// Streaming chat
    async fn chat_stream(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<String>>>;
}
