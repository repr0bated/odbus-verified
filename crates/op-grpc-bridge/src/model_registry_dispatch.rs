//! model_registry methods: the provider list from the root-owned TOML, each
//! provider's current models fetched over HTTPS, and the system-wide Cozo
//! catalog they are folded into.
//!
//! The caller holds the method's exact grant, rechecked here; the
//! control-plane chatbot runs `refresh` hourly through its lungs, and any
//! granted identity reads. Keys come only from root-owned 0400/0600 files
//! under /etc/op-dbus/secrets/ that the provider entry names, are sent only
//! to that provider's own `models_url` (no redirects are followed), and are
//! never logged or stored. A provider that cannot be read keeps its last good
//! list; the admitted call is the chain record of each refresh.

use std::{
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use futures::StreamExt;
use op_cozo_store::{CozoError, CozoGraphShuttle, RegistryModel, RegistryModelRecord};
use op_plugins::state_plugins::model_registry::{
    DEFAULT_PROVIDERS_PATH, ListModelsInput, ListModelsOutput, ListProvidersInput,
    ListProvidersOutput, MAX_KEY_BYTES, MAX_MODELS_RESPONSE_BYTES, MAX_PROVIDERS_FILE_BYTES,
    MaintainCredentialsInput, MaintainCredentialsOutput, ModelListFormat, ModelRecord,
    ObservedModel, ProviderAuth, ProviderConfig, ProviderRefresh, ProviderStatus, ProvidersFile,
    RefreshInput, RefreshOutput, SECRETS_DIR, SetModelProfileInput, SetModelProfileOutput,
    bounded_error, page_limit, parse_models_response, parse_providers_toml, valid_model_id,
    valid_provider_id, validate_list_models, validate_set_model_profile,
};
use op_state_store::PluginSchema;
use serde_json::Value;

use crate::{
    interceptor::load_capability_grants, method_contract_admission, mutation_engine::MutationEngine,
};

const PLUGIN: &str = "model_registry";
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
// Bound provider response/parse working sets independently of the list size.
const MAX_CONCURRENT_MODEL_LISTS: usize = 4;
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// One refresh at a time; a second waits rather than interleaving.
static REFRESH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
#[path = "model_registry_dispatch_tests.rs"]
mod tests;

/// Where the provider list and key files live, and who must own them.
pub(crate) struct Environment {
    pub(crate) providers_path: PathBuf,
    /// The directory a `key_file` under [`SECRETS_DIR`] resolves into.
    pub(crate) secrets_dir: PathBuf,
    pub(crate) required_uid: u32,
}

impl Environment {
    pub(crate) fn from_env() -> Self {
        Self {
            providers_path: std::env::var_os("OP_MODEL_PROVIDERS_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_PROVIDERS_PATH)),
            secrets_dir: PathBuf::from(SECRETS_DIR),
            required_uid: 0,
        }
    }
}

/// Lists one provider's current models.
#[async_trait]
pub(crate) trait ModelLister: Send + Sync {
    async fn list(
        &self,
        provider: &ProviderConfig,
        key: Option<&str>,
    ) -> Result<Vec<ObservedModel>>;
}

/// Production lister: HTTPS `GET models_url`.
pub(crate) struct HttpLister;

#[async_trait]
impl ModelLister for HttpLister {
    async fn list(
        &self,
        provider: &ProviderConfig,
        key: Option<&str>,
    ) -> Result<Vec<ObservedModel>> {
        fetch_models(&provider.models_url, provider.format, provider.auth, key).await
    }
}

/// Describe a transport failure without echoing anything it carried.
fn transport(error: &reqwest::Error) -> anyhow::Error {
    if error.is_timeout() {
        anyhow::anyhow!("request timed out")
    } else if error.is_connect() {
        anyhow::anyhow!("connection failed")
    } else if error.is_redirect() {
        anyhow::anyhow!("provider redirected; redirects are not followed")
    } else {
        anyhow::anyhow!("request failed")
    }
}

/// GET a model list. The URL was validated with the provider entry.
pub(crate) async fn fetch_models(
    url: &str,
    format: ModelListFormat,
    auth: ProviderAuth,
    key: Option<&str>,
) -> Result<Vec<ObservedModel>> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent("op-dbus-model-registry/1.0")
        .build()
        .context("HTTP client is unavailable")?;
    let mut request = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json");
    request = match (auth, key) {
        (ProviderAuth::None, _) => request,
        (ProviderAuth::Bearer, Some(key)) => request.bearer_auth(key),
        (ProviderAuth::XApiKey, Some(key)) => request.header("x-api-key", key),
        (_, None) => bail!("provider key is required"),
    };
    if format == ModelListFormat::Anthropic {
        request = request
            .header("anthropic-version", ANTHROPIC_VERSION)
            .query(&[("limit", "1000")]);
    }
    let mut response = request.send().await.map_err(|error| transport(&error))?;
    let status = response.status();
    ensure!(status.is_success(), "HTTP {}", status.as_u16());
    if let Some(length) = response.content_length() {
        ensure!(
            length <= MAX_MODELS_RESPONSE_BYTES as u64,
            "model list exceeds {MAX_MODELS_RESPONSE_BYTES} bytes"
        );
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| transport(&error))? {
        ensure!(
            body.len() + chunk.len() <= MAX_MODELS_RESPONSE_BYTES,
            "model list exceeds {MAX_MODELS_RESPONSE_BYTES} bytes"
        );
        body.extend_from_slice(&chunk);
    }
    parse_models_response(format, &body)
}

/// Read a root-owned regular file, refusing symlinks, anything writable by
/// others (or, for a key, readable by others) and anything over `maximum`.
fn read_owned(path: &Path, required_uid: u32, maximum: usize, private: bool) -> Result<Vec<u8>> {
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .with_context(|| format!("{} is unavailable", path.display()))?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file(),
        "{} is not a regular file",
        path.display()
    );
    ensure!(
        metadata.uid() == required_uid,
        "{} must be owned by root",
        path.display()
    );
    let mode = metadata.mode() & 0o777;
    if private {
        ensure!(
            mode & 0o077 == 0,
            "{} must be mode 0400 or 0600",
            path.display()
        );
    } else {
        ensure!(
            mode & 0o022 == 0,
            "{} must not be group or world writable",
            path.display()
        );
    }
    ensure!(
        metadata.len() <= maximum as u64,
        "{} exceeds {maximum} bytes",
        path.display()
    );
    let mut bytes = Vec::new();
    file.by_ref()
        .take(maximum as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= maximum,
        "{} exceeds {maximum} bytes",
        path.display()
    );
    Ok(bytes)
}

