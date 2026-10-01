//! Chat Manager - Manages provider switching and chat sessions
//!
//! ## Authentication Priority
//!
//! 1. **Factory** (explicit FACTORY_BASE_URL)
//! 2. **Antigravity** (native Google ADC refresh or configured bridge)
//! 3. **Gemini** (API key fallback)
//! 4. **Anthropic** (API key)
//!
//! ## Environment Variables
//!
//! ```bash
//! # Optional: Factory endpoint
//! LLM_PROVIDER=factory
//! FACTORY_BASE_URL=http://127.0.0.1:<factory-port>/v1
//! FACTORY_API_KEY=<token>
//! FACTORY_DEFAULT_MODEL=<model>
//!
//! # Provider selection override
//! LLM_PROVIDER=factory  # or openclaw, gemini, gemini-cli, anthropic
//! LLM_MODEL=gemini-2.5-flash
//!
//! # Optional API key fallbacks
//! GEMINI_API_KEY=xxx
//! ANTHROPIC_API_KEY=xxx
//! ```

use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use tracing::{debug, info, warn};

use crate::anthropic::AnthropicClient;
use crate::antigravity::AntigravityProvider;
use crate::assistant::AssistantProvider;
use crate::factory::FactoryProvider;
use crate::gemini::GeminiClient;
use crate::gemini_cli::create_gemini_cli_provider;
use crate::openclaw::OpenClawProvider;
use crate::provider::{
    BoxedProvider, ChatMessage, ChatRequest, ChatResponse, LlmProvider, ModelInfo, ProviderType,
};
use crate::salad::SaladProvider;
use async_trait::async_trait;

/// Chat manager - handles multiple providers and model selection
pub struct ChatManager {
    providers: HashMap<ProviderType, BoxedProvider>,
    current_provider: Arc<RwLock<ProviderType>>,
    current_model: Arc<RwLock<String>>,
    current_model_non_sandboxed: Arc<RwLock<bool>>,
    model_cache: Arc<RwLock<HashMap<ProviderType, Vec<ModelInfo>>>>,
}

