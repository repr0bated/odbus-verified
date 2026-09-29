//! Durable values for configuration/registry plugins. Plugin contracts remain
//! in their owning Rust modules and sealed blobs; this store holds only their
//! mutable domain data, never schemas, grants or credentials.
use anyhow::Result;
use op_cozo_store::{
    CozoError, CozoGraphShuttle, RuntimeDocumentCommit, RuntimeDocumentRecord,
    RuntimeDocumentUncertainPhase,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::{Mutex, OnceCell};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeDocument {
    pub revision: u64,
    pub document: Value,
}

impl From<RuntimeDocumentRecord> for RuntimeDocument {
    fn from(row: RuntimeDocumentRecord) -> Self {
        Self {
            revision: row.revision,
            document: row.document,
        }
    }
}

/// Downcastable failure after native commit submission. The candidate is not
/// a successful receipt and must not be blindly retried or published to SHM.
pub struct RuntimeDocumentUncertain {
    pub candidate: RuntimeDocument,
    pub phase: RuntimeDocumentUncertainPhase,
    pub cause: CozoError,
}

impl std::fmt::Display for RuntimeDocumentUncertain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Domain documents can contain opaque private values. Never echo them
        // or a native diagnostic that might contain serialized values here.
        write!(
            f,
            "runtime_document_uncertain: revision {} has an uncertain {:?} outcome; reconcile before publication or retry",
            self.candidate.revision, self.phase
        )
    }
}
impl std::error::Error for RuntimeDocumentUncertain {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // anyhow's alternate/debug formatting walks source chains. Native
        // diagnostics can contain query-bound values, so only trusted code
        // that explicitly inspects `cause` may access that detail.
        None
    }
}
impl std::fmt::Debug for RuntimeDocumentUncertain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeDocumentUncertain")
            .field("code", &"runtime_document_uncertain")
            .field("revision", &self.candidate.revision)
            .field("phase", &self.phase)
            .finish()
    }
}

/// A failure proven to occur before a durable commit was submitted.
pub struct RuntimeDocumentRejected {
    cause: anyhow::Error,
}

impl RuntimeDocumentRejected {
    pub(crate) fn new(cause: impl Into<anyhow::Error>) -> Self {
        Self {
            cause: cause.into(),
        }
    }

    pub fn into_cause(self) -> anyhow::Error {
        self.cause
    }
}

impl std::fmt::Display for RuntimeDocumentRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.cause.fmt(f)
    }
}

impl std::fmt::Debug for RuntimeDocumentRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeDocumentRejected")
            .field("code", &"runtime_document_rejected")
            .finish()
    }
}

impl std::error::Error for RuntimeDocumentRejected {}

fn committed_result<R>(result: RuntimeDocumentCommit, receipt: R) -> Result<(RuntimeDocument, R)> {
    match result {
        RuntimeDocumentCommit::Committed(row) => Ok((row.into(), receipt)),
        RuntimeDocumentCommit::Uncertain {
            candidate,
            phase,
            error,
        } => Err(RuntimeDocumentUncertain {
            candidate: candidate.into(),
            phase,
            cause: error,
        }
        .into()),
    }
}

pub struct RuntimeDocuments {
    store: Arc<CozoGraphShuttle>,
    writer: Arc<Mutex<()>>,
}

impl RuntimeDocuments {
    pub fn new(store: CozoGraphShuttle) -> Self {
        Self::from_shared(Arc::new(store))
    }

    /// Build a runtime-document view over an existing process-owned Cozo
    /// handle. This keeps domain adapters and native database operations on the
    /// same database instance.
    pub fn from_shared(store: Arc<CozoGraphShuttle>) -> Self {
        Self {
            store,
            writer: Arc::new(Mutex::new(())),
        }
    }

    pub async fn read(&self, plugin_id: &str) -> Result<Option<RuntimeDocument>> {
        let store = self.store.clone();
        let id = plugin_id.to_string();
        tokio::task::spawn_blocking(move || {
            let result = Self::read_sync(&store, &id);
            // Release the captured native handle before Tokio publishes the
            // task result. A caller may drop its owner and reopen immediately.
            drop(store);
            result
        })
        .await?
    }

    fn read_sync(store: &CozoGraphShuttle, id: &str) -> Result<Option<RuntimeDocument>> {
        Ok(store.read_runtime_document(id)?.map(Into::into))
    }

