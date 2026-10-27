//! Antigravity SDK provider plugin.
//!
//! Publishes the proven native credential, safety policy, provider route and
//! local response-usage surface through PluginSchema. Gemini model catalog
//! ownership lives on the delegated `llm_plugin` (default
//! `chat_router`) — this plugin only holds `llm_plugin` /
//! `provider_route` / `selected_model` references.

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use simd_json::OwnedValue as Value;
use std::collections::HashMap;

/// Vertex AI authentication configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.vertex-auth.schema@v1"))]
pub struct VertexAuth {
    /// Whether Vertex AI is enabled.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.vertex-auth.enabled@v1"))]
    pub enabled: bool,
    /// Whether to use the Vertex AI backend (true) or the Gemini API (false).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.vertex-auth.use-vertexai@v1"))]
    pub use_vertexai: String,
    /// Google Cloud project ID.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.antigravity.vertex-auth.project@v1"))]
    pub project: String,
    /// Google Cloud location/region.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.antigravity.vertex-auth.location@v1"))]
    pub location: String,
    /// Vertex AI API endpoint URL.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.vertex-auth.api-endpoint@v1"))]
    pub api_endpoint: String,
    /// Credentials type used for Vertex AI.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.vertex-auth.credentials-type@v1"))]
    pub credentials_type: String,
    /// Service account email for Vertex AI.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.antigravity.vertex-auth.service-account@v1"))]
    pub service_account_email: String,
    /// Quota project for Vertex AI billing.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.antigravity.vertex-auth.quota-project@v1"))]
    pub quota_project: String,
}

/// A single API key configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.api-key.schema@v1"))]
pub struct ApiKey {
    /// Whether the key is configured.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.api-key.configured@v1"))]
    pub configured: bool,
    /// Environment variable holding the key.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.api-key.env-var@v1"))]
    pub env_var: String,
    /// Masked value displayed in the UI.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.antigravity.api-key.masked-value@v1"))]
    pub masked_value: String,
    /// Restriction class for the key.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.api-key.restriction@v1"))]
    pub restriction: String,
}

/// Map of API keys by service name.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.api-keys.schema@v1"))]
pub struct ApiKeys {
    /// Generative Language API key.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.api-keys.generative-language@v1"))]
    pub generative_language: ApiKey,
    /// Vertex AI API key.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.api-keys.vertex-ai@v1"))]
    pub vertex_ai: ApiKey,
}

/// OAuth bridge status.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.oauth-bridge.schema@v1"))]
pub struct OauthBridge {
    /// Bridge URL.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.oauth-bridge.url@v1"))]
    pub url: String,
    /// Bridge status.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.oauth-bridge.status@v1"))]
    pub status: String,
    /// Bridge version.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.oauth-bridge.version@v1"))]
    pub version: String,
}

/// Antigravity authentication state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.auth.schema@v1"))]
pub struct Auth {
    /// Authentication method.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.auth.method@v1"))]
    pub method: String,
    /// Identity provider.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.antigravity.auth.provider@v1"))]
    pub provider: String,
    /// Token source (e.g. gcloud).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "src.service.antigravity.auth.token-source@v1"))]
    pub token_source: String,
    /// Path to the token file.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.auth.token-file@v1"))]
    pub token_file: String,
    /// Whether the current token is valid.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.auth.token-valid@v1"))]
    pub token_valid: bool,
    /// Token expiry timestamp, null when not known.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.auth.token-expiry@v1"))]
    pub token_expiry: Option<JsonValue>,
    /// OAuth scopes granted.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.antigravity.auth.scopes@v1"))]
    pub scopes: Vec<String>,
    /// Vertex AI authentication details.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.auth.vertex-ai@v1"))]
    pub vertex_ai: VertexAuth,
    /// Configured API keys.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.auth.api-keys@v1"))]
    pub api_keys: ApiKeys,
    /// OAuth bridge endpoint.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.auth.oauth-bridge@v1"))]
    pub oauth_bridge: OauthBridge,
}

