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
use op_cozo_store::{CozoError, CozoGraphShuttle, RegistryModel, RegistryModelRecord};
use op_plugins::state_plugins::model_registry::{
    DEFAULT_PROVIDERS_PATH, ListModelsInput, ListModelsOutput, ListProvidersInput,
    ListProvidersOutput, MAX_KEY_BYTES, MAX_MODELS_RESPONSE_BYTES, MAX_PROVIDERS_FILE_BYTES,
    ModelListFormat, ModelRecord, ObservedModel, ProviderAuth, ProviderConfig, ProviderRefresh,
    ProviderStatus, ProvidersFile, RefreshInput, RefreshOutput, SECRETS_DIR, bounded_error,
    page_limit, parse_models_response, parse_providers_toml, valid_provider_id,
    validate_list_models,
};
use op_state_store::PluginSchema;
use serde_json::Value;

use crate::{
    interceptor::load_capability_grants, method_contract_admission, mutation_engine::MutationEngine,
};

const PLUGIN: &str = "model_registry";
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
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
    fn from_env() -> Self {
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
    parse_providers_toml(&text)
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

pub(crate) async fn refresh(
    env: &Environment,
    lister: &dyn ModelLister,
    store: Arc<CozoGraphShuttle>,
    input: RefreshInput,
) -> Result<RefreshOutput> {
    let _guard = REFRESH_LOCK.lock().await;
    let file = load_providers(env)?;
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
            .collect(),
    };
    let at = now();
    let observed = futures::future::join_all(
        targets
            .iter()
            .map(|provider| async move { (*provider, observe(env, lister, provider).await) }),
    )
    .await;
    let mut providers = Vec::new();
    let mut revision = 0;
    for (provider, result) in observed {
        let listed = result.map_err(|error| bounded_error(&format!("{error:#}")));
        let failure = listed.as_ref().err().cloned();
        let (store, id, when) = (store.clone(), provider.id.clone(), at.clone());
        let (counts, next) = blocking(move || {
            store
                .record_model_registry_refresh(
                    &id,
                    &when,
                    listed.as_deref().map_err(String::as_str),
                )
                .map_err(cozo)
        })
        .await?;
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
            }
        })
        .collect();
    providers.extend(
        rows.iter()
            .filter(|row| !file.providers.iter().any(|p| p.id == row.provider_id))
            .map(|row| ProviderStatus {
                provider_id: row.provider_id.clone(),
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
            }),
    );
    Ok(ListProvidersOutput {
        providers,
        revision,
    })
}

fn model_record(row: RegistryModelRecord) -> ModelRecord {
    ModelRecord {
        provider_id: row.provider_id,
        model_id: row.model.model_id,
        name: row.model.name,
        context_length: row.model.context_length,
        prompt_price: row.model.prompt_price,
        completion_price: row.model.completion_price,
        available: row.available,
        first_seen: row.first_seen,
        last_seen: row.last_seen,
    }
}

pub(crate) async fn list_models(
    store: Arc<CozoGraphShuttle>,
    input: ListModelsInput,
) -> Result<ListModelsOutput> {
    validate_list_models(&input)?;
    let limit = page_limit(input.limit)?;
    let provider = input.provider_id.clone();
    let (rows, revision) = blocking(move || {
        store
            .list_model_registry_models(provider.as_deref())
            .map_err(cozo)
    })
    .await?;
    let query = input.query.as_deref().map(str::to_lowercase);
    let include_unavailable = input.include_unavailable.unwrap_or(false);
    let mut models: Vec<ModelRecord> = rows
        .into_iter()
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
        .map(model_record)
        .collect();
    let more = models.len() > limit;
    models.truncate(limit);
    Ok(ListModelsOutput {
        models,
        more,
        revision,
    })
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

fn args<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T> {
    serde_json::from_value(args.clone()).context("invalid model_registry arguments")
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
        "list_models" => serde_json::to_value(list_models(store, args(arguments)?).await?)?,
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
