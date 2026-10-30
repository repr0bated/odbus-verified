//! Canonical Cron method integration.
//!
//! The admitted OP-DBUS method and verified session authorize the call; the
//! chat-router bearer authenticates only this bridge to the loopback daemon.
//! Cron records are install-global: scheduled execution remains autonomous
//! under the configured native agent's risk/tool policy and does not retain or
//! re-check the caller's OP-DBUS grant.

use crate::{
    cron_gateway::ChatRouterCronGateway,
    cron_methods::{CronMethods, mark_publication_unverified},
    method_contract_admission,
    mutation_engine::MutationEngine,
    sealed_schema_reader::{
        ManifestPinnedSchemaDocument, blob_catalog_dir_from_env,
        read_manifest_pinned_schema_document,
    },
};
use anyhow::{Context, Result};
use op_state_store::PluginSchema;
use std::sync::Arc;

const PLUGIN: &str = "cron";
static CRON_OPERATION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct SealPin {
    schema_hash: String,
    schema_json: String,
}

fn is_mutation(method: &str) -> bool {
    matches!(
        method,
        "create_job" | "update_job" | "delete_job" | "enable_job" | "disable_job" | "run_job"
    )
}

fn pin_current_contract() -> Result<SealPin> {
    let pinned = read_manifest_pinned_schema_document(&blob_catalog_dir_from_env(), PLUGIN)?;
    method_contract_admission::admit_compiled_state_contract(&pinned, PLUGIN)?;
    Ok(SealPin {
        schema_hash: pinned.schema_hash,
        schema_json: pinned.blob.schema_json,
    })
}

fn seal_is_unchanged(expected: &SealPin) -> Result<()> {
    let current: ManifestPinnedSchemaDocument =
        read_manifest_pinned_schema_document(&blob_catalog_dir_from_env(), PLUGIN)?;
    method_contract_admission::admit_compiled_state_contract(&current, PLUGIN)?;
    anyhow::ensure!(
        current.schema_hash == expected.schema_hash
            && current.blob.schema_json == expected.schema_json,
        "cron plugin seal changed during native operation"
    );
    Ok(())
}

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(operation)
        .await
        .context("cron sealed-contract worker failed")?
}

/// Commit one complete native Cron readback. This is shared by admitted method
/// calls and the autonomous event observer; the observer never projects event
/// payloads. The global operation guard orders mutations and observations.
pub(crate) async fn publish_observed_snapshot(
    engine: &MutationEngine,
    service: &CronMethods,
) -> Result<()> {
    let _operation = CRON_OPERATION.lock().await;
    let pin = blocking(pin_current_contract).await?;
    let state = service.observe_state().await?;
    let expected_hash = pin.schema_hash.clone();
    blocking(move || seal_is_unchanged(&pin)).await?;
    engine
        .process_observed_state(
            PLUGIN,
            simd_json::serde::to_owned_value(state)?,
            expected_hash,
        )
        .await
        .map_err(anyhow::Error::msg)
        .context("complete native Cron observation could not be committed")
        .map(|_| ())
}

/// Dispatch an already method/capability-admitted call. Identity is checked
/// again before the first daemon request; no caller-supplied identity reaches
/// the native gateway.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn dispatch(
    engine: &MutationEngine,
    method: &str,
    args: &serde_json::Value,
    actor: &str,
    capability: Option<&str>,
    session: Option<&str>,
    genesis: Option<&str>,
    admitted_schema: &PluginSchema,
) -> Result<serde_json::Value> {
    let mutation = is_mutation(method);
    let _operation = CRON_OPERATION.lock().await;
    let caller = crate::chat_router_admission::RouterCaller {
        actor: actor.to_owned(),
        capability: capability.map(str::to_owned),
        session: session.map(str::to_owned),
        genesis: genesis.map(str::to_owned),
    };
    let pinned = crate::chat_router_admission::readmit_call(
        engine,
        admitted_schema,
        PLUGIN,
        method,
        &caller,
    )
    .await?;
    let pin = SealPin {
        schema_hash: pinned.schema_hash,
        schema_json: pinned.blob.schema_json,
    };
    let service = CronMethods::new(Arc::new(ChatRouterCronGateway::production()));
    let mut receipt = service.dispatch_observed(method, args).await?;

    let expected_hash = pin.schema_hash.clone();
    let post_seal = blocking(move || seal_is_unchanged(&pin)).await;
    if post_seal.is_err() {
        if mutation {
            if receipt
                .output
                .get("disposition")
                .and_then(serde_json::Value::as_str)
                != Some("indeterminate")
            {
                mark_publication_unverified(method, &mut receipt.output)?;
            }
            return Ok(receipt.output);
        }
        return post_seal.map(|_| receipt.output);
    }

    if let Some(state) = receipt.state {
        let state = simd_json::serde::to_owned_value(state)?;
        if let Err(_error) = engine
            .process_observed_state(PLUGIN, state, expected_hash)
            .await
        {
            if mutation {
                mark_publication_unverified(method, &mut receipt.output)?;
                return Ok(receipt.output);
            }
            anyhow::bail!("cron observation could not be committed to authoritative state");
        }
    }
    Ok(receipt.output)
}
