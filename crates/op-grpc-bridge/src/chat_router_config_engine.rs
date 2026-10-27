//! Prior name: tched_config_engine.
//! Exact sealed-contract adapter for the native chat_router configuration surface.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::Context;
use serde_json::{Value, json};

use crate::{
    mutation_engine::MutationEngine,
    sealed_schema_reader::{
        ManifestPinnedSchemaDocument, blob_catalog_dir_from_env,
        read_manifest_pinned_schema_document,
    },
    chat_router_config_dispatch::{self, ConfigReadDispatch},
    chat_router_config_secret_policy::PinnedConfigSecretPolicy,
    chat_router_runtime::ChatRouterRuntimeClient,
};

const PLUGIN: &str = "chat_router";

struct PinnedConfigContract {
    schema_hash: String,
    schema_json: String,
    config_schema: Value,
    secrets: PinnedConfigSecretPolicy,
}

fn load_contract(pinned: &ManifestPinnedSchemaDocument) -> anyhow::Result<PinnedConfigContract> {
    crate::method_contract_admission::admit_compiled_state_contract(pinned, PLUGIN)?;
    // The exact field retains the original self-contained native document.
    // Method-output schemas embed a pointer-rebased derivative beneath
    // /properties/config and therefore are not an extraction authority.
    let exact = pinned
        .schema
        .fields
        .get("native_config")
        .context("sealed chat_router has no native_config state field")?;
    let op_state_store::FieldType::JsonSchema(config_schema) = &exact.field_type else {
        anyhow::bail!("sealed chat_router native_config field is not an exact schema");
    };
    let config_schema = serde_json::to_value(config_schema.as_ref())?;
    let secrets = PinnedConfigSecretPolicy::new(Arc::new(config_schema.clone()))?;
    Ok(PinnedConfigContract {
        schema_hash: pinned.schema_hash.clone(),
        schema_json: pinned.blob.schema_json.clone(),
        config_schema,
        secrets,
    })
}

async fn ensure_current_pin(expected: &str, expected_json: &str) -> anyhow::Result<()> {
    let expected = expected.to_string();
    let expected_json = expected_json.to_string();
    tokio::task::spawn_blocking(move || {
        let current = read_manifest_pinned_schema_document(&blob_catalog_dir_from_env(), PLUGIN)?;
        anyhow::ensure!(
            current.schema_hash == expected && current.blob.schema_json == expected_json,
            "chat_router seal changed during native config dispatch"
        );
        Ok(())
    })
    .await?
}

fn validate_output(
    admitted: &op_state_store::PluginSchema,
    method: &str,
    output: &Value,
) -> anyhow::Result<()> {
    let declaration = admitted
        .methods
        .get(method)
        .with_context(|| format!("admitted chat_router method '{method}' is absent"))?;
    let returns = declaration
        .returns
        .as_ref()
        .with_context(|| format!("admitted chat_router method '{method}' has no output schema"))?;
    let schema = serde_json::to_value(returns)?;
    let validator =
        jsonschema::Validator::new(&schema).context("invalid admitted method output contract")?;
    validator.validate(output).map_err(|_| {
        anyhow::anyhow!(
            "native output does not conform to admitted chat_router.{method} during output validation"
        )
    })
}

fn validate_native_config(contract: &PinnedConfigContract, config: &Value) -> anyhow::Result<()> {
    let validator = jsonschema::Validator::new(&contract.config_schema)
        .context("invalid sealed native configuration contract")?;
    validator.validate(config).map_err(|_| {
        anyhow::anyhow!("masked native configuration failed sealed-contract validation")
    })
}

fn select_native_config(config: &Value, section: Option<&str>) -> anyhow::Result<Value> {
    let selected = match section {
        None => config.clone(),
        Some(section) => config
            .get(section)
            .cloned()
            .with_context(|| format!("native config section '{section}' is absent"))?,
    };
    Ok(json!({ "config": selected }))
}

