//! Salad AI Gateway LLM Provider
//!
//! Connects to the Salad AI Gateway — a managed, OpenAI-compatible LLM API
//! powered by SaladCloud's distributed GPU network — over HTTPS.
//!
//! ## Configuration
//!
//! ```bash
//! SALAD_API_KEY=salad-...            # required (Bearer auth)
//! SALAD_BASE_URL=https://ai.salad.cloud/v1  # default
//! SALAD_DEFAULT_MODEL=qwen3.6-35b-a3b        # default
//! SALAD_MAX_TOKENS=4096              # default cap when a caller doesn't set one; "0"/"unlimited" disables it
//! ```
//!
//! The API key is read from the environment only and is never embedded in
//! source. Authentication uses the standard `Authorization: Bearer` header.
//!
//! ## Reasoning models
//!
//! The Qwen models served behind this gateway emit a `reasoning` block ahead
//! of `content`. A `max_tokens` budget that's too tight for that reasoning
//! phase truncates the response with `finish_reason: "length"` and an empty
//! `content` — [`SaladProvider::chat_with_request`] falls back to `reasoning`
//! in that case so callers never see a silently empty reply.

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::provider::{
    ChatMessage, ChatRequest, ChatResponse, LlmProvider, ModelInfo, ProviderType, TokenUsage,
    ToolCallInfo,
};

const DEFAULT_BASE_URL: &str = "https://ai.salad.cloud/v1";
const DEFAULT_MODEL: &str = "qwen3.6-35b-a3b";
const API_KEY_ENV: &str = "SALAD_API_KEY";
/// Fallback `max_tokens` applied only when a caller doesn't set one — sized
/// to absorb the reasoning-model preamble seen in practice (~200-300 tokens
/// for trivial prompts) while still bounding worst-case latency/cost.
const DEFAULT_MAX_TOKENS: u32 = 4096;

pub struct SaladProvider {
    client: Client,
    base_url: String,
    default_model: String,
    api_key: Option<String>,
    /// `None` means unbounded (no `max_tokens` sent when the caller omits one).
    default_max_tokens: Option<u32>,
}

impl SaladProvider {
    pub fn new(
        api_key: Option<String>,
        base_url: Option<String>,
        default_model: Option<String>,
    ) -> Self {
        Self::with_max_tokens(api_key, base_url, default_model, Some(DEFAULT_MAX_TOKENS))
    }