impl ChatManager {
    /// Create a new chat manager
    ///
    /// Initialization order:
    /// 1. Check LLM_PROVIDER environment variable
    /// 2. Factory — Local proxy (default)
    /// 3. GCloud ADC
    /// 4. Gemini (API key)
    /// 5. Anthropic (API key)
    pub fn new() -> Self {
        let mut providers: HashMap<ProviderType, BoxedProvider> = HashMap::new();
        let mut default_provider = None;
        let mut default_model = std::env::var("FACTORY_DEFAULT_MODEL")
            .or_else(|_| std::env::var("OPENCLAW_DEFAULT_MODEL"))
            .unwrap_or_else(|_| "gemini-2.5-flash".to_string());

        // Check environment variables
        let env_provider = std::env::var("LLM_PROVIDER").ok();
        let env_model = std::env::var("LLM_MODEL").ok();

        if let Some(ref provider_name) = env_provider {
            info!("📋 LLM_PROVIDER={}", provider_name);
        }
        if let Some(ref model_name) = env_model {
            info!("📋 LLM_MODEL={}", model_name);
            default_model = model_name.clone();
        }

        // =====================================================
        // Factory — explicit endpoint only.
        // =====================================================
        match FactoryProvider::from_env() {
            Ok(factory) => {
                info!("✅ Factory provider initialized");
                providers.insert(ProviderType::Factory, Box::new(factory));
                if default_provider.is_none() {
                    default_provider = Some(ProviderType::Factory);
                }
            }
            Err(e) => {
                debug!("Factory provider failed: {}", e);
            }
        }

        // =====================================================
        // Gemini CLI provider (optional)
        // =====================================================
        let wants_gemini_cli = std::env::var("ENABLE_GEMINI_CLI_PROVIDER")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
            || env_provider
                .as_deref()
                .map(|v| {
                    v.eq_ignore_ascii_case("gemini-cli")
                        || v.eq_ignore_ascii_case("gemini_cli")
                        || v.eq_ignore_ascii_case("geminicli")
                })
                .unwrap_or(false);

        if wants_gemini_cli {
            let gemini_cli = create_gemini_cli_provider();
            info!("✅ Gemini CLI provider initialized");
            providers.insert(ProviderType::GeminiCli, Box::new(gemini_cli));
            if default_provider.is_none() {
                default_provider = Some(ProviderType::GeminiCli);
            }
        }

        // =====================================================
        // Antigravity - native ADC/API-key/OpenAI-compatible owner. Provider
        // requests consume the revisioned RuntimeDocument and record returned
        // token usage; no `gcloud` subprocess participates in this route.
        // =====================================================
        match AntigravityProvider::from_env() {
            Ok(antigravity) => {
                info!("✅ Antigravity native provider initialized");
                providers.insert(ProviderType::Antigravity, Box::new(antigravity));
                if default_provider.is_none() {
                    default_provider = Some(ProviderType::Antigravity);
                }
            }
            Err(error) => {
                debug!("Antigravity provider unavailable: {error}");
            }
        }

        // =====================================================
        // Gemini - API key fallback
        // =====================================================
        if std::env::var("GEMINI_API_KEY").is_ok() {
            match GeminiClient::from_env() {
                Ok(gemini) => {
                    info!("✅ Gemini provider initialized (API key)");
                    providers.insert(ProviderType::Gemini, Box::new(gemini));
                    if default_provider.is_none() {
                        default_provider = Some(ProviderType::Gemini);
                    }
                }
                Err(e) => {
                    debug!("Gemini provider failed: {}", e);
                }
            }
        }

        // =====================================================
        // Assistant - trusted internal network (Incus container)
        // =====================================================
        if matches!(env_provider.as_deref(), Some("assistant"))
            || std::env::var("ASSISTANT_BASE_URL").is_ok()
            || std::env::var("ASSISTANT_DEFAULT_MODEL").is_ok()
        {
            match AssistantProvider::from_env() {
                Ok(assistant) => {
                    info!("✅ Assistant provider initialized");
                    providers.insert(ProviderType::Assistant, Box::new(assistant));
                    if default_provider.is_none() {
                        default_provider = Some(ProviderType::Assistant);
                    }
                }
                Err(e) => {
                    debug!("Assistant provider failed: {}", e);
                }
            }
        }

        // =====================================================
        // OpenClaw - trusted internal network
        // =====================================================
        if matches!(env_provider.as_deref(), Some("openclaw"))
            || std::env::var("OPENCLAW_BASE_URL").is_ok()
            || std::env::var("OPENCLAW_DEFAULT_MODEL").is_ok()
        {
            match OpenClawProvider::from_env() {
                Ok(openclaw) => {
                    info!("✅ OpenClaw provider initialized");
                    providers.insert(ProviderType::OpenClaw, Box::new(openclaw));
                    if default_provider.is_none() {
                        default_provider = Some(ProviderType::OpenClaw);
                    }
                }
                Err(e) => {
                    debug!("OpenClaw provider failed: {}", e);
                }
            }
        }

        // =====================================================
        // Salad AI Gateway - managed OpenAI-compatible endpoint (Bearer auth)
        // =====================================================
        if matches!(env_provider.as_deref(), Some("salad"))
            || std::env::var("SALAD_BASE_URL").is_ok()
            || std::env::var("SALAD_API_KEY").is_ok()
            || std::env::var("SALAD_DEFAULT_MODEL").is_ok()
        {
            match SaladProvider::from_env() {
                Ok(salad) => {
                    info!("✅ Salad AI Gateway provider initialized");
                    providers.insert(ProviderType::Salad, Box::new(salad));
                    if default_provider.is_none() {
                        default_provider = Some(ProviderType::Salad);
                    }
                }
                Err(e) => {
                    debug!("Salad provider failed: {}", e);
                }
            }
        }

        // =====================================================
        // Anthropic - OAuth2 Bearer token (preferred) or API key
        // =====================================================
        if let Ok(token) = std::env::var("ANTHROPIC_OAUTH_TOKEN") {
            let anthropic = AnthropicClient::with_oauth_token(token);
            info!("✅ Anthropic provider initialized (OAuth2 Bearer)");
            providers.insert(ProviderType::Anthropic, Box::new(anthropic));
            if default_provider.is_none() {
                default_provider = Some(ProviderType::Anthropic);
            }
        } else if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            match AnthropicClient::from_env() {
                Ok(anthropic) => {
                    info!("✅ Anthropic provider initialized (API key)");
                    providers.insert(ProviderType::Anthropic, Box::new(anthropic));
                    if default_provider.is_none() {
                        default_provider = Some(ProviderType::Anthropic);
                    }
                }
                Err(e) => {
                    debug!("Anthropic provider failed: {}", e);
                }
            }
        }

