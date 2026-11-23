//! The identity's sled ledger (owner, 2026-11-17).
//!
//! The container IS the identity. Its sled is the pool volume
//! `identity-<session>`, mounted read-only in the container at /agent/user.
//! The host bridge alone writes `ledger/` there: one line per committed call
//! of that session, already filtered by the sealed schema that created it (no
//! secret or private fields). `finalize_session` vectorizes the ledger once,
//! links each vector in Cozo (actor, session, event hash, Qdrant point id),
//! then clears the ledger. Fixed identity data on the sled is never touched.
//!
//! The journal stays the authority; the ledger is the session's copy for
//! end-of-session vectorization. Both appends and finalization run inside
//! Runtime's admitted method path, never beside it.
//!
//! Ordering (owner, 2026-11-22): call -> execution tracker -> sled, as ONE
//! action. `Runtime::call_owned` builds the schema-filtered entry
//! (`SledEntry`, private fields: only this module can build one) and returns
//! it with the result; the tracker's `ReceiptCompleted` commit
//! (`Runtime::checkpoint_record`) appends it right after `apply_committed`,
//! applying the personal filter inline (`chat_mode_policy::excluded_from_sled`:
//! personal-mode calls, personal namespaces and soul writes never reach the
//! ledger or its vectors). `append_from_tracker` is the sole writer.

use std::io::Write as _;
use std::os::unix::fs::DirBuilderExt as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context as _, ensure};
use serde_json::{Value, json};

use crate::core::{BackendStateUpdate, StateScope};
use crate::runtime::{BackendContext, BackendOutcome, Runtime, RuntimeError, VerifiedCaller};
use op_execution_tracker::{ExecutionRecord, ExecutionStatus};

const CURRENT: &str = "current.jsonl";
const FINALIZING_PREFIX: &str = "finalizing-";

/// Serializes appends with the rename that starts a finalization.
static LEDGER_LOCK: Mutex<()> = Mutex::new(());

/// One committed call's ledger line, produced by `entry_for` and consumed only
/// by `append_from_tracker`. Fields are private to this module.
#[derive(Clone)]
pub(crate) struct SledEntry {
    session: String,
    method: String,
    args: Value,
    terminal_event_hash: Value,
    entry: Value,
}

pub(crate) fn supports(plugin: &str, method: &str) -> bool {
    plugin == "identity_sled" && method == "finalize_session"
}

struct LedgerLocation {
    directory: PathBuf,
    chain_id: Option<String>,
}

/// The sled is selected by the identity owner, never a caller path or pool.
fn ledger_dir(store: &op_cozo_store::CozoGraphShuttle, session: &str) -> anyhow::Result<Option<LedgerLocation>> {
    let parsed = uuid::Uuid::parse_str(session).context("session id is not a UUID")?;
    ensure!(parsed.to_string() == session, "session id is not canonical");
    let relation = store.read_identity_revision(session)?.record
        .map(|record| record.org_relation_json).unwrap_or_default();
    let row = store.lxd_subsidiaries()?.into_iter().find(|row| row.session_id == session);
    let row = crate::runtime::subsidiary::binding(row, &relation)?;
    let (sled, chain_id) = if let Some(row) = row {
        ensure!(!row.native_uuid.is_empty(), "subsidiary sled requires its admitted native binding");
        (PathBuf::from(format!("/var/lib/lxd/storage-pools/lxd-pool/custom/default_identity-{session}")),
            Some(format!("{}::mainchain", row.name)))
    } else {
        let pool = std::env::var("OP_CONTAINER_POOL").unwrap_or_else(|_| "3tched-pool".into());
        (PathBuf::from(format!("/var/lib/incus/storage-pools/{pool}/custom/default_identity-{session}")), None)
    };
    let stored = match std::fs::read_to_string(sled.join("session_id")) {
        Ok(stored) => stored,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && chain_id.is_none() => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("reading {}", sled.display())),
    };
    ensure!(stored.trim() == session, "sled session_id differs from its volume name");
    let ledger = sled.join("ledger");
    std::fs::DirBuilder::new().mode(0o700).recursive(true).create(&ledger)?;
    Ok(Some(LedgerLocation { directory: ledger, chain_id }))
}

