//! Google Gemini API Client
//!
//! ## Supported Authentication Modes
//!
//! ### 1. Service Account (Vertex AI) - Recommended for servers
//! Uses service account JSON file for JWT-based authentication.
//! Set environment variable:
//! - `GOOGLE_APPLICATION_CREDENTIALS=/path/to/service-account.json`
//! - Or uses default: `~/.config/gcloud/*.json` (service account file)
//!
//! ### 2. Application Default Credentials (OAuth refresh token)
//! Uses `~/.config/gcloud/application_default_credentials.json`
//! Set environment variable:
//! - `GOOGLE_GENAI_USE_VERTEXAI=true`
//!
//! ### 3. API Key (generativelanguage.googleapis.com)
//! Uses API key authentication with Google AI Studio endpoint.
//! Set environment variable:
//! - `GEMINI_API_KEY` or `GOOGLE_API_KEY`
//!
//! ## Endpoint URLs
//!
//! | Mode | Base URL |
//! |------|----------|
//! | Vertex AI | `https://{LOCATION}-aiplatform.googleapis.com/v1/projects/{PROJECT}/locations/{LOCATION}/publishers/google/models` |
//! | API Key | `https://generativelanguage.googleapis.com/v1beta/models` |

use anyhow::{Context, Result};
use async_trait::async_trait;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use std::time::Duration;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing::{debug, info};

use crate::provider::{
    ChatMessage, ChatRequest, ChatResponse, LlmProvider, ModelInfo, ProviderType, TokenUsage,
    ToolCallInfo, ToolChoice,
};

// =============================================================================
// API ENDPOINT CONFIGURATION
// =============================================================================

/// Gemini API endpoints
pub mod endpoints {
    /// Google AI Studio (API key mode)
    pub const GOOGLE_AI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";

    /// OAuth2 token endpoint
    pub const OAUTH_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

    /// Vertex AI endpoint template
    ///
    /// For global location, uses `aiplatform.googleapis.com` (no region prefix)
    /// For regional locations, uses `{location}-aiplatform.googleapis.com`
    pub fn vertex_ai_base_url(project: &str, location: &str) -> String {
        let hostname = if location == "global" {
            "aiplatform.googleapis.com".to_string()
        } else {
            format!("{}-aiplatform.googleapis.com", location)
        };
        format!(
            "https://{}/v1/projects/{}/locations/{}/publishers/google/models",
            hostname, project, location
        )
    }
}

// =============================================================================
// AUTHENTICATION
// =============================================================================

/// Authentication mode for Gemini API
#[derive(Debug, Clone)]
pub enum GeminiAuth {
    /// API Key authentication (query parameter)
    ApiKey(String),
    /// Service Account (JWT-based)
    ServiceAccount(ServiceAccountCredentials),
    /// OAuth with refresh token (application default credentials)
    OAuthRefreshToken(OAuthCredentials),
}

/// Service account credentials from JSON file
#[derive(Debug, Clone, Deserialize)]
pub struct ServiceAccountCredentials {
    #[serde(rename = "type")]
    pub cred_type: String,
    pub project_id: String,
    pub private_key_id: String,
    pub private_key: String,
    pub client_email: String,
    pub client_id: String,
    pub token_uri: String,
}

/// OAuth credentials from application_default_credentials.json
#[derive(Debug, Clone, Deserialize)]
pub struct OAuthCredentials {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    #[serde(default)]
    pub quota_project_id: Option<String>,
}

/// Cached access token
struct CachedToken {
    token: String,
    expires_at: Instant,
}

/// Token cache (global, thread-safe)
static TOKEN_CACHE: std::sync::OnceLock<RwLock<Option<CachedToken>>> = std::sync::OnceLock::new();

fn get_token_cache() -> &'static RwLock<Option<CachedToken>> {
    TOKEN_CACHE.get_or_init(|| RwLock::new(None))
}

/// OAuth token response
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    #[serde(default)]
    _token_type: Option<String>,
}

/// JWT Claims for service account (used by jsonwebtoken)
#[derive(Debug, Serialize)]
struct JwtClaims {
    iss: String,
    sub: String,
    aud: String,
    iat: u64,
    exp: u64,
    scope: String,
}

