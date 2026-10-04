//! Loopback adapter for the real tched_router daemon.
//!
//! The upstream binary owns provider/model discovery, selection, persistence and
//! chat execution. The bridge remains the capability and audit boundary; it
//! does not implement another model provider.

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::Mutex;

use anyhow::{Context, anyhow};
use futures::StreamExt;
use op_plugins::state_plugins::common::llm_projection::{ModelRoute, Provider};
use op_plugins::state_plugins::tched_router::{
    ChatInput, ChatOutput, ConfigInitInput, ConfigMigrateInput, ConfigPatchInput,
    ConfigPatchOperation, ConfigSetInput, MAX_CHAT_REQUEST_BYTES, MAX_CHAT_RESPONSE_BYTES,
    TchedRouterState,
};
use reqwest::RequestBuilder;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:8084";
const DEFAULT_AGENT_ALIAS: &str = "dashboard";
const CONFIG_TIMEOUT: Duration = Duration::from_secs(15);
const CHAT_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_CONFIG_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_CONFIG_LIST_ENTRIES: usize = 4096;
const MAX_CONFIG_PATH_BYTES: usize = 512;
const MAX_CONFIG_PATCH_OPERATIONS: usize = 64;
const MAX_CONFIG_VALUE_BYTES: usize = 256 * 1024;
const MAX_GATEWAY_PATH_BYTES: usize = 2048;
const MAX_GATEWAY_QUERY_PAIRS: usize = 32;
const MAX_GATEWAY_QUERY_KEY_BYTES: usize = 64;
const MAX_GATEWAY_QUERY_VALUE_BYTES: usize = 4096;
const MAX_GATEWAY_REQUEST_BYTES: usize = MAX_CHAT_REQUEST_BYTES;
static RUNTIME_SWITCH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
pub(crate) enum GatewayMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum GatewayProfile {
    Config,
    LongRunning,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct GatewayJsonRequest<'a> {
    pub(crate) method: GatewayMethod,
    pub(crate) relative_path: &'a str,
    pub(crate) query: &'a [(&'a str, &'a str)],
    pub(crate) body: Option<&'a Value>,
    pub(crate) operation: &'static str,
    pub(crate) profile: GatewayProfile,
    pub(crate) mutation: bool,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum GatewayRequestError {
    #[error("native gateway request rejected before an effect: {0}")]
    BeforeEffect(String),
    #[error("native gateway rejected the request with HTTP {status}: {detail}")]
    ProviderRejected { status: u16, detail: String },
    #[error("native gateway returned an invalid response: {0}")]
    InvalidResponse(String),
    #[error("native gateway mutation outcome is indeterminate: {0}")]
    IndeterminateMutation(String),
}

#[derive(Clone)]
pub(crate) struct TchedRouterRuntimeClient {
    http: Option<reqwest::Client>,
    client_error: Option<String>,
    endpoint: String,
    endpoint_error: Option<String>,
    agent_alias: String,
    token: Option<String>,
    sealed_identity: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ConfiguredProvider {
    family: String,
    reference: String,
    selected_model: String,
}

fn matches_provider_selector(selector: &str, observed: &ConfiguredProvider) -> bool {
    selector.trim().is_empty()
        || selector.eq_ignore_ascii_case(&observed.family)
        || selector.eq_ignore_ascii_case(&observed.reference)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuntimeSelection {
    pub(crate) provider: String,
    pub(crate) model: String,
}

#[derive(Debug)]
pub(crate) struct NativeConfigMutation {
    /// Exact safe method result returned to the canonical caller.
    pub(crate) result: Value,
    /// Safe authoritative readback used by MutationEngine if this operation
    /// represents a schema-owned state field.
    pub(crate) readback: Value,
}

#[derive(Debug)]
pub(crate) struct NativeConfigObservation {
    /// Exact masked native configuration bounded and validated by the pinned
    /// secret policy. This is the only native configuration image committed.
    pub(crate) native_config: Value,
    /// Selection and catalog fields observed while that image remained stable.
    pub(crate) represented: Value,
    pub(crate) state: TchedRouterState,
}

#[derive(Debug)]
pub(crate) struct CatalogBindingObservation {
    pub(crate) native_config: Value,
    pub(crate) binding: op_plugins::state_plugins::tched_router::ResolveCatalogModelBindingOutput,
}

impl NativeConfigObservation {
    pub(crate) fn into_state_and_fields(self) -> anyhow::Result<(TchedRouterState, Value)> {
        let mut fields = self
            .represented
            .as_object()
            .cloned()
            .context("native router fields must be an object")?;
        fields.insert("native_config".into(), self.native_config);
        Ok((self.state, Value::Object(fields)))
    }
}

/// Exact secret classification derived from the manifest-pinned v0.8.4 config
/// contract (`x-secret`, including resolved refs/dynamic-map value schemas).
/// An unresolved path is an error so response handling fails closed.
pub(crate) trait ConfigSecretPolicy: Send + Sync {
    fn is_secret(&self, path: &str) -> anyhow::Result<bool>;
    fn redact_config(&self, config: &mut Value) -> anyhow::Result<()>;
}

#[derive(Debug, Deserialize)]
struct HealthResponse {
    status: String,
}

#[derive(Debug, Deserialize)]
struct PropResponse {
    value: Value,
}

#[derive(Debug, Deserialize)]
struct ConfigListResponse {
    entries: Vec<ConfigListEntry>,
}

#[derive(Debug, Deserialize)]
struct ConfigListEntry {
    path: String,
    #[serde(default)]
    value: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ModelCatalogResponse {
    models: Vec<String>,
    live: bool,
}

#[derive(Debug, Deserialize)]
struct NativeMigrateResponse {
    migrated: bool,
    #[allow(dead_code)]
    backup_path: Option<String>,
    schema_version: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeMapKeyResponse {
    path: String,
    key: String,
    created: bool,
    #[serde(default)]
    warnings: Option<Vec<String>>,
}

impl TchedRouterRuntimeClient {
    /// Serialize upstream changes through the caller's projection commit.
    pub(crate) async fn selection_guard() -> tokio::sync::MutexGuard<'static, ()> {
        RUNTIME_SWITCH_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .await
    }

    pub(crate) fn from_env() -> Self {
        let endpoint = std::env::var("TCHED_ROUTER_RUNTIME_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_ENDPOINT.to_string());
        let agent_alias = std::env::var("TCHED_ROUTER_RUNTIME_AGENT")
            .unwrap_or_else(|_| DEFAULT_AGENT_ALIAS.to_string());
        let token = std::env::var("TCHED_ROUTER_RUNTIME_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty());
        Self::new(endpoint, agent_alias, token)
    }

    /// Build a client for a dedicated native agent alias while retaining the
    /// endpoint and token configured for the bridge process.
    pub(crate) fn for_agent_alias(agent_alias: &str) -> Self {
        let mut client = Self::from_env();
        client.agent_alias = agent_alias.to_owned();
        client
    }

    pub(crate) fn new(endpoint: String, agent_alias: String, token: Option<String>) -> Self {
        let endpoint = endpoint.trim_end_matches('/').to_string();
        let endpoint_error = validate_loopback_endpoint(&endpoint).err();
        let (http, client_error) = match reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
        {
            Ok(client) => (Some(client), None),
            Err(error) => (
                None,
                Some(format!(
                    "failed to initialize isolated tched_router HTTP client: {error}"
                )),
            ),
        };
        Self {
            http,
            client_error,
            endpoint,
            endpoint_error,
            agent_alias,
            token,
            sealed_identity: None,
        }
    }

    pub(crate) fn with_sealed_identity(mut self, sealed_identity: Option<&str>) -> Self {
        self.sealed_identity = sealed_identity.map(ToOwned::to_owned);
        self
    }

    fn http(&self) -> anyhow::Result<&reqwest::Client> {
        if let Some(error) = &self.endpoint_error {
            return Err(anyhow!(error.clone()));
        }
        self.http.as_ref().ok_or_else(|| {
            anyhow!(
                self.client_error
                    .clone()
                    .unwrap_or_else(|| "tched_router HTTP client is unavailable".to_string())
            )
        })
    }

    /// Construct the private stateless generation transport from this same
    /// runtime owner's endpoint and credential. The native adapter requires a
    /// versioned stateless ACK before it releases the prompt; old gateways fail
    /// closed. No separate environment-selected provider/client is introduced.
    pub(crate) fn spawn_stateless_generation(
        &self,
        input: op_plugins::state_plugins::tched_router::generation::StartGenerationInput,
        native_session_id: String,
        before_prompt: std::sync::Arc<
            dyn crate::tched_router_generation_native::NativeGenerationAdmission,
        >,
    ) -> anyhow::Result<crate::tched_router_generation_native::NativeGenerationTask> {
        self.spawn_stateless_generation_with_expectations(
            input,
            native_session_id,
            before_prompt,
            None,
            None,
        )
    }

    fn spawn_stateless_generation_with_expectations(
        &self,
        input: op_plugins::state_plugins::tched_router::generation::StartGenerationInput,
        native_session_id: String,
        before_prompt: std::sync::Arc<
            dyn crate::tched_router_generation_native::NativeGenerationAdmission,
        >,
        expected_provider: Option<String>,
        expected_model: Option<String>,
    ) -> anyhow::Result<crate::tched_router_generation_native::NativeGenerationTask> {
        // Reuse the configured client's strict endpoint validation, and require
        // an explicit credential rather than anonymous shared-gateway access.
        self.http()?;
        anyhow::ensure!(
            self.token.as_ref().is_some_and(|token| !token.is_empty()),
            "authenticated stateless generation requires a configured runtime token"
        );
        input.validate()?;
        crate::tched_router_generation_native::NativeGenerationTask::spawn(
            crate::tched_router_generation_native::NativeGenerationRequest {
                endpoint: self.endpoint.clone(),
                bearer_token: self.token.clone(),
                agent_alias: input.agent_alias,
                native_session_id,
                prompt: input.prompt,
                expected_provider,
                expected_model,
                sealed_identity: before_prompt.sealed_identity().map(ToOwned::to_owned),
                before_prompt,
            },
        )
        .map_err(|_| anyhow!("native stateless generation request could not be started"))
    }

    /// Complete one provider-only native generation turn. The caller holds the
    /// selection guard; the native adapter performs the final identity/schema
    /// admission immediately before releasing the prompt.
    pub(crate) async fn complete_under_guard(
        &self,
        input: ChatInput,
        before_prompt: std::sync::Arc<
            dyn crate::tched_router_generation_native::NativeGenerationAdmission,
        >,
    ) -> anyhow::Result<ChatOutput> {
        let client = self
            .clone()
            .with_sealed_identity(before_prompt.sealed_identity());
        let prompt = conversation_prompt(&input)?;
        let observed = client.current_provider().await?;
        if !matches_provider_selector(&input.provider, &observed) {
            return Err(anyhow!(
                "requested provider '{}' differs from native selection '{}'",
                input.provider,
                observed.family
            ));
        }
        if !input.model.trim().is_empty()
            && !input.model.eq_ignore_ascii_case(&observed.selected_model)
        {
            return Err(anyhow!(
                "requested model '{}' differs from native selection '{}'",
                input.model,
                observed.selected_model
            ));
        }
        // The stateless ACK uses the exact native provider reference (for
        // example `custom.salad` or `openrouter.accountability`), while the
        // public Chat contract may carry only the provider family.
        let expected_provider = Some(observed.reference.clone());
        let expected_model = Some(observed.selected_model.clone());
        let task_input =
            op_plugins::state_plugins::tched_router::generation::StartGenerationInput {
                agent_alias: self.agent_alias.clone(),
                prompt,
            };
        let mut task = client.spawn_stateless_generation_with_expectations(
            task_input,
            uuid::Uuid::new_v4().to_string(),
            before_prompt,
            expected_provider,
            expected_model,
        )?;
        while task.recv_event().await.is_some() {}
        let terminal = task
            .wait()
            .await
            .map_err(|error| anyhow!(error.to_string()))?;
        let done = match terminal {
            crate::tched_router_generation_native::NativeGenerationTerminal::Done(done) => done,
            crate::tched_router_generation_native::NativeGenerationTerminal::Cancelled => {
                return Err(anyhow!("native generation was cancelled"));
            }
        };
        let _native_provider = done
            .provider
            .context("native generation omitted provider")?;
        let model = done.model.context("native generation omitted model")?;
        Ok(ChatOutput {
            content: done.full_response,
            // Preserve the public Chat contract's provider-family shape while
            // the native adapter has already verified the exact reference.
            provider: observed.family,
            model,
            finish_reason: String::new(),
            usage: serde_json::json!({
                "input_tokens": done.usage.input_tokens,
                "output_tokens": done.usage.output_tokens,
                "tokens_used": done.usage.tokens_used,
                "cost_usd_micros": done.usage.cost_usd_micros,
            }),
        })
    }

    fn request(&self, request: RequestBuilder) -> anyhow::Result<RequestBuilder> {
        if let Some(error) = &self.endpoint_error {
            return Err(anyhow!(error.clone()));
        }
        let mut request = match &self.token {
            Some(token) => request.bearer_auth(token),
            None => request,
        };
        if let Some(identity) = self.sealed_identity.as_deref() {
            let mut header = reqwest::header::HeaderValue::from_str(identity)
                .map_err(|_| anyhow!("invalid canonical SID1 metadata"))?;
            header.set_sensitive(true);
            request = request.header("x-opdbus-sealed-id-bin", header);
        }
        Ok(request)
    }

    async fn send_json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        operation: &str,
        timeout: Duration,
        max_response_bytes: usize,
    ) -> anyhow::Result<T> {
        let response = self
            .request(request)?
            .timeout(timeout)
            .send()
            .await
            .with_context(|| format!("tched_router {operation} request failed"))?;
        let status = response.status();
        if operation == "chat"
            && self.sealed_identity.is_some()
            && response
                .headers()
                .get("x-opdbus-identity-authenticated")
                .and_then(|value| value.to_str().ok())
                != Some("SID1")
        {
            return Err(anyhow!(
                "tched_router {operation} did not authenticate the forwarded SID1"
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > max_response_bytes as u64)
        {
            return Err(anyhow!(
                "tched_router {operation} response exceeds {max_response_bytes} bytes"
            ));
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.with_context(|| format!("tched_router {operation} response body failed"))?;
            anyhow::ensure!(
                body.len().saturating_add(chunk.len()) <= max_response_bytes,
                "tched_router {operation} response exceeds {max_response_bytes} bytes"
            );
            body.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            let detail = String::from_utf8_lossy(&body);
            return Err(anyhow!(
                "tched_router {operation} returned {status}: {}",
                truncate_error(&detail)
            ));
        }
        serde_json::from_slice(&body)
            .with_context(|| format!("tched_router {operation} returned invalid JSON"))
    }

    async fn send_config_json<T: DeserializeOwned>(
        &self,
        request: RequestBuilder,
        operation: &str,
    ) -> anyhow::Result<T> {
        self.send_json(
            request,
            operation,
            CONFIG_TIMEOUT,
            MAX_CONFIG_RESPONSE_BYTES,
        )
        .await
    }

    /// Shared, bounded loopback transport for fixed native gateway adapters.
    /// It never retries. Adapters own route metadata and percent-encode every
    /// dynamic path segment before constructing `relative_path`.
    pub(crate) async fn send_gateway_json<T: DeserializeOwned>(
        &self,
        request: GatewayJsonRequest<'_>,
    ) -> Result<T, GatewayRequestError> {
        validate_gateway_request(&request)
            .map_err(|error| GatewayRequestError::BeforeEffect(error.to_string()))?;
        let http = self
            .http()
            .map_err(|error| GatewayRequestError::BeforeEffect(error.to_string()))?;
        let url = format!("{}{}", self.endpoint, request.relative_path);
        let mut builder = match request.method {
            GatewayMethod::Get => http.get(url),
            GatewayMethod::Post => http.post(url),
            GatewayMethod::Put => http.put(url),
            GatewayMethod::Patch => http.patch(url),
            GatewayMethod::Delete => http.delete(url),
        };
        if !request.query.is_empty() {
            builder = builder.query(request.query);
        }
        if let Some(body) = request.body {
            builder = builder.json(body);
        }
        builder = self
            .request(builder)
            .map_err(|error| GatewayRequestError::BeforeEffect(error.to_string()))?;
        let (timeout, max_response_bytes) = match request.profile {
            GatewayProfile::Config => (CONFIG_TIMEOUT, MAX_CONFIG_RESPONSE_BYTES),
            GatewayProfile::LongRunning => (CHAT_TIMEOUT, MAX_CHAT_RESPONSE_BYTES),
        };
        let response = builder.timeout(timeout).send().await.map_err(|error| {
            if request.mutation {
                GatewayRequestError::IndeterminateMutation(format!(
                    "{} request failed after it may have reached the provider: {error}",
                    request.operation
                ))
            } else {
                GatewayRequestError::BeforeEffect(format!(
                    "{} request failed: {error}",
                    request.operation
                ))
            }
        })?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > max_response_bytes as u64)
        {
            if !status.is_success() {
                return Err(gateway_status_failure(
                    &request,
                    status,
                    "bounded error response omitted".to_string(),
                ));
            }
            return Err(if request.mutation {
                GatewayRequestError::IndeterminateMutation(format!(
                    "{} succeeded but its response exceeded {max_response_bytes} bytes",
                    request.operation
                ))
            } else {
                GatewayRequestError::InvalidResponse(format!(
                    "{} response exceeds {max_response_bytes} bytes",
                    request.operation
                ))
            });
        }
        let mut body = Vec::new();
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| {
                gateway_response_failure(&request, format!("response body failed: {error}"))
            })?;
            if body.len().saturating_add(chunk.len()) > max_response_bytes {
                return Err(gateway_response_failure(
                    &request,
                    format!("response exceeds {max_response_bytes} bytes"),
                ));
            }
            body.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            return Err(gateway_status_failure(
                &request,
                status,
                truncate_error(&String::from_utf8_lossy(&body)),
            ));
        }
        serde_json::from_slice(&body).map_err(|error| {
            gateway_response_failure(&request, format!("response is invalid JSON: {error}"))
        })
    }

    /// Open the fixed native event stream through the same loopback endpoint,
    /// bearer, redirect policy and connection bounds as other gateway calls.
    /// The response has no whole-operation/read timeout because it is a stream;
    /// only the initial connection and response headers are deadline-bounded.
    pub(crate) async fn open_gateway_event_stream(
        &self,
    ) -> Result<reqwest::Response, GatewayRequestError> {
        let http = self
            .http()
            .map_err(|error| GatewayRequestError::BeforeEffect(error.to_string()))?;
        let request = self
            .request(
                http.get(format!("{}/api/events", self.endpoint))
                    .header(reqwest::header::ACCEPT, "text/event-stream"),
            )
            .map_err(|error| GatewayRequestError::BeforeEffect(error.to_string()))?;
        let response = tokio::time::timeout(CONFIG_TIMEOUT, request.send())
            .await
            .map_err(|_| {
                GatewayRequestError::BeforeEffect(
                    "tched_router event subscription timed out".to_string(),
                )
            })?
            .map_err(|error| {
                GatewayRequestError::BeforeEffect(format!(
                    "tched_router event subscription failed: {error}"
                ))
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(GatewayRequestError::ProviderRejected {
                status: status.as_u16(),
                detail: "event subscription rejected".to_string(),
            });
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if !content_type
            .split(';')
            .next()
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("text/event-stream"))
        {
            return Err(GatewayRequestError::InvalidResponse(
                "event subscription did not return text/event-stream".to_string(),
            ));
        }
        Ok(response)
    }

    /// Read the daemon-owned current configuration. The v0.8.4 handler masks
    /// secrets before serialization; the recursive pass is defense in depth
    /// and runs before method-output validation, audit, or return.
    pub(crate) async fn read_masked_config(
        &self,
        secrets: &dyn ConfigSecretPolicy,
    ) -> anyhow::Result<Value> {
        self.health().await?;
        let mut config: Value = self
            .send_config_json(
                self.http()?.get(format!("{}/api/config", self.endpoint)),
                "read masked config",
            )
            .await?;
        anyhow::ensure!(
            config.is_object(),
            "tched_router masked config response is not an object"
        );
        secrets.redact_config(&mut config)?;
        redact_secret_leaves(&mut config);
        Ok(config)
    }

    /// Read the v0.8.4 per-property catalog without allowing a secret entry to
    /// carry a value. The returned shape is validated by the pinned plugin
    /// method contract at the bridge boundary.
    pub(crate) async fn read_config_list(
        &self,
        prefix: Option<&str>,
        secrets: &dyn ConfigSecretPolicy,
    ) -> anyhow::Result<Value> {
        if let Some(prefix) = prefix {
            validate_config_path(prefix)?;
        }
        self.health().await?;
        let mut request = self
            .http()?
            .get(format!("{}/api/config/list", self.endpoint));
        if let Some(prefix) = prefix {
            request = request.query(&[("prefix", prefix)]);
        }
        let mut response: Value = self
            .send_config_json(request, "list config properties")
            .await?;
        let object = response
            .as_object_mut()
            .context("tched_router config list response is not an object")?;
        let entries = object
            .get_mut("entries")
            .and_then(Value::as_array_mut)
            .context("tched_router config list response has no entries array")?;
        anyhow::ensure!(
            entries.len() <= MAX_CONFIG_LIST_ENTRIES,
            "tched_router config list contains too many entries"
        );
        for entry in entries {
            let entry = entry
                .as_object_mut()
                .context("tched_router config list entry is not an object")?;
            let path = entry
                .get("path")
                .and_then(Value::as_str)
                .context("tched_router config list entry has no path")?;
            validate_config_path(path)?;
            let pinned_secret = secrets.is_secret(path)?;
            if pinned_secret || entry.get("is_secret").and_then(Value::as_bool) == Some(true) {
                entry.remove("value");
            } else if let Some(value) = entry.get_mut("value") {
                anyhow::ensure!(
                    value.is_string(),
                    "config list display value is not a string"
                );
                redact_secret_leaves(value);
            }
        }
        let drifted = object
            .get_mut("drifted")
            .and_then(Value::as_array_mut)
            .context("tched_router config list response has no drifted array")?;
        anyhow::ensure!(
            drifted.len() <= MAX_CONFIG_LIST_ENTRIES,
            "tched_router config drift list contains too many entries"
        );
        for drift in drifted {
            let drift = drift
                .as_object_mut()
                .context("tched_router config drift entry is not an object")?;
            let path = drift
                .get("path")
                .and_then(Value::as_str)
                .context("tched_router config drift entry has no path")?;
            validate_config_path(path)?;
            anyhow::ensure!(
                drift.get("drifted").and_then(Value::as_bool) == Some(true),
                "tched_router config drift entry is not marked drifted"
            );
            let secret = secrets.is_secret(path)?
                || drift.get("secret").and_then(Value::as_bool) == Some(true);
            if secret {
                drift.remove("in_memory_value");
                drift.remove("on_disk_value");
            } else {
                for field in ["in_memory_value", "on_disk_value"] {
                    if let Some(value) = drift.get(field) {
                        anyhow::ensure!(
                            value.is_string(),
                            "tched_router config drift display value is not a string"
                        );
                    }
                }
            }
        }
        redact_secret_leaves(&mut response);
        Ok(response)
    }

    /// Read one property. Secret responses are populated-only upstream. A
    /// response carrying both `populated` and `value` is rejected.
    pub(crate) async fn read_config_property(
        &self,
        path: &str,
        secrets: &dyn ConfigSecretPolicy,
    ) -> anyhow::Result<Value> {
        validate_config_path(path)?;
        self.health().await?;
        let mut response: Value = self
            .send_config_json(
                self.http()?
                    .get(format!("{}/api/config/prop", self.endpoint))
                    .query(&[("path", path)]),
                "read config property",
            )
            .await?;
        let object = response
            .as_object_mut()
            .context("tched_router config property response is not an object")?;
        anyhow::ensure!(
            object.get("path").and_then(Value::as_str) == Some(path),
            "tched_router config property response path mismatch"
        );
        let secret = secrets.is_secret(path)?;
        anyhow::ensure!(
            !secret || (object.contains_key("populated") && !object.contains_key("value")),
            "tched_router secret property response was not populated-only"
        );
        anyhow::ensure!(
            secret || (object.contains_key("value") && !object.contains_key("populated")),
            "tched_router public property response was not value-only"
        );
        if secret {
            anyhow::ensure!(
                object.get("populated").and_then(Value::as_bool).is_some(),
                "tched_router secret property populated flag is not boolean"
            );
            object.insert("kind".to_string(), Value::String("secret".to_string()));
        } else {
            anyhow::ensure!(
                object.get("value").and_then(Value::as_str).is_some(),
                "tched_router property display value is not a string"
            );
            object.insert("kind".to_string(), Value::String("value".to_string()));
        }
        redact_secret_leaves(&mut response);
        Ok(response)
    }

    pub(crate) async fn set_config_property(
        &self,
        input: &ConfigSetInput,
        secrets: &dyn ConfigSecretPolicy,
    ) -> anyhow::Result<NativeConfigMutation> {
        validate_config_path(&input.path)?;
        validate_config_value(&input.value)?;
        self.health().await?;
        let secret = secrets.is_secret(&input.path)?;
        let _before = self.read_config_property(&input.path, secrets).await?;
        if secret {
            reject_masked_or_empty_secret(&input.value)?;
        }
        let body = serde_json::to_value(input)?;
        let mut result: Value = self
            .send_gateway_json(GatewayJsonRequest {
                method: GatewayMethod::Put,
                relative_path: "/api/config/prop",
                query: &[],
                body: Some(&body),
                operation: "set config property",
                profile: GatewayProfile::Config,
                mutation: true,
            })
            .await
            .map_err(config_gateway_error)?;
        sanitize_property_response(&mut result, &input.path, secret)?;
        let readback = self
            .read_config_property(&input.path, secrets)
            .await
            .with_context(
                || "native config property was updated but authoritative readback failed",
            )?;
        if secret {
            anyhow::ensure!(
                result.get("populated") == readback.get("populated"),
                "tched_router secret property changed during readback"
            );
        } else {
            anyhow::ensure!(
                result.get("value") == readback.get("value"),
                "tched_router property changed during readback"
            );
        }
        Ok(NativeConfigMutation { result, readback })
    }

    pub(crate) async fn patch_config(
        &self,
        input: &ConfigPatchInput,
        secrets: &dyn ConfigSecretPolicy,
    ) -> anyhow::Result<NativeConfigMutation> {
        anyhow::ensure!(
            !input.operations.is_empty() && input.operations.len() <= MAX_CONFIG_PATCH_OPERATIONS,
            "config patch must contain 1..={MAX_CONFIG_PATCH_OPERATIONS} operations"
        );
        self.health().await?;
        let mut readback_paths = std::collections::BTreeSet::new();
        for operation in &input.operations {
            let (pointer, value, is_test) = patch_operation_parts(operation);
            let dotted = validate_patch_pointer(pointer)?;
            let secret = secrets.is_secret(&dotted)?;
            let _before = self.read_config_property(&dotted, secrets).await?;
            if secret {
                anyhow::ensure!(!is_test, "config patch cannot test a secret property");
                if let Some(value) = value {
                    reject_masked_or_empty_secret(value)?;
                }
            }
            if let Some(value) = value {
                validate_config_value(value)?;
            }
            readback_paths.insert(dotted);
        }
        let mut operations = serde_json::to_value(&input.operations)
            .context("serialize admitted config patch operations")?;
        normalize_config_patch_numbers(&mut operations);
        let mut result: Value = self
            .send_gateway_json(GatewayJsonRequest {
                method: GatewayMethod::Patch,
                relative_path: "/api/config",
                query: &[],
                body: Some(&operations),
                operation: "patch config properties",
                profile: GatewayProfile::Config,
                mutation: true,
            })
            .await
            .map_err(config_gateway_error)?;
        sanitize_patch_response(&mut result, &input.operations, secrets).context(
            "native config patch completed but its receipt was invalid; reconcile before retry",
        )?;

        let mut readback = Vec::with_capacity(readback_paths.len());
        for (index, path) in readback_paths.into_iter().enumerate() {
            readback.push(
                self.read_config_property(&path, secrets)
                    .await
                    .with_context(|| {
                        format!("native config patch succeeded but readback {index} failed")
                    })?,
            );
        }
        verify_patch_readbacks(&result, &readback, &input.operations)?;
        Ok(NativeConfigMutation {
            result,
            readback: Value::Array(readback),
        })
    }

    pub(crate) async fn init_config(
        &self,
        input: &ConfigInitInput,
        secrets: &dyn ConfigSecretPolicy,
    ) -> anyhow::Result<NativeConfigMutation> {
        if let Some(section) = input.section.as_deref() {
            validate_config_path(section)?;
            if section == "mcp.servers" || section.starts_with("mcp.servers.") {
                let alias = section
                    .strip_prefix("mcp.servers.")
                    .filter(|alias| valid_config_segment(alias))
                    .context("MCP server init requires one valid alias")?;
                // The plugin-owned, sealed config contract must recognize this
                // natural-key entry before the native owner creates it.
                anyhow::ensure!(
                    !secrets.is_secret(&format!("mcp.servers.{alias}.name"))?,
                    "MCP server name must be public in the pinned config contract"
                );
                self.health().await?;
                let receipt: NativeMapKeyResponse = self
                    .send_gateway_json(GatewayJsonRequest {
                        method: GatewayMethod::Post,
                        relative_path: "/api/config/map-key",
                        query: &[("path", "mcp.servers"), ("key", alias)],
                        body: None,
                        operation: "initialize MCP server config entry",
                        profile: GatewayProfile::Config,
                        mutation: true,
                    })
                    .await
                    .map_err(config_gateway_error)?;
                anyhow::ensure!(
                    receipt.path == "mcp.servers"
                        && receipt.key == alias
                        && receipt.warnings.as_ref().is_none_or(Vec::is_empty),
                    "native MCP server creation receipt is invalid; reconcile before retry"
                );
                let readback = self
                    .read_masked_config(secrets)
                    .await
                    .context("native MCP server creation succeeded but readback failed; reconcile before retry")?;
                let matching = readback
                    .pointer("/mcp/servers")
                    .and_then(Value::as_array)
                    .context(
                        "native MCP server readback has no servers array; reconcile before retry",
                    )?
                    .iter()
                    .filter(|entry| entry.get("name").and_then(Value::as_str) == Some(alias))
                    .count();
                anyhow::ensure!(
                    matching == 1,
                    "native MCP server entry did not appear exactly once; reconcile before retry"
                );
                return Ok(NativeConfigMutation {
                    result: json!({"initialized": if receipt.created { vec![section] } else { vec![] }}),
                    readback,
                });
            }
        }
        self.health().await?;
        let query = input
            .section
            .as_deref()
            .map(|section| vec![("section", section)])
            .unwrap_or_default();
        let result: Value = self
            .send_gateway_json(GatewayJsonRequest {
                method: GatewayMethod::Post,
                relative_path: "/api/config/init",
                query: &query,
                body: None,
                operation: "initialize config",
                profile: GatewayProfile::Config,
                mutation: true,
            })
            .await
            .map_err(config_gateway_error)?;
        validate_init_response(&result)?;
        let readback = self
            .read_masked_config(secrets)
            .await
            .context("native config initialization succeeded but readback failed")?;
        Ok(NativeConfigMutation { result, readback })
    }

    /// Create `providers.models.<family>.<alias>` through the native map-key API
    /// when the pinned contract admits the profile path.
    pub(crate) async fn init_provider_model_reference(
        &self,
        reference: &str,
        secrets: &dyn ConfigSecretPolicy,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            crate::tched_catalog_binding::valid_reference(reference),
            "native provider reference is invalid"
        );
        let (family, alias) = reference
            .split_once('.')
            .context("native provider reference must be family.alias")?;
        anyhow::ensure!(
            valid_config_segment(family) && valid_config_segment(alias),
            "native provider reference segments are invalid"
        );
        let model_path = format!("providers.models.{reference}.model");
        match secrets.is_secret(&model_path) {
            Ok(false) => {}
            Ok(true) => {
                anyhow::bail!("native provider model path must be public in the pinned contract")
            }
            Err(_) => anyhow::bail!(
                "native provider reference {reference} is absent from the pinned config contract"
            ),
        }
        self.health().await?;
        let map_path = format!("providers.models.{family}");
        let receipt: NativeMapKeyResponse = self
            .send_gateway_json(GatewayJsonRequest {
                method: GatewayMethod::Post,
                relative_path: "/api/config/map-key",
                query: &[("path", map_path.as_str()), ("key", alias)],
                body: None,
                operation: "initialize native provider profile",
                profile: GatewayProfile::Config,
                mutation: true,
            })
            .await
            .map_err(config_gateway_error)?;
        anyhow::ensure!(
            receipt.path == map_path && receipt.key == alias,
            "native provider profile creation receipt is invalid"
        );
        anyhow::ensure!(
            receipt.warnings.as_ref().is_none_or(Vec::is_empty),
            "native provider profile creation returned warnings"
        );
        let readback = self
            .read_masked_config(secrets)
            .await
            .context("native provider profile creation succeeded but readback failed")?;
        let configured = readback
            .pointer(&format!("/providers/models/{family}/{alias}/model"))
            .is_some();
        anyhow::ensure!(
            configured || receipt.created,
            "native provider profile did not appear in configuration readback"
        );
        Ok(())
    }

    pub(crate) async fn migrate_config(
        &self,
        _: &ConfigMigrateInput,
        secrets: &dyn ConfigSecretPolicy,
    ) -> anyhow::Result<NativeConfigMutation> {
        self.health().await?;
        let response: NativeMigrateResponse = self
            .send_gateway_json(GatewayJsonRequest {
                method: GatewayMethod::Post,
                relative_path: "/api/config/migrate",
                query: &[],
                body: None,
                operation: "migrate config",
                profile: GatewayProfile::Config,
                mutation: true,
            })
            .await
            .map_err(config_gateway_error)?;
        // The native backup path is an implementation/source locator and is
        // deliberately not part of the plugin-owned response contract.
        let result = serde_json::json!({
            "migrated": response.migrated,
            "schema_version": response.schema_version,
        });
        let readback = self
            .read_masked_config(secrets)
            .await
            .context("native config migration succeeded but readback failed")?;
        Ok(NativeConfigMutation { result, readback })
    }

    async fn health(&self) -> anyhow::Result<()> {
        let health: HealthResponse = self
            .send_config_json(
                self.http()?.get(format!("{}/health", self.endpoint)),
                "health",
            )
            .await?;
        if health.status != "ok" {
            return Err(anyhow!(
                "tched_router health status is '{}', expected 'ok'",
                health.status
            ));
        }
        Ok(())
    }

    async fn get_prop(&self, path: &str) -> anyhow::Result<String> {
        let response: PropResponse = self
            .send_config_json(
                self.http()?
                    .get(format!("{}/api/config/prop", self.endpoint))
                    .query(&[("path", path)]),
                &format!("read config property {path}"),
            )
            .await?;
        response
            .value
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("tched_router config property '{path}' is not a string"))
    }

    async fn configured_providers(&self) -> anyhow::Result<Vec<ConfiguredProvider>> {
        let response: ConfigListResponse = self
            .send_config_json(
                self.http()?
                    .get(format!("{}/api/config/list", self.endpoint))
                    .query(&[("prefix", "providers.models")]),
                "list configured providers",
            )
            .await?;
        anyhow::ensure!(
            response.entries.len() <= MAX_CONFIG_LIST_ENTRIES,
            "native provider inventory exceeds bound"
        );
        parse_configured_providers(response.entries)
    }

    /// Observe only eligible ordinary profile/model bindings, with independent
    /// catalog endpoint trust and a stable masked native configuration image.
    /// The caller retains the final-admitted operation guard and publishes the
    /// resulting pair before returning a committed field to the caller.
    pub(crate) async fn observe_catalog_binding(
        &self,
        target: &crate::tched_catalog_binding::BindingTarget,
        secrets: &dyn ConfigSecretPolicy,
    ) -> anyhow::Result<CatalogBindingObservation> {
        anyhow::ensure!(
            self.sealed_identity
                .as_deref()
                .is_some_and(|value| !value.is_empty()),
            "catalog binding requires verified caller identity metadata"
        );
        use crate::tched_catalog_binding::{
            MAX_BINDING_PROFILES, MAX_PROFILE_MODELS, ProfileCandidate,
        };
        let before = self
            .read_masked_config(secrets)
            .await
            .map_err(|_| anyhow!("catalog binding native configuration is unavailable"))?;
        let configured = self
            .configured_providers()
            .await
            .map_err(|_| anyhow!("catalog binding native provider inventory is unavailable"))?;
        let eligible = binding_profiles(&before, configured, target, self.opencode_only())?;
        anyhow::ensure!(
            eligible.len() <= MAX_BINDING_PROFILES,
            "catalog binding profile inventory exceeds bound"
        );
        let mut candidates = Vec::with_capacity(eligible.len());
        for profile in eligible {
            let (family, alias) = profile
                .reference
                .split_once('.')
                .context("catalog binding profile reference is invalid")?;
            let native_profile = before
                .get("providers")
                .and_then(|p| p.get("models"))
                .and_then(|m| m.get(family))
                .and_then(|f| f.get(alias))
                .context("catalog binding profile is absent from native observation")?;
            anyhow::ensure!(
                native_profile.get("model").and_then(Value::as_str)
                    == Some(profile.selected_model.as_str())
                    || target
                        .ordinary_reference
                        .as_ref()
                        .is_some_and(|reference| reference == &profile.reference)
                        && native_profile.get("model").and_then(Value::as_str)
                            == Some(target.model_id.as_str()),
                "catalog binding native profile changed during observation"
            );
            let oauth = native_profile
                .get("requires_openai_auth")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let uri = if oauth {
                // Subscription profiles are observed (and located by the
                // binding) without an HTTP endpoint; the codex auth store
                // owns the origin.
                native_profile
                    .get("uri")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            } else {
                native_profile
                    .get("uri")
                    .and_then(Value::as_str)
                    .context("catalog binding native endpoint is absent")?
                    .to_string()
            };
            let catalog = self
                .model_catalog(&profile.reference)
                .await
                .map_err(|_| anyhow!("catalog binding native model catalog is unavailable"))?;
            anyhow::ensure!(
                catalog.models.len() <= MAX_PROFILE_MODELS,
                "catalog binding native model catalog exceeds bound"
            );
            let mut models = catalog.models;
            let mut live = catalog.live;
            // When the catalog names an exact ordinary profile, a configured model match
            // may bind even if the upstream live inventory is empty or offline.
            if target
                .ordinary_reference
                .as_ref()
                .is_some_and(|reference| reference == &profile.reference)
            {
                let configured_model = native_profile.get("model").and_then(Value::as_str);
                if configured_model == Some(target.model_id.as_str()) {
                    if !models.iter().any(|model| model == &target.model_id) {
                        models.push(target.model_id.clone());
                    }
                    if !live {
                        live = true;
                    }
                }
            }
            candidates.push(ProfileCandidate {
                reference: profile.reference,
                uri,
                models,
                live,
                oauth,
            });
        }
        let after = self
            .read_masked_config(secrets)
            .await
            .map_err(|_| anyhow!("catalog binding native configuration readback is unavailable"))?;
        anyhow::ensure!(
            before == after,
            "catalog binding native configuration changed during observation"
        );
        let binding = crate::tched_catalog_binding::select(target, &candidates)?;
        Ok(CatalogBindingObservation {
            native_config: after,
            binding,
        })
    }

    /// Native provider inventory for the dashboard lookup: every configured
    /// `family.alias` profile with its served model, subscription (OAuth)
    /// class, and door selectability. Values are observed live from the
    /// runtime; credential values never appear.
    pub(crate) async fn native_providers_inventory(
        &self,
        secrets: &dyn ConfigSecretPolicy,
    ) -> anyhow::Result<Vec<op_plugins::state_plugins::model_registry::NativeProviderProfile>> {
        let config = self.read_masked_config(secrets).await?;
        let configured = self.configured_providers().await?;
        let agents = config
            .get("agents")
            .and_then(Value::as_object)
            .context("native provider inventory agent section is absent")?;
        let mut profiles = Vec::new();
        for provider in configured {
            let reference = provider.reference.clone();
            let (family, alias) = reference.split_once('.').with_context(|| {
                format!("native provider inventory reference {reference} is invalid")
            })?;
            let (family, alias) = (family.to_string(), alias.to_string());
            let native_profile = config
                .get("providers")
                .and_then(|p| p.get("models"))
                .and_then(|m| m.get(family.as_str()))
                .and_then(|f| f.get(alias.as_str()))
                .context("native provider inventory profile is absent")?;
            let oauth = native_profile
                .get("requires_openai_auth")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let selectable = alias == DEFAULT_AGENT_ALIAS
                || agents
                    .get(alias.as_str())
                    .and_then(|agent| agent.get("model_provider"))
                    .and_then(Value::as_str)
                    .is_none();
            profiles.push(
                op_plugins::state_plugins::model_registry::NativeProviderProfile {
                    reference: provider.reference,
                    family,
                    model: provider.selected_model,
                    oauth,
                    selectable,
                },
            );
        }
        Ok(profiles)
    }

    fn opencode_only(&self) -> bool {
        std::env::var("TCHED_ROUTER_REQUIRE_OPENCODE").as_deref() == Ok("1")
            && self.agent_alias != crate::chat_mode_routing::CONTROL_PLANE_AGENT
    }

    /// Providers a door caller may select.
    async fn selectable_providers(&self) -> anyhow::Result<Vec<ConfiguredProvider>> {
        let mut providers = self.configured_providers().await?;
        if self.opencode_only() {
            providers.retain(is_door_selectable_route);
        }
        Ok(providers)
    }

    async fn published_providers(
        &self,
        selected_ref: &str,
    ) -> anyhow::Result<Vec<ConfiguredProvider>> {
        Ok(publish_providers(
            self.configured_providers().await?,
            selected_ref,
            self.opencode_only(),
        ))
    }

    async fn runtime_defaults(&self) -> anyhow::Result<(String, String)> {
        let provider_path = self.agent_provider_path()?;
        let provider = self.get_prop(&provider_path).await?;
        let model = self.get_prop(&provider_model_path(&provider)?).await?;
        anyhow::ensure!(
            !provider.trim().is_empty() && !model.trim().is_empty(),
            "tched_router agent provider/model observation is empty"
        );
        Ok((provider, model))
    }

    fn agent_provider_path(&self) -> anyhow::Result<String> {
        anyhow::ensure!(
            !self.agent_alias.is_empty()
                && self
                    .agent_alias
                    .bytes()
                    .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') }),
            "tched_router agent alias cannot be addressed through the config API"
        );
        Ok(format!("agents.{}.model_provider", self.agent_alias))
    }

    async fn model_catalog(&self, reference: &str) -> anyhow::Result<ModelCatalogResponse> {
        self.send_config_json(
            self.http()?
                .get(format!("{}/api/config/catalog/models", self.endpoint))
                .query(&[("model_provider", reference)]),
            &format!("list {reference} models"),
        )
        .await
    }

    async fn current_provider(&self) -> anyhow::Result<ConfiguredProvider> {
        let (provider_ref, selected_model) = self.runtime_defaults().await?;
        let configured = self.published_providers(&provider_ref).await?;
        let provider = configured
            .into_iter()
            .find(|provider| provider.reference == provider_ref)
            .with_context(|| {
                format!(
                    "tched_router agent selects provider '{provider_ref}' absent from the configured provider list"
                )
            })?;
        anyhow::ensure!(
            provider.selected_model == selected_model,
            "tched_router selected provider model changed during observation"
        );
        Ok(provider)
    }

    /// Read the authoritative current selection without consulting the SHM
    /// projection. Callers that pair this with a mutation hold
    /// [`Self::selection_guard`] across this read, the upstream write, and the
    /// represented-state commit.
    pub(crate) async fn current_selection(&self) -> anyhow::Result<RuntimeSelection> {
        self.health().await?;
        let observed = self.current_provider().await?;
        Ok(RuntimeSelection {
            provider: observed.family,
            model: observed.selected_model,
        })
    }

    #[allow(dead_code)] // Runtime catalog projection helper; selection dispatch reads its own inputs.
    pub(crate) async fn project_state(
        &self,
        mut state: TchedRouterState,
    ) -> anyhow::Result<TchedRouterState> {
        self.health().await?;
        let (selected_ref, selected_model) = self.runtime_defaults().await?;
        let configured = self.published_providers(&selected_ref).await?;
        let selected = configured
            .iter()
            .find(|provider| provider.reference == selected_ref)
            .context(
                "tched_router selected provider is absent from the configured provider list",
            )?;
        anyhow::ensure!(
            selected.selected_model == selected_model,
            "tched_router selected provider model changed during observation"
        );
        let selected_family = selected.family.clone();

        let mut families: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for provider in &configured {
            families
                .entry(provider.family.clone())
                .or_default()
                .push(provider.reference.clone());
        }
        let mut providers = Vec::with_capacity(families.len());
        let mut routes = Vec::new();
        for (family, aliases) in families {
            let route = aliases
                .iter()
                .find(|reference| reference.as_str() == selected_ref)
                .or_else(|| aliases.first())
                .context("tched_router provider family has no configured aliases")?
                .clone();
            let mut observed_models = BTreeMap::<String, bool>::new();
            let mut selected_alias_has_model = false;
            for reference in &aliases {
                let catalog = self.model_catalog(reference).await?;
                let mut alias_models = std::collections::BTreeSet::new();
                anyhow::ensure!(
                    catalog.models.iter().all(|model| {
                        !model.trim().is_empty() && alias_models.insert(model.as_str())
                    }),
                    "tched_router '{}' model catalog contains an empty or duplicate model",
                    reference
                );
                if reference == &selected_ref {
                    anyhow::ensure!(
                        !catalog.models.is_empty(),
                        "tched_router selected '{}' model catalog is empty",
                        reference
                    );
                    selected_alias_has_model = alias_models.contains(selected_model.as_str());
                }
                for model in catalog.models {
                    observed_models
                        .entry(model)
                        .and_modify(|live| *live |= catalog.live)
                        .or_insert(catalog.live);
                }
            }
            if family == selected_family {
                anyhow::ensure!(
                    selected_alias_has_model,
                    "tched_router selected model is absent from its configured alias catalog"
                );
            }
            providers.push(Provider {
                id: family.clone(),
                route,
                kind: "tched-router-runtime".to_string(),
                aliases: aliases.clone(),
                sdk: "tched_router".to_string(),
                description: format!("Configured tched_router provider family '{family}'."),
                ..Default::default()
            });
            for (model, live) in observed_models {
                routes.push(ModelRoute {
                    hint: "runtime".to_string(),
                    provider: family.clone(),
                    upstream_provider: family.clone(),
                    transport: "tched-router-loopback".to_string(),
                    model,
                    kind: "chat".to_string(),
                    status: if live {
                        "available".to_string()
                    } else {
                        "catalogued".to_string()
                    },
                    available: live,
                    status_reason: if live {
                        "Discovered through the live tched_router model catalog.".to_string()
                    } else {
                        "Returned by tched_router's cached model catalog.".to_string()
                    },
                    api_key: Some(Value::Null),
                    ..Default::default()
                });
            }
        }

        // The door UI starts on the first provider: the router's own default
        // when the door cannot select the agent's native provider.
        if is_router_default(&selected_ref, self.opencode_only()) {
            providers.sort_by_key(|provider| provider.id != selected_family);
        }

        let (confirmed_ref, confirmed_model) = self.runtime_defaults().await?;
        anyhow::ensure!(
            confirmed_ref == selected_ref && confirmed_model == selected_model,
            "tched_router selection changed during catalog observation"
        );

        state.status = "active".to_string();
        state.selected_provider = selected_family.clone();
        state.selected_model = selected_model.clone();
        state.transport.grpc_target = self.endpoint.clone();
        state.catalog.providers = providers;
        state.catalog.model_routes = routes;
        state.catalog.router.provider = selected_family;
        state.catalog.router.model = selected_model;
        state.catalog.router.endpoint = self.endpoint.clone();
        state.catalog.router.role = "tched-router-runtime-authority".to_string();
        Ok(state)
    }

    /// Observe configuration, selection, and catalogs as one coherent input
    /// to a single MutationEngine commit. The second full-config read detects
    /// concurrent upstream edits; there is deliberately no retry loop.
    pub(crate) async fn observe_config_and_state(
        &self,
        secrets: &dyn ConfigSecretPolicy,
    ) -> anyhow::Result<NativeConfigObservation> {
        let before = self.read_masked_config(secrets).await?;
        let observed = self.project_state(TchedRouterState::default()).await?;
        let after = self.read_masked_config(secrets).await?;
        anyhow::ensure!(
            before == after,
            "tched_router configuration changed during native observation"
        );
        anyhow::ensure!(
            !observed.status.is_empty()
                && !observed.selected_provider.is_empty()
                && !observed.selected_model.is_empty()
                && !observed.catalog.providers.is_empty(),
            "tched_router observation is missing status, selection, or provider catalog"
        );
        let provider_ids = observed
            .catalog
            .providers
            .iter()
            .map(|provider| provider.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        anyhow::ensure!(
            provider_ids.len() == observed.catalog.providers.len()
                && provider_ids.iter().all(|id| !id.is_empty())
                && provider_ids.contains(observed.selected_provider.as_str())
                && observed.catalog.model_routes.iter().all(|route| {
                    !route.model.trim().is_empty() && provider_ids.contains(route.provider.as_str())
                })
                && observed.catalog.model_routes.iter().any(|route| {
                    route.provider == observed.selected_provider
                        && route.model == observed.selected_model
                }),
            "tched_router observed selection/catalog bindings are inconsistent"
        );
        let mut model_bindings = std::collections::BTreeSet::new();
        anyhow::ensure!(
            observed.catalog.model_routes.iter().all(|route| {
                model_bindings.insert((route.provider.as_str(), route.model.as_str()))
            }),
            "native tched_router catalog contains duplicate provider/model bindings"
        );
        Ok(NativeConfigObservation {
            native_config: after,
            represented: serde_json::json!({
                "status": observed.status.clone(),
                "selected_provider": observed.selected_provider.clone(),
                "selected_model": observed.selected_model.clone(),
                "providers": observed.catalog.providers.clone(),
                "model_routes": observed.catalog.model_routes.clone(),
            }),
            state: observed,
        })
    }

    async fn resolve_provider(&self, selector: &str) -> anyhow::Result<ConfiguredProvider> {
        let current = self.current_provider().await?;
        if current.reference.eq_ignore_ascii_case(selector)
            || current.family.eq_ignore_ascii_case(selector)
        {
            return Ok(current);
        }
        let providers = self.selectable_providers().await?;
        if let Some(provider) = providers
            .iter()
            .find(|provider| provider.reference.eq_ignore_ascii_case(selector))
        {
            return Ok(provider.clone());
        }
        let mut matches = providers
            .into_iter()
            .filter(|provider| provider.family.eq_ignore_ascii_case(selector));
        let selected = matches
            .next()
            .ok_or_else(|| anyhow!("tched_router provider '{selector}' is not configured"))?;
        anyhow::ensure!(
            matches.next().is_none(),
            "tched_router provider '{selector}' has multiple configured aliases; specify the provider reference"
        );
        Ok(selected)
    }

    // The pinned v0.8.4 binary schema makes selection agent-scoped at
    // `agents.<alias>.model_provider`; the model is owned by that selected
    // `providers.models.<family>.<alias>` profile. The config API persists and
    // swaps those values for subsequent A2A turns without a restart.
    pub(crate) async fn set_provider(&self, provider_id: &str) -> anyhow::Result<RuntimeSelection> {
        self.health().await?;
        let provider = self.resolve_provider(provider_id).await?;
        let provider_path = self.agent_provider_path()?;
        let (current_provider, _) = self.runtime_defaults().await?;
        let patch = property_patch(&provider_path, &current_provider, &provider.reference);
        let _: Value = self
            .send_config_json(
                self.http()?
                    .patch(format!("{}/api/config", self.endpoint))
                    .json(&patch),
                "update agent provider",
            )
            .await
            .context(
                "native provider selection request may have taken effect without a complete response; reconcile before retry",
            )?;
        let observed = self.current_provider().await.context(
            "native provider selection completed but authoritative readback failed; reconcile before retry",
        )?;
        if observed.reference != provider.reference {
            return Err(anyhow!(
                "native provider selection completed but authoritative readback did not retain it; reconcile before retry"
            ));
        }
        Ok(RuntimeSelection {
            provider: observed.family,
            model: observed.selected_model,
        })
    }

    pub(crate) async fn set_model(&self, model_id: &str) -> anyhow::Result<RuntimeSelection> {
        self.health().await?;
        let provider = self.current_provider().await?;
        ensure_router_default_model(&provider, model_id, self.opencode_only())?;
        let catalog = self.model_catalog(&provider.reference).await?;
        if !catalog.models.iter().any(|model| model == model_id) {
            return Err(anyhow!(
                "tched_router model '{model_id}' is not present in the observed '{}' catalog",
                provider.family
            ));
        }
        let provider_path = self.agent_provider_path()?;
        let model_path = provider_model_path(&provider.reference)?;
        let patch = model_patch(
            &provider_path,
            &provider.reference,
            &model_path,
            &provider.selected_model,
            model_id,
        );
        let _: Value = self
            .send_config_json(
                self.http()?
                    .patch(format!("{}/api/config", self.endpoint))
                    .json(&patch),
                "update selected provider model",
            )
            .await
            .context(
                "native model selection request may have taken effect without a complete response; reconcile before retry",
            )?;
        let observed = self.current_provider().await.context(
            "native model selection completed but authoritative readback failed; reconcile before retry",
        )?;
        if observed.reference != provider.reference || observed.selected_model != model_id {
            return Err(anyhow!(
                "native model selection completed but authoritative readback did not retain it; reconcile before retry"
            ));
        }
        Ok(RuntimeSelection {
            provider: observed.family,
            model: observed.selected_model,
        })
    }

    pub(crate) async fn set_selection(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> anyhow::Result<RuntimeSelection> {
        self.health().await?;
        let provider = self.resolve_provider(provider_id).await?;
        ensure_router_default_model(&provider, model_id, self.opencode_only())?;
        let catalog = self.model_catalog(&provider.reference).await?;
        if !catalog.models.iter().any(|model| model == model_id) {
            return Err(anyhow!(
                "tched_router model '{model_id}' is not present in the observed '{}' catalog",
                provider.family
            ));
        }
        let provider_path = self.agent_provider_path()?;
        let model_path = provider_model_path(&provider.reference)?;
        let (current_provider, _) = self.runtime_defaults().await?;
        let patch = selection_patch(
            &provider_path,
            &model_path,
            &current_provider,
            &provider.selected_model,
            &provider.reference,
            model_id,
        );
        let _: Value = self
            .send_config_json(
                self.http()?
                    .patch(format!("{}/api/config", self.endpoint))
                    .json(&patch),
                "update selection",
            )
            .await
            .context(
                "native provider/model selection request may have taken effect without a complete response; reconcile before retry",
            )?;
        let observed = self.current_provider().await.context(
            "native provider/model selection completed but authoritative readback failed; reconcile before retry",
        )?;
        if observed.reference != provider.reference || observed.selected_model != model_id {
            return Err(anyhow!(
                "native provider/model selection completed but authoritative readback did not retain it; reconcile before retry"
            ));
        }
        Ok(RuntimeSelection {
            provider: observed.family,
            model: observed.selected_model,
        })
    }

    /// Caller retains the admitted router operation guard through this native request.
    pub(crate) async fn chat_under_guard(&self, input: ChatInput) -> anyhow::Result<ChatOutput> {
        let prompt = conversation_prompt(&input)?;
        let body = json!({
            "jsonrpc": "2.0",
            "id": uuid::Uuid::new_v4().to_string(),
            "method": "message/send",
            "params": {"message": {"role": "user", "parts": [{"kind": "text", "text": prompt}]}}
        });
        anyhow::ensure!(
            serde_json::to_vec(&body)?.len() <= MAX_CHAT_REQUEST_BYTES,
            "native chat request exceeds {MAX_CHAT_REQUEST_BYTES} bytes"
        );
        self.health().await?;
        let observed = self.current_provider().await?;
        if !matches_provider_selector(&input.provider, &observed) {
            return Err(anyhow!(
                "requested provider '{}' differs from tched_router's selected provider '{}'",
                input.provider,
                observed.reference
            ));
        }
        if !input.model.trim().is_empty()
            && !input.model.eq_ignore_ascii_case(&observed.selected_model)
        {
            return Err(anyhow!(
                "requested model '{}' differs from tched_router's selected model '{}'",
                input.model,
                observed.selected_model
            ));
        }

        let response: Value = self
            .send_json(
                self.http()?
                    .post(format!("{}/a2a/{}", self.endpoint, self.agent_alias))
                    .json(&body),
                "chat",
                CHAT_TIMEOUT,
                MAX_CHAT_RESPONSE_BYTES,
            )
            .await?;
        if let Some(error) = response.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown tched_router A2A error");
            return Err(anyhow!(
                "tched_router chat failed: {}",
                truncate_error(message)
            ));
        }
        let content = extract_chat_text(&response)?;
        Ok(ChatOutput {
            content,
            provider: observed.family,
            model: observed.selected_model,
            finish_reason: String::new(),
            usage: Value::Null,
        })
    }
}

fn valid_config_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_CONFIG_PATH_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn validate_config_value(value: &Value) -> anyhow::Result<()> {
    let encoded = serde_json::to_vec(value).context("serialize config value for bounds check")?;
    anyhow::ensure!(
        encoded.len() <= MAX_CONFIG_VALUE_BYTES,
        "config value exceeds {MAX_CONFIG_VALUE_BYTES} bytes"
    );
    Ok(())
}

fn validate_patch_pointer(pointer: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        pointer.starts_with('/'),
        "config patch path must be a JSON pointer"
    );
    let segments: Vec<&str> = pointer[1..].split('/').collect();
    anyhow::ensure!(
        !segments.is_empty()
            && segments.iter().all(|segment| valid_config_segment(segment))
            && pointer.len() <= MAX_CONFIG_PATH_BYTES,
        "invalid config patch path"
    );
    Ok(segments.join("."))
}

fn patch_operation_parts(operation: &ConfigPatchOperation) -> (&str, Option<&Value>, bool) {
    match operation {
        ConfigPatchOperation::Add { path, value }
        | ConfigPatchOperation::Replace { path, value } => (path, Some(value), false),
        ConfigPatchOperation::Remove { path } => (path, None, false),
        ConfigPatchOperation::Test { path, value } => (path, Some(value), true),
    }
}

fn contains_masked_secret(value: &Value) -> bool {
    match value {
        Value::String(value) => matches!(value.as_str(), "***MASKED***" | "****"),
        Value::Array(items) => items.iter().any(contains_masked_secret),
        Value::Object(object) => object.values().any(contains_masked_secret),
        _ => false,
    }
}

fn reject_masked_or_empty_secret(value: &Value) -> anyhow::Result<()> {
    anyhow::ensure!(
        !contains_masked_secret(value),
        "masked secret cannot be written"
    );
    anyhow::ensure!(
        !matches!(value, Value::Null) && !matches!(value, Value::String(text) if text.is_empty()),
        "empty secret cannot be written"
    );
    Ok(())
}

fn sanitize_property_response(
    response: &mut Value,
    expected_path: &str,
    secret: bool,
) -> anyhow::Result<()> {
    let object = response
        .as_object_mut()
        .context("tched_router config property response is not an object")?;
    anyhow::ensure!(
        object.get("path").and_then(Value::as_str) == Some(expected_path),
        "tched_router config property response path mismatch"
    );
    anyhow::ensure!(
        !secret || (object.contains_key("populated") && !object.contains_key("value")),
        "tched_router secret property response was not populated-only"
    );
    anyhow::ensure!(
        secret || (object.contains_key("value") && !object.contains_key("populated")),
        "tched_router public property response was not value-only"
    );
    if secret {
        anyhow::ensure!(
            object.get("populated").and_then(Value::as_bool).is_some(),
            "tched_router secret property populated flag is not boolean"
        );
        object.insert("kind".to_string(), Value::String("secret".to_string()));
    } else {
        anyhow::ensure!(
            object.get("value").and_then(Value::as_str).is_some(),
            "tched_router property display value is not a string"
        );
        object.insert("kind".to_string(), Value::String("value".to_string()));
    }
    redact_secret_leaves(response);
    Ok(())
}

fn sanitize_patch_response(
    response: &mut Value,
    operations: &[ConfigPatchOperation],
    secrets: &dyn ConfigSecretPolicy,
) -> anyhow::Result<()> {
    let object = response
        .as_object_mut()
        .context("tched_router config patch response is not an object")?;
    anyhow::ensure!(
        object.get("saved").and_then(Value::as_bool) == Some(true),
        "tched_router config patch did not report a saved configuration"
    );
    let results = object
        .get_mut("results")
        .and_then(Value::as_array_mut)
        .context("tched_router config patch response has no results array")?;
    anyhow::ensure!(
        results.len() == operations.len(),
        "tched_router config patch result count mismatch"
    );
    for (result, operation) in results.iter_mut().zip(operations) {
        let result = result
            .as_object_mut()
            .context("tched_router config patch result is not an object")?;
        let path = result
            .get("path")
            .and_then(Value::as_str)
            .context("tched_router config patch result has no path")?;
        validate_config_path(path)?;
        let expected_path = validate_patch_pointer(patch_operation_parts(operation).0)?;
        anyhow::ensure!(
            path == expected_path
                && result.get("op").and_then(Value::as_str)
                    == Some(patch_operation_name(operation)),
            "native config patch result does not match submitted operation sequence"
        );
        let secret = secrets.is_secret(path)?;
        if secret {
            anyhow::ensure!(
                result.get("populated").and_then(Value::as_bool).is_some()
                    && !result.contains_key("value"),
                "tched_router secret patch result was not populated-only"
            );
            if matches!(operation, ConfigPatchOperation::Remove { .. }) {
                anyhow::ensure!(
                    result.get("populated") == Some(&Value::Bool(false)),
                    "native secret reset result remains populated"
                );
            }
            result.remove("value");
        } else {
            anyhow::ensure!(
                result.contains_key("value") && !result.contains_key("populated"),
                "tched_router public patch result was not value-only"
            );
            if let Some(value) = result.get("value") {
                anyhow::ensure!(
                    if matches!(operation, ConfigPatchOperation::Remove { .. }) {
                        value.is_null()
                    } else {
                        value.is_string()
                    },
                    "tched_router patch display value disagrees with operation kind"
                );
            }
        }
    }
    redact_secret_leaves(response);
    Ok(())
}

fn patch_operation_name(operation: &ConfigPatchOperation) -> &'static str {
    match operation {
        ConfigPatchOperation::Add { .. } => "add",
        ConfigPatchOperation::Remove { .. } => "remove",
        ConfigPatchOperation::Replace { .. } => "replace",
        ConfigPatchOperation::Test { .. } => "test",
    }
}

/// A later operation on a path supersedes an earlier result. Compare only the
/// final result for each path to the independently obtained target readback.
/// Native remove returns a reset marker (null), not the resulting default; it
/// establishes a reset receipt plus a current value, not equality to null.
fn verify_patch_readbacks(
    response: &Value,
    readbacks: &[Value],
    operations: &[ConfigPatchOperation],
) -> anyhow::Result<()> {
    let mut structured_paths = BTreeMap::new();
    for operation in operations {
        let (pointer, value, _) = patch_operation_parts(operation);
        structured_paths.insert(
            validate_patch_pointer(pointer)?,
            value.is_some_and(|value| value.is_array() || value.is_object()),
        );
    }
    let results = response
        .get("results")
        .and_then(Value::as_array)
        .context("patch results absent")?;
    let mut final_results = BTreeMap::new();
    for result in results {
        let path = result
            .get("path")
            .and_then(Value::as_str)
            .context("patch result path absent")?;
        final_results.insert(path, result);
    }
    anyhow::ensure!(
        final_results.len() == readbacks.len(),
        "patch target readback count mismatch"
    );
    let mut seen = std::collections::BTreeSet::new();
    for readback in readbacks {
        let path = readback
            .get("path")
            .and_then(Value::as_str)
            .context("patch readback path absent")?;
        anyhow::ensure!(seen.insert(path), "duplicate patch target readback");
        let result = final_results
            .get(path)
            .context("patch readback is not a submitted target")?;
        if result.get("populated").is_some() {
            anyhow::ensure!(
                result.get("populated") == readback.get("populated")
                    && !readback
                        .as_object()
                        .is_some_and(|o| o.contains_key("value")),
                "native secret patch completion disagrees with target readback; reconcile before retry"
            );
        } else if result.get("op").and_then(Value::as_str) == Some("remove") {
            anyhow::ensure!(
                result.get("value") == Some(&Value::Null)
                    && readback.get("value").is_some_and(Value::is_string),
                "native reset receipt or current target readback is invalid; reconcile before retry"
            );
        } else {
            anyhow::ensure!(
                config_patch_display_values_agree(
                    result.get("value"),
                    readback.get("value"),
                    structured_paths.get(path).copied().unwrap_or(false),
                ) && !readback
                    .as_object()
                    .is_some_and(|o| o.contains_key("populated")),
                "native patch completion disagrees with target readback; reconcile before retry"
            );
        }
    }
    Ok(())
}

// Protobuf Struct carries all numbers as doubles. Preserve their value while
// restoring exact, safely representable integers for the native typed setter.
fn normalize_config_patch_numbers(value: &mut Value) {
    match value {
        Value::Number(number) if number.is_f64() => {
            if let Some(number) = number.as_f64()
                && number.fract() == 0.0
                && number.abs() <= 9_007_199_254_740_991.0
            {
                *value = Value::Number((number as i64).into());
            }
        }
        Value::Array(values) => values.iter_mut().for_each(normalize_config_patch_numbers),
        Value::Object(values) => values.values_mut().for_each(normalize_config_patch_numbers),
        _ => {}
    }
}

fn config_patch_display_values_agree(
    receipt: Option<&Value>,
    readback: Option<&Value>,
    structured: bool,
) -> bool {
    if !structured {
        return receipt == readback;
    }
    let decode = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .and_then(|value| serde_json::from_str::<Value>(value).ok())
            .filter(|value| value.is_array() || value.is_object())
    };
    match (decode(receipt), decode(readback)) {
        (Some(receipt), Some(readback)) => receipt == readback,
        _ => false,
    }
}

fn validate_init_response(response: &Value) -> anyhow::Result<()> {
    let initialized = response
        .get("initialized")
        .and_then(Value::as_array)
        .context("tched_router config init response has no initialized array")?;
    anyhow::ensure!(
        initialized.len() <= 256,
        "too many initialized config sections"
    );
    for value in initialized {
        validate_config_path(
            value
                .as_str()
                .context("tched_router config init response contains a non-string section")?,
        )
        .context("tched_router config init response contains an invalid section")?;
    }
    Ok(())
}

fn validate_config_path(path: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !path.is_empty()
            && path.len() <= MAX_CONFIG_PATH_BYTES
            && path.split('.').all(valid_config_segment),
        "invalid tched_router config property path"
    );
    Ok(())
}