pub(crate) fn load_providers(env: &Environment) -> Result<ProvidersFile> {
    let bytes = read_owned(
        &env.providers_path,
        env.required_uid,
        MAX_PROVIDERS_FILE_BYTES,
        false,
    )
    .context("provider list is unavailable")?;
    let text = String::from_utf8(bytes).context("provider list is not UTF-8")?;
    let file = parse_providers_toml(&text)?;
    crate::model_providers_validation::validate_providers_config(&file.providers)
        .context("provider configuration validation failed")?;
    Ok(file)
}

/// Read a provider key file validated at refresh time.
pub(crate) fn read_provider_key(env: &Environment, key_file: &str) -> Result<String> {
    read_key(env, key_file)
}

/// A provider's key: the file's single line, printable ASCII, no spaces.
/// `key_file` was validated to name one file directly under [`SECRETS_DIR`].
fn read_key(env: &Environment, key_file: &str) -> Result<String> {
    let name = key_file
        .strip_prefix(SECRETS_DIR)
        .filter(|name| !name.is_empty() && !name.contains('/'))
        .context("key_file is outside the secrets directory")?;
    let bytes = read_owned(
        &env.secrets_dir.join(name),
        env.required_uid,
        MAX_KEY_BYTES,
        true,
    )
    .context("key file is unavailable")?;
    let text = String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("key file is not text"))?;
    let key = text.trim_end_matches(['\r', '\n']);
    ensure!(
        !key.is_empty() && key.bytes().all(|b| (0x21..=0x7e).contains(&b)),
        "key file must hold one printable key"
    );
    Ok(key.to_string())
}