    pub fn with_max_tokens(
        api_key: Option<String>,
        base_url: Option<String>,
        default_model: Option<String>,
        default_max_tokens: Option<u32>,
    ) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url
                .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
                .trim_end_matches('/')
                .to_string(),
            default_model: default_model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            api_key,
            default_max_tokens,
        }
    }

    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var(API_KEY_ENV).ok();
        let base_url = std::env::var("SALAD_BASE_URL").ok();
        let default_model = std::env::var("SALAD_DEFAULT_MODEL").ok();
        let default_max_tokens = match std::env::var("SALAD_MAX_TOKENS") {
            Ok(v) if v.eq_ignore_ascii_case("unlimited") || v == "0" => None,
            Ok(v) => match v.parse::<u32>() {
                Ok(n) => Some(n),
                Err(_) => {
                    warn!(
                        "Invalid SALAD_MAX_TOKENS={:?}, using default {}",
                        v, DEFAULT_MAX_TOKENS
                    );
                    Some(DEFAULT_MAX_TOKENS)
                }
            },
            Err(_) => Some(DEFAULT_MAX_TOKENS),
        };
        Ok(Self::with_max_tokens(
            api_key,
            base_url,
            default_model,
            default_max_tokens,
        ))
    }

    fn models_url(&self) -> String {
        format!("{}/models", self.base_url)
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    fn resolve_model(&self, model: &str) -> String {
        if model.is_empty() {
            self.default_model.clone()
        } else {
            model.to_string()
        }
    }

    fn api_request(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut builder = builder.header("Content-Type", "application/json");
        if let Some(ref key) = self.api_key {
            builder = builder.bearer_auth(key);
        }
        builder
    }

    fn declared_model_info(&self) -> ModelInfo {
        ModelInfo {
            id: self.default_model.clone(),
            name: self.default_model.clone(),
            description: Some(
                "Salad AI Gateway Qwen model (managed, OpenAI-compatible)".to_string(),
            ),
            parameters: None,
            available: true,
            tags: vec![
                "salad".to_string(),
                "qwen".to_string(),
                "remote".to_string(),
            ],
            downloads: None,
            updated_at: None,
        }
    }

    fn parse_models_response(response_text: &str) -> Result<Vec<ModelInfo>> {
        let mut response_text_mut = response_text.to_string();
        let response_json: Value = unsafe { simd_json::from_str(&mut response_text_mut) }
            .map_err(|e| anyhow::anyhow!("Failed to parse Salad models response: {}", e))?;

        let models = response_json
            .get("data")
            .and_then(|v| v.as_array())
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        let id = entry.get("id")?.as_str()?.to_string();
                        let owned_by = entry
                            .get("owned_by")
                            .and_then(|v| v.as_str())
                            .unwrap_or("salad")
                            .to_string();

                        Some(ModelInfo {
                            id: id.clone(),
                            name: id,
                            description: Some(format!("Salad model owned by {}", owned_by)),
                            parameters: None,
                            available: true,
                            tags: vec!["salad".to_string(), owned_by],
                            downloads: None,
                            updated_at: None,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(models)
    }
}

#[async_trait]
impl LlmProvider for SaladProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Salad
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        if self.api_key.is_none() {
            warn!("SALAD_API_KEY not set; returning declared default model only");
            return Ok(vec![self.declared_model_info()]);
        }

        let response = self
            .api_request(self.client.get(self.models_url()))
            .send()
            .await
            .context("Failed to query Salad models")?;

        let status = response.status();
        let response_text = response.text().await?;

        if !status.is_success() {
            warn!(
                "Salad model listing failed ({}), falling back to declared default model",
                status
            );
            return Ok(vec![self.declared_model_info()]);
        }

        let mut models = match Self::parse_models_response(&response_text) {
            Ok(models) => models,
            Err(err) => {
                warn!(
                    "Salad /models did not return a usable model list ({}), falling back to declared default model",
                    err
                );
                return Ok(vec![self.declared_model_info()]);
            }
        };
        if models.is_empty() {
            models.push(self.declared_model_info());
        }

        Ok(models)
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
        warn!("Using chat() without tools - consider using chat_with_request()");
        let request = ChatRequest::new(messages);
        self.chat_with_request(model, request).await
    }

    async fn chat_with_request(&self, model: &str, request: ChatRequest) -> Result<ChatResponse> {
        let model = self.resolve_model(model);
        let url = self.chat_url();

        let messages: Vec<Value> = request
            .messages
            .iter()
            .map(|m| {
                let mut msg = json!({
                    "role": m.role,
                    "content": m.content
                });

                if let Some(ref id) = m.tool_call_id {
                    msg["tool_call_id"] = json!(id);
                }

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

        let tools: Vec<Value> = request.tools.iter().map(|t| t.to_openai_format()).collect();

        let mut body = json!({
            "model": model,
            "messages": messages,
            "stream": false
        });
        let body_object = body
            .as_object_mut()
            .expect("salad request body should be an object");

        if !tools.is_empty() {
            body_object.insert("tools".into(), json!(tools));
            body_object.insert("tool_choice".into(), request.tool_choice.to_api_format());
            info!(
                "Salad request with {} tools, tool_choice={:?}",
                tools.len(),
                request.tool_choice
            );
        }

        // Caller-supplied max_tokens always wins; otherwise fall back to the
        // provider default so reasoning models get enough headroom to answer.
        if let Some(max_tokens) = request.max_tokens.or(self.default_max_tokens) {
            body_object.insert("max_tokens".into(), json!(max_tokens));
        }
        if let Some(temp) = request.temperature {
            body_object.insert("temperature".into(), json!(temp));
        }
        if let Some(top_p) = request.top_p {
            body_object.insert("top_p".into(), json!(top_p));
        }

        debug!(
            "Salad request: {}",
            simd_json::to_string_pretty(&body).unwrap_or_default()
        );

        let response = self
            .api_request(self.client.post(&url))
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Salad")?;

        let status = response.status();
        let response_text = response.text().await?;

        debug!(
            "Salad response ({}): {}",
            status,
            &response_text[..response_text.len().min(500)]
        );

        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Salad API error ({}): {}",
                status,
                response_text
            ));
        }

        let mut response_text_mut = response_text;
        let response_json: Value =
            unsafe { simd_json::from_str(&mut response_text_mut) }.map_err(|e| {
                anyhow::anyhow!(
                    "Failed to parse Salad response: {}. Body: {}",
                    e,
                    response_text_mut
                )
            })?;

        let choice = response_json
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .ok_or_else(|| anyhow::anyhow!("No choices returned from Salad"))?;

        let message = choice
            .get("message")
            .ok_or_else(|| anyhow::anyhow!("No message in Salad response"))?;

        let reasoning = message.get("reasoning").and_then(|c| c.as_str());
        let has_tool_calls = message
            .get("tool_calls")
            .and_then(|tc| tc.as_array())
            .is_some_and(|a| !a.is_empty());

        let content = match message.get("content").and_then(|c| c.as_str()) {
            Some(c) if !c.is_empty() => c.to_string(),
            _ if has_tool_calls => String::new(),
            // Reasoning models (e.g. Qwen) can exhaust max_tokens mid-"thinking"
            // and return empty content; surface the reasoning trace instead of
            // silently handing the caller an empty reply.
            _ => match reasoning {
                Some(r) if !r.is_empty() => {
                    warn!(
                        "Salad: empty content, falling back to reasoning trace ({} chars) - consider raising max_tokens",
                        r.len()
                    );
                    r.to_string()
                }
                _ => String::new(),
            },
        };

        let role = message
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("assistant")
            .to_string();

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
            info!("Salad: parsed {} tool calls", calls.len());
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
            model,
            provider: "salad".to_string(),
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
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let response = self.chat(model, messages).await?;
        let _ = tx.send(Ok(response.message.content)).await;
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_model_is_qwen_default() {
        let provider = SaladProvider::new(None, None, None);
        let info = provider.declared_model_info();
        assert_eq!(info.id, "qwen3.6-35b-a3b");
        assert!(info.tags.iter().any(|t| t == "salad"));
    }

    #[test]
    fn resolves_empty_model_to_default() {
        let provider = SaladProvider::new(None, None, Some("qwen3.5-9b".to_string()));
        assert_eq!(provider.resolve_model(""), "qwen3.5-9b");
        assert_eq!(provider.resolve_model("qwen3.6-27b"), "qwen3.6-27b");
    }

    #[test]
    fn from_env_reads_key() {
        std::env::set_var("SALAD_API_KEY", "salad-test-key");
        let provider = SaladProvider::from_env().expect("from_env should succeed");
        assert_eq!(provider.api_key.as_deref(), Some("salad-test-key"));
        std::env::remove_var("SALAD_API_KEY");
    }
}
