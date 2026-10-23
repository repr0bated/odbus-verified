//! json_render UI gallery and active catalog methods (OD-93).
//!
//! The caller is a verified human session holding the method's exact grant,
//! rechecked here. Candidates are scoped by the caller's principal; the
//! catalog is system-wide. Canvases are validated against the chat Canvas
//! profile sealed in the manifest-pinned json_render blob, and the admitted
//! schema must be that blob's. The dedicated Cozo owner holds every row;
//! reads page through it. The admitted call is itself the chain record of
//! each save, deletion, promotion and retirement.

use anyhow::{Context, Result};
use op_cozo_store::{CozoError, UiCandidateRecord, UiCatalogRecord};
use op_plugins::state_plugins::json_render::{
    chat_canvas::ChatCanvasValidator,
    gallery::{
        Candidate, CandidateOutput, CandidatePage, CandidateRefInput, CandidateSummary,
        CatalogEntry, CatalogEntryOutput, CatalogEntryRefInput, CatalogEntrySummary, CatalogPage,
        CatalogPageInput, DeletedOutput, PageInput, PromoteInput, RetireInput, SaveCandidateInput,
        page_limit, spec_hash, validate_candidate, validate_description, validate_entry,
        validate_mode, validate_name, validate_prompt, validate_reason, validate_title,
        validate_uuid_v7,
    },
};
use op_state_store::PluginSchema;
use serde_json::Value;

use crate::{
    interceptor::load_capability_grants,
    method_contract_admission,
    mutation_engine::MutationEngine,
    project_dispatch,
    sealed_schema_reader::{blob_catalog_dir_from_env, read_manifest_pinned_schema_document},
};

const PLUGIN: &str = "json_render";

pub(crate) use op_plugins::state_plugins::json_render::gallery::is_gallery_method;

#[cfg(test)]
#[path = "json_render_gallery_tests.rs"]
pub(crate) mod tests;

struct Caller {
    principal_id: String,
}

/// The sealed chat Canvas validator and its profile digest, from the blob
/// the admitted schema must be.
struct Seal {
    validator: ChatCanvasValidator,
    profile_sha256: String,
}

async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(operation)
        .await
        .context("json_render gallery worker failed")?
}

fn pin(admitted: &PluginSchema) -> Result<Seal> {
    let pinned = read_manifest_pinned_schema_document(&blob_catalog_dir_from_env(), PLUGIN)
        .context("json_render is absent from the sealed catalog")?;
    method_contract_admission::admit_compiled_state_contract(&pinned, PLUGIN)?;
    anyhow::ensure!(
        serde_json::to_value(&pinned.schema)? == serde_json::to_value(admitted)?,
        "admitted json_render schema differs from the current sealed plugin"
    );
    let validator = ChatCanvasValidator::from_pinned_schema_json(&pinned.blob.schema_json)?;
    let profile_sha256 = validator.profile().evidence.profile_sha256.clone();
    Ok(Seal {
        validator,
        profile_sha256,
    })
}

fn required_capability<'a>(schema: &'a PluginSchema, method: &str) -> Result<&'a str> {
    schema
        .methods
        .get(method)
        .with_context(|| format!("admitted {PLUGIN}.{method} declaration is absent"))?
        .required_capability
        .as_deref()
        .context("admitted json_render gallery method has no exact capability")
}

fn validate_output(schema: &PluginSchema, method: &str, output: &Value) -> Result<()> {
    let declaration = schema
        .methods
        .get(method)
        .with_context(|| format!("admitted {PLUGIN}.{method} declaration is absent"))?;
    method_contract_admission::validate_admitted_output(declaration, output)
        .context("json_render gallery output does not conform to the admitted sealed contract")
}

fn args<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T> {
    serde_json::from_value(args.clone()).context("invalid json_render gallery arguments")
}

fn cozo(error: CozoError) -> anyhow::Error {
    anyhow::anyhow!("{error}")
}

fn decode_candidate(row: &UiCandidateRecord, principal_id: &str) -> Result<Candidate> {
    let candidate: Candidate = serde_json::from_value(row.candidate.clone())
        .context("durable candidate is not the typed contract")?;
    validate_candidate(&candidate)?;
    anyhow::ensure!(
        row.principal_id == principal_id
            && candidate.principal_id == principal_id
            && candidate.candidate_id == row.candidate_id,
        "durable candidate differs from its principal key"
    );
    Ok(candidate)
}

fn decode_entry(row: &UiCatalogRecord) -> Result<CatalogEntry> {
    let entry: CatalogEntry = serde_json::from_value(row.entry.clone())
        .context("durable catalog entry is not the typed contract")?;
    validate_entry(&entry)?;
    anyhow::ensure!(
        entry.entry_id == row.entry_id && entry.status == row.status,
        "durable catalog entry differs from its key"
    );
    Ok(entry)
}

