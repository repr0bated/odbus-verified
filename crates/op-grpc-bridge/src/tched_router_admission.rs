//! Final router admission under the one runtime operation guard.
//!
//! This retains the original caller metadata and admitted plugin; it mints no
//! identity and grants no authority. Resealing and revocation remain separate
//! transactions, so a change after this check can still race a native request.

use crate::{mutation_engine::MutationEngine, sealed_schema_reader::ManifestPinnedSchemaDocument};
use anyhow::{Context, Result};
use op_state_store::PluginSchema;

pub(crate) struct RouterCaller {
    pub actor: String,
    pub capability: Option<String>,
    pub session: Option<String>,
    pub genesis: Option<String>,
}

/// Original facade admission carried to the final native operation boundary.
/// This is internal evidence, never a wire-supplied identity or grant.
#[derive(Clone)]
pub(crate) struct FacadeOrigin {
    pub plugin: &'static str,
    pub method: String,
    pub target_plugin: &'static str,
    pub target_method: &'static str,
    pub admitted: PluginSchema,
    pub capability: String,
}

impl FacadeOrigin {
    pub fn validate_target(&self, plugin: &str, method: &str) -> Result<()> {
        anyhow::ensure!(
            plugin == self.target_plugin && method == self.target_method,
            "facade evidence does not match its target"
        );
        Ok(())
    }

    pub async fn readmit(&self, engine: &MutationEngine, caller: &RouterCaller) -> Result<()> {
        let outer = RouterCaller {
            actor: caller.actor.clone(),
            capability: Some(self.capability.clone()),
            session: caller.session.clone(),
            genesis: caller.genesis.clone(),
        };
        readmit_call(engine, &self.admitted, self.plugin, &self.method, &outer).await?;
        Ok(())
    }
}

pub(crate) struct GuardedRouterCall {
    pub pinned: ManifestPinnedSchemaDocument,
    _operation: tokio::sync::MutexGuard<'static, ()>,
}

pub(crate) async fn admit_guarded(
    engine: &MutationEngine,
    admitted: &PluginSchema,
    method: &str,
    caller: &RouterCaller,
) -> Result<GuardedRouterCall> {
    let operation = crate::tched_router_runtime::TchedRouterRuntimeClient::selection_guard().await;
    let pinned = readmit_call(engine, admitted, "tched_router", method, caller).await?;
    Ok(GuardedRouterCall {
        pinned,
        _operation: operation,
    })
}

/// Caller already retains the backend's operation guard. This shared check is
/// also used by the Cron facade, whose independent guard orders its own jobs.
pub(crate) async fn readmit_call(
    engine: &MutationEngine,
    admitted: &PluginSchema,
    plugin: &'static str,
    method: &str,
    caller: &RouterCaller,
) -> Result<ManifestPinnedSchemaDocument> {
    // Queued calls must not use identity or grants captured before the wait.
    crate::project_dispatch::verified_session(
        engine,
        &caller.actor,
        caller.session.as_deref(),
        caller.genesis.as_deref(),
    )
    .await?;
    let admitted = admitted.clone();
    let method = method.to_owned();
    let capability = caller.capability.clone();
    let actor = caller.actor.clone();
    let pinned = tokio::task::spawn_blocking(move || -> Result<_> {
        let pinned = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
            &crate::sealed_schema_reader::blob_catalog_dir_from_env(),
            plugin,
        )?;
        crate::method_contract_admission::admit_compiled_state_contract(&pinned, plugin)?;
        crate::method_contract_admission::admit_pinned_compiled_method(
            &pinned,
            plugin,
            &method,
            capability.as_deref(),
        )?;
        anyhow::ensure!(
            serde_json::to_value(&pinned.schema)? == serde_json::to_value(&admitted)?,
            "backend seal differs from the originally admitted call"
        );
        let required = capability
            .as_deref()
            .context("backend call requires its exact declared capability")?;
        anyhow::ensure!(
            crate::interceptor::load_capability_grants(&actor).contains(required),
            "current exact backend capability grant is required"
        );
        Ok(pinned)
    })
    .await
    .context("router admission worker failed")??;
    Ok(pinned)
}

#[cfg(test)]
#[path = "tched_router_admission_tests.rs"]
mod tests;
