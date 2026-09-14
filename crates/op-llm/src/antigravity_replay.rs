//! Antigravity Replay Provider
//!
//! Uses captured Antigravity IDE session (OAuth token + headers) to make
//! API requests that appear to come from the IDE.
//!
//! This allows op-dbus to leverage Code Assist enterprise subscriptions.
//!
//! ## Setup
//!
//! 1. Run `antigravity-proxy-capture.sh` to capture IDE credentials
//! 2. Set `ANTIGRAVITY_SESSION_FILE` environment variable
//! 3. Use `LLM_PROVIDER=antigravity_replay`
//!
//! ## How It Works
//!
//! The Antigravity IDE sends specific headers that identify it as an IDE client:
//! - X-Goog-Api-Client: contains IDE version info
//! - User-Agent: identifies as Antigravity
//! - Other proprietary headers
//!
//! By capturing and replaying these headers along with the OAuth token,
//! our requests appear to come from the IDE and get Code Assist benefits.

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::provider::{
    ChatMessage, ChatRequest, ChatResponse, LlmProvider, ModelInfo,
    ProviderType, TokenUsage, ToolCallInfo, ToolChoice, ToolDefinition,
};

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

/// Captured session data from Antigravity IDE
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedSession {
    /// OAuth tokens captured from IDE
    pub tokens: Vec<CapturedToken>,
    /// Captured HTTP headers (including IDE identification)
    pub headers: HashMap<String, String>,
    /// Captured API endpoints
    pub endpoints: Vec<CapturedEndpoint>,
    /// Raw requests for debugging
    pub requests: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedToken {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub captured_at: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedEndpoint {
    pub url: String,
    pub method: String,
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub timestamp: Option<String>,
}

impl CapturedSession {
    /// Load from session file
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read session file: {}", path.display()))?;
        
        let session: Self = simd_json::from_str(&content)
            .with_context(|| "Failed to parse session JSON")?;
        
        if session.tokens.is_empty() {
            anyhow::bail!("No tokens found in session file");
        }
        
        Ok(session)
    }
    
    /// Get the latest access token
    pub fn latest_token(&self) -> Option<&str> {
        self.tokens.last().map(|t| t.access_token.as_str())
    }
    
    /// Build request headers that mimic the IDE
    pub fn build_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        
        // Add captured IDE headers
        for (key, value) in &self.headers {
            // Skip authorization (we'll add it separately)
            if key.to_lowercase() == "authorization" {
                continue;
            }
            headers.insert(key.clone(), value.clone());
        }
        
        // Add authorization
        if let Some(token) = self.latest_token() {
            headers.insert("Authorization".to_string(), format!("Bearer {}", token));
        }
        
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        
        headers
    }
}

/// Configuration for Antigravity Replay provider
#[derive(Debug, Clone)]
pub struct AntigravityReplayConfig {
    /// Path to captured session file
    pub session_file: PathBuf,
    /// Default model to use
    pub default_model: String,
    /// Whether to auto-route based on task
    pub auto_routing: bool,
}

impl AntigravityReplayConfig {
    pub fn from_env() -> Result<Self> {
        let session_file = std::env::var("ANTIGRAVITY_SESSION_FILE")
            .map(PathBuf::from)
            .or_else(|_| {
                let default = dirs::config_dir()
                    .map(|d| d.join("antigravity").join("captured").join("session.json"))
                    .ok_or_else(|| anyhow::anyhow!("Could not determine config directory"))?;
                
                if default.exists() {
                    Ok(default)
                } else {
                    Err(anyhow::anyhow!(
                        "No session file found. Run antigravity-proxy-capture.sh first."
                    ))
                }
            })?;
        
        Ok(Self {
            session_file,
            default_model: std::env::var("ANTIGRAVITY_MODEL")
                .unwrap_or_else(|_| "gemini-2.0-flash".to_string()),
            auto_routing: std::env::var("ANTIGRAVITY_AUTO_ROUTING")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(true),
        })
    }
}

