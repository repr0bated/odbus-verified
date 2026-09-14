//! HuggingFace LLM Provider with FORCED tool support
//!
//! This implementation properly passes tools and tool_choice to the API.

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::time::Duration;
use tracing::{debug, info, warn};

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

const HF_API_BASE: &str = "https://api-inference.huggingface.co";
const DEFAULT_MODEL: &str = "meta-llama/Llama-3.3-70B-Instruct";

pub struct HuggingFaceClient {
    client: Client,
    api_key: String,
    base_url: String,
}

impl HuggingFaceClient {
    pub fn new(api_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            api_key,
            base_url: HF_API_BASE.to_string(),
        }
    }

    pub fn with_base_url(mut self, url: &str) -> Self {
        self.base_url = url.to_string();
        self
    }

    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("HF_TOKEN")
            .or_else(|_| std::env::var("HUGGINGFACE_TOKEN"))
            .context("HF_TOKEN or HUGGINGFACE_TOKEN must be set")?;
        Ok(Self::new(api_key))
    }
}

#[async_trait]
impl LlmProvider for HuggingFaceClient {
    fn provider_type(&self) -> ProviderType {
        ProviderType::HuggingFace
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        // Return commonly used models
        Ok(vec![
            ModelInfo {
                id: "meta-llama/Llama-3.3-70B-Instruct".to_string(),
                name: "Llama 3.3 70B Instruct".to_string(),
                description: Some("Meta's Llama 3.3 70B with instruction tuning".to_string()),
                parameters: Some("70B".to_string()),
                available: true,
                tags: vec!["llama".to_string(), "instruct".to_string()],
                downloads: None,
                updated_at: None,
            },
            ModelInfo {
                id: "mistralai/Mixtral-8x7B-Instruct-v0.1".to_string(),
                name: "Mixtral 8x7B Instruct".to_string(),
                description: Some("Mistral's MoE model".to_string()),
                parameters: Some("46.7B".to_string()),
                available: true,
                tags: vec!["mixtral".to_string(), "moe".to_string()],
                downloads: None,
                updated_at: None,
            },
        ])
    }

    async fn search_models(&self, query: &str, limit: usize) -> Result<Vec<ModelInfo>> {
        let models = self.list_models().await?;
        Ok(models
            .into_iter()
            .filter(|m| m.name.to_lowercase().contains(&query.to_lowercase()))
            .take(limit)
            .collect())
    }

    async fn get_model(&self, model_id: &str) -> Result<Option<ModelInfo>> {
        let models = self.list_models().await?;
        Ok(models.into_iter().find(|m| m.id == model_id))
    }

    async fn is_model_available(&self, model_id: &str) -> Result<bool> {
        Ok(self.get_model(model_id).await?.is_some())
    }

    async fn chat(&self, model: &str, messages: Vec<ChatMessage>) -> Result<ChatResponse> {
        // Simple chat without tools - NOT RECOMMENDED
        warn!("Using chat() without tools - consider using chat_with_request()");

        let request = ChatRequest::new(messages);
        self.chat_with_request(model, request).await
    }

    /// Chat with FULL tool support
    ///
    /// This is the CORRECT implementation that:
    /// 1. Passes tools to the API
    /// 2. Sets tool_choice (including "required")
    /// 3. Parses tool_calls from response
    async fn chat_with_request(&self, model: &str, request: ChatRequest) -> Result<ChatResponse> {
        let model = if model.is_empty() {
            DEFAULT_MODEL
        } else {
            model
        };
        let url = format!("{}/models/{}/v1/chat/completions", self.base_url, model);

        // Convert messages to API format
        let messages: Vec<Value> = request
            .messages
            .iter()
            .map(|m| {
                let mut msg = json!({
                    "role": m.role,
                    "content": m.content
                });

                // Add tool_call_id for tool responses
                if let Some(ref id) = m.tool_call_id {
                    msg["tool_call_id"] = json!(id);
                }

                // Add tool_calls for assistant messages
                if let Some(ref calls) = m.tool_calls {
                    msg["tool_calls"] = json!(calls.iter().map(|tc| {
                        json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": simd_json::to_string(&tc.arguments).unwrap_or_default()
                            }
                        })
                    }).collect::<Vec<_>>());
                }

