//! Capability-preserving facade dispatch into canonical tched_router methods.
//!
//! This module owns no provider, catalog, selection or generation state. The
//! facade call is admitted first, then the exact nested target method is
//! independently admitted and dispatched with the same verified identity.

use crate::{
    interceptor::load_capability_grants, method_contract_admission,
    mutation_engine::MutationEngine, project_dispatch,
};
use anyhow::{Context, Result};
use op_plugins::state_plugins::large_language_model_methods::{
    complete_method, plan_method, TargetPlan,
};

fn admit_target(plan: &TargetPlan, actor: &str) -> Result<()> {
    let declaration = method_contract_admission::admit_compiled_method(
        plan.plugin_id,
        plan.method,
        Some(&plan.required_capability),
    )?;
    method_contract_admission::validate_admitted_arguments(&declaration, &plan.args)?;
    anyhow::ensure!(
        load_capability_grants(actor).contains(&plan.required_capability),
        "exact nested tched_router capability grant is required"
    );
    Ok(())
}

/// Forward one already outer-admitted facade method through the canonical
/// target path. The recursive call intentionally performs target admission a
/// second time at the final effect boundary and records its own target audit.
#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch<'a>(
    engine: &'a MutationEngine,
    method: &'a str,
    args: &'a serde_json::Value,
    actor: &'a str,
    session: Option<&'a str>,
    genesis: Option<&'a str>,
    admitted_schema: Option<&'a op_state_store::PluginSchema>,
) -> futures::future::BoxFuture<'a, Result<serde_json::Value>> {
    // Erase the recursive canonical-dispatch future at the forwarding boundary
    // while retaining Send, so the owned target task can outlive its caller.
    Box::pin(async move {
        let plan = plan_method(method, args)?;
        admit_target(&plan, actor)?;
        let verified_session =
            project_dispatch::verified_session(engine, actor, session, genesis).await?;
        let origin = if method == "stream_generate" {
            anyhow::ensure!(
                load_capability_grants(actor).contains("llm.invoke"),
                "current exact facade grant is required"
            );
            Some(crate::generation_admission::GenerationOrigin::admit(
                &crate::sealed_schema_reader::blob_catalog_dir_from_env(),
                admitted_schema.context("generation facade admission is missing")?,
            )?)
        } else {
            None
        };
        let target_args = serde_json::to_string(&plan.args)?;
        let engine = MutationEngine::clone(engine);
        let actor = actor.to_string();
        let genesis = genesis.map(str::to_string);
        let target_capability = plan.required_capability.clone();
        let target_plugin = plan.plugin_id;
        let target_method = plan.method;
        // MutationEngine clones share every authority and lock. Dropping the outer
        // facade request therefore does not cancel a target effect between native
        // completion, readback and SHM publication.
        let target_result = tokio::spawn(async move {
            Box::pin(engine.dispatch_method_call_with_generation_origin(
                target_plugin,
                target_method,
                &target_args,
                Some(&target_capability),
                &actor,
                Some(&verified_session),
                genesis.as_deref(),
                origin,
            ))
            .await
        })
        .await
        .context("large_language_model target task failed")?
        .with_context(|| {
            format!(
                "large_language_model.{method} target {}.{} failed",
                target_plugin, target_method
            )
        })?;
        let target_domain = target_result
            .get("result")
            .cloned()
            .context("canonical tched_router response is missing its domain result")?;
        complete_method(method, target_domain)
    })
}

/// Refresh one paired config/selection observation while retaining the caller's
/// final admission and runtime guard through authoritative publication.
pub(crate) async fn observed_tched_router_state(
    engine: &MutationEngine,
    pinned: &crate::sealed_schema_reader::ManifestPinnedSchemaDocument,
    sealed_identity: &str,
) -> Result<op_plugins::state_plugins::tched_router::TchedRouterState> {
    let (observed, represented) =
        crate::tched_config_engine::coherent_observation(pinned, sealed_identity).await?;
    engine
        .persist_tched_router_observation(&represented, &pinned.schema_hash)
        .await
        .context("native tched_router read completed but projection publication failed")?;
    Ok(observed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_state::StatePlugin;

    #[test]
    fn facade_plan_never_weakens_the_nested_capability() {
        for (facade, args) in [
            ("list_providers", serde_json::json!({})),
            ("get_provider", serde_json::json!({})),
            ("set_provider", serde_json::json!({"provider_id":"native"})),
            ("generate", serde_json::json!({"message":"hello"})),
            ("list_models", serde_json::json!({"provider":"native"})),
            ("get_config", serde_json::json!({})),
            (
                "update_config",
                serde_json::json!({"provider_id":"native","model_id":"model"}),
            ),
        ] {
            let plan = plan_method(facade, &args).unwrap();
            let target = op_plugins::state_plugins::tched_router::TchedRouterPlugin::new()
                .schema()
                .unwrap();
            let declaration = &target.methods[plan.method];
            assert_eq!(
                plan.required_capability,
                target.resolve_method_capability(declaration).capability()
            );
        }
    }

    #[test]
    fn stream_targets_exact_generation_start_contract() {
        let plan = plan_method(
            "stream_generate",
            &serde_json::json!({"agent_alias":"isolated", "prompt":"hi"}),
        )
        .unwrap();
        assert_eq!(plan.method, "start_generation");
        assert_eq!(
            plan.required_capability,
            op_plugins::state_plugins::tched_router::generation::START_CAPABILITY
        );
    }
}
