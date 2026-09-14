use super::*;
use tokio::net::UnixListener;

const ROOT: &str = "00000000-0000-0000-0000-000000000001";
const BRIDGE: &str = "00000000-0000-0000-0000-000000000002";
const PORT: &str = "00000000-0000-0000-0000-000000000003";
const INTERFACE: &str = "00000000-0000-0000-0000-000000000004";

fn initial() -> Value {
    json!({
        "Open_vSwitch":{(ROOT):{"new":{"bridges":["uuid",BRIDGE]}}},
        "Bridge":{(BRIDGE):{"new":{"name":"br-fixture","ports":["uuid",PORT],"datapath_type":"system","fail_mode":["set",[]],"stp_enable":false,"mcast_snooping_enable":true}}},
        "Port":{(PORT):{"new":{"name":"p-fixture","interfaces":["uuid",INTERFACE],"tag":["set",[]],"trunks":["set",[0,100]],"vlan_mode":["set",[]],"bond_mode":["set",[]]}}},
        "Interface":{(INTERFACE):{"new":{"name":"eth-fixture","type":"system","mac_in_use":["set",[]],"mac":["set",[]],"admin_state":"up","link_state":"down"}}}
    })
}

fn schema() -> Value {
    let tables = COLUMNS
        .iter()
        .map(|(table, columns)| {
            let mut definitions: serde_json::Map<String, Value> = columns
                .iter()
                .map(|column| ((*column).to_owned(), json!({"type":"string"})))
                .collect();
            // Server schema may contain opaque configuration; the request must
            // still exclude it before any values enter the native parser/logs.
            definitions.insert(
                "other_config".into(),
                json!({"type":{"key":"string","value":"string"}}),
            );
            definitions.insert(
                "options".into(),
                json!({"type":{"key":"string","value":"string"}}),
            );
            ((*table).to_owned(), json!({"columns":definitions}))
        })
        .collect::<serde_json::Map<_, _>>();
    json!({"name":"Open_vSwitch","version":"1.0.0","tables":tables})
}

struct Fixture {
    _directory: tempfile::TempDir,
    listener: UnixListener,
    peer: Connection,
    receiver: watch::Receiver<MonitorObservation>,
    monitor: String,
}

async fn receive(peer: &mut Connection) -> Message {
    tokio::time::timeout(Duration::from_secs(2), peer.recv_message())
        .await
        .unwrap()
        .unwrap()
}
async fn fixture(initial: Value) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("ovs.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let address = format!("unix:{}", socket.display());
    let connecting = tokio::spawn(async move { subscribe(&address, "Open_vSwitch").await });
    let (stream, _) = listener.accept().await.unwrap();
    let mut peer = Connection::new(Stream::Unix(stream));
    let Message::Request(request) = receive(&mut peer).await else {
        panic!("expected get_schema");
    };
    assert_eq!(request.method, "get_schema");
    assert_eq!(request.params, json!(["Open_vSwitch"]));
    // Echo during a handshake must not deadlock waiting for a response.
    peer.send_message(&Message::Request(Request::new(
        "echo",
        json!(["probe"]),
        99,
    )))
    .await
    .unwrap();
    let Message::Response(echo) = receive(&mut peer).await else {
        panic!("expected echo reply");
    };
    assert_eq!(echo.id, RpcId::Number(99));
    assert_eq!(echo.result, Some(json!(["probe"])));
    peer.send_message(&Message::Response(Response::success(
        request.id.unwrap(),
        schema(),
    )))
    .await
    .unwrap();
    let Message::Request(request) = receive(&mut peer).await else {
        panic!("expected monitor");
    };
    assert_eq!(request.method, "monitor");
    let requested = request.params[2].as_object().unwrap();
    assert_eq!(requested.len(), 4);
    for (table, columns) in COLUMNS {
        assert_eq!(requested[*table]["columns"], json!(columns));
        assert!(!requested[*table]["columns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| matches!(
                value.as_str(),
                Some("options" | "other_config" | "external_ids")
            )));
    }
    let monitor = request.params[1].as_str().unwrap().to_owned();
    peer.send_message(&Message::Response(Response::success(
        request.id.unwrap(),
        initial,
    )))
    .await
    .unwrap();
    let receiver = connecting.await.unwrap().unwrap();
    Fixture {
        _directory: directory,
        listener,
        peer,
        receiver,
        monitor,
    }
}

fn snapshot_value(receiver: &watch::Receiver<MonitorObservation>) -> Arc<OvsdbSnapshot> {
    match receiver.borrow().clone() {
        MonitorObservation::Snapshot(snapshot) => snapshot,
        MonitorObservation::Unavailable(reason) => panic!("unexpected failure: {reason}"),
    }
}

