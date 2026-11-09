//! The retained principal-owned gallery and global promoted catalog. Native
//! Cozo transactions own candidate, catalog and staged relation atomicity.
use std::{path::PathBuf, sync::Arc};
use anyhow::{Context, Result, ensure};
use op_cozo_store::{CozoError, CozoGraphShuttle, UiCandidateRecord, UiCatalogRecord};
use op_plugins::state_plugins::json_render::{chat_canvas::ChatCanvasValidator, gallery::*};
use serde_json::Value;
use crate::runtime::{BackendContext, RuntimeError};

pub(super) async fn open(runtime_store: Arc<CozoGraphShuttle>) -> Result<Arc<CozoGraphShuttle>> {
    let path = std::env::var("UI_GALLERY_DB_PATH").unwrap_or_else(|_| "/var/lib/op-dbus/ui-gallery.db".into());
    ensure!(!path.is_empty() && path != ":memory:" && path != "memory", "UI gallery requires its durable owner");
    tokio::task::spawn_blocking(move || {
        let path = PathBuf::from(path);
        ensure!(path.is_absolute(), "UI gallery database path must be absolute");
        for (variable, default) in [
            ("CTL_PLANE_CHATBOT_EPISODE_DB_PATH", "/var/lib/op-dbus/chatbot-episodes.db"),
            ("CHAT_RESPONSE_DB_PATH", "/var/lib/op-dbus/chat-responses.db"),
            ("MODEL_REGISTRY_DB_PATH", "/var/lib/op-dbus/model-registry.db"),
        ] {
            let other = PathBuf::from(std::env::var(variable).unwrap_or_else(|_| default.into()));
            ensure!(path != other, "UI gallery aliases another native database owner");
            if let (Ok(a), Ok(b)) = (std::fs::canonicalize(&path), std::fs::canonicalize(&other)) {
                ensure!(a != b, "UI gallery aliases another native database owner");
            }
        }
        let store = CozoGraphShuttle::new_persistent_private(path)?;
        store.ensure_distinct_from(&runtime_store)?;
        store.initialize_ui_gallery_store()?;
        Ok(Arc::new(store))
    }).await.context("UI gallery initialization worker failed")?
}

async fn blocking<T: Send + 'static>(ctx: &BackendContext<'_>, mutation: bool, run: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T, RuntimeError> {
    let authority = ctx.caller.authority.clone();
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        handle.block_on(authority.revalidate())?;
        run()
    }).await.map_err(|error| super::domain_error(error.into(), mutation))?
        .map_err(|error| super::domain_error(error, mutation))
}

fn candidate(row: &UiCandidateRecord, principal: &str) -> Result<Candidate> {
    let value: Candidate = serde_json::from_value(row.candidate.clone())?;
    validate_candidate(&value)?;
    ensure!(row.principal_id == principal && value.principal_id == principal && value.candidate_id == row.candidate_id, "gallery candidate owner binding differs");
    Ok(value)
}

fn entry(row: &UiCatalogRecord) -> Result<CatalogEntry> {
    let value: CatalogEntry = serde_json::from_value(row.entry.clone())?;
    validate_entry(&value)?;
    ensure!(value.entry_id == row.entry_id && value.status == row.status, "gallery entry key binding differs");
    Ok(value)
}

fn entry_row(value: &CatalogEntry) -> Result<UiCatalogRecord> {
    validate_entry(value)?;
    Ok(UiCatalogRecord { entry_id: value.entry_id.clone(), name: value.name.clone(), status: value.status.clone(), spec_hash: value.spec_hash.clone(), from_principal: value.promoted_by.clone(), from_candidate_id: value.from_candidate_id.clone(), entry: serde_json::to_value(value)? })
}