/// Load service account credentials from file
fn load_service_account_credentials() -> Result<ServiceAccountCredentials> {
    // First check GOOGLE_APPLICATION_CREDENTIALS
    if let Ok(path) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
        let contents =
            std::fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path))?;
        let mut contents_mut = contents;
        let creds: ServiceAccountCredentials = unsafe { simd_json::from_str(&mut contents_mut) }
            .context("Failed to parse service account JSON")?;
        return Ok(creds);
    }

    // Look for service account JSON in gcloud config
    let home = std::env::var("HOME").context("HOME not set")?;
    let gcloud_dir = format!("{}/.config/gcloud", home);

    // Find any service account JSON file
    for entry in
        std::fs::read_dir(&gcloud_dir).with_context(|| format!("Failed to read {}", gcloud_dir))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            if let Ok(contents) = std::fs::read_to_string(&path) {
                let mut contents_mut = contents;
                if let Ok(creds) =
                    unsafe { simd_json::from_str::<ServiceAccountCredentials>(&mut contents_mut) }
                {
                    if creds.cred_type == "service_account" {
                        info!("Found service account: {}", path.display());
                        return Ok(creds);
                    }
                }
            }
        }
    }

    Err(anyhow::anyhow!("No service account credentials found"))
}

/// Load OAuth credentials from application_default_credentials.json
fn load_oauth_credentials() -> Result<OAuthCredentials> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let creds_path = format!(
        "{}/.config/gcloud/application_default_credentials.json",
        home
    );

    let contents = std::fs::read_to_string(&creds_path)
        .with_context(|| format!("Failed to read {}", creds_path))?;

    let mut contents_mut = contents;
    let creds: OAuthCredentials = unsafe { simd_json::from_str(&mut contents_mut) }
        .context("Failed to parse application_default_credentials.json")?;

    Ok(creds)
}

/// Create JWT for service account authentication
fn create_service_account_jwt(creds: &ServiceAccountCredentials) -> Result<String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("Time error")?
        .as_secs();

    // JWT Claims for Google OAuth
    let claims = JwtClaims {
        iss: creds.client_email.clone(),
        sub: creds.client_email.clone(),
        aud: creds.token_uri.clone(),
        iat: now,
        exp: now + 3600, // 1 hour
        scope: "https://www.googleapis.com/auth/cloud-platform".to_string(),
    };

    // Create header with key ID
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(creds.private_key_id.clone());

    // Create encoding key from PEM
    let encoding_key = EncodingKey::from_rsa_pem(creds.private_key.as_bytes())
        .context("Failed to parse private key")?;

    // Encode and sign JWT
    let jwt = encode(&header, &claims, &encoding_key).context("Failed to create JWT")?;

    Ok(jwt)
}

/// Get access token for service account using JWT
async fn get_service_account_token(creds: &ServiceAccountCredentials) -> Result<String> {
    // Check cache first
    {
        let cache = get_token_cache().read().unwrap();
        if let Some(ref cached) = *cache {
            if cached.expires_at > Instant::now() + Duration::from_secs(60) {
                debug!("Using cached service account token");
                return Ok(cached.token.clone());
            }
        }
    }

    info!("Getting service account access token...");

    let jwt = create_service_account_jwt(creds)?;

    let client = Client::new();
    let response = client
        .post(&creds.token_uri)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", &jwt),
        ])
        .send()
        .await
        .context("Failed to request access token")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Token request failed {}: {}", status, body));
    }

    let token_resp: TokenResponse = response
        .json()
        .await
        .context("Failed to parse token response")?;

    // Cache the token
    {
        let mut cache = get_token_cache().write().unwrap();
        *cache = Some(CachedToken {
            token: token_resp.access_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(token_resp.expires_in),
        });
    }

    info!(
        "✅ Service account token obtained (expires in {}s)",
        token_resp.expires_in
    );
    Ok(token_resp.access_token)
}