/// Observe a separately admitted, bounded public binding through the same
/// pinned native contract and secret policy used by the config facade.
pub(crate) async fn observe_catalog_binding(
    pinned: &ManifestPinnedSchemaDocument,
    sealed_identity: &str,
    target: &crate::chat_router_catalog_binding::BindingTarget,
) -> anyhow::Result<crate::chat_router_runtime::CatalogBindingObservation> {
    let contract = load_contract(pinned)?;
    let runtime = ChatRouterRuntimeClient::for_agent_alias("dashboard")
        .with_sealed_identity(Some(sealed_identity));
    let observation = runtime
        .observe_catalog_binding(target, &contract.secrets)
        .await?;
    validate_native_config(&contract, &observation.native_config)?;
    ensure_current_pin(&contract.schema_hash, &contract.schema_json).await?;
    Ok(observation)
}

pub(crate) fn pinned_config_secrets(
    pinned: &ManifestPinnedSchemaDocument,
) -> anyhow::Result<PinnedConfigSecretPolicy> {
    Ok(load_contract(pinned)?.secrets)
}

/// `$MODEL.<n>` slot tokens: the only model values an operator's flag files
/// and dashboards may express besides a concrete catalog id. A token carries
/// no provider path; resolution is scoped by the mutated path itself
/// (`providers[./.]models[./.]<family>[./.]…model`), so a model listed under
/// one provider path can never resolve for another. Families that are not
/// catalog provider ids (the composite `custom` family) fail closed until an
/// explicit mapping lands.
fn model_token_slot(value: &str) -> Option<u64> {
    value
        .strip_prefix("$MODEL.")
        .and_then(|slot| slot.parse::<u64>().ok())
        .filter(|slot| (1..=99).contains(slot))
}

fn path_family(path: &str) -> Option<String> {
    let segments: Vec<&str> = path
        .split(['.', '/'])
        .filter(|segment| !segment.is_empty())
        .collect();
    for position in 0..segments.len() {
        if segments[position] != "providers" {
            continue;
        }
        let rest = &segments[position + 1..];
        if rest.first().is_some_and(|segment| *segment == "models") {
            return rest.get(1).map(ToString::to_string);
        }
    }
    None
}

fn collect_model_tokens(
    value: &Value,
    requested: &mut Vec<(String, u64)>,
    invalid: &mut Vec<(String, u64)>,
) {
    match value {
        Value::Object(object) => {
            if let Some(path) = object.get("path").and_then(Value::as_str) {
                let mut candidates: Vec<&str> = Vec::new();
                if let Some(Value::String(candidate)) = object.get("value") {
                    candidates.push(candidate.as_str());
                }
                if let Some(values) = object.get("values").and_then(Value::as_array) {
                    candidates.extend(values.iter().filter_map(Value::as_str));
                }
                let family = path_family(path);
                for candidate in candidates {
                    if let Some(slot) = model_token_slot(candidate) {
                        match &family {
                            Some(family) => requested.push((family.clone(), slot)),
                            None => invalid.push((path.to_string(), slot)),
                        }
                    }
                }
            }
            for child in object.values() {
                collect_model_tokens(child, requested, invalid);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_model_tokens(item, requested, invalid);
            }
        }
        _ => {}
    }
}

