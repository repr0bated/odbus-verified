//! Anthropic Claude API Client
//!
//! ## API Endpoints
//!
//! | Endpoint | URL | Purpose |
//! |----------|-----|--------|
//! | Base URL | `https://api.anthropic.com/v1` | All Claude APIs |
//! | Messages | `/messages` | Chat completions |
//!
//! ## Authentication
//! - Header: `x-api-key: {ANTHROPIC_API_KEY}`
//! - Header: `anthropic-version: 2023-06-01`
//!
//! ## Tool Calling
//! Supports `tool_choice: {type: "any"}` to force tool usage (anti-hallucination)

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;
use std::time::Duration;
use tracing::{debug, info};

use crate::provider::{
    ChatMessage,
    ChatRequest,
    ChatResponse,
    LlmProvider,
    ModelInfo,
    ProviderType,
    TokenUsage,
    ToolCallInfo,
    // ToolChoice,
};

// =============================================================================
// API ENDPOINT CONFIGURATION
// =============================================================================

pub mod endpoints {
    pub const BASE_URL: &str = "https://api.anthropic.com/v1";
    pub const MESSAGES: &str = "/messages";
    pub const API_VERSION: &str = "2023-06-01";
}

// =============================================================================
// AUTHENTICATION
// =============================================================================

#[derive(Debug, Clone)]
enum AuthMethod {
    ApiKey(String),
    BearerToken(String),
}

// =============================================================================
// DATA STRUCTURES
// =============================================================================

const CLAUDE_MODELS: &[(&str, &str, &str)] = &[
    (
        "claude-sonnet-4-20250514",
        "Claude Sonnet 4",
        "Latest Sonnet model - best balance",
    ),
    (
        "claude-3-5-sonnet-20241022",
        "Claude 3.5 Sonnet",
        "Previous Sonnet - very capable",
    ),
    (
        "claude-3-opus-20240229",
        "Claude 3 Opus",
        "Most capable, slower",
    ),
    (
        "claude-3-haiku-20240307",
        "Claude 3 Haiku",
        "Fastest, most affordable",
    ),
];

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    /// Tools available to the model
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<Value>>,
    /// Tool choice: {type: "auto"}, {type: "any"}, {type: "tool", name: "..."}
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: AnthropicContent,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum AnthropicContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<ResponseContentBlock>,
    model: String,
    stop_reason: Option<String>,
    usage: Option<AnthropicUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ResponseContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

// =============================================================================
// CLIENT IMPLEMENTATION
// =============================================================================

pub struct AnthropicClient {
    client: Client,
    auth: AuthMethod,
    api_url: String,
}

