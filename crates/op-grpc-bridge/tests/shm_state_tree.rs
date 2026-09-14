//! MutationEngine publishes complete committed trees, including descendant changes.
use std::sync::Arc;

use op_grpc_bridge::{ChangeSource, ChangeType, MutationEngine};
use op_state::StatePlugin;
use op_state_store::PluginSchema;
use simd_json::{json, OwnedValue};
use tokio::sync::broadcast::error::TryRecvError;

mod support;

fn engine() -> MutationEngine {
    MutationEngine::new(
        Arc::new(tokio::sync::RwLock::new(op_state_store::EventChain::new(
            op_state_store::ChainConfig::default(),
        ))),
        Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new()),
    )
}

fn catalog() -> support::CatalogScope {
    // This fixture models two schema-declared children; the real Incus
    // adapter must publish its own schema's topology representation.
    let child = PluginSchema::builder("child")
        .string_field("status", false, "Observed status")
        .integer_field("pid", false, "Observed process ID")
        .build();
    let children = PluginSchema::builder("children")
        .object_field("one", child.fields.clone(), false, "First child")
        .object_field("two", child.fields, false, "Second child")
        .build();
    support::CatalogScope::new(&[PluginSchema::builder("incus")
        .object_field("instances", children.fields, false, "Observed children")
        .integer_field("keep", false, "Sibling value")
        .boolean_field("actual", false, "Observed flag")
        .boolean_field("committed", false, "Committed flag")
        .public_state_paths(&["/instances", "/keep", "/actual", "/committed"])
        .build()])
}

async fn change(
    engine: &MutationEngine,
    suffix: &str,
    kind: ChangeType,
    member: Option<&str>,
    value: OwnedValue,
) -> Result<op_grpc_bridge::mutation_engine::StateChange, String> {
    engine
        .process_authoritative_change(
            "incus".into(),
            format!("/org/opdbus/v1/plugins/incus{suffix}"),
            kind,
            member.map(str::to_string),
            None,
            value,
            vec!["state".into()],
            "fixture".into(),
            None,
            ChangeSource::Internal,
        )
        .await
}

#[tokio::test]
async fn children_merge_against_shm_and_parent_notifications_contain_the_complete_tree() {
    let _catalog = catalog();
    let first = engine();
    change(
        &first,
        "",
        ChangeType::PropertySet,
        None,
        json!({"instances":{"one":{"status":"up"},"two":{"status":"down","pid":3}},"keep":7}),
    )
    .await
    .unwrap();

    // A second engine has no RAM state: the fold must use the committed tree.
    let second = engine();
    let mut rx = second.change_tx().subscribe();
    change(
        &second,
        "/instances/two",
        ChangeType::PropertySet,
        Some("status"),
        json!("ready"),
    )
    .await
    .unwrap();
    let notification = rx.try_recv().unwrap();
    let state = second.get_public_state("incus").await.unwrap();
    assert_eq!(state["instances"]["one"]["status"], "up");
    assert_eq!(state["instances"]["two"], json!({"status":"ready","pid":3}));
    assert_eq!(state["keep"], 7);
    assert_eq!(notification.object_path, "/org/opdbus/v1/plugins/incus");
    assert_eq!(notification.new_value, state);

    change(
        &second,
        "/instances/two",
        ChangeType::PropertyDelete,
        Some("pid"),
        json!(null),
    )
    .await
    .unwrap();
    assert_eq!(
        second.get_public_state("incus").await.unwrap()["instances"]["two"],
        json!({"status":"ready"})
    );
    change(
        &second,
        "/instances/two",
        ChangeType::ObjectRemoved,
        None,
        json!(null),
    )
    .await
    .unwrap();
    assert_eq!(
        second.get_public_state("incus").await.unwrap(),
        json!({"instances":{"one":{"status":"up"}},"keep":7})
    );
    change(&second, "", ChangeType::ObjectRemoved, None, json!(null))
        .await
        .unwrap();
    assert!(second.get_public_state("incus").await.is_none());
    assert!(second.get_state("incus").await.is_none());
}

