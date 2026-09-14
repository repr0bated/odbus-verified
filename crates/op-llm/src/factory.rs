//! Factory LLM Provider
//!
//! Connects to Factory AI via an explicitly configured OpenAI-compatible
//! `/v1/chat/completions` endpoint.
//!
//! ## Configuration
//!
//! ```bash
//! FACTORY_BASE_URL=http://127.0.0.1:<factory-port>/v1
//! FACTORY_API_KEY=<token>
//! FACTORY_DEFAULT_MODEL=<model>
//! ```

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

const DEFAULT_API_KEY: &str = "";
const DEFAULT_MODEL: &str = "factory-default";

pub struct FactoryProvider {
    client: Client,
    base_url: String,
    api_key: String,
    default_model: String,
}

impl FactoryProvider {
    pub fn new(
        base_url: Option<String>,
        api_key: Option<String>,
        default_model: Option<String>,
    ) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(180))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url
                .unwrap_or_default()
                .trim_end_matches('/')
                .to_string(),
            api_key: api_key.unwrap_or_else(|| DEFAULT_API_KEY.to_string()),
            default_model: default_model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        }
    }

    pub fn from_env() -> Result<Self> {
        let base_url = std::env::var("FACTORY_BASE_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("FACTORY_BASE_URL is not configured"))?;
        let api_key = std::env::var("FACTORY_API_KEY").ok();
        let default_model = std::env::var("FACTORY_DEFAULT_MODEL").ok();
        Ok(Self::new(Some(base_url), api_key, default_model))
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
        builder
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.api_key))
    }

    fn fallback_model_info(&self) -> ModelInfo {
        ModelInfo {
            id: self.default_model.clone(),
            name: self.default_model.clone(),
            description: Some("Factory AI via configured endpoint".to_string()),
            parameters: None,
            available: true,
            tags: vec!["factory".to_string(), "default".to_string()],
            downloads: None,
            updated_at: None,
        }
    }

    fn parse_models_response(response_text: &str) -> Result<Vec<ModelInfo>> {
        let mut response_text_mut = response_text.to_string();
        let response_json: Value = unsafe { simd_json::from_str(&mut response_text_mut) }
            .map_err(|e| anyhow::anyhow!("Failed to parse Factory models response: {}", e))?;

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
                            .unwrap_or("factory")
                            .to_string();
                        let created = entry
                            .get("created")
                            .and_then(|v| v.as_i64())
                            .map(|ts| ts.to_string());

                        Some(ModelInfo {
                            id: id.clone(),
                            name: id,
                            description: Some(format!("Factory model owned by {}", owned_by)),
                            parameters: None,
                            available: true,
                            tags: vec!["factory".to_string(), owned_by],
                            downloads: None,
                            updated_at: created,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(models)
    }
}

#[async_trait]
impl LlmProvider for FactoryProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Factory
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let response = self
            .api_request(self.client.get(self.models_url()))
            .send()
            .await
            .context("Failed to query Factory models")?;

        let status = response.status();
        let response_text = response.text().await?;

        if !status.is_success() {
            warn!(
                "Factory model listing failed ({}), falling back to default",
                status
            );
            return Ok(vec![self.fallback_model_info()]);
        }

        let mut models = match Self::parse_models_response(&response_text) {
            Ok(models) => models,
            Err(err) => {
                warn!(
                    "Factory /v1/models did not return a usable model list ({}), falling back to default",
                    err
                );
                return Ok(vec![self.fallback_model_info()]);
            }
        };
        if models.is_empty() {
            models.push(self.fallback_model_info());
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
            .expect("factory request body should be an object");

        if !tools.is_empty() {
            body_object.insert("tools".into(), json!(tools));
            body_object.insert("tool_choice".into(), request.tool_choice.to_api_format());
            info!(
                "Factory request with {} tools, tool_choice={:?}",
                tools.len(),
                request.tool_choice
            );
        }

        if let Some(max_tokens) = request.max_tokens {
            body_object.insert("max_tokens".into(), json!(max_tokens));
        }
        if let Some(temp) = request.temperature {
            body_object.insert("temperature".into(), json!(temp));
        }
        if let Some(top_p) = request.top_p {
            body_object.insert("top_p".into(), json!(top_p));
        }

        debug!(
            "Factory request: {}",
            simd_json::to_string_pretty(&body).unwrap_or_default()
        );

        let response = self
            .api_request(self.client.post(&url))
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Factory")?;

        let status = response.status();
        let response_text = response.text().await?;

        debug!(
            "Factory response ({}): {}",
            status,
            &response_text[..response_text.len().min(500)]
        );

        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Factory API error ({}): {}",
                status,
                response_text
            ));
        }

        let mut response_text_mut = response_text;
        let response_json: Value =
            unsafe { simd_json::from_str(&mut response_text_mut) }.map_err(|e| {
                anyhow::anyhow!(
                    "Failed to parse Factory response: {}. Body: {}",
                    e,
                    response_text_mut
                )
            })?;

        let choice = response_json
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .ok_or_else(|| anyhow::anyhow!("No choices returned from Factory"))?;

        let message = choice
            .get("message")
            .ok_or_else(|| anyhow::anyhow!("No message in Factory response"))?;

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
            info!("Factory: parsed {} tool calls", calls.len());
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
            provider: "factory".to_string(),
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
    fn parses_model_listing_response() {
        let models = FactoryProvider::parse_models_response(
            r#"{"data":[{"id":"factory-default","owned_by":"factory","created":1710000000}]}"#,
        )
        .expect("model response should parse");

        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "factory-default");
        assert!(models[0].tags.iter().any(|tag| tag == "factory"));
    }

    #[test]
    fn defaults_from_constants() {
        let p = FactoryProvider::new(None, None, None);
        assert_eq!(p.provider_type(), ProviderType::Factory);
    }
}