async fn observe(
    env: &Environment,
    lister: &dyn ModelLister,
    provider: &ProviderConfig,
) -> Result<Vec<RegistryModel>> {
    let key = match &provider.key_file {
        Some(path) => Some(read_key(env, path)?),
        None => None,
    };
    let models = lister.list(provider, key.as_deref()).await?;
    Ok(models
        .into_iter()
        .map(|model| RegistryModel {
            model_id: model.model_id,
            name: model.name,
            context_length: model.context_length,
            prompt_price: model.prompt_price,
            completion_price: model.completion_price,
        })
        .collect())
}

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(operation)
        .await
        .context("model registry worker failed")?
}

fn cozo(error: CozoError) -> anyhow::Error {
    anyhow::anyhow!("{error}")
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// Operator credential gate read from `OP_BASH_MODELS_PATH`
/// (default `/home/jeremy/.bash_models`). Line grammar:
/// `provider_id:kind=slot` with kind ∈ {key, oauth, off}; off entries may
/// omit the slot (`xai:off`). Slots are *references* (the bash_secrets
/// variable a key installs from, or the native auth-profile id), never
/// credential values. An absent file disables the gate → TOML `enabled`
/// governs (migration-safe). A present file is fail-closed: every provider
/// without a line is off, and a duplicate line, unknown kind, malformed
/// grammar or oversized slot fails the whole gate.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BashModelsGate {
    kinds: std::collections::BTreeMap<String, CredentialKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CredentialKind {
    Key,
    Oauth,
    Off,
}

impl BashModelsGate {
    fn off(&self, provider_id: &str) -> bool {
        // With the gate file in place an unlisted provider is off by design.
        self.kinds
            .get(provider_id)
            .is_none_or(|kind| *kind == CredentialKind::Off)
    }
}

fn load_bash_models_gate() -> Result<Option<BashModelsGate>> {
    let path = std::env::var("OP_BASH_MODELS_PATH")
        .unwrap_or_else(|_| "/home/jeremy/.bash_models".to_string());
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(anyhow::Error::from(error)
                .context(format!("bash_models gate file {path} is unreadable")));
        }
    };
    let mut kinds = std::collections::BTreeMap::new();
    for (index, raw) in raw.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let context = format!("bash_models line {}", index + 1);
        let (provider_id, rest) = line
            .split_once(':')
            .with_context(|| format!("{context}: expected provider:kind=slot grammar"))?;
        ensure!(
            valid_provider_id(provider_id),
            "{context}: invalid provider id"
        );
        let (kind_text, slot) = rest
            .split_once('=')
            .with_context(|| format!("{context}: expected kind=slot after the provider"))?;
        let kind = match kind_text.trim() {
            "key" => CredentialKind::Key,
            "oauth" => CredentialKind::Oauth,
            "off" => CredentialKind::Off,
            other => anyhow::bail!("{context}: unknown kind {other:?}"),
        };
        if kind != CredentialKind::Off {
            let slot = slot.trim();
            ensure!(
                !slot.is_empty()
                    && slot.len() <= 128
                    && !slot.chars().any(|c| c.is_whitespace() || c.is_control())
                    && !slot.contains('#'),
                "{context}: slot must be a compact non-empty reference"
            );
        }
        ensure!(
            kinds.insert(provider_id.to_string(), kind).is_none(),
            "{context}: duplicate provider {provider_id}"
        );
    }
    Ok(Some(BashModelsGate { kinds }))
}