/// Google Cloud project metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.project.schema@v1"))]
pub struct Project {
    /// Google Cloud project ID.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.antigravity.project.id@v1"))]
    pub project_id: String,
    /// Numeric project identifier.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.antigravity.project.number@v1"))]
    pub project_number: String,
    /// Display name of the project.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.antigravity.project.name@v1"))]
    pub name: String,
    /// Default location/region.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.antigravity.project.location@v1"))]
    pub location: String,
    /// Region (same as location for Antigravity).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.antigravity.project.region@v1"))]
    pub region: String,
    /// Enabled Google Cloud APIs.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.project.api-enabled@v1"))]
    pub api_enabled: Vec<String>,
    /// Project labels.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.project.labels@v1"))]
    pub labels: HashMap<String, String>,
}

/// Thinking mode configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.thinking-config.schema@v1"))]
pub struct ThinkingConfig {
    /// Whether to include thoughts in the output.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.thinking-config.include-thoughts@v1"))]
    pub include_thoughts: bool,
    /// Thinking budget in tokens.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.thinking-config.budget@v1"))]
    pub thinking_budget: i64,
}

/// Default generation settings for a specific use case.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.generation-defaults.schema@v1"))]
pub struct GenerationDefaultsEntry {
    /// Sampling temperature.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-defaults.temperature@v1"))]
    pub temperature: f64,
    /// Nucleus sampling threshold.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-defaults.top-p@v1"))]
    pub top_p: f64,
    /// Top-k sampling threshold.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-defaults.top-k@v1"))]
    pub top_k: i64,
    /// Maximum output tokens.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-defaults.max-output-tokens@v1"))]
    pub max_output_tokens: i64,
    /// Optional thinking override.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.antigravity.generation-defaults.thinking-config@v1"))]
    pub thinking_config: Option<ThinkingConfig>,
}

/// Default generation settings by use case.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.generation-defaults-map.schema@v1"))]
pub struct GenerationDefaults {
    /// General chat defaults.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-defaults.chat@v1"))]
    pub chat: GenerationDefaultsEntry,
    /// Code generation defaults.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-defaults.code@v1"))]
    pub code: GenerationDefaultsEntry,
    /// Reasoning defaults.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-defaults.reasoning@v1"))]
    pub reasoning: GenerationDefaultsEntry,
    /// Creative generation defaults.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-defaults.creative@v1"))]
    pub creative: GenerationDefaultsEntry,
    /// Precise generation defaults.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-defaults.precise@v1"))]
    pub precise: GenerationDefaultsEntry,
}

/// Generation configuration controls.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.generation-config.schema@v1"))]
pub struct GenerationConfig {
    /// Sampling temperature.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-config.temperature@v1"))]
    pub temperature: f64,
    /// Nucleus sampling threshold.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-config.top-p@v1"))]
    pub top_p: f64,
    /// Top-k sampling threshold.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-config.top-k@v1"))]
    pub top_k: i64,
    /// Maximum output tokens.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-config.max-output-tokens@v1"))]
    pub max_output_tokens: i64,
    /// Number of candidate responses to generate.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.generation-config.candidate-count@v1"))]
    pub candidate_count: i64,
    /// Stop sequences.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-config.stop-sequences@v1"))]
    pub stop_sequences: Vec<String>,
    /// Response modalities.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.antigravity.generation-config.response-modalities@v1"))]
    pub response_modalities: Vec<String>,
    /// Presence penalty.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-config.presence-penalty@v1"))]
    pub presence_penalty: f64,
    /// Frequency penalty.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-config.frequency-penalty@v1"))]
    pub frequency_penalty: f64,
    /// Deterministic seed, null for random.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generation-config.seed@v1"))]
    pub seed: Option<JsonValue>,
    /// Thinking mode configuration.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.antigravity.generation-config.thinking-config@v1"))]
    pub thinking_config: ThinkingConfig,
    /// Per-use-case defaults.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.antigravity.generation-config.defaults@v1"))]
    pub defaults: GenerationDefaults,
}