/// Get access token using OAuth refresh token
async fn get_oauth_refresh_token(creds: &OAuthCredentials) -> Result<String> {
    // Check cache first
    {
        let cache = get_token_cache().read().unwrap();
        if let Some(ref cached) = *cache {
            if cached.expires_at > Instant::now() + Duration::from_secs(60) {
                debug!("Using cached OAuth token");
                return Ok(cached.token.clone());
            }
        }
    }

    info!("Refreshing OAuth access token...");

    let client = Client::new();
    let response = client
        .post(endpoints::OAUTH_TOKEN_URL)
        .form(&[
            ("client_id", creds.client_id.as_str()),
            ("client_secret", creds.client_secret.as_str()),
            ("refresh_token", creds.refresh_token.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .context("Failed to request OAuth token")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "OAuth token request failed {}: {}",
            status,
            body
        ));
    }

    let token_resp: TokenResponse = response
        .json()
        .await
        .context("Failed to parse OAuth token response")?;

    // Cache the token
    {
        let mut cache = get_token_cache().write().unwrap();
        *cache = Some(CachedToken {
            token: token_resp.access_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(token_resp.expires_in),
        });
    }

    info!(
        "✅ OAuth token refreshed (expires in {}s)",
        token_resp.expires_in
    );
    Ok(token_resp.access_token)
}

// =============================================================================
// DATA STRUCTURES
// =============================================================================

/// Gemini model category
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeminiCategory {
    TextOut,
    MultiModalGenerative,
    LiveApi,
    Other,
}

impl std::fmt::Display for GeminiCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GeminiCategory::TextOut => write!(f, "Text-out models"),
            GeminiCategory::MultiModalGenerative => write!(f, "Multi-modal generative"),
            GeminiCategory::LiveApi => write!(f, "Live API"),
            GeminiCategory::Other => write!(f, "Other models"),
        }
    }
}

/// Gemini model with rate limits
#[derive(Debug, Clone)]
pub struct GeminiModel {
    pub id: String,
    pub category: GeminiCategory,
    pub rpm: u32,
    pub tpm: u64,
    pub rpd: u32,
}

impl GeminiModel {
    fn new(id: &str, category: GeminiCategory, rpm: u32, tpm: u64, rpd: u32) -> Self {
        Self {
            id: id.to_string(),
            category,
            rpm,
            tpm,
            rpd,
        }
    }
}

/// Static list of Gemini models
fn get_gemini_models() -> Vec<GeminiModel> {
    use GeminiCategory::*;

    vec![
        // Auto-routing models
        GeminiModel::new("gemini-auto", TextOut, 2_000, 4_000_000, 0),
        // Current stable Gemini aliases
        GeminiModel::new("gemini-2.5-pro", TextOut, 360, 4_000_000, 0),
        GeminiModel::new("gemini-2.5-flash", TextOut, 2_000, 4_000_000, 0),
        GeminiModel::new("gemini-2.5-flash-lite", TextOut, 4_000, 4_000_000, 0),
        GeminiModel::new("gemini-2.0-flash", TextOut, 2_000, 4_000_000, 0),
        GeminiModel::new("gemini-2.0-flash-lite", TextOut, 4_000, 4_000_000, 0),
        // Multi-modal & Images
        GeminiModel::new("gemini-2.5-flash-image", MultiModalGenerative, 100, 0, 0),
        GeminiModel::new("imagen-3.0-generate-001", MultiModalGenerative, 10, 0, 70),
        // Live API
        GeminiModel::new(
            "gemini-live-2.5-flash-native-audio",
            LiveApi,
            0,
            4_000_000,
            0,
        ),
        // Embeddings
        GeminiModel::new("gemini-embedding-001", Other, 0, 0, 0),
    ]
}

/// Gemini API request
/// Gemini API request with optional tools
#[derive(Debug, Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "systemInstruction", skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiContent>,
    #[serde(rename = "generationConfig", skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiTool>>,
    #[serde(rename = "toolConfig", skip_serializing_if = "Option::is_none")]
    tool_config: Option<GeminiToolConfig>,
}

/// Gemini tool definition
#[derive(Debug, Serialize)]
struct GeminiTool {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<GeminiFunctionDeclaration>,
}

/// Gemini function declaration
#[derive(Debug, Serialize)]
struct GeminiFunctionDeclaration {
    name: String,
    description: String,
    parameters: simd_json::OwnedValue,
}

/// Gemini tool configuration
#[derive(Debug, Serialize)]
struct GeminiToolConfig {
    #[serde(rename = "functionCallingConfig")]
    function_calling_config: FunctionCallingConfig,
}

#[derive(Debug, Serialize)]
struct FunctionCallingConfig {
    mode: String, // "AUTO", "ANY", "NONE"
}

#[derive(Debug, Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Debug, Serialize)]
struct GenerationConfig {
    temperature: Option<f32>,
    #[serde(rename = "topP")]
    top_p: Option<f32>,
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: Option<u32>,
    #[serde(rename = "routingConfig", skip_serializing_if = "Option::is_none")]
    routing_config: Option<RoutingConfig>,
}

