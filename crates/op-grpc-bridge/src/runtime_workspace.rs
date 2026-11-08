//! Native workspace composition. Domain owners retain their existing Cozo
//! formats; only Runtime commits the method journal and canonical projection.
use std::{path::{Path, PathBuf}, sync::Arc};

use anyhow::{Context, Result, ensure};
use op_cozo_store::CozoGraphShuttle;
use op_plugins::state_plugins::{
    gemma_brain::workspace::{UiAccessContext, UiEmptyInput, UiWorkspaceMethods, UiWorkspaceRecovery, UiWorkspaceOutput},
    json_render::{JsonRenderState, canvas_profile::PinnedCanvasValidator, methods::JsonRenderMethods},
    oscal_field_classification::{AssessFieldsInput, FieldSource, FieldSourcePin},
    oscal_subid_registry_methods::{CatalogPinVerifier, OscalInvocationContext, OscalSubidRegistryMethods, SealedCatalogSnapshot, SealedSchemaDocument},
    runtime_documents::{RuntimeDocumentRejected, RuntimeDocumentUncertain, RuntimeDocuments, runtime_documents},
    schema_renderer::TargetPluginInput,
    schema_renderer_methods::{SchemaRendererMethods, complete_gallery_method, plan_gallery_method},
    web_ui_methods,
};
use op_state_store::{PluginSchema, PublicStateProjectionError, SideEffect};
use serde_json::Value;

use crate::{catalog::Catalog, core::{BackendStateUpdate, StateScope}, runtime::{BackendContext, BackendOutcome, RuntimeError}};

#[path = "antigravity_dispatch.rs"]
mod antigravity;
#[path = "runtime_workspace_gallery.rs"]
mod gallery;
#[path = "runtime_workspace_project.rs"]
mod project;
#[path = "runtime_workspace_routing.rs"]
mod routing;
#[path = "runtime_workspace_workflows.rs"]
mod workflows;

pub(crate) struct WorkspaceRuntime {
    documents: Arc<RuntimeDocuments>,
    ui: UiWorkspaceMethods,
    json: JsonRenderMethods,
    renderer: SchemaRendererMethods,
    oscal: OscalSubidRegistryMethods,
    oscal_snapshot: SealedCatalogSnapshot,
    gallery: Arc<CozoGraphShuttle>,
    cron: crate::cron_methods::CronMethods,
    projects: project::Projects,
    routing: routing::Routing,
    workflows: workflows::Workflows,
}

struct CatalogPin(PathBuf);
impl CatalogPinVerifier for CatalogPin {
    fn verify_current(&self, expected_catalog_hash: &str) -> Result<()> {
        let manifest: Value = serde_json::from_slice(&std::fs::read(
            self.0.join(op_blob::catalog::MANIFEST_FILENAME),
        )?)?;
        ensure!(manifest["catalog_hash"].as_str() == Some(expected_catalog_hash),
            "sealed catalog changed during workspace operation");
        Ok(())
    }
}

pub(crate) fn schema(catalog: &Catalog, plugin: &str) -> Result<PluginSchema> {
    let entry = catalog.plugin(plugin).context("plugin is absent from sealed catalog")?;
    serde_json::from_str(entry.sealed_schema_json.as_deref()
        .context("plugin has no manifest-pinned schema bytes")?).map_err(Into::into)
}

impl WorkspaceRuntime {
    pub(crate) async fn open(catalog: &Catalog, catalog_dir: &Path) -> Result<Self> {
        let documents = runtime_documents().await?;
        let renderer = catalog.plugin("json_render").context("json_render is not sealed")?;
        let renderer_json: Arc<str> = renderer.sealed_schema_json.as_deref()
            .context("json_render lacks pinned schema bytes")?.into();
        let json_schema = schema(catalog, "json_render")?;
        let snapshot: JsonRenderState = serde_json::from_value(serde_json::to_value(
            op_plugins::projection_seed_state_from_schema(&json_schema),
        )?)?;
        let gallery = gallery::open(documents.shared_store()).await?;
        let oscal_snapshot = SealedCatalogSnapshot::from_documents(
            catalog.hash().to_owned(),
            catalog.plugins().iter().map(|entry| Ok(SealedSchemaDocument {
                plugin_id: entry.id.clone(),
                schema_hash: entry.schema_hash.clone(),
                schema_json: entry.sealed_schema_json.clone()
                    .context("sealed OSCAL source bytes are absent")?,
            })).collect::<Result<Vec<_>>>()?,
        )?;
        Ok(Self {
            ui: UiWorkspaceMethods::from_manifest_pinned_renderer(
                documents.clone(), renderer_json.clone(), renderer.schema_hash.clone(),
            )?,
            json: JsonRenderMethods::new(snapshot, json_schema, documents.clone())?,
            renderer: SchemaRendererMethods::new(PinnedCanvasValidator::from_manifest_pinned_schema(
                renderer_json, renderer.schema_hash.clone(),
            )?)?,
            oscal: OscalSubidRegistryMethods::from_shared(documents.clone(), Arc::new(CatalogPin(catalog_dir.to_owned()))),
            oscal_snapshot,
            documents,
            gallery,
            cron: crate::cron_methods::CronMethods::new(Arc::new(crate::cron_gateway::ChatRouterCronGateway::production())),
            projects: project::Projects::new(),
            routing: routing::Routing::new()?,
            workflows: workflows::Workflows::new(),
        })
    }