/// A single safety filter threshold.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.safety-setting.schema@v1"))]
pub struct SafetySetting {
    /// Harm category.
    #[serde(default)]
    #[schemars(
        regex(pattern = "^HARM_CATEGORY_(UNSPECIFIED|HARASSMENT|HATE_SPEECH|SEXUALLY_EXPLICIT|DANGEROUS_CONTENT|CIVIC_INTEGRITY)$"),
        extend("x-oscal-subid" = "obs.software.antigravity.safety-setting.category@v1")
    )]
    pub category: String,
    /// Default blocking threshold.
    #[serde(default)]
    #[schemars(
        regex(pattern = "^(HARM_BLOCK_THRESHOLD_UNSPECIFIED|BLOCK_LOW_AND_ABOVE|BLOCK_MEDIUM_AND_ABOVE|BLOCK_ONLY_HIGH|BLOCK_NONE|OFF)$"),
        extend("x-oscal-subid" = "mut.service.antigravity.safety-setting.threshold@v1")
    )]
    pub threshold: String,
}

/// Sanitized status emitted by the native Google credential owner.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-opdbus-exact-json-schema" = true))]
pub struct AntigravityAuthStatus {
    /// Native credential kind (`authorized_user`, `service_account`, `api_key`, or `none`).
    pub credential_type: String,
    /// Non-secret source class (`application_default_credentials` or `environment`).
    pub credential_source: String,
    /// Whether enough credential material exists to attempt a provider request.
    pub credential_available: bool,
    /// Whether the credential owner can refresh this credential without a subprocess.
    pub refreshable: bool,
    /// OAuth scopes requested from the native credential owner.
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// Locally observed usage for one provider model.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-opdbus-exact-json-schema" = true))]
pub struct LocalModelUsage {
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// Durable usage report derived only from successful provider responses.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-opdbus-exact-json-schema" = true))]
pub struct LocalUsageReport {
    pub source: String,
    pub requests: u64,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    #[serde(default)]
    pub models: HashMap<String, LocalModelUsage>,
    #[serde(default)]
    pub last_recorded_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetUsageReportInput {
    /// Restrict the local response ledger to one exact model id.
    #[serde(default)]
    #[schemars(length(min = 1, max = 256), regex(pattern = "^[A-Za-z0-9._-]+$"))]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-opdbus-exact-json-schema" = true))]
pub struct ConfigureSafetyInput {
    /// Revision of the shared Antigravity runtime document; zero means absent.
    pub expected_revision: u64,
    /// Complete Gemini `safetySettings` replacement applied to later requests.
    #[serde(default)]
    #[schemars(length(max = 16))]
    pub settings: Vec<SafetySetting>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-opdbus-exact-json-schema" = true))]
pub struct ConfigureSafetyOutput {
    pub revision: u64,
    pub settings: Vec<SafetySetting>,
}

/// Token usage counters.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.token-tracking.schema@v1"))]
pub struct TokenTracking {
    /// Total input tokens consumed.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.token-tracking.total-input@v1"))]
    pub total_input_tokens: i64,
    /// Total output tokens generated.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.token-tracking.total-output@v1"))]
    pub total_output_tokens: i64,
    /// Total cached tokens used.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.token-tracking.total-cached@v1"))]
    pub total_cached_tokens: i64,
    /// Total thinking tokens consumed.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.token-tracking.total-thinking@v1"))]
    pub total_thinking_tokens: i64,
    /// Input tokens in the current session.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.token-tracking.session-input@v1"))]
    pub session_input_tokens: i64,
    /// Output tokens in the current session.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.token-tracking.session-output@v1"))]
    pub session_output_tokens: i64,
}

/// A single rate/quota limit bucket.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.quota-limit.schema@v1"))]
pub struct QuotaLimit {
    /// Limit value, null when unlimited.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.quota-limit.limit@v1"))]
    pub limit: Option<i64>,
    /// Tokens/requests already consumed.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.quota-limit.used@v1"))]
    pub used: i64,
    /// Remaining tokens/requests, null when unlimited.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.quota-limit.remaining@v1"))]
    pub remaining: Option<i64>,
}

/// Per-model quota limits.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.model-quota.schema@v1"))]
// Preserve numeric formats and explicit nulls at each dynamically keyed record.
#[schemars(extend("x-opdbus-exact-json-schema" = true))]
pub struct ModelQuota {
    /// Requests per minute limit.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.model-quota.requests-per-minute@v1"))]
    pub requests_per_minute: QuotaLimit,
    /// Tokens per minute limit.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.model-quota.tokens-per-minute@v1"))]
    pub tokens_per_minute: QuotaLimit,
    /// Requests per day limit.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.model-quota.requests-per-day@v1"))]
    pub requests_per_day: QuotaLimit,
}

/// Cost and billing tracking.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.cost-tracking.schema@v1"))]
pub struct CostTracking {
    /// Billing currency.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.cost-tracking.currency@v1"))]
    pub currency: String,
    /// Pricing model (e.g. pay_as_you_go).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.cost-tracking.pricing-model@v1"))]
    pub pricing_model: String,
    /// Total cost accrued.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.cost-tracking.total@v1"))]
    pub total_cost: f64,
    /// Cost in the current session.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.cost-tracking.session@v1"))]
    pub session_cost: f64,
    /// Billing account identifier.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.service.antigravity.cost-tracking.billing-account@v1"))]
    pub billing_account: String,
}

/// Monitoring endpoints.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.monitoring.schema@v1"))]
pub struct Monitoring {
    /// Metrics endpoint URL.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.monitoring.metrics-endpoint@v1"))]
    pub metrics_endpoint: String,
    /// Whether Cloud Logging is enabled.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.monitoring.logging-enabled@v1"))]
    pub logging_enabled: bool,
    /// Whether Cloud Trace is enabled.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.monitoring.trace-enabled@v1"))]
    pub trace_enabled: bool,
}

/// Usage and quota state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.usage.schema@v1"))]
pub struct Usage {
    /// Token counters.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.usage.token-tracking@v1"))]
    pub token_tracking: TokenTracking,
    /// Per-model quotas.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.usage.quotas@v1"))]
    pub quotas: HashMap<String, ModelQuota>,
    /// Cost tracking.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.usage.cost-tracking@v1"))]
    pub cost_tracking: CostTracking,
    /// Monitoring configuration.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.usage.monitoring@v1"))]
    pub monitoring: Monitoring,
}

/// Vertex AI endpoint templates.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.vertex-endpoints.schema@v1"))]
pub struct VertexEndpoints {
    /// Base URL.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.vertex-endpoints.base-url@v1"))]
    pub base_url: String,
    /// Stream generate content URL template.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.vertex-endpoints.stream-generate-content@v1"))]
    pub stream_generate_content: String,
    /// Predict URL template.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.vertex-endpoints.predict@v1"))]
    pub predict: String,
    /// Count tokens URL template.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.vertex-endpoints.count-tokens@v1"))]
    pub count_tokens: String,
    /// Batch prediction URL template.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.vertex-endpoints.batch-predict@v1"))]
    pub batch_predict: String,
}

/// Generative Language API endpoint templates.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.generative-language-endpoints.schema@v1"))]
pub struct GenerativeLanguageEndpoints {
    /// Base URL.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generative-language-endpoints.base-url@v1"))]
    pub base_url: String,
    /// Generate content URL template.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generative-language-endpoints.generate-content@v1"))]
    pub generate_content: String,
    /// Stream generate content URL template.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generative-language-endpoints.stream-generate-content@v1"))]
    pub stream_generate_content: String,
    /// Count tokens URL template.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.generative-language-endpoints.count-tokens@v1"))]
    pub count_tokens: String,
}

/// OAuth bridge endpoint details.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.oauth-bridge-endpoint.schema@v1"))]
pub struct OauthBridgeEndpoint {
    /// Bridge URL.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.oauth-bridge-endpoint.url@v1"))]
    pub url: String,
    /// Health check URL.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.service.antigravity.oauth-bridge-endpoint.health-check@v1"))]
    pub health_check: String,
    /// Token refresh URL.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.oauth-bridge-endpoint.token-refresh@v1"))]
    pub token_refresh: String,
    /// Endpoint status.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.oauth-bridge-endpoint.status@v1"))]
    pub status: String,
}

/// Gemma API endpoint templates.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.gemma-endpoints.schema@v1"))]
pub struct GemmaEndpoints {
    /// Base URL.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.gemma-endpoints.base-url@v1"))]
    pub base_url: String,
    /// Generate URL template.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.gemma-endpoints.generate@v1"))]
    pub generate: String,
}

/// Service endpoint collection.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.antigravity.endpoints.schema@v1"))]
pub struct Endpoints {
    /// Vertex AI endpoints.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.endpoints.vertex-ai@v1"))]
    pub vertex_ai: VertexEndpoints,
    /// Generative Language API endpoints.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.endpoints.generative-language@v1"))]
    pub generative_language_api: GenerativeLanguageEndpoints,
    /// OAuth bridge endpoint.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.endpoints.oauth-bridge@v1"))]
    pub oauth_bridge: OauthBridgeEndpoint,
    /// Gemma API endpoints.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.endpoints.gemma@v1"))]
    pub gemma_api: GemmaEndpoints,
}

fn default_llm_plugin() -> String {
    "chat_router".to_string()
}

fn default_provider_route() -> String {
    "gemini".to_string()
}

/// Top-level Antigravity state containing only native observations and the
/// revisioned provider intent actually consumed by generation requests.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.antigravity.schema@v1"))]
#[schemars(extend("x-oscal-category" = "llm"))]
pub struct AntigravityState {
    /// Operational status.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.antigravity.status@v1"))]
    pub status: String,
    /// Sanitized native credential observation.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.auth@v1"))]
    pub auth: AntigravityAuthStatus,
    /// Plugin that owns the model catalog (default chat_router).
    #[serde(default = "default_llm_plugin")]
    #[schemars(
        description = "Catalog plugin Antigravity delegates to for Gemini models (default chat_router).",
        example = &"chat_router",
        extend("x-oscal-subid" = "mut.software.antigravity.llm-plugin@v1")
    )]
    pub llm_plugin: String,
    /// Provider route inside `llm_plugin` (default gemini).
    #[serde(default = "default_provider_route")]
    #[schemars(
        description = "Provider route on llm_plugin for the Gemini catalog (default gemini).",
        example = &"gemini",
        extend("x-oscal-subid" = "mut.software.antigravity.provider-route@v1")
    )]
    pub provider_route: String,
    /// Session-selected model id from the delegated catalog; empty = provider default.
    #[serde(default)]
    #[schemars(
        description = "Selected model id from the delegated catalog. Empty falls back to the provider default.",
        extend("x-oscal-subid" = "mut.software.antigravity.selected-model@v1")
    )]
    pub selected_model: String,
    /// Provider protocol selected in the shared runtime document.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.antigravity.provider@v1"))]
    pub provider: String,
    /// Provider base URL selected in the shared runtime document.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.software.antigravity.base-url@v1"))]
    pub base_url: String,
    /// Safety filter settings.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.antigravity.safety-settings@v1"))]
    pub safety_settings: Vec<SafetySetting>,
    /// Local response usage. This contains no provider quota or billing claim.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.service.antigravity.usage@v1"))]
    pub usage: LocalUsageReport,
}

