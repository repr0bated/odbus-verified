//! model_registry — the system-wide catalog of LLM providers and the models
//! each one serves right now.
//!
//! The provider list is a root-owned TOML file
//! (`/etc/op-dbus/model-providers.toml`, shipped from
//! `deploy/config/model-providers.toml`); this plugin is its typed contract.
//! `refresh` asks every enabled provider for its current model list and folds
//! the answers into a dedicated Cozo catalog: new models are added, a model a
//! provider stops listing is kept but marked unavailable, and a provider that
//! cannot be reached keeps its last good list. OpenCode and OpenRouter change
//! their lists constantly, so the control-plane chatbot runs `refresh` hourly
//! through its own lungs, and every identity reads the catalog with
//! `list_providers` and `list_models`.
//!
//! The catalog is read-only evidence of what providers serve. Choosing the
//! Control Plane model remains a protected `tched_router` task (elevation);
//! this plugin never changes a route.

use std::collections::BTreeSet;

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use simd_json::OwnedValue as Value;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;

pub const PLUGIN_ID: &str = "model_registry";
const PLUGIN_VERSION: &str = "1.0.0";
pub const READ_CAPABILITY: &str = "model_registry.read";
pub const REFRESH_CAPABILITY: &str = "model_registry.refresh";

/// Where the bridge reads the provider list unless `OP_MODEL_PROVIDERS_PATH`
/// names another root-owned file.
pub const DEFAULT_PROVIDERS_PATH: &str = "/etc/op-dbus/model-providers.toml";
/// API keys are read only from root-owned files in this directory.
pub const SECRETS_DIR: &str = "/etc/op-dbus/secrets/";
pub const MAX_PROVIDERS: usize = 32;
pub const MAX_MODELS_PER_PROVIDER: usize = 5000;
pub const MAX_PROVIDERS_FILE_BYTES: usize = 64 * 1024;
pub const MAX_MODELS_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_KEY_BYTES: usize = 4096;
pub const DEFAULT_PAGE: u32 = 100;
pub const MAX_PAGE: u32 = 500;
pub const MAX_MODEL_ID_BYTES: usize = 200;
pub const MAX_NAME_BYTES: usize = 200;
pub const MAX_QUERY_BYTES: usize = 100;
pub const MAX_ERROR_BYTES: usize = 300;
const MAX_PRICE_BYTES: usize = 32;
const MAX_CONTEXT_LENGTH: u64 = 100_000_000;

/// How a provider's model list is shaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModelListFormat {
    /// OpenAI-compatible `GET /models`: `{"data": [{"id": ...}]}` (OpenCode,
    /// OpenRouter, most gateways).
    Openai,
    /// Anthropic `GET /v1/models`: `{"data": [{"id", "display_name"}]}`.
    Anthropic,
}

/// How the key is presented when listing models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuth {
    /// The list is public.
    None,
    /// `Authorization: Bearer <key>`.
    Bearer,
    /// `x-api-key: <key>` (Anthropic).
    XApiKey,
}

fn enabled_default() -> bool {
    true
}

/// One provider in the TOML file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    /// Stable id: lowercase letters, digits and `_`, starting with a letter.
    pub id: String,
    pub name: String,
    /// HTTPS endpoint that lists the provider's models.
    pub models_url: String,
    pub format: ModelListFormat,
    pub auth: ProviderAuth,
    /// Root-owned key file under /etc/op-dbus/secrets/, required unless
    /// `auth = "none"`. The key is only ever sent to `models_url`.
    #[serde(default)]
    pub key_file: Option<String>,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
}

/// The whole provider list: `[[providers]]` tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProvidersFile {
    pub providers: Vec<ProviderConfig>,
}

pub fn valid_provider_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id.as_bytes()[0].is_ascii_lowercase()
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn printable(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

pub fn valid_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_MODEL_ID_BYTES
        && !id.chars().any(|c| c.is_control() || c.is_whitespace())
}