    pub(crate) fn ui_methods(&self) -> &UiWorkspaceMethods { &self.ui }
    pub(crate) fn gallery_store(&self) -> Arc<CozoGraphShuttle> { self.gallery.clone() }

    pub(crate) async fn after_commit(&self, ctx: &BackendContext<'_>, plugin: &str, method: &str, result: &Value) -> Result<(), RuntimeError> {
        if plugin == "workflows" { self.workflows.after_commit(ctx, method, result).await?; }
        Ok(())
    }

    async fn json_state(&self) -> Result<Value> {
        let mut state = self.json.projection_state().await?;
        let store = self.gallery.clone();
        let page = tokio::task::spawn_blocking(move || store.list_ui_catalog(None, 64, false)).await??;
        state.catalog_revision = page.revision;
        state.active_catalog_entry_names = page.entries.into_iter().map(|entry| entry.name).collect();
        Ok(serde_json::to_value(state)?)
    }
}

pub(crate) fn supports(plugin: &str, method: &str) -> bool {
    match plugin {
        "antigravity" => matches!(method, "get_auth_status" | "get_usage_report" | "configure_safety"),
        "antigravity_chat" => matches!(method, "get_bridge_status" | "refresh_token" | "configure"),
        "cron" => matches!(method, "list_jobs" | "get_job" | "get_history" | "create_job" | "update_job" | "delete_job" | "enable_job" | "disable_job" | "run_job"),
        "datastore" => matches!(method, "list_objects" | "get_object" | "query" | "get_stats" | "export"),
        "json_render" => matches!(method, "get_config" | "set_config" | "get_health" | "list_tools" | "list_packages" | "get_package" | "list_components" | "get_component_schema" | "list_actions" | "list_core_exports" | "list_renderers" | "get_spec_schema" | "validate_spec" | "export_json_schema" | "build_prompt_surface")
            || op_plugins::state_plugins::json_render::gallery::is_gallery_method(method),
        "oscal_subid_registry" => matches!(method, "register" | "lookup" | "list_by_category" | "resolve" | "export" | "assess_fields"),
        "project" => project::supports(method),
        "routing_decision" | "schema_tag_registry" => routing::supports(plugin, method),
        "schema_renderer" => matches!(method, "render_schema" | "validate_schema" | "list_galleries" | "get_rendering"),
        "web_ui" => web_ui_methods::target_method_for(method).is_some(),
        "workflows" => workflows::supports(method),
        _ => false,
    }
}

pub(crate) fn private_scope(ctx: &BackendContext<'_>) -> StateScope {
    StateScope::Private { principal_id: ctx.caller.principal_id.clone(), session_id: ctx.caller.session_id.clone() }
}

fn outcome(result: Value, state: BackendStateUpdate, scope: StateScope) -> BackendOutcome {
    BackendOutcome { result, state, scope, additional: Vec::new(), guard: None }
}

fn require(condition: bool, message: &str) -> Result<()> {
    ensure!(condition, "{message}");
    Ok(())
}

/// RuntimeDocuments marks pre-submission rejection explicitly. Anything else
/// after entering a mutator is conservatively unknown and is never retried.
pub(crate) fn domain_error(error: anyhow::Error, mutation: bool) -> RuntimeError {
    if error.downcast_ref::<RuntimeDocumentRejected>().is_some() {
        RuntimeError::backend(error)
    } else if mutation || error.downcast_ref::<RuntimeDocumentUncertain>().is_some() {
        RuntimeError::unknown(error)
    } else {
        RuntimeError::backend(error)
    }
}

