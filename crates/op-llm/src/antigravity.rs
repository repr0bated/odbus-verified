//! Native Google/Antigravity provider and its durable control-plane owner.
//!
//! Provider configuration and safety policy are revisioned `RuntimeDocuments`.
//! Every production request reads that document before selecting its endpoint,
//! model and Gemini `safetySettings`. Successful provider responses append their
//! returned token counts to a separate durable local ledger. Google ADC refresh
//! uses the native Google Cloud auth SDK; this module never invokes `gcloud`.

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use google_cloud_auth::credentials::{AccessTokenCredentials, Builder as GoogleCredentialsBuilder};
use op_plugins::state_plugins::{
    antigravity::{
        AntigravityAuthStatus, ConfigureSafetyInput, ConfigureSafetyOutput, LocalModelUsage,
        LocalUsageReport, SafetySetting,
    },
    antigravity_chat::{ConfigureInput, ConfigureOutput, NativeBridgeStatus, TokenRefreshReceipt},
    runtime_documents::{RuntimeDocuments, runtime_documents},
};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{OwnedValue as Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tracing::debug;
use uuid::Uuid;

use crate::provider::{
    ChatMessage, ChatRequest, ChatResponse, LlmProvider, ModelInfo, ProviderType, TokenUsage,
    ToolCallInfo, ToolChoice, ToolDefinition,
};

const DEFAULT_GEMINI_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";
const RUNTIME_DOCUMENT_ID: &str = "antigravity.runtime.v1";
const USAGE_DOCUMENT_ID: &str = "antigravity.usage.v1";
const BRIDGE_STATUS_DOCUMENT_ID: &str = "antigravity.bridge-status.v1";
const CLOUD_PLATFORM_SCOPE: &str = "https://www.googleapis.com/auth/cloud-platform";
const MAX_PROVIDER_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_BRIDGE_MODELS: usize = 512;

/// Runtime state shared by both Antigravity plugin surfaces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityRuntimeConfig {
    pub provider: String,
    pub base_url: String,
    pub selected_model: String,
    #[serde(default)]
    pub safety_settings: Vec<SafetySetting>,
}