pub(super) async fn dispatch(ctx: &BackendContext<'_>, method: &str, arguments: &Value) -> Result<Value, RuntimeError> {
    let store = ctx.runtime.workspace().gallery_store();
    store.ensure_distinct_from(&ctx.runtime.chat().episode_store())?;
    let principal = ctx.caller.principal_id.clone();
    let owner = ctx.runtime.catalog().plugin("json_render").context("json_render not sealed")?;
    let validator = ChatCanvasValidator::from_pinned_schema_json(owner.sealed_schema_json.as_deref().context("json_render schema bytes absent")?)?;
    let profile_sha256 = validator.profile().evidence.profile_sha256.clone();
    let result = match method {
        "save_candidate" => {
            let input: SaveCandidateInput = serde_json::from_value(arguments.clone())?;
            validate_name(&input.name)?;
            validate_description(&input.description)?;
            validator.validate(&input.spec)?;
            let spec_hash = spec_hash(&input.spec);
            let value = Candidate {
                candidate_id: uuid::Uuid::now_v7().to_string(), principal_id: principal,
                name: input.name.clone(), description: input.description.clone(), spec: input.spec.clone(),
                svelte_source: input.svelte_source.clone(), compiled_output: input.compiled_output.clone(),
                content_hash: content_hash(&input.name, &input.description, &input.spec, &input.svelte_source, &input.compiled_output, &input.cozo_relations),
                spec_hash, cozo_relations: input.cozo_relations.clone(),
                profile_sha256, created_at: chrono::Utc::now().to_rfc3339(), promoted_entry_id: None,
            };
            validate_candidate(&value)?;
            let row = UiCandidateRecord {
                principal_id: value.principal_id.clone(), candidate_id: value.candidate_id.clone(),
                spec_hash: value.spec_hash.clone(), promoted_entry_id: None,
                candidate: serde_json::to_value(&value)?,
                backend_relations: String::new(),
            };
            let committed = blocking(ctx, true, move || {
                store.insert_ui_candidate(&row)?;
                store.get_ui_candidate(&row.principal_id, &row.candidate_id)?
                    .context("saved candidate missing from durable owner")
            }).await?;
            serde_json::to_value(CandidateOutput { candidate: candidate(&committed, &ctx.caller.principal_id).map_err(RuntimeError::unknown)? })?
        }
        "list_candidates" => {
            let input: PageInput = serde_json::from_value(arguments.clone())?;
            let limit = page_limit(input.limit)?;
            if let Some(before) = &input.before { validate_uuid_v7("before", before)?; }
            let key = principal.clone();
            let page = blocking(ctx, false, move || Ok(store.list_ui_candidates(&key, input.before.as_deref(), limit)?)).await?;
            let candidates = page.candidates.iter().map(|row| candidate(row, &principal).map(|value| CandidateSummary::from(&value))).collect::<Result<Vec<_>>>()?;
            let next_before = page.more.then(|| candidates.last().map(|value| value.candidate_id.clone())).flatten();
            serde_json::to_value(CandidatePage { candidates, next_before, total: page.total })?
        }
        "get_candidate" => {
            let input: CandidateRefInput = serde_json::from_value(arguments.clone())?;
            validate_uuid_v7("candidate_id", &input.candidate_id)?;
            let key = principal.clone();
            let row = blocking(ctx, false, move || store.get_ui_candidate(&key, &input.candidate_id)?.context("candidate not found")).await?;
            serde_json::to_value(CandidateOutput { candidate: candidate(&row, &principal)? })?
        }
        "delete_candidate" => {
            let input: CandidateRefInput = serde_json::from_value(arguments.clone())?;
            validate_uuid_v7("candidate_id", &input.candidate_id)?;
            let deleted = blocking(ctx, true, move || Ok(store.delete_ui_candidate(&principal, &input.candidate_id)?)).await?;
            serde_json::to_value(DeletedOutput { deleted })?
        }
        "promote_candidate" => {
            let input: PromoteInput = serde_json::from_value(arguments.clone())?;
            validate_uuid_v7("candidate_id", &input.candidate_id)?;
            let entry_id = uuid::Uuid::now_v7().to_string();
            let promoted_at = chrono::Utc::now().to_rfc3339();
            let record = blocking(ctx, true, move || {
                let build = |row: &UiCandidateRecord| -> std::result::Result<UiCatalogRecord, CozoError> {
                    let make = || -> Result<_> {
                        let value = candidate(row, &principal)?;
                        validator.validate(&value.spec).context("candidate rejected by current sealed Canvas profile")?;
                        entry_row(&CatalogEntry {
                            entry_id: entry_id.clone(), name: value.name.clone(), description: value.description.clone(),
                            spec: value.spec.clone(), svelte_source: value.svelte_source.clone(), compiled_output: value.compiled_output.clone(),
                            content_hash: value.content_hash.clone(), spec_hash: value.spec_hash.clone(), cozo_relations: value.cozo_relations.clone(),
                            profile_sha256: profile_sha256.clone(),
                            from_candidate_id: value.candidate_id, promoted_by: principal.clone(), promoted_at: promoted_at.clone(),
                            status: "active".into(), retired_at: None, retired_by: None, retire_reason: None,
                        })
                    };
                    make().map_err(|error| CozoError::Other(error.to_string()))
                };
                // promote_ui_candidate applies staged backend relations inside
                // this same transaction. No second import or retry follows it.
                Ok(store.promote_ui_candidate(&principal, &input.candidate_id, &build)?.0)
            }).await?;
            serde_json::to_value(CatalogEntryOutput { entry: entry(&record).map_err(RuntimeError::unknown)? })?
        }
        "list_catalog" => {
            let input: CatalogPageInput = serde_json::from_value(arguments.clone())?;
            let limit = page_limit(input.limit)?;
            if let Some(before) = &input.before { validate_uuid_v7("before", before)?; }
            let page = blocking(ctx, false, move || Ok(store.list_ui_catalog(input.before.as_deref(), limit, input.include_retired)?)).await?;
            let entries = page.entries.iter().map(|row| entry(row).map(|value| CatalogEntrySummary::from(&value))).collect::<Result<Vec<_>>>()?;
            let next_before = page.more.then(|| entries.last().map(|value| value.entry_id.clone())).flatten();
            serde_json::to_value(CatalogPage { entries, next_before, total: page.total })?
        }
        "get_catalog_entry" => {
            let input: CatalogEntryRefInput = serde_json::from_value(arguments.clone())?;
            validate_uuid_v7("entry_id", &input.entry_id)?;
            let row = blocking(ctx, false, move || store.get_ui_catalog_entry(&input.entry_id)?.context("catalog entry not found")).await?;
            serde_json::to_value(CatalogEntryOutput { entry: entry(&row)? })?
        }
        "retire_catalog_entry" => {
            let input: RetireInput = serde_json::from_value(arguments.clone())?;
            validate_uuid_v7("entry_id", &input.entry_id)?;
            validate_reason(&input.reason)?;
            let retired_at = chrono::Utc::now().to_rfc3339();
            let row = blocking(ctx, true, move || {
                let build = |row: &UiCatalogRecord| -> std::result::Result<UiCatalogRecord, CozoError> {
                    let make = || -> Result<_> {
                        let mut value = entry(row)?;
                        value.status = "retired".into();
                        value.retired_at = Some(retired_at.clone());
                        value.retired_by = Some(principal.clone());
                        value.retire_reason = (!input.reason.is_empty()).then(|| input.reason.clone());
                        entry_row(&value)
                    };
                    make().map_err(|error| CozoError::Other(error.to_string()))
                };
                Ok(store.retire_ui_catalog_entry(&input.entry_id, &build)?)
            }).await?;
            serde_json::to_value(CatalogEntryOutput { entry: entry(&row).map_err(RuntimeError::unknown)? })?
        }
        _ => return Err(RuntimeError::unsupported(format!("unknown json_render gallery method {method}"))),
    };
    ctx.caller.authority.revalidate().await.map_err(|error| RuntimeError::denied(error.to_string()))?;
    Ok(result)
}