pub(crate) async fn refresh(
    env: &Environment,
    lister: &dyn ModelLister,
    store: Arc<CozoGraphShuttle>,
    input: RefreshInput,
) -> Result<RefreshOutput> {
    let _guard = REFRESH_LOCK.lock().await;
    let file = load_providers(env)?;
    let gate = load_bash_models_gate()?;
    if let (Some(gate), Some(explicit)) = (gate.as_ref(), &input.provider_id) {
        ensure!(
            !gate.off(explicit),
            "provider {explicit} is off in the bash_models gate"
        );
    }
    let targets: Vec<&ProviderConfig> = match &input.provider_id {
        Some(id) => {
            ensure!(valid_provider_id(id), "invalid provider_id");
            let provider = file
                .providers
                .iter()
                .find(|provider| &provider.id == id)
                .with_context(|| format!("provider {id} is not in the provider list"))?;
            ensure!(provider.enabled, "provider {id} is disabled");
            vec![provider]
        }
        None => file
            .providers
            .iter()
            .filter(|provider| provider.enabled)
            .filter(|provider| gate.as_ref().is_none_or(|gate| !gate.off(&provider.id)))
            .collect(),
    };
    let at = now();
    // Persist each completed provider before admitting more responses. An
    // ordered bounded stream preserves configured reporting order without
    // retaining every provider's full model list until the slowest finishes.
    // Erase the request future type before buffering. This keeps the admitted
    // dispatch future Send across the async-trait lister's borrowed lifetime.
    type Request<'a> =
        futures::future::BoxFuture<'a, (&'a ProviderConfig, Result<Vec<RegistryModel>>)>;
    let requests: Vec<Request<'_>> = targets
        .into_iter()
        .map(|provider| {
            Box::pin(async move { (provider, observe(env, lister, provider).await) }) as Request<'_>
        })
        .collect();
    let mut observed = futures::stream::iter(requests).buffered(MAX_CONCURRENT_MODEL_LISTS);
    let mut providers = Vec::new();
    let mut revision = 0;
    while let Some((provider, result)) = observed.next().await {
        let listed = result.map_err(|error| bounded_error(&format!("{error:#}")));
        let mut failure = listed.as_ref().err().cloned();
        let (store, id, when) = (store.clone(), provider.id.clone(), at.clone());
        let record_result = blocking(move || {
            store
                .record_model_registry_refresh(
                    &id,
                    &when,
                    listed.as_deref().map_err(String::as_str),
                )
                .map_err(cozo)
        })
        .await;
        let (counts, next) = match record_result {
            Ok((counts, revision)) => (counts, revision),
            Err(error) => {
                tracing::error!(%error, provider_id = %provider.id, "model registry refresh store error");
                failure.get_or_insert_with(|| format!("store error: {error}"));
                (op_cozo_store::RegistryRefreshCounts::default(), revision)
            }
        };
        revision = next;
        providers.push(ProviderRefresh {
            provider_id: provider.id.clone(),
            status: if failure.is_some() { "error" } else { "ok" }.to_string(),
            error: failure,
            available_models: counts.available,
            added: counts.added,
            dropped: counts.dropped,
        });
    }
    if input.provider_id.is_none() {
        // Providers that left the list are retired; disabled ones keep their
        // last list until they are removed or enabled again.
        let configured: Vec<String> = file
            .providers
            .iter()
            .map(|provider| provider.id.clone())
            .collect();
        let (store, when) = (store.clone(), at.clone());
        let (retired, next) = blocking(move || {
            store
                .retire_model_registry_providers(&configured, &when)
                .map_err(cozo)
        })
        .await?;
        revision = revision.max(next);
        providers.extend(
            retired
                .into_iter()
                .map(|(provider_id, counts)| ProviderRefresh {
                    provider_id,
                    status: "removed".into(),
                    error: None,
                    available_models: 0,
                    added: 0,
                    dropped: counts.dropped,
                }),
        );
    }
    Ok(RefreshOutput {
        refreshed_at: at,
        revision,
        providers,
    })
}

