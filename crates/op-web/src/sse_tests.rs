use super::*;
use axum::response::IntoResponse;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture {
    state: tempfile::TempDir,
    catalog: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let state = tempfile::tempdir().unwrap();
        let catalog = tempfile::tempdir().unwrap();
        let one = op_state_store::PluginSchema::builder("one")
            .boolean_field("enabled", false, "Enabled flag")
            .integer_field("count", false, "Item count")
            .build()
            .fields;
        let removed = op_state_store::PluginSchema::builder("removed")
            .integer_field("value", false, "Prior child value")
            .build()
            .fields;
        let children = op_state_store::PluginSchema::builder("children")
            .object_field("one", one, false, "Current child")
            .object_field("removed", removed, false, "Removable child")
            .build()
            .fields;
        let schema = op_state_store::PluginSchema::builder("demo")
            .string_field("value", false, "Current value")
            .boolean_field("enabled", false, "Enabled flag")
            .object_field("children", children, false, "Typed child state")
            .public_state_paths(&["/value", "/enabled", "/children"])
            .build();
        op_blob::BlobStore::open(catalog.path())
            .unwrap()
            .write(&op_blob::blobify_plugin_schema("demo", schema))
            .unwrap();
        Self { state, catalog }
    }
    fn commit(&self, data: &[u8]) {
        op_core::projection_shm::update_projection_in(
            self.state.path().to_str().unwrap(),
            "demo",
            |_| Ok(Some(data.to_vec())),
        )
        .unwrap();
    }
    fn refresh(&self) -> impl FnMut() -> Refresh + Send + 'static + use<> {
        let state = self.state.path().to_str().unwrap().to_owned();
        let catalog = self.catalog.path().to_owned();
        move || {
            let state = state.clone();
            let catalog = catalog.clone();
            Box::pin(async move {
                hydration_from_tree(
                    crate::state_tree::read_committed_tree_from(state, catalog).await?,
                )
            })
        }
    }
}

async fn collected(
    stream: impl Stream<Item = std::result::Result<Event, Infallible>> + Send + 'static,
) -> Vec<(String, Value)> {
    let response = Sse::new(stream).into_response();
    let bytes = axum::body::to_bytes(response.into_body(), MAX_HYDRATION_BYTES * 2)
        .await
        .unwrap();
    std::str::from_utf8(&bytes)
        .unwrap()
        .split("\n\n")
        .filter_map(|block| {
            let event = block
                .lines()
                .find_map(|line| line.strip_prefix("event:"))
                .map(str::trim)?;
            let data = block
                .lines()
                .find_map(|line| line.strip_prefix("data:"))
                .map(str::trim)?;
            Some((event.to_owned(), serde_json::from_str(data).unwrap()))
        })
        .collect()
}

#[tokio::test]
async fn actual_sse_hydration_reads_shm_contract_then_whole_nested_state() {
    let fixture = Fixture::new();
    fixture.commit(br#"{"children":{"one":{"enabled":false,"count":0}}}"#);
    let (tx, rx) = broadcast::channel(4);
    drop(tx);
    let frames = collected(state_event_stream(rx, fixture.refresh())).await;
    let kinds: Vec<_> = frames.iter().map(|(kind, _)| kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "state_reset",
            "schema_update",
            "state_update",
            "state_ready"
        ]
    );
    assert_eq!(
        frames[2].1["new_value"]["children"]["one"]["enabled"],
        false
    );
    assert_eq!(frames[2].1["new_value"]["children"]["one"]["count"], 0);
    assert_eq!(frames[2].1["state_generation"], "1");
    assert_eq!(frames[2].1["replace"], true);
    assert_eq!(frames[1].1["schema_hash"], frames[2].1["schema_hash"]);
    assert_eq!(frames[0].1["catalog_hash"], frames[2].1["catalog_hash"]);
}

