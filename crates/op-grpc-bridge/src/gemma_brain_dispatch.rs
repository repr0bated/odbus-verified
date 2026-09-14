//! Authenticated facade for the retained Gemma route method.
//!
//! Routing remains owned by `tched_router`. This module only translates the
//! legacy lower-case facade method into the canonical `ResolveRoute` call.

use crate::{
    interceptor::load_capability_grants, method_contract_admission,
    mutation_engine::MutationEngine, project_dispatch,
};
use anyhow::{Context, Result};
use op_plugins::state_plugins::tched_router::{ResolveRouteInput, ResolveRouteOutput};
use op_state::StatePlugin;
use op_state_store::PluginSchema;

const TARGET_PLUGIN: &str = "tched_router";
const TARGET_METHOD: &str = "ResolveRoute";

fn target_capability() -> Result<String> {
    let schema = op_plugins::state_plugins::tched_router::TchedRouterPlugin::new()
        .schema()
        .context("tched_router schema is unavailable")?;
    let declaration = schema
        .methods
        .get(TARGET_METHOD)
        .context("tched_router does not declare ResolveRoute")?;
    Ok(schema
        .resolve_method_capability(declaration)
        .capability()
        .to_string())
}

fn plan(args: &serde_json::Value) -> Result<(serde_json::Value, String)> {
    let input: ResolveRouteInput =
        serde_json::from_value(args.clone()).context("invalid gemma_brain.route arguments")?;
    anyhow::ensure!(
        !input.hint.trim().is_empty(),
        "gemma_brain.route hint must not be empty"
    );
    // Keep the original arguments for the nested schema validator; do not
    // silently discard unknown fields during typed deserialization.
    Ok((args.clone(), target_capability()?))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch<'a>(
    engine: &'a MutationEngine,
    args: &'a serde_json::Value,
    actor: &'a str,
    session: Option<&'a str>,
    genesis: Option<&'a str>,
    admitted: &'a PluginSchema,
) -> futures::future::BoxFuture<'a, Result<serde_json::Value>> {
    Box::pin(async move {
        let outer = admitted
            .methods
            .get("route")
            .context("admitted gemma_brain.route declaration is absent")?;
        let outer_requirement = admitted.resolve_method_capability(outer);
        let outer_capability = outer_requirement.capability();
        anyhow::ensure!(
            load_capability_grants(actor).contains(outer_capability),
            "exact Gemma route capability grant is required"
        );
        let verified_session =
            project_dispatch::verified_session(engine, actor, session, genesis).await?;
        let (target_args, target_capability) = plan(args)?;
        let target_declaration = method_contract_admission::admit_compiled_method(
            TARGET_PLUGIN,
            TARGET_METHOD,
            Some(&target_capability),
        )?;
        method_contract_admission::validate_admitted_arguments(&target_declaration, &target_args)?;
        anyhow::ensure!(
            load_capability_grants(actor).contains(&target_capability),
            "exact nested tched_router route capability grant is required"
        );

        let target_args = serde_json::to_string(&target_args)?;
        let actor = actor.to_owned();
        let genesis = genesis.map(str::to_owned);
        let target_capability_for_call = target_capability.clone();
        let engine = engine.clone();
        let target_result = tokio::spawn(async move {
            engine
                .dispatch_method_call_with_identity(
                    TARGET_PLUGIN,
                    TARGET_METHOD,
                    &target_args,
                    Some(&target_capability_for_call),
                    &actor,
                    Some(&verified_session),
                    genesis.as_deref(),
                )
                .await
        })
        .await
        .context("gemma route target task failed")??;

        let domain = target_result
            .get("result")
            .cloned()
            .context("canonical tched_router response is missing its domain result")?;
        Ok(serde_json::to_value(
            serde_json::from_value::<ResolveRouteOutput>(domain)
                .context("tched_router.ResolveRoute returned an invalid route")?,
        )?)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_plan_preserves_hint_and_target_contract() {
        let (args, capability) = plan(&serde_json::json!({"hint":"fast"})).unwrap();
        assert_eq!(args, serde_json::json!({"hint":"fast"}));
        assert_eq!(capability, "cap.software.3tched-router.route.resolve@v1");
    }

    #[test]
    fn route_plan_rejects_missing_hint() {
        assert!(plan(&serde_json::json!({"hint":"  "})).is_err());
    }
}