pub(crate) async fn list_providers(
    env: &Environment,
    store: Arc<CozoGraphShuttle>,
) -> Result<ListProvidersOutput> {
    let file = load_providers(env)?;
    let (rows, revision) =
        blocking(move || store.list_model_registry_providers().map_err(cozo)).await?;
    let mut providers: Vec<ProviderStatus> = file
        .providers
        .iter()
        .map(|provider| {
            let row = rows.iter().find(|row| row.provider_id == provider.id);
            ProviderStatus {
                provider_id: provider.id.clone(),
                native_provider_id: provider.native_provider_id.clone(),
                inference_url_prefix: provider.inference_url_prefix.clone(),
                name: Some(provider.name.clone()),
                configured: true,
                enabled: provider.enabled,
                models_url: Some(provider.models_url.clone()),
                status: row.map_or_else(|| "never".to_string(), |row| row.status.clone()),
                error: row.and_then(|row| row.error.clone()),
                refreshed_at: row.map(|row| row.refreshed_at.clone()),
                succeeded_at: row.and_then(|row| row.succeeded_at.clone()),
                available_models: row.map_or(0, |row| row.available_count),
                known_models: row.map_or(0, |row| row.known_count),
                control_plane_profiles: provider
                    .control_plane
                    .as_ref()
                    .map(|routing| routing.profiles().into_iter().map(str::to_string).collect())
                    .unwrap_or_default(),
            }
        })
        .collect();
    providers.extend(
        rows.iter()
            .filter(|row| !file.providers.iter().any(|p| p.id == row.provider_id))
            .map(|row| ProviderStatus {
                provider_id: row.provider_id.clone(),
                native_provider_id: None,
                inference_url_prefix: None,
                name: None,
                configured: false,
                enabled: false,
                models_url: None,
                status: row.status.clone(),
                error: row.error.clone(),
                refreshed_at: Some(row.refreshed_at.clone()),
                succeeded_at: row.succeeded_at.clone(),
                available_models: row.available_count,
                known_models: row.known_count,
                control_plane_profiles: Vec::new(),
            }),
    );
    Ok(ListProvidersOutput {
        providers,
        revision,
    })
}

fn model_record(
    row: RegistryModelRecord,
    providers: &ProvidersFile,
    recorded: Option<&str>,
) -> ModelRecord {
    let provider = providers
        .providers
        .iter()
        .find(|provider| provider.id == row.provider_id);
    ModelRecord {
        provider_id: row.provider_id,
        model_id: row.model.model_id,
        native_provider_id: provider
            .and_then(|provider| provider.native_provider_id_for_model(recorded)),
        inference_url_prefix: provider.and_then(|provider| provider.inference_url_prefix.clone()),
        name: row.model.name,
        context_length: row.model.context_length,
        prompt_price: row.model.prompt_price,
        completion_price: row.model.completion_price,
        available: row.available,
        first_seen: row.first_seen,
        last_seen: row.last_seen,
        recorded_profile: recorded.map(str::to_string),
        control_plane_profile: provider
            .and_then(|provider| provider.control_plane_profile(recorded).map(str::to_string)),
    }
}

pub(crate) async fn list_models(
    env: &Environment,
    store: Arc<CozoGraphShuttle>,
    input: ListModelsInput,
) -> Result<ListModelsOutput> {
    validate_list_models(&input)?;
    let file = load_providers(env)?;
    let limit = page_limit(input.limit)?;
    let provider = input.provider_id.clone();
    let (rows, profiles, revision) = blocking(move || {
        let (rows, revision) = store
            .list_model_registry_models(provider.as_deref())
            .map_err(cozo)?;
        let profiles = store.list_model_registry_profiles().map_err(cozo)?;
        Ok((rows, profiles, revision))
    })
    .await?;
    let recorded: std::collections::BTreeMap<(String, String), String> = profiles
        .into_iter()
        .map(|(provider_id, model_id, profile)| ((provider_id, model_id), profile))
        .collect();
    let query = input.query.as_deref().map(str::to_lowercase);
    let include_unavailable = input.include_unavailable.unwrap_or(false);
    let configured_providers: std::collections::HashSet<&str> = file
        .providers
        .iter()
        .filter(|p| p.enabled)
        .map(|p| p.id.as_str())
        .collect();
    // Credential gate second: with a bash_models file in place, rows of a
    // gate-off provider disappear from the menu entirely (fail-closed hide,
    // no error noise). With no gate file the TOML `enabled` flag governs.
    let gate = load_bash_models_gate()?;
    let rows = if let Some(gate) = gate.as_ref() {
        rows.into_iter()
            .filter(|row| !gate.off(&row.provider_id))
            .collect::<Vec<_>>()
    } else {
        rows
    };
    let mut models: Vec<ModelRecord> = rows
        .into_iter()
        .filter(|row| configured_providers.contains(row.provider_id.as_str()))
        .filter(|row| include_unavailable || row.available)
        .filter(|row| match &input.after {
            Some(after) => {
                (row.provider_id.as_str(), row.model.model_id.as_str())
                    > (after.provider_id.as_str(), after.model_id.as_str())
            }
            None => true,
        })
        .filter(|row| match &query {
            Some(query) => {
                row.model.model_id.to_lowercase().contains(query)
                    || row
                        .model
                        .name
                        .as_deref()
                        .is_some_and(|name| name.to_lowercase().contains(query))
            }
            None => true,
        })
        .take(limit + 1)
        .map(|row| {
            let key = (row.provider_id.clone(), row.model.model_id.clone());
            let profile = recorded.get(&key).map(String::as_str);
            model_record(row, &file, profile)
        })
        .collect();
    let more = models.len() > limit;
    models.truncate(limit);
    Ok(ListModelsOutput {
        models,
        more,
        revision,
    })
}

