//! Typed runtime-document CAS over the existing Cozo relation and owner.
//!
//! These are mutable domain values, never plugin schemas or credentials. A
//! persistent successful write includes a native filesystem barrier. Generic
//! queries retain their existing visibility semantics and are not upgraded to
//! crash-durable writes by this module. Raw queries bypass this typed owner.
use super::identity_transactions::commit_checked;
use super::*;
use cozo::MultiTransaction;

/// Native diagnostics can contain whole query-bound private documents. All
/// standard formatting is safe; only trusted diagnostic code that explicitly
/// accesses this concrete error may retrieve the underlying detail.
pub struct RuntimeDocumentFailure {
    code: &'static str,
    diagnostic: String,
}

impl RuntimeDocumentFailure {
    pub fn code(&self) -> &'static str {
        self.code
    }
    pub fn trusted_diagnostic(&self) -> &str {
        &self.diagnostic
    }
}
impl std::fmt::Display for RuntimeDocumentFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code)
    }
}
impl std::fmt::Debug for RuntimeDocumentFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeDocumentFailure")
            .field("code", &self.code)
            .finish()
    }
}
impl std::error::Error for RuntimeDocumentFailure {}

#[derive(Clone, PartialEq)]
pub struct RuntimeDocumentRecord {
    pub revision: u64,
    pub document: Value,
}

impl std::fmt::Debug for RuntimeDocumentRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeDocumentRecord")
            .field("revision", &self.revision)
            .field("document", &"<private>")
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeDocumentUncertainPhase {
    /// Commit was submitted; its result could not be established.
    Commit,
    /// Cozo reported a commit, but persistence could not be established.
    Persistence,
}

pub enum RuntimeDocumentCommit {
    /// Exact committed row, after the barrier for persistent stores. In-memory
    /// owners are explicitly ephemeral fixtures, never crash-durable stores.
    Committed(RuntimeDocumentRecord),
    /// An attempted row, NOT a successful receipt or permission to retry.
    /// Neither variant of uncertainty claims that the previous row survived.
    Uncertain {
        candidate: RuntimeDocumentRecord,
        phase: RuntimeDocumentUncertainPhase,
        error: CozoError,
    },
}

impl std::fmt::Debug for RuntimeDocumentCommit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Committed(row) => f.debug_tuple("Committed").field(row).finish(),
            Self::Uncertain {
                candidate, phase, ..
            } => f
                .debug_struct("Uncertain")
                .field("code", &"runtime_document_uncertain")
                .field("revision", &candidate.revision)
                .field("phase", phase)
                .finish(),
        }
    }
}

fn error(message: &str) -> CozoError {
    CozoError::Other(message.into())
}
fn native(code: &'static str, error: impl std::fmt::Display) -> CozoError {
    RuntimeDocumentFailure {
        code,
        diagnostic: error.to_string(),
    }
    .into()
}
fn params(plugin_id: &str) -> Params {
    BTreeMap::from([("id".into(), DataValue::Str(plugin_id.into()))])
}

fn read_row(
    tx: &MultiTransaction,
    plugin_id: &str,
) -> std::result::Result<Option<RuntimeDocumentRecord>, CozoError> {
    let rows = tx
        .run_script(
            "?[revision, document] := *plugin_runtime_documents{plugin_id, revision, document}, plugin_id = $id",
            params(plugin_id),
        )
        .map_err(|error| native("runtime_document_read_failed", error))?;
    let row = match rows.rows.as_slice() {
        [] => return Ok(None),
        [row] if row.len() == 2 => row,
        _ => return Err(error("corrupt runtime document row cardinality")),
    };
    let revision = match &row[0] {
        DataValue::Num(Num::Int(revision)) if *revision > 0 => *revision as u64,
        _ => return Err(error("corrupt runtime document revision")),
    };
    let DataValue::Str(document) = &row[1] else {
        return Err(error("corrupt runtime document encoding"));
    };
    Ok(Some(RuntimeDocumentRecord {
        revision,
        document: serde_json::from_str(document.as_str())
            .map_err(|error| native("runtime_document_decode_failed", error))?,
    }))
}

impl CozoGraphShuttle {
    /// Read the visible typed row. This does not claim a persistence barrier;
    /// use read_runtime_document_durable for explicit recovery/publication.
    pub fn read_runtime_document(
        &self,
        plugin_id: &str,
    ) -> std::result::Result<Option<RuntimeDocumentRecord>, CozoError> {
        let _guard = self
            .runtime_document_transaction
            .lock()
            .map_err(|_| error("runtime document owner is poisoned"))?;
        let tx = self.db.multi_transaction(false);
        let row = read_row(&tx, plugin_id)?;
        commit_checked(&tx).map_err(|error| native("runtime_document_commit_failed", error))?;
        Ok(row)
    }