impl AnthropicClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            auth: AuthMethod::ApiKey(api_key.into()),
            api_url: endpoints::BASE_URL.to_string(),
        }
    }

    pub fn with_oauth_token(token: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            auth: AuthMethod::BearerToken(token.into()),
            api_url: endpoints::BASE_URL.to_string(),
        }
    }

    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .context("ANTHROPIC_API_KEY environment variable not set")?;
        Ok(Self::new(api_key))
    }

    pub fn with_endpoint(api_key: impl Into<String>, endpoint: impl Into<String>) -> Self {
        let mut client = Self::new(api_key);
        client.api_url = endpoint.into();
        client
    }

    pub fn with_oauth_endpoint(token: impl Into<String>, endpoint: impl Into<String>) -> Self {
        let mut client = Self::with_oauth_token(token);
        client.api_url = endpoint.into();
        client
    }

    pub fn api_url(&self) -> &str {
        &self.api_url
    }

    /// Chat with full request configuration including tools
    async fn chat_with_tools(&self, model: &str, request: &ChatRequest) -> Result<ChatResponse> {
        let url = format!("{}/messages", self.api_url);

        // Extract system message
        let system_msg = request
            .messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| m.content.clone());

        // Convert messages (excluding system, handling tool results)
        let anthropic_messages: Vec<AnthropicMessage> = request
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                if m.role == "tool" {
                    // Tool result message
                    AnthropicMessage {
                        role: "user".to_string(),
                        content: AnthropicContent::Blocks(vec![ContentBlock::ToolResult {
                            tool_use_id: m.tool_call_id.clone().unwrap_or_default(),
                            content: m.content.clone(),
                        }]),
                    }
                } else if let Some(ref tool_calls) = m.tool_calls {
                    // Assistant message with tool calls
                    let blocks: Vec<ContentBlock> = tool_calls
                        .iter()
                        .map(|tc| ContentBlock::ToolUse {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            input: tc.arguments.clone(),
                        })
                        .collect();
                    AnthropicMessage {
                        role: m.role.clone(),
                        content: AnthropicContent::Blocks(blocks),
                    }
                } else {
                    AnthropicMessage {
                        role: m.role.clone(),
                        content: AnthropicContent::Text(m.content.clone()),
                    }
                }
            })
            .collect();

        // Convert tools to Anthropic format
        let tools: Option<Vec<Value>> = if request.tools.is_empty() {
            None
        } else {
            Some(
                request
                    .tools
                    .iter()
                    .map(|t| t.to_anthropic_format())
                    .collect(),
            )
        };

        // Convert tool_choice to Anthropic format
        let tool_choice: Option<Value> = if request.tools.is_empty() {
            None
        } else {
            Some(request.tool_choice.to_anthropic_format())
        };

        let api_request = AnthropicRequest {
            model: model.to_string(),
            messages: anthropic_messages,
            max_tokens: request.max_tokens.unwrap_or(4096),
            system: system_msg,
            temperature: request.temperature.or(Some(0.7)),
            tools,
            tool_choice,
        };

        debug!(
            "Anthropic request to: {} with tool_choice: {:?}",
            url, request.tool_choice
        );

        let mut req_builder = self
            .client
            .post(&url)
            .header("anthropic-version", endpoints::API_VERSION)
            .header("Content-Type", "application/json");

        req_builder = match &self.auth {
            AuthMethod::ApiKey(key) => req_builder.header("x-api-key", key.as_str()),
            AuthMethod::BearerToken(token) => {
                req_builder.header("Authorization", format!("Bearer {}", token))
            }
        };

        let response = req_builder
            .json(&api_request)
            .send()
            .await
            .context("Failed to send Anthropic request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Anthropic API error {}: {}", status, body));
        }

        let result: AnthropicResponse = response
            .json()
            .await
            .context("Failed to parse Anthropic response")?;

        // Extract text and tool calls from response
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for block in result.content {
            match block {
                ResponseContentBlock::Text { text } => text_parts.push(text),
                ResponseContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(ToolCallInfo {
                        id,
                        name,
                        arguments: input,
                    });
                }
            }
        }

        let text = text_parts.join("");
        let tool_calls_opt = if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls.clone())
        };

        let usage = result.usage.map(|u| TokenUsage {
            prompt_tokens: u.input_tokens,
            completion_tokens: u.output_tokens,
            total_tokens: u.input_tokens + u.output_tokens,
        });

        Ok(ChatResponse {
            message: ChatMessage {
                role: "assistant".to_string(),
                content: text,
                tool_calls: tool_calls_opt.clone(),
                tool_call_id: None,
            },
            model: result.model,
            provider: "anthropic".to_string(),
            finish_reason: result.stop_reason,
            usage,
            tool_calls: tool_calls_opt,
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicClient {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Anthropic
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        info!("Anthropic models (static list)");
        info!("  Endpoint: {}", self.api_url);

        Ok(CLAUDE_MODELS
            .iter()
            .map(|(id, name, desc)| ModelInfo {
                id: id.to_string(),
                name: name.to_string(),
                description: Some(desc.to_string()),
                parameters: None,
                available: true,
                tags: vec!["claude".to_string()],
                downloads: None,
                updated_at: None,
            })
            .collect())
    }

    async fn search_models(&self, query: &str, limit: usize) -> Result<Vec<ModelInfo>> {
        let query_lower = query.to_lowercase();
        let models = self.list_models().await?;
        Ok(models
            .into_iter()
            .filter(|m| {
                m.id.to_lowercase().contains(&query_lower)
                    || m.name.to_lowercase().contains(&query_lower)
            })
            .take(limit)
            .collect())
    }

    async fn get_model(&self, model_id: &str) -> Result<Option<ModelInfo>> {
        let models = self.list_models().await?;
        Ok(models.into_iter().find(|m| m.id == model_id))
    }

    async fn is_model_available(&self, model_id: &str) -> Result<bool> {
        Ok(CLAUDE_MODELS.iter().any(|(id, _, _)| *id == model_id))
    }

    /// Basic chat without tools
    async fn chat(&self, model: &str, messages: Vec<ChatMessage>) -> Result<ChatResponse> {
        let request = ChatRequest::new(messages);
        self.chat_with_request(model, request).await
    }

    /// Chat with full request configuration including tools and tool_choice
    async fn chat_with_request(&self, model: &str, request: ChatRequest) -> Result<ChatResponse> {
        info!(
            "Anthropic chat: model={}, endpoint={}, tool_choice={:?}",
            model, self.api_url, request.tool_choice
        );
        self.chat_with_tools(model, &request).await
    }

    async fn chat_stream(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<String>>> {
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let response = self.chat(model, messages).await?;
        tx.send(Ok(response.message.content)).await.ok();
        Ok(rx)
    }
}
