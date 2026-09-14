//! MCP server configuration reuses the native router owner. Public gateway
//! toolsets use the one root-protected policy file and the live gateway owner.
//! Original admission is retained until each guarded effect commits.

use anyhow::{Context, Result};
use op_plugins::state_plugins::mcp::methods::{complete_method, plan_method};
use op_plugins::state_plugins::mcp::ConfigureToolGroupsInput;
use op_state_store::PluginSchema;
use serde_json::Value;

use crate::{
    interceptor::load_capability_grants,
    method_contract_admission,
    mutation_engine::MutationEngine,
    project_dispatch,
    sealed_schema_reader::{blob_catalog_dir_from_env, read_manifest_pinned_schema_document},
    tched_router_admission::FacadeOrigin,
};

async fn configure_tool_groups(
    engine: &MutationEngine,
    args: &Value,
    actor: &str,
    session: &str,
    genesis: Option<&str>,
    admitted: &PluginSchema,
    capability: &str,
) -> Result<Value> {
    let input: ConfigureToolGroupsInput = serde_json::from_value(args.clone())
        .context("invalid mcp.configure_tool_groups arguments")?;
    let pinned = read_manifest_pinned_schema_document(&blob_catalog_dir_from_env(), "mcp")?;
    method_contract_admission::admit_compiled_state_contract(&pinned, "mcp")?;
    anyhow::ensure!(
        serde_json::to_value(&pinned.schema)? == serde_json::to_value(admitted)?,
        "admitted MCP schema differs from current sealed plugin"
    );
    let pin_hash = pinned.schema_hash;
    let pin_json = pinned.blob.schema_json;
    let owner = engine.mcp_policy_owner()?;
    let mutation = owner.begin_mutation().await;

    project_dispatch::verified_session(engine, actor, Some(session), genesis).await?;
    anyhow::ensure!(
        load_capability_grants(actor).contains(capability),
        "current exact MCP facade capability grant is required"
    );
    let current = read_manifest_pinned_schema_document(&blob_catalog_dir_from_env(), "mcp")?;
    method_contract_admission::admit_compiled_state_contract(&current, "mcp")?;
    anyhow::ensure!(
        current.schema_hash == pin_hash && current.blob.schema_json == pin_json,
        "MCP plugin seal changed during policy update"
    );

    let output = mutation.configure(input)?;
    engine
        .publish_mcp_toolsets(&output, &pin_hash)
        .await
        .context("MCP policy committed and activated but projection publication failed")?;
    drop(mutation);
    Ok(serde_json::to_value(output)?)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch<'a>(
    engine: &'a MutationEngine,
    method: &'a str,
    args: &'a Value,
    actor: &'a str,
    session: Option<&'a str>,
    genesis: Option<&'a str>,
    admitted: &'a PluginSchema,
) -> futures::future::BoxFuture<'a, Result<Value>> {
    Box::pin(async move {
        let outer = admitted
            .methods
            .get(method)
            .context("MCP method declaration absent")?;
        let requirement = admitted.resolve_method_capability(outer);
        let capability = requirement.capability().to_owned();
        anyhow::ensure!(
            load_capability_grants(actor).contains(&capability),
            "exact MCP facade capability grant is required"
        );
        let session = project_dispatch::verified_session(engine, actor, session, genesis).await?;
        if method == "configure_tool_groups" {
            return configure_tool_groups(
                engine,
                args,
                actor,
                &session,
                genesis,
                admitted,
                &capability,
            )
            .await;
        }
        let plan = plan_method(method, args)?;
        let target = method_contract_admission::admit_compiled_method(
            "tched_router",
            plan.target_method,
            Some(&plan.required_capability),
        )?;
        method_contract_admission::validate_admitted_arguments(&target, &plan.args)?;
        anyhow::ensure!(
            load_capability_grants(actor).contains(&plan.required_capability),
            "exact nested router configuration grant is required"
        );
        let origin = FacadeOrigin {
            plugin: "mcp",
            method: method.to_owned(),
            target_plugin: "tched_router",
            target_method: plan.target_method,
            admitted: admitted.clone(),
            capability,
        };
        let input = serde_json::to_string(&plan.args)?;
        let result = engine
            .dispatch_method_call_with_facade_origin(
                "tched_router",
                plan.target_method,
                &input,
                Some(&plan.required_capability),
                actor,
                Some(&session),
                genesis,
                origin,
            )
            .await?;
        complete_method(
            method,
            result
                .get("result")
                .cloned()
                .context("router domain result absent")?,
        )
    })
}

#[cfg(test)]
#[path = "mcp_config_dispatch_tests.rs"]
mod tests;