impl Default for AntigravityRuntimeConfig {
    fn default() -> Self {
        let bridge = std::env::var("ANTIGRAVITY_BRIDGE_URL")
            .ok()
            .filter(|value| !value.is_empty());
        Self {
            provider: if bridge.is_some() {
                "openai_compatible".to_string()
            } else {
                "gemini".to_string()
            },
            base_url: bridge.unwrap_or_else(|| DEFAULT_GEMINI_BASE.to_string()),
            selected_model: std::env::var("LLM_MODEL")
                .unwrap_or_else(|_| "gemini-2.5-flash".to_string()),
            safety_settings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct UsageLedgerDocument {
    #[serde(default)]
    models: HashMap<String, LocalModelUsage>,
    #[serde(default)]
    last_recorded_at: Option<String>,
}

/// Process credential owner. It reads only the explicitly selected ADC source,
/// delegates parsing, refresh and token caching to the Google Cloud auth SDK,
/// and never returns credential material in status.
pub struct GoogleCredentialOwner {
    credentials: Option<AccessTokenCredentials>,
    scopes: Vec<String>,
}

impl GoogleCredentialOwner {
    pub fn from_env() -> Self {
        let scopes = vec![CLOUD_PLATFORM_SCOPE.to_string()];
        let credentials = GoogleCredentialsBuilder::default()
            .with_scopes(scopes.clone())
            .build_access_token_credentials()
            .ok();
        Self {
            credentials,
            scopes,
        }
    }

    pub async fn status(&self) -> AntigravityAuthStatus {
        let available = self.credentials.is_some();
        AntigravityAuthStatus {
            credential_type: if self.credentials.is_some() {
                "application_default_credentials".to_string()
            } else {
                "none".to_string()
            },
            credential_source: "google_cloud_auth_sdk".to_string(),
            credential_available: available,
            refreshable: self.credentials.is_some(),
            scopes: self.scopes.clone(),
        }
    }

    pub async fn refresh(&self) -> Result<TokenRefreshReceipt> {
        let token = self
            .credentials
            .as_ref()
            .context("Google application-default credentials are unavailable")?
            .access_token()
            .await
            .context("Google Cloud auth SDK could not obtain an access token")?;
        anyhow::ensure!(!token.token.is_empty(), "Google access token is empty");
        Ok(TokenRefreshReceipt {
            credential_type: "application_default_credentials".to_string(),
            token_available: true,
            observed_at: Utc::now().to_rfc3339(),
            scopes: self.scopes.clone(),
        })
    }

    pub async fn access_token(&self) -> Result<String> {
        let token = self
            .credentials
            .as_ref()
            .context("Google application-default credentials are unavailable")?
            .access_token()
            .await
            .context("Google Cloud auth SDK could not obtain an access token")?;
        anyhow::ensure!(!token.token.is_empty(), "Google access token is empty");
        Ok(token.token)
    }

    /// Share an explicitly configured SDK credential owner without changing
    /// process environment or rebuilding credentials for each admitted call.
    pub fn from_credentials(credentials: AccessTokenCredentials, scopes: Vec<String>) -> Self {
        Self {
            credentials: Some(credentials),
            scopes,
        }
    }
}

static GOOGLE_CREDENTIALS: OnceLock<Arc<GoogleCredentialOwner>> = OnceLock::new();

pub fn google_credential_owner() -> Arc<GoogleCredentialOwner> {
    GOOGLE_CREDENTIALS
        .get_or_init(|| Arc::new(GoogleCredentialOwner::from_env()))
        .clone()
}

/// Observe only the credential mechanism selected by the durable provider
/// configuration. A bridge URL alone is never reported as authentication.
pub async fn auth_status_for_config(
    owner: &GoogleCredentialOwner,
    config: &AntigravityRuntimeConfig,
) -> AntigravityAuthStatus {
    if config.provider == "openai_compatible" {
        return match bridge_api_key_for_url(&config.base_url) {
            Ok(Some(_)) => AntigravityAuthStatus {
                credential_type: "api_key".to_string(),
                credential_source: "bound_environment".to_string(),
                credential_available: true,
                refreshable: false,
                scopes: Vec::new(),
            },
            _ => AntigravityAuthStatus {
                credential_type: "none".to_string(),
                credential_source: "bridge".to_string(),
                ..Default::default()
            },
        };
    }

    if std::env::var("GEMINI_API_KEY").is_ok_and(|key| !key.is_empty()) {
        AntigravityAuthStatus {
            credential_type: "api_key".to_string(),
            credential_source: "environment".to_string(),
            credential_available: true,
            refreshable: false,
            scopes: Vec::new(),
        }
    } else {
        owner.status().await
    }
}

fn is_loopback(url: &Url) -> bool {
    url.host_str()
        .is_some_and(|host| matches!(host, "127.0.0.1" | "::1" | "localhost"))
}

fn parse_clean_base_url(value: &str) -> Result<Url> {
    let url = Url::parse(value).context("invalid provider base_url")?;
    anyhow::ensure!(
        url.username().is_empty() && url.password().is_none(),
        "provider base_url must not contain user information"
    );
    anyhow::ensure!(
        url.query().is_none() && url.fragment().is_none(),
        "provider base_url must not contain a query or fragment"
    );
    Ok(url)
}

fn bridge_api_key_for_url(base_url: &str) -> Result<Option<String>> {
    let key = std::env::var("ANTIGRAVITY_BRIDGE_API_KEY")
        .ok()
        .filter(|value| !value.is_empty());
    let Some(key) = key else {
        return Ok(None);
    };
    let bound = std::env::var("ANTIGRAVITY_BRIDGE_AUTH_ORIGIN")
        .context("ANTIGRAVITY_BRIDGE_AUTH_ORIGIN is required when a bridge key is configured")?;
    let target = parse_clean_base_url(base_url)?;
    let origin = parse_clean_base_url(&bound).context("invalid bridge auth origin")?;
    anyhow::ensure!(
        origin.path() == "/",
        "bridge auth origin must contain only scheme, host and port"
    );
    anyhow::ensure!(
        target.origin() == origin.origin(),
        "bridge API key is not bound to the configured provider origin"
    );
    Ok(Some(key))
}

fn validate_model_id(model: &str) -> Result<()> {
    anyhow::ensure!(
        !model.is_empty()
            && model.len() <= 256
            && model
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')),
        "model id is invalid"
    );
    Ok(())
}

fn validate_runtime_config_inner(
    config: &AntigravityRuntimeConfig,
    allow_gemini_loopback_fixture: bool,
) -> Result<()> {
    anyhow::ensure!(
        matches!(config.provider.as_str(), "gemini" | "openai_compatible"),
        "provider must be 'gemini' or 'openai_compatible'"
    );
    validate_model_id(&config.selected_model)?;
    anyhow::ensure!(
        config.safety_settings.len() <= 16,
        "too many safety settings"
    );
    let url = parse_clean_base_url(&config.base_url)?;
    match config.provider.as_str() {
        "gemini"
            if allow_gemini_loopback_fixture && url.scheme() == "http" && is_loopback(&url) => {}
        "gemini" => {
            anyhow::ensure!(
                url.scheme() == "https"
                    && url.host_str() == Some("generativelanguage.googleapis.com")
                    && url.port().is_none()
                    && url.path().trim_end_matches('/') == "/v1beta",
                "Gemini credentials are restricted to the Google Generative Language v1beta endpoint"
            );
        }
        "openai_compatible" => {
            anyhow::ensure!(
                url.scheme() == "https" || (url.scheme() == "http" && is_loopback(&url)),
                "bridge URL must use HTTPS or loopback HTTP"
            );
            anyhow::ensure!(
                url.path().trim_matches('/').is_empty(),
                "bridge base_url must be an origin without a path"
            );
            bridge_api_key_for_url(&config.base_url)?;
        }
        _ => unreachable!("provider was validated above"),
    }
    validate_safety_settings(&config.safety_settings)
}

fn validate_runtime_config(config: &AntigravityRuntimeConfig) -> Result<()> {
    validate_runtime_config_inner(config, false)
}

fn validate_safety_settings(settings: &[SafetySetting]) -> Result<()> {
    const CATEGORIES: &[&str] = &[
        "HARM_CATEGORY_HARASSMENT",
        "HARM_CATEGORY_HATE_SPEECH",
        "HARM_CATEGORY_SEXUALLY_EXPLICIT",
        "HARM_CATEGORY_DANGEROUS_CONTENT",
        "HARM_CATEGORY_CIVIC_INTEGRITY",
    ];
    const THRESHOLDS: &[&str] = &[
        "HARM_BLOCK_THRESHOLD_UNSPECIFIED",
        "BLOCK_LOW_AND_ABOVE",
        "BLOCK_MEDIUM_AND_ABOVE",
        "BLOCK_ONLY_HIGH",
        "BLOCK_NONE",
        "OFF",
    ];
    let mut seen = HashSet::new();
    for setting in settings {
        anyhow::ensure!(
            CATEGORIES.contains(&setting.category.as_str()),
            "unsupported Gemini safety category"
        );
        anyhow::ensure!(
            THRESHOLDS.contains(&setting.threshold.as_str()),
            "unsupported Gemini safety threshold"
        );
        anyhow::ensure!(
            seen.insert(&setting.category),
            "duplicate Gemini safety category"
        );
    }
    Ok(())
}

async fn bounded_response_bytes(mut response: reqwest::Response) -> Result<(u16, Vec<u8>)> {
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            bytes.len().saturating_add(chunk.len()) <= MAX_PROVIDER_RESPONSE_BYTES,
            "provider response exceeds 2 MiB"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok((status, bytes))
}

pub async fn load_runtime_config_from(
    documents: &RuntimeDocuments,
) -> Result<(u64, AntigravityRuntimeConfig)> {
    match documents.read(RUNTIME_DOCUMENT_ID).await? {
        Some(row) => {
            let config = serde_json::from_value(row.document)
                .context("invalid durable Antigravity runtime document")?;
            validate_runtime_config(&config)?;
            Ok((row.revision, config))
        }
        None => Ok((0, AntigravityRuntimeConfig::default())),
    }
}

pub async fn load_runtime_config() -> Result<(u64, AntigravityRuntimeConfig)> {
    let documents = runtime_documents().await?;
    load_runtime_config_from(&documents).await
}

pub async fn configure_runtime_with_documents(
    documents: Arc<RuntimeDocuments>,
    input: ConfigureInput,
) -> Result<ConfigureOutput> {
    let ConfigureInput {
        expected_revision,
        provider,
        base_url,
        selected_model,
    } = input;
    let (row, config) = documents
        .update_at_revision_with_result(RUNTIME_DOCUMENT_ID, expected_revision, move |old| {
            let mut config = match old {
                Some(value) => serde_json::from_value(value)?,
                None => AntigravityRuntimeConfig::default(),
            };
            config.provider = provider;
            config.base_url = base_url.trim_end_matches('/').to_string();
            config.selected_model = selected_model;
            validate_runtime_config(&config)?;
            Ok((serde_json::to_value(&config)?, config))
        })
        .await?;
    Ok(ConfigureOutput {
        revision: row.revision,
        provider: config.provider,
        base_url: config.base_url,
        selected_model: config.selected_model,
    })
}

pub async fn configure_safety_with_documents(
    documents: Arc<RuntimeDocuments>,
    input: ConfigureSafetyInput,
) -> Result<ConfigureSafetyOutput> {
    validate_safety_settings(&input.settings)?;
    let settings = input.settings;
    let (row, committed) = documents
        .update_at_revision_with_result(RUNTIME_DOCUMENT_ID, input.expected_revision, move |old| {
            let mut config = match old {
                Some(value) => serde_json::from_value(value)?,
                None => AntigravityRuntimeConfig::default(),
            };
            config.safety_settings = settings;
            validate_runtime_config(&config)?;
            let committed = config.safety_settings.clone();
            Ok((serde_json::to_value(config)?, committed))
        })
        .await?;
    Ok(ConfigureSafetyOutput {
        revision: row.revision,
        settings: committed,
    })
}

fn fold_usage(
    ledger: &mut UsageLedgerDocument,
    model: &str,
    usage: &TokenUsage,
    now: DateTime<Utc>,
) {
    let model_usage = ledger.models.entry(model.to_string()).or_default();
    model_usage.requests = model_usage.requests.saturating_add(1);
    model_usage.prompt_tokens = model_usage
        .prompt_tokens
        .saturating_add(u64::from(usage.prompt_tokens));
    model_usage.completion_tokens = model_usage
        .completion_tokens
        .saturating_add(u64::from(usage.completion_tokens));
    model_usage.total_tokens = model_usage
        .total_tokens
        .saturating_add(u64::from(usage.total_tokens));
    ledger.last_recorded_at = Some(now.to_rfc3339());
}

pub async fn record_usage_with_documents(
    documents: Arc<RuntimeDocuments>,
    model: String,
    usage: TokenUsage,
) -> Result<()> {
    documents
        .update(USAGE_DOCUMENT_ID, move |old| {
            let mut ledger: UsageLedgerDocument = match old {
                Some(value) => serde_json::from_value(value)?,
                None => UsageLedgerDocument::default(),
            };
            fold_usage(&mut ledger, &model, &usage, Utc::now());
            serde_json::to_value(ledger).map_err(Into::into)
        })
        .await?;
    Ok(())
}

async fn record_usage(model: String, usage: TokenUsage) -> Result<()> {
    record_usage_with_documents(runtime_documents().await?, model, usage).await
}

pub async fn usage_report_with_documents(
    documents: &RuntimeDocuments,
    model: Option<&str>,
) -> Result<LocalUsageReport> {
    let ledger: UsageLedgerDocument = match documents.read(USAGE_DOCUMENT_ID).await? {
        Some(row) => {
            serde_json::from_value(row.document).context("invalid Antigravity usage ledger")?
        }
        None => UsageLedgerDocument::default(),
    };
    let models: HashMap<String, LocalModelUsage> = match model {
        Some(model) => ledger
            .models
            .get(model)
            .cloned()
            .map(|usage| [(model.to_string(), usage)].into_iter().collect())
            .unwrap_or_default(),
        None => ledger.models,
    };
    Ok(LocalUsageReport {
        source: "local_provider_response_ledger".to_string(),
        requests: models.values().map(|value| value.requests).sum(),
        prompt_tokens: models.values().map(|value| value.prompt_tokens).sum(),
        completion_tokens: models.values().map(|value| value.completion_tokens).sum(),
        total_tokens: models.values().map(|value| value.total_tokens).sum(),
        models,
        last_recorded_at: ledger.last_recorded_at,
    })
}

pub async fn usage_report(model: Option<&str>) -> Result<LocalUsageReport> {
    let documents = runtime_documents().await?;
    usage_report_with_documents(&documents, model).await
}

pub async fn bridge_status_with_config(
    config: &AntigravityRuntimeConfig,
) -> Result<NativeBridgeStatus> {
    anyhow::ensure!(
        config.provider == "openai_compatible",
        "configured provider is not an OpenAI-compatible bridge"
    );
    validate_runtime_config(config)?;
    let url = format!("{}/v1/models", config.base_url.trim_end_matches('/'));
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let mut request = client.get(&url);
    if let Some(key) = bridge_api_key_for_url(&config.base_url)? {
        request = request.bearer_auth(key);
    }
    let response = match request.send().await {
        Ok(response) => response,
        Err(_) => {
            return Ok(NativeBridgeStatus {
                url: config.base_url.clone(),
                reachable: false,
                http_status: 0,
                model_ids: Vec::new(),
                checked_at: Utc::now().to_rfc3339(),
            });
        }
    };
    let (status, bytes) = bounded_response_bytes(response).await?;
    let mut model_ids = Vec::new();
    if (200..300).contains(&status) {
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).context("invalid bridge models response")?;
        if let Some(models) = value.get("data").and_then(serde_json::Value::as_array) {
            anyhow::ensure!(
                models.len() <= MAX_BRIDGE_MODELS,
                "bridge returned too many models"
            );
            for model in models {
                if let Some(id) = model.get("id").and_then(serde_json::Value::as_str) {
                    anyhow::ensure!(
                        !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control),
                        "invalid bridge model id"
                    );
                    model_ids.push(id.to_string());
                }
            }
        }
    }
    Ok(NativeBridgeStatus {
        url: config.base_url.clone(),
        reachable: (200..300).contains(&status),
        http_status: status,
        model_ids,
        checked_at: Utc::now().to_rfc3339(),
    })
}

pub async fn bridge_status() -> Result<NativeBridgeStatus> {
    let (_, config) = load_runtime_config().await?;
    bridge_status_with_config(&config).await
}

pub async fn observe_bridge_with_documents(
    documents: Arc<RuntimeDocuments>,
    config: &AntigravityRuntimeConfig,
) -> Result<NativeBridgeStatus> {
    let status = bridge_status_with_config(config).await?;
    let committed = status.clone();
    documents
        .update(BRIDGE_STATUS_DOCUMENT_ID, move |_| {
            serde_json::to_value(&committed).map_err(Into::into)
        })
        .await?;
    Ok(status)
}

pub async fn last_bridge_status_with_documents(
    documents: &RuntimeDocuments,
) -> Result<Option<NativeBridgeStatus>> {
    documents
        .read(BRIDGE_STATUS_DOCUMENT_ID)
        .await?
        .map(|row| {
            serde_json::from_value(row.document)
                .context("invalid durable Antigravity bridge observation")
        })
        .transpose()
}

#[derive(Clone)]
enum AuthMethod {
    Automatic {
        google: Arc<GoogleCredentialOwner>,
        api_key: Option<String>,
    },
    Google(Arc<GoogleCredentialOwner>),
    ApiKey(String),
}

pub struct AntigravityProvider {
    client: Client,
    auth: AuthMethod,
    runtime_override: Option<AntigravityRuntimeConfig>,
    record_usage: bool,
}

impl AntigravityProvider {
    pub fn from_env() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("failed to create Antigravity HTTP client")?;
        let auth = AuthMethod::Automatic {
            google: google_credential_owner(),
            api_key: std::env::var("GEMINI_API_KEY")
                .ok()
                .filter(|value| !value.is_empty()),
        };
        Ok(Self {
            client,
            auth,
            runtime_override: None,
            record_usage: true,
        })
    }