pub(crate) async fn dispatch(ctx: &BackendContext<'_>, plugin: &str, method: &str, args: &Value) -> Result<BackendOutcome, RuntimeError> {
    let owner = ctx.runtime.workspace();
    let mutation = matches!(ctx.declaration.side_effect, SideEffect::Mutation);
    match plugin {
        "antigravity" | "antigravity_chat" => {
            let value = antigravity::dispatch(plugin, method, args, owner.documents.clone(), op_llm::antigravity::google_credential_owner())
                .await.map_err(|error| domain_error(error, mutation))?;
            Ok(outcome(value.output, BackendStateUpdate::Replace(value.state), StateScope::Public))
        }
        "cron" => {
            let value = owner.cron.dispatch_observed(method, args).await
                .map_err(|error| domain_error(error, mutation))?;
            let uncertain = matches!(value.output["disposition"].as_str(), Some("indeterminate" | "provider_completed_unverified"))
                || (mutation && value.state.is_none());
            let state = value.state.map(serde_json::to_value).transpose()?
                .map_or(BackendStateUpdate::Unchanged, BackendStateUpdate::Replace);
            if uncertain {
                return Err(RuntimeError::unknown(anyhow::anyhow!("cron effect or authoritative readback is uncertain; inspect native history before any retry"))
                    .with_state(state, StateScope::Public));
            }
            Ok(outcome(value.output, state, StateScope::Public))
        }
        "json_render" => {
            let result = if op_plugins::state_plugins::json_render::gallery::is_gallery_method(method) {
                gallery::dispatch(ctx, method, args).await?
            } else {
                owner.json.dispatch(method, args).await.map_err(|error| domain_error(error, mutation))?
            };
            let state = owner.json_state().await.map_err(|error| domain_error(error, mutation))?;
            Ok(outcome(result, BackendStateUpdate::Replace(state), StateScope::Public))
        }
        "oscal_subid_registry" => {
            let caller = OscalInvocationContext { actor_id: ctx.caller.principal_id.clone(), capability_id: ctx.schema.resolve_method_capability(ctx.declaration).capability().to_owned() };
            let result = if method == "assess_fields" {
                let input: AssessFieldsInput = serde_json::from_value(args.clone())?;
                let entry = ctx.runtime.catalog().plugin(&input.plugin_id).context("OSCAL target is not sealed")?;
                let target = schema(ctx.runtime.catalog(), &input.plugin_id)?;
                let assessed = owner.oscal.assess_fields(&caller, FieldSourcePin {
                    plugin_id: input.plugin_id, schema_version: target.version,
                    source: FieldSource::Sealed { catalog_hash: ctx.runtime.catalog().hash().to_owned(), schema_hash: entry.schema_hash.clone() },
                }, entry.sealed_schema_json.clone().context("OSCAL source bytes are absent")?).await
                    .map_err(|error| domain_error(error, mutation))?;
                serde_json::to_value(assessed).map_err(|error| domain_error(error.into(), mutation))?
            } else {
                owner.oscal.dispatch(method, args, &caller, &owner.oscal_snapshot).await
                    .map_err(|error| domain_error(error, mutation))?
            };
            // OSCAL annotations are a private method-owned index; the entry
            // schema has no aggregate annotation-state root to publish.
            Ok(outcome(result, BackendStateUpdate::Unchanged, private_scope(ctx)))
        }
        "schema_renderer" => {
            if matches!(method, "list_galleries" | "get_rendering") {
                let plan = plan_gallery_method(method, args)?;
                let result = ctx.runtime.call_nested(ctx.caller, "gemma_brain", plan.method, plan.args).await?;
                ctx.caller.authority.revalidate().await.map_err(|error| RuntimeError::denied(error.to_string()))?;
                Ok(outcome(complete_gallery_method(method, result.result)?, BackendStateUpdate::Unchanged, private_scope(ctx)))
            } else {
                let input: TargetPluginInput = serde_json::from_value(args.clone())?;
                let entry = ctx.runtime.catalog().plugin(&input.plugin_id).context("render target is not sealed")?;
                let target = schema(ctx.runtime.catalog(), &input.plugin_id)?;
                Ok(outcome(owner.renderer.dispatch(method, &target, &entry.schema_hash)?, BackendStateUpdate::Unchanged, StateScope::Public))
            }
        }
        "web_ui" => {
            let plan = web_ui_methods::plan_method(method, args)?;
            let result = ctx.runtime.call_nested(ctx.caller, plan.plugin_id, plan.method, plan.args).await?;
            ctx.caller.authority.revalidate().await.map_err(|error| RuntimeError::denied(error.to_string()))?;
            Ok(outcome(web_ui_methods::complete_method(method, result.result)?, BackendStateUpdate::Unchanged, private_scope(ctx)))
        }
        "datastore" => datastore(ctx, method, args).await,
        "project" => owner.projects.dispatch(ctx, method, args).await,
        "routing_decision" | "schema_tag_registry" => routing::dispatch(ctx, plugin, method, args).await,
        "workflows" => owner.workflows.dispatch(ctx, method, args).await,
        _ => Err(RuntimeError::unsupported(format!("no workspace method {plugin}.{method}"))),
    }
}