pub struct AntigravityPlugin;

impl Default for AntigravityPlugin {
    fn default() -> Self {
        Self
    }
}

impl AntigravityPlugin {
    pub fn new() -> Self {
        Self
    }

    pub fn current_state() -> AntigravityState {
        AntigravityState {
            status: "unobserved".to_string(),
            auth: AntigravityAuthStatus::default(),
            llm_plugin: default_llm_plugin(),
            provider_route: default_provider_route(),
            selected_model: String::new(),
            provider: "gemini".to_string(),
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
            safety_settings: Vec::new(),
            usage: LocalUsageReport {
                source: "local_provider_response_ledger".to_string(),
                ..Default::default()
            },
        }
    }
}

#[async_trait]
impl StatePlugin for AntigravityPlugin {
    fn name(&self) -> &str {
        "antigravity"
    }

    fn version(&self) -> &str {
        "3.0.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = antigravity_schema();
        super::common::llm_projection::rewrite_projection_subids_for_plugin(
            &mut schema,
            "antigravity",
        );
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        anyhow::bail!("antigravity reconciliation has no authoritative provider backend")
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        anyhow::bail!("antigravity apply has no authoritative provider backend")
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        anyhow::bail!("antigravity verification requires provider readback")
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        anyhow::bail!("antigravity has no authoritative checkpoint owner")
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        anyhow::bail!("antigravity has no authoritative rollback owner")
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: false,
            atomic_operations: false,
        }
    }
}