pub(crate) async fn set_model_profile(
    env: &Environment,
    store: Arc<CozoGraphShuttle>,
    input: SetModelProfileInput,
) -> Result<SetModelProfileOutput> {
    validate_set_model_profile(&input)?;
    let file = load_providers(env)?;
    let provider = file
        .providers
        .iter()
        .find(|provider| provider.id == input.provider_id)
        .context("provider is not in the provider list")?;
    if let Some(profile) = &input.profile {
        let offered = provider
            .control_plane
            .as_ref()
            .map(|routing| routing.profiles())
            .unwrap_or_default();
        ensure!(
            offered.iter().any(|candidate| *candidate == profile),
            "profile is not one this provider offers the Control Plane"
        );
    }
    let provider_id = input.provider_id.clone();
    let model_id = input.model_id.clone();
    let profile = input.profile.clone();
    let reading = Arc::clone(&store);
    let revision = blocking(move || {
        store
            .set_model_registry_profile(&provider_id, &model_id, profile.as_deref())
            .map_err(cozo)
    })
    .await?;
    let provider_id = input.provider_id.clone();
    let model_id = input.model_id.clone();
    let row = blocking(move || {
        let (rows, _) = reading
            .list_model_registry_models(Some(&provider_id))
            .map_err(cozo)?;
        rows.into_iter()
            .find(|row| row.model.model_id == model_id)
            .context("model is not in the catalog")
    })
    .await?;
    let model = model_record(row, &file, input.profile.as_deref());
    Ok(SetModelProfileOutput { model, revision })
}

fn required_capability<'a>(schema: &'a PluginSchema, method: &str) -> Result<&'a str> {
    schema
        .methods
        .get(method)
        .with_context(|| format!("admitted {PLUGIN}.{method} declaration is absent"))?
        .required_capability
        .as_deref()
        .context("admitted model_registry method has no exact capability")
}

/// Catalog authority for the separately admitted native binding reflection.
/// This helper reads no native configuration and grants no native authority.
pub(crate) async fn catalog_binding_target(
    engine: &MutationEngine,
    provider_id: &str,
    model_id: &str,
) -> Result<crate::tched_catalog_binding::BindingTarget> {
    binding_target_from(
        &Environment::from_env(),
        engine.model_registry_cozo().await?,
        provider_id,
        model_id,
    )
    .await
}