    /// Explicitly recover a visible row under the same owner and persistence
    /// barrier as writes. Never reruns an earlier domain closure. An error
    /// yields no durable receipt; a generic visibility read is not recovery.
    pub fn read_runtime_document_durable(
        &self,
        plugin_id: &str,
    ) -> std::result::Result<Option<RuntimeDocumentRecord>, CozoError> {
        let _guard = self
            .runtime_document_transaction
            .lock()
            .map_err(|_| error("runtime document owner is poisoned"))?;
        self.runtime_document_preflight()?;
        let tx = self.db.multi_transaction(false);
        let row = read_row(&tx, plugin_id)?;
        commit_checked(&tx).map_err(|error| native("runtime_document_commit_failed", error))?;
        self.sync_runtime_document_store()?;
        Ok(row)
    }

    /// Compare the complete expected row in the same transaction that writes
    /// its checked next revision. The domain closure runs once in its adapter;
    /// conflicts never silently rerun it. This gate serializes independent
    /// adapters sharing this Cozo owner, not just clones of one adapter mutex.
    pub fn compare_exchange_runtime_document(
        &self,
        plugin_id: &str,
        expected: Option<&RuntimeDocumentRecord>,
        document: Value,
    ) -> std::result::Result<RuntimeDocumentCommit, CozoError> {
        let _guard = self
            .runtime_document_transaction
            .lock()
            .map_err(|_| error("runtime document owner is poisoned"))?;
        self.runtime_document_preflight()?;
        let tx = self.db.multi_transaction(true);
        let current = read_row(&tx, plugin_id)?;
        if current.as_ref() != expected {
            return Err(error("runtime document compare-exchange conflict"));
        }
        let revision = current
            .as_ref()
            .map_or(0, |row| row.revision)
            .checked_add(1)
            .filter(|revision| *revision <= i64::MAX as u64)
            .ok_or_else(|| error("runtime document revision exhausted"))?;
        let encoded = serde_json::to_string(&document)
            .map_err(|error| native("runtime_document_encode_failed", error))?;
        let candidate = RuntimeDocumentRecord { revision, document };
        tx.run_script(
            "?[plugin_id, revision, document] <- [[$id, $revision, $document]] :put plugin_runtime_documents {plugin_id => revision, document}",
            BTreeMap::from([
                ("id".into(), DataValue::Str(plugin_id.into())),
                ("revision".into(), dv_int(revision as i64)),
                ("document".into(), DataValue::Str(encoded.into())),
            ]),
        )
        .map_err(|error| native("runtime_document_write_failed", error))?;
        #[cfg(test)]
        tests::checkpoint("before_commit")?;
        // Inspect both channel and native replies; never call Cozo0.7.6's
        // convenience commit() which discards its inner native failure.
        if let Err(error) = commit_checked(&tx) {
            return Ok(RuntimeDocumentCommit::Uncertain {
                candidate,
                phase: RuntimeDocumentUncertainPhase::Commit,
                error: native("runtime_document_commit_failed", error),
            });
        }
        #[cfg(test)]
        if let Err(error) = tests::checkpoint("commit_reply") {
            return Ok(RuntimeDocumentCommit::Uncertain {
                candidate,
                phase: RuntimeDocumentUncertainPhase::Commit,
                error,
            });
        }
        if let Err(error) = self.sync_runtime_document_store() {
            return Ok(RuntimeDocumentCommit::Uncertain {
                candidate,
                phase: RuntimeDocumentUncertainPhase::Persistence,
                error,
            });
        }
        Ok(RuntimeDocumentCommit::Committed(candidate))
    }

    pub(crate) fn runtime_document_preflight(&self) -> std::result::Result<(), CozoError> {
        if let Some(directory) = &self.identity_directory {
            directory
                .require_default_layout()
                .map_err(|error| native("runtime_document_layout_failed", error))?;
        }
        Ok(())
    }

    pub(crate) fn sync_runtime_document_store(&self) -> std::result::Result<(), CozoError> {
        #[cfg(test)]
        tests::checkpoint("barrier")?;
        if let Some(directory) = &self.identity_directory {
            // Shared native primitive, held validated root/data fds; no file
            // scan, reopened default owner, vendor patch or SQLite. syncfs
            // flushes ENTIRE filesystems and can have substantial latency.
            directory
                .sync()
                .map_err(|error| native("runtime_document_barrier_failed", error))?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "runtime_document_transactions_tests.rs"]
mod tests;