        // Use environment provider if specified and available
        let final_provider = if let Some(ref provider_name) = env_provider {
            if let Ok(pt) = provider_name.parse::<ProviderType>() {
                if providers.contains_key(&pt) {
                    info!("✅ Using LLM_PROVIDER: {:?}", pt);
                    pt
                } else {
                    warn!("⚠️  LLM_PROVIDER '{}' not available", provider_name);
                    default_provider.unwrap_or(ProviderType::Factory)
                }
            } else {
                warn!("⚠️  Invalid LLM_PROVIDER '{}'", provider_name);
                default_provider.unwrap_or(ProviderType::Factory)
            }
        } else {
            default_provider.unwrap_or(ProviderType::Factory)
        };

        if providers.is_empty() {
            warn!("⚠️  No LLM providers available!");
            warn!("   Configure authentication:");
            warn!("   1. Set FACTORY_BASE_URL for Factory");
            warn!("   2. Configure Google ADC or ANTIGRAVITY_BRIDGE_URL");
            warn!("   3. Or set OPENCLAW_BASE_URL and LLM_PROVIDER=openclaw");
            warn!("   4. Or set GEMINI_API_KEY environment variable");
        } else {
            info!("\n📊 Default provider: {:?}", final_provider);
            info!("📊 Default model: {}", default_model);
            info!("📊 Available providers: {}\n", providers.len());
        }

        Self {
            providers,
            current_provider: Arc::new(RwLock::new(final_provider)),
            current_model: Arc::new(RwLock::new(default_model)),
            current_model_non_sandboxed: Arc::new(RwLock::new(
                std::env::var("LLM_MODEL_NON_SANDBOXED")
                    .ok()
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false),
            )),
            model_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Add a provider
    pub fn add_provider(&mut self, provider: BoxedProvider) {
        let provider_type = provider.provider_type();
        self.providers.insert(provider_type, provider);
    }

    /// Get current provider type
    pub async fn current_provider(&self) -> ProviderType {
        self.current_provider.read().unwrap().clone()
    }

    /// Get current model
    pub async fn current_model(&self) -> String {
        self.current_model.read().unwrap().clone()
    }

    pub async fn current_model_non_sandboxed(&self) -> bool {
        *self.current_model_non_sandboxed.read().unwrap()
    }

    /// Switch provider
    pub async fn switch_provider(&self, provider_type: ProviderType) -> Result<()> {
        if !self.providers.contains_key(&provider_type) {
            return Err(anyhow::anyhow!(
                "Provider {:?} not available. Available: {:?}",
                provider_type,
                self.available_providers()
            ));
        }

        *self.current_provider.write().unwrap() = provider_type.clone();
        info!("Switched to provider: {:?}", provider_type);

        // Get first available model for this provider
        let models = self.list_models().await?;
        if let Some(first) = models.first() {
            *self.current_model.write().unwrap() = first.id.clone();
            info!("Default model set to: {}", first.id);
        }

        Ok(())
    }

    /// Switch model
    pub async fn switch_model(&self, model_id: impl Into<String>) -> Result<()> {
        self.switch_model_with_options(model_id, None).await
    }

    pub async fn switch_model_with_options(
        &self,
        model_id: impl Into<String>,
        non_sandboxed: Option<bool>,
    ) -> Result<()> {
        let model_id = model_id.into();
        *self.current_model.write().unwrap() = model_id.clone();
        if let Some(non_sandboxed) = non_sandboxed {
            *self.current_model_non_sandboxed.write().unwrap() = non_sandboxed;
        }
        info!(
            "Switched to model: {} (non_sandboxed={})",
            model_id,
            self.current_model_non_sandboxed().await
        );
        Ok(())
    }