#[derive(Debug, Serialize)]
struct RoutingConfig {
    #[serde(rename = "autoMode", skip_serializing_if = "Option::is_none")]
    auto_mode: Option<AutoRoutingMode>,
}

#[derive(Debug, Serialize)]
struct AutoRoutingMode {
    #[serde(rename = "modelRoutingPreference")]
    model_routing_preference: String, // "BALANCED", "PRIORITIZE_QUALITY", or "PRIORITIZE_COST"
}

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<UsageMetadata>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiContentResponse,
    #[serde(rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiContentResponse {
    parts: Vec<GeminiPartResponse>,
}

#[derive(Debug, Deserialize)]
struct GeminiPartResponse {
    text: Option<String>,
    #[serde(rename = "functionCall")]
    function_call: Option<GeminiFunctionCall>,
}

#[derive(Debug, Deserialize)]
struct GeminiFunctionCall {
    name: String,
    args: simd_json::OwnedValue,
}

#[derive(Debug, Deserialize)]
struct UsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: Option<u32>,
    #[serde(rename = "candidatesTokenCount")]
    candidates_token_count: Option<u32>,
    #[serde(rename = "totalTokenCount")]
    total_token_count: Option<u32>,
}

// =============================================================================
// CLIENT IMPLEMENTATION
// =============================================================================

/// Google Gemini Client
///
/// Supports Service Account, OAuth, and API Key authentication modes.
pub struct GeminiClient {
    client: Client,
    auth: GeminiAuth,
    /// Base API URL
    api_url: String,
    /// Whether using Vertex AI mode
    use_vertex_ai: bool,
    /// Project ID (for Vertex AI)
    _project: Option<String>,
    /// Location (for Vertex AI)
    _location: Option<String>,
    models: Vec<GeminiModel>,
}

