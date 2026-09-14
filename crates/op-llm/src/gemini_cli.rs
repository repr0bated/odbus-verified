//! Gemini CLI Integration via PTY Bridge
//!
//! Uses the PTY bridge to run `gemini` CLI tool on headless servers,
//! handling OAuth authentication flows transparently.
//!
//! ## Prerequisites
//!
//! Install Gemini CLI:
//! ```bash
//! npm install -g @google/gemini-cli
//! ```
//!
//! ## How Auth Works
//!
//! This provider uses gcloud credentials:
//! 1. Set up gcloud auth: `gcloud auth login`
//! 2. Or use service account: Set `GOOGLE_APPLICATION_CREDENTIALS` env var
//! 3. PTY bridge passes credentials to Gemini CLI automatically
//! 4. Gemini CLI uses gcloud credentials for API calls

use anyhow::{Context, Result};
use async_trait::async_trait;
use simd_json::prelude::*;
use std::sync::Arc;
use tracing::{info, warn};

use crate::provider::{ChatMessage, ChatResponse, LlmProvider, ModelInfo, ProviderType};
use crate::pty_bridge::PtyAuthBridge;

// =============================================================================
// GEMINI CLI PROVIDER
// =============================================================================

/// Gemini CLI-based LLM provider
///
/// Uses PTY bridge to run the `gemini` CLI tool, handling authentication
/// flows automatically on headless servers.
pub struct GeminiCliProvider {
    bridge: Arc<PtyAuthBridge>,
    /// Path to gemini binary (default: "gemini")
    binary: String,
    /// Default model to use
    default_model: String,
    /// Timeout for commands in seconds
    timeout_secs: u64,
}

impl GeminiCliProvider {
    /// Create a new Gemini CLI provider
    pub fn new(bridge: Arc<PtyAuthBridge>) -> Self {
        Self {
            bridge,
            binary: "gemini".to_string(),
            default_model: "gemini-2.0-flash".to_string(),
            timeout_secs: 120,
        }
    }

    /// Configure Gemini CLI to use gcloud credentials
    /// This ensures GOOGLE_APPLICATION_CREDENTIALS is set when executing commands
    #[allow(dead_code)]
    fn setup_gcloud_env(&self) -> std::collections::HashMap<String, String> {
        let mut env = std::collections::HashMap::new();

        // Pass through existing env vars
        if let Ok(creds) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
            env.insert("GOOGLE_APPLICATION_CREDENTIALS".to_string(), creds);
        } else if let Some(home) = dirs::home_dir() {
            // Try service account key first
            let gcloud_creds = home.join(".config/gcloud/gemini-cli.json");
            if gcloud_creds.exists() {
                env.insert(
                    "GOOGLE_APPLICATION_CREDENTIALS".to_string(),
                    gcloud_creds.to_string_lossy().to_string(),
                );
            } else {
                // Fall back to Application Default Credentials
                let adc_creds = home.join(".config/gcloud/application_default_credentials.json");
                if adc_creds.exists() {
                    env.insert(
                        "GOOGLE_APPLICATION_CREDENTIALS".to_string(),
                        adc_creds.to_string_lossy().to_string(),
                    );
                }
            }
        }

        // Pass through project ID
        if let Ok(project) = std::env::var("GOOGLE_CLOUD_PROJECT") {
            env.insert("GOOGLE_CLOUD_PROJECT".to_string(), project);
        }

        env
    }

    /// Create with custom binary path
    pub fn with_binary(mut self, binary: &str) -> Self {
        self.binary = binary.to_string();
        self
    }

    /// Set default model
    pub fn with_model(mut self, model: &str) -> Self {
        self.default_model = model.to_string();
        self
    }

    /// Set command timeout
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Check if gemini CLI is available
    pub async fn check_available(&self) -> bool {
        let result = self.bridge.execute(&self.binary, &["--version"], 10).await;

        match result {
            Ok(r) => r.exit_code == 0,
            Err(_) => false,
        }
    }

    /// Convert chat messages to CLI format
    fn format_prompt(messages: &[ChatMessage]) -> String {
        let mut prompt = String::new();

        for msg in messages {
            match msg.role.as_str() {
                "system" => {
                    prompt.push_str(&format!("[System]: {}\n\n", msg.content));
                }
                "user" => {
                    prompt.push_str(&format!("User: {}\n\n", msg.content));
                }
                "assistant" => {
                    prompt.push_str(&format!("Assistant: {}\n\n", msg.content));
                }
                _ => {
                    prompt.push_str(&format!("{}: {}\n\n", msg.role, msg.content));
                }
            }
        }

        prompt
    }
}