fn validate_gateway_request(request: &GatewayJsonRequest<'_>) -> anyhow::Result<()> {
    let path = request.relative_path.as_bytes();
    anyhow::ensure!(
        path.len() <= MAX_GATEWAY_PATH_BYTES
            && request.relative_path.starts_with("/api/")
            && !request
                .relative_path
                .chars()
                .any(|character| matches!(character, '?' | '#' | '\\'))
            && !request.relative_path.contains("://"),
        "native gateway route is not a bounded relative /api path"
    );
    anyhow::ensure!(
        request
            .relative_path
            .split('/')
            .all(|segment| !matches!(segment, "." | "..")),
        "native gateway route contains traversal"
    );
    let mut bytes = path.iter().copied();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes.next().context("incomplete route percent escape")?;
            let low = bytes.next().context("incomplete route percent escape")?;
            anyhow::ensure!(
                high.is_ascii_hexdigit() && low.is_ascii_hexdigit(),
                "invalid route percent escape"
            );
        } else {
            anyhow::ensure!(
                byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.' | b'~'),
                "native gateway route contains an unencoded character"
            );
        }
    }
    for segment in request.relative_path.split('/') {
        let mut decoded = Vec::with_capacity(segment.len());
        let bytes = segment.as_bytes();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'%' {
                let value = |byte: u8| match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    b'A'..=b'F' => byte - b'A' + 10,
                    _ => unreachable!("percent escapes validated above"),
                };
                decoded.push((value(bytes[index + 1]) << 4) | value(bytes[index + 2]));
                index += 3;
            } else {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
        anyhow::ensure!(
            !matches!(decoded.as_slice(), b"." | b".."),
            "native gateway route contains encoded traversal"
        );
    }
    anyhow::ensure!(
        request.query.len() <= MAX_GATEWAY_QUERY_PAIRS
            && request.query.iter().all(|(key, value)| {
                !key.is_empty()
                    && key.len() <= MAX_GATEWAY_QUERY_KEY_BYTES
                    && value.len() <= MAX_GATEWAY_QUERY_VALUE_BYTES
                    && key.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    })
            }),
        "native gateway query exceeds its bounds or has an invalid key"
    );
    if let Some(body) = request.body {
        anyhow::ensure!(
            serde_json::to_vec(body)?.len() <= MAX_GATEWAY_REQUEST_BYTES,
            "native gateway request body exceeds {MAX_GATEWAY_REQUEST_BYTES} bytes"
        );
    }
    anyhow::ensure!(
        !matches!(request.method, GatewayMethod::Get) || !request.mutation,
        "GET cannot be classified as a native mutation"
    );
    Ok(())
}