                msg
            })
            .collect();

        // Convert tools to API format
        let tools: Vec<Value> = request.tools.iter().map(|t| t.to_openai_format()).collect();

        // Build request body
        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": false
        });

        // Add tools if present
        if !tools.is_empty() {
            body["tools"] = json!(tools);

            // CRITICAL: Set tool_choice
            body["tool_choice"] = request.tool_choice.to_api_format();

            info!(
                "Sending request with {} tools, tool_choice={:?}",
                tools.len(),
                request.tool_choice
            );
        }

        // Add optional parameters
        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }
        if let Some(temp) = request.temperature {
            body["temperature"] = json!(temp);
        }
        if let Some(top_p) = request.top_p {
            body["top_p"] = json!(top_p);
        }

        debug!(
            "HuggingFace request: {}",
            simd_json::to_string_pretty(&body).unwrap_or_default()
        );

        // Send request
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .context("Failed to send request to HuggingFace")?;

        let status = response.status();
        let response_text = response.text().await?;

        debug!(
            "HuggingFace response ({}): {}",
            status,
            &response_text[..response_text.len().min(500)]
        );

        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "HuggingFace API error ({}): {}",
                status,
                response_text
            ));
        }

        // Parse response
        let mut response_text_mut = response_text;
        let response_json: Value =
            unsafe { simd_json::from_str(&mut response_text_mut) }.map_err(|e| {
                anyhow::anyhow!(
                    "Failed to parse HuggingFace response: {}. Body: {}",
                    e,
                    response_text_mut
                )
            })?;

        let choice = response_json
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.get(0))
            .ok_or_else(|| anyhow::anyhow!("No choices returned from HuggingFace"))?;

        let message = choice
            .get("message")
            .ok_or_else(|| anyhow::anyhow!("No message in response"))?;

        let content = message
            .get("content")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        let role = message
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("assistant")
            .to_string();

        // Parse tool_calls from response
        let tool_calls: Option<Vec<ToolCallInfo>> = message
            .get("tool_calls")
            .and_then(|tc| tc.as_array())
            .map(|calls| {
                calls
                    .iter()
                    .filter_map(|call| {
                        let id = call.get("id")?.as_str()?.to_string();
                        let function = call.get("function")?;
                        let name = function.get("name")?.as_str()?.to_string();
                        let args_str = function.get("arguments")?.as_str()?;
                        let mut args_mut = args_str.to_string();
                        let arguments: Value =
                            unsafe { simd_json::from_str(&mut args_mut) }.ok()?;

                        Some(ToolCallInfo {
                            id,
                            name,
                            arguments,
                        })
                    })
                    .collect()
            });

        if let Some(ref calls) = tool_calls {
            info!("Parsed {} tool calls from response", calls.len());
            for call in calls {
                debug!("  Tool call: {} ({})", call.name, call.id);
            }
        }

        let finish_reason = choice
            .get("finish_reason")
            .and_then(|f| f.as_str())
            .map(|s| s.to_string());

        let usage = response_json.get("usage").map(|u| TokenUsage {
            prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            completion_tokens: u
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        });

        Ok(ChatResponse {
            message: ChatMessage {
                role,
                content,
                tool_calls: tool_calls.clone(),
                tool_call_id: None,
            },
            model: model.to_string(),
            provider: "huggingface".to_string(),
            finish_reason,
            usage,
            tool_calls,
        })
    }

    async fn chat_stream(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<String>>> {
        // Streaming not implemented for this example
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let response = self.chat(model, messages).await?;
        let _ = tx.send(Ok(response.message.content)).await;
        Ok(rx)
    }
}