    /// List available providers
    pub fn available_providers(&self) -> Vec<ProviderType> {
        self.providers.keys().cloned().collect()
    }

    /// Check if a provider is available
    pub fn has_provider(&self, provider_type: &ProviderType) -> bool {
        self.providers.contains_key(provider_type)
    }

    async fn resolve_provider(&self) -> Result<ProviderType> {
        let current = self.current_provider.read().unwrap().clone();
        if self.providers.contains_key(&current) {
            return Ok(current);
        }

        if let Some(fallback) = self.providers.keys().next().cloned() {
            warn!(
                "Provider {:?} not available, falling back to {:?}",
                current, fallback
            );
            *self.current_provider.write().unwrap() = fallback.clone();
            return Ok(fallback);
        }

        Err(anyhow!(
            "No LLM providers configured.\n\n\
            To authenticate:\n\
            1. Set FACTORY_BASE_URL for Factory\n\
            2. Configure Google ADC or ANTIGRAVITY_BRIDGE_URL\n\
            3. Or set OPENCLAW_BASE_URL and LLM_PROVIDER=openclaw\n\n\
            Or set GEMINI_API_KEY environment variable."
        ))
    }

    /// List models from current provider
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let provider_type = self.current_provider.read().unwrap().clone();

        // Check cache first
        {
            let cache = self.model_cache.read().unwrap();
            if let Some(models) = cache.get(&provider_type) {
                return Ok(models.clone());
            }
        }

        let provider = self
            .providers
            .get(&provider_type)
            .ok_or_else(|| anyhow!("Provider not available"))?;

        let models = provider.list_models().await?;

        // Cache
        {
            let mut cache = self.model_cache.write().unwrap();
            cache.insert(provider_type, models.clone());
        }