fn gateway_response_failure(
    request: &GatewayJsonRequest<'_>,
    detail: String,
) -> GatewayRequestError {
    if request.mutation {
        GatewayRequestError::IndeterminateMutation(format!(
            "{} may have completed but its {detail}",
            request.operation
        ))
    } else {
        GatewayRequestError::InvalidResponse(format!("{} {detail}", request.operation))
    }
}

fn gateway_status_failure(
    request: &GatewayJsonRequest<'_>,
    status: reqwest::StatusCode,
    detail: String,
) -> GatewayRequestError {
    if request.mutation && status.is_server_error() {
        GatewayRequestError::IndeterminateMutation(format!(
            "{} may have completed before HTTP {}",
            request.operation,
            status.as_u16()
        ))
    } else {
        GatewayRequestError::ProviderRejected {
            status: status.as_u16(),
            detail,
        }
    }
}

fn config_gateway_error(error: GatewayRequestError) -> anyhow::Error {
    match error {
        GatewayRequestError::BeforeEffect(_) => {
            anyhow!("native config request failed before submission")
        }
        GatewayRequestError::ProviderRejected { status, .. } => {
            anyhow!("native config request was rejected with HTTP {status}")
        }
        GatewayRequestError::InvalidResponse(_) => {
            anyhow!("native config read returned an invalid response")
        }
        GatewayRequestError::IndeterminateMutation(_) => anyhow!(
            "native config mutation outcome is indeterminate; observe current config before retrying"
        ),
    }
}

