//! Google Cloud ADC Provider - Uses gcloud application-default credentials
//!
//! This provider replaces the old Antigravity provider and uses the
//! Cloud AI Companion (Subscription) endpoint.

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::process::Command;
use std::time::Duration;
use uuid;

use crate::provider::{
    ChatMessage, ChatRequest, ChatResponse, LlmProvider, ModelInfo, ProviderType, TokenUsage,
    ToolCallInfo, ToolChoice, ToolDefinition,
};

/// Cloud AI Companion base URL - configurable via GCP_BASE_URL
fn cloud_ai_base() -> String {
    std::env::var("GCP_BASE_URL")
        .unwrap_or_else(|_| "https://cloudaicompanion.googleapis.com/v1".to_string())
}

fn project_id() -> String {
    std::env::var("GCP_PROJECT")
        .or_else(|_| std::env::var("GOOGLE_CLOUD_PROJECT"))
        .unwrap_or_else(|_| "geminidev-479406".to_string())
}

fn location() -> String {
    std::env::var("GCP_LOCATION").unwrap_or_else(|_| "global".to_string())
}

fn adc_fallback_enabled() -> bool {
    std::env::var("OP_ENABLE_ADC_FALLBACK")
        .ok()
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

pub struct GCloudADCProvider {
    client: Client,
    model: String,
}

impl GCloudADCProvider {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .unwrap_or_default();

        let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "gemini-2.5-flash".to_string());

        Self { client, model }
    }

    /// Get OAuth token from gcloud
    async fn get_token(&self) -> Result<String> {
        if let Ok(token) = std::env::var("GCLOUD_TOKEN") {
            return Ok(token);
        }

        // Prefer active gcloud user credentials.
        let output = Command::new("gcloud")
            .args(["auth", "print-access-token"])
            .output()
            .context("Failed to execute gcloud auth print-access-token")?;

        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
        }

        // Optional ADC fallback (disabled by default to avoid metadata-server auth on Compute hosts).
        if adc_fallback_enabled() {
            let output = Command::new("gcloud")
                .args(["auth", "application-default", "print-access-token"])
                .output()
                .context("Failed to execute gcloud auth application-default print-access-token")?;

            if output.status.success() {
                return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
            }
        }

        anyhow::bail!("Could not obtain gcloud token from GCLOUD_TOKEN or gcloud CLI credentials")
    }

    /// Convert messages to Gemini format
    fn convert_messages(&self, messages: &[ChatMessage]) -> (Vec<Value>, Option<Value>) {
        let mut system_instruction = None;
        let mut contents = Vec::new();

        for msg in messages {
            if msg.role == "system" {
                system_instruction = Some(json!({
                    "parts": [{"text": msg.content}]
                }));
                continue;
            }

            let role = match msg.role.as_str() {
                "assistant" | "model" => "model",
                _ => "user",
            };

            contents.push(json!({
                "role": role,
                "parts": [{"text": msg.content}]
            }));
        }

        (contents, system_instruction)
    }

    /// Convert tools to Gemini format
    fn convert_tools(&self, tools: &[ToolDefinition]) -> Value {
        let function_declarations: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.input_schema
                })
            })
            .collect();

        json!([{
            "functionDeclarations": function_declarations
        }])
    }

    /// Convert tool choice to Gemini format
    fn convert_tool_choice(&self, choice: &ToolChoice) -> Option<Value> {
        match choice {
            ToolChoice::Auto => Some(json!({"mode": "AUTO"})),
            ToolChoice::Required => Some(json!({"mode": "ANY"})),
            ToolChoice::None => Some(json!({"mode": "NONE"})),
            ToolChoice::Tool(name) => Some(json!({
                "mode": "ANY",
                "allowedFunctionNames": [name]
            })),
        }
    }
}