#[tokio::test]
async fn native_monitor_hydrates_then_coalesces_complete_rows_and_deletes() {
    let mut fixture = fixture(initial()).await;
    assert_eq!(snapshot_value(&fixture.receiver).sequence, 1);
    let interface: Uuid = INTERFACE.parse().unwrap();
    for state in ["up", "down", "up"] {
        fixture.peer.send_message(&Message::Request(Request::notification("update", json!([
            fixture.monitor, {"Interface":{(INTERFACE):{"old":{"link_state":"unused"},"new":{"link_state":state}}}}
        ])))).await.unwrap();
    }
    fixture
        .peer
        .send_message(&Message::Request(Request::new("echo", json!([]), 100)))
        .await
        .unwrap();
    assert!(matches!(
        receive(&mut fixture.peer).await,
        Message::Response(_)
    ));
    // Receiver never consumed deltas; it reads the latest complete IDL view.
    let latest = snapshot_value(&fixture.receiver);
    assert_eq!(latest.sequence, 4);
    assert_eq!(
        latest.tables["Interface"][&interface]["name"],
        "eth-fixture"
    );
    assert_eq!(latest.tables["Interface"][&interface]["link_state"], "up");
    fixture.receiver.borrow_and_update();
    fixture
        .peer
        .send_message(&Message::Request(Request::notification(
            "update",
            json!([
                fixture.monitor, {
                    "Bridge": {(BRIDGE):{"old":{"ports":["uuid",PORT]},"new":{"ports":["set",[]]}}},
                    "Port": {(PORT):{"old":{}}}, "Interface": {(INTERFACE):{"old":{}}}
                }
            ]),
        )))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), fixture.receiver.changed())
        .await
        .unwrap()
        .unwrap();
    let latest = snapshot_value(&fixture.receiver);
    assert!(latest.tables["Port"].is_empty());
    assert!(latest.tables["Interface"].is_empty());
    assert_eq!(
        latest.tables["Bridge"][&BRIDGE.parse::<Uuid>().unwrap()]["ports"],
        json!(["set", []])
    );
}

#[tokio::test]
async fn idle_has_no_poll_and_last_receiver_drop_closes_native_connection() {
    let fixture = fixture(initial()).await;
    let Fixture {
        mut peer, receiver, ..
    } = fixture;
    assert!(
        tokio::time::timeout(Duration::from_millis(30), peer.recv_message())
            .await
            .is_err()
    );
    drop(receiver);
    assert!(
        tokio::time::timeout(Duration::from_secs(2), peer.recv_message())
            .await
            .unwrap()
            .is_err()
    );
}

#[tokio::test]
async fn disconnect_is_terminal_unavailable_not_empty_snapshot_or_reconnect() {
    let fixture = fixture(initial()).await;
    let Fixture {
        peer,
        mut receiver,
        listener,
        ..
    } = fixture;
    drop(peer);
    tokio::time::timeout(Duration::from_secs(2), receiver.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        &*receiver.borrow_and_update(),
        MonitorObservation::Unavailable(_)
    ));
    assert!(receiver.changed().await.is_err());
    assert!(
        tokio::time::timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn wrong_subscription_unknown_method_and_wrong_protocol_terminate_without_state() {
    for (method, id) in [
        ("update", "wrong"),
        ("update2", "actual"),
        ("monitor_canceled", "actual"),
    ] {
        let mut fixture = fixture(initial()).await;
        let id = if id == "actual" {
            fixture.monitor.clone()
        } else {
            id.to_owned()
        };
        fixture
            .peer
            .send_message(&Message::Request(Request::notification(
                method,
                json!([id, {}]),
            )))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), fixture.receiver.changed())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(
            &*fixture.receiver.borrow(),
            MonitorObservation::Unavailable(_)
        ));
    }
}

#[tokio::test]
async fn malformed_and_unsolicited_response_never_replace_snapshot_with_empty_data() {
    let mut fixture = fixture(initial()).await;
    fixture
        .peer
        .send_message(&Message::Response(Response::success(500u64, json!({}))))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), fixture.receiver.changed())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        &*fixture.receiver.borrow(),
        MonitorObservation::Unavailable(_)
    ));
    assert!(subscribe("tcp:127.0.0.1:1", "Open_vSwitch").await.is_err());
    assert!(subscribe("unix:/unused", "another_database").await.is_err());
}

#[test]
fn malformed_rows_private_columns_and_size_limits_are_rejected() {
    assert!(validate_update(&json!({"SSL":{}}), true).is_err());
    assert!(validate_update(&json!({"Bridge":{"bad-id":{"new":{}}}}), true).is_err());
    assert!(validate_update(&json!({"Bridge":{(BRIDGE):{"new":{"name":"only"}}}}), true).is_err());
    assert!(validate_update(
        &json!({"Bridge":{(BRIDGE):{"old":{},"new":{"other_config":"private"}}}}),
        false
    )
    .is_err());
    assert!(bounded_json(&"x".repeat(MAX_JSON_BYTES)).is_err());
    let rows: serde_json::Map<_, _> = (0..=MAX_SNAPSHOT_ROWS)
        .map(|index| {
            (
                Uuid::from_u128(index as u128).to_string(),
                json!({"old":{}}),
            )
        })
        .collect();
    assert!(validate_update(&json!({"Bridge":rows}), false).is_err());
    let mut idl = Idl::new();
    idl.set_schema(DbSchema::from_json(&schema()).unwrap());
    let rows: serde_json::Map<_, _> = (0..=MAX_SNAPSHOT_ROWS)
        .map(|index| {
            (
                Uuid::from_u128(index as u128).to_string(),
                json!({"new":{"bridges":["set",[]]}}),
            )
        })
        .collect();
    idl.process_update(&json!({"Open_vSwitch":rows}));
    assert!(snapshot(&idl, 1).is_err());
}