    /// Explicit recovery of an uncertain operation: return the actual row
    /// only after a successful persistence barrier, without rerunning its
    /// domain closure or inventing an earlier operation's domain receipt.
    pub async fn read_durable(&self, plugin_id: &str) -> Result<Option<RuntimeDocument>> {
        let store = self.store.clone();
        let id = plugin_id.to_string();
        tokio::task::spawn_blocking(move || {
            let result = store.read_runtime_document_durable(&id);
            drop(store);
            Ok::<_, anyhow::Error>(result?.map(Into::into))
        })
        .await?
    }

    /// Serializes domain validation and durable write together. An error in
    /// the closure leaves the prior document unchanged; no RAM success fallback.
    pub async fn update<F>(&self, plugin_id: &str, change: F) -> Result<RuntimeDocument>
    where
        F: FnOnce(Option<Value>) -> Result<Value> + Send + 'static,
    {
        self.update_with_result(plugin_id, move |old| change(old).map(|value| (value, ())))
            .await
            .map(|(row, ())| row)
    }

    /// Return a domain receipt computed once for the exact CAS-committed row
    /// (for example whether a deleted key actually existed). Persistent stores
    /// complete their barrier before success. Conflicts never rerun the closure;
    /// commit/barrier uncertainty returns RuntimeDocumentUncertain, not success.
    pub async fn update_with_result<F, R>(
        &self,
        plugin_id: &str,
        change: F,
    ) -> Result<(RuntimeDocument, R)>
    where
        F: FnOnce(Option<Value>) -> Result<(Value, R)> + Send + 'static,
        R: Send + 'static,
    {
        self.update_with_result_checked(plugin_id, None, change)
            .await
    }

    /// Apply a domain transition only at this exact durable document revision.
    /// Revision zero means absent. The check occurs inside the same owned
    /// writer/native CAS path, before invoking the domain closure.
    pub async fn update_at_revision_with_result<F, R>(
        &self,
        plugin_id: &str,
        expected_revision: u64,
        change: F,
    ) -> Result<(RuntimeDocument, R)>
    where
        F: FnOnce(Option<Value>) -> Result<(Value, R)> + Send + 'static,
        R: Send + 'static,
    {
        self.update_with_result_checked(plugin_id, Some(expected_revision), change)
            .await
    }

    async fn update_with_result_checked<F, R>(
        &self,
        plugin_id: &str,
        expected_revision: Option<u64>,
        change: F,
    ) -> Result<(RuntimeDocument, R)>
    where
        F: FnOnce(Option<Value>) -> Result<(Value, R)> + Send + 'static,
        R: Send + 'static,
    {
        let guard = self.writer.clone().lock_owned().await;
        let store = self.store.clone();
        let id = plugin_id.to_string();
        // Keep ownership of the writer guard in the task. Dropping a caller's
        // future must not let another writer read before this commit finishes.
        tokio::task::spawn_blocking(move || {
            let _guard = guard;
            let result = (|| {
                let previous =
                    Self::read_sync(&store, &id).map_err(RuntimeDocumentRejected::new)?;
                if let Some(expected_revision) = expected_revision {
                    if previous.as_ref().map_or(0, |row| row.revision) != expected_revision {
                        return Err(RuntimeDocumentRejected::new(anyhow::anyhow!(
                            "runtime_document_revision_conflict"
                        ))
                        .into());
                    }
                }
                let expected = previous.as_ref().map(|row| RuntimeDocumentRecord {
                    revision: row.revision,
                    document: row.document.clone(),
                });
                let (document, receipt) = change(previous.map(|row| row.document))
                    .map_err(RuntimeDocumentRejected::new)?;
                let result = store
                    .compare_exchange_runtime_document(&id, expected.as_ref(), document)
                    .map_err(RuntimeDocumentRejected::new)?;
                committed_result(result, receipt)
            })();
            drop(store);
            result
        })
        .await?
    }
}

static DOCUMENTS: OnceCell<Arc<RuntimeDocuments>> = OnceCell::const_new();

async fn open_runtime_documents(path: std::path::PathBuf) -> Result<Arc<RuntimeDocuments>> {
    // The SAME configured process owner holds private mutable values. Upgrade
    // only its held root/data directories before opening Cozo; never open a
    // second default store or mutate generic Cozo constructor permissions.
    let store = tokio::task::spawn_blocking(move || CozoGraphShuttle::new_persistent_private(path))
        .await??;
    Ok(Arc::new(RuntimeDocuments::new(store)))
}