    pub fn with_api_key(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_default(),
            auth: AuthMethod::ApiKey(api_key.into()),
            runtime_override: None,
            record_usage: true,
        }
    }

    pub fn with_google_credentials(owner: Arc<GoogleCredentialOwner>) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_default(),
            auth: AuthMethod::Google(owner),
            runtime_override: None,
            record_usage: true,
        }
    }

    #[cfg(test)]
    fn with_fixture(api_key: &str, runtime: AntigravityRuntimeConfig) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            auth: AuthMethod::ApiKey(api_key.to_string()),
            runtime_override: Some(runtime),
            record_usage: false,
        }
    }

    async fn runtime(&self) -> Result<AntigravityRuntimeConfig> {
        if let Some(runtime) = &self.runtime_override {
            return Ok(runtime.clone());
        }
        let (_, runtime) = load_runtime_config().await?;
        Ok(runtime)
    }

    async fn authenticated_post(
        &self,
        url: &str,
        provider: &str,
    ) -> Result<reqwest::RequestBuilder> {
        if provider == "openai_compatible" {
            let mut request = self
                .client
                .post(url)
                .header("Content-Type", "application/json");
            if let Some(key) = bridge_api_key_for_url(url)? {
                request = request.bearer_auth(key);
            }
            return Ok(request);
        }
        match &self.auth {
            AuthMethod::Automatic { google, api_key } => {
                if let Some(key) = api_key {
                    let mut url = Url::parse(url)?;
                    url.query_pairs_mut().append_pair("key", key);
                    Ok(self
                        .client
                        .post(url)
                        .header("Content-Type", "application/json"))
                } else if google.status().await.credential_available {
                    Ok(self
                        .client
                        .post(url)
                        .bearer_auth(google.access_token().await?)
                        .header("Content-Type", "application/json"))
                } else {
                    anyhow::bail!("Gemini mode requires Google ADC or GEMINI_API_KEY")
                }
            }
            AuthMethod::Google(owner) => Ok(self
                .client
                .post(url)
                .bearer_auth(owner.access_token().await?)
                .header("Content-Type", "application/json")),
            AuthMethod::ApiKey(key) => {
                let mut url = Url::parse(url)?;
                url.query_pairs_mut().append_pair("key", key);
                Ok(self
                    .client
                    .post(url)
                    .header("Content-Type", "application/json"))
            }
        }
    }

    fn convert_messages(messages: &[ChatMessage]) -> (Vec<Value>, Option<Value>) {
        let system_instruction = crate::graph_turn_reminder::ordered_system_content(messages)
            .map(|content| json!({"parts": [{"text": content}]}));
        let mut contents = Vec::new();
        for message in messages {
            if message.role == "system" {
                continue;
            } else {
                let role = if message.role == "assistant" {
                    "model"
                } else {
                    "user"
                };
                contents.push(json!({"role": role, "parts": [{"text": message.content}]}));
            }
        }
        (contents, system_instruction)
    }

    fn convert_tools(tools: &[ToolDefinition]) -> Value {
        json!([{"functionDeclarations": tools.iter().map(|tool| json!({
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema
        })).collect::<Vec<_>>() }])
    }

    fn convert_tool_choice(choice: &ToolChoice) -> Value {
        match choice {
            ToolChoice::Auto => json!({"mode": "AUTO"}),
            ToolChoice::Required => json!({"mode": "ANY"}),
            ToolChoice::None => json!({"mode": "NONE"}),
            ToolChoice::Tool(name) => {
                json!({"mode": "ANY", "allowedFunctionNames": [name]})
            }
        }
    }

    async fn record_response_usage(&self, model: &str, usage: Option<&TokenUsage>) -> Result<()> {
        if self.record_usage
            && let Some(usage) = usage
        {
            record_usage(model.to_string(), usage.clone())
                .await
                .context("provider response completed but durable usage commit failed")?;
        }
        Ok(())
    }
}

