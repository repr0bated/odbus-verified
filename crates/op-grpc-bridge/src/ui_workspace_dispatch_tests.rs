use super::*;
use op_state::StatePlugin;
use op_state_store::{ChainConfig, EventChain};
use serde_json::json;
use std::{future::Future, task::Poll};
use tokio::sync::broadcast::error::TryRecvError;

struct Fixture {
    _root: tempfile::TempDir,
    catalog: std::path::PathBuf,
    publisher: UiWorkspacePublisher,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let catalog = root.path().join("catalog");
        let mut sealer = op_blob::BlobStore::open(&catalog).unwrap();
        for schema in [
            op_plugins::state_plugins::gemma_brain::GemmaBrainPlugin::new()
                .schema()
                .unwrap(),
            op_plugins::state_plugins::json_render::JsonRenderPlugin::new()
                .schema()
                .unwrap(),
        ] {
            sealer
                .write(&op_blob::blobify_plugin_schema(
                    &schema.name.clone(),
                    schema,
                ))
                .unwrap();
        }
        let documents = Arc::new(RuntimeDocuments::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let private = PrivateProjectionDirectory::prepare(&root.path().join("private")).unwrap();
        Self {
            _root: root,
            catalog: catalog.clone(),
            publisher: UiWorkspacePublisher::new(private, documents, catalog),
        }
    }

    fn owner(principal: &str, session: &str) -> VerifiedOwner {
        let access = UiAccessContext {
            principal_id: principal.into(),
            session_id: session.into(),
            session_genesis: None,
        };
        let key = access.owner_key().unwrap();
        VerifiedOwner { access, key }
    }

    fn prepared(&self) -> (ContractPins, Arc<UiWorkspaceMethods>) {
        let pins = pin_contracts(&self.catalog, None).unwrap();
        let methods = self.publisher.methods(&pins).unwrap();
        (pins, methods)
    }

    async fn publish(
        &self,
        slot: Arc<OwnerSlot>,
        owner: VerifiedOwner,
        method: &str,
        args: Value,
        recovery: bool,
    ) -> Result<Value> {
        let (pins, methods) = self.prepared();
        self.publisher
            .publish_owned(
                slot,
                owner,
                pins,
                methods,
                method.into(),
                args,
                uuid::Uuid::new_v4().to_string(),
                recovery,
            )
            .await
    }
}

fn spec(text: &str) -> Value {
    json!({"root":"root","elements":{
        "root":{"type":"text","props":{"text":text,"className":null},"children":[]}
    }})
}