/// Canonical `antigravity` schema derived from [`AntigravityState`] via schemars.
pub(crate) fn antigravity_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(AntigravityState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "antigravity",
        "3.0.0",
        "Google Antigravity provider — sanitized native auth, applied safety policy, and local response usage; model catalog delegated to chat_router",
        &root,
    );
    if let Ok(state) = simd_json::serde::to_owned_value(AntigravityPlugin::current_state()) {
        super::schemars_adapter::apply_state_defaults(&mut schema, &state);
        schema.example = Some(state);
    }

    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
    use op_state_store::SideEffect;

    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub struct GetAuthStatusOutput {
        pub auth: AntigravityAuthStatus,
    }
    #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    pub struct GetUsageReportOutput {
        pub usage: LocalUsageReport,
    }

    // NOTE: no `chat` / `list_models` here by design — catalog and chat live on
    // llm_plugin (default chat_router) via provider_route.
    schema.methods.insert(
        "get_auth_status".to_string(),
        method_decl_from_schemars_with_output::<
            super::plugin_scaffold_helpers::EmptyInput,
            GetAuthStatusOutput,
        >(
            "get_auth_status",
            SideEffect::Read,
            true,
            "antigravity.read",
            "obs.software.antigravity.auth.status@v1",
        ),
    );
    schema.methods.insert(
        "get_usage_report".to_string(),
        method_decl_from_schemars_with_output::<GetUsageReportInput, GetUsageReportOutput>(
            "get_usage_report",
            SideEffect::Read,
            true,
            "antigravity.read",
            "obs.software.antigravity.usage.report@v1",
        ),
    );
    schema.methods.insert(
        "configure_safety".to_string(),
        method_decl_from_schemars_with_output::<ConfigureSafetyInput, ConfigureSafetyOutput>(
            "configure_safety",
            SideEffect::Mutation,
            false,
            "antigravity.write",
            "mut.software.antigravity.safety.configure@v1",
        ),
    );

    schema.capabilities.insert(
        "antigravity.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "antigravity.read".to_string(),
            description: "Grants native auth observation and local response-usage reporting."
                .to_string(),
        },
    );
    schema.capabilities.insert(
        "antigravity.write".to_string(),
        op_state_store::CapabilityDecl {
            id: "antigravity.write".to_string(),
            description: "Grants revisioned Gemini safety-policy replacement.".to_string(),
        },
    );

    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::common::oscal::validate_subid;
    use serde_json::Value as JVal;

    fn collect_subids(node: &JVal, out: &mut Vec<String>) {
        if let Some(subid) = node.get("x-oscal-subid").and_then(JVal::as_str) {
            out.push(subid.to_string());
        }
        if let Some(props) = node.get("properties").and_then(JVal::as_object) {
            for v in props.values() {
                collect_subids(v, out);
            }
        }
        if let Some(defs) = node
            .get("$defs")
            .or_else(|| node.get("definitions"))
            .and_then(JVal::as_object)
        {
            for v in defs.values() {
                collect_subids(v, out);
            }
        }
        if let Some(items) = node.get("items") {
            collect_subids(items, out);
        }
        if let Some(alternatives) = node
            .get("anyOf")
            .or_else(|| node.get("oneOf"))
            .and_then(JVal::as_array)
        {
            for v in alternatives {
                collect_subids(v, out);
            }
        }
    }

    #[test]
    fn schema_is_schemars_seeded_and_typed() {
        let schema = antigravity_schema();
        assert_eq!(schema.name, "antigravity");
        assert_eq!(schema.version, "3.0.0");
        assert!(schema.fields.contains_key("auth"));
        assert!(schema.fields.contains_key("llm_plugin"));
        assert!(schema.fields.contains_key("provider_route"));
        assert!(schema.fields.contains_key("selected_model"));
        assert!(schema.fields.contains_key("provider"));
        assert!(schema.fields.contains_key("base_url"));
        assert!(!schema.fields.contains_key("models"));
        assert!(!schema.methods.contains_key("list_models"));
    }

    #[test]
    fn state_schema_contains_only_native_or_durable_observations() {
        let schema = antigravity_schema();
        assert_eq!(AntigravityPlugin::new().version(), schema.version);
        for removed in ["project", "generation_config", "endpoints", "projection"] {
            assert!(
                !schema.fields.contains_key(removed),
                "stale field {removed}"
            );
        }
        let example = serde_json::to_value(schema.example.as_ref().unwrap()).unwrap();
        assert_eq!(example["usage"]["source"], "local_provider_response_ledger");
        assert!(example["usage"].get("quotas").is_none());
        assert!(example["usage"].get("cost_tracking").is_none());
        assert!(
            !serde_json::to_string(&example)
                .unwrap()
                .contains("billingAccounts")
        );
    }

    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(AntigravityState)).unwrap();
        let mut subids = Vec::new();
        collect_subids(&raw, &mut subids);
        assert!(
            !subids.is_empty(),
            "expected at least one x-oscal-subid in the derived schema"
        );
        for subid in subids {
            validate_subid(&subid).expect("invalid subid: {subid}");
        }
    }

    #[tokio::test]
    async fn generic_lifecycle_never_returns_a_false_receipt() {
        let plugin = AntigravityPlugin::new();
        let state = simd_json::json!({});
        assert!(plugin.calculate_diff(&state, &state).await.is_err());
        let diff = StateDiff {
            plugin: plugin.name().to_owned(),
            actions: vec![],
            metadata: op_state::DiffMetadata {
                timestamp: 0,
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        };
        assert!(plugin.apply_state(&diff).await.is_err());
        assert!(plugin.verify_state(&state).await.is_err());
        assert!(plugin.create_checkpoint().await.is_err());
        let checkpoint = Checkpoint {
            id: "fixture".into(),
            plugin: plugin.name().into(),
            timestamp: 0,
            state_snapshot: state,
            backend_checkpoint: None,
        };
        assert!(plugin.rollback(&checkpoint).await.is_err());
        let capabilities = plugin.capabilities();
        assert!(!capabilities.supports_verification);
        assert!(!capabilities.supports_checkpoints);
        assert!(!capabilities.supports_rollback);
        assert!(!capabilities.atomic_operations);
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("antigravity", |_ctx| std::sync::Arc::new(AntigravityPlugin::new()))
}