fn validate_models_url(raw: &str) -> Result<()> {
    let url =
        reqwest::Url::parse(raw).with_context(|| format!("models_url {raw:?} is not a URL"))?;
    ensure!(url.scheme() == "https", "models_url must be https: {raw}");
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "models_url must not carry credentials"
    );
    ensure!(
        url.fragment().is_none(),
        "models_url must not have a fragment"
    );
    ensure!(
        url.host_str().is_some_and(|host| !host.is_empty()),
        "models_url needs a host"
    );
    Ok(())
}

fn validate_key_file(path: &str) -> Result<()> {
    let name = path
        .strip_prefix(SECRETS_DIR)
        .with_context(|| format!("key_file must be under {SECRETS_DIR}"))?;
    ensure!(
        !name.is_empty()
            && name.len() <= 128
            && !name.starts_with('.')
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')),
        "key_file must name one file directly under {SECRETS_DIR}"
    );
    Ok(())
}

pub fn validate_provider(provider: &ProviderConfig) -> Result<()> {
    ensure!(
        valid_provider_id(&provider.id),
        "provider id {:?} must be lowercase letters, digits and _ (starting with a letter)",
        provider.id
    );
    ensure!(
        printable(&provider.name, MAX_NAME_BYTES),
        "provider {} needs a printable name",
        provider.id
    );
    validate_models_url(&provider.models_url)
        .with_context(|| format!("provider {}", provider.id))?;
    match (provider.auth, provider.key_file.as_deref()) {
        (ProviderAuth::None, None) => {}
        (ProviderAuth::None, Some(_)) => {
            bail!(
                "provider {} has auth = \"none\" but names a key_file",
                provider.id
            )
        }
        (_, None) => bail!("provider {} needs a key_file for its auth", provider.id),
        (_, Some(path)) => {
            validate_key_file(path).with_context(|| format!("provider {}", provider.id))?
        }
    }
    Ok(())
}

/// Parse and validate the provider TOML.
pub fn parse_providers_toml(text: &str) -> Result<ProvidersFile> {
    ensure!(
        text.len() <= MAX_PROVIDERS_FILE_BYTES,
        "provider list exceeds {MAX_PROVIDERS_FILE_BYTES} bytes"
    );
    let file: ProvidersFile = toml::from_str(text).context("provider list is not valid TOML")?;
    ensure!(
        file.providers.len() <= MAX_PROVIDERS,
        "provider list names more than {MAX_PROVIDERS} providers"
    );
    let mut ids = BTreeSet::new();
    for provider in &file.providers {
        validate_provider(provider)?;
        ensure!(
            ids.insert(provider.id.as_str()),
            "provider id {} is listed twice",
            provider.id
        );
    }
    Ok(file)
}

/// One model as a provider lists it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObservedModel {
    pub model_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub context_length: Option<u64>,
    /// Price per input token in USD, as the provider states it.
    #[serde(default)]
    pub prompt_price: Option<String>,
    /// Price per output token in USD, as the provider states it.
    #[serde(default)]
    pub completion_price: Option<String>,
}

fn price(value: Option<&JsonValue>) -> Option<String> {
    let text = match value? {
        JsonValue::String(text) => text.trim().to_string(),
        JsonValue::Number(number) => number.to_string(),
        _ => return None,
    };
    let parsed: f64 = text.parse().ok()?;
    (parsed.is_finite() && parsed >= 0.0 && text.len() <= MAX_PRICE_BYTES).then_some(text)
}

fn name(value: Option<&JsonValue>) -> Option<String> {
    let text = value?.as_str()?.trim();
    printable(text, MAX_NAME_BYTES).then(|| text.to_string())
}