#[tokio::test(flavor = "multi_thread")]
async fn private_workspace_publication_is_complete_scoped_recoverable_and_cancellation_owned() {
    let fixture = Fixture::new();
    let engine = MutationEngine::new(
        Arc::new(tokio::sync::RwLock::new(EventChain::new(
            ChainConfig::default(),
        ))),
        Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new()),
    );
    assert!(fixture
        .publisher
        .subscribe_workspace(
            &engine,
            "principal-a",
            Some("session-a"),
            Some("genesis-a"),
            "gemma.invoke",
        )
        .await
        .err()
        .unwrap()
        .to_string()
        .contains("exact gemma.read"));
    let owner = Fixture::owner("principal-a", "session-a");
    let slot = fixture.publisher.owner_slot(&owner.key).unwrap();
    let mut wakes = slot.wake.subscribe();

    // Explicit activation reconciles an absent durable row as absent. It does
    // not mint an empty workspace or send a state wake.
    let activated = fixture
        .publish(slot.clone(), owner, "recover_ui_workspace", json!({}), true)
        .await
        .unwrap();
    assert_eq!(activated, json!({"workspace":null}));
    assert!(slot.activated.load(Ordering::Acquire));
    assert!(matches!(wakes.try_recv(), Err(TryRecvError::Empty)));

    let owner = Fixture::owner("principal-a", "session-a");
    fixture
        .publish(
            slot.clone(),
            owner,
            "create_ui_spec",
            json!({"spec":spec("one"),"tags":[]}),
            false,
        )
        .await
        .unwrap();
    wakes.recv().await.unwrap();
    let owner = Fixture::owner("principal-a", "session-a");
    fixture
        .publish(
            slot.clone(),
            owner,
            "create_ui_spec",
            json!({"spec":spec("PRIVATE_SIBLING"),"tags":[]}),
            false,
        )
        .await
        .unwrap();
    let second = wakes.recv().await.unwrap();

    let (pins, methods) = fixture.prepared();
    let key = Fixture::owner("principal-a", "session-a").key;
    let complete = fixture
        .publisher
        .read_committed_locked(&key, &pins, &methods, "get_ui_workspace", &json!({}))
        .await
        .unwrap();
    let workspace = &complete["workspace"];
    assert_eq!(workspace["document_revision"], 2);
    assert_eq!(
        workspace["projection"]["specs"].as_object().unwrap().len(),
        2
    );
    assert_eq!(second.document_revision, 2);

    let child = fixture
        .publisher
        .private_root
        .open_child(key.namespace_component())
        .unwrap()
        .unwrap();
    let snapshot = child.read_snapshot(&[PLUGIN.to_owned()]).unwrap();
    let prior = snapshot.states[PLUGIN].clone();
    let mut unequal: Value = serde_json::from_slice(&prior).unwrap();
    unequal["specs"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap()["tags"] = json!(["changed-without-revision"]);
    let unequal = serde_json::to_vec(&unequal).unwrap();
    assert!(compare_revision(Some(&prior), &unequal, &pins.gemma_schema).is_err());

    // A proven pre-effect rejection clears its exact marker. The previous
    // projection remains readable at the same generation and emits no wake.
    let generation = snapshot.generation;
    let owner = Fixture::owner("principal-a", "session-a");
    let rejected = fixture
        .publish(
            slot.clone(),
            owner,
            "update_ui_spec",
            json!({"spec_id":"absent","expected_revision":1,"element_id":"root","properties":{}}),
            false,
        )
        .await
        .unwrap_err();
    assert!(format!("{rejected:#}").contains("UI spec not found"));
    assert!(slot.activated.load(Ordering::Acquire));
    assert!(matches!(wakes.try_recv(), Err(TryRecvError::Empty)));
    assert!(child.pending_document().unwrap().is_none());
    let after_rejection = child.read_snapshot(&[PLUGIN.to_owned()]).unwrap();
    assert_eq!(after_rejection.generation, generation);
    assert_eq!(after_rejection.states[PLUGIN], prior);
    let still_committed = fixture
        .publisher
        .read_committed_locked(&key, &pins, &methods, "get_ui_workspace", &json!({}))
        .await
        .unwrap();
    assert_eq!(still_committed["workspace"]["document_revision"], 2);

    // A successful durable mutation followed by a pinned-contract change is
    // genuinely uncertain for publication: it retains the gate and receipt,
    // deactivates reads, and emits no wake until exact durable recovery.
    let (stale_pins, stale_methods) = fixture.prepared();
    let mut changed = op_plugins::state_plugins::gemma_brain::GemmaBrainPlugin::new()
        .schema()
        .unwrap();
    changed
        .description
        .push_str("; changed after the durable operation was prepared");
    op_blob::BlobStore::open(&fixture.catalog)
        .unwrap()
        .write(&op_blob::blobify_plugin_schema(PLUGIN, changed))
        .unwrap();
    let pending = fixture
        .publisher
        .publish_owned(
            slot.clone(),
            Fixture::owner("principal-a", "session-a"),
            stale_pins,
            stale_methods,
            "create_ui_spec".into(),
            json!({"spec":spec("durable-before-contract-change"),"tags":[]}),
            uuid::Uuid::new_v4().to_string(),
            false,
        )
        .await
        .unwrap_err();
    assert!(format!("{pending:#}").contains("committed_unpublished"));
    assert!(!slot.activated.load(Ordering::Acquire));
    assert!(matches!(wakes.try_recv(), Err(TryRecvError::Empty)));
    assert!(child.pending_document().unwrap().is_some());

    // Restore the compiled contract, then exact recovery rereads the durable
    // row, clears the gate and emits the complete workspace as one generation.
    let canonical = op_plugins::state_plugins::gemma_brain::GemmaBrainPlugin::new()
        .schema()
        .unwrap();
    op_blob::BlobStore::open(&fixture.catalog)
        .unwrap()
        .write(&op_blob::blobify_plugin_schema(PLUGIN, canonical))
        .unwrap();

    let owner = Fixture::owner("principal-a", "session-a");
    fixture
        .publish(slot.clone(), owner, "recover_ui_workspace", json!({}), true)
        .await
        .unwrap();
    assert!(slot.activated.load(Ordering::Acquire));
    let recovered = wakes.recv().await.unwrap();
    assert_eq!(recovered.document_revision, 3);
    assert!(child.pending_document().unwrap().is_none());
    let (pins, methods) = fixture.prepared();
    let recovered = fixture
        .publisher
        .read_committed_locked(&key, &pins, &methods, "get_ui_workspace", &json!({}))
        .await
        .unwrap();
    assert_eq!(recovered["workspace"]["document_revision"], 3);
    assert_eq!(
        recovered["workspace"]["projection"]["specs"]
            .as_object()
            .unwrap()
            .len(),
        3
    );

    // Another verified owner maps through the plugin-owned opaque key and
    // cannot see this workspace after its own explicit absent activation.
    let other = Fixture::owner("principal-a", "session-b");
    let other_slot = fixture.publisher.owner_slot(&other.key).unwrap();
    fixture
        .publish(
            other_slot.clone(),
            other,
            "recover_ui_workspace",
            json!({}),
            true,
        )
        .await
        .unwrap();
    let (pins, methods) = fixture.prepared();
    let other_key = Fixture::owner("principal-a", "session-b").key;
    let absent = fixture
        .publisher
        .read_committed_locked(&other_key, &pins, &methods, "get_ui_workspace", &json!({}))
        .await
        .unwrap();
    assert_eq!(absent, json!({"workspace":null}));

    // Once submitted, caller cancellation cannot strand a domain success
    // before SHM publication: the independently owned worker waits for the
    // owner lock and completes after the caller future is dropped.
    let cancel_owner = Fixture::owner("principal-c", "session-c");
    let cancel_slot = fixture.publisher.owner_slot(&cancel_owner.key).unwrap();
    fixture
        .publish(
            cancel_slot.clone(),
            cancel_owner,
            "recover_ui_workspace",
            json!({}),
            true,
        )
        .await
        .unwrap();
    let guard = cancel_slot.operation.clone().lock_owned().await;
    let mut cancel_wakes = cancel_slot.wake.subscribe();
    let (pins, methods) = fixture.prepared();
    let mut operation = Box::pin(fixture.publisher.publish_owned(
        cancel_slot.clone(),
        Fixture::owner("principal-c", "session-c"),
        pins,
        methods,
        "create_ui_spec".into(),
        json!({"spec":spec("owned"),"tags":[]}),
        uuid::Uuid::new_v4().to_string(),
        false,
    ));
    let poll = std::future::poll_fn(|cx| Poll::Ready(operation.as_mut().poll(cx))).await;
    assert!(matches!(poll, Poll::Pending));
    drop(operation);
    drop(guard);
    tokio::time::timeout(std::time::Duration::from_secs(5), cancel_wakes.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(cancel_slot.activated.load(Ordering::Acquire));
    let (pins, methods) = fixture.prepared();
    let workspace = fixture
        .publisher
        .read_committed_locked(
            &Fixture::owner("principal-c", "session-c").key,
            &pins,
            &methods,
            "get_ui_workspace",
            &json!({}),
        )
        .await
        .unwrap();
    let specs = workspace["workspace"]["projection"]["specs"]
        .as_object()
        .unwrap();
    assert_eq!(specs.len(), 1);
    let (minted_id, record) = specs.iter().next().unwrap();
    assert_eq!(
        uuid::Uuid::parse_str(minted_id).unwrap().to_string(),
        *minted_id
    );
    assert_eq!(record["id"].as_str(), Some(minted_id.as_str()));
    assert_eq!(record["spec"]["elements"]["root"]["props"]["text"], "owned");
    let committed = fixture
        .publisher
        .read_committed_locked(
            &Fixture::owner("principal-c", "session-c").key,
            &pins,
            &methods,
            "get_ui_spec",
            &json!({"spec_id":minted_id}),
        )
        .await
        .unwrap();
    assert_eq!(committed["spec"]["id"].as_str(), Some(minted_id.as_str()));
}