fn entry_record(entry: &CatalogEntry) -> Result<UiCatalogRecord> {
    validate_entry(entry)?;
    Ok(UiCatalogRecord {
        entry_id: entry.entry_id.clone(),
        name: entry.name.clone(),
        status: entry.status.clone(),
        spec_hash: entry.spec_hash.clone(),
        from_principal: entry.promoted_by.clone(),
        from_candidate_id: entry.from_candidate_id.clone(),
        entry: serde_json::to_value(entry)?,
    })
}

pub(crate) async fn dispatch(
    engine: &MutationEngine,
    method: &str,
    arguments: &Value,
    actor: &str,
    session: Option<&str>,
    genesis: Option<&str>,
    admitted: &PluginSchema,
) -> Result<Value> {
    anyhow::ensure!(
        is_gallery_method(method),
        "not a json_render gallery method"
    );
    // The session must be a live, anchored session of this registered
    // principal; the principal is the scope key.
    project_dispatch::verified_session(engine, actor, session, genesis).await?;
    let principal_id = actor.to_string();
    let required = required_capability(admitted, method)?;
    anyhow::ensure!(
        load_capability_grants(&principal_id).contains(required),
        "current exact {required} grant is required"
    );
    let caller = Caller { principal_id };
    let store = engine.ui_gallery_cozo().await?;
    let admitted_for_pin = admitted.clone();
    let seal = blocking(move || pin(&admitted_for_pin)).await?;
    let output = match method {
        "save_candidate" => save(&caller, &seal, store, args(arguments)?).await?,
        "list_candidates" => list_candidates(&caller, store, args(arguments)?).await?,
        "get_candidate" => {
            let input: CandidateRefInput = args(arguments)?;
            validate_uuid_v7("candidate_id", &input.candidate_id)?;
            let principal = caller.principal_id.clone();
            let row = blocking(move || {
                store
                    .get_ui_candidate(&principal, &input.candidate_id)
                    .map_err(cozo)
            })
            .await?
            .context("candidate not found")?;
            serde_json::to_value(CandidateOutput {
                candidate: decode_candidate(&row, &caller.principal_id)?,
            })?
        }
        "delete_candidate" => {
            let input: CandidateRefInput = args(arguments)?;
            validate_uuid_v7("candidate_id", &input.candidate_id)?;
            let principal = caller.principal_id.clone();
            let deleted = blocking(move || {
                store
                    .delete_ui_candidate(&principal, &input.candidate_id)
                    .map_err(cozo)
            })
            .await?;
            serde_json::to_value(DeletedOutput { deleted })?
        }
        "promote_candidate" => promote(&caller, &seal, store, args(arguments)?).await?,
        "list_catalog" => list_catalog(store, args(arguments)?).await?,
        "get_catalog_entry" => {
            let input: CatalogEntryRefInput = args(arguments)?;
            validate_uuid_v7("entry_id", &input.entry_id)?;
            let row = blocking(move || store.get_ui_catalog_entry(&input.entry_id).map_err(cozo))
                .await?
                .context("catalog entry not found")?;
            serde_json::to_value(CatalogEntryOutput {
                entry: decode_entry(&row)?,
            })?
        }
        "retire_catalog_entry" => retire(&caller, store, args(arguments)?).await?,
        _ => unreachable!("gallery methods are matched above"),
    };
    validate_output(admitted, method, &output)?;
    Ok(output)
}

async fn save(
    caller: &Caller,
    seal: &Seal,
    store: std::sync::Arc<op_cozo_store::CozoGraphShuttle>,
    input: SaveCandidateInput,
) -> Result<Value> {
    validate_title(&input.title)?;
    validate_mode(&input.mode)?;
    validate_prompt(input.prompt.as_deref())?;
    seal.validator
        .validate(&input.spec)
        .context("canvas rejected by the sealed chat Canvas profile; nothing was saved")?;
    let candidate = Candidate {
        candidate_id: uuid::Uuid::now_v7().to_string(),
        principal_id: caller.principal_id.clone(),
        title: input.title,
        mode: input.mode,
        prompt: input.prompt,
        spec_hash: spec_hash(&input.spec),
        spec: input.spec,
        profile_sha256: seal.profile_sha256.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        promoted_entry_id: None,
    };
    validate_candidate(&candidate)?;
    let row = UiCandidateRecord {
        principal_id: candidate.principal_id.clone(),
        candidate_id: candidate.candidate_id.clone(),
        spec_hash: candidate.spec_hash.clone(),
        promoted_entry_id: None,
        candidate: serde_json::to_value(&candidate)?,
    };
    blocking(move || store.insert_ui_candidate(&row).map_err(cozo)).await?;
    Ok(serde_json::to_value(CandidateOutput { candidate })?)
}

async fn list_candidates(
    caller: &Caller,
    store: std::sync::Arc<op_cozo_store::CozoGraphShuttle>,
    input: PageInput,
) -> Result<Value> {
    let limit = page_limit(input.limit)?;
    if let Some(before) = &input.before {
        validate_uuid_v7("before", before)?;
    }
    let principal = caller.principal_id.clone();
    let page = blocking(move || {
        store
            .list_ui_candidates(&principal, input.before.as_deref(), limit)
            .map_err(cozo)
    })
    .await?;
    let candidates = page
        .candidates
        .iter()
        .map(|row| decode_candidate(row, &caller.principal_id).map(|c| CandidateSummary::from(&c)))
        .collect::<Result<Vec<_>>>()?;
    let next_before = page
        .more
        .then(|| candidates.last().map(|c| c.candidate_id.clone()))
        .flatten();
    Ok(serde_json::to_value(CandidatePage {
        candidates,
        next_before,
        total: page.total,
    })?)
}