#[tokio::test]
async fn lag_rehydrates_from_current_shm_and_exposes_unreplayed_audit_gap() {
    let fixture = Fixture::new();
    fixture.commit(br#"{"children":{"removed":{"value":1}}}"#);
    let state = fixture.state.path().to_str().unwrap().to_owned();
    let catalog = fixture.catalog.path().to_owned();
    let (tx, rx) = broadcast::channel(1);
    let mut sender = Some(tx);
    let reads = Arc::new(AtomicUsize::new(0));
    let count = reads.clone();
    let refresh = move || -> Refresh {
        let state = state.clone();
        let catalog = catalog.clone();
        let first_sender = sender.take();
        let count = count.clone();
        Box::pin(async move {
            count.fetch_add(1, Ordering::SeqCst);
            let snapshot =
                crate::state_tree::read_committed_tree_from(state.clone(), catalog).await?;
            if let Some(sender) = first_sender {
                op_core::projection_shm::update_projection_in(&state, "demo", |_| {
                    Ok(Some(br#"{"children":{}}"#.to_vec()))
                })?;
                for value in 0..3 {
                    sender
                        .send(event("state_unavailable", json!({"test_wakeup":value}))?)
                        .unwrap();
                }
            }
            hydration_from_tree(snapshot)
        })
    };
    let frames = collected(state_event_stream(rx, refresh)).await;
    assert_eq!(reads.load(Ordering::SeqCst), 2);
    assert_eq!(
        frames
            .iter()
            .filter(|(kind, _)| kind == "state_reset")
            .count(),
        2
    );
    assert!(frames
        .iter()
        .any(|(kind, value)| kind == "stream_gap" && value["state"] == "rehydrating"));
    let latest = frames
        .iter()
        .rfind(|(kind, _)| kind == "state_update")
        .unwrap();
    assert_eq!(latest.1["new_value"], json!({"children":{}}));
    assert_eq!(latest.1["state_generation"], "2");
    assert_eq!(latest.1["replace"], true);
}

#[tokio::test]
async fn reconnect_clears_removed_plugin_even_if_its_old_file_remains() {
    let fixture = Fixture::new();
    fixture.commit(br#"{"value":"old"}"#);
    let (tx, rx) = broadcast::channel(1);
    drop(tx);
    let old = collected(state_event_stream(rx, fixture.refresh())).await;
    assert!(old.iter().any(|(kind, _)| kind == "state_update"));
    op_blob::BlobStore::open(fixture.catalog.path())
        .unwrap()
        .remove_blob("demo")
        .unwrap();
    let (tx, rx) = broadcast::channel(1);
    drop(tx);
    let current = collected(state_event_stream(rx, fixture.refresh())).await;
    assert_eq!(
        current
            .iter()
            .map(|(kind, _)| kind.as_str())
            .collect::<Vec<_>>(),
        ["state_reset", "state_ready"]
    );
    assert_ne!(old[0].1["catalog_hash"], current[0].1["catalog_hash"]);
    assert!(fixture.state.path().join("demo.json").exists());
}

#[tokio::test]
async fn removed_public_policy_clears_prior_values_but_retains_sealed_contract() {
    let fixture = Fixture::new();
    fixture.commit(br#"{"value":"formerly-visible"}"#);
    let (tx, rx) = broadcast::channel(1);
    drop(tx);
    let old = collected(state_event_stream(rx, fixture.refresh())).await;
    assert_eq!(old[2].1["new_value"]["value"], "formerly-visible");

    let tree = crate::state_tree::read_committed_tree_from(
        fixture.state.path().to_str().unwrap().to_owned(),
        fixture.catalog.path().to_owned(),
    )
    .await
    .unwrap();
    let mut schema = tree.plugins["demo"].schema.clone();
    schema.public_state.paths.clear();
    op_blob::BlobStore::open(fixture.catalog.path())
        .unwrap()
        .write(&op_blob::blobify_plugin_schema("demo", schema))
        .unwrap();
    let (tx, rx) = broadcast::channel(1);
    drop(tx);
    let current = collected(state_event_stream(rx, fixture.refresh())).await;
    assert_eq!(current[0].0, "state_reset");
    assert_eq!(current[1].0, "schema_update");
    assert_ne!(old[1].1["schema_hash"], current[1].1["schema_hash"]);
    assert_eq!(current[2].0, "state_update");
    assert_eq!(current[2].1["new_value"], Value::Null);
    assert_eq!(current[2].1["removed"], true);
    assert_eq!(current[2].1["state_generation"], "1");
    assert_eq!(current[3].0, "state_ready");
    assert!(!current
        .iter()
        .any(|(_, value)| value.to_string().contains("formerly-visible")));
}

#[tokio::test]
async fn incomplete_publication_never_replays_old_cached_or_partial_state() {
    let fixture = Fixture::new();
    fixture.commit(br#"{"value":"committed"}"#);
    std::fs::write(
        fixture.state.path().join(".manifest.json"),
        br#"{"generation":1,"publishing":true}"#,
    )
    .unwrap();
    std::fs::write(
        fixture.state.path().join("demo.json"),
        br#"{"value":"partial"}"#,
    )
    .unwrap();
    let (tx, rx) = broadcast::channel(1);
    drop(tx);
    let frames = collected(state_event_stream(rx, fixture.refresh())).await;
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].0, "state_unavailable");
    assert!(!frames[0].1.to_string().contains("partial"));
}

fn message() -> StateUpdateMessage {
    StateUpdateMessage {
        plugin_id: "demo".into(),
        object_path: "/org/opdbus/v1/plugins/demo".into(),
        property_name: None,
        new_value: Some(simd_json::json!({"enabled":false})),
        event_id: "0".into(),
        tags_touched: vec![],
        change_type: ChangeType::PropertySet as i32,
        frame_kind: StateFrameKind::Update as i32,
        schema_hash: "a".repeat(64),
        catalog_hash: "b".repeat(64),
        state_generation: u64::MAX,
    }
}

#[test]
fn metadata_retains_exact_u64_and_tombstones_keep_existing_payload_fields() {
    let mut message = message();
    let encoded = state_message_event(&message).unwrap();
    assert_eq!(encoded.event_type, "state_update");
    let value: Value = serde_json::from_str(&encoded.data).unwrap();
    assert_eq!(value["state_generation"], u64::MAX.to_string());
    assert_eq!(value["new_value"]["enabled"], false);
    assert_eq!(state_cursor(&encoded).unwrap().generation, u64::MAX);
    message.new_value = None;
    message.change_type = ChangeType::ObjectRemoved as i32;
    message.schema_hash.clear();
    let encoded = state_message_event(&message).unwrap();
    let value: Value = serde_json::from_str(&encoded.data).unwrap();
    assert_eq!(value["removed"], true);
    assert!(value["new_value"].is_null());
    message.change_type = ChangeType::SchemaMigration as i32;
    assert_eq!(
        state_message_event(&message).unwrap().event_type,
        "schema_update"
    );
    message.change_type = ChangeType::MethodCall as i32;
    assert!(state_message_event(&message).is_err());
}

#[tokio::test]
async fn shared_sse_cannot_forward_privileged_audit_metadata() {
    let broadcaster = SseEventBroadcaster::new();
    let mut receiver = broadcaster.subscribe();
    broadcaster.broadcast("audit_event", "{\"target\":\"private-target\"}");
    assert!(matches!(
        receiver.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
    broadcaster.broadcast("state_resync", "{}");
    assert_eq!(receiver.try_recv().unwrap().event_type, "state_resync");

    // Defend even against an internal sender bypassing the broadcaster wrapper.
    let fixture = Fixture::new();
    fixture.commit(br#"{"value":"public"}"#);
    let (sender, receiver) = broadcast::channel(4);
    sender
        .send(event("audit_event", json!({"target":"private-target"})).unwrap())
        .unwrap();
    drop(sender);
    let frames = collected(state_event_stream(receiver, fixture.refresh())).await;
    assert!(!frames.iter().any(|(kind, _)| kind == "audit_event"));
    assert!(frames
        .iter()
        .any(|(kind, value)| kind == "state_update" && value["new_value"]["value"] == "public"));
}
