//! Perplexity API Client
//!
//! ## API Endpoints
//!
//! | Endpoint | URL | Purpose |
//! |----------|-----|--------|
//! | Base URL | `https://api.perplexity.ai` | All Perplexity APIs |
//! | Chat | `/chat/completions` | OpenAI-compatible chat |
//!
//! ## Authentication
//! - Header: `Authorization: Bearer {PERPLEXITY_API_KEY}`
//! - Environment: `PERPLEXITY_API_KEY`
//!
//! ## Features
//! - Online search capability (real-time web data)
//! - Citations in responses
//!
//! ## Pricing
//! - ~$5 per 1000 requests

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info};

use crate::provider::{
    ChatMessage, ChatResponse, LlmProvider, ModelInfo, ProviderType, TokenUsage,
};

// =============================================================================
// API ENDPOINT CONFIGURATION
// =============================================================================

/// Perplexity API endpoints
pub mod endpoints {
    /// Base API URL
    pub const BASE_URL: &str = "https://api.perplexity.ai";

    /// Chat completions endpoint (OpenAI-compatible)
    /// Full URL: {BASE_URL}/chat/completions
    pub const CHAT_COMPLETIONS: &str = "/chat/completions";
}

// =============================================================================
// DATA STRUCTURES
// =============================================================================

/// Available Perplexity models
const PERPLEXITY_MODELS: &[(&str, &str, &str)] = &[
    ("sonar", "Sonar", "Default online model with search"),
    ("sonar-pro", "Sonar Pro", "Advanced online model"),
    (
        "sonar-reasoning",
        "Sonar Reasoning",
        "Enhanced reasoning with search",
    ),
    (
        "llama-3.1-sonar-small-128k-online",
        "Sonar Small Online",
        "Fast online model",
    ),
    (
        "llama-3.1-sonar-large-128k-online",
        "Sonar Large Online",
        "Capable online model",
    ),
    (
        "llama-3.1-sonar-huge-128k-online",
        "Sonar Huge Online",
        "Most capable online",
    ),
];

#[derive(Debug, Serialize)]
struct PerplexityRequest {
    model: String,
    messages: Vec<PerplexityMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PerplexityMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct PerplexityResponse {
    choices: Vec<PerplexityChoice>,
    model: Option<String>,
    usage: Option<PerplexityUsage>,
}

#[derive(Debug, Deserialize)]
struct PerplexityChoice {
    message: PerplexityMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PerplexityUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

// =============================================================================
// CLIENT IMPLEMENTATION
// =============================================================================

/// Perplexity API Client
pub struct PerplexityClient {
    client: Client,
    api_key: String,
    /// Base API URL
    api_url: String,
}

impl PerplexityClient {
    /// Create a new Perplexity client
    ///
    /// Uses default endpoint: https://api.perplexity.ai
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            api_key: api_key.into(),
            api_url: endpoints::BASE_URL.to_string(),
        }
    }

    /// Create from environment variable
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("PERPLEXITY_API_KEY")
            .context("PERPLEXITY_API_KEY environment variable not set")?;
        Ok(Self::new(api_key))
    }

    /// Create with custom endpoint
    pub fn with_endpoint(api_key: impl Into<String>, endpoint: impl Into<String>) -> Self {
        let mut client = Self::new(api_key);
        client.api_url = endpoint.into();
        client
    }

    /// Get the current API URL
    pub fn api_url(&self) -> &str {
        &self.api_url
    }
}

#[async_trait]
impl LlmProvider for PerplexityClient {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Perplexity
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        info!("Perplexity models (static list)");
        info!("  Endpoint: {}", self.api_url);

        Ok(PERPLEXITY_MODELS
            .iter()
            .map(|(id, name, desc)| ModelInfo {
                id: id.to_string(),
                name: name.to_string(),
                description: Some(desc.to_string()),
                parameters: None,
                available: true,
                tags: vec!["online".to_string(), "search".to_string()],
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
        Ok(PERPLEXITY_MODELS.iter().any(|(id, _, _)| *id == model_id))
    }

    async fn chat(&self, model: &str, messages: Vec<ChatMessage>) -> Result<ChatResponse> {
        // Build URL: {api_url}/chat/completions
        let url = format!("{}/chat/completions", self.api_url);

        info!(
            "Perplexity chat: model={}, endpoint={}",
            model, self.api_url
        );

        let perplexity_messages: Vec<PerplexityMessage> = messages
            .iter()
            .map(|m| PerplexityMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        let request = PerplexityRequest {
            model: model.to_string(),
            messages: perplexity_messages,
            max_tokens: Some(2048),
            temperature: Some(0.7),
        };

        debug!("Perplexity request to: {}", url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .context("Failed to send Perplexity request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("Perplexity API error {}: {}", status, body));
        }

        let result: PerplexityResponse = response
            .json()
            .await
            .context("Failed to parse Perplexity response")?;

        let choice = result
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No response from Perplexity"))?;

        let usage = result.usage.map(|u| TokenUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });

        Ok(ChatResponse {
            message: ChatMessage::assistant(choice.message.content),
            model: result.model.unwrap_or_else(|| model.to_string()),
            provider: "perplexity".to_string(),
            finish_reason: choice.finish_reason,
            usage,
            tool_calls: None,
        })
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
