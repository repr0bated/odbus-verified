//! Manifest-pinned bridge adapter for deterministic schema rendering.

use anyhow::{Context, Result};
use op_plugins::state_plugins::{
    json_render::canvas_profile::PinnedCanvasValidator,
    schema_renderer::TargetPluginInput,
    schema_renderer_methods::{
        SchemaRendererMethods, complete_gallery_method, plan_gallery_method,
    },
};

use crate::{
    interceptor::load_capability_grants,
    method_contract_admission,
    mutation_engine::MutationEngine,
    project_dispatch,
    sealed_schema_reader::{blob_catalog_dir_from_env, read_manifest_pinned_catalog},
};

pub(crate) fn dispatch<'a>(
    engine: &'a MutationEngine,
    method: &'a str,
    args: &'a serde_json::Value,
    actor: &'a str,
    session: Option<&'a str>,
    genesis: Option<&'a str>,
    admitted: &'a op_state_store::PluginSchema,
) -> futures::future::BoxFuture<'a, Result<serde_json::Value>> {
    Box::pin(async move {
        if matches!(method, "list_galleries" | "get_rendering") {
            let caller = crate::chat_router_admission::RouterCaller {
                actor: actor.to_owned(),
                capability: Some("schema_renderer.read".to_owned()),
                session: session.map(str::to_owned),
                genesis: genesis.map(str::to_owned),
            };
            let pinned = crate::chat_router_admission::readmit_call(
                engine,
                admitted,
                "schema_renderer",
                method,
                &caller,
            )
            .await?;
            let plan = plan_gallery_method(method, args)?;
            let declaration = method_contract_admission::admit_compiled_method(
                "gemma_brain",
                plan.method,
                Some(&plan.capability),
            )?;
            method_contract_admission::validate_admitted_arguments(&declaration, &plan.args)?;
            anyhow::ensure!(
                load_capability_grants(actor).contains(&plan.capability),
                "exact nested Gemma workspace read grant is required"
            );
            let session =
                project_dispatch::verified_session(engine, actor, session, genesis).await?;
            let result = engine
                .dispatch_method_call_with_identity(
                    "gemma_brain",
                    plan.method,
                    &serde_json::to_string(&plan.args)?,
                    Some(&plan.capability),
                    actor,
                    Some(&session),
                    genesis,
                )
                .await?;
            let result = result
                .get("result")
                .cloned()
                .context("canonical workspace result is absent")?;
            // Both read authorities must still be current before exposing a
            // private result, including when admission waited on workspace I/O.
            project_dispatch::verified_session(engine, actor, Some(&session), genesis).await?;
            let grants = load_capability_grants(actor);
            anyhow::ensure!(
                grants.contains(&plan.capability) && grants.contains("schema_renderer.read"),
                "gallery read authority was withdrawn"
            );
            let current = crate::chat_router_admission::readmit_call(
                engine,
                admitted,
                "schema_renderer",
                method,
                &caller,
            )
            .await?;
            anyhow::ensure!(
                current.schema_hash == pinned.schema_hash
                    && current.blob.schema_json == pinned.blob.schema_json,
                "gallery facade seal changed during workspace read"
            );
            method_contract_admission::admit_compiled_method(
                "gemma_brain",
                plan.method,
                Some(&plan.capability),
            )?;
            return complete_gallery_method(method, result);
        }
        dispatch_schema(method, args).await
    })
}

async fn dispatch_schema(method: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
    let target_id = match method {
        "render_schema" | "validate_schema" => {
            let input: TargetPluginInput = serde_json::from_value(args.clone())
                .context("invalid schema_renderer target arguments")?;
            validate_plugin_id(&input.plugin_id)?;
            input.plugin_id
        }
        other => anyhow::bail!("unsupported schema_renderer method: {other}"),
    };
    let catalog_dir = blob_catalog_dir_from_env();
    let method = method.to_owned();
    let args = args.clone();
    tokio::task::spawn_blocking(move || {
        let catalog = read_manifest_pinned_catalog(&catalog_dir)?;
        let owner = catalog
            .plugins
            .get("schema_renderer")
            .context("schema_renderer is absent from the sealed catalog")?;
        let declaration = method_contract_admission::admit_pinned_compiled_method(
            owner,
            "schema_renderer",
            &method,
            Some("schema_renderer.read"),
        )?;
        method_contract_admission::validate_admitted_arguments(&declaration, &args)?;
        let renderer = catalog
            .plugins
            .get("json_render")
            .context("json_render is absent from the sealed catalog")?;
        let target = catalog
            .plugins
            .get(&target_id)
            .context("target plugin is absent from the sealed catalog")?;
        method_contract_admission::admit_compiled_state_contract(renderer, "json_render")?;
        method_contract_admission::admit_compiled_state_contract(target, &target_id)?;
        let validator = PinnedCanvasValidator::from_manifest_pinned_schema(
            renderer.blob.schema_json.clone().into(),
            renderer.schema_hash.clone(),
        )?;
        SchemaRendererMethods::new(validator)?.dispatch(
            &method,
            &target.schema,
            &target.schema_hash,
        )
    })
    .await
    .context("schema_renderer worker failed")?
}

fn validate_plugin_id(value: &str) -> Result<()> {
    anyhow::ensure!(
        !value.is_empty() && value.len() <= 128,
        "invalid target plugin id"
    );
    anyhow::ensure!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')),
        "invalid target plugin id"
    );
    Ok(())
}