fn redact_secret_leaves(value: &mut Value) {
    const SECRET_KEYS: &[&str] = &[
        "api_key",
        "apikey",
        "token",
        "secret",
        "password",
        "private_key",
    ];
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                let key = key.to_ascii_lowercase();
                if value.is_string() && SECRET_KEYS.iter().any(|part| key.contains(part)) {
                    *value = Value::String("***MASKED***".to_string());
                } else {
                    redact_secret_leaves(value);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact_secret_leaves),
        _ => {}
    }
}

fn parse_configured_providers(
    entries: Vec<ConfigListEntry>,
) -> anyhow::Result<Vec<ConfiguredProvider>> {
    let mut providers = BTreeMap::new();
    for entry in entries {
        let Some(path) = entry.path.strip_prefix("providers.models.") else {
            continue;
        };
        let Some(reference) = path.strip_suffix(".model") else {
            continue;
        };
        let (family, alias) = reference.split_once('.').ok_or_else(|| {
            anyhow!("tched_router configured provider path has no family/alias binding")
        })?;
        anyhow::ensure!(
            !family.is_empty() && !alias.is_empty() && !alias.split('.').any(str::is_empty),
            "tched_router configured provider path has an empty family or alias segment"
        );
        let selected_model = entry
            .value
            .and_then(|value| value.as_str().map(str::to_string))
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                anyhow!("tched_router configured provider model is missing a string value")
            })?;
        anyhow::ensure!(
            providers
                .insert(
                    reference.to_string(),
                    ConfiguredProvider {
                        family: family.to_string(),
                        reference: reference.to_string(),
                        selected_model,
                    },
                )
                .is_none(),
            "tched_router configured provider catalog contains a duplicate reference"
        );
    }
    Ok(providers.into_values().collect())
}

