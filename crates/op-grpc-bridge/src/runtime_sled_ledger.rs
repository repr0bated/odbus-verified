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

use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context as _, ensure};
use serde_json::{Value, json};

use crate::core::{BackendStateUpdate, StateScope};
use crate::runtime::{BackendContext, BackendOutcome, RuntimeError, VerifiedCaller};

const CURRENT: &str = "current.jsonl";
const FINALIZING_PREFIX: &str = "finalizing-";

/// Serializes appends with the rename that starts a finalization.
static LEDGER_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn supports(plugin: &str, method: &str) -> bool {
    plugin == "identity_sled" && method == "finalize_session"
}

/// The sled ledger directory for `session`, if that identity has a sled.
fn ledger_dir(session: &str) -> anyhow::Result<Option<PathBuf>> {
    let parsed = uuid::Uuid::parse_str(session).context("session id is not a UUID")?;
    ensure!(parsed.to_string() == session, "session id is not canonical");
    let pool = std::env::var("OP_CONTAINER_POOL").unwrap_or_else(|_| "3tched-pool".into());
    let sled = PathBuf::from(format!("/var/lib/incus/storage-pools/{pool}/custom/default_identity-{session}"));
    let stored = match std::fs::read_to_string(sled.join("session_id")) {
        Ok(stored) => stored,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("reading {}", sled.display())),
    };
    ensure!(stored.trim() == session, "sled session_id differs from its volume name");
    let ledger = sled.join("ledger");
    std::fs::DirBuilder::new().mode(0o700).recursive(true).create(&ledger)?;
    Ok(Some(ledger))
}

/// Append one committed call to the caller's sled ledger. A session without
/// a sled (a human without a container) has no ledger. Called after commit:
/// a failure here is logged, never turned into an unknown outcome, because
/// the journal already holds the event.
pub(crate) async fn append_committed(
    caller: &VerifiedCaller, plugin: &str, method: &str, declaration: &op_state_store::MethodDecl,
    args: &Value, receipt: &op_state_store::method_receipt::MethodExecutionReceipt,
) {
    if supports(plugin, method) { return; }
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
    let session = caller.session_id.clone();
    let outcome = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let Some(ledger) = ledger_dir(&session)? else { return Ok(()) };
        let mut line = serde_json::to_vec(&entry)?;
        line.push(b'\n');
        let _guard = LEDGER_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut file = std::fs::OpenOptions::new().create(true).append(true).mode(0o600)
            .open(ledger.join(CURRENT))?;
        file.write_all(&line)?;
        file.sync_data()?;
        Ok(())
    }).await;
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(error)) => tracing::warn!(%error, plugin, method, "sled ledger append failed"),
        Err(error) => tracing::warn!(%error, plugin, method, "sled ledger append task failed"),
    }
}

#[derive(serde::Deserialize)]
struct FinalizeInput { session_id: String }

pub(crate) async fn dispatch(ctx: &BackendContext<'_>, args: &Value) -> Result<BackendOutcome, RuntimeError> {
    let input: FinalizeInput = serde_json::from_value(args.clone())
        .map_err(|error| RuntimeError::before_effect(error.into()))?;
    crate::runtime_misc::revalidate(ctx).await?;
    let session = input.session_id.clone();
    // Start a finalization: later appends open a fresh current ledger, and an
    // interrupted earlier finalization is retried with this one.
    let segments = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<PathBuf>> {
        let ledger = ledger_dir(&session)?.context("this identity has no sled ledger")?;
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
        Ok(segments)
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
        for name in ["session_id", "actor_id", "plugin_id", "method", "subid", "operation_id", "terminal_event_hash", "at"] {
            payload.insert(name.into(), field(name).into());
        }
        op_cognitive::chain_vectors::LedgerPoint { event_hash: field("event_hash"), text: field("text"), payload }
    }).collect::<Vec<_>>();
    let (model, collection, ids) = op_cognitive::chain_vectors::upsert_session_ledger(&points).await
        .map_err(RuntimeError::unknown)?;

    // Cozo is the index: each Qdrant point linked to its actor, session and event.
    let cozo = ctx.runtime.identity_store().clone();
    let links = entries.iter().zip(&ids).map(|(entry, id)| {
        let mut link = vec![("source".to_owned(), "sled_ledger".to_owned()),
            ("collection".to_owned(), collection.clone()), ("embedding_model".to_owned(), model.clone())];
        for name in ["session_id", "actor_id", "event_hash", "plugin_id", "method", "operation_id"] {
            link.push((name.to_owned(), entry[name].as_str().unwrap_or_default().to_owned()));
        }
        (id.clone(), link)
    }).collect::<Vec<_>>();
    let cleared = segments.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        for (id, link) in &links {
            let pairs = link.iter().filter(|(_, value)| !value.is_empty())
                .map(|(kind, value)| (kind.as_str(), value.as_str())).collect::<Vec<_>>();
            cozo.replace_chain_vector_links(id, &pairs)?;
        }
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
