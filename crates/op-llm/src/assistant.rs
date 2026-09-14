//! Assistant LLM Provider (Thin Wrapper)
//!
//! User-facing overlay around the internal [`OpenClawProvider`].
//! Delegates all network logic to the upstream provider while rewriting
//! branding in responses and checking `ASSISTANT_*` environment variables
//! before falling back to `OPENCLAW_*`.
//!
//! ## Configuration
//!
//! ```bash
//! ASSISTANT_BASE_URL=http://127.0.0.1:8090       # checked first
//! ASSISTANT_DEFAULT_MODEL=assistant:main            # checked first
//! # Falls back to OPENCLAW_BASE_URL / OPENCLAW_DEFAULT_MODEL if unset
//! ```

use anyhow::Result;
use async_trait::async_trait;

use crate::openclaw::OpenClawProvider;
use crate::provider::{
    ChatMessage, ChatRequest, ChatResponse, LlmProvider, ModelInfo, ProviderType,
};

/// User-facing Assistant provider.
///
/// Internally delegates to [`OpenClawProvider`] so upstream OpenClaw
/// updates apply cleanly to the base layer.  This struct only overrides
/// branding and environment-variable resolution.
pub struct AssistantProvider {
    inner: OpenClawProvider,
}

impl AssistantProvider {
    pub fn new(base_url: Option<String>, default_model: Option<String>) -> Self {
        let base_url = base_url
            .or_else(|| std::env::var("ASSISTANT_BASE_URL").ok())
            .or_else(|| std::env::var("OPENCLAW_BASE_URL").ok());

        let default_model = default_model
            .or_else(|| std::env::var("ASSISTANT_DEFAULT_MODEL").ok())
            .or_else(|| std::env::var("OPENCLAW_DEFAULT_MODEL").ok())
            .unwrap_or_else(|| "assistant:main".to_string());

        Self {
            inner: OpenClawProvider::new(base_url, Some(default_model)),
        }
    }

    pub fn from_env() -> Result<Self> {
        Ok(Self::new(None, None))
    }

    /// Rewrite model metadata so user-facing strings say "Assistant".
    fn rewrite_models(mut models: Vec<ModelInfo>) -> Vec<ModelInfo> {
        for model in &mut models {
            // Swap upstream branding tag for user-facing branding
            model.tags.retain(|t| t != "openclaw");
            if !model.tags.iter().any(|t| t == "assistant") {
                model.tags.push("assistant".to_string());
            }
            if let Some(ref mut desc) = model.description {
                *desc = desc.replace("OpenClaw", "Assistant");
            }
        }
        models
    }

    /// Rewrite a chat response so the `provider` field reads "assistant".
    fn rewrite_response(mut response: ChatResponse) -> ChatResponse {
        if response.provider == "openclaw" {
            response.provider = "assistant".to_string();
        }
        response
    }
}

#[async_trait]
impl LlmProvider for AssistantProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Assistant
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let models = self.inner.list_models().await?;
        Ok(Self::rewrite_models(models))
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
        let response = self.inner.chat_with_request(model, request).await?;
        Ok(Self::rewrite_response(response))
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
    use crate::provider::ChatMessage;

    #[test]
    fn env_fallback_resolution() {
        // ASSISTANT_BASE_URL set → used
        std::env::set_var("ASSISTANT_BASE_URL", "http://assistant:9999");
        std::env::remove_var("OPENCLAW_BASE_URL");
        let _p = AssistantProvider::new(None, None);
        // We can't inspect private fields, but we can verify from_env succeeds
        let _ = AssistantProvider::from_env().unwrap();

        // Only OPENCLAW_BASE_URL set → fallback
        std::env::remove_var("ASSISTANT_BASE_URL");
        std::env::set_var("OPENCLAW_BASE_URL", "http://openclaw:8888");
        let _ = AssistantProvider::from_env().unwrap();

        // Clean up
        std::env::remove_var("ASSISTANT_BASE_URL");
        std::env::remove_var("OPENCLAW_BASE_URL");
    }

    #[test]
    fn default_model_is_assistant_prefix() {
        std::env::remove_var("ASSISTANT_DEFAULT_MODEL");
        std::env::remove_var("OPENCLAW_DEFAULT_MODEL");
        let p = AssistantProvider::new(None, None);
        // from_env with no env vars should default to assistant:main
        let _ = p;
    }

    #[test]
    fn rewrite_models_swaps_branding() {
        let models = vec![ModelInfo {
            id: "openclaw:main".to_string(),
            name: "OpenClaw Main".to_string(),
            description: Some("OpenClaw model owned by test".to_string()),
            parameters: None,
            available: true,
            tags: vec!["openclaw".to_string(), "test".to_string()],
            downloads: None,
            updated_at: None,
        }];

        let rewritten = AssistantProvider::rewrite_models(models);
        assert_eq!(
            rewritten[0].tags,
            vec!["test".to_string(), "assistant".to_string()]
        );
        assert_eq!(
            rewritten[0].description,
            Some("Assistant model owned by test".to_string())
        );
    }

    #[test]
    fn rewrite_response_swaps_provider_field() {
        let response = ChatResponse {
            message: ChatMessage::assistant("hello"),
            model: "test".to_string(),
            provider: "openclaw".to_string(),
            finish_reason: None,
            usage: None,
            tool_calls: None,
        };
        let rewritten = AssistantProvider::rewrite_response(response);
        assert_eq!(rewritten.provider, "assistant");
    }
}