#[async_trait]
impl LlmProvider for AntigravityProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Antigravity
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let runtime = self.runtime().await?;
        if runtime.provider == "openai_compatible" {
            let status = bridge_status_with_config(&runtime).await?;
            return Ok(status
                .model_ids
                .into_iter()
                .map(|id| ModelInfo {
                    name: id.clone(),
                    id,
                    description: None,
                    parameters: None,
                    available: true,
                    tags: vec!["openai-compatible".to_string()],
                    downloads: None,
                    updated_at: None,
                })
                .collect());
        }
        Ok(vec![ModelInfo {
            id: runtime.selected_model.clone(),
            name: runtime.selected_model,
            description: Some("Configured Gemini model".to_string()),
            parameters: None,
            available: true,
            tags: vec!["gemini".to_string()],
            downloads: None,
            updated_at: None,
        }])
    }

    async fn search_models(&self, query: &str, limit: usize) -> Result<Vec<ModelInfo>> {
        let query = query.to_ascii_lowercase();
        Ok(self
            .list_models()
            .await?
            .into_iter()
            .filter(|model| model.id.to_ascii_lowercase().contains(&query))
            .take(limit)
            .collect())
    }

    async fn get_model(&self, model_id: &str) -> Result<Option<ModelInfo>> {
        Ok(self
            .list_models()
            .await?
            .into_iter()
            .find(|model| model.id == model_id))
    }

    async fn is_model_available(&self, model_id: &str) -> Result<bool> {
        Ok(self.get_model(model_id).await?.is_some())
    }

    async fn chat(&self, model: &str, messages: Vec<ChatMessage>) -> Result<ChatResponse> {
        self.chat_with_request(model, ChatRequest::new(messages))
            .await
    }

    async fn chat_with_request(&self, model: &str, request: ChatRequest) -> Result<ChatResponse> {
        let runtime = self.runtime().await?;
        validate_runtime_config_inner(&runtime, self.runtime_override.is_some())?;
        let actual_model = if model.is_empty() || model == "auto" {
            runtime.selected_model.as_str()
        } else {
            model
        };
        validate_model_id(actual_model)?;

        if runtime.provider == "openai_compatible" {
            let url = format!(
                "{}/v1/chat/completions",
                runtime.base_url.trim_end_matches('/')
            );
            let body = json!({
                "model": actual_model,
                "messages": request.messages.iter().map(|message| json!({"role": message.role, "content": message.content})).collect::<Vec<_>>(),
                "temperature": request.temperature.unwrap_or(0.7),
            });
            let response = self
                .authenticated_post(&url, &runtime.provider)
                .await?
                .json(&body)
                .send()
                .await
                .context("bridge request failed")?;
            let (status, mut bytes) = bounded_response_bytes(response).await?;
            anyhow::ensure!((200..300).contains(&status), "bridge returned {status}");
            let result: Value =
                simd_json::to_owned_value(&mut bytes).context("invalid bridge response")?;
            let content = result
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|values| values.first())
                .and_then(|choice| choice.get("message"))
                .and_then(|message| message.get("content"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let finish_reason = result
                .get("choices")
                .and_then(Value::as_array)
                .and_then(|values| values.first())
                .and_then(|choice| choice.get("finish_reason"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            let usage = result.get("usage").map(|usage| TokenUsage {
                prompt_tokens: usage
                    .get("prompt_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .min(u64::from(u32::MAX)) as u32,
                completion_tokens: usage
                    .get("completion_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .min(u64::from(u32::MAX)) as u32,
                total_tokens: usage
                    .get("total_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .min(u64::from(u32::MAX)) as u32,
            });
            self.record_response_usage(actual_model, usage.as_ref())
                .await?;
            return Ok(ChatResponse {
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content,
                    tool_calls: None,
                    tool_call_id: None,
                },
                model: actual_model.to_string(),
                provider: "antigravity-bridge".to_string(),
                finish_reason,
                usage,
                tool_calls: None,
            });
        }

        let url = format!(
            "{}/models/{}:generateContent",
            runtime.base_url.trim_end_matches('/'),
            actual_model
        );
        let (contents, system_instruction) = Self::convert_messages(&request.messages);
        let mut body = json!({"contents": contents});
        if !runtime.safety_settings.is_empty() {
            body.as_object_mut()
                .context("Gemini request body is not an object")?
                .insert(
                    "safetySettings".to_string(),
                    simd_json::serde::to_owned_value(&runtime.safety_settings)?,
                );
        }
        if let Some(system_instruction) = system_instruction {
            body.as_object_mut()
                .context("Gemini request body is not an object")?
                .insert("systemInstruction".to_string(), system_instruction);
        }
        if !request.tools.is_empty() {
            let body = body
                .as_object_mut()
                .context("Gemini request body is not an object")?;
            body.insert("tools".to_string(), Self::convert_tools(&request.tools));
            body.insert(
                "toolConfig".to_string(),
                json!({
                    "functionCallingConfig": Self::convert_tool_choice(&request.tool_choice)
                }),
            );
        }
        let mut generation = json!({});
        if let Some(value) = request.temperature {
            generation
                .as_object_mut()
                .context("Gemini generation config is not an object")?
                .insert("temperature".to_string(), json!(value));
        }
        if let Some(value) = request.max_tokens {
            generation
                .as_object_mut()
                .context("Gemini generation config is not an object")?
                .insert("maxOutputTokens".to_string(), json!(value));
        }
        if let Some(value) = request.top_p {
            generation
                .as_object_mut()
                .context("Gemini generation config is not an object")?
                .insert("topP".to_string(), json!(value));
        }
        if generation != json!({}) {
            body.as_object_mut()
                .context("Gemini request body is not an object")?
                .insert("generationConfig".to_string(), generation);
        }
        debug!(url = %url, "sending native Antigravity Gemini request");
        let response = self
            .authenticated_post(&url, &runtime.provider)
            .await?
            .json(&body)
            .send()
            .await
            .context("Gemini request failed")?;
        let (status, mut bytes) = bounded_response_bytes(response).await?;
        anyhow::ensure!((200..300).contains(&status), "Gemini returned {status}");
        let result: Value =
            simd_json::to_owned_value(&mut bytes).context("invalid Gemini response")?;
        let candidates = result
            .get("candidates")
            .and_then(Value::as_array)
            .context("Gemini response has no candidates")?;
        let first = candidates
            .first()
            .context("Gemini response has no candidate")?;
        let mut text = String::new();
        let mut tool_calls = Vec::new();
        if let Some(parts) = first
            .get("content")
            .and_then(|content| content.get("parts"))
            .and_then(Value::as_array)
        {
            for part in parts {
                if let Some(fragment) = part.get("text").and_then(Value::as_str) {
                    text.push_str(fragment);
                }
                if let Some(call) = part.get("functionCall") {
                    tool_calls.push(ToolCallInfo {
                        id: format!("call_{}", Uuid::new_v4()),
                        name: call
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        arguments: call.get("args").cloned().unwrap_or_else(|| json!({})),
                    });
                }
            }
        }
        let usage = result.get("usageMetadata").map(|usage| TokenUsage {
            prompt_tokens: usage
                .get("promptTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(u64::from(u32::MAX)) as u32,
            completion_tokens: usage
                .get("candidatesTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(u64::from(u32::MAX)) as u32,
            total_tokens: usage
                .get("totalTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .min(u64::from(u32::MAX)) as u32,
        });
        self.record_response_usage(actual_model, usage.as_ref())
            .await?;
        Ok(ChatResponse {
            message: ChatMessage {
                role: "assistant".to_string(),
                content: text,
                tool_calls: (!tool_calls.is_empty()).then(|| tool_calls.clone()),
                tool_call_id: None,
            },
            model: actual_model.to_string(),
            provider: "antigravity".to_string(),
            finish_reason: first
                .get("finishReason")
                .and_then(Value::as_str)
                .map(str::to_owned),
            usage,
            tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
        })
    }

    async fn chat_stream(
        &self,
        model: &str,
        messages: Vec<ChatMessage>,
    ) -> Result<tokio::sync::mpsc::Receiver<Result<String>>> {
        let response = self.chat(model, messages).await?;
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let _ = sender.send(Ok(response.message.content)).await;
        Ok(receiver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn one_request_server(response: String) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let read = stream.read(&mut chunk).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                if let Some(header_end) =
                    request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&request[..header_end + 4]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(str::trim)
                                .and_then(|value| value.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + length {
                        break;
                    }
                }
            }
            stream.write_all(response.as_bytes()).await.unwrap();
            request
        });
        (format!("http://{address}"), task)
    }

    #[tokio::test]
    async fn google_sdk_authorized_user_refresh_is_sanitized_and_uses_native_http() {
        let body = r#"{"access_token":"PRIVATE_ACCESS","token_type":"Bearer","expires_in":3600}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let (base, request_task) = one_request_server(response).await;
        let credentials =
            google_cloud_auth::credentials::user_account::Builder::new(serde_json::json!({
                "type": "authorized_user",
                "client_id": "PRIVATE_CLIENT",
                "client_secret": "PRIVATE_SECRET",
                "refresh_token": "PRIVATE_REFRESH"
            }))
            .with_token_uri(format!("{base}/token"))
            .with_scopes(["scope-a", "scope-b"])
            .build_access_token_credentials()
            .unwrap();
        let owner = GoogleCredentialOwner::from_credentials(
            credentials,
            vec!["scope-a".to_string(), "scope-b".to_string()],
        );
        let before = owner.status().await;
        assert!(before.credential_available);
        let receipt = owner.refresh().await.unwrap();
        assert_eq!(receipt.credential_type, "application_default_credentials");
        assert!(receipt.token_available);
        let request = String::from_utf8(request_task.await.unwrap()).unwrap();
        assert!(request.starts_with("POST /token HTTP/1.1"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("content-type: application/json")
        );
        let payload: serde_json::Value = serde_json::from_str(
            request
                .split_once("\r\n\r\n")
                .expect("complete HTTP request")
                .1,
        )
        .unwrap();
        assert_eq!(payload["grant_type"], "refresh_token");
        assert_eq!(payload["refresh_token"], "PRIVATE_REFRESH");
        assert_eq!(payload["client_id"], "PRIVATE_CLIENT");
        assert_eq!(payload["client_secret"], "PRIVATE_SECRET");
        let serialized = serde_json::to_string(&owner.status().await).unwrap();
        assert!(!serialized.contains("PRIVATE_"));
        assert!(serialized.contains("scope-a"));
    }

    #[tokio::test]
    async fn gemini_request_applies_safety_and_parses_native_usage() {
        let body = r#"{"candidates":[{"content":{"parts":[{"text":"ok"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":7,"candidatesTokenCount":3,"totalTokenCount":10}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let (base, request_task) = one_request_server(response).await;
        let runtime = AntigravityRuntimeConfig {
            provider: "gemini".to_string(),
            base_url: base,
            selected_model: "fixture-model".to_string(),
            safety_settings: vec![SafetySetting {
                category: "HARM_CATEGORY_HARASSMENT".to_string(),
                threshold: "BLOCK_ONLY_HIGH".to_string(),
            }],
        };
        let provider = AntigravityProvider::with_fixture("PRIVATE_KEY", runtime);
        let result = provider
            .chat_with_request(
                "",
                ChatRequest {
                    messages: vec![
                        ChatMessage::system("system fixture"),
                        ChatMessage::user("hello"),
                    ],
                    tools: vec![ToolDefinition {
                        name: "fixture_tool".to_string(),
                        description: "fixture tool".to_string(),
                        input_schema: json!({"type":"object","properties":{}}),
                        schema_version: String::new(),
                        category: String::new(),
                        tags: Vec::new(),
                        namespace: String::new(),
                    }],
                    tool_choice: ToolChoice::Required,
                    max_tokens: Some(321),
                    temperature: Some(0.25),
                    top_p: Some(0.75),
                },
            )
            .await
            .unwrap();
        assert_eq!(result.usage.unwrap().total_tokens, 10);
        let request = String::from_utf8(request_task.await.unwrap()).unwrap();
        assert!(
            request
                .starts_with("POST /models/fixture-model:generateContent?key=PRIVATE_KEY HTTP/1.1")
        );
        let payload: serde_json::Value = serde_json::from_str(
            request
                .split_once("\r\n\r\n")
                .expect("complete Gemini HTTP request")
                .1,
        )
        .unwrap();
        assert_eq!(
            payload["safetySettings"][0]["category"],
            "HARM_CATEGORY_HARASSMENT"
        );
        assert_eq!(
            payload["systemInstruction"]["parts"][0]["text"],
            "system fixture"
        );
        assert_eq!(
            payload["tools"][0]["functionDeclarations"][0]["name"],
            "fixture_tool"
        );
        assert_eq!(
            payload["toolConfig"]["functionCallingConfig"]["mode"],
            "ANY"
        );
        assert_eq!(payload["generationConfig"]["maxOutputTokens"], 321);
        assert_eq!(payload["generationConfig"]["temperature"], 0.25);
        assert_eq!(payload["generationConfig"]["topP"], 0.75);
        assert!(!request.contains("max_threshold"));
        assert!(!request.contains("\"method\""));
    }

    #[tokio::test]
    async fn bridge_status_reads_bounded_native_models_without_fabricated_version() {
        let body = r#"{"object":"list","data":[{"id":"model-one"},{"id":"model-two"}]}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let (base, request_task) = one_request_server(response).await;
        let status = bridge_status_with_config(&AntigravityRuntimeConfig {
            provider: "openai_compatible".to_string(),
            base_url: base,
            selected_model: "model-one".to_string(),
            safety_settings: Vec::new(),
        })
        .await
        .unwrap();
        assert!(status.reachable);
        assert_eq!(status.http_status, 200);
        assert_eq!(status.model_ids, ["model-one", "model-two"]);
        let request = String::from_utf8(request_task.await.unwrap()).unwrap();
        assert!(request.starts_with("GET /v1/models HTTP/1.1"));
        let serialized = serde_json::to_value(status).unwrap();
        assert!(serialized.get("version").is_none());
    }

    #[test]
    fn local_usage_fold_never_claims_quota_or_cost() {
        let mut ledger = UsageLedgerDocument::default();
        fold_usage(
            &mut ledger,
            "model-a",
            &TokenUsage {
                prompt_tokens: 4,
                completion_tokens: 6,
                total_tokens: 10,
            },
            Utc::now(),
        );
        fold_usage(
            &mut ledger,
            "model-a",
            &TokenUsage {
                prompt_tokens: 1,
                completion_tokens: 2,
                total_tokens: 3,
            },
            Utc::now(),
        );
        let usage = &ledger.models["model-a"];
        assert_eq!(
            (
                usage.requests,
                usage.prompt_tokens,
                usage.completion_tokens,
                usage.total_tokens
            ),
            (2, 5, 8, 13)
        );
        let value = serde_json::to_value(ledger).unwrap();
        assert!(value.get("quota").is_none());
        assert!(value.get("cost").is_none());
    }

    #[test]
    fn runtime_rejects_public_plaintext_and_unknown_provider() {
        let mut config = AntigravityRuntimeConfig {
            base_url: "http://example.com".to_string(),
            ..Default::default()
        };
        assert!(validate_runtime_config(&config).is_err());
        config.base_url = DEFAULT_GEMINI_BASE.to_string();
        config.provider = "fabricated".to_string();
        assert!(validate_runtime_config(&config).is_err());
    }

    #[test]
    fn gemini_credentials_are_bound_to_the_google_endpoint() {
        let mut config = AntigravityRuntimeConfig {
            base_url: "https://attacker.example/v1beta".to_string(),
            ..Default::default()
        };
        assert!(validate_runtime_config(&config).is_err());
        config.base_url = "https://user@example.com/v1beta".to_string();
        assert!(validate_runtime_config(&config).is_err());
        config.base_url = "https://generativelanguage.googleapis.com/v1beta?redirect=1".to_string();
        assert!(validate_runtime_config(&config).is_err());
    }
}