/// The models in a provider's list response. Entries with an unusable id
/// are skipped; a response over [`MAX_MODELS_PER_PROVIDER`] models, or with
/// no list at all, is rejected so a truncated answer never marks the rest
/// unavailable.
pub fn parse_models_response(format: ModelListFormat, body: &[u8]) -> Result<Vec<ObservedModel>> {
    ensure!(
        body.len() <= MAX_MODELS_RESPONSE_BYTES,
        "model list exceeds {MAX_MODELS_RESPONSE_BYTES} bytes"
    );
    let document: JsonValue = serde_json::from_slice(body).context("model list is not JSON")?;
    let entries = match &document {
        JsonValue::Object(object) => object.get("data").and_then(JsonValue::as_array),
        JsonValue::Array(entries) if format == ModelListFormat::Openai => Some(entries),
        _ => None,
    }
    .context("model list has no data array")?;
    ensure!(
        entries.len() <= MAX_MODELS_PER_PROVIDER,
        "provider lists more than {MAX_MODELS_PER_PROVIDER} models"
    );
    let mut seen = BTreeSet::new();
    let mut models = Vec::new();
    for entry in entries {
        let Some(id) = entry.get("id").and_then(JsonValue::as_str) else {
            continue;
        };
        if !valid_model_id(id) || !seen.insert(id.to_string()) {
            continue;
        }
        let (display, context_length, prompt_price, completion_price) = match format {
            ModelListFormat::Openai => (
                name(entry.get("name")),
                entry
                    .get("context_length")
                    .and_then(JsonValue::as_u64)
                    .filter(|length| (1..=MAX_CONTEXT_LENGTH).contains(length)),
                price(entry.get("pricing").and_then(|p| p.get("prompt"))),
                price(entry.get("pricing").and_then(|p| p.get("completion"))),
            ),
            ModelListFormat::Anthropic => (name(entry.get("display_name")), None, None, None),
        };
        models.push(ObservedModel {
            model_id: id.to_string(),
            name: display,
            context_length,
            prompt_price,
            completion_price,
        });
    }
    Ok(models)
}

/// Bound an error for the catalog and for callers; never the key or a body.
pub fn bounded_error(message: &str) -> String {
    let clean: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let clean = clean.trim();
    if clean.len() <= MAX_ERROR_BYTES {
        return clean.to_string();
    }
    let mut end = MAX_ERROR_BYTES;
    while !clean.is_char_boundary(end) {
        end -= 1;
    }
    clean[..end].to_string()
}