/// Antigravity Replay Provider
///
/// Replays captured IDE credentials to access Gemini API with Code Assist benefits.
pub struct AntigravityReplayProvider {
    config: AntigravityReplayConfig,
    session: RwLock<CapturedSession>,
    client: Client,
}

impl AntigravityReplayProvider {
    /// Create from environment
    pub fn from_env() -> Result<Self> {
        let config = AntigravityReplayConfig::from_env()?;
        Self::new(config)
    }
    
    /// Create with config
    pub fn new(config: AntigravityReplayConfig) -> Result<Self> {
        let session = CapturedSession::load(&config.session_file)?;
        
        info!("Antigravity Replay provider initialized");
        info!("  Session file: {}", config.session_file.display());
        info!("  Captured headers: {}", session.headers.len());
        info!("  Captured tokens: {}", session.tokens.len());
        
        // Log important headers (sanitized)
        for (key, _) in &session.headers {
            debug!("  Header captured: {}", key);
        }
        
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .build()?;
        
        Ok(Self {
            config,
            session: RwLock::new(session),
            client,
        })
    }
    
    /// Reload session from file (if token expired)
    pub fn reload_session(&self) -> Result<()> {
        let session = CapturedSession::load(&self.config.session_file)?;
        *self.session.write().unwrap() = session;
        info!("Session reloaded");
        Ok(())
    }
    
    /// Build HTTP request with captured headers
    fn build_request(&self, url: &str) -> reqwest::RequestBuilder {
        let session = self.session.read().unwrap();
        let headers = session.build_headers();
        
        let mut request = self.client.post(url);
        
        for (key, value) in headers {
            request = request.header(&key, &value);
        }
        
        request
    }
    
    /// Auto-select model based on task
    fn select_model(&self, messages: &[ChatMessage], has_tools: bool) -> String {
        if !self.config.auto_routing {
            return self.config.default_model.clone();
        }
        
        let total_length: usize = messages.iter().map(|m| m.content.len()).sum();
        let needs_reasoning = messages.iter().any(|m| {
            let lower = m.content.to_lowercase();
            lower.contains("think") ||
            lower.contains("reason") ||
            lower.contains("step by step")
        });
        
        if has_tools {
            "gemini-2.0-flash".to_string()
        } else if needs_reasoning {
            "gemini-2.0-flash-thinking-exp-01-21".to_string()
        } else if total_length > 100000 {
            "gemini-1.5-pro".to_string()
        } else {
            "gemini-2.0-flash".to_string()
        }
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
                "assistant" => "model",
                _ => "user",
            };
            
            contents.push(json!({
                "role": role,
                "parts": [{"text": msg.content}]
            }));
        }
        
        (contents, system_instruction)
    }
}