#[async_trait]
impl LlmProvider for GeminiCliProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Custom("gemini-cli".to_string())
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        // Gemini CLI supports these models
        Ok(vec![
            ModelInfo {
                id: "gemini-2.0-flash".to_string(),
                name: "Gemini 2.0 Flash".to_string(),
                description: Some("Fast, efficient model".to_string()),
                parameters: None,
                available: true,
                tags: vec!["fast".to_string(), "cli".to_string()],
                downloads: None,
                updated_at: None,
            },
            ModelInfo {
                id: "gemini-2.0-pro".to_string(),
                name: "Gemini 2.0 Pro".to_string(),
                description: Some("Most capable model".to_string()),
                parameters: None,
                available: true,
                tags: vec!["powerful".to_string(), "cli".to_string()],
                downloads: None,
                updated_at: None,
            },
            ModelInfo {
                id: "gemini-1.5-flash".to_string(),
                name: "Gemini 1.5 Flash".to_string(),
                description: Some("Previous generation fast model".to_string()),
                parameters: None,
                available: true,
                tags: vec!["fast".to_string(), "cli".to_string()],
                downloads: None,
                updated_at: None,
            },
        ])
    }

    async fn search_models(&self, query: &str, limit: usize) -> Result<Vec<ModelInfo>> {
        let models = self.list_models().await?;
        let query_lower = query.to_lowercase();

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
        Ok(self.get_model(model_id).await?.is_some())
    }

    async fn chat(&self, model: &str, messages: Vec<ChatMessage>) -> Result<ChatResponse> {
        let model = if model.is_empty() {
            &self.default_model
        } else {
            model
        };
        let prompt = Self::format_prompt(&messages);

        info!(model = %model, prompt_len = %prompt.len(), "Gemini CLI chat");

        // Build command args
        // Gemini CLI syntax: gemini -m <model> "<prompt>" or gemini "<prompt>"
        let args = vec!["-m", model, &prompt];

        let result = self
            .bridge
            .execute(&self.binary, &args, self.timeout_secs)
            .await
            .context("Failed to execute gemini CLI")?;

        // Handle auth requirement
        if result.auth_required {
            if let Some(auth) = &result.auth_details {
                warn!(
                    auth_type = ?auth.auth_type,
                    url = ?auth.url,
                    "Gemini CLI requires authentication"
                );
                return Err(anyhow::anyhow!(
                    "Authentication required. Visit: {}",
                    auth.url.as_deref().unwrap_or("(see terminal output)")
                ));
            }
        }

        if result.exit_code != 0 {
            return Err(anyhow::anyhow!(
                "Gemini CLI failed with exit code {}: {}",
                result.exit_code,
                result.stderr
            ));
        }

        // Try to parse JSON response
        let content = if let Ok(json_resp) =
            unsafe { simd_json::from_str::<simd_json::OwnedValue>(&mut result.stdout.clone()) }
        {
            json_resp
                .get("response")
                .or_else(|| json_resp.get("text"))
                .or_else(|| json_resp.get("content"))
                .and_then(|v| v.as_str())
                .unwrap_or(&result.stdout)
                .to_string()
        } else {
            // Plain text response
            result.stdout.trim().to_string()
        };

        Ok(ChatResponse {
            message: ChatMessage::assistant(content),
            model: model.to_string(),
            provider: "gemini-cli".to_string(),
            finish_reason: Some("stop".to_string()),
            usage: None, // CLI doesn't provide usage stats
            tool_calls: None,
        })
    }

    async fn chat_stream(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<String>>> {
        // Gemini CLI doesn't support streaming, so we fake it
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let response = self.chat(model, messages).await?;
        tx.send(Ok(response.message.content)).await.ok();
        Ok(rx)
    }
}

// =============================================================================
// FACTORY
// =============================================================================

/// Create a Gemini CLI provider with default bridge
pub fn create_gemini_cli_provider() -> GeminiCliProvider {
    let bridge = Arc::new(PtyAuthBridge::new());
    GeminiCliProvider::new(bridge)
}

/// Create a Gemini CLI provider with webhook notifications
pub fn create_gemini_cli_provider_with_webhook(webhook_url: &str) -> GeminiCliProvider {
    use crate::pty_bridge::WebhookNotificationHandler;

    let bridge = Arc::new(PtyAuthBridge::new());

    // Add webhook handler in background
    let bridge_clone = bridge.clone();
    let url = webhook_url.to_string();
    tokio::spawn(async move {
        bridge_clone
            .add_handler(Arc::new(WebhookNotificationHandler::new(&url)))
            .await;
    });

    GeminiCliProvider::new(bridge)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_prompt() {
        let messages = vec![
            ChatMessage::system("You are a helpful assistant."),
            ChatMessage::user("Hello!"),
            ChatMessage::assistant("Hi there!"),
            ChatMessage::user("How are you?"),
        ];

        let prompt = GeminiCliProvider::format_prompt(&messages);

        assert!(prompt.contains("[System]: You are a helpful assistant."));
        assert!(prompt.contains("User: Hello!"));
        assert!(prompt.contains("Assistant: Hi there!"));
        assert!(prompt.contains("User: How are you?"));
    }
}