impl GeminiClient {
    /// Create a new Gemini client with API key
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            auth: GeminiAuth::ApiKey(api_key.into()),
            api_url: endpoints::GOOGLE_AI_BASE_URL.to_string(),
            use_vertex_ai: false,
            _project: None,
            _location: None,
            models: get_gemini_models(),
        }
    }

    /// Automatically select the best model for the task
    /// Returns the model ID to use
    pub fn select_auto_model(&self, _messages: &[ChatMessage]) -> String {
        // For Vertex AI, use gemini-auto for automatic routing
        // This lets Google's infrastructure choose the best model
        if self.use_vertex_ai {
            "gemini-auto".to_string()
        } else {
            // For API key mode, default to the current Flash stable alias.
            "gemini-2.5-flash".to_string()
        }
    }

    /// Create a new Gemini client for Vertex AI with service account
    pub fn new_vertex_ai_service_account(
        creds: ServiceAccountCredentials,
        location: impl Into<String>,
    ) -> Self {
        let project = creds.project_id.clone();
        let location = location.into();
        let api_url = endpoints::vertex_ai_base_url(&project, &location);

        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            auth: GeminiAuth::ServiceAccount(creds),
            api_url,
            use_vertex_ai: true,
            _project: Some(project),
            _location: Some(location),
            models: get_gemini_models(),
        }
    }

    /// Create a new Gemini client for Vertex AI with OAuth
    pub fn new_vertex_ai_oauth(
        creds: OAuthCredentials,
        project: impl Into<String>,
        location: impl Into<String>,
    ) -> Self {
        let project = project.into();
        let location = location.into();
        let api_url = endpoints::vertex_ai_base_url(&project, &location);

        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            auth: GeminiAuth::OAuthRefreshToken(creds),
            api_url,
            use_vertex_ai: true,
            _project: Some(project),
            _location: Some(location),
            models: get_gemini_models(),
        }
    }

    /// Create from environment variables
    ///
    /// Priority:
    /// 1. Service account (GOOGLE_APPLICATION_CREDENTIALS or ~/.config/gcloud/*.json)
    /// 2. OAuth refresh token (GOOGLE_GENAI_USE_VERTEXAI=true)
    /// 3. API key (GEMINI_API_KEY or GOOGLE_API_KEY)
    pub fn from_env() -> Result<Self> {
        let use_vertex = std::env::var("GOOGLE_GENAI_USE_VERTEXAI")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(false);

        let location =
            std::env::var("GOOGLE_CLOUD_LOCATION").unwrap_or_else(|_| "us-central1".to_string());

        // Only try Vertex AI if explicitly enabled
        if use_vertex {
            // Try service account first
            if let Ok(sa_creds) = load_service_account_credentials() {
                info!(
                    "✅ Vertex AI mode (service account): project={}, location={}",
                    sa_creds.project_id, location
                );
                return Ok(Self::new_vertex_ai_service_account(sa_creds, location));
            }

            // Try OAuth refresh token if Vertex AI mode enabled
            if let Ok(oauth_creds) = load_oauth_credentials() {
                let project = std::env::var("GOOGLE_CLOUD_PROJECT")
                    .or_else(|_| {
                        oauth_creds
                            .quota_project_id
                            .clone()
                            .ok_or(std::env::VarError::NotPresent)
                    })
                    .context("GOOGLE_CLOUD_PROJECT not set for OAuth Vertex AI mode")?;

                info!(
                    "✅ Vertex AI mode (OAuth): project={}, location={}",
                    project, location
                );
                return Ok(Self::new_vertex_ai_oauth(oauth_creds, project, location));
            }
        }

        // Fall back to API key (default mode)
        let api_key = std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("GOOGLE_API_KEY"))
            .context("No Gemini credentials found. Set GEMINI_API_KEY or GOOGLE_API_KEY for API key mode")?;

        info!("✅ API Key mode (generativelanguage.googleapis.com)");
        Ok(Self::new(api_key))
    }

    /// Create with custom endpoint (API key mode)
    pub fn with_endpoint(api_key: impl Into<String>, endpoint: impl Into<String>) -> Self {
        let mut client = Self::new(api_key);
        client.api_url = endpoint.into();
        client
    }

    /// Get the current API URL
    pub fn api_url(&self) -> &str {
        &self.api_url
    }

    /// Check if using Vertex AI mode
    pub fn is_vertex_ai(&self) -> bool {
        self.use_vertex_ai
    }

    /// Build the full URL for a model endpoint
    fn build_url(&self, model: &str, action: &str) -> Result<String> {
        match &self.auth {
            GeminiAuth::ApiKey(key) => Ok(format!(
                "{}/models/{}:{}?key={}",
                self.api_url, model, action, key
            )),
            GeminiAuth::ServiceAccount(_) | GeminiAuth::OAuthRefreshToken(_) => {
                Ok(format!("{}/{}:{}", self.api_url, model, action))
            }
        }
    }

    /// Get authorization header
    async fn get_auth_header(&self) -> Result<Option<String>> {
        match &self.auth {
            GeminiAuth::ApiKey(_) => Ok(None),
            GeminiAuth::ServiceAccount(creds) => {
                let token = get_service_account_token(creds).await?;
                Ok(Some(format!("Bearer {}", token)))
            }
            GeminiAuth::OAuthRefreshToken(creds) => {
                let token = get_oauth_refresh_token(creds).await?;
                Ok(Some(format!("Bearer {}", token)))
            }
        }
    }

    fn to_model_info(&self, model: &GeminiModel) -> ModelInfo {
        let description = format!(
            "{} - RPM: {}, TPM: {}{}",
            model.category,
            if model.rpm == 0 {
                "Unlimited".to_string()
            } else {
                model.rpm.to_string()
            },
            if model.tpm >= 1_000_000 {
                format!("{}M", model.tpm / 1_000_000)
            } else if model.tpm >= 1_000 {
                format!("{}K", model.tpm / 1_000)
            } else if model.tpm == 0 {
                "N/A".to_string()
            } else {
                model.tpm.to_string()
            },
            if model.rpd == 0 {
                ", RPD: Unlimited".to_string()
            } else {
                format!(", RPD: {}", model.rpd)
            }
        );

        ModelInfo {
            id: model.id.clone(),
            name: model.id.clone(),
            description: Some(description),
            parameters: None,
            available: true,
            tags: vec![model.category.to_string()],
            downloads: None,
            updated_at: None,
        }
    }
}