fn is_door_selectable_route(provider: &ConfiguredProvider) -> bool {
    is_door_selectable_reference(&provider.reference)
}

fn binding_profiles(
    config: &Value,
    configured: Vec<ConfiguredProvider>,
    target: &crate::tched_catalog_binding::BindingTarget,
    opencode_only: bool,
) -> anyhow::Result<Vec<ConfiguredProvider>> {
    let mut private = target.excluded_references.clone();
    let agents = config
        .get("agents")
        .and_then(Value::as_object)
        .context("catalog binding native agent inventory is absent")?;
    for (alias, agent) in agents {
        if alias != DEFAULT_AGENT_ALIAS
            && let Some(reference) = agent.get("model_provider").and_then(Value::as_str)
        {
            private.insert(reference.to_string());
        }
    }
    let mut eligible = Vec::new();
    for provider in configured {
        anyhow::ensure!(
            crate::tched_catalog_binding::valid_reference(&provider.reference),
            "catalog binding native profile reference is invalid"
        );
        // The provider's serve family bounds eligibility: a profile outside
        // the target family is never a binding candidate, so one live model
        // name listed by an unrelated family cannot create an ambiguous
        // "unique" match.
        if !private.contains(&provider.reference)
            && (!opencode_only || is_door_selectable_route(&provider))
            && crate::tched_catalog_binding::reference_family(&provider.reference)
                == Some(target.family.as_str())
            && target
                .ordinary_reference
                .as_ref()
                .is_none_or(|reference| reference == &provider.reference)
        {
            eligible.push(provider);
        }
    }
    Ok(eligible)
}

/// Upstream provider profiles a door caller may select: OpenCode (the Go
/// plan's `opencode.go_*` chat/Responses profiles and `anthropic.go_*`
/// Messages-API profiles, and the older `custom.opencode_*` session
/// profiles) and Cheaper Inference. Any other configured provider, such as
/// a hand-made native default, is the router's own and not door-selectable.
fn is_door_selectable_reference(reference: &str) -> bool {
    reference.starts_with("custom.opencode_")
        || reference.starts_with("opencode.go_")
        || reference.starts_with("anthropic.go_")
        || reference == "custom.cheaper_inference"
}

/// Whether the door must present `selected_ref` as the router's own default:
/// the agent's native selection is not a profile the door can select.
fn is_router_default(selected_ref: &str, opencode_only: bool) -> bool {
    opencode_only && !is_door_selectable_reference(selected_ref)
}

fn publish_providers(
    configured: Vec<ConfiguredProvider>,
    selected_ref: &str,
    opencode_only: bool,
) -> Vec<ConfiguredProvider> {
    if !opencode_only {
        return configured;
    }
    let mut published = Vec::with_capacity(configured.len());
    for provider in configured {
        if is_door_selectable_route(&provider) {
            published.push(provider);
        } else if provider.reference == selected_ref {
            // The router's own default keeps its real provider family, so the
            // picker names the provider that actually serves it.
            published.insert(0, provider);
        }
    }
    published
}

/// The router default is not a door-selectable profile, so the door never
/// rewrites its model.
fn ensure_router_default_model(
    provider: &ConfiguredProvider,
    model_id: &str,
    opencode_only: bool,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !is_router_default(&provider.reference, opencode_only)
            || provider.selected_model == model_id,
        "the router default keeps its own model; select a configured provider to choose a model"
    );
    Ok(())
}

fn validate_loopback_endpoint(endpoint: &str) -> Result<(), String> {
    let url = reqwest::Url::parse(endpoint)
        .map_err(|error| format!("invalid tched_router runtime endpoint: {error}"))?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err("tched_router runtime endpoint must be an HTTP loopback origin".to_string());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "tched_router runtime endpoint has no host".to_string())?;
    let address = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<std::net::IpAddr>()
        .map_err(|_| "tched_router runtime endpoint host must be an IP literal".to_string())?;
    if !address.is_loopback() {
        return Err("tched_router runtime endpoint must use a loopback address".to_string());
    }
    Ok(())
}

fn provider_model_path(reference: &str) -> anyhow::Result<String> {
    let (family, alias) = reference
        .split_once('.')
        .context("tched_router provider reference has no family/alias binding")?;
    anyhow::ensure!(
        !family.is_empty()
            && !alias.is_empty()
            && family
                .bytes()
                .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_') })
            && alias.split('.').all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            }),
        "tched_router provider reference cannot be addressed through the config API"
    );
    Ok(format!("providers.models.{reference}.model"))
}

fn selection_patch(
    provider_path: &str,
    model_path: &str,
    current_provider: &str,
    current_model: &str,
    target_provider: &str,
    target_model: &str,
) -> Value {
    json!([
        { "op": "test", "path": config_pointer(provider_path), "value": current_provider },
        { "op": "test", "path": config_pointer(model_path), "value": current_model },
        { "op": "replace", "path": config_pointer(provider_path), "value": target_provider },
        { "op": "replace", "path": config_pointer(model_path), "value": target_model }
    ])
}

fn property_patch(path: &str, current: &str, target: &str) -> Value {
    json!([
        { "op": "test", "path": config_pointer(path), "value": current },
        { "op": "replace", "path": config_pointer(path), "value": target }
    ])
}

fn model_patch(
    provider_path: &str,
    current_provider: &str,
    model_path: &str,
    current_model: &str,
    target_model: &str,
) -> Value {
    json!([
        { "op": "test", "path": config_pointer(provider_path), "value": current_provider },
        { "op": "test", "path": config_pointer(model_path), "value": current_model },
        { "op": "replace", "path": config_pointer(model_path), "value": target_model }
    ])
}

fn config_pointer(path: &str) -> String {
    format!(
        "/{}",
        path.split('.')
            .map(|part| part.replace('~', "~0").replace('/', "~1"))
            .collect::<Vec<_>>()
            .join("/")
    )
}

fn conversation_prompt(input: &ChatInput) -> anyhow::Result<String> {
    if input.messages.is_empty() {
        if input.message.trim().is_empty() {
            return Err(anyhow!("tched_router.Chat requires message or messages"));
        }
        return Ok(input.message.clone());
    }
    let mut prompt = String::from(
        "Continue the following ordered conversation. Respond only as the assistant.\n\n",
    );
    for message in &input.messages {
        prompt.push('[');
        prompt.push_str(message.role.trim());
        prompt.push_str("]\n");
        prompt.push_str(&message.content);
        prompt.push_str("\n\n");
    }
    Ok(prompt)
}