pub(crate) fn supports_gemma_workspace(method: &str) -> bool {
    matches!(method, "get_ui_workspace" | "get_ui_spec" | "get_ui_job" | "list_ui_specs" | "create_ui_spec" | "update_ui_spec" | "handle_ui_event" | "delete_ui_spec" | "delete_ui_job" | "rebind_ui_renderer" | "recover_ui_workspace" | "classify" | "list_perspectives" | "list_tags" | "register_tag" | "analyze_intent")
}

pub(crate) async fn dispatch_gemma_workspace(ctx: &BackendContext<'_>, method: &str, args: &Value) -> Result<BackendOutcome, RuntimeError> {
    let methods = &ctx.runtime.workspace().ui;
    let scope = private_scope(ctx);
    let access = UiAccessContext { principal_id: ctx.caller.principal_id.clone(), session_id: ctx.caller.session_id.clone(), session_genesis: Some(ctx.caller.session_genesis.clone()) };
    if method == "recover_ui_workspace" {
        let _: UiEmptyInput = serde_json::from_value(args.clone())?;
        let path = op_plugins::canonical::plugin_path("gemma_brain");
        let previous = match ctx.runtime.core().committed_snapshot(&scope, "gemma_brain", &path) {
            Ok(previous) => previous,
            Err(_) => {
                ctx.runtime.core().recover_projection_from_ledger(&scope, "gemma_brain")?;
                ctx.runtime.core().committed_snapshot(&scope, "gemma_brain", &path)?
            }
        };
        let recovered = methods.recover_snapshot(&access).await.map_err(|error| domain_error(error, true))?;
        require(recovered.is_some() || previous.as_ref().is_none_or(|snapshot| snapshot.state.get("document_revision").is_none()), "retained UI workspace document disappeared")?;
        let state = recovered.as_ref().map(UiWorkspaceRecovery::plugin_state).transpose()?
            .map_or(BackendStateUpdate::Unchanged, BackendStateUpdate::Fields);
        return Ok(outcome(serde_json::to_value(UiWorkspaceOutput { workspace: recovered })?, state, scope));
    }
    if matches!(method, "get_ui_workspace" | "get_ui_spec" | "get_ui_job" | "list_ui_specs" | "classify" | "list_perspectives" | "list_tags" | "analyze_intent") {
        let current = ctx.runtime.core().committed_snapshot(&scope, "gemma_brain", &op_plugins::canonical::plugin_path("gemma_brain"))?;
        let snapshot = current.as_ref().filter(|current| current.state.get("document_revision").is_some())
            .map(|current| workspace_snapshot(&current.state)).transpose()?;
        if snapshot.is_none() && methods.snapshot(&access).await?.is_some() {
            return Err(RuntimeError::backend(anyhow::anyhow!("UI workspace recovery is required before committed reads")));
        }
        return Ok(outcome(methods.dispatch_committed_read(method, args, snapshot)?, BackendStateUpdate::Unchanged, scope));
    }
    let minted = (method == "create_ui_spec").then(|| uuid::Uuid::now_v7().to_string());
    let committed = methods.dispatch_mutation(&access, method, args, minted.as_deref()).await
        .map_err(|error| domain_error(error, true))?;
    let result = committed.method_result().map_err(RuntimeError::unknown)?;
    let state = committed.plugin_state().map_err(RuntimeError::unknown)?;
    Ok(outcome(result, BackendStateUpdate::Fields(state), scope))
}

pub(super) fn workspace_snapshot(state: &Value) -> Result<UiWorkspaceRecovery> {
    let mut workspace = serde_json::Map::new();
    for key in ["document_revision", "catalog_hash", "jobs", "specs", "tags", "taxonomy_revision"] {
        workspace.insert(key.into(), state.get(key).with_context(|| format!("committed UI field {key} missing"))?.clone());
    }
    UiWorkspaceRecovery::from_plugin_state(&Value::Object(workspace))
}