#[async_trait]
impl LlmProvider for GeminiClient {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Gemini
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        info!("Gemini models (static list)");
        info!(
            "  Mode: {}",
            if self.use_vertex_ai {
                "Vertex AI"
            } else {
                "API Key"
            }
        );
        info!("  Endpoint: {}", self.api_url);
        Ok(self.models.iter().map(|m| self.to_model_info(m)).collect())
    }

    async fn search_models(&self, query: &str, limit: usize) -> Result<Vec<ModelInfo>> {
        let query_lower = query.to_lowercase();
        Ok(self
            .models
            .iter()
            .filter(|m| m.id.to_lowercase().contains(&query_lower))
            .take(limit)
            .map(|m| self.to_model_info(m))
            .collect())
    }

    async fn get_model(&self, model_id: &str) -> Result<Option<ModelInfo>> {
        Ok(self
            .models
            .iter()
            .find(|m| m.id == model_id)
            .map(|m| self.to_model_info(m)))
    }

    async fn is_model_available(&self, model_id: &str) -> Result<bool> {
        Ok(self.models.iter().any(|m| m.id == model_id))
    }

    async fn chat(&self, model: &str, messages: Vec<ChatMessage>) -> Result<ChatResponse> {
        // Support "auto" model selection
        let actual_model = if model == "auto" || model == "gemini-auto" {
            let selected = self.select_auto_model(&messages);
            info!("Auto model selection: {} -> {}", model, selected);
            selected
        } else {
            model.to_string()
        };

        let url = self.build_url(&actual_model, "generateContent")?;

        info!(
            "Gemini chat: model={}, mode={}",
            actual_model,
            if self.use_vertex_ai {
                "Vertex AI"
            } else {
                "API Key"
            }
        );

        // Extract system message if present
        let system_instruction =
            messages
                .iter()
                .find(|m| m.role == "system")
                .map(|m| GeminiContent {
                    role: "user".to_string(),
                    parts: vec![GeminiPart {
                        text: m.content.clone(),
                    }],
                });

        // Build contents excluding system messages
        let contents: Vec<GeminiContent> = messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| GeminiContent {
                role: if m.role == "assistant" {
                    "model".to_string()
                } else {
                    "user".to_string()
                },
                parts: vec![GeminiPart {
                    text: m.content.clone(),
                }],
            })
            .collect();

        // Enable auto-routing for Gemini 3 models (only in Vertex AI mode)
        // Note: Auto-routing is not supported in API key mode
        let use_auto_routing = actual_model.starts_with("gemini-3") && self.use_vertex_ai;
        let routing_config = if use_auto_routing {
            Some(RoutingConfig {
                auto_mode: Some(AutoRoutingMode {
                    model_routing_preference: "BALANCED".to_string(),
                }),
            })
        } else {
            None
        };

        if use_auto_routing {
            info!("🔀 Auto-routing enabled (BALANCED mode) - Vertex AI only");
        }

        let gemini_req = GeminiRequest {
            contents,
            system_instruction,
            generation_config: Some(GenerationConfig {
                temperature: Some(0.7),
                top_p: Some(0.95),
                max_output_tokens: Some(8192),
                routing_config,
            }),
            tools: None,
            tool_config: None,
        };

        debug!(
            "Gemini request to: {}",
            url.split('?').next().unwrap_or(&url)
        );

        // Retry with exponential backoff for rate limiting (429) errors
        let max_retries = 5;
        let mut retry_count = 0;

        loop {
            // Build request with appropriate auth (regenerate token for each retry)
            let mut req = self.client.post(&url).json(&gemini_req);

            if let Some(auth_header) = self.get_auth_header().await? {
                req = req.header("Authorization", auth_header);
            }

            let response = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("Gemini HTTP request failed: {}", e);
                    return Err(anyhow::anyhow!("Failed to send Gemini request: {}", e));
                }
            };

            let status = response.status();

            // Check if we got a 429 (rate limit) error
            if status.as_u16() == 429 {
                if retry_count >= max_retries {
                    let body = response.text().await.unwrap_or_default();
                    tracing::error!(
                        "Gemini API rate limit exceeded after {} retries: {}",
                        max_retries,
                        body
                    );
                    return Err(anyhow::anyhow!(
                        "Gemini API rate limit exceeded after {} retries. Please try again later.",
                        max_retries
                    ));
                }

                // Exponential backoff: 1s, 2s, 4s, 8s, 16s
                let delay_secs = 1u64 << retry_count;
                tracing::warn!(
                    "Gemini API rate limit (429), retrying in {}s (attempt {}/{})",
                    delay_secs,
                    retry_count + 1,
                    max_retries
                );
                tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                retry_count += 1;
                continue;
            }

            // For non-429 errors, fail immediately
            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                tracing::error!("Gemini API error {}: {}", status, body);
                return Err(anyhow::anyhow!("Gemini API error {}: {}", status, body));
            }

            // Get raw response text first for debugging
            let raw_body = response
                .text()
                .await
                .context("Failed to read Gemini response body")?;

            let mut raw_body_mut = raw_body;
            let result: GeminiResponse = match unsafe { simd_json::from_str(&mut raw_body_mut) } {
                Ok(r) => r,
                Err(e) => {
                    let preview = if raw_body_mut.len() > 1000 {
                        format!("{}...[truncated]", &raw_body_mut[..1000])
                    } else {
                        raw_body_mut.clone()
                    };
                    tracing::error!("Failed to parse Gemini response: {}", e);
                    tracing::error!("Raw response: {}", preview);
                    return Err(anyhow::anyhow!(
                        "Failed to parse Gemini response: {}. Raw: {}",
                        e,
                        preview
                    ));
                }
            };

            let text = result
                .candidates
                .first()
                .and_then(|c| c.content.parts.first())
                .and_then(|p| p.text.clone())
                .unwrap_or_default();

            let finish_reason = result
                .candidates
                .first()
                .and_then(|c| c.finish_reason.clone());

            let usage = result.usage_metadata.map(|u| TokenUsage {
                prompt_tokens: u.prompt_token_count.unwrap_or(0),
                completion_tokens: u.candidates_token_count.unwrap_or(0),
                total_tokens: u.total_token_count.unwrap_or(0),
            });

            return Ok(ChatResponse {
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: text,
                    tool_calls: None,
                    tool_call_id: None,
                },
                model: model.to_string(),
                provider: "gemini".to_string(),
                finish_reason,
                usage,
                tool_calls: None,
            });
        }
    }

    async fn chat_with_request(&self, model: &str, request: ChatRequest) -> Result<ChatResponse> {
        // Support "auto" model selection
        let actual_model = if model == "auto" || model == "gemini-auto" {
            let selected = self.select_auto_model(&request.messages);
            info!("Auto model selection: {} -> {}", model, selected);
            selected
        } else {
            model.to_string()
        };

        let url = self.build_url(&actual_model, "generateContent")?;

        info!(
            "Gemini chat_with_request: model={}, tools={}, mode={}",
            actual_model,
            request.tools.len(),
            if self.use_vertex_ai {
                "Vertex AI"
            } else {
                "API Key"
            }
        );

        // Extract system message if present
        let system_instruction = request
            .messages
            .iter()
            .find(|m| m.role == "system")
            .map(|m| GeminiContent {
                role: "user".to_string(),
                parts: vec![GeminiPart {
                    text: m.content.clone(),
                }],
            });

        // Build contents excluding system messages
        let contents: Vec<GeminiContent> = request
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| GeminiContent {
                role: if m.role == "assistant" {
                    "model".to_string()
                } else {
                    "user".to_string()
                },
                parts: vec![GeminiPart {
                    text: m.content.clone(),
                }],
            })
            .collect();

        // Convert tools to Gemini format
        let tools = if request.tools.is_empty() {
            None
        } else {
            let function_declarations: Vec<GeminiFunctionDeclaration> = request
                .tools
                .iter()
                .map(|t| GeminiFunctionDeclaration {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.input_schema.clone(),
                })
                .collect();

            Some(vec![GeminiTool {
                function_declarations,
            }])
        };

        // Convert tool_choice to Gemini format
        let tool_config = if !request.tools.is_empty() {
            let mode = match request.tool_choice {
                ToolChoice::Auto => "AUTO",
                ToolChoice::None => "NONE",
                ToolChoice::Required => "ANY",
                ToolChoice::Tool(_) => "ANY", // Gemini doesn't support specific tool selection
            };
            Some(GeminiToolConfig {
                function_calling_config: FunctionCallingConfig {
                    mode: mode.to_string(),
                },
            })
        } else {
            None
        };

        // Enable auto-routing for Gemini 3 models (only in Vertex AI mode)
        // Note: Auto-routing is not supported in API key mode
        let use_auto_routing = actual_model.starts_with("gemini-3") && self.use_vertex_ai;
        let routing_config = if use_auto_routing {
            Some(RoutingConfig {
                auto_mode: Some(AutoRoutingMode {
                    model_routing_preference: "BALANCED".to_string(),
                }),
            })
        } else {
            None
        };

        if use_auto_routing {
            info!("🔀 Auto-routing enabled (BALANCED mode) - Vertex AI only");
        }

        let gemini_request = GeminiRequest {
            contents,
            system_instruction,
            generation_config: Some(GenerationConfig {
                temperature: request.temperature,
                top_p: request.top_p,
                max_output_tokens: request.max_tokens,
                routing_config,
            }),
            tools,
            tool_config,
        };

        debug!(
            "Gemini request to: {}",
            url.split('?').next().unwrap_or(&url)
        );

        // Retry with exponential backoff for rate limiting (429) errors
        let max_retries = 5;
        let mut retry_count = 0;

        loop {
            // Build request with appropriate auth
            let mut req = self.client.post(&url).json(&gemini_request);

            if let Some(auth_header) = self.get_auth_header().await? {
                req = req.header("Authorization", auth_header);
            }

            let response = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("Gemini HTTP request failed: {}", e);
                    return Err(anyhow::anyhow!("Failed to send Gemini request: {}", e));
                }
            };

            let status = response.status();

            // Handle 429 rate limit
            if status.as_u16() == 429 {
                if retry_count >= max_retries {
                    let body = response.text().await.unwrap_or_default();
                    tracing::error!(
                        "Gemini API rate limit exceeded after {} retries: {}",
                        max_retries,
                        body
                    );
                    return Err(anyhow::anyhow!("Gemini API rate limit exceeded"));
                }

                let delay_secs = 1u64 << retry_count;
                tracing::warn!("Gemini API rate limit (429), retrying in {}s", delay_secs);
                tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                retry_count += 1;
                continue;
            }

            if !status.is_success() {
                let body = response.text().await.unwrap_or_default();
                tracing::error!("Gemini API error {}: {}", status, body);
                return Err(anyhow::anyhow!("Gemini API error {}: {}", status, body));
            }

            // Get raw response text first for debugging
            let raw_body = response
                .text()
                .await
                .context("Failed to read Gemini response body")?;

            // Parse response
            let mut raw_body_mut = raw_body;
            let result: GeminiResponse = match unsafe { simd_json::from_str(&mut raw_body_mut) } {
                Ok(r) => r,
                Err(e) => {
                    // Log the raw response for debugging
                    let preview = if raw_body_mut.len() > 1000 {
                        format!("{}...[truncated]", &raw_body_mut[..1000])
                    } else {
                        raw_body_mut.clone()
                    };
                    tracing::error!("Failed to parse Gemini response: {}", e);
                    tracing::error!("Raw response: {}", preview);
                    return Err(anyhow::anyhow!(
                        "Failed to parse Gemini response: {}. Raw: {}",
                        e,
                        preview
                    ));
                }
            };

            // Extract text and function calls
            let mut text = String::new();
            let mut tool_calls: Vec<ToolCallInfo> = Vec::new();

            if let Some(candidate) = result.candidates.first() {
                for part in &candidate.content.parts {
                    if let Some(ref t) = part.text {
                        text.push_str(t);
                    }
                    if let Some(ref fc) = part.function_call {
                        tool_calls.push(ToolCallInfo {
                            id: format!("call_{}", tool_calls.len()),
                            name: fc.name.clone(),
                            arguments: fc.args.clone(),
                        });
                    }
                }
            }

            let finish_reason = result
                .candidates
                .first()
                .and_then(|c| c.finish_reason.clone());

            let usage = result.usage_metadata.map(|u| TokenUsage {
                prompt_tokens: u.prompt_token_count.unwrap_or(0),
                completion_tokens: u.candidates_token_count.unwrap_or(0),
                total_tokens: u.total_token_count.unwrap_or(0),
            });

            // Log tool calls if any
            if !tool_calls.is_empty() {
                info!("Gemini returned {} tool calls", tool_calls.len());
                for tc in &tool_calls {
                    debug!("  Tool call: {}({})", tc.name, tc.arguments);
                }
            }

            return Ok(ChatResponse {
                message: ChatMessage {
                    role: "assistant".to_string(),
                    content: text,
                    tool_calls: None, // We put them in the response.tool_calls field
                    tool_call_id: None,
                },
                model: model.to_string(),
                provider: "gemini".to_string(),
                finish_reason,
                usage,
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
            });
        }
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