async fn binding_target_from(
    env: &Environment,
    store: Arc<CozoGraphShuttle>,
    provider_id: &str,
    model_id: &str,
) -> Result<crate::tched_catalog_binding::BindingTarget> {
    ensure!(valid_provider_id(provider_id), "invalid catalog provider");
    ensure!(valid_model_id(model_id), "invalid catalog model");
    let file = load_providers(env)?;
    let provider = file
        .providers
        .iter()
        .find(|provider| provider.id == provider_id)
        .context("catalog provider is not configured")?;
    ensure!(provider.enabled, "catalog provider is disabled");
    let prefix = provider
        .inference_url_prefix
        .clone()
        .context("catalog provider has no inference endpoint authority")?;
    let id = provider_id.to_string();
    let model = model_id.to_string();
    let (revision, recorded) = blocking(move || {
        let (rows, revision) = store.list_model_registry_models(Some(&id)).map_err(cozo)?;
        let row = rows
            .into_iter()
            .find(|row| row.model.model_id == model)
            .context("model is not in the catalog")?;
        ensure!(row.available, "catalog model is unavailable");
        let recorded = store
            .list_model_registry_profiles()
            .map_err(cozo)?
            .into_iter()
            .find(|(provider, candidate, _)| provider == &id && candidate == &model)
            .map(|(_, _, profile)| profile);
        let (_, after) = store.list_model_registry_providers().map_err(cozo)?;
        ensure!(revision == after, "catalog changed during binding lookup");
        Ok((revision, recorded))
    })
    .await?;
    // A recorded alternative specifies a wire contract. Discovery must not
    // substitute the default endpoint for an unmapped or retired alternative.
    let ordinary = provider
        .native_provider_id_for_model(recorded.as_deref())
        .or_else(|| {
            recorded
                .is_none()
                .then(|| op_plugins::state_plugins::model_registry::ProviderConfig::derived_native_provider_id(provider_id))
                .flatten()
        });
    if let Some(recorded) = recorded.as_deref() {
        let default = provider
            .control_plane
            .as_ref()
            .map(|routing| routing.profile.as_str());
        ensure!(
            Some(recorded) == default || ordinary.is_some(),
            "recorded model profile has no ordinary routing counterpart"
        );
    }
    let excluded_references = file
        .providers
        .iter()
        .flat_map(|provider| provider.control_plane.as_ref())
        .flat_map(|routing| routing.profiles())
        .map(str::to_string)
        .collect();
    ensure!(
        load_providers(env)? == file,
        "provider authority changed during binding lookup"
    );
    Ok(crate::tched_catalog_binding::BindingTarget {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
        inference_url_prefix: prefix,
        catalog_revision: revision,
        ordinary_reference: ordinary,
        excluded_references,
    })
}

fn args<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T> {
    serde_json::from_value(args.clone()).context("invalid model_registry arguments")
}

/// Apply add/subtract/off operations to the `~/.bash_models` flag file. The
/// file is a value-free credential-gate list (`provider:kind=slot`); slots
/// are references (bash_secrets variable names or native auth-profile ids),
/// never credentials. Writes are atomic (temp file + rename), preserve mode
/// 0600, and each applied operation is re-read back before admission.
async fn maintain_credentials(
    env: &Environment,
    input: MaintainCredentialsInput,
) -> Result<MaintainCredentialsOutput> {
    let _ = env;
    anyhow::ensure!(
        !input.operations.is_empty() && input.operations.len() <= 32,
        "maintain_credentials accepts 1..=32 operations"
    );
    let path = std::env::var("OP_BASH_MODELS_PATH")
        .unwrap_or_else(|_| "/home/jeremy/.bash_models".to_string());
    let existing = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(anyhow::Error::from(error)
                .context(format!("bash_models gate file {path} is unreadable")));
        }
    };
    // Parse the existing lines with the same strict grammar the gate uses.
    let mut lines: Vec<(String, String)> = existing
        .lines()
        .filter(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with('#')
        })
        .map(|line| {
            let (provider_id, rest) = line.split_once(':').with_context(|| {
                format!("bash_models entry {line:?}: expected provider:kind=slot")
            })?;
            Ok((provider_id.to_string(), rest.to_string()))
        })
        .collect::<Result<Vec<(String, String)>>>()?;
    for operation in &input.operations {
        let kind = operation.kind.as_str();
        match kind {
            "key" | "oauth" => {
                let slot = operation.slot.as_deref().map(str::trim).with_context(|| {
                    format!(
                        "credential op {}: kind {kind} needs a slot",
                        operation.provider_id
                    )
                })?;
                anyhow::ensure!(
                    !slot.is_empty()
                        && slot.len() <= 128
                        && !slot.chars().any(|c| c.is_whitespace() || c.is_control())
                        && !slot.contains('#'),
                    "credential op {}: slot must be a compact non-empty reference",
                    operation.provider_id
                );
                lines.retain(|(provider_id, _)| provider_id != &operation.provider_id);
                lines.push((operation.provider_id.clone(), format!("{kind}={slot}")));
            }
            "off" => {
                anyhow::ensure!(
                    operation.slot.is_none(),
                    "credential op {}: off entries carry no slot",
                    operation.provider_id
                );
                lines.retain(|(provider_id, _)| provider_id != &operation.provider_id);
                lines.push((operation.provider_id.clone(), "off".to_string()));
            }
            "remove" => {
                anyhow::ensure!(
                    operation.slot.is_none(),
                    "credential op {}: remove entries carry no slot",
                    operation.provider_id
                );
                lines.retain(|(provider_id, _)| provider_id != &operation.provider_id);
            }
            other => anyhow::bail!(
                "credential op {}: unknown kind {other:?}",
                operation.provider_id
            ),
        }
        anyhow::ensure!(
            valid_provider_id(&operation.provider_id),
            "credential op: invalid provider id {}",
            operation.provider_id
        );
    }
    let mut body = String::from(
        "# Credential gate: provider:kind=slot (slots are references, never values)\n",
    );
    for (provider_id, rest) in &lines {
        body.push_str(&format!("{provider_id}:{rest}\n"));
    }
    // Atomic replace preserving 0600; the gate parser re-validates on the
    // next read, so a malformed write cannot take effect silently.
    let parent = std::path::Path::new(&path)
        .parent()
        .with_context(|| format!("bash_models gate path {path} has no parent"))?;
    let temp = parent.join(format!(
        ".bash_models.tmp-{}",
        uuid::Uuid::new_v4().simple()
    ));
    tokio::fs::write(&temp, body.as_bytes())
        .await
        .with_context(|| format!("bash_models gate temp write {temp:?} failed"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("bash_models gate temp chmod {temp:?} failed"))?;
    }
    tokio::fs::rename(&temp, &path)
        .await
        .with_context(|| format!("bash_models gate atomic replace {path} failed"))?;
    // Authoritative readback: the file must parse exactly as intended.
    let verify = load_bash_models_gate()?.with_context(|| "bash_models gate readback missing")?;
    for operation in &input.operations {
        let off = matches!(operation.kind.as_str(), "off" | "remove");
        anyhow::ensure!(
            off == verify.off(&operation.provider_id),
            "credential op {} readback diverged (expected off={off})",
            operation.provider_id
        );
    }
    Ok(MaintainCredentialsOutput {
        applied: input.operations,
    })
}