#[async_trait]
impl LlmProvider for AntigravityReplayProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Antigravity
    }
    
    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        Ok(vec![
            ModelInfo {
                id: "gemini-2.0-flash".to_string(),
                name: "Gemini 2.0 Flash (via IDE)".to_string(),
                description: Some("Fast model via captured IDE session".to_string()),
                parameters: None,
                available: true,
                tags: vec!["ide".to_string(), "replay".to_string()],
                downloads: None,
                updated_at: None,
            },
            ModelInfo {
                id: "gemini-2.0-flash-thinking-exp-01-21".to_string(),
                name: "Gemini Flash Thinking (via IDE)".to_string(),
                description: Some("Reasoning model via captured IDE session".to_string()),
                parameters: None,
                available: true,
                tags: vec!["ide".to_string(), "reasoning".to_string()],
                downloads: None,
                updated_at: None,
            },
            ModelInfo {
                id: "gemini-1.5-pro".to_string(),
                name: "Gemini 1.5 Pro (via IDE)".to_string(),
                description: Some("High quality model via captured IDE session".to_string()),
                parameters: None,
                available: true,
                tags: vec!["ide".to_string(), "quality".to_string()],
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
            .filter(|m| m.id.to_lowercase().contains(&query_lower))
            .take(limit)
            .collect())
    }
    
    async fn get_model(&self, model_id: &str) -> Result<Option<ModelInfo>> {
        let models = self.list_models().await?;
        Ok(models.into_iter().find(|m| m.id == model_id))
    }
    
    async fn is_model_available(&self, _model_id: &str) -> Result<bool> {
        // Check if we have a valid session
        let session = self.session.read().unwrap();
        Ok(session.latest_token().is_some())
    }
    
    async fn chat(&self, model: &str, messages: Vec<ChatMessage>) -> Result<ChatResponse> {
        let request = ChatRequest::new(messages);
        self.chat_with_request(model, request).await
    }
    
    async fn chat_with_request(&self, model: &str, request: ChatRequest) -> Result<ChatResponse> {
        let actual_model = if model == "auto" || model.is_empty() {
            self.select_model(&request.messages, !request.tools.is_empty())
        } else {
            model.to_string()
        };
        
        let url = format!(
            "{}/models/{}:generateContent",
            GEMINI_API_BASE,
            actual_model
        );
        
        let (contents, system_instruction) = self.convert_messages(&request.messages);
        
        let mut body = json!({
            "contents": contents,
        });
        
        if let Some(sys) = system_instruction {
            body["systemInstruction"] = sys;
        }
        
        if let Some(temp) = request.temperature {
            body["generationConfig"] = json!({"temperature": temp});
        }
        
        debug!("Antigravity Replay request to: {}", url);
        
        let response = self
            .build_request(&url)
            .json(&body)
            .send()
            .await
            .context("Failed to send request")?;
        
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            
            // Check if token expired
            if status.as_u16() == 401 {
                warn!("Token may have expired. Try rerunning antigravity-proxy-capture.sh");
            }
            
            return Err(anyhow::anyhow!("API error {}: {}", status, body));
        }
        
        let result: Value = response.json().await
            .context("Failed to parse response")?;
        
        // Parse Gemini response
        let candidates = result.get("candidates")
            .and_then(|c| c.as_array())
            .ok_or_else(|| anyhow::anyhow!("No candidates in response"))?;
        
        let first_candidate = candidates.first()
            .ok_or_else(|| anyhow::anyhow!("Empty candidates"))?;
        
        let mut text_parts = Vec::new();
        if let Some(parts) = first_candidate
            .get("content")
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
        {
            for part in parts {
                if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                    text_parts.push(text.to_string());
                }
            }
        }
        
        let usage = result.get("usageMetadata").map(|u| TokenUsage {
            prompt_tokens: u.get("promptTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            completion_tokens: u.get("candidatesTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
            total_tokens: u.get("totalTokenCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32,
        });
        
        Ok(ChatResponse {
            message: ChatMessage {
                role: "assistant".to_string(),
                content: text_parts.join(""),
                tool_calls: None,
                tool_call_id: None,
            },
            model: actual_model,
            provider: "antigravity_replay".to_string(),
            finish_reason: first_candidate.get("finishReason")
                .and_then(|f| f.as_str())
                .map(String::from),
            usage,
            tool_calls: None,
        })
    }
    
    async fn chat_stream(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<String>>> {
        // Fall back to non-streaming
        let response = self.chat(model, messages).await?;
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let _ = tx.send(Ok(response.message.content)).await;
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_session_headers() {
        let session = CapturedSession {
            tokens: vec![CapturedToken {
                access_token: "test-token".to_string(),
                refresh_token: None,
                captured_at: None,
                endpoint: None,
                headers: HashMap::new(),
            }],
            headers: {
                let mut h = HashMap::new();
                h.insert("X-Goog-Api-Client".to_string(), "test-client".to_string());
                h.insert("User-Agent".to_string(), "Antigravity/1.0".to_string());
                h
            },
            endpoints: vec![],
            requests: vec![],
        };
        
        let headers = session.build_headers();
        
        assert!(headers.contains_key("Authorization"));
        assert!(headers.get("Authorization").unwrap().contains("test-token"));
        assert!(headers.contains_key("X-Goog-Api-Client"));
        assert!(headers.contains_key("User-Agent"));
    }
}