/// The ledger line for one committed call (nothing is written here). `None`
/// for the ledger's own finalization method.
pub(crate) fn entry_for(
    caller: &VerifiedCaller, plugin: &str, method: &str, declaration: &op_state_store::MethodDecl,
    args: &Value, receipt: &op_state_store::method_receipt::MethodExecutionReceipt,
) -> Option<SledEntry> {
    if supports(plugin, method) { return None; }
    // The journal's audit view of the arguments, filtered by the sealed args
    // schema: only described fields, no secret or private subtree.
    let audited = if plugin == "elevation" && method == "authorize_task" {
        crate::task_elevation::audit_arguments(args)
    } else {
        args.clone()
    };
    let schema: Value = simd_json::serde::from_owned_value(declaration.args.clone()).unwrap_or_default();
    let mut lines = vec![format!("plugin: {plugin}"), format!("method: {method}")];
    for field in op_state_store::schema_projection::described_fields(&schema, &audited) {
        let label = if field.description.is_empty() { field.path } else { format!("{} ({})", field.path, field.description) };
        let mut value = field.value;
        if value.len() > 512 {
            let mut cut = 512;
            while !value.is_char_boundary(cut) { cut -= 1; }
            value.truncate(cut);
            value.push('…');
        }
        lines.push(format!("{label}: {value}"));
    }
    let entry = json!({
        "at": chrono::Utc::now().to_rfc3339(),
        "session_id": caller.session_id, "actor_id": caller.principal_id,
        "plugin_id": plugin, "method": method, "subid": declaration.subid,
        "operation_id": receipt.operation_id, "event_hash": receipt.event_hash,
        "terminal_event_hash": receipt.terminal_event_hash, "text": lines.join("\n"),
    });
    Some(SledEntry {
        session: caller.session_id.clone(),
        method: method.to_owned(),
        args: audited,
        terminal_event_hash: entry["terminal_event_hash"].clone(),
        entry,
    })
}

/// The tracker record admits the entry only at `ReceiptCompleted`, with the
/// entry's own receipt terminal proof and session.
fn admitted_by_record(record: &ExecutionRecord, staged: &SledEntry) -> bool {
    record.status == ExecutionStatus::ReceiptCompleted
        && record.context.executor_session_id.as_deref() == Some(staged.session.as_str())
        && record.proof.as_ref().is_some_and(|proof| {
            proof.operation_id == record.context.operation_id
                && proof.terminal.as_ref().is_some_and(|terminal| {
                    serde_json::to_value(&terminal.event_hash).ok().as_ref() == Some(&staged.terminal_event_hash)
                })
        })
}

/// Is the session's effective chat mode Personal right now? Stored selection
/// first; otherwise the session kind's default (`chat_mode_policy`).
fn session_is_personal(store: &op_cozo_store::CozoGraphShuttle, session: &str) -> anyhow::Result<bool> {
    let (kind, _) = crate::chat_mode_policy::session_identity(store, session)?;
    let rows = store.run_query(
        "?[document] := *chat_context_selection{session_id, document}, session_id = $session",
        Some(json!({"session": session})),
    )?;
    let stored = match rows.as_array().and_then(|rows| rows.first()) {
        None => None,
        Some(row) => {
            let document = row["document"].as_str().context("invalid stored chat context")?;
            let selection: op_plugins::state_plugins::project::ChatContextSelection =
                serde_json::from_str(document)?;
            Some(selection.mode)
        }
    };
    Ok(crate::chat_mode_policy::effective_mode_is_personal(stored, kind))
}

/// The ONLY sled ledger writer, part of the tracker's `ReceiptCompleted`
/// commit (`Runtime::checkpoint_record`, right after `apply_committed`). A
/// failure is logged, never turned into an unknown outcome: the journal and
/// the tracker already hold the event.
pub(crate) async fn append_from_tracker(runtime: &Runtime, record: &ExecutionRecord, staged: SledEntry) {
    if !admitted_by_record(record, &staged) { return; }
    let store = runtime.identity_store().clone();
    let outcome = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let personal = session_is_personal(&store, &staged.session)?;
        if crate::chat_mode_policy::excluded_from_sled(personal, &staged.session, &staged.method, &staged.args) {
            return Ok(());
        }
        let SledEntry { session, mut entry, .. } = staged;
        let Some(location) = ledger_dir(&store, &session)? else { return Ok(()) };
        if let Some(chain_id) = location.chain_id {
            entry["chain_id"] = chain_id.into();
        }
        let ledger = location.directory;
        let mut line = serde_json::to_vec(&entry)?;
        line.push(b'\n');
        let _guard = LEDGER_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut file = std::fs::OpenOptions::new().create(true).append(true).mode(0o600)
            .open(ledger.join(CURRENT))?;
        file.write_all(&line)?;
        file.sync_data()?;
        Ok(())
    }).await;
    let tool = &record.context.tool_name;
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(%error, %tool, "sled ledger append failed"),
        Err(error) => tracing::warn!(%error, %tool, "sled ledger append task failed"),
    }
}

#[derive(serde::Deserialize)]
struct FinalizeInput { session_id: String }