#[tokio::test]
async fn failed_commit_changes_neither_state_mirror_nor_state_notifications() {
    let catalog = catalog();
    let engine = engine();
    let original = json!({"instances":{"one":{"status":"up"}}});
    change(&engine, "", ChangeType::PropertySet, None, original.clone())
        .await
        .unwrap();
    let mut rx = engine.change_tx().subscribe();
    let state_path = catalog.root().join("state/incus.json");
    let manifest_path = catalog.root().join("state/.manifest.json");
    let original_bytes = std::fs::read(&state_path).unwrap();
    let original_manifest = std::fs::read(&manifest_path).unwrap();
    // An interrupted transaction may not be treated as committed state.
    std::fs::write(&manifest_path, br#"{"generation":1,"publishing":true}"#).unwrap();
    assert!(change(
        &engine,
        "/instances/one",
        ChangeType::PropertySet,
        Some("status"),
        json!("bad")
    )
    .await
    .is_err());
    assert_eq!(std::fs::read(&state_path).unwrap(), original_bytes);
    assert_eq!(engine.get_state("incus").await, Some(original));
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    assert!(engine.get_public_state("incus").await.is_none());
    std::fs::write(manifest_path, original_manifest).unwrap();
}

#[tokio::test]
async fn private_backend_fields_stay_in_shm_and_out_of_state_notifications_and_observation_audit() {
    let scope = catalog();
    let engine = engine();
    let mut updates = engine.change_tx().subscribe();
    let mut audit = engine.chain_tx().subscribe();
    change(
        &engine,
        "",
        ChangeType::PropertySet,
        None,
        json!({
            "instances": {"one": {"status": "up", "private_config": "child-secret"}},
            "private_config": "root-secret"
        }),
    )
    .await
    .unwrap();
    let expected = json!({"instances": {"one": {"status": "up"}}});
    assert_eq!(updates.try_recv().unwrap().new_value, expected);
    let event = audit.try_recv().unwrap();
    let serialized = serde_json::to_string(&event).unwrap();
    assert!(!serialized.contains("child-secret"));
    assert!(!serialized.contains("root-secret"));
    let bytes = std::fs::read(scope.root().join("state/incus.json")).unwrap();
    let private: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        private["instances"]["one"]["private_config"],
        "child-secret"
    );
    assert_eq!(private["private_config"], "root-secret");

    change(
        &engine,
        "/instances/one",
        ChangeType::PropertySet,
        Some("private_config"),
        json!("replacement-secret"),
    )
    .await
    .unwrap();
    let update = updates.try_recv().unwrap();
    assert_eq!(update.old_value, Some(expected.clone()));
    assert_eq!(update.new_value, expected);
    assert!(!serde_json::to_string(&audit.try_recv().unwrap())
        .unwrap()
        .contains("replacement-secret"));
}

#[tokio::test]
async fn method_arguments_and_unsealed_orphan_files_are_never_present_state() {
    let catalog = catalog();
    let engine = engine();
    change(
        &engine,
        "",
        ChangeType::PropertySet,
        None,
        json!({"actual":true}),
    )
    .await
    .unwrap();
    let mut rx = engine.change_tx().subscribe();
    change(
        &engine,
        "",
        ChangeType::MethodCall,
        Some("method"),
        json!({"argument":"not state"}),
    )
    .await
    .unwrap();
    assert_eq!(
        engine.get_public_state("incus").await.unwrap(),
        json!({"actual":true})
    );
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    std::fs::write(
        catalog.root().join("state/unsealed.json"),
        br#"{"secret":true}"#,
    )
    .unwrap();
    assert!(engine.get_public_state("unsealed").await.is_none());
    assert!(engine
        .process_authoritative_change(
            "unsealed".into(),
            "/org/opdbus/v1/plugins/unsealed".into(),
            ChangeType::PropertySet,
            None,
            None,
            json!({"new":true}),
            vec![],
            "fixture".into(),
            None,
            ChangeSource::Internal,
        )
        .await
        .is_err());
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
}

#[tokio::test]
async fn public_reads_ignore_a_prepared_but_uncommitted_working_mirror() {
    let _catalog = catalog();
    let engine = engine();
    change(
        &engine,
        "",
        ChangeType::PropertySet,
        None,
        json!({"committed":true}),
    )
    .await
    .unwrap();
    engine
        .update_state_cache("incus".into(), json!({"prepared":true}))
        .await;
    assert_eq!(
        engine.get_public_state("incus").await.unwrap(),
        json!({"committed":true})
    );
}

#[tokio::test]
async fn generic_identity_updates_cannot_bypass_the_durable_owner() {
    let catalog = support::CatalogScope::new(&[
        op_plugins::state_plugins::identity_sled::IdentitySledPlugin::new()
            .schema()
            .unwrap(),
    ]);
    let engine = engine();
    let mut changes = engine.change_tx().subscribe();
    let mut audit = engine.chain_tx().subscribe();
    for (suffix, kind, member, value) in [
        ("", ChangeType::PropertySet, None, json!({"sleds":[]})),
        (
            "/sleds/one",
            ChangeType::PropertySet,
            Some("active".into()),
            json!(false),
        ),
        ("", ChangeType::ObjectRemoved, None, json!(null)),
    ] {
        let error = engine
            .process_authoritative_change(
                "identity_sled".into(),
                format!("/org/opdbus/v1/plugins/identity_sled{suffix}"),
                kind,
                member,
                None,
                value,
                vec![],
                "fixture".into(),
                None,
                ChangeSource::Internal,
            )
            .await
            .unwrap_err();
        assert!(error.contains("durable intent owner"), "{error}");
    }
    assert!(!catalog.root().join("state/identity_sled.json").exists());
    assert!(!catalog
        .root()
        .join("state/.credentials/identity_sled.json")
        .exists());
    assert!(engine.get_state("identity_sled").await.is_none());
    assert!(matches!(changes.try_recv(), Err(TryRecvError::Empty)));
    assert!(matches!(audit.try_recv(), Err(TryRecvError::Empty)));
}