// ---- methods ----

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListProvidersInput {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderStatus {
    pub provider_id: String,
    /// Absent for a provider that was removed from the list.
    #[serde(default)]
    pub name: Option<String>,
    /// In the current provider list.
    pub configured: bool,
    pub enabled: bool,
    #[serde(default)]
    pub models_url: Option<String>,
    /// `never`, `ok`, `error` or `removed`.
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
    /// Last refresh attempt (RFC 3339).
    #[serde(default)]
    pub refreshed_at: Option<String>,
    /// Last successful refresh (RFC 3339).
    #[serde(default)]
    pub succeeded_at: Option<String>,
    pub available_models: u64,
    pub known_models: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListProvidersOutput {
    pub providers: Vec<ProviderStatus>,
    /// Advances on every refresh.
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelRef {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListModelsInput {
    /// Only this provider's models.
    #[serde(default)]
    pub provider_id: Option<String>,
    /// Include models providers no longer list (default false).
    #[serde(default)]
    pub include_unavailable: Option<bool>,
    /// Case-insensitive substring of the model id or name.
    #[serde(default)]
    pub query: Option<String>,
    /// Continue after this model (the last one of the previous page).
    #[serde(default)]
    pub after: Option<ModelRef>,
    /// Page size, 1 to 500 (default 100).
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelRecord {
    pub provider_id: String,
    pub model_id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub context_length: Option<u64>,
    #[serde(default)]
    pub prompt_price: Option<String>,
    #[serde(default)]
    pub completion_price: Option<String>,
    /// The provider listed it at its last successful refresh.
    pub available: bool,
    pub first_seen: String,
    pub last_seen: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListModelsOutput {
    /// Ordered by provider, then model id.
    pub models: Vec<ModelRecord>,
    /// More models follow this page.
    pub more: bool,
    pub revision: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefreshInput {
    /// Refresh only this configured provider (default: every enabled one).
    #[serde(default)]
    pub provider_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderRefresh {
    pub provider_id: String,
    /// `ok`, `error` or `removed`.
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
    pub available_models: u64,
    /// Models listed now that were not available before.
    pub added: u64,
    /// Models no longer listed.
    pub dropped: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefreshOutput {
    pub refreshed_at: String,
    pub revision: u64,
    pub providers: Vec<ProviderRefresh>,
}

pub fn page_limit(limit: Option<u32>) -> Result<usize> {
    let limit = limit.unwrap_or(DEFAULT_PAGE);
    ensure!(
        (1..=MAX_PAGE).contains(&limit),
        "limit must be 1 to {MAX_PAGE}"
    );
    Ok(limit as usize)
}

pub fn validate_list_models(input: &ListModelsInput) -> Result<()> {
    if let Some(provider) = &input.provider_id {
        ensure!(valid_provider_id(provider), "invalid provider_id");
    }
    if let Some(query) = &input.query {
        ensure!(
            printable(query, MAX_QUERY_BYTES),
            "query must be 1 to {MAX_QUERY_BYTES} printable bytes"
        );
    }
    if let Some(after) = &input.after {
        ensure!(
            valid_provider_id(&after.provider_id) && valid_model_id(&after.model_id),
            "invalid after cursor"
        );
    }
    page_limit(input.limit)?;
    Ok(())
}

/// The provider-list contract, sealed in this plugin's blob.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.plugin.model-registry.schema@v1"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct ModelRegistryState {
    /// Root-owned TOML file listing the providers.
    #[schemars(extend("x-oscal-subid" = "obs.service.model-registry.providers-path@v1"))]
    pub providers_path: String,
    /// Directory the key files must live in.
    #[schemars(extend("x-oscal-subid" = "obs.service.model-registry.secrets-dir@v1"))]
    pub secrets_dir: String,
    #[schemars(extend("x-oscal-subid" = "obs.service.model-registry.max-providers@v1"))]
    pub max_providers: u64,
    #[schemars(extend("x-oscal-subid" = "obs.service.model-registry.max-models@v1"))]
    pub max_models_per_provider: u64,
}

impl Default for ModelRegistryState {
    fn default() -> Self {
        Self {
            providers_path: DEFAULT_PROVIDERS_PATH.to_string(),
            secrets_dir: SECRETS_DIR.to_string(),
            max_providers: MAX_PROVIDERS as u64,
            max_models_per_provider: MAX_MODELS_PER_PROVIDER as u64,
        }
    }
}

pub fn model_registry_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(ModelRegistryState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_ID,
        PLUGIN_VERSION,
        "System-wide catalog of LLM providers and the models they currently serve",
        &root,
    );
    schema.category = "service".to_string();
    schema.display_name = Some("Model Registry".to_string());
    let state = simd_json::serde::to_owned_value(&ModelRegistryState::default())
        .expect("ModelRegistryState default serializes");
    super::schemars_adapter::apply_state_defaults(&mut schema, &state);
    schema.methods.insert(
        "list_providers".to_string(),
        method_decl_from_schemars_with_output::<ListProvidersInput, ListProvidersOutput>(
            "list_providers",
            SideEffect::Read,
            true,
            READ_CAPABILITY,
            "obs.service.model-registry.providers.list@v1",
        ),
    );
    schema.methods.insert(
        "list_models".to_string(),
        method_decl_from_schemars_with_output::<ListModelsInput, ListModelsOutput>(
            "list_models",
            SideEffect::Read,
            true,
            READ_CAPABILITY,
            "obs.service.model-registry.models.list@v1",
        ),
    );
    schema.methods.insert(
        "refresh".to_string(),
        method_decl_from_schemars_with_output::<RefreshInput, RefreshOutput>(
            "refresh",
            SideEffect::Mutation,
            false,
            REFRESH_CAPABILITY,
            "mut.service.model-registry.catalog.refresh@v1",
        ),
    );
    schema.capabilities.insert(
        READ_CAPABILITY.to_string(),
        op_state_store::CapabilityDecl {
            id: READ_CAPABILITY.to_string(),
            description: "Grants: list_providers, list_models.".to_string(),
        },
    );
    schema.capabilities.insert(
        REFRESH_CAPABILITY.to_string(),
        op_state_store::CapabilityDecl {
            id: REFRESH_CAPABILITY.to_string(),
            description: "Grants: refresh (ask every configured provider for its current models)."
                .to_string(),
        },
    );
    schema
}

pub struct ModelRegistryPlugin;

impl ModelRegistryPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ModelRegistryPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatePlugin for ModelRegistryPlugin {
    fn name(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = model_registry_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: PLUGIN_ID.to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "schema".to_string(),
                desired_hash: "schema".to_string(),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: PLUGIN_ID.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(ModelRegistryState::default())?,
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_ID, |_ctx| std::sync::Arc::new(ModelRegistryPlugin::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_blob::subid::validate_subid;
    use serde_json::json;

    const PROVIDERS: &str = r#"
[[providers]]
id = "opencode_go"
name = "OpenCode Go"
models_url = "https://opencode.ai/zen/go/v1/models"
format = "openai"
auth = "bearer"
key_file = "/etc/op-dbus/secrets/opencode-api-key"

[[providers]]
id = "openrouter"
name = "OpenRouter"
models_url = "https://openrouter.ai/api/v1/models"
format = "openai"
auth = "none"
enabled = false
"#;

    #[test]
    fn provider_list_parses_with_defaults() {
        let file = parse_providers_toml(PROVIDERS).unwrap();
        assert_eq!(file.providers.len(), 2);
        assert!(file.providers[0].enabled);
        assert!(!file.providers[1].enabled);
        assert_eq!(file.providers[0].auth, ProviderAuth::Bearer);
    }

    #[test]
    fn deployed_provider_list_is_valid() {
        let text = include_str!("../../../../deploy/config/model-providers.toml");
        let file = parse_providers_toml(text).unwrap();
        assert!(file.providers.iter().any(|p| p.id == "opencode_go"));
        assert!(file.providers.iter().any(|p| p.id == "openrouter"));
    }

    #[test]
    fn provider_list_rejects_unsafe_entries() {
        let one = |body: &str| {
            format!("[[providers]]\nid = \"p\"\nname = \"P\"\nformat = \"openai\"\n{body}")
        };
        for (body, why) in [
            (
                "models_url = \"http://x.test/models\"\nauth = \"none\"",
                "https",
            ),
            (
                "models_url = \"https://u:p@x.test/models\"\nauth = \"none\"",
                "credentials",
            ),
            (
                "models_url = \"https://x.test/models\"\nauth = \"bearer\"",
                "needs a key_file",
            ),
            (
                "models_url = \"https://x.test/models\"\nauth = \"bearer\"\nkey_file = \"/root/key\"",
                "secrets",
            ),
            (
                "models_url = \"https://x.test/models\"\nauth = \"bearer\"\nkey_file = \"/etc/op-dbus/secrets/../shadow\"",
                "one file",
            ),
            (
                "models_url = \"https://x.test/models\"\nauth = \"none\"\nkey_file = \"/etc/op-dbus/secrets/k\"",
                "names a key_file",
            ),
            (
                "models_url = \"https://x.test/models\"\nauth = \"none\"\nurl = \"x\"",
                "TOML",
            ),
        ] {
            let error = format!("{:#}", parse_providers_toml(&one(body)).unwrap_err());
            assert!(error.contains(why), "{why}: {error}");
        }
        let twice = format!(
            "{PROVIDERS}\n{}",
            &PROVIDERS[PROVIDERS.find("[[providers]]").unwrap()..]
        );
        assert!(
            format!("{:#}", parse_providers_toml(&twice).unwrap_err()).contains("listed twice")
        );
        let bad_id = PROVIDERS.replace("\"opencode_go\"", "\"OpenCode\"");
        assert!(parse_providers_toml(&bad_id).is_err());
    }

    #[test]
    fn openai_and_openrouter_lists_parse() {
        let body = json!({
            "object": "list",
            "data": [
                {"id": "glm-5.3-flash", "object": "model"},
                {"id": "anthropic/claude-fable-5-1", "name": "Anthropic: Claude Fable 5.1",
                 "context_length": 200000,
                 "pricing": {"prompt": "0.000003", "completion": "0.000015"}},
                {"id": "openrouter/auto", "pricing": {"prompt": "-1", "completion": "-1"}},
                {"id": "glm-5.3-flash"},
                {"id": "bad id"},
                {"id": ""},
                {"name": "no id"}
            ]
        });
        let models =
            parse_models_response(ModelListFormat::Openai, body.to_string().as_bytes()).unwrap();
        let ids: Vec<_> = models.iter().map(|m| m.model_id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "glm-5.3-flash",
                "anthropic/claude-fable-5-1",
                "openrouter/auto"
            ]
        );
        assert_eq!(models[1].context_length, Some(200000));
        assert_eq!(models[1].prompt_price.as_deref(), Some("0.000003"));
        assert_eq!(
            models[1].name.as_deref(),
            Some("Anthropic: Claude Fable 5.1")
        );
        assert_eq!(models[2].prompt_price, None);
    }

    #[test]
    fn anthropic_list_parses() {
        let body = json!({"data": [{"type": "model", "id": "claude-fable-5-1",
            "display_name": "Claude Fable 5.1", "created_at": "2026-08-01T00:00:00Z"}],
            "has_more": false});
        let models =
            parse_models_response(ModelListFormat::Anthropic, body.to_string().as_bytes()).unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].name.as_deref(), Some("Claude Fable 5.1"));
    }

    #[test]
    fn unusable_lists_are_rejected_not_truncated() {
        assert!(parse_models_response(ModelListFormat::Openai, b"{\"error\":\"x\"}").is_err());
        assert!(parse_models_response(ModelListFormat::Openai, b"not json").is_err());
        let many: Vec<_> = (0..=MAX_MODELS_PER_PROVIDER)
            .map(|i| json!({"id": format!("m{i}")}))
            .collect();
        let body = json!({"data": many}).to_string();
        assert!(parse_models_response(ModelListFormat::Openai, body.as_bytes()).is_err());
    }

    #[test]
    fn errors_are_bounded_and_single_line() {
        let long = format!("line\n{}", "é".repeat(400));
        let bounded = bounded_error(&long);
        assert!(bounded.len() <= MAX_ERROR_BYTES);
        assert!(!bounded.contains('\n'));
    }

    #[test]
    fn list_models_input_is_bounded() {
        assert!(validate_list_models(&ListModelsInput::default()).is_ok());
        assert!(
            validate_list_models(&ListModelsInput {
                limit: Some(MAX_PAGE + 1),
                ..Default::default()
            })
            .is_err()
        );
        assert!(
            validate_list_models(&ListModelsInput {
                query: Some(" padded".into()),
                ..Default::default()
            })
            .is_err()
        );
    }

    #[test]
    fn schema_declares_three_methods_with_valid_subids() {
        let schema = model_registry_schema();
        assert_eq!(schema.methods.len(), 3);
        for method in schema.methods.values() {
            validate_subid(&method.subid).unwrap();
        }
        let raw = serde_json::to_value(schemars::schema_for!(ModelRegistryState)).unwrap();
        let mut stack = vec![&raw];
        while let Some(node) = stack.pop() {
            if let Some(subid) = node.get("x-oscal-subid").and_then(|v| v.as_str()) {
                validate_subid(subid).unwrap();
            }
            if let Some(object) = node.as_object() {
                stack.extend(object.values());
            }
        }
        assert!(schema.fields.contains_key("providers_path"));
        assert_eq!(
            schema.methods["refresh"].required_capability.as_deref(),
            Some(REFRESH_CAPABILITY)
        );
        assert_eq!(
            schema.methods["list_models"].required_capability.as_deref(),
            Some(READ_CAPABILITY)
        );
    }
}