async fn datastore(ctx: &BackendContext<'_>, method: &str, args: &Value) -> Result<BackendOutcome, RuntimeError> {
    use op_plugins::state_plugins::datastore::*;
    let ids = match method {
        "get_object" => vec![serde_json::from_value::<GetObjectInput>(args.clone())?.plugin_id],
        "query" => serde_json::from_value::<QueryInput>(args.clone())?.plugin_ids,
        "get_stats" => { let _: GetStatsInput = serde_json::from_value(args.clone())?; Vec::new() },
        _ => serde_json::from_value::<PluginSelection>(args.clone())?.plugin_ids,
    };
    require(ids.len() <= MAX_DATASTORE_SELECTION, "datastore selection exceeds capacity")?;
    let unique: std::collections::BTreeSet<_> = ids.iter().collect();
    require(unique.len() == ids.len(), "duplicate datastore plugin selection")?;
    let catalog = ctx.runtime.catalog().clone();
    for id in &ids { require(catalog.plugin(id).is_some(), "datastore selection is not sealed")?; }
    let selected: Vec<_> = if ids.is_empty() { catalog.plugins().iter().map(|entry| entry.id.clone()).collect() } else { ids };
    let state_dir = op_core::projection_shm::shm_state_dir();
    let read_ids = selected.clone();
    let snapshot = tokio::task::spawn_blocking(move || op_core::projection_shm::read_projection_snapshot_in(&state_dir, &read_ids)).await??;
    let binding = SnapshotBinding { generation: snapshot.generation, catalog_hash: catalog.hash().to_owned() };
    let mut objects = Vec::with_capacity(selected.len());
    for id in selected {
        let entry = catalog.plugin(&id).context("datastore catalog entry disappeared")?;
        let contract = schema(&catalog, &id)?;
        contract.validate_public_state_policy().map_err(|e| RuntimeError::backend(anyhow::anyhow!("{}", e)))?;
        let state = match snapshot.states.get(&id) {
            Some(bytes) => {
                let mut bytes = bytes.clone();
                let value = simd_json::to_owned_value(&mut bytes)?;
                contract.project_public_state(&value).map_err(|e| RuntimeError::backend(anyhow::anyhow!("{}", e)))?.map(|value| serde_json::to_value(crate::state_reader::public_plugin_value(&id, &value))).transpose()?
            }
            None => None,
        };
        let exposure = match (state.is_some(), contract.public_state.paths.is_empty()) {
            (true, false) => PublicObjectExposure::Present,
            (false, true) => PublicObjectExposure::Withheld,
            (false, false) => PublicObjectExposure::Unobserved,
            (true, true) => return Err(RuntimeError::backend(anyhow::anyhow!("withheld datastore state escaped public policy"))),
        };
        let state_bytes = state.as_ref().map(serde_json::to_vec).transpose()?.map_or(0, |bytes| bytes.len() as u64);
        objects.push(DataStoreObject { reference: DataStoreObjectRef { plugin_id: id.clone(), object_path: op_plugins::canonical::plugin_path(&id), schema_hash: entry.schema_hash.clone(), exposure, state_bytes }, schema: entry.schema.clone().context("datastore schema absent")?, state });
    }
    let result = match method {
        "list_objects" => serde_json::to_value(ListObjectsOutput { binding, objects: objects.into_iter().map(|object| object.reference).collect() })?,
        "get_object" => serde_json::to_value(GetObjectOutput { binding, object: objects.pop().context("datastore object absent")? })?,
        "query" => {
            let input: QueryInput = serde_json::from_value(args.clone())?;
            if let Some(exposure) = input.exposure { objects.retain(|object| object.reference.exposure == exposure); }
            serde_json::to_value(QueryOutput { binding, results: objects })?
        }
        "get_stats" => serde_json::to_value(GetStatsOutput { binding, object_count: objects.len(), present_count: objects.iter().filter(|object| object.reference.exposure == PublicObjectExposure::Present).count(), withheld_count: objects.iter().filter(|object| object.reference.exposure == PublicObjectExposure::Withheld).count(), unobserved_count: objects.iter().filter(|object| object.reference.exposure == PublicObjectExposure::Unobserved).count(), state_bytes: objects.iter().map(|object| object.reference.state_bytes).sum() })?,
        "export" => serde_json::to_value(ExportOutput { binding, objects })?,
        _ => return Err(RuntimeError::unsupported(format!("unknown datastore method {method}"))),
    };
    require(serde_json::to_vec(&result)?.len() <= MAX_DATASTORE_RESPONSE_BYTES, "datastore response exceeds capacity")?;
    Ok(outcome(result, BackendStateUpdate::Unchanged, StateScope::Public))
}