pub(crate) async fn dispatch(ctx: &BackendContext<'_>, args: &Value) -> Result<BackendOutcome, RuntimeError> {
    let input: FinalizeInput = serde_json::from_value(args.clone())
        .map_err(|error| RuntimeError::before_effect(error.into()))?;
    crate::runtime_misc::revalidate(ctx).await?;
    let subsidiary = crate::runtime::subsidiary::resolve(ctx).await?;
    if subsidiary.is_some() && input.session_id != ctx.caller.session_id {
        return Err(RuntimeError::denied("a subsidiary finalizes only its own session ledger"));
    }
    let store = ctx.runtime.identity_store().clone();
    let session = input.session_id.clone();
    // Start a finalization: later appends open a fresh current ledger, and an
    // interrupted earlier finalization is retried with this one.
    let (segments, collection) = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let location = ledger_dir(&store, &session)?.context("this identity has no sled ledger")?;
        let ledger = location.directory;
        {
            let _guard = LEDGER_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let current = ledger.join(CURRENT);
            if current.exists() {
                let target = ledger.join(format!("{FINALIZING_PREFIX}{}.jsonl", uuid::Uuid::new_v4().simple()));
                std::fs::rename(&current, &target)?;
                std::fs::File::open(&ledger)?.sync_all()?;
            }
        }
        let mut segments = std::fs::read_dir(&ledger)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.file_name().and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(FINALIZING_PREFIX) && name.ends_with(".jsonl")))
            .collect::<Vec<_>>();
        segments.sort();
        Ok((segments, location.chain_id))
    }).await.map_err(|error| RuntimeError::before_effect(error.into()))?
        .map_err(RuntimeError::before_effect)?;

    let mut entries = Vec::new();
    for segment in &segments {
        let text = std::fs::read_to_string(segment).map_err(|error| RuntimeError::unknown(error.into()))?;
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let entry: Value = serde_json::from_str(line).map_err(|error| RuntimeError::unknown(error.into()))?;
            if entry["session_id"] != input.session_id.as_str() {
                return Err(RuntimeError::unknown(anyhow::anyhow!("sled ledger holds another session's entry")));
            }
            entries.push(entry);
        }
    }
    let points = entries.iter().map(|entry| {
        let field = |name: &str| entry[name].as_str().unwrap_or_default().to_owned();
        let mut payload = serde_json::Map::new();
        for name in ["session_id", "actor_id", "plugin_id", "method", "subid", "operation_id", "terminal_event_hash", "at", "chain_id"] {
            payload.insert(name.into(), field(name).into());
        }
        op_cognitive::chain_vectors::LedgerPoint { event_hash: field("event_hash"), text: field("text"), payload }
    }).collect::<Vec<_>>();
    let (model, collection, ids) = op_cognitive::chain_vectors::upsert_session_ledger(
        ctx.runtime.identity_store().as_ref(),
        &points,
        collection.as_deref(),
    ).await.map_err(RuntimeError::unknown)?;

    let cleared = segments.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        // Vectorized and linked: clear the ledger.
        for segment in &cleared { std::fs::remove_file(segment)?; }
        if let Some(parent) = cleared.first().and_then(|segment| segment.parent()) {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    }).await.map_err(|error| RuntimeError::unknown(error.into()))?
        .map_err(RuntimeError::unknown)?;
    crate::runtime_misc::revalidate(ctx).await?;
    let result = json!({"session_id": input.session_id, "vectorized": ids.len() as u64,
        "collection": collection, "embedding_model": model});
    Ok(BackendOutcome { result, state: BackendStateUpdate::Unchanged,
        scope: StateScope::Private { principal_id: ctx.caller.principal_id.clone(), session_id: ctx.caller.session_id.clone() },
        additional: Vec::new(), guard: None })
}

#[cfg(test)]
mod tracker_boundary_tests {
    use super::*;
    use op_execution_tracker::{ExecutionContext, ExecutionProof};
    use op_state_store::method_receipt::MethodEventProof;

    const SID: &str = "2a2d9aa8-5468-309c-2db4-342221450de8";

    fn staged(hash: &str) -> SledEntry {
        SledEntry { session: SID.into(), method: "m".into(), args: json!({}),
            terminal_event_hash: serde_json::to_value(hash.to_owned()).unwrap(), entry: json!({}) }
    }

    fn record(status: ExecutionStatus, terminal: Option<&str>) -> ExecutionRecord {
        let now = chrono::Utc::now();
        ExecutionRecord {
            context: ExecutionContext {
                operation_id: "op-1".into(), trace_id: String::new(), parent_operation_id: None,
                tool_name: "p.m".into(), executor_principal_id: Some("principal".into()),
                executor_session_id: Some(SID.into()), attribution: None,
            },
            status, created_at: now, updated_at: now, duration_ms: 0, cursor: None,
            proof: terminal.map(|hash| ExecutionProof {
                operation_id: "op-1".into(),
                attempt: MethodEventProof { event_id: 1, event_hash: "a".into() },
                terminal: Some(MethodEventProof { event_id: 2, event_hash: hash.into() }),
            }),
        }
    }

    #[test]
    fn only_the_receipt_completed_tracker_commit_admits_a_sled_append() {
        let entry = staged("t");
        assert!(admitted_by_record(&record(ExecutionStatus::ReceiptCompleted, Some("t")), &entry));
        // Not yet committed by the tracker, or a different terminal proof.
        assert!(!admitted_by_record(&record(ExecutionStatus::Running, Some("t")), &entry));
        assert!(!admitted_by_record(&record(ExecutionStatus::ReceiptCompleted, None), &entry));
        assert!(!admitted_by_record(&record(ExecutionStatus::ReceiptCompleted, Some("other")), &entry));
        assert!(!admitted_by_record(&record(ExecutionStatus::Completed, Some("t")), &entry));
    }

}