async fn resolve_model_tokens(engine: &MutationEngine, args: &Value) -> anyhow::Result<Value> {
    let mut requested: Vec<(String, u64)> = Vec::new();
    let mut invalid: Vec<(String, u64)> = Vec::new();
    collect_model_tokens(args, &mut requested, &mut invalid);
    anyhow::ensure!(
        invalid.is_empty(),
        "model slot tokens are reserved for provider model paths: {invalid:?}"
    );
    if requested.is_empty() {
        return Ok(args.clone());
    }
    requested.sort();
    requested.dedup();
    let store = engine.model_registry_cozo().await?;
    let mut resolved: BTreeMap<(String, u64), String> = BTreeMap::new();
    let families: BTreeSet<String> = requested.iter().map(|(family, _)| family.clone()).collect();
    for family in families {
        let store = store.clone();
        let token_family = family.clone();
        let (rows, _) = chat_blocking(move || {
            store
                .list_model_registry_models(Some(&token_family))
                .map_err(|error| anyhow::anyhow!("model registry catalog read failed: {error}"))
        })
        .await?;
        anyhow::ensure!(
            !rows.is_empty(),
            "$MODEL token cannot resolve: provider {family} has no catalog rows"
        );
        // Deterministic order: available first, then model id ascending.
        let mut order: Vec<&_> = rows.iter().collect();
        order.sort_by(|left, right| {
            right
                .available
                .cmp(&left.available)
                .then_with(|| left.model.model_id.cmp(&right.model.model_id))
        });
        let slots_for_family: Vec<u64> = requested
            .iter()
            .filter(|(token_family, _)| *token_family == family)
            .map(|(_, slot)| *slot)
            .collect();
        for (position, row) in order.iter().enumerate() {
            let slot = (position + 1) as u64;
            if slot <= 99 && slots_for_family.contains(&slot) {
                resolved.insert((family.clone(), slot), row.model.model_id.clone());
            }
        }
    }
    for (family, slot) in &requested {
        anyhow::ensure!(
            resolved.contains_key(&(family.clone(), *slot)),
            "$MODEL.{slot} has no row for provider {family}"
        );
    }
    rewrite_model_tokens(args.clone(), &resolved)
}

async fn chat_blocking<T: Send + 'static>(
    work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .context("chat config engine blocking worker failed")?
}

fn rewrite_model_tokens(
    value: Value,
    resolved: &BTreeMap<(String, u64), String>,
) -> anyhow::Result<Value> {
    match value {
        Value::Object(mut object) => {
            if let Some((family, slot)) = object
                .get("path")
                .and_then(Value::as_str)
                .and_then(path_family)
                .zip(
                    object
                        .get("value")
                        .and_then(Value::as_str)
                        .and_then(model_token_slot),
                )
            {
                let concrete = resolved
                    .get(&(family.clone(), slot))
                    .with_context(|| format!("$MODEL.{slot} unresolved for provider {family}"))?;
                object.insert("value".into(), Value::String(concrete.clone()));
                return Ok(Value::Object(object));
            }
            let rewritten: anyhow::Result<Vec<(String, Value)>> = object
                .into_iter()
                .map(|(key, child)| Ok((key, rewrite_model_tokens(child, resolved)?)))
                .collect();
            Ok(Value::Object(rewritten?.into_iter().collect()))
        }
        Value::Array(items) => {
            let rewritten: anyhow::Result<Vec<Value>> = items
                .into_iter()
                .map(|item| rewrite_model_tokens(item, resolved))
                .collect();
            Ok(Value::Array(rewritten?))
        }
        leaf => Ok(leaf),
    }
}

