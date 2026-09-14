//! Capability-preserving Web UI facade into the canonical private Gemma workspace.

use anyhow::{Context, Result};

use crate::{
    interceptor::load_capability_grants, method_contract_admission,
    mutation_engine::MutationEngine, project_dispatch, tched_router_admission::FacadeOrigin,
};
use op_plugins::state_plugins::web_ui_methods::{complete_method, plan_method, TargetPlan};
use op_state_store::PluginSchema;

fn admit_target(plan: &TargetPlan, actor: &str) -> Result<()> {
    let declaration = method_contract_admission::admit_compiled_method(
        plan.plugin_id,
        plan.method,
        Some(&plan.required_capability),
    )?;
    method_contract_admission::validate_admitted_arguments(&declaration, &plan.args)?;
    anyhow::ensure!(
        load_capability_grants(actor).contains(&plan.required_capability),
        "exact nested gemma_brain capability grant is required"
    );
    Ok(())
}

/// The outer Web UI call is already admitted. The nested call repeats exact
/// Gemma admission at the final workspace boundary with the same verified
/// identity/session/genesis and therefore cannot turn a Web UI grant into a
/// workspace grant.
pub(crate) fn dispatch<'a>(
    engine: &'a MutationEngine,
    method: &'a str,
    args: &'a serde_json::Value,
    actor: &'a str,
    session: Option<&'a str>,
    genesis: Option<&'a str>,
    admitted: &'a PluginSchema,
) -> futures::future::BoxFuture<'a, Result<serde_json::Value>> {
    Box::pin(async move {
        let outer = admitted
            .methods
            .get(method)
            .context("admitted web_ui method is absent")?;
        let outer_capability = admitted
            .resolve_method_capability(outer)
            .capability()
            .to_owned();
        anyhow::ensure!(
            load_capability_grants(actor).contains(&outer_capability),
            "current exact web_ui facade capability grant is required"
        );
        let plan = plan_method(method, args)?;
        admit_target(&plan, actor)?;
        let verified_session =
            project_dispatch::verified_session(engine, actor, session, genesis).await?;
        let target_args = serde_json::to_string(&plan.args)?;
        let engine = MutationEngine::clone(engine);
        let actor = actor.to_owned();
        let genesis = genesis.map(str::to_owned);
        let target_capability = plan.required_capability.clone();
        let target_plugin = plan.plugin_id;
        let target_method = plan.method;
        let origin = FacadeOrigin {
            plugin: "web_ui",
            method: method.to_owned(),
            target_plugin,
            target_method,
            admitted: admitted.clone(),
            capability: outer_capability,
        };
        let target_result = tokio::spawn(async move {
            Box::pin(engine.dispatch_method_call_with_facade_origin(
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
        .context("web_ui target task failed")?
        .with_context(|| {
            format!("web_ui.{method} target {target_plugin}.{target_method} failed")
        })?;
        let target_domain = target_result
            .get("result")
            .cloned()
            .context("canonical gemma_brain response is missing its domain result")?;
        complete_method(method, target_domain)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_state::StatePlugin;

    #[test]
    fn facade_plan_never_weakens_workspace_capability() {
        for (facade, args) in [
            ("render_page", serde_json::json!({"spec_id":"spec"})),
            (
                "update_component",
                serde_json::json!({
                    "spec_id":"spec",
                    "expected_revision":2,
                    "element_id":"heading",
                    "properties":{"text":"new"}
                }),
            ),
        ] {
            let plan = plan_method(facade, &args).unwrap();
            let target = op_plugins::state_plugins::gemma_brain::GemmaBrainPlugin::new()
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
    fn event_uses_exact_workspace_mutation_capability() {
        let plan = plan_method(
            "handle_event",
            &serde_json::json!({"spec_id":"spec","expected_revision":1,"element_id":"tile","event":"resize"}),
        )
        .unwrap();
        assert_eq!(plan.method, "handle_ui_event");
        assert_eq!(plan.required_capability, "gemma.invoke");
    }
}