fn extract_chat_text(response: &Value) -> anyhow::Result<String> {
    response
        .pointer("/result/artifacts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|artifact| {
            artifact
                .get("parts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .find_map(|part| {
            (part.get("kind").and_then(Value::as_str) == Some("text"))
                .then(|| part.get("text").and_then(Value::as_str))
                .flatten()
        })
        .map(str::to_string)
        .ok_or_else(|| anyhow!("tched_router chat response contains no text artifact"))
}

fn truncate_error(detail: &str) -> String {
    const LIMIT: usize = 512;
    let mut end = detail.len().min(LIMIT);
    while end > 0 && !detail.is_char_boundary(end) {
        end -= 1;
    }
    let suffix = if end < detail.len() { "…" } else { "" };
    format!("{}{}", &detail[..end], suffix)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding_target() -> crate::tched_catalog_binding::BindingTarget {
        crate::tched_catalog_binding::BindingTarget {
            provider_id: "openai".into(),
            model_id: "test-model".into(),
            inference_url_prefix: "https://inference.example/v1".into(),
            catalog_revision: 7,
            ordinary_reference: None,
            excluded_references: Default::default(),
        }
    }

    #[test]
    fn catalog_binding_policy_respects_flag_and_never_overrides_private_exclusion() {
        let config = json!({"agents":{"dashboard":{"model_provider":"openai.dashboard"}, "control_plane":{"model_provider":"custom.private"}}});
        let configured = || {
            vec![
                ConfiguredProvider {
                    family: "openai".into(),
                    reference: "openai.dashboard".into(),
                    selected_model: "test-model".into(),
                },
                ConfiguredProvider {
                    family: "custom".into(),
                    reference: "custom.private".into(),
                    selected_model: "test-model".into(),
                },
                ConfiguredProvider {
                    family: "opencode".into(),
                    reference: "opencode.go_dashboard".into(),
                    selected_model: "test-model".into(),
                },
            ]
        };
        assert_eq!(
            binding_profiles(&config, configured(), &binding_target(), false)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            binding_profiles(&config, configured(), &binding_target(), true)
                .unwrap()
                .len(),
            1
        );
        let mut target = binding_target();
        target.ordinary_reference = Some("custom.private".into());
        assert!(
            binding_profiles(&config, configured(), &target, false)
                .unwrap()
                .is_empty()
        );
        target.ordinary_reference = Some("openai.dashboard".into());
        assert!(
            binding_profiles(&config, configured(), &target, true)
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn catalog_binding_observation_rejects_changed_config_and_masks_secrets() {
        use axum::{Json, extract::State, routing::get};
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        #[derive(Clone)]
        struct StateData {
            reads: Arc<AtomicUsize>,
            drift: bool,
        }
        async fn config(State(state): State<StateData>) -> Json<Value> {
            let read = state.reads.fetch_add(1, Ordering::SeqCst);
            Json(
                json!({"agents":{"dashboard":{"model_provider":"openai.dashboard"}}, "providers":{"models":{"openai":{"dashboard":{"model":"test-model", "uri":"https://inference.example/v1", "api_key":"SECRET"}}}}, "generation":if state.drift && read > 0 { 2 } else { 1 }}),
            )
        }
        for drift in [false, true] {
            let reads = Arc::new(AtomicUsize::new(0));
            let app = axum::Router::new()
                .route("/health", get(|| async { Json(json!({"status":"ok"})) }))
                .route("/api/config", get(config))
                .route("/api/config/list", get(|| async { Json(json!({"entries":[{"path":"providers.models.openai.dashboard.model", "value":"test-model"}]})) }))
                .route("/api/config/catalog/models", get(|| async { Json(json!({"models":["test-model"], "live":true})) }))
                .layer(axum::middleware::from_fn(|request: axum::extract::Request, next: axum::middleware::Next| async move {
                    assert_eq!(request.headers().get("x-opdbus-sealed-id-bin").unwrap(), "fixture-caller-sid1");
                    next.run(request).await
                }))
                .with_state(StateData {reads:reads.clone(), drift});
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            // A private test profile that current policy accepts independently
            // of ambient routing restrictions; no env variables are changed.
            let client = TchedRouterRuntimeClient::new(
                endpoint,
                "control_plane".into(),
                Some("test-token".into()),
            )
            .with_sealed_identity(Some("fixture-caller-sid1"));
            let output = client
                .observe_catalog_binding(&binding_target(), &StaticSecretPolicy)
                .await;
            assert_eq!(reads.load(Ordering::SeqCst), 2);
            if drift {
                assert!(
                    output
                        .unwrap_err()
                        .to_string()
                        .contains("changed during observation")
                );
            } else {
                let output = output.unwrap();
                assert_eq!(output.binding.reference, "openai.dashboard");
                assert_eq!(output.binding.catalog_revision, 7);
                assert!(!output.native_config.to_string().contains("SECRET"));
            }
            server.abort();
        }
    }
    use futures::{SinkExt, StreamExt};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_tungstenite::{
        accept_hdr_async,
        tungstenite::{
            Message,
            handshake::server::{Request, Response},
        },
    };

    #[test]
    fn chat_selector_accepts_exact_native_reference_without_confusing_catalog_keys() {
        let selected = provider("opencode", "opencode.go_dashboard", "current-model");
        for selector in ["", "opencode", "opencode.go_dashboard"] {
            assert!(matches_provider_selector(selector, &selected));
        }
        for selector in [
            "opencode_go",
            "opencode.go",
            "opencode.go_control_plane",
            "custom.go_dashboard",
        ] {
            assert!(!matches_provider_selector(selector, &selected));
        }
    }

    #[tokio::test]
    async fn a2a_forwards_canonical_sid1_and_requires_native_ack() {
        use axum::{Json, Router, http::HeaderMap, response::IntoResponse, routing::post};

        for acknowledge in [false, true] {
            let app = Router::new().route(
                "/a2a/dashboard",
                post(move |headers: HeaderMap| async move {
                    assert_eq!(
                        headers
                            .get("x-opdbus-sealed-id-bin")
                            .and_then(|value| value.to_str().ok()),
                        Some("canonical-fixture-sid1")
                    );
                    let mut response = Json(serde_json::json!({"result":"ok"})).into_response();
                    if acknowledge {
                        response.headers_mut().insert(
                            "x-opdbus-identity-authenticated",
                            axum::http::HeaderValue::from_static("SID1"),
                        );
                    }
                    response
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            let runtime = TchedRouterRuntimeClient::new(endpoint.clone(), "dashboard".into(), None)
                .with_sealed_identity(Some("canonical-fixture-sid1"));
            let response: anyhow::Result<Value> = runtime
                .send_json(
                    runtime
                        .http()
                        .unwrap()
                        .post(format!("{endpoint}/a2a/dashboard")),
                    "chat",
                    Duration::from_secs(2),
                    1024,
                )
                .await;
            if acknowledge {
                assert_eq!(response.unwrap(), serde_json::json!({"result":"ok"}));
            } else {
                assert!(
                    response
                        .unwrap_err()
                        .to_string()
                        .contains("did not authenticate")
                );
            }
            server.abort();
        }
    }

    struct AllowGenerationAdmission;
    impl crate::tched_router_generation_native::NativeGenerationAdmission for AllowGenerationAdmission {
        fn check(
            &self,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<
                            (),
                            crate::tched_router_generation_native::NativeGenerationError,
                        >,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async { Ok(()) })
        }
    }

    // Tungstenite fixes the handshake callback's error type to a large HTTP response.
    #[allow(clippy::result_large_err)]
    async fn complete_fixture(
        alias: &str,
        provider_ref: &str,
        public_provider: &str,
        ack_provider: &str,
    ) -> anyhow::Result<ChatOutput> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let alias = alias.to_owned();
        let provider_ref = provider_ref.to_owned();
        let ack_provider = ack_provider.to_owned();
        let server_alias = alias.clone();
        let server = tokio::spawn(async move {
            for _ in 0..4 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut peek = [0u8; 8];
                let n = stream.peek(&mut peek).await.unwrap();
                if std::str::from_utf8(&peek[..n])
                    .unwrap_or_default()
                    .starts_with("GET /ws")
                {
                    let socket =
                        accept_hdr_async(stream, |request: &Request, mut response: Response| {
                            assert!(
                                request
                                    .uri()
                                    .query()
                                    .unwrap_or_default()
                                    .contains(&format!("agent_alias={server_alias}"))
                            );
                            response.headers_mut().insert(
                                "Sec-WebSocket-Protocol",
                                "tched_router.generation.v1".parse().unwrap(),
                            );
                            Ok(response)
                        })
                        .await
                        .unwrap();
                    let (mut sink, mut incoming) = socket.split();
                    let connect = incoming.next().await.unwrap().unwrap();
                    let connect: Value = serde_json::from_str(connect.to_text().unwrap()).unwrap();
                    assert_eq!(connect["type"], "connect");
                    sink.send(Message::Text(json!({"type":"session_start","session_id":connect["session_id"],"resumed":false,"message_count":0,"stateless_generation":1}).to_string())).await.unwrap();
                    sink.send(Message::Text(format!(r#"{{"type":"connected","stateless_generation":1,"provider":"{}","model":"fixture-model"}}"#, ack_provider))).await.unwrap();
                    let prompt =
                        tokio::time::timeout(Duration::from_secs(2), incoming.next()).await;
                    if ack_provider != provider_ref {
                        assert!(
                            !matches!(prompt, Ok(Some(Ok(Message::Text(_))))),
                            "wrong ACK released a prompt"
                        );
                        return;
                    }
                    let prompt = prompt.unwrap().unwrap().unwrap();
                    let prompt: Value = serde_json::from_str(prompt.to_text().unwrap()).unwrap();
                    assert_eq!(prompt["type"], "message");
                    assert_eq!(prompt["content"], "hello");
                    sink.send(Message::Text(
                        r#"{"type":"chunk","content":"fixture answer"}"#.into(),
                    ))
                    .await
                    .unwrap();
                    sink.send(Message::Text(format!(r#"{{"type":"done","full_response":"fixture answer","provider":"{}","model":"fixture-model","tokens_used":4}}"#, ack_provider))).await.unwrap();
                } else {
                    let mut buffer = vec![0u8; 4096];
                    let n = stream.read(&mut buffer).await.unwrap();
                    let request = String::from_utf8_lossy(&buffer[..n]);
                    let path = request
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or_default();
                    let body = if path.starts_with("/health") {
                        r#"{"status":"ok"}"#.to_string()
                    } else if path.starts_with("/api/config/prop")
                        && path.contains("model_provider")
                    {
                        format!(r#"{{"value":"{}"}}"#, provider_ref)
                    } else if path.starts_with("/api/config/prop") {
                        r#"{"value":"fixture-model"}"#.to_string()
                    } else {
                        format!(
                            r#"{{"entries":[{{"path":"providers.models.{}.model","value":"fixture-model"}}]}}"#,
                            provider_ref
                        )
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).await.unwrap();
                }
            }
        });
        let client = TchedRouterRuntimeClient::new(
            format!("http://{address}"),
            alias,
            Some("fixture-token".into()),
        );
        let output = client
            .complete_under_guard(
                ChatInput {
                    message: "hello".into(),
                    provider: public_provider.into(),
                    model: "fixture-model".into(),
                    ..Default::default()
                },
                std::sync::Arc::new(AllowGenerationAdmission),
            )
            .await;
        server.await.unwrap();
        output
    }

    #[tokio::test]
    async fn provider_only_wrapper_accepts_exact_role_ack_and_returns_public_family() {
        let output = complete_fixture(
            "accountability",
            "openrouter.accountability",
            "openrouter",
            "openrouter.accountability",
        )
        .await
        .unwrap();
        assert_eq!(output.content, "fixture answer");
        assert_eq!(output.provider, "openrouter");
        assert_eq!(output.model, "fixture-model");
    }

    #[tokio::test]
    async fn provider_only_wrapper_accepts_custom_family_against_exact_reference() {
        let output = complete_fixture("dashboard", "custom.salad", "custom", "custom.salad")
            .await
            .unwrap();
        assert_eq!(output.provider, "custom");
    }

    #[tokio::test]
    async fn provider_only_wrapper_rejects_wrong_native_ack_before_prompt() {
        let error = complete_fixture(
            "accountability",
            "openrouter.accountability",
            "openrouter",
            "openrouter.other",
        )
        .await
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("native generation protocol rejected")
        );
    }

    #[tokio::test]
    async fn oversized_chat_request_is_rejected_before_native_io() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = TchedRouterRuntimeClient::new(
            format!("http://{}", listener.local_addr().unwrap()),
            "test".into(),
            None,
        );
        let error = client
            .chat_under_guard(ChatInput {
                message: "x".repeat(MAX_CHAT_REQUEST_BYTES),
                ..Default::default()
            })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("native chat request exceeds"));
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    }
    use op_plugins::state_plugins::tched_router::TchedChatMessage;

    struct StaticSecretPolicy;
    impl ConfigSecretPolicy for StaticSecretPolicy {
        fn is_secret(&self, path: &str) -> anyhow::Result<bool> {
            Ok(path.ends_with("api_key"))
        }
        fn redact_config(&self, config: &mut Value) -> anyhow::Result<()> {
            redact_secret_leaves(config);
            Ok(())
        }
    }

    #[test]
    fn configured_provider_catalog_is_derived_from_runtime_paths() {
        let providers = parse_configured_providers(vec![
            ConfigListEntry {
                path: "providers.models.opencode.go.model".to_string(),
                value: Some(json!("deepseek-v4-flash-free")),
            },
            ConfigListEntry {
                path: "providers.models.opencode.go.api_key".to_string(),
                value: None,
            },
        ])
        .unwrap();
        assert_eq!(
            providers,
            vec![ConfiguredProvider {
                family: "opencode".to_string(),
                reference: "opencode.go".to_string(),
                selected_model: "deepseek-v4-flash-free".to_string(),
            }]
        );
    }

    fn provider(family: &str, reference: &str, model: &str) -> ConfiguredProvider {
        ConfiguredProvider {
            family: family.to_string(),
            reference: reference.to_string(),
            selected_model: model.to_string(),
        }
    }

    #[test]
    fn opencode_only_publishes_an_unselectable_native_selection_as_the_router_default() {
        let configured = vec![
            provider("custom", "custom.opencode_dashboard", "nemotron"),
            provider("openai", "openai.chatgpt", "gpt"),
            provider("opencode", "opencode.go", "space-bunny-free"),
        ];
        assert_eq!(
            publish_providers(configured.clone(), "opencode.go", true),
            // The router default leads under its real family, never a
            // router-invented label.
            vec![
                provider("opencode", "opencode.go", "space-bunny-free"),
                provider("custom", "custom.opencode_dashboard", "nemotron"),
            ]
        );
        assert_eq!(
            publish_providers(configured.clone(), "custom.opencode_dashboard", true),
            vec![provider("custom", "custom.opencode_dashboard", "nemotron")]
        );
        assert_eq!(
            publish_providers(configured.clone(), "opencode.go", false),
            configured
        );
        // A native selection absent from the configuration publishes no
        // router default at all.
        assert!(
            publish_providers(configured, "ollama.main", true)
                .iter()
                .all(is_door_selectable_route)
        );
    }

    #[test]
    fn router_default_model_cannot_be_changed_through_the_door() {
        let default = provider("opencode", "opencode.go", "space-bunny-free");
        assert!(ensure_router_default_model(&default, "space-bunny-free", true).is_ok());
        assert!(ensure_router_default_model(&default, "other", true).is_err());
        let selectable = provider("custom", "custom.opencode_dashboard", "nemotron");
        assert!(ensure_router_default_model(&selectable, "other", true).is_ok());
        // Without the OpenCode-only door, every provider keeps its models
        // selectable, as before.
        assert!(ensure_router_default_model(&default, "other", false).is_ok());
    }

    #[test]
    fn the_door_selects_opencode_go_and_cheaper_inference_profiles() {
        for reference in [
            "opencode.go_dashboard",
            "opencode.go_cp_route_grok_47",
            "anthropic.go_cp_route_qwen38_flash",
            "custom.opencode_dashboard",
            "custom.cheaper_inference",
        ] {
            assert!(is_door_selectable_reference(reference), "{reference}");
            assert!(!is_router_default(reference, true), "{reference}");
        }
        for reference in [
            "opencode.go",
            "openai.chatgpt",
            "anthropic.main",
            "custom.other",
        ] {
            assert!(!is_door_selectable_reference(reference), "{reference}");
        }
        let configured = vec![
            provider("opencode", "opencode.go_dashboard", "glm-5.3-flash"),
            provider("custom", "custom.cheaper_inference", "gpt-5.4-mini"),
            provider("openai", "openai.chatgpt", "gpt"),
        ];
        assert_eq!(
            publish_providers(configured, "opencode.go_dashboard", true),
            vec![
                provider("opencode", "opencode.go_dashboard", "glm-5.3-flash"),
                provider("custom", "custom.cheaper_inference", "gpt-5.4-mini"),
            ]
        );
    }

    #[test]
    fn router_default_is_only_an_unselectable_native_provider_behind_the_door() {
        assert!(is_router_default("opencode.go", true));
        assert!(!is_router_default("custom.opencode_dashboard", true));
        assert!(!is_router_default("opencode.go", false));
    }

    #[test]
    fn configured_provider_catalog_rejects_missing_or_malformed_model_entries() {
        assert!(parse_configured_providers(Vec::new()).unwrap().is_empty());
        assert!(
            parse_configured_providers(vec![ConfigListEntry {
                path: "providers.models.opencode.go.model".into(),
                value: None,
            }])
            .is_err()
        );
        assert!(
            parse_configured_providers(vec![ConfigListEntry {
                path: "providers.models.opencode..model".into(),
                value: Some(json!("model")),
            }])
            .is_err()
        );
    }

    #[test]
    fn provider_model_path_is_exactly_profile_scoped() {
        assert_eq!(
            provider_model_path("salad.default").unwrap(),
            "providers.models.salad.default.model"
        );
        assert!(provider_model_path("salad").is_err());
        assert!(provider_model_path("salad.bad/path").is_err());
    }

    #[test]
    fn runtime_endpoint_accepts_only_literal_loopback_http_origins() {
        assert!(validate_loopback_endpoint("http://127.0.0.1:8084").is_ok());
        assert!(validate_loopback_endpoint("http://[::1]:8084").is_ok());
        assert!(validate_loopback_endpoint("http://localhost:8084").is_err());
        assert!(validate_loopback_endpoint("http://10.0.0.1:8084").is_err());
        assert!(validate_loopback_endpoint("https://127.0.0.1:8084").is_err());
        assert!(validate_loopback_endpoint("http://user@127.0.0.1:8084").is_err());
        assert!(validate_loopback_endpoint("http://127.0.0.1:8084/config").is_err());
    }

    #[test]
    fn config_paths_are_bounded_dotted_identifiers() {
        assert!(validate_config_path("providers.models.open-ai.work.model").is_ok());
        assert!(validate_config_path("").is_err());
        assert!(validate_config_path("providers..model").is_err());
        assert!(validate_config_path("providers/model").is_err());
        assert!(validate_config_path(&"x".repeat(MAX_CONFIG_PATH_BYTES + 1)).is_err());
    }

    #[test]
    fn shared_gateway_transport_accepts_only_bounded_adapter_routes() {
        let valid = GatewayJsonRequest {
            method: GatewayMethod::Delete,
            relative_path: "/api/cron/jobs/job%2Fone",
            query: &[("project", "default")],
            body: None,
            operation: "delete cron job",
            profile: GatewayProfile::Config,
            mutation: true,
        };
        assert!(validate_gateway_request(&valid).is_ok());
        for path in [
            "http://127.0.0.1/api/cron",
            "/api/cron/../config",
            "/api/cron/job?token=x",
            "/api/cron/job%2",
            "/api/cron/%2e%2E/config",
            "/api/cron/.%2e/config",
            "/api/cron/raw value",
        ] {
            let invalid = GatewayJsonRequest {
                relative_path: path,
                ..valid
            };
            assert!(
                validate_gateway_request(&invalid).is_err(),
                "accepted {path}"
            );
        }
    }

    #[test]
    fn mutation_response_failures_are_never_reported_before_effect() {
        let mutation = GatewayJsonRequest {
            method: GatewayMethod::Patch,
            relative_path: "/api/config",
            query: &[],
            body: None,
            operation: "patch config",
            profile: GatewayProfile::Config,
            mutation: true,
        };
        assert!(matches!(
            gateway_response_failure(&mutation, "truncated response".into()),
            GatewayRequestError::IndeterminateMutation(_)
        ));
        let read = GatewayJsonRequest {
            method: GatewayMethod::Get,
            mutation: false,
            ..mutation
        };
        assert!(matches!(
            gateway_response_failure(&read, "invalid JSON".into()),
            GatewayRequestError::InvalidResponse(_)
        ));
        assert!(matches!(
            gateway_status_failure(
                &mutation,
                reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                "failed".into()
            ),
            GatewayRequestError::IndeterminateMutation(_)
        ));
        assert!(matches!(
            gateway_status_failure(
                &mutation,
                reqwest::StatusCode::BAD_REQUEST,
                "rejected".into()
            ),
            GatewayRequestError::ProviderRejected { status: 400, .. }
        ));
    }

    async fn one_raw_gateway_response(
        status: &'static str,
        body: &'static str,
    ) -> (TchedRouterRuntimeClient, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).await.unwrap();
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        (
            TchedRouterRuntimeClient::new(format!("http://{address}"), "fixture".into(), None),
            task,
        )
    }

    #[tokio::test]
    async fn real_http_mutation_failures_preserve_uncertain_outcomes() {
        for (status, body, indeterminate) in [
            ("500 Internal Server Error", r#"{"error":"late"}"#, true),
            ("400 Bad Request", r#"{"error":"rejected"}"#, false),
            ("200 OK", "not-json", true),
        ] {
            let (runtime, task) = one_raw_gateway_response(status, body).await;
            let result = runtime
                .send_gateway_json::<Value>(GatewayJsonRequest {
                    method: GatewayMethod::Post,
                    relative_path: "/api/cron/job/run",
                    query: &[],
                    body: None,
                    operation: "run fixture job",
                    profile: GatewayProfile::Config,
                    mutation: true,
                })
                .await;
            if indeterminate {
                assert!(matches!(
                    result,
                    Err(GatewayRequestError::IndeterminateMutation(_))
                ));
            } else {
                assert!(matches!(
                    result,
                    Err(GatewayRequestError::ProviderRejected { status: 400, .. })
                ));
            }
            task.await.unwrap();
        }
    }

    #[test]
    fn patch_paths_are_bounded_json_pointers() {
        assert_eq!(
            validate_patch_pointer("/providers/models/open-ai/work/model").unwrap(),
            "providers.models.open-ai.work.model"
        );
        assert!(validate_patch_pointer("providers/models").is_err());
        assert!(validate_patch_pointer("/providers//model").is_err());
        assert!(validate_patch_pointer("/providers/~1/model").is_err());
    }

    #[test]
    fn secret_write_guards_reject_placeholders_and_empty_values() {
        assert!(reject_masked_or_empty_secret(&json!("real-value")).is_ok());
        assert!(reject_masked_or_empty_secret(&json!("***MASKED***")).is_err());
        assert!(reject_masked_or_empty_secret(&json!("****")).is_err());
        assert!(reject_masked_or_empty_secret(&json!("")).is_err());
        assert!(reject_masked_or_empty_secret(&Value::Null).is_err());
    }

    #[test]
    fn patch_receipt_rejects_value_bearing_secret_and_wrong_count() {
        let mut secret = json!({
            "saved": true,
            "results": [{"op":"replace", "path":"provider.api_key", "populated":true,
                         "value":"must-not-leave"}],
            "warnings": []
        });
        assert!(
            sanitize_patch_response(
                &mut secret,
                &[ConfigPatchOperation::Replace {
                    path: "/providers/api_key".into(),
                    value: json!("private")
                }],
                &StaticSecretPolicy
            )
            .is_err()
        );

        let mut count = json!({"saved":true, "results":[], "warnings":[]});
        assert!(
            sanitize_patch_response(
                &mut count,
                &[ConfigPatchOperation::Replace {
                    path: "/gateway/port".into(),
                    value: json!(8085)
                }],
                &StaticSecretPolicy
            )
            .is_err()
        );
    }

    #[test]
    fn defense_redaction_preserves_public_values_and_masks_secret_leaves() {
        let mut value = json!({
            "provider": {"model": "safe", "api_key": "must-not-leave"},
            "nested": [{"access_token": "must-not-leave", "enabled": true}],
            "token_budget": 2048
        });
        redact_secret_leaves(&mut value);
        assert_eq!(value["provider"]["model"], "safe");
        assert_eq!(value["provider"]["api_key"], "***MASKED***");
        assert_eq!(value["nested"][0]["access_token"], "***MASKED***");
        assert_eq!(value["token_budget"], 2048);
    }

    #[test]
    fn conversation_history_is_forwarded_in_order() {
        let input = ChatInput {
            messages: vec![
                TchedChatMessage {
                    role: "user".to_string(),
                    content: "first".to_string(),
                },
                TchedChatMessage {
                    role: "assistant".to_string(),
                    content: "second".to_string(),
                },
            ],
            ..Default::default()
        };
        let prompt = conversation_prompt(&input).unwrap();
        assert!(
            prompt.find("[user]\nfirst").unwrap() < prompt.find("[assistant]\nsecond").unwrap()
        );
    }

    #[test]
    fn a2a_text_artifact_is_extracted() {
        let response = json!({
            "result": { "artifacts": [{ "parts": [{ "kind": "text", "text": "PONG" }] }] }
        });
        assert_eq!(extract_chat_text(&response).unwrap(), "PONG");
    }

    #[test]
    fn selection_patch_tests_both_current_values_before_replacements() {
        let patch = selection_patch(
            "agents.dashboard.model_provider",
            "providers.models.target.alias.model",
            "old.family",
            "old-model",
            "target.family.alias",
            "new-model",
        );
        let ops = patch.as_array().unwrap();
        assert_eq!(ops.len(), 4);
        assert_eq!(ops[0]["op"], "test");
        assert_eq!(ops[1]["op"], "test");
        assert_eq!(ops[1]["value"], "old-model");
        assert_eq!(ops[2]["op"], "replace");
        assert_eq!(ops[3]["op"], "replace");
        assert_eq!(ops[3]["path"], "/providers/models/target/alias/model");
    }

    #[test]
    fn single_field_selection_patches_test_before_replace() {
        let provider = property_patch("agents.dashboard.model_provider", "old.main", "new.main");
        assert_eq!(
            provider,
            json!([
                {"op":"test","path":"/agents/dashboard/model_provider","value":"old.main"},
                {"op":"replace","path":"/agents/dashboard/model_provider","value":"new.main"}
            ])
        );

        let model = model_patch(
            "agents.dashboard.model_provider",
            "old.main",
            "providers.models.old.main.model",
            "old-model",
            "new-model",
        );
        assert_eq!(
            model,
            json!([
                {"op":"test","path":"/agents/dashboard/model_provider","value":"old.main"},
                {"op":"test","path":"/providers/models/old/main/model","value":"old-model"},
                {"op":"replace","path":"/providers/models/old/main/model","value":"new-model"}
            ])
        );
    }

    #[test]
    fn config_patch_preserves_integral_struct_numbers_for_native_integer_fields() {
        let mut value = json!({"value": [600.0, -12.0, 1.5, 9_007_199_254_740_992.0]});
        normalize_config_patch_numbers(&mut value);
        assert_eq!(value["value"][0].as_u64(), Some(600));
        assert_eq!(value["value"][1].as_i64(), Some(-12));
        assert_eq!(value["value"][2].as_f64(), Some(1.5));
        assert!(value["value"][3].as_number().unwrap().is_f64());
    }

    #[test]
    fn patch_readback_compares_structured_values_without_relaxing_scalar_strings() {
        let operations = vec![ConfigPatchOperation::Add {
            path: "/risk_profiles/control_plane/allowed_tools".into(),
            value: json!(["one", "two"]),
        }];
        let response = json!({"results":[{
            "op":"add", "path":"risk_profiles.control_plane.allowed_tools",
            "value":"[\"one\",\"two\"]"
        }]});
        let readback = json!({
            "path":"risk_profiles.control_plane.allowed_tools", "kind":"value",
            "value":"[\"one\", \"two\"]"
        });
        verify_patch_readbacks(&response, std::slice::from_ref(&readback), &operations).unwrap();
        let changed = json!({
            "path":"risk_profiles.control_plane.allowed_tools", "kind":"value",
            "value":"[\"two\", \"one\"]"
        });
        assert!(verify_patch_readbacks(&response, &[changed], &operations).is_err());
        let scalar_operations = vec![ConfigPatchOperation::Add {
            path: "/risk_profiles/control_plane/allowed_tools".into(),
            value: json!("[\"one\",\"two\"]"),
        }];
        assert!(verify_patch_readbacks(&response, &[readback], &scalar_operations).is_err());
        assert!(!config_patch_display_values_agree(None, None, true));
        assert!(!config_patch_display_values_agree(
            Some(&json!("invalid")),
            Some(&json!("invalid")),
            true
        ));
    }

    #[test]
    fn patch_receipt_is_bound_to_ordered_operations_and_final_readback() {
        let operations = vec![
            ConfigPatchOperation::Test {
                path: "/gateway/port".into(),
                value: json!(8084),
            },
            ConfigPatchOperation::Replace {
                path: "/gateway/port".into(),
                value: json!(8085),
            },
        ];
        let valid = json!({"saved":true,"results":[
            {"op":"test","path":"gateway.port","value":"8084"},
            {"op":"replace","path":"gateway.port","value":"8085"}
        ]});
        let mut sanitized = valid.clone();
        sanitize_patch_response(&mut sanitized, &operations, &StaticSecretPolicy).unwrap();
        verify_patch_readbacks(
            &sanitized,
            &[json!({"path":"gateway.port","kind":"value","value":"8085"})],
            &operations,
        )
        .unwrap();
        assert!(
            verify_patch_readbacks(
                &sanitized,
                &[json!({"path":"gateway.port","kind":"value","value":"8084"})],
                &operations,
            )
            .is_err()
        );
        assert!(
            verify_patch_readbacks(
                &sanitized,
                &[json!({"path":"gateway.host","kind":"value","value":"8085"})],
                &operations,
            )
            .is_err()
        );
        for mismatch in ["order", "path", "op"] {
            let mut response = valid.clone();
            match mismatch {
                "order" => response["results"].as_array_mut().unwrap().reverse(),
                "path" => response["results"][1]["path"] = json!("gateway.host"),
                _ => response["results"][1]["op"] = json!("add"),
            }
            assert!(
                sanitize_patch_response(&mut response, &operations, &StaticSecretPolicy).is_err()
            );
        }
        let reset = vec![ConfigPatchOperation::Remove {
            path: "/gateway/port".into(),
        }];
        let mut reset_response =
            json!({"saved":true,"results":[{"op":"remove","path":"gateway.port","value":null}]});
        sanitize_patch_response(&mut reset_response, &reset, &StaticSecretPolicy).unwrap();
        // Native reset has a null receipt and a possibly non-null default.
        verify_patch_readbacks(
            &reset_response,
            &[json!({"path":"gateway.port","kind":"value","value":"8084"})],
            &reset,
        )
        .unwrap();
    }

    #[test]
    fn paired_native_config_and_selection_commit_preserves_sibling_children() {
        use crate::state_tree_fold::{StateTreeFold, merge_plugin_object_fields};
        let directory = tempfile::tempdir().unwrap();
        let state_dir = directory.path().to_str().unwrap();
        op_core::projection_shm::prepare_projection_directory_in(state_dir).unwrap();
        let original = json!({
            "selected_provider":"native","selected_model":"old",
            "native_config":{"providers":{"models":{"native":{"model":"old"}}}},
            "memory_namespaces":{"sibling":{"keep":true}},
            "model_assignments":{"vectorization":"embedding-sibling"}
        });
        op_core::projection_shm::update_projection_in(state_dir, "tched_router", |_| {
            Ok(Some(serde_json::to_vec(&original)?))
        })
        .unwrap();
        let paired = NativeConfigObservation {
            state: TchedRouterState::default(),
            native_config: json!({"providers":{"models":{"native":{"model":"new","api_key":"***MASKED***"}}}}),
            represented: json!({"status":"active","selected_provider":"native","selected_model":"new","providers":[],"model_routes":[]}),
        };
        let (_, fields) = paired.into_state_and_fields().unwrap();
        let commit =
            op_core::projection_shm::update_projection_in(state_dir, "tched_router", |previous| {
                let previous: serde_json::Value = serde_json::from_slice(previous.unwrap())?;
                let previous = simd_json::serde::to_owned_value(previous)?;
                let fields = simd_json::serde::to_owned_value(fields)?;
                let folded = merge_plugin_object_fields(
                    Some(&previous),
                    "tched_router",
                    "/org/opdbus/v1/plugins/tched_router",
                    &fields,
                )?;
                let StateTreeFold::Present(state) = folded else {
                    panic!("paired field update missing")
                };
                Ok(Some(simd_json::to_vec(&state)?))
            })
            .unwrap();
        let snapshot = op_core::projection_shm::read_projection_snapshot_in(
            state_dir,
            &["tched_router".into()],
        )
        .unwrap();
        let state: Value = serde_json::from_slice(&snapshot.states["tched_router"]).unwrap();
        assert_eq!(state["selected_model"], "new");
        assert_eq!(
            state["native_config"]["providers"]["models"]["native"]["model"],
            state["selected_model"]
        );
        assert_eq!(state["memory_namespaces"], original["memory_namespaces"]);
        assert_eq!(state["model_assignments"], original["model_assignments"]);
        assert_eq!(
            commit.current.as_ref().unwrap(),
            &snapshot.states["tched_router"]
        );
    }
    // Insert inside tched_router_runtime.rs::tests.
    mod config_http_fixture {
        use super::*;
        use axum::{
            Json, Router,
            extract::{Query, State},
            routing::{get, post},
        };
        use serde_json::json;
        use std::{
            collections::HashMap,
            net::SocketAddr,
            sync::{
                Arc,
                atomic::{AtomicUsize, Ordering},
            },
        };

        struct FixturePolicy;
        impl ConfigSecretPolicy for FixturePolicy {
            fn is_secret(&self, path: &str) -> anyhow::Result<bool> {
                anyhow::ensure!(path != "unknown", "unknown fixture path");
                Ok(path.ends_with("api_key"))
            }
            fn redact_config(&self, config: &mut Value) -> anyhow::Result<()> {
                config["providers"]["api_key"] = json!("***MASKED***");
                Ok(())
            }
        }

        #[derive(Clone)]
        struct FixtureState {
            writes: Arc<AtomicUsize>,
        }

        async fn health() -> Json<Value> {
            Json(json!({"status":"ok"}))
        }
        async fn full() -> Json<Value> {
            Json(json!({"gateway":{"port":8084}, "providers":{"api_key":"provider-leak"}}))
        }
        async fn prop(
            State(state): State<FixtureState>,
            Query(q): Query<HashMap<String, String>>,
        ) -> Json<Value> {
            let path = q.get("path").cloned().unwrap_or_default();
            if path.ends_with("api_key") {
                Json(json!({"path":path, "populated":true}))
            } else {
                let value = if state.writes.load(Ordering::SeqCst) == 0 {
                    "8084"
                } else {
                    "8085"
                };
                Json(json!({"path":path, "value":value, "warnings":[]}))
            }
        }
        async fn list() -> Json<Value> {
            Json(json!({"entries":[
            {"path":"gateway.port","category":"Gateway","kind":"integer","type_hint":"u16",
             "value":"8084","populated":true,"is_secret":false,"enum_variants":[],"tab":"","multiline":false},
            {"path":"providers.api_key","category":"Providers","kind":"string","type_hint":"Option<String>",
             "value":"provider-leak","populated":true,"is_secret":true,"enum_variants":[],"tab":"","multiline":false}
        ],"drifted":[]}))
        }
        async fn put_prop(
            State(state): State<FixtureState>,
            Json(_body): Json<Value>,
        ) -> Json<Value> {
            state.writes.fetch_add(1, Ordering::SeqCst);
            Json(json!({"path":"gateway.port","value":"8085","warnings":[]}))
        }
        async fn patch_config(
            State(state): State<FixtureState>,
            Json(body): Json<Value>,
        ) -> Json<Value> {
            state.writes.fetch_add(1, Ordering::SeqCst);
            let results = body.as_array().unwrap().iter().map(|op| json!({
            "op":op["op"], "path":op["path"].as_str().unwrap().trim_start_matches('/').replace('/', "."),
            "value":"8085"
        })).collect::<Vec<_>>();
            Json(json!({"saved":true,"results":results,"warnings":[]}))
        }
        async fn init(Query(q): Query<HashMap<String, String>>) -> Json<Value> {
            Json(json!({"initialized":[q.get("section").cloned().unwrap_or_default()]}))
        }
        async fn migrate() -> Json<Value> {
            Json(json!({"migrated":false,"schema_version":4}))
        }

        async fn fixture() -> (
            TchedRouterRuntimeClient,
            Arc<AtomicUsize>,
            tokio::task::JoinHandle<()>,
        ) {
            let writes = Arc::new(AtomicUsize::new(0));
            let state = FixtureState {
                writes: writes.clone(),
            };
            let app = Router::new()
                .route("/health", get(health))
                .route("/api/config", get(full).patch(patch_config))
                .route("/api/config/prop", get(prop).put(put_prop))
                .route("/api/config/list", get(list))
                .route("/api/config/init", post(init))
                .route("/api/config/migrate", post(migrate))
                .with_state(state);
            let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
                .await
                .unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let task = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            (
                TchedRouterRuntimeClient::new(endpoint, "dashboard".into(), None),
                writes,
                task,
            )
        }

        #[tokio::test]
        async fn reads_use_native_shapes_and_pinned_secret_policy() {
            let (runtime, _, task) = fixture().await;
            let policy = FixturePolicy;
            let full = runtime.read_masked_config(&policy).await.unwrap();
            assert_eq!(full["providers"]["api_key"], "***MASKED***");
            assert_eq!(full["gateway"]["port"], 8084);
            let list = runtime.read_config_list(None, &policy).await.unwrap();
            assert!(list["entries"][1].get("value").is_none());
            let secret = runtime
                .read_config_property("providers.api_key", &policy)
                .await
                .unwrap();
            assert_eq!(secret["kind"], "secret");
            assert!(secret.get("value").is_none());
            task.abort();
        }

        #[tokio::test]
        async fn mutations_are_bounded_and_read_back_after_effect() {
            let (runtime, writes, task) = fixture().await;
            let policy = FixturePolicy;
            let set = runtime
                .set_config_property(
                    &ConfigSetInput {
                        path: "gateway.port".into(),
                        value: json!(8085),
                    },
                    &policy,
                )
                .await
                .unwrap();
            assert_eq!(set.readback["kind"], "value");
            assert_eq!(writes.load(Ordering::SeqCst), 1);
            let rejected = runtime
                .set_config_property(
                    &ConfigSetInput {
                        path: "providers.api_key".into(),
                        value: json!("***MASKED***"),
                    },
                    &policy,
                )
                .await;
            assert!(rejected.is_err());
            assert_eq!(
                writes.load(Ordering::SeqCst),
                1,
                "validation precedes effects"
            );
            task.abort();
        }

        #[tokio::test]
        async fn patch_init_and_migrate_return_native_receipts_after_readback() {
            let (runtime, writes, task) = fixture().await;
            let policy = FixturePolicy;
            let patch = runtime
                .patch_config(
                    &ConfigPatchInput {
                        operations: vec![ConfigPatchOperation::Replace {
                            path: "/gateway/port".into(),
                            value: json!(8085),
                        }],
                    },
                    &policy,
                )
                .await
                .unwrap();
            assert_eq!(patch.result["saved"], true);
            assert_eq!(patch.readback[0]["kind"], "value");
            assert_eq!(writes.load(Ordering::SeqCst), 1);

            let init = runtime
                .init_config(
                    &ConfigInitInput {
                        section: Some("gateway".into()),
                    },
                    &policy,
                )
                .await
                .unwrap();
            assert_eq!(init.result["initialized"], json!(["gateway"]));
            assert_eq!(init.readback["gateway"]["port"], 8084);

            let nested_init = runtime
                .init_config(
                    &ConfigInitInput {
                        section: Some("memory.settings".into()),
                    },
                    &policy,
                )
                .await
                .unwrap();
            assert_eq!(
                nested_init.result["initialized"],
                json!(["memory.settings"])
            );

            let migrate = runtime
                .migrate_config(&ConfigMigrateInput {}, &policy)
                .await
                .unwrap();
            assert_eq!(migrate.result, json!({"migrated":false,"schema_version":4}));
            assert!(migrate.result.get("backup_path").is_none());
            task.abort();
        }

        #[derive(Clone, Copy)]
        enum McpFixtureMode {
            Honest,
            WrongReceipt,
            MissingReadback,
        }

        #[derive(Clone)]
        struct McpFixtureState {
            names: Arc<Mutex<Vec<String>>>,
            writes: Arc<AtomicUsize>,
            mode: McpFixtureMode,
        }

        struct McpFixturePolicy;

        impl ConfigSecretPolicy for McpFixturePolicy {
            fn is_secret(&self, path: &str) -> anyhow::Result<bool> {
                anyhow::ensure!(
                    path == "mcp.servers.opdbus_compact.name",
                    "path absent from fixture contract"
                );
                Ok(false)
            }

            fn redact_config(&self, _: &mut Value) -> anyhow::Result<()> {
                Ok(())
            }
        }

        async fn mcp_full(State(state): State<McpFixtureState>) -> Json<Value> {
            let names = state.names.lock().await.clone();
            Json(json!({"mcp":{"servers":names.into_iter().map(|name| json!({
                "name":name,"transport":"stdio"
            })).collect::<Vec<_>>()}}))
        }

        async fn mcp_map_key(
            State(state): State<McpFixtureState>,
            Query(q): Query<HashMap<String, String>>,
        ) -> Json<Value> {
            assert_eq!(q.get("path").map(String::as_str), Some("mcp.servers"));
            assert_eq!(q.get("key").map(String::as_str), Some("opdbus_compact"));
            state.writes.fetch_add(1, Ordering::SeqCst);
            let mut names = state.names.lock().await;
            let created = !names.iter().any(|name| name == "opdbus_compact");
            if created && !matches!(state.mode, McpFixtureMode::MissingReadback) {
                names.push("opdbus_compact".into());
            }
            Json(json!({
                "path":if matches!(state.mode, McpFixtureMode::WrongReceipt) { "agents" } else { "mcp.servers" },
                "key":"opdbus_compact","created":created,"warnings":null
            }))
        }

        async fn mcp_fixture(
            mode: McpFixtureMode,
        ) -> (
            TchedRouterRuntimeClient,
            Arc<AtomicUsize>,
            tokio::task::JoinHandle<()>,
        ) {
            let writes = Arc::new(AtomicUsize::new(0));
            let state = McpFixtureState {
                names: Arc::new(Mutex::new(Vec::new())),
                writes: writes.clone(),
                mode,
            };
            let app = Router::new()
                .route("/health", get(health))
                .route("/api/config", get(mcp_full))
                .route("/api/config/map-key", post(mcp_map_key))
                .with_state(state);
            let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
                .await
                .unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
            (
                TchedRouterRuntimeClient::new(endpoint, "dashboard".into(), None),
                writes,
                task,
            )
        }

        #[tokio::test]
        async fn mcp_server_init_uses_native_map_key_and_exact_masked_readback() {
            let (runtime, writes, task) = mcp_fixture(McpFixtureMode::Honest).await;
            let input = ConfigInitInput {
                section: Some("mcp.servers.opdbus_compact".into()),
            };
            let first = runtime
                .init_config(&input, &McpFixturePolicy)
                .await
                .unwrap();
            assert_eq!(
                first.result,
                json!({"initialized":["mcp.servers.opdbus_compact"]})
            );
            assert_eq!(
                first.readback["mcp"]["servers"][0]["name"],
                "opdbus_compact"
            );
            let second = runtime
                .init_config(&input, &McpFixturePolicy)
                .await
                .unwrap();
            assert_eq!(second.result, json!({"initialized":[]}));
            assert_eq!(writes.load(Ordering::SeqCst), 2);
            task.abort();
        }

        #[tokio::test]
        async fn malformed_or_unpinned_mcp_server_init_is_rejected_before_effect() {
            let (runtime, writes, task) = mcp_fixture(McpFixtureMode::Honest).await;
            for section in [
                "mcp.servers",
                "mcp.servers.opdbus_compact.extra",
                "mcp.servers.other",
            ] {
                assert!(
                    runtime
                        .init_config(
                            &ConfigInitInput {
                                section: Some(section.into()),
                            },
                            &McpFixturePolicy,
                        )
                        .await
                        .is_err()
                );
            }
            assert_eq!(writes.load(Ordering::SeqCst), 0);
            task.abort();
        }

        #[tokio::test]
        async fn mcp_server_init_rejects_dishonest_receipt_or_missing_readback() {
            for mode in [
                McpFixtureMode::WrongReceipt,
                McpFixtureMode::MissingReadback,
            ] {
                let (runtime, writes, task) = mcp_fixture(mode).await;
                assert!(
                    runtime
                        .init_config(
                            &ConfigInitInput {
                                section: Some("mcp.servers.opdbus_compact".into()),
                            },
                            &McpFixturePolicy,
                        )
                        .await
                        .is_err()
                );
                assert_eq!(writes.load(Ordering::SeqCst), 1);
                task.abort();
            }
        }

        #[tokio::test]
        async fn actual_patch_transport_rejects_dishonest_results_without_retry() {
            for mismatch in ["path", "op", "readback"] {
                let writes = Arc::new(AtomicUsize::new(0));
                let state = FixtureState {
                    writes: writes.clone(),
                };
                let app = Router::new()
                    .route("/health", get(health))
                    .route("/api/config/prop", get(prop))
                    .route("/api/config", axum::routing::patch(move |State(state): State<FixtureState>| async move {
                        state.writes.fetch_add(1, Ordering::SeqCst);
                        Json(json!({"saved":true,"results":[{
                            "op": if mismatch == "op" { "add" } else { "replace" },
                            "path": if mismatch == "path" { "gateway.host" } else { "gateway.port" },
                            "value": if mismatch == "readback" { "PRIVATE_UNVERIFIED_VALUE" } else { "8085" }
                        }]}))
                    }))
                    .with_state(state);
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let runtime = TchedRouterRuntimeClient::new(
                    format!("http://{}", listener.local_addr().unwrap()),
                    "fixture".into(),
                    None,
                );
                let task = tokio::spawn(async move {
                    axum::serve(listener, app).await.unwrap();
                });
                let error = runtime
                    .patch_config(
                        &ConfigPatchInput {
                            operations: vec![ConfigPatchOperation::Replace {
                                path: "/gateway/port".into(),
                                value: json!(8085),
                            }],
                        },
                        &FixturePolicy,
                    )
                    .await
                    .unwrap_err();
                assert_eq!(
                    writes.load(Ordering::SeqCst),
                    1,
                    "no automatic retry of an uncertain effect"
                );
                assert!(!format!("{error:#}").contains("PRIVATE_UNVERIFIED_VALUE"));
                task.abort();
            }
        }
    }
}