        Ok(models)
    }

    /// List models for a specific provider
    pub async fn list_models_for_provider(
        &self,
        provider_type: &ProviderType,
    ) -> Result<Vec<ModelInfo>> {
        let provider = self
            .providers
            .get(provider_type)
            .ok_or_else(|| anyhow!("Provider {:?} not available", provider_type))?;

        provider.list_models().await
    }

    /// Search models
    pub async fn search_models(&self, query: &str) -> Result<Vec<ModelInfo>> {
        let provider_type = self.current_provider.read().unwrap().clone();
        let provider = self
            .providers
            .get(&provider_type)
            .ok_or_else(|| anyhow!("Provider not available"))?;

        provider.search_models(query, 20).await
    }

    /// Refresh models (clear cache)
    pub async fn refresh_models(&self) -> Result<Vec<ModelInfo>> {
        let provider_type = self.current_provider.read().unwrap().clone();
        {
            let mut cache = self.model_cache.write().unwrap();
            cache.remove(&provider_type);
        }
        self.list_models().await
    }

    /// Get model info
    pub async fn get_model(&self, model_id: &str) -> Result<Option<ModelInfo>> {
        let provider_type = self.current_provider.read().unwrap().clone();
        let provider = self
            .providers
            .get(&provider_type)
            .ok_or_else(|| anyhow!("Provider not available"))?;

        provider.get_model(model_id).await
    }

    /// Check if model is available
    pub async fn is_model_available(&self, model_id: &str) -> Result<bool> {
        let provider_type = self.current_provider.read().unwrap().clone();
        let provider = self
            .providers
            .get(&provider_type)
            .ok_or_else(|| anyhow!("Provider not available"))?;

        provider.is_model_available(model_id).await
    }

    /// Send chat message
    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<ChatResponse> {
        let provider_type = self.resolve_provider().await?;
        let model = self.current_model.read().unwrap().clone();

        let provider = self
            .providers
            .get(&provider_type)
            .ok_or_else(|| anyhow!("Provider not available"))?;

        provider
            .chat(
                &model,
                crate::graph_turn_reminder::with_system_graph_reminder(messages),
            )
            .await
    }

    /// Send chat with specific provider and model
    pub async fn chat_with(
        &self,
        provider_type: &ProviderType,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<ChatResponse> {
        let provider = self
            .providers
            .get(provider_type)
            .ok_or_else(|| anyhow!("Provider {:?} not available", provider_type))?;

        provider
            .chat(
                model,
                crate::graph_turn_reminder::with_system_graph_reminder(messages),
            )
            .await
    }

    /// Send chat stream with specific provider and model
    pub async fn chat_stream_with(
        &self,
        provider_type: &ProviderType,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<String>>> {
        let provider = self
            .providers
            .get(provider_type)
            .ok_or_else(|| anyhow!("Provider {:?} not available", provider_type))?;

        provider
            .chat_stream(
                model,
                crate::graph_turn_reminder::with_system_graph_reminder(messages),
            )
            .await
    }

    /// Find which provider supports the given model
    pub async fn find_provider_for_model(&self, model: &str) -> Option<ProviderType> {
        let providers = self.available_providers();
        // 1. Direct match in list_models_for_provider
        for ptype in &providers {
            if let Ok(models) = self.list_models_for_provider(ptype).await
                && models.iter().any(|m| m.id == model)
            {
                return Some(ptype.clone());
            }
        }

        // 2. Prefix / substring match
        let model_lower = model.to_lowercase();
        if (model_lower.contains("factory")
            || model_lower.contains("droid")
            || model_lower.contains("kimi"))
            && providers.contains(&ProviderType::Factory)
        {
            return Some(ProviderType::Factory);
        }
        if model_lower.contains("claude") && providers.contains(&ProviderType::Anthropic) {
            return Some(ProviderType::Anthropic);
        }
        if model_lower.contains("gemini") {
            // Check in order of preference
            if providers.contains(&ProviderType::Antigravity) {
                return Some(ProviderType::Antigravity);
            }
            if providers.contains(&ProviderType::Gemini) {
                return Some(ProviderType::Gemini);
            }
            if providers.contains(&ProviderType::GeminiCli) {
                return Some(ProviderType::GeminiCli);
            }
        }
        if model_lower.contains("gpt") {
            if providers.contains(&ProviderType::OpenClaw) {
                return Some(ProviderType::OpenClaw);
            }
            if providers.contains(&ProviderType::Assistant) {
                return Some(ProviderType::Assistant);
            }
            if providers.contains(&ProviderType::OpenAI) {
                return Some(ProviderType::OpenAI);
            }
        }

        // 3. Fallback to first available provider
        providers.first().cloned()
    }

    /// Get status
    pub async fn get_status(&self) -> simd_json::OwnedValue {
        let provider = self.current_provider.read().unwrap().clone();
        let model = self.current_model.read().unwrap().clone();
        let providers: Vec<String> = self
            .available_providers()
            .iter()
            .map(|p| p.to_string())
            .collect();

        simd_json::json!({
            "provider": provider.to_string(),
            "model": model,
            "available_providers": providers,
        })
    }

    /// Get detailed status
    pub async fn get_detailed_status(&self) -> simd_json::OwnedValue {
        let current_provider = self.current_provider.read().unwrap().clone();
        let current_model = self.current_model.read().unwrap().clone();

        let mut provider_status = simd_json::value::owned::Object::new();

        for ptype in self.providers.keys() {
            let (available, model_count, model_list_error) =
                model_listing_status(self.list_models_for_provider(ptype).await);
            let (auth_type, features) = match ptype {
                ProviderType::Factory => (
                    "Factory AI endpoint (FACTORY_BASE_URL)",
                    vec![
                        "OpenAI-compatible API",
                        "Configured Factory endpoint",
                        "Default for op-web chat",
                    ],
                ),
                ProviderType::Antigravity => (
                    "Native Google ADC or OpenAI-compatible bridge",
                    vec!["Configured provider model", "Durable local response usage"],
                ),
                ProviderType::Gemini => (
                    "API key (GEMINI_API_KEY)",
                    vec!["Gemini models", "Multimodal", "Long context"],
                ),
                ProviderType::GeminiCli => (
                    "Local Gemini CLI bridge",
                    vec![
                        "Gemini CLI binary",
                        "ADC/service account auth",
                        "Headless-friendly",
                    ],
                ),
                ProviderType::Anthropic => (
                    "API key (ANTHROPIC_API_KEY)",
                    vec!["Claude models", "Best reasoning", "Tool use"],
                ),
                ProviderType::OpenClaw => (
                    "Trusted internal network (OPENCLAW_BASE_URL)",
                    vec!["OpenAI-compatible API", "Agent platform", "Tool use"],
                ),
                ProviderType::Salad => (
                    "API key (SALAD_API_KEY, Bearer)",
                    vec![
                        "OpenAI-compatible API",
                        "SaladCloud GPU network",
                        "Qwen models",
                        "Tool use",
                    ],
                ),
                ProviderType::Assistant => (
                    "Trusted internal network (ASSISTANT_BASE_URL)",
                    vec!["OpenAI-compatible API", "Incus container", "Tool use"],
                ),
                _ => ("API key", vec![]),
            };

            provider_status.insert(
                ptype.to_string(),
                simd_json::json!({
                    "available": available,
                    "model_count": model_count,
                    "model_list_error": model_list_error,
                    "auth_type": auth_type,
                    "features": features,
                }),
            );
        }

        simd_json::json!({
            "current_provider": current_provider.to_string(),
            "current_model": current_model,
            "providers": provider_status,
        })
    }
}

fn model_listing_status<E>(result: Result<Vec<ModelInfo>, E>) -> (bool, usize, bool) {
    match result {
        Ok(models) => (true, models.len(), false),
        Err(_) => (false, 0, true),
    }
}

#[async_trait]
impl LlmProvider for ChatManager {
    fn provider_type(&self) -> ProviderType {
        self.current_provider.read().unwrap().clone()
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        ChatManager::list_models(self).await
    }

    async fn search_models(&self, query: &str, limit: usize) -> Result<Vec<ModelInfo>> {
        let _limit = limit;
        ChatManager::search_models(self, query).await
    }

    async fn get_model(&self, model_id: &str) -> Result<Option<ModelInfo>> {
        ChatManager::get_model(self, model_id).await
    }

    async fn is_model_available(&self, model_id: &str) -> Result<bool> {
        ChatManager::is_model_available(self, model_id).await
    }

    async fn chat(&self, model: &str, messages: Vec<ChatMessage>) -> Result<ChatResponse> {
        let provider_type = self.resolve_provider().await?;
        let provider = self
            .providers
            .get(&provider_type)
            .ok_or_else(|| anyhow!("Provider {:?} not available", provider_type))?;

        provider
            .chat(
                model,
                crate::graph_turn_reminder::with_system_graph_reminder(messages),
            )
            .await
    }

    async fn chat_with_request(
        &self,
        model: &str,
        mut request: ChatRequest,
    ) -> Result<ChatResponse> {
        let provider_type = self.resolve_provider().await?;
        let provider = self
            .providers
            .get(&provider_type)
            .ok_or_else(|| anyhow!("Provider {:?} not available", provider_type))?;

        request.messages = crate::graph_turn_reminder::with_system_graph_reminder(request.messages);
        provider.chat_with_request(model, request).await
    }

    async fn chat_stream(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<String>>> {
        let provider_type = self.resolve_provider().await?;
        let provider = self
            .providers
            .get(&provider_type)
            .ok_or_else(|| anyhow!("Provider {:?} not available", provider_type))?;

        provider
            .chat_stream(
                model,
                crate::graph_turn_reminder::with_system_graph_reminder(messages),
            )
            .await
    }
}

impl Default for ChatManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod detailed_status_tests {
    use super::model_listing_status;
    use crate::provider::ModelInfo;

    #[test]
    fn model_listing_failure_is_not_reported_as_available() {
        let (available, count, failed) =
            model_listing_status::<anyhow::Error>(Err(anyhow::anyhow!("provider offline")));
        assert!(!available);
        assert_eq!(count, 0);
        assert!(failed);

        let (available, count, failed) =
            model_listing_status::<anyhow::Error>(Ok(Vec::<ModelInfo>::new()));
        assert!(available);
        assert_eq!(count, 0);
        assert!(!failed);
    }
}

#[cfg(test)]
#[path = "graph_turn_manager_tests.rs"]
mod graph_turn_manager_tests;