/// A single persistent handle owned by the bridge process. Tests inject their
/// own store instead of changing global environment or touching production data.
pub async fn runtime_documents() -> Result<Arc<RuntimeDocuments>> {
    DOCUMENTS
        .get_or_try_init(|| async {
            let path = std::env::var_os("OP_PLUGIN_RUNTIME_COZO_PATH")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| "/var/lib/op-dbus/plugin-runtime-cozo".into());
            open_runtime_documents(path).await
        })
        .await
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn expected_revision_rejects_before_domain_closure_and_native_write() {
        let documents = RuntimeDocuments::new(CozoGraphShuttle::new_in_memory().unwrap());
        let (first, ()) = documents
            .update_at_revision_with_result("revision-fixture", 0, |_| {
                Ok((serde_json::json!({"value":1}), ()))
            })
            .await
            .unwrap();
        assert_eq!(first.revision, 1);
        let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = called.clone();
        let error = documents
            .update_at_revision_with_result("revision-fixture", 0, move |_| {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                Ok((serde_json::json!({"value":2}), ()))
            })
            .await
            .unwrap_err();
        assert_eq!(error.to_string(), "runtime_document_revision_conflict");
        assert!(error.downcast_ref::<RuntimeDocumentRejected>().is_some());
        assert!(!called.load(std::sync::atomic::Ordering::SeqCst));
        let actual = documents.read("revision-fixture").await.unwrap().unwrap();
        assert_eq!(actual.revision, 1);
        assert_eq!(actual.document, serde_json::json!({"value":1}));
    }
    use anyhow::anyhow;
    use serde_json::json;

    #[tokio::test]
    async fn committed_documents_survive_reopen_and_failed_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("runtime");
        {
            let store =
                RuntimeDocuments::new(CozoGraphShuttle::new_persistent(path.clone()).unwrap());
            let row = store
                .update("config", |_| Ok(json!({"configs":{"limit":3}})))
                .await
                .unwrap();
            assert_eq!(row.revision, 1);
            assert!(store
                .update("config", |_| Err(anyhow!("invalid setting")))
                .await
                .is_err());
            assert_eq!(
                store.read("config").await.unwrap().unwrap().document["configs"]["limit"],
                3
            );
            assert!(store.read("endpoint").await.unwrap().is_none());
            assert_eq!(
                Arc::strong_count(&store.store),
                1,
                "completed operations must release their native handles before returning"
            );
        }
        let store = RuntimeDocuments::new(CozoGraphShuttle::new_persistent(path).unwrap());
        let row = store.read("config").await.unwrap().unwrap();
        assert_eq!(row.revision, 1);
        assert_eq!(row.document, json!({"configs":{"limit":3}}));
    }

    #[tokio::test]
    async fn concurrent_updates_preserve_every_committed_change() {
        let store = Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let mut jobs = tokio::task::JoinSet::new();
        for _ in 0..16 {
            let store = store.clone();
            jobs.spawn(async move {
                store
                    .update("counter", |old| {
                        Ok(json!(old.and_then(|v| v.as_u64()).unwrap_or(0) + 1))
                    })
                    .await
            });
        }
        while let Some(job) = jobs.join_next().await {
            job.unwrap().unwrap();
        }
        let row = store.read("counter").await.unwrap().unwrap();
        assert_eq!(row.revision, 16);
        assert_eq!(row.document, 16);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn independent_wrappers_cannot_overwrite_the_same_revision_or_rerun_closures() {
        let cozo = Arc::new(CozoGraphShuttle::new_in_memory().unwrap());
        let first = Arc::new(RuntimeDocuments::from_shared(cozo.clone()));
        let second = Arc::new(RuntimeDocuments::from_shared(cozo));
        let both_read = Arc::new(std::sync::Barrier::new(2));
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut jobs = tokio::task::JoinSet::new();
        for (store, value) in [(first.clone(), 1), (second, 2)] {
            let both_read = both_read.clone();
            let calls = calls.clone();
            jobs.spawn(async move {
                store
                    .update_with_result("page", move |old| {
                        assert!(old.is_none());
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        both_read.wait();
                        Ok((json!({"page":value}), value))
                    })
                    .await
            });
        }
        let mut successes = vec![];
        let mut conflicts = 0;
        while let Some(job) = jobs.join_next().await {
            match job.unwrap() {
                Ok(receipt) => successes.push(receipt),
                Err(error) => {
                    assert!(error.to_string().contains("compare-exchange conflict"));
                    conflicts += 1;
                }
            }
        }
        assert_eq!(conflicts, 1);
        assert_eq!(successes.len(), 1);
        let (row, receipt) = successes.pop().unwrap();
        assert_eq!(row.revision, 1);
        assert_eq!(row.document["page"], receipt);
        assert_eq!(first.read("page").await.unwrap(), Some(row));
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cancelled_waiter_keeps_writer_owned_until_exact_receipt_is_committed() {
        let store = Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let writer = store.clone();
        let (entered_tx, entered) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let first = tokio::spawn(async move {
            writer
                .update_with_result("page", move |_| {
                    entered_tx.send(()).unwrap();
                    released
                        .recv_timeout(std::time::Duration::from_secs(10))
                        .unwrap();
                    Ok((json!({"count":1}), "first"))
                })
                .await
        });
        entered.await.unwrap();
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        assert!(store.writer.try_lock().is_err());
        let second_store = store.clone();
        let second = tokio::spawn(async move {
            second_store
                .update_with_result("page", |old| {
                    let count = old.unwrap()["count"].as_u64().unwrap();
                    Ok((json!({"count":count+1}), "second"))
                })
                .await
        });
        release.send(()).unwrap();
        let (row, receipt) = second.await.unwrap().unwrap();
        assert_eq!(row.revision, 2);
        assert_eq!(row.document, json!({"count":2}));
        assert_eq!(receipt, "second");
        assert_eq!(store.read("page").await.unwrap(), Some(row));
    }

    #[test]
    fn uncertain_backend_receipt_cannot_become_successful_domain_result() {
        let candidate = RuntimeDocumentRecord {
            revision: 7,
            document: json!({"private":"candidate"}),
        };
        let error = committed_result(
            RuntimeDocumentCommit::Uncertain {
                candidate: candidate.clone(),
                phase: RuntimeDocumentUncertainPhase::Persistence,
                error: CozoError::Other("private-native-bound-value".into()),
            },
            "domain receipt",
        )
        .unwrap_err();
        let uncertain = error.downcast_ref::<RuntimeDocumentUncertain>().unwrap();
        assert_eq!(uncertain.candidate, candidate.into());
        assert_eq!(uncertain.phase, RuntimeDocumentUncertainPhase::Persistence);
        assert!(!error.to_string().contains("private"));
        assert!(!error.to_string().contains("domain receipt"));
        for rendered in [
            format!("{error:?}"),
            format!("{error:#}"),
            format!("{uncertain:?}"),
        ] {
            assert!(rendered.contains("runtime_document_uncertain"));
            assert!(!rendered.contains("private"));
            assert!(!rendered.contains("candidate"));
            assert!(!rendered.contains("domain receipt"));
        }
    }

    #[tokio::test]
    async fn durable_recovery_preserves_visible_revision_without_reexecuting_update() {
        let dir = tempfile::tempdir().unwrap();
        let store = RuntimeDocuments::new(
            CozoGraphShuttle::new_persistent(dir.path().join("runtime")).unwrap(),
        );
        let (written, receipt) = store
            .update_with_result("page", |_| Ok((json!({"title":"one"}), 19)))
            .await
            .unwrap();
        assert_eq!(receipt, 19);
        assert_eq!(store.read_durable("page").await.unwrap(), Some(written));
        assert!(store.read_durable("missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn configured_owner_reopens_same_data_and_restricts_existing_public_directories() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("configured-runtime-owner");
        let before;
        {
            let store =
                RuntimeDocuments::new(CozoGraphShuttle::new_persistent(path.clone()).unwrap());
            before = store
                .update("existing-config", |_| {
                    Ok(json!({"kept":"exact-existing-data"}))
                })
                .await
                .unwrap();
        }
        for directory in [&path, &path.join("data")] {
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let store = open_runtime_documents(path.clone()).await.unwrap();
        assert_eq!(
            store.read_durable("existing-config").await.unwrap(),
            Some(before)
        );
        for directory in [&path, &path.join("data")] {
            assert_eq!(
                std::fs::metadata(directory).unwrap().permissions().mode() & 0o7777,
                0o700
            );
        }
        store
            .update("private-ui", |_| Ok(json!({"private":"principal-state"})))
            .await
            .unwrap();
        assert_eq!(store.read("private-ui").await.unwrap().unwrap().revision, 1);
    }
}