/// Returns None only when the method is outside the v3 config facade.
pub(crate) async fn dispatch(
    engine: &MutationEngine,
    pinned: &ManifestPinnedSchemaDocument,
    method: &str,
    args: &Value,
    sealed_identity: &str,
) -> anyhow::Result<Option<Value>> {
    let admitted = &pinned.schema;
    let contract = load_contract(pinned)?;

    // `GetConfigSchema` retired with the generated layer: the config's
    // schema authority is the sealed `native_config` field itself, served
    // by the mirror — not a receipt.

    let runtime = ChatRouterRuntimeClient::from_env().with_sealed_identity(Some(sealed_identity));
    match chat_router_config_dispatch::dispatch_config_read(&runtime, method, args, &contract.secrets)
        .await?
    {
        ConfigReadDispatch::Handled(output) => {
            ensure_current_pin(&contract.schema_hash, &contract.schema_json).await?;
            validate_output(admitted, method, &output)?;
            return Ok(Some(output));
        }
        ConfigReadDispatch::NativeConfig { section } => {
            let observation = runtime.observe_config_and_state(&contract.secrets).await?;
            validate_native_config(&contract, &observation.native_config)?;
            let native_config = observation.native_config.clone();
            let (_, represented) = observation.into_state_and_fields()?;
            engine
                .persist_chat_router_config_observation(
                    method,
                    &represented,
                    &contract.schema_hash,
                )
                .await?;
            let committed = engine
                .read_chat_router_config_field("native_config", &contract.schema_hash)
                .await?;
            anyhow::ensure!(
                committed == native_config,
                "chat_router native config changed after observation publication"
            );
            ensure_current_pin(&contract.schema_hash, &contract.schema_json).await?;
            let output = select_native_config(&committed, section)?;
            validate_output(admitted, method, &output)?;
            return Ok(Some(output));
        }
        ConfigReadDispatch::NotAConfigRead => {}
    }

    let maybe_mutation = async {
        let args = resolve_model_tokens(engine, args)
            .await
            .context("model slot token resolution failed before the native config mutation")?;
        chat_router_config_dispatch::dispatch_config_mutation(&runtime, method, &args, &contract.secrets)
            .await
    }
    .await;
    let mutation = match maybe_mutation {
        Err(error) => return Err(error),
        Ok(None) => return Ok(None),
        Ok(Some(mutation)) => mutation,
    };
    anyhow::ensure!(
        !mutation.readback.is_null(),
        "native config mutation completed without authoritative target readback"
    );

    // Method-specific receipts are bound to their target readbacks. A native
    // remove receipt denotes reset, not equality to a null resulting value.
    // This stable full observation supplies one paired config + selection
    // image for one protected SHM generation; no global native CAS is implied.
    let observation = runtime
        .observe_config_and_state(&contract.secrets)
        .await
        .context(
            "native config mutation completed but coherent observation failed; outcome requires reconciliation",
        )?;
    validate_native_config(&contract, &observation.native_config).context(
        "native config mutation completed but its full observation failed contract validation",
    )?;
    let (_, represented) = observation.into_state_and_fields()?;
    engine
        .persist_chat_router_config_observation(
            method,
            &represented,
            &contract.schema_hash,
        )
        .await
        .context(
            "native config mutation completed but protected state publication failed; outcome requires reconciliation",
        )?;
    ensure_current_pin(&contract.schema_hash, &contract.schema_json)
        .await
        .context("native config mutation completed but the admitted seal changed")?;
    validate_output(admitted, method, &mutation.result)
        .context("native config mutation completed but its receipt failed output validation")?;
    Ok(Some(mutation.result))
}

/// Complete paired native configuration and selection observation. The caller
/// holds the final-admitted router guard; this helper never reads a new seal.
pub(crate) async fn coherent_observation(
    pinned: &ManifestPinnedSchemaDocument,
    sealed_identity: &str,
) -> anyhow::Result<(
    op_plugins::state_plugins::chat_router::ChatRouterState,
    Value,
)> {
    let contract = load_contract(pinned)?;
    let observation = ChatRouterRuntimeClient::from_env()
        .with_sealed_identity(Some(sealed_identity))
        .observe_config_and_state(&contract.secrets)
        .await?;
    validate_native_config(&contract, &observation.native_config)?;
    let paired = observation.into_state_and_fields()?;
    ensure_current_pin(&contract.schema_hash, &contract.schema_json).await?;
    Ok(paired)
}

/// Ordinary catalog selection always observes dashboard, independently of the
/// bridge's configurable default agent. Dedicated agents retain their facade.
pub(crate) async fn coherent_catalog_selection_observation(
    pinned: &ManifestPinnedSchemaDocument,
    sealed_identity: &str,
) -> anyhow::Result<Value> {
    let contract = load_contract(pinned)?;
    let observation = ChatRouterRuntimeClient::for_agent_alias("dashboard")
        .with_sealed_identity(Some(sealed_identity))
        .observe_config_and_state(&contract.secrets)
        .await?;
    validate_native_config(&contract, &observation.native_config)?;
    let (_, fields) = observation.into_state_and_fields()?;
    ensure_current_pin(&contract.schema_hash, &contract.schema_json).await?;
    Ok(fields)
}