async fn promote(
    caller: &Caller,
    seal: &Seal,
    store: std::sync::Arc<op_cozo_store::CozoGraphShuttle>,
    input: PromoteInput,
) -> Result<Value> {
    validate_uuid_v7("candidate_id", &input.candidate_id)?;
    validate_name(&input.name)?;
    validate_description(&input.description)?;
    let principal = caller.principal_id.clone();
    let candidate_id = input.candidate_id.clone();
    // The candidate is re-read and re-validated inside the promotion
    // transaction; a canvas the current profile rejects is not promoted.
    let validator_check = {
        let row = {
            let store = store.clone();
            let principal = principal.clone();
            let candidate_id = candidate_id.clone();
            blocking(move || {
                store
                    .get_ui_candidate(&principal, &candidate_id)
                    .map_err(cozo)
            })
            .await?
            .context("candidate not found")?
        };
        let candidate = decode_candidate(&row, &principal)?;
        seal.validator.validate(&candidate.spec).context(
            "the candidate's canvas no longer passes the sealed chat Canvas profile; it cannot be promoted",
        )?;
        candidate.spec_hash
    };
    let entry_id = uuid::Uuid::now_v7().to_string();
    let promoted_at = chrono::Utc::now().to_rfc3339();
    let profile_sha256 = seal.profile_sha256.clone();
    let (record, _) = blocking(move || {
        let build = |row: &UiCandidateRecord| -> std::result::Result<UiCatalogRecord, CozoError> {
            let candidate = decode_candidate(row, &principal)
                .map_err(|error| CozoError::Other(error.to_string()))?;
            if candidate.spec_hash != validator_check {
                return Err(CozoError::Other(
                    "candidate changed while it was being promoted".into(),
                ));
            }
            let entry = CatalogEntry {
                entry_id: entry_id.clone(),
                name: input.name.clone(),
                description: input.description.clone(),
                spec: candidate.spec,
                spec_hash: candidate.spec_hash,
                profile_sha256: profile_sha256.clone(),
                from_candidate_id: candidate.candidate_id,
                promoted_by: principal.clone(),
                promoted_at: promoted_at.clone(),
                status: "active".into(),
                retired_at: None,
                retired_by: None,
                retire_reason: None,
            };
            entry_record(&entry).map_err(|error| CozoError::Other(error.to_string()))
        };
        store
            .promote_ui_candidate(&principal, &candidate_id, &build)
            .map_err(cozo)
    })
    .await?;
    Ok(serde_json::to_value(CatalogEntryOutput {
        entry: decode_entry(&record)?,
    })?)
}

async fn list_catalog(
    store: std::sync::Arc<op_cozo_store::CozoGraphShuttle>,
    input: CatalogPageInput,
) -> Result<Value> {
    let limit = page_limit(input.limit)?;
    if let Some(before) = &input.before {
        validate_uuid_v7("before", before)?;
    }
    let page = blocking(move || {
        store
            .list_ui_catalog(input.before.as_deref(), limit, input.include_retired)
            .map_err(cozo)
    })
    .await?;
    let entries = page
        .entries
        .iter()
        .map(|row| decode_entry(row).map(|entry| CatalogEntrySummary::from(&entry)))
        .collect::<Result<Vec<_>>>()?;
    let next_before = page
        .more
        .then(|| entries.last().map(|entry| entry.entry_id.clone()))
        .flatten();
    Ok(serde_json::to_value(CatalogPage {
        entries,
        next_before,
        total: page.total,
    })?)
}

async fn retire(
    caller: &Caller,
    store: std::sync::Arc<op_cozo_store::CozoGraphShuttle>,
    input: RetireInput,
) -> Result<Value> {
    validate_uuid_v7("entry_id", &input.entry_id)?;
    validate_reason(&input.reason)?;
    let retired_by = caller.principal_id.clone();
    let retired_at = chrono::Utc::now().to_rfc3339();
    let record = blocking(move || {
        let build = |row: &UiCatalogRecord| -> std::result::Result<UiCatalogRecord, CozoError> {
            let mut entry =
                decode_entry(row).map_err(|error| CozoError::Other(error.to_string()))?;
            entry.status = "retired".into();
            entry.retired_at = Some(retired_at.clone());
            entry.retired_by = Some(retired_by.clone());
            entry.retire_reason = (!input.reason.is_empty()).then(|| input.reason.clone());
            entry_record(&entry).map_err(|error| CozoError::Other(error.to_string()))
        };
        store
            .retire_ui_catalog_entry(&input.entry_id, &build)
            .map_err(cozo)
    })
    .await?;
    Ok(serde_json::to_value(CatalogEntryOutput {
        entry: decode_entry(&record)?,
    })?)
}