pub(crate) async fn dispatch(
    engine: &MutationEngine,
    method: &str,
    arguments: &Value,
    actor: &str,
    session: Option<&str>,
    admitted: &PluginSchema,
) -> Result<Value> {
    ensure!(
        session.is_some_and(|session| !session.is_empty()),
        "a verified session is required"
    );
    let required = required_capability(admitted, method)?;
    ensure!(
        load_capability_grants(actor).contains(required),
        "current exact {required} grant is required"
    );
    let env = Environment::from_env();
    let store = engine.model_registry_cozo().await?;
    let output = match method {
        "list_providers" => {
            let _: ListProvidersInput = args(arguments)?;
            serde_json::to_value(list_providers(&env, store).await?)?
        }
        "list_models" => serde_json::to_value(list_models(&env, store, args(arguments)?).await?)?,
        "native_list_providers" => {
            let _: op_plugins::state_plugins::model_registry::NativeListProvidersInput =
                args(arguments)?;
            let runtime = crate::tched_router_runtime::TchedRouterRuntimeClient::from_env();
            let pinned = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
                &crate::sealed_schema_reader::blob_catalog_dir_from_env(),
                "tched_router",
            )?;
            let secrets = crate::tched_config_engine::pinned_config_secrets(&pinned)?;
            let providers = runtime.native_providers_inventory(&secrets).await?;
            serde_json::to_value(
                op_plugins::state_plugins::model_registry::NativeListProvidersOutput { providers },
            )?
        }
        "maintain_credentials" => {
            serde_json::to_value(maintain_credentials(&env, args(arguments)?).await?)?
        }
        "set_model_profile" => {
            serde_json::to_value(set_model_profile(&env, store, args(arguments)?).await?)?
        }
        "refresh" => {
            serde_json::to_value(refresh(&env, &HttpLister, store, args(arguments)?).await?)?
        }
        other => bail!("{PLUGIN} has no method {other}"),
    };
    let declaration = admitted
        .methods
        .get(method)
        .with_context(|| format!("admitted {PLUGIN}.{method} declaration is absent"))?;
    method_contract_admission::validate_admitted_output(declaration, &output)
        .context("model_registry output does not conform to the admitted sealed contract")?;
    Ok(output)
}
