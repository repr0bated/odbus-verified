//! Exact sealed-contract adapter for the native tched_router configuration surface.

use std::sync::Arc;

use anyhow::Context;
use serde_json::{json, Value};

use crate::{
    mutation_engine::MutationEngine,
    sealed_schema_reader::{
        blob_catalog_dir_from_env, read_manifest_pinned_schema_document,
        ManifestPinnedSchemaDocument,
    },
    tched_config_dispatch::{self, ConfigReadDispatch},
    tched_config_secret_policy::PinnedConfigSecretPolicy,
    tched_router_runtime::TchedRouterRuntimeClient,
};

const PLUGIN: &str = "tched_router";

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
        .context("sealed tched_router has no native_config state field")?;
    let op_state_store::FieldType::JsonSchema(config_schema) = &exact.field_type else {
        anyhow::bail!("sealed tched_router native_config field is not an exact schema");
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
            "tched_router seal changed during native config dispatch"
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
        .with_context(|| format!("admitted tched_router method '{method}' is absent"))?;
    let returns = declaration
        .returns
        .as_ref()
        .with_context(|| format!("admitted tched_router method '{method}' has no output schema"))?;
    let schema = serde_json::to_value(returns)?;
    let validator =
        jsonschema::Validator::new(&schema).context("invalid admitted method output contract")?;
    validator.validate(output).map_err(|_| {
        anyhow::anyhow!(
            "native output does not conform to admitted tched_router.{method} during output validation"
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

/// Returns None only when the method is outside the v3 config facade.
pub(crate) async fn dispatch(
    engine: &MutationEngine,
    pinned: &ManifestPinnedSchemaDocument,
    method: &str,
    args: &Value,
) -> anyhow::Result<Option<Value>> {
    let admitted = &pinned.schema;
    let contract = load_contract(pinned)?;

    if method == "GetConfigSchema" {
        let output = json!({ "config_schema": contract.config_schema.clone() });
        ensure_current_pin(&contract.schema_hash, &contract.schema_json).await?;
        validate_output(admitted, method, &output)?;
        return Ok(Some(output));
    }

    let runtime = TchedRouterRuntimeClient::from_env();
    match tched_config_dispatch::dispatch_config_read(&runtime, method, args, &contract.secrets)
        .await?
    {
        ConfigReadDispatch::Handled(output) => {
            ensure_current_pin(&contract.schema_hash, &contract.schema_json).await?;
            validate_output(admitted, method, &output)?;
            return Ok(Some(output));
        }
        ConfigReadDispatch::CommittedField(field) => {
            let output = engine
                .read_tched_router_config_field(field, &contract.schema_hash)
                .await?;
            let output = json!({ "config": output });
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
                .persist_tched_router_config_observation(
                    method,
                    &represented,
                    &contract.schema_hash,
                )
                .await?;
            let committed = engine
                .read_tched_router_config_field("native_config", &contract.schema_hash)
                .await?;
            anyhow::ensure!(
                committed == native_config,
                "tched_router native config changed after observation publication"
            );
            ensure_current_pin(&contract.schema_hash, &contract.schema_json).await?;
            let output = select_native_config(&committed, section)?;
            validate_output(admitted, method, &output)?;
            return Ok(Some(output));
        }
        ConfigReadDispatch::NotAConfigRead => {}
    }

    let Some(mutation) =
        tched_config_dispatch::dispatch_config_mutation(&runtime, method, args, &contract.secrets)
            .await?
    else {
        return Ok(None);
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
        .persist_tched_router_config_observation(
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
) -> anyhow::Result<(
    op_plugins::state_plugins::tched_router::TchedRouterState,
    Value,
)> {
    let contract = load_contract(pinned)?;
    let observation = TchedRouterRuntimeClient::from_env()
        .observe_config_and_state(&contract.secrets)
        .await?;
    validate_native_config(&contract, &observation.native_config)?;
    let paired = observation.into_state_and_fields()?;
    ensure_current_pin(&contract.schema_hash, &contract.schema_json).await?;
    Ok(paired)
}