#[async_trait]
impl LlmProvider for GCloudADCProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Antigravity // Reusing this for now to minimize changes in ChatManager
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        Ok(vec![
            ModelInfo {
                id: "gemini-2.5-pro".to_string(),
                name: "Gemini 2.5 Pro".to_string(),
                description: Some("Latest stable Gemini Pro model".to_string()),
                parameters: None,
                available: true,
                tags: vec!["google".to_string()],
                downloads: None,
                updated_at: None,
            },
            ModelInfo {
                id: "gemini-2.5-flash".to_string(),
                name: "Gemini 2.5 Flash".to_string(),
                description: Some("Latest stable Gemini Flash model".to_string()),
                parameters: None,
                available: true,
                tags: vec!["google".to_string()],
                downloads: None,
                updated_at: None,
            },
            ModelInfo {
                id: "gemini-2.5-flash-lite".to_string(),
                name: "Gemini 2.5 Flash-Lite".to_string(),
                description: Some("Latest stable cost-efficient Gemini model".to_string()),
                parameters: None,
                available: true,
                tags: vec!["google".to_string()],
                downloads: None,
                updated_at: None,
            },
            ModelInfo {
                id: "gemini-2.5-flash-image".to_string(),
                name: "Gemini 2.5 Flash Image".to_string(),
                description: Some("Gemini image generation and editing model".to_string()),
                parameters: None,
                available: true,
                tags: vec!["google".to_string(), "image".to_string()],
                downloads: None,
                updated_at: None,
            },
            ModelInfo {
                id: "gemini-2.0-flash".to_string(),
                name: "Gemini 2.0 Flash".to_string(),
                description: Some("Gemini 2.0 Flash alias".to_string()),
                parameters: None,
                available: true,
                tags: vec!["google".to_string()],
                downloads: None,
                updated_at: None,
            },
            ModelInfo {
                id: "gemini-2.0-flash-lite".to_string(),
                name: "Gemini 2.0 Flash-Lite".to_string(),
                description: Some("Gemini 2.0 Flash-Lite alias".to_string()),
                parameters: None,
                available: true,
                tags: vec!["google".to_string()],
                downloads: None,
                updated_at: None,
            },
        ])
    }

    async fn search_models(&self, query: &str, limit: usize) -> Result<Vec<ModelInfo>> {
        let models = self.list_models().await?;
        let query = query.to_lowercase();
        Ok(models
            .into_iter()
            .filter(|m| m.id.contains(&query) || m.name.to_lowercase().contains(&query))
            .take(limit)
            .collect())
    }

    async fn get_model(&self, model_id: &str) -> Result<Option<ModelInfo>> {
        let models = self.list_models().await?;
        Ok(models.into_iter().find(|m| m.id == model_id))
    }

    async fn is_model_available(&self, model_id: &str) -> Result<bool> {
        Ok(self
            .list_models()
            .await?
            .iter()
            .any(|model| model.id == model_id))
    }

    async fn chat(&self, model: &str, messages: Vec<ChatMessage>) -> Result<ChatResponse> {
        let request = ChatRequest::new(messages);
        self.chat_with_request(model, request).await
    }

    async fn chat_with_request(&self, model: &str, request: ChatRequest) -> Result<ChatResponse> {
        let model = if model.is_empty() { &self.model } else { model };
        let token = self.get_token().await?;

        let url = format!(
            "{}/projects/{}/locations/{}/publishers/google/models/{}:generateContent",
            cloud_ai_base(),
            project_id(),
            location(),
            model
        );

        let (contents, system_instruction) = self.convert_messages(&request.messages);

        // Build body with all optional fields included from the start
        let mut body_map = HashMap::new();
        body_map.insert("contents".to_string(), json!(contents));
        body_map.insert(
            "generationConfig".to_string(),
            json!({
                "temperature": request.temperature.unwrap_or(0.7) as f64,
                "maxOutputTokens": request.max_tokens.unwrap_or(8192) as u64,
            }),
        );

        if let Some(sys) = system_instruction {
            body_map.insert("systemInstruction".to_string(), sys);
        }

        // Add tools if present
        if !request.tools.is_empty() {
            let tools = self.convert_tools(&request.tools);
            body_map.insert("tools".to_string(), tools);

            if let Some(tool_config) = self.convert_tool_choice(&request.tool_choice) {
                body_map.insert(
                    "toolConfig".to_string(),
                    json!({"functionCallingConfig": tool_config}),
                );
            }
        }

        let body = Value::from(body_map);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("Cloud AI error {}: {}", status, text);
        }

        let result: Value = response.json().await?;

        // Parse candidates
        let candidates = result
            .get("candidates")
            .and_then(|c| c.as_array())
            .ok_or_else(|| anyhow::anyhow!("No candidates in response"))?;

        let first_candidate = candidates
            .first()
            .ok_or_else(|| anyhow::anyhow!("Empty candidates"))?;

        // Extract text and tool calls
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        if let Some(parts) = first_candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
        {
            for part in parts {
                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    text_parts.push(text.to_string());
                }
                if let Some(fc) = part.get("functionCall") {
                    let name = fc.get("name").and_then(|n| n.as_str()).unwrap_or_default();
                    let args = fc.get("args").cloned().unwrap_or(json!({}));
                    tool_calls.push(ToolCallInfo {
                        id: format!("call_{}", uuid::Uuid::new_v4()),
                        name: name.to_string(),
                        arguments: args,
                    });
                }
            }
        }

        let usage = result.get("usageMetadata").map(|u| TokenUsage {
            prompt_tokens: u
                .get("promptTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: u
                .get("candidatesTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: u
                .get("totalTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        });

        Ok(ChatResponse {
            message: ChatMessage {
                role: "assistant".to_string(),
                content: if text_parts.is_empty() && !tool_calls.is_empty() {
                    "[Executing tools...]".to_string()
                } else if text_parts.is_empty() {
                    "Task completed.".to_string()
                } else {
                    text_parts.join("")
                },
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls.clone())
                },
                tool_call_id: None,
            },
            model: model.to_string(),
            provider: "gcloud-adc".to_string(),
            finish_reason: first_candidate
                .get("finishReason")
                .and_then(|f| f.as_str())
                .map(String::from),
            usage,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
        })
    }

    async fn chat_stream(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<String>>> {
        let response = self.chat(model, messages).await?;
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let _ = tx.send(Ok(response.message.content)).await;
        Ok(rx)
    }
}

impl Default for GCloudADCProvider {
    fn default() -> Self {
        Self::new()
    }
}
