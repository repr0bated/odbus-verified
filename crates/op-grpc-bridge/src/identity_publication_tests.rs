//! Real engine/core publication boundary; all filesystem paths are injected.
use super::*;
use op_state::StatePlugin;
use op_state_store::{ChainConfig, EventChain, FieldType, PluginSchema};
use simd_json::json;
use std::{
    future::Future,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::Poll,
};
use tokio::sync::{broadcast::error::TryRecvError, oneshot};

type NoCommit<T> = fn(Option<&[u8]>) -> Result<State<T>>;
type NoRecover<T> = fn() -> Result<State<T>>;

struct Fixture {
    _root: tempfile::TempDir,
    state: String,
    catalog: PathBuf,
    engine: MutationEngine,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state").to_str().unwrap().to_string();
        let catalog = root.path().join("blobs");
        seal(&catalog, &canonical_schema());
        Self {
            _root: root,
            state,
            catalog,
            engine: MutationEngine::new(
                Arc::new(tokio::sync::RwLock::new(EventChain::new(
                    ChainConfig::default(),
                ))),
                Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new()),
            ),
        }
    }
    async fn commit<T, F>(&self, id: &str, commit: F) -> Result<CredentialPublication<T>>
    where
        T: Send + 'static,
        F: FnOnce(Option<&[u8]>) -> Result<State<T>> + Send + 'static,
    {
        self.engine
            .run_identity_publication_in(
                self.state.clone(),
                self.catalog.clone(),
                nix::unistd::getegid().as_raw(),
                id.into(),
                Some(commit),
                None::<NoRecover<T>>,
            )
            .await
    }
    async fn recover<T, F>(&self, id: &str, recover: F) -> Result<CredentialPublication<T>>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<State<T>> + Send + 'static,
    {
        self.engine
            .run_identity_publication_in(
                self.state.clone(),
                self.catalog.clone(),
                nix::unistd::getegid().as_raw(),
                id.into(),
                None::<NoCommit<T>>,
                Some(recover),
            )
            .await
    }
    async fn seed(&self) {
        assert!(matches!(
            self.commit("seed", |_| Ok((Some(state(false)), ())))
                .await
                .unwrap(),
            CredentialPublication::Committed { .. }
        ));
    }
}
fn canonical_schema() -> PluginSchema {
    // Test fixtures use the actual plugin producer, exactly as the sealer and
    // build evidence do. Only deployment-owned public disclosure is customized.
    let mut schema = op_plugins::state_plugins::identity_sled::IdentitySledPlugin::new()
        .schema()
        .unwrap();
    schema.public_state.paths = vec!["/sleds/*/session_id".into(), "/sleds/*/active".into()];
    schema
}
fn seal(path: &std::path::Path, schema: &PluginSchema) {
    let mut store = op_blob::BlobStore::open(path).unwrap();
    store
        .write(&op_blob::blobify_plugin_schema(
            "identity_sled",
            schema.clone(),
        ))
        .unwrap();
}
fn state(active: bool) -> OwnedValue {
    let typed: op_plugins::state_plugins::identity_sled::IdentitySledState =
        simd_json::serde::from_owned_value(json!({"sleds":[{"session_id":"one","wireguard_pubkey":"fixture-key","active":active,
        "sealed_id":"sid1:fixture-private","principal_kind":"human",
        "instance":{"name":"one","status":"Stopped","type":"container", "config":{"opaque":"never-public"}}}],
        "events":[{"session_id":"one","seq":0,"kind":"fixture","subid":"","content":"private-event","created_at":1}]}))
        .unwrap();
    simd_json::serde::to_owned_value(typed).unwrap()
}
async fn next(receiver: &mut tokio::sync::broadcast::Receiver<StateChange>) -> StateChange {
    tokio::time::timeout(std::time::Duration::from_secs(5), receiver.recv())
        .await
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn absent_or_skewed_seal_rejects_before_pending_marker_and_durable_callback() {
    for skew in [
        "absent",
        "version",
        "missing_field",
        "field_type",
        "nested_field",
    ] {
        let fixture = Fixture::new();
        fixture.seed().await;
        let before = op_core::projection_shm::read_projection_snapshot_in(
            &fixture.state,
            &["identity_sled".into()],
        )
        .unwrap();
        let mut schema = canonical_schema();
        match skew {
            "absent" => {
                std::fs::remove_file(fixture.catalog.join(op_blob::catalog::MANIFEST_FILENAME))
                    .unwrap();
            }
            "version" => schema.version = "1.0.0".into(),
            "missing_field" => {
                assert!(schema.fields.remove("events").is_some());
            }
            "field_type" => {
                schema.fields.get_mut("events").unwrap().field_type = FieldType::String;
            }
            "nested_field" => {
                let FieldType::Array(item) =
                    &mut schema.fields.get_mut("sleds").unwrap().field_type
                else {
                    panic!("canonical sleds must be an array")
                };
                let FieldType::Object(fields) = item.as_mut() else {
                    panic!("canonical sled item must be a typed object")
                };
                let field = fields.get_mut("active").unwrap();
                field.read_only = !field.read_only;
            }
            _ => unreachable!(),
        }
        if skew != "absent" {
            seal(&fixture.catalog, &schema);
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut updates = fixture.engine.change_tx.subscribe();
        let result = fixture
            .commit("rejected-contract", move |_| {
                called.fetch_add(1, Ordering::SeqCst);
                Ok((Some(state(true)), ()))
            })
            .await;
        assert!(result.is_err(), "skew {skew} must be denied");
        assert_eq!(calls.load(Ordering::SeqCst), 0, "skew {skew}");
        assert!(
            op_core::projection_shm::pending_identity_publication_in(&fixture.state)
                .unwrap()
                .is_none(),
            "skew {skew} must not create a pending marker"
        );
        assert_eq!(
            fixture.engine.get_state("identity_sled").await,
            Some(state(false))
        );
        assert!(matches!(updates.try_recv(), Err(TryRecvError::Empty)));
        let after = op_core::projection_shm::read_projection_snapshot_in(
            &fixture.state,
            &["identity_sled".into()],
        )
        .unwrap();
        assert_eq!(after.generation, before.generation);
        assert_eq!(after.states, before.states);
    }
}

#[tokio::test]
async fn recovery_contract_mismatch_neither_runs_callback_nor_clears_pending() {
    let fixture = Fixture::new();
    fixture.seed().await;
    fixture
        .commit::<(), _>("needs-recovery", |_| anyhow::bail!("uncertain"))
        .await
        .unwrap();
    let mut schema = canonical_schema();
    schema.version = "1.0.0".into();
    seal(&fixture.catalog, &schema);
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let mut updates = fixture.engine.change_tx.subscribe();
    assert!(
        fixture
            .recover("needs-recovery", move || {
                called.fetch_add(1, Ordering::SeqCst);
                Ok((None, ()))
            })
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        op_core::projection_shm::pending_identity_publication_in(&fixture.state)
            .unwrap()
            .unwrap()
            .operation_id,
        "needs-recovery"
    );
    assert!(matches!(updates.try_recv(), Err(TryRecvError::Empty)));
    assert_eq!(
        fixture.engine.get_state("identity_sled").await,
        Some(state(false))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cancellation_while_waiting_for_publication_lock_does_not_drop_submitted_intent() {
    let fixture = Fixture::new();
    let guard = fixture.engine.state_publication.clone().lock_owned().await;
    let mut updates = fixture.engine.change_tx.subscribe();
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let mut operation = Box::pin(fixture.commit("waiting", move |_| {
        called.fetch_add(1, Ordering::SeqCst);
        Ok((Some(state(true)), ()))
    }));
    // The first poll submits the independently owned task, then waits for it.
    let polled = std::future::poll_fn(|cx| Poll::Ready(operation.as_mut().poll(cx))).await;
    assert!(matches!(polled, Poll::Pending));
    drop(operation);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    drop(guard);
    let update = next(&mut updates).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        update.new_value,
        json!({"sleds":[{"session_id":"one","active":true}]})
    );
    assert_eq!(
        fixture.engine.get_state("identity_sled").await,
        Some(state(true))
    );
    assert!(
        op_core::projection_shm::pending_identity_publication_in(&fixture.state)
            .unwrap()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cancellation_after_durable_work_keeps_callback_pair_mirror_and_wakeup_owned() {
    let fixture = Fixture::new();
    fixture.seed().await;
    let mut updates = fixture.engine.change_tx.subscribe();
    let (entered, inside) = oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let native = Arc::new(AtomicUsize::new(0));
    let applied = native.clone();
    let mut operation = Box::pin(fixture.commit("native-completed", move |_| {
        applied.store(1, Ordering::SeqCst);
        entered.send(()).unwrap();
        released.recv().unwrap();
        Ok((Some(state(true)), 1u64))
    }));
    assert!(
        std::future::poll_fn(|cx| Poll::Ready(operation.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    inside.await.unwrap();
    drop(operation);
    assert_eq!(native.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture.engine.get_state("identity_sled").await,
        Some(state(false))
    );
    assert!(matches!(updates.try_recv(), Err(TryRecvError::Empty)));
    release.send(()).unwrap();
    next(&mut updates).await;
    assert_eq!(
        fixture.engine.get_state("identity_sled").await,
        Some(state(true))
    );
    let private = op_core::projection_shm::read_credential_projection_bytes_in(
        &fixture.state,
        "identity_sled",
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&private).unwrap(),
        serde_json::from_str::<serde_json::Value>(&simd_json::to_string(&state(true)).unwrap())
            .unwrap()
    );
}

#[tokio::test]
async fn pending_durable_error_changes_neither_mirror_nor_outward_notification() {
    let fixture = Fixture::new();
    fixture.seed().await;
    let mut updates = fixture.engine.change_tx.subscribe();
    let result = fixture
        .commit::<(), _>("uncertain", |_| anyhow::bail!("native outcome uncertain"))
        .await
        .unwrap();
    assert!(matches!(
        result,
        CredentialPublication::Pending { durable: None, .. }
    ));
    assert_eq!(
        fixture.engine.get_state("identity_sled").await,
        Some(state(false))
    );
    assert!(matches!(updates.try_recv(), Err(TryRecvError::Empty)));
    assert!(
        op_core::projection_shm::read_credential_projection_bytes_in(
            &fixture.state,
            "identity_sled"
        )
        .is_err()
    );
}

#[tokio::test]
async fn exact_recovery_to_absence_removes_mirror_and_emits_deletion_wakeup() {
    let fixture = Fixture::new();
    fixture.seed().await;
    fixture
        .commit::<(), _>("delete-uncertain", |_| {
            anyhow::bail!("delete may have committed")
        })
        .await
        .unwrap();
    let mut updates = fixture.engine.change_tx.subscribe();
    let invoked = Arc::new(AtomicUsize::new(0));
    let wrong = invoked.clone();
    assert!(
        fixture
            .recover("wrong", move || {
                wrong.fetch_add(1, Ordering::SeqCst);
                Ok((None, ()))
            })
            .await
            .is_err()
    );
    assert_eq!(invoked.load(Ordering::SeqCst), 0);
    let receipt = fixture
        .recover("delete-uncertain", || Ok((None, ())))
        .await
        .unwrap();
    assert!(matches!(receipt, CredentialPublication::Committed { .. }));
    let update = next(&mut updates).await;
    assert_eq!(update.change_type, ChangeType::ObjectRemoved);
    assert_eq!(update.new_value, json!(null));
    assert!(fixture.engine.get_state("identity_sled").await.is_none());
    assert!(
        op_core::projection_shm::read_credential_projection_bytes_in(
            &fixture.state,
            "identity_sled"
        )
        .unwrap()
        .is_none()
    );
}

#[tokio::test]
async fn reseal_after_durable_callback_keeps_identity_pending_and_old_mirror_unadvertised() {
    let fixture = Fixture::new();
    fixture.seed().await;
    let catalog = fixture.catalog.clone();
    let mut updates = fixture.engine.change_tx.subscribe();
    let result = fixture
        .commit("schema-change", move |_| {
            let mut schema = canonical_schema();
            schema.version.push_str("-test-reseal");
            seal(&catalog, &schema);
            Ok((Some(state(true)), ()))
        })
        .await
        .unwrap();
    assert!(matches!(result, CredentialPublication::Pending { .. }));
    assert_eq!(
        fixture.engine.get_state("identity_sled").await,
        Some(state(false))
    );
    assert!(matches!(updates.try_recv(), Err(TryRecvError::Empty)));
    assert!(
        op_core::projection_shm::read_credential_projection_bytes_in(
            &fixture.state,
            "identity_sled"
        )
        .is_err()
    );
}

#[tokio::test]
async fn clone_shares_publication_cache_and_lazily_configured_owner_cells() {
    let fixture = Fixture::new();
    let clone = fixture.engine.clone();
    assert!(Arc::ptr_eq(
        &fixture.engine.state_publication,
        &clone.state_publication
    ));
    assert!(Arc::ptr_eq(&fixture.engine.state_cache, &clone.state_cache));
    assert!(Arc::ptr_eq(&fixture.engine.event_chain, &clone.event_chain));
    assert!(Arc::ptr_eq(
        &fixture.engine.observers_started,
        &clone.observers_started
    ));
    assert!(Arc::ptr_eq(&fixture.engine.sessions, &clone.sessions));
    fixture
        .engine
        .shared_socket_path
        .set("fixture-owned-path".into())
        .unwrap();
    assert_eq!(
        clone.shared_socket_path.get().map(String::as_str),
        Some("fixture-owned-path")
    );
    let guard = fixture.engine.state_publication.clone().lock_owned().await;
    assert!(clone.state_publication.try_lock().is_err());
    drop(guard);
    fixture.seed().await;
    assert_eq!(clone.get_state("identity_sled").await, Some(state(false)));
}

#[tokio::test]
async fn local_metadata_keeps_existing_identity_consumer_fields_and_private_tree_exact() {
    #[derive(serde::Deserialize)]
    struct View {
        sleds: Vec<op_identity::SessionIdentity>,
    }
    let fixture = Fixture::new();
    fixture.seed().await;
    let public = op_core::projection_shm::read_projection_snapshot_in(
        &fixture.state,
        &["identity_sled".into()],
    )
    .unwrap();
    let public = &public.states["identity_sled"];
    let private = op_core::projection_shm::read_credential_projection_bytes_in(
        &fixture.state,
        "identity_sled",
    )
    .unwrap()
    .unwrap();
    let mut expected: View = serde_json::from_slice(&private).unwrap();
    for row in &mut expected.sleds {
        row.sealed_id = None;
    }
    let actual: View = serde_json::from_slice(public).unwrap();
    assert_eq!(actual.sleds, expected.sleds);
    let public_json: serde_json::Value = serde_json::from_slice(public).unwrap();
    assert!(public_json.get("events").is_none());
    assert!(public_json["sleds"][0].get("instance").is_none());
    assert!(public_json["sleds"][0].get("sealed_id").is_none());
    let private_json: serde_json::Value = serde_json::from_slice(&private).unwrap();
    assert_eq!(
        private_json["sleds"][0]["sealed_id"],
        "sid1:fixture-private"
    );
    assert_eq!(
        private_json["sleds"][0]["instance"]["config"]["opaque"],
        "never-public"
    );
    assert_eq!(private_json["events"][0]["content"], "private-event");
}
