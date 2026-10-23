use super::*;
use crate::mutation_engine::{ChangeSource, ChangeType, MutationEngine, StateChange};
use op_plugins::state_plugins::runtime_documents::RuntimeDocuments;
use op_state::StatePlugin;
use std::sync::Arc;

fn operation(plugin_id: &str, method: &str) -> OperationMetadata {
    OperationMetadata {
        operation_id: "fixture-operation".into(),
        event_id: 7,
        plugin_id: plugin_id.into(),
        method: method.into(),
    }
}
fn engine() -> MutationEngine {
    MutationEngine::new(
        Arc::new(tokio::sync::RwLock::new(op_state_store::EventChain::new(
            op_state_store::ChainConfig::default(),
        ))),
        Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new()),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn caller_drop_after_durable_commit_still_folds_child_into_shm_and_wakes_readers() {
    // This is the production task owner and actual Cozo/SHM publication path,
    // with injected temp paths. Authentication/admission are covered by the
    // transport and compiled-contract fixtures; this tests the accepted phase.
    let root = tempfile::tempdir().unwrap();
    let state_dir = root.path().join("state").to_str().unwrap().to_owned();
    let catalog = root.path().join("blobs");
    let schema = op_plugins::state_plugins::config::ConfigPlugin::default()
        .schema()
        .unwrap();
    let mut sealer = op_blob::BlobStore::open(&catalog).unwrap();
    sealer
        .write(&op_blob::blobify_plugin_schema("config", schema))
        .unwrap();
    op_core::projection_shm::update_projection_in(&state_dir, "config", |_| {
        Ok(Some(serde_json::to_vec(
            &serde_json::json!({"configs":{"child":{"value":1},"keep":7}}),
        )?))
    })
    .unwrap();
    let documents = Arc::new(RuntimeDocuments::new(
        op_cozo_store::CozoGraphShuttle::new_persistent_private(root.path().join("runtime"))
            .unwrap(),
    ));
    let engine = engine();
    let guard = engine.state_publication.clone().lock_owned().await;
    let mut updates = engine.change_tx.subscribe();
    let (committed_tx, committed) = tokio::sync::oneshot::channel();
    let worker_engine = engine.clone();
    let worker_documents = documents.clone();
    let worker_state = state_dir.clone();
    let task = spawn(operation("config", "set_value"), async move {
        let row = worker_documents
            .update("config", |_| {
                Ok(serde_json::json!({"configs":{"child":{"value":9},"keep":7}}))
            })
            .await?;
        committed_tx.send(row.revision).unwrap();
        worker_engine
            .commit_state_update_in(
                StateChange {
                    change_id: "fixture-child-commit".into(),
                    event_id: 7,
                    plugin_id: "config".into(),
                    object_path: "/org/opdbus/v1/plugins/config/configs/child".into(),
                    change_type: ChangeType::PropertySet,
                    member_name: Some("value".into()),
                    old_value: None,
                    new_value: simd_json::json!(9),
                    tags_touched: Vec::new(),
                    event_hash: String::new(),
                    timestamp: chrono::Utc::now(),
                    actor_id: "fixture".into(),
                    source: ChangeSource::Internal,
                },
                None,
                false,
                worker_state,
                catalog,
            )
            .await?;
        Ok(serde_json::json!({"revision":row.revision}))
    });
    let caller = tokio::spawn(task);
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(10), committed)
            .await
            .unwrap()
            .unwrap(),
        1
    );
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    // The durable write has completed, but the existing SHM worker has NOT
    // been submitted: it is blocked on the engine publication mutex. This
    // distinguishes task ownership from merely owning a submitted file write.
    assert!(matches!(
        updates.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    let before =
        op_core::projection_shm::read_projection_snapshot_in(&state_dir, &["config".into()])
            .unwrap();
    let before: serde_json::Value = serde_json::from_slice(&before.states["config"]).unwrap();
    assert_eq!(before["configs"]["child"]["value"], 1);
    drop(guard);
    let update = tokio::time::timeout(std::time::Duration::from_secs(5), updates.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(update.object_path, "/org/opdbus/v1/plugins/config");
    assert_eq!(update.change_type, ChangeType::PropertySet);
    let after =
        op_core::projection_shm::read_projection_snapshot_in(&state_dir, &["config".into()])
            .unwrap();
    let after: serde_json::Value = serde_json::from_slice(&after.states["config"]).unwrap();
    assert_eq!(
        after,
        serde_json::json!({"configs":{"child":{"value":9},"keep":7}})
    );
    assert_eq!(
        documents
            .read_durable("config")
            .await
            .unwrap()
            .unwrap()
            .document,
        after
    );
    assert_eq!(
        serde_json::to_value(engine.get_state("config").await.unwrap()).unwrap(),
        after
    );
    // Config's sealed default policy withholds opaque values: the wakeup is
    // not permission to put its private document onto the shared wire feed.
    assert_eq!(update.new_value, simd_json::json!(null));
}

#[derive(Clone, Default)]
struct CapturedLog {
    bytes: Arc<std::sync::Mutex<Vec<u8>>>,
    written: Arc<tokio::sync::Notify>,
}
impl std::io::Write for CapturedLog {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes.lock().unwrap().extend_from_slice(bytes);
        if bytes.contains(&b'\n') {
            self.written.notify_one();
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLog {
    type Writer = Self;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::test]
async fn abandoned_failure_remains_observable_without_private_diagnostics() {
    let log = CapturedLog::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(log.clone())
        .with_ansi(false)
        .without_time()
        .finish();
    let dispatch = tracing::Dispatch::new(subscriber);
    let (release, released) = tokio::sync::oneshot::channel();
    let task = tracing::dispatcher::with_default(&dispatch, || {
        spawn(operation("config", "set_value"), async move {
            released.await.unwrap();
            Err(anyhow::anyhow!(
                "PRIVATE_ARGUMENT_RESULT_CREDENTIAL_NATIVE_DETAIL"
            ))
        })
    });
    drop(task);
    release.send(()).unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), log.written.notified())
        .await
        .unwrap();
    let text = String::from_utf8(log.bytes.lock().unwrap().clone()).unwrap();
    assert!(text.contains("admitted_method_failed"));
    assert!(text.contains("fixture-operation"));
    assert!(text.contains("config"));
    assert!(text.contains("set_value"));
    assert!(!text.contains("PRIVATE_"));
}

#[tokio::test]
async fn nested_admitted_tasks_complete_without_a_global_execution_lock() {
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        spawn(operation("large_language_model", "stream_generate"), async {
            let target = spawn(operation("tched_router", "start_generation"), async {
                Ok(serde_json::json!({"job_id":"fixture"}))
            })
            .await??;
            Ok(serde_json::json!({"result":target}))
        })
        .await
        .unwrap()
        .unwrap()
    })
    .await
    .unwrap();
    assert_eq!(result["result"]["job_id"], "fixture");
}
