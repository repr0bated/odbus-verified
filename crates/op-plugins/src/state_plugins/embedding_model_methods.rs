//! Native embedding adapters. Protocol references:
//! https://docs.voyageai.com/reference/embeddings-api-1
//! https://docs.ollama.com/api/embed and https://docs.ollama.com/api/tags
//! https://developers.openai.com/api/reference/resources/embeddings/methods/create
//!
//! The transport takes credentials only from independently pinned environment
//! settings, never from mutable configuration or method inputs.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, ensure, Context, Result};
use futures::StreamExt;
use reqwest::{Client, Method, Url};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{EmbeddingModelPlugin, EmbeddingModelState, VOYAGE_PUBLIC_URL};
use crate::state_plugins::runtime_documents::{runtime_documents, RuntimeDocuments};

const PLUGIN_ID: &str = "embedding_model";
const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingProvider {
    Voyage,
    Ollama,
    #[serde(alias = "openai")]
    OpenaiCompatible,
}

impl EmbeddingProvider {
    fn name(self) -> &'static str {
        match self {
            Self::Voyage => "voyage",
            Self::Ollama => "ollama",
            Self::OpenaiCompatible => "openai_compatible",
        }
    }
}

fn default_timeout_ms() -> u64 {
    30_000
}
fn request_dimensions_default() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EmbeddingConfig {
    pub provider: EmbeddingProvider,
    pub model_id: String,
    /// An embedding endpoint or API base; normalized to the provider's
    /// embedding URL before persistence. Credentials/query strings forbidden.
    pub endpoint: String,
    /// Expected vector length. Different models are not assumed to share a
    /// vector space simply because their dimensions match.
    pub dimensions: u32,
    /// Disable for models/protocol versions without a dimensions argument;
    /// returned vector lengths are still checked against dimensions.
    #[serde(default = "request_dimensions_default")]
    pub request_dimensions: bool,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

impl EmbeddingConfig {
    pub(super) fn from_environment() -> Result<Self> {
        let provider: EmbeddingProvider = serde_json::from_value(json!(std::env::var(
            "EMBEDDING_PROVIDER"
        )
        .unwrap_or_else(|_| "voyage".into())))
        .context("unsupported EMBEDDING_PROVIDER")?;
        let provider_default = match provider {
            EmbeddingProvider::Voyage => EmbeddingModelPlugin::voyage_api_key()
                .map(|key| EmbeddingModelPlugin::voyage_url_for_key(&key))
                .unwrap_or_else(|| VOYAGE_PUBLIC_URL.into()),
            EmbeddingProvider::Ollama => {
                std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://127.0.0.1:11434".into())
            }
            EmbeddingProvider::OpenaiCompatible => std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
        };
        let model_id = std::env::var("EMBEDDING_MODEL_ID")
            .ok()
            .or_else(|| {
                (provider == EmbeddingProvider::Voyage)
                    .then(|| {
                        std::env::var("COGNITIVE_MCP_VOYAGE_MODEL")
                            .or_else(|_| std::env::var("VOYAGE_MODEL"))
                            .ok()
                    })
                    .flatten()
            })
            .unwrap_or_default();
        Self {
            provider,
            model_id,
            endpoint: std::env::var("EMBEDDING_ENDPOINT").unwrap_or(provider_default),
            dimensions: env_number("EMBEDDING_DIMENSIONS", 1024)?,
            request_dimensions: match std::env::var("EMBEDDING_REQUEST_DIMENSIONS") {
                Ok(value) => value
                    .parse()
                    .context("invalid EMBEDDING_REQUEST_DIMENSIONS")?,
                Err(_) => true,
            },
            timeout_ms: env_number("EMBEDDING_TIMEOUT_MS", default_timeout_ms())?,
        }
        .normalized()
    }

    fn normalized(mut self) -> Result<Self> {
        ensure!(
            self.model_id.len() <= 512 && !self.model_id.chars().any(char::is_control),
            "invalid embedding model identifier"
        );
        self.model_id = self.model_id.trim().to_owned();
        ensure!(
            (1..=16_384).contains(&self.dimensions),
            "embedding dimensions must be 1..=16384"
        );
        ensure!(
            (100..=120_000).contains(&self.timeout_ms),
            "embedding timeout_ms must be 100..=120000"
        );
        self.endpoint = embedding_url(self.provider, &self.endpoint)?.to_string();
        Ok(self)
    }

    pub(super) fn state(&self, revision: u64) -> EmbeddingModelState {
        EmbeddingModelState {
            config_revision: revision,
            provider: self.provider.name().to_owned(),
            model_id: self.model_id.clone(),
            endpoint: self.endpoint.clone(),
            dimensions: self.dimensions,
            status: "unprobed".into(),
            available_models: vec![],
            model_digest: String::new(),
        }
    }
}

fn env_number<T: std::str::FromStr>(key: &str, fallback: T) -> Result<T> {
    match std::env::var(key) {
        Ok(value) => value.parse().map_err(|_| anyhow::anyhow!("invalid {key}")),
        Err(_) => Ok(fallback),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingInputType {
    Query,
    Document,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmbedInput {
    pub texts: Vec<String>,
    /// Retrieval role applied by adapters with native support. Adapters without
    /// a corresponding wire field accept the semantic hint but omit it from
    /// their provider request.
    #[serde(default)]
    pub input_type: Option<EmbeddingInputType>,
    #[serde(default)]
    pub truncate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EmbedOutput {
    pub vectors: Vec<Vec<f32>>,
    pub dimensions: u32,
    /// Requested model from the configuration revision used by this request.
    pub model: String,
    pub provider_model: Option<String>,
    pub provider: EmbeddingProvider,
    pub config_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListModelsOutput {
    pub models: Vec<String>,
    pub provider: EmbeddingProvider,
    /// "provider" for a native model-list response, "configured" when the
    /// provider has no model-list endpoint (Voyage).
    pub source: String,
    pub enumerated: bool,
    /// Model-list APIs do not prove that every listed model embeds text.
    pub embedding_support_verified: bool,
    pub config_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConfigOutput {
    pub config: EmbeddingConfig,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetModelInput {
    pub model_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateConfigInput {
    pub config: EmbeddingConfig,
}

// No Debug/Serialize implementations: keys can never become method output,
// persisted configuration, a schema field, or a debugging representation.
#[derive(Clone)]
struct Credential {
    provider: EmbeddingProvider,
    key: String,
    origin: String,
}

#[derive(Clone)]
struct EmbeddingHttp {
    client: Client,
    credentials: Vec<Credential>,
    #[cfg(test)]
    allow_test_loopback_credentials: bool,
}

impl EmbeddingHttp {
    fn from_environment() -> Result<Self> {
        let mut credentials = Vec::new();
        if let Some(key) = EmbeddingModelPlugin::voyage_api_key() {
            let fallback = EmbeddingModelPlugin::voyage_url_for_key(&key);
            credentials.push(credential_from_environment(
                EmbeddingProvider::Voyage,
                key,
                &fallback,
            )?);
        }
        if let Some(key) = ["EMBEDDING_OPENAI_API_KEY", "OPENAI_API_KEY"]
            .iter()
            .find_map(|name| {
                std::env::var(name)
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
        {
            let fallback = std::env::var("OPENAI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into());
            credentials.push(credential_from_environment(
                EmbeddingProvider::OpenaiCompatible,
                key,
                &fallback,
            )?);
        }
        if let Ok(key) = std::env::var("OLLAMA_API_KEY") {
            if !key.trim().is_empty() {
                credentials.push(credential_from_environment(
                    EmbeddingProvider::Ollama,
                    key,
                    "https://ollama.com",
                )?);
            }
        }
        Ok(Self {
            client: bounded_client()?,
            credentials,
            #[cfg(test)]
            allow_test_loopback_credentials: false,
        })
    }

    fn credential(&self, provider: EmbeddingProvider) -> Option<&Credential> {
        self.credentials
            .iter()
            .find(|credential| credential.provider == provider)
    }

    fn validate_destination(&self, config: &EmbeddingConfig) -> Result<()> {
        let url = embedding_url(config.provider, &config.endpoint)?;
        if let Some(credential) = self.credential(config.provider) {
            ensure!(
                url.origin().ascii_serialization() == credential.origin,
                "embedding credential destination differs from its environment-pinned origin"
            );
            let secure = url.scheme() == "https";
            #[cfg(test)]
            let secure = secure || (self.allow_test_loopback_credentials && literal_loopback(&url));
            ensure!(secure, "embedding credentials require HTTPS");
        }
        Ok(())
    }

    async fn request(
        &self,
        config: &EmbeddingConfig,
        method: Method,
        url: Url,
        body: Option<Value>,
    ) -> Result<Value> {
        self.validate_destination(config)?;
        ensure!(
            url.origin() == embedding_url(config.provider, &config.endpoint)?.origin(),
            "embedding request changed origin"
        );
        let mut request = self
            .client
            .request(method, url)
            .timeout(Duration::from_millis(config.timeout_ms));
        if let Some(credential) = self.credential(config.provider) {
            request = request.bearer_auth(&credential.key);
        } else {
            ensure!(
                config.provider != EmbeddingProvider::Voyage,
                "Voyage embedding credentials are unavailable"
            );
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .context("embedding provider request failed")?;
        ensure!(
            response.status().is_success(),
            "embedding provider returned HTTP {}",
            response.status().as_u16()
        );
        ensure!(
            response
                .content_length()
                .is_none_or(|size| size <= MAX_RESPONSE_BYTES as u64),
            "embedding provider response is too large"
        );
        let mut bytes = Vec::new();
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.context("embedding provider response body failed")?;
            ensure!(
                bytes.len().saturating_add(chunk.len()) <= MAX_RESPONSE_BYTES,
                "embedding provider response is too large"
            );
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).context("embedding provider returned invalid JSON")
    }
}

fn bounded_client() -> Result<Client> {
    Ok(Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(30))
        .no_proxy()
        .build()?)
}

fn credential_from_environment(
    provider: EmbeddingProvider,
    key: String,
    fallback: &str,
) -> Result<Credential> {
    let specific = match provider {
        EmbeddingProvider::Voyage => "EMBEDDING_VOYAGE_CREDENTIAL_ORIGIN",
        EmbeddingProvider::Ollama => "EMBEDDING_OLLAMA_CREDENTIAL_ORIGIN",
        EmbeddingProvider::OpenaiCompatible => "EMBEDDING_OPENAI_CREDENTIAL_ORIGIN",
    };
    let selected_provider = std::env::var("EMBEDDING_PROVIDER").unwrap_or_else(|_| "voyage".into());
    let same_provider = serde_json::from_value::<EmbeddingProvider>(json!(selected_provider)).ok()
        == Some(provider);
    let endpoint = std::env::var(specific)
        .ok()
        .or_else(|| {
            same_provider
                .then(|| std::env::var("EMBEDDING_ENDPOINT").ok())
                .flatten()
        })
        .unwrap_or_else(|| fallback.to_owned());
    let origin = checked_url(&endpoint)?.origin().ascii_serialization();
    Ok(Credential {
        provider,
        key,
        origin,
    })
}

/// Shared by the legacy Voyage consumer before attaching its bearer token.
pub fn validate_voyage_credential_endpoint(endpoint: &str, key: &str) -> Result<()> {
    let credential = credential_from_environment(
        EmbeddingProvider::Voyage,
        key.to_owned(),
        &EmbeddingModelPlugin::voyage_url_for_key(key),
    )?;
    let url = embedding_url(EmbeddingProvider::Voyage, endpoint)?;
    ensure!(
        url.scheme() == "https" && url.origin().ascii_serialization() == credential.origin,
        "Voyage endpoint is not the credential's HTTPS origin"
    );
    Ok(())
}

fn checked_url(endpoint: &str) -> Result<Url> {
    ensure!(endpoint.len() <= 2048, "embedding endpoint is too long");
    let url = Url::parse(endpoint).context("invalid embedding endpoint URL")?;
    ensure!(
        matches!(url.scheme(), "https" | "http") && url.host_str().is_some(),
        "embedding endpoint must be HTTP(S)"
    );
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "embedding endpoints cannot contain credentials, queries, or fragments"
    );
    if url.scheme() == "http" {
        let local = url
            .host_str()
            .and_then(|host| {
                host.trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .ok()
            })
            .is_some_and(|ip| match ip {
                std::net::IpAddr::V4(ip) => ip.is_loopback() || ip.is_private(),
                std::net::IpAddr::V6(ip) => ip.is_loopback() || ip.is_unique_local(),
            });
        ensure!(
            local || url.host_str() == Some("localhost"),
            "unencrypted embedding endpoints require a private or loopback host"
        );
    }
    Ok(url)
}

#[cfg(test)]
fn literal_loopback(url: &Url) -> bool {
    url.host_str()
        .and_then(|host| {
            host.trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .ok()
        })
        .is_some_and(|ip| ip.is_loopback())
}

fn embedding_url(provider: EmbeddingProvider, endpoint: &str) -> Result<Url> {
    let mut url = checked_url(endpoint)?;
    let path = url.path().trim_end_matches('/');
    let path = match provider {
        EmbeddingProvider::Voyage => match path {
            "" | "/" => "/v1/embeddings".into(),
            "/v1" => "/v1/embeddings".into(),
            "/v1/embeddings" => path.into(),
            _ => bail!("Voyage endpoint must use /v1/embeddings"),
        },
        EmbeddingProvider::Ollama => match path {
            "" | "/" => "/api/embed".into(),
            "/api" => "/api/embed".into(),
            "/api/embed" => path.into(),
            _ => bail!("Ollama endpoint must use /api/embed"),
        },
        EmbeddingProvider::OpenaiCompatible => {
            if path.is_empty() {
                "/v1/embeddings".into()
            } else if path.ends_with("/embeddings") {
                path.into()
            } else {
                format!("{path}/embeddings")
            }
        }
    };
    url.set_path(&path);
    Ok(url)
}

struct EmbeddingService {
    store: Arc<RuntimeDocuments>,
    bootstrap: EmbeddingConfig,
    http: EmbeddingHttp,
}

impl EmbeddingService {
    async fn config(&self) -> Result<ConfigOutput> {
        let row = self.store.read(PLUGIN_ID).await?;
        let (config, revision) = match row {
            Some(row) => (
                serde_json::from_value::<EmbeddingConfig>(row.document)?.normalized()?,
                row.revision,
            ),
            None => (self.bootstrap.clone(), 0),
        };
        self.http.validate_destination(&config)?;
        Ok(ConfigOutput { config, revision })
    }

    async fn dispatch(&self, method: &str, args: &Value) -> Result<Value> {
        match method {
            "get_config" => {
                serde_json::from_value::<EmptyInput>(args.clone())?;
                Ok(serde_json::to_value(self.config().await?)?)
            }
            "set_model" | "update_config" => {
                let new_config = if method == "update_config" {
                    Some(
                        serde_json::from_value::<UpdateConfigInput>(args.clone())?
                            .config
                            .normalized()?,
                    )
                } else {
                    None
                };
                let model = if method == "set_model" {
                    let input: SetModelInput = serde_json::from_value(args.clone())?;
                    ensure!(
                        !input.model_id.trim().is_empty(),
                        "embedding model_id cannot be empty"
                    );
                    Some(input.model_id)
                } else {
                    None
                };
                if let Some(config) = &new_config {
                    self.http.validate_destination(config)?;
                }
                let bootstrap = self.bootstrap.clone();
                let http = self.http.clone();
                // Endpoint/provider changes are validated before entering the
                // store. set_model preserves the existing endpoint atomically.
                let row = self
                    .store
                    .update(PLUGIN_ID, move |previous| {
                        let mut config = match new_config {
                            Some(config) => config,
                            None => previous
                                .map(serde_json::from_value::<EmbeddingConfig>)
                                .transpose()?
                                .unwrap_or(bootstrap),
                        };
                        if let Some(model) = model {
                            config.model_id = model;
                        }
                        let config = config.normalized()?;
                        http.validate_destination(&config)?;
                        Ok(serde_json::to_value(config)?)
                    })
                    .await?;
                Ok(serde_json::to_value(ConfigOutput {
                    config: serde_json::from_value(row.document)?,
                    revision: row.revision,
                })?)
            }
            "embed" => {
                let input: EmbedInput = serde_json::from_value(args.clone())?;
                let config = self.config().await?;
                Ok(serde_json::to_value(self.embed(config, input).await?)?)
            }
            "list_models" => {
                serde_json::from_value::<EmptyInput>(args.clone())?;
                Ok(serde_json::to_value(
                    self.list_models(self.config().await?).await?,
                )?)
            }
            _ => bail!("unknown embedding_model method: {method}"),
        }
    }

    async fn embed(&self, current: ConfigOutput, input: EmbedInput) -> Result<EmbedOutput> {
        let config = current.config;
        ensure!(
            !config.model_id.is_empty(),
            "no embedding model is configured"
        );
        ensure!(
            (1..=1000).contains(&input.texts.len()),
            "embed requires 1..=1000 texts"
        );
        ensure!(
            input.texts.iter().all(|text| !text.is_empty()),
            "embedding texts cannot be empty"
        );
        let bytes = input
            .texts
            .iter()
            .try_fold(0usize, |total, text| total.checked_add(text.len()))
            .context("embedding input is too large")?;
        ensure!(bytes <= 2 * 1024 * 1024, "embedding input exceeds 2 MiB");
        ensure!(
            input.texts.len().saturating_mul(config.dimensions as usize) <= 1_000_000,
            "embedding batch output exceeds one million dimensions"
        );
        if config.provider == EmbeddingProvider::OpenaiCompatible {
            ensure!(
                !input.truncate,
                "OpenAI-compatible embeddings do not support native truncation"
            );
        }
        let mut body = json!({"model": config.model_id, "input": input.texts});
        match config.provider {
            EmbeddingProvider::Voyage => {
                body["output_dtype"] = json!("float");
                body["encoding_format"] = Value::Null;
                body["truncation"] = json!(input.truncate);
                if let Some(input_type) = input.input_type {
                    body["input_type"] = serde_json::to_value(input_type)?;
                }
                if config.request_dimensions {
                    body["output_dimension"] = json!(config.dimensions);
                }
            }
            EmbeddingProvider::Ollama => {
                body["truncate"] = json!(input.truncate);
                if config.request_dimensions {
                    body["dimensions"] = json!(config.dimensions);
                }
            }
            EmbeddingProvider::OpenaiCompatible => {
                body["encoding_format"] = json!("float");
                if config.request_dimensions {
                    body["dimensions"] = json!(config.dimensions);
                }
            }
        }
        let response = self
            .http
            .request(
                &config,
                Method::POST,
                embedding_url(config.provider, &config.endpoint)?,
                Some(body),
            )
            .await?;
        let vectors = decode_vectors(
            config.provider,
            &response,
            input.texts.len(),
            config.dimensions as usize,
        )?;
        Ok(EmbedOutput {
            vectors,
            dimensions: config.dimensions,
            model: config.model_id,
            provider_model: response
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_owned),
            provider: config.provider,
            config_revision: current.revision,
        })
    }

    async fn list_models(&self, current: ConfigOutput) -> Result<ListModelsOutput> {
        let config = current.config;
        if config.provider == EmbeddingProvider::Voyage {
            return Ok(ListModelsOutput {
                models: if config.model_id.is_empty() {
                    vec![]
                } else {
                    vec![config.model_id]
                },
                provider: config.provider,
                source: "configured".into(),
                enumerated: false,
                embedding_support_verified: false,
                config_revision: current.revision,
            });
        }
        let mut url = embedding_url(config.provider, &config.endpoint)?;
        let (collection, field) = if config.provider == EmbeddingProvider::Ollama {
            url.set_path("/api/tags");
            ("models", "name")
        } else {
            let path = url
                .path()
                .strip_suffix("embeddings")
                .context("invalid embedding endpoint")?
                .to_owned();
            url.set_path(&format!("{path}models"));
            ("data", "id")
        };
        let value = self.http.request(&config, Method::GET, url, None).await?;
        let rows = value[collection]
            .as_array()
            .context("provider model list is missing")?;
        ensure!(rows.len() <= 20_000, "provider model list is too large");
        let mut models = BTreeSet::new();
        for row in rows {
            let id = row[field]
                .as_str()
                .filter(|id| !id.is_empty() && id.len() <= 512)
                .context("provider model list contains an invalid identifier")?;
            models.insert(id.to_owned());
        }
        Ok(ListModelsOutput {
            models: models.into_iter().collect(),
            provider: config.provider,
            source: "provider".into(),
            enumerated: true,
            embedding_support_verified: false,
            config_revision: current.revision,
        })
    }
}

fn decode_vectors(
    provider: EmbeddingProvider,
    response: &Value,
    count: usize,
    dimensions: usize,
) -> Result<Vec<Vec<f32>>> {
    let arrays = if provider == EmbeddingProvider::Ollama {
        response["embeddings"]
            .as_array()
            .context("provider embeddings are missing")?
            .iter()
            .collect::<Vec<_>>()
    } else {
        let data = response["data"]
            .as_array()
            .context("provider embedding data is missing")?;
        ensure!(
            data.len() == count,
            "provider returned the wrong embedding cardinality"
        );
        let mut indexed = vec![None; count];
        for row in data {
            let index = row["index"]
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .context("provider embedding index is invalid")?;
            ensure!(
                index < count && indexed[index].is_none(),
                "provider embedding indices are duplicated or out of range"
            );
            indexed[index] = Some(&row["embedding"]);
        }
        indexed
            .into_iter()
            .map(|value| value.context("provider omitted an embedding index"))
            .collect::<Result<Vec<_>>>()?
    };
    ensure!(
        arrays.len() == count,
        "provider returned the wrong embedding cardinality"
    );
    arrays
        .into_iter()
        .map(|value| {
            let array = value
                .as_array()
                .context("provider embedding must be a numeric array")?;
            ensure!(
                array.len() == dimensions,
                "provider embedding dimension does not match configuration"
            );
            array
                .iter()
                .map(|value| {
                    let number = value
                        .as_f64()
                        .context("provider embedding contains a nonnumeric value")?;
                    ensure!(
                        number.is_finite() && (number as f32).is_finite(),
                        "provider embedding contains an out-of-range number"
                    );
                    Ok(number as f32)
                })
                .collect()
        })
        .collect()
}

pub async fn dispatch_embedding_model_method(method: &str, args: &Value) -> Result<Value> {
    EmbeddingService {
        store: runtime_documents().await?,
        bootstrap: EmbeddingConfig::from_environment()?,
        http: EmbeddingHttp::from_environment()?,
    }
    .dispatch(method, args)
    .await
}

/// Nonsecret publication payload for the bridge after a committed config write.
pub async fn embedding_model_state() -> Result<EmbeddingModelState> {
    let service = EmbeddingService {
        store: runtime_documents().await?,
        bootstrap: EmbeddingConfig::from_environment()?,
        http: EmbeddingHttp::from_environment()?,
    };
    let current = service.config().await?;
    Ok(current.config.state(current.revision))
}

/// Rebuild the public projection from a durable config row without contacting
/// a provider or discovering credentials. Provider availability stays unprobed.
pub fn restored_embedding_state(
    row: &crate::state_plugins::runtime_documents::RuntimeDocument,
) -> Result<EmbeddingModelState> {
    Ok(
        serde_json::from_value::<EmbeddingConfig>(row.document.clone())?
            .normalized()?
            .state(row.revision),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct Response {
        status: u16,
        body: String,
        location: Option<String>,
        delay_ms: u64,
    }

    impl Response {
        fn json(body: Value) -> Self {
            Self {
                status: 200,
                body: body.to_string(),
                location: None,
                delay_ms: 0,
            }
        }
    }

    #[derive(Debug)]
    struct Request {
        method: String,
        path: String,
        headers: String,
        body: Value,
    }

    async fn fixture(
        provider: EmbeddingProvider,
        responses: Vec<Response>,
    ) -> (EmbeddingService, tokio::task::JoinHandle<Vec<Request>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let endpoint = embedding_url(provider, &origin).unwrap().to_string();
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) =
                    tokio::time::timeout(Duration::from_secs(3), listener.accept())
                        .await
                        .unwrap()
                        .unwrap();
                let mut bytes = Vec::new();
                let header_end = loop {
                    let mut chunk = [0u8; 4096];
                    let count = stream.read(&mut chunk).await.unwrap();
                    assert!(count > 0, "fixture received an incomplete request");
                    bytes.extend_from_slice(&chunk[..count]);
                    if let Some(offset) = bytes.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        break offset + 4;
                    }
                    assert!(bytes.len() < 64 * 1024);
                };
                let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                while bytes.len() < header_end + length {
                    let mut chunk = [0u8; 4096];
                    let count = stream.read(&mut chunk).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                }
                let mut request_line = headers.lines().next().unwrap().split_whitespace();
                requests.push(Request {
                    method: request_line.next().unwrap().to_owned(),
                    path: request_line.next().unwrap().to_owned(),
                    body: if length == 0 {
                        Value::Null
                    } else {
                        serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap()
                    },
                    headers,
                });
                if response.delay_ms > 0 {
                    tokio::time::sleep(Duration::from_millis(response.delay_ms)).await;
                }
                let redirect = response
                    .location
                    .map(|url| format!("Location: {url}\r\n"))
                    .unwrap_or_default();
                let encoded = format!("HTTP/1.1 {} fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{redirect}Connection: close\r\n\r\n{}", response.status, response.body.len(), response.body);
                // The timeout fixture intentionally closes its client first.
                let _ = stream.write_all(encoded.as_bytes()).await;
            }
            requests
        });
        let store = Arc::new(RuntimeDocuments::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let service = EmbeddingService {
            store,
            bootstrap: EmbeddingConfig {
                provider,
                model_id: "fixture-old".into(),
                endpoint,
                dimensions: 2,
                request_dimensions: true,
                timeout_ms: 2000,
            },
            http: EmbeddingHttp {
                client: bounded_client().unwrap(),
                credentials: vec![Credential {
                    provider,
                    key: "fixture-secret-key".into(),
                    origin,
                }],
                allow_test_loopback_credentials: true,
            },
        };
        (service, server)
    }

    fn validate_output(method: &str, output: &Value) {
        let schema = super::super::embedding_model_schema();
        let output_schema =
            serde_json::to_value(schema.methods[method].returns.as_ref().unwrap()).unwrap();
        let validator = jsonschema::Validator::new(&output_schema).unwrap();
        if let Err(error) = validator.validate(output) {
            panic!("{method} returned invalid output: {error}");
        }
        assert!(!output.to_string().contains("fixture-secret-key"));
    }

    #[tokio::test]
    async fn all_five_methods_use_committed_voyage_configuration_and_real_vectors() {
        let (service, server) = fixture(EmbeddingProvider::Voyage, vec![Response::json(json!({
            "model":"provider-reported-model",
            "data":[{"index":1,"embedding":[4.0,5.0,6.0]}, {"index":0,"embedding":[1.0,2.0,3.0]}]
        }))]).await;
        let config = service.dispatch("get_config", &json!({})).await.unwrap();
        validate_output("get_config", &config);
        assert_eq!(config["revision"], 0);
        let models = service.dispatch("list_models", &json!({})).await.unwrap();
        validate_output("list_models", &models);
        assert_eq!(models["models"], json!(["fixture-old"]));
        assert_eq!(models["enumerated"], false);

        let selection = service
            .dispatch("set_model", &json!({"model_id":"fixture-new"}))
            .await
            .unwrap();
        validate_output("set_model", &selection);
        assert_eq!(selection["revision"], 1);
        let mut changed =
            serde_json::from_value::<EmbeddingConfig>(selection["config"].clone()).unwrap();
        changed.dimensions = 3;
        let updated = service
            .dispatch("update_config", &json!({"config":changed}))
            .await
            .unwrap();
        validate_output("update_config", &updated);
        assert_eq!(updated["revision"], 2);
        let embedded = service
            .dispatch(
                "embed",
                &json!({"texts":["first","second"],"input_type":"query"}),
            )
            .await
            .unwrap();
        validate_output("embed", &embedded);
        assert_eq!(
            embedded["vectors"],
            json!([[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]])
        );
        assert_eq!(embedded["config_revision"], 2);
        assert_eq!(embedded["model"], "fixture-new");
        let requests = server.await.unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/v1/embeddings");
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].body["model"], "fixture-new");
        assert_eq!(requests[0].body["output_dimension"], 3);
        assert_eq!(requests[0].body["output_dtype"], "float");
        assert_eq!(requests[0].body["encoding_format"], Value::Null);
        assert_eq!(requests[0].body["input_type"], "query");
        assert!(requests[0]
            .headers
            .to_lowercase()
            .contains("authorization: bearer fixture-secret-key"));
        let stored = service.store.read(PLUGIN_ID).await.unwrap().unwrap();
        assert_eq!(stored.revision, 2);
        assert!(!stored.document.to_string().contains("fixture-secret-key"));
    }

    #[tokio::test]
    async fn ollama_uses_native_tags_and_embed_shapes() {
        let (service, server) = fixture(
            EmbeddingProvider::Ollama,
            vec![
                Response::json(
                    json!({"models":[{"name":"z-model"},{"name":"a-model"},{"name":"z-model"}]}),
                ),
                Response::json(json!({"model":"fixture-old:latest", "embeddings":[[0.25,0.75]]})),
            ],
        )
        .await;
        let models = service.dispatch("list_models", &json!({})).await.unwrap();
        validate_output("list_models", &models);
        assert_eq!(models["models"], json!(["a-model", "z-model"]));
        assert_eq!(models["embedding_support_verified"], false);
        let embedded = service
            .dispatch("embed", &json!({"texts":["text"], "input_type":"document"}))
            .await
            .unwrap();
        validate_output("embed", &embedded);
        assert_eq!(embedded["vectors"], json!([[0.25, 0.75]]));
        let requests = server.await.unwrap();
        assert_eq!(requests[0].method, "GET");
        assert_eq!(requests[0].path, "/api/tags");
        assert_eq!(requests[1].path, "/api/embed");
        assert_eq!(requests[1].body["input"], json!(["text"]));
        assert_eq!(requests[1].body["truncate"], false);
        assert_eq!(requests[1].body["dimensions"], 2);
        assert!(requests[1].body.get("input_type").is_none());
    }

    #[tokio::test]
    async fn openai_compatible_models_and_embedding_support_optional_dimensions() {
        let (service, server) = fixture(
            EmbeddingProvider::OpenaiCompatible,
            vec![
                Response::json(json!({"data":[{"id":"embedding-model"},{"id":"chat-model"}]})),
                Response::json(
                    json!({"model":"fixture-old", "data":[{"index":0,"embedding":[0.5,0.5]}]}),
                ),
            ],
        )
        .await;
        let models = service.dispatch("list_models", &json!({})).await.unwrap();
        assert_eq!(models["models"], json!(["chat-model", "embedding-model"]));
        let mut changed = service.bootstrap.clone();
        changed.request_dimensions = false;
        service
            .dispatch("update_config", &json!({"config":changed}))
            .await
            .unwrap();
        let embedded = service
            .dispatch("embed", &json!({"texts":["text"], "input_type":"query"}))
            .await
            .unwrap();
        validate_output("embed", &embedded);
        assert_eq!(embedded["config_revision"], 1);
        let requests = server.await.unwrap();
        assert_eq!(requests[0].path, "/v1/models");
        assert_eq!(requests[1].path, "/v1/embeddings");
        assert_eq!(requests[1].body["encoding_format"], "float");
        assert!(requests[1].body.get("dimensions").is_none());
        assert!(requests[1].body.get("truncation").is_none());
        assert!(requests[1].body.get("input_type").is_none());
    }

    #[tokio::test]
    async fn credential_origin_and_config_validation_fail_before_persistence() {
        let (service, server) = fixture(EmbeddingProvider::Voyage, vec![]).await;
        let mut changed = service.bootstrap.clone();
        changed.endpoint = "https://different-origin.example/v1/embeddings".into();
        assert!(service
            .dispatch("update_config", &json!({"config":changed}))
            .await
            .is_err());
        assert!(service
            .dispatch("set_model", &json!({"model_id":"  "}))
            .await
            .is_err());
        let mut config = serde_json::to_value(&service.bootstrap).unwrap();
        config["api_key"] = json!("must-not-store");
        assert!(service
            .dispatch("update_config", &json!({"config":config}))
            .await
            .is_err());
        assert!(service.store.read(PLUGIN_ID).await.unwrap().is_none());
        assert!(server.await.unwrap().is_empty());
        let mut production_policy = service.http.clone();
        production_policy.allow_test_loopback_credentials = false;
        assert!(production_policy
            .validate_destination(&service.bootstrap)
            .is_err());
        for endpoint in [
            "https://user:password@example.com/v1/embeddings",
            "https://example.com/v1/embeddings?key=private",
            "https://example.com/v1/embeddings#fragment",
            "http://example.com/v1/embeddings",
            "file:///etc/passwd",
        ] {
            assert!(checked_url(endpoint).is_err());
        }
    }

    #[tokio::test]
    async fn redirects_and_provider_errors_do_not_forward_credentials_or_echo_bodies() {
        for status in [307, 401, 500] {
            let mut response =
                Response::json(json!({"error":"fixture-secret-key should never be returned"}));
            response.status = status;
            response.location = Some("http://127.0.0.1:1/stolen".into());
            let (service, server) = fixture(EmbeddingProvider::Voyage, vec![response]).await;
            let error = service
                .dispatch("embed", &json!({"texts":["text"]}))
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains(&format!("HTTP {status}")));
            assert!(!error.contains("fixture-secret-key"));
            assert_eq!(server.await.unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn bounded_timeout_and_malformed_json_are_provider_errors() {
        let mut slow = Response::json(json!({"data":[{"index":0,"embedding":[1.0,2.0]}]}));
        slow.delay_ms = 250;
        let (mut service, server) = fixture(EmbeddingProvider::Voyage, vec![slow]).await;
        service.bootstrap.timeout_ms = 100;
        assert!(service
            .dispatch("embed", &json!({"texts":["text"]}))
            .await
            .is_err());
        server.await.unwrap();
        let (service, server) = fixture(
            EmbeddingProvider::Voyage,
            vec![Response {
                status: 200,
                body: "not JSON".into(),
                location: None,
                delay_ms: 0,
            }],
        )
        .await;
        assert!(service
            .dispatch("embed", &json!({"texts":["text"]}))
            .await
            .unwrap_err()
            .to_string()
            .contains("invalid JSON"));
        server.await.unwrap();
    }

    #[test]
    fn vectors_require_exact_cardinality_dimensions_indices_and_finite_numbers() {
        for response in [
            json!({"data":[]}),
            json!({"data":[{"index":0,"embedding":[1.0]}]}),
            json!({"data":[{"index":0,"embedding":[1.0,"bad"]}]}),
            json!({"data":[{"index":0,"embedding":[1.0,1e100]}]}),
            json!({"data":[{"index":1,"embedding":[1.0,2.0]}]}),
            json!({"data":[{"embedding":[1.0,2.0]}]}),
        ] {
            assert!(
                decode_vectors(EmbeddingProvider::Voyage, &response, 1, 2).is_err(),
                "accepted {response}"
            );
        }
        assert!(decode_vectors(
            EmbeddingProvider::OpenaiCompatible,
            &json!({"data":[{"index":0,"embedding":[1,2]},{"index":0,"embedding":[3,4]}]}),
            2,
            2
        )
        .is_err());
        assert!(decode_vectors(
            EmbeddingProvider::Ollama,
            &json!({"embeddings":[[1,2],[3,4]]}),
            1,
            2
        )
        .is_err());
    }

    #[tokio::test]
    async fn unsupported_protocol_options_and_empty_inputs_do_not_call_provider() {
        let (service, server) = fixture(EmbeddingProvider::OpenaiCompatible, vec![]).await;
        for (args, reason) in [
            (json!({"texts":[]}), "1..=1000"),
            (json!({"texts":[""]}), "cannot be empty"),
            (
                json!({"texts":["text"],"truncate":true}),
                "do not support native truncation",
            ),
        ] {
            let error = service
                .dispatch("embed", &args)
                .await
                .unwrap_err()
                .to_string();
            assert!(error.contains(reason), "unexpected rejection: {error}");
        }
        assert!(server.await.unwrap().is_empty());
    }
}
