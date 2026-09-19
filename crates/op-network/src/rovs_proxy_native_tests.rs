use super::*;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

static OVSDB_ENVIRONMENT: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct NativeOvsdbFixture {
    _directory: tempfile::TempDir,
    server: Child,
    socket: PathBuf,
}

impl Drop for NativeOvsdbFixture {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

async fn native_ovsdb_fixture() -> NativeOvsdbFixture {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let database = root.join("db");
    let socket = root.join("db.sock");
    let status = Command::new("/usr/bin/ovsdb-tool")
        .args([
            "create".to_owned(),
            database.display().to_string(),
            "/usr/share/openvswitch/vswitch.ovsschema".to_owned(),
        ])
        .env("OVS_RUNDIR", root)
        .env("OVS_DBDIR", root)
        .env("OVS_LOGDIR", root)
        .status()
        .unwrap();
    assert!(status.success(), "ovsdb-tool create failed");
    let server = Command::new("/usr/bin/ovsdb-server")
        .args([
            format!("--remote=punix:{}", socket.display()),
            format!("--pidfile={}", root.join("db.pid").display()),
            format!("--unixctl={}", root.join("db.ctl").display()),
            database.display().to_string(),
        ])
        .env("OVS_RUNDIR", root)
        .env("OVS_DBDIR", root)
        .env("OVS_LOGDIR", root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !socket.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("private OVSDB socket did not appear");
    let status = Command::new("/usr/bin/ovs-vsctl")
        .args([
            format!("--db=unix:{}", socket.display()),
            "--no-wait".to_owned(),
            "init".to_owned(),
        ])
        .env("OVS_RUNDIR", root)
        .env("OVS_DBDIR", root)
        .env("OVS_LOGDIR", root)
        .status()
        .unwrap();
    assert!(status.success(), "private OVSDB init failed");
    NativeOvsdbFixture {
        _directory: directory,
        server,
        socket,
    }
}

struct SocketEnvironment(Option<std::ffi::OsString>);

impl SocketEnvironment {
    fn set(socket: &Path) -> Self {
        let previous = std::env::var_os("OVSDB_SOCKET_ADDR");
        std::env::set_var("OVSDB_SOCKET_ADDR", format!("unix:{}", socket.display()));
        Self(previous)
    }
}

impl Drop for SocketEnvironment {
    fn drop(&mut self) {
        match self.0.take() {
            Some(value) => std::env::set_var("OVSDB_SOCKET_ADDR", value),
            None => std::env::remove_var("OVSDB_SOCKET_ADDR"),
        }
    }
}

#[tokio::test]
async fn native_owner_round_trip_covers_rovs_commands_and_ovsdb_bridge_methods() {
    let _environment_guard = OVSDB_ENVIRONMENT.lock().await;
    let fixture = native_ovsdb_fixture().await;
    let _socket = SocketEnvironment::set(&fixture.socket);
    let client = OvsdbDbusClient::new();

    let databases = client.list_dbs().await.unwrap();
    assert!(databases.iter().any(|name| name == "Open_vSwitch"));
    let schema = client.get_schema().await.unwrap();
    assert!(schema.get("tables").and_then(|v| v.get("Bridge")).is_some());
    assert_eq!(
        client
            .native_rpc("echo", serde_json::json!(["fixture", 7]))
            .await
            .unwrap(),
        serde_json::json!(["fixture", 7])
    );

    client.create_bridge("fixture-br").await.unwrap();
    assert!(client
        .list_bridges()
        .await
        .unwrap()
        .iter()
        .any(|name| name == "fixture-br"));

    client
        .add_port_with_type("fixture-br", "fixture-int", Some("internal"))
        .await
        .unwrap();
    assert!(client
        .list_bridge_ports("fixture-br")
        .await
        .unwrap()
        .iter()
        .any(|name| name == "fixture-int"));

    assert!(!client
        .ensure_internal_port("fixture-br", "fixture-int")
        .await
        .unwrap());
    assert!(client
        .ensure_internal_port("fixture-br", "fixture-int2")
        .await
        .unwrap());
    let ports = client.list_bridge_ports("fixture-br").await.unwrap();
    assert!(ports.iter().any(|name| name == "fixture-int2"));

    let selected = client
        .transact_simd(simd_json::json!([{
            "op": "select",
            "table": "Bridge",
            "where": [["name", "==", "fixture-br"]],
            "columns": ["name", "ports"]
        }]))
        .await
        .unwrap();
    assert_eq!(selected[0]["rows"][0]["name"], "fixture-br");

    // RFC 7047 lock sessions retain their native connections. A queued grant
    // is observed before unlock, and a stolen owner still alternates with an
    // unlock on its original connection.
    let first = client.lock("caller-a", "fixture-lock").await.unwrap();
    assert!(first.locked && !first.queued);
    let queued = client.lock("caller-b", "fixture-lock").await.unwrap();
    assert!(!queued.locked && queued.queued);
    assert!(client.lock("caller-b", "fixture-lock").await.is_err());
    client.unlock("caller-a", "fixture-lock").await.unwrap();
    let granted = client.unlock("caller-b", "fixture-lock").await.unwrap();
    assert!(granted.acquired_while_queued);

    client.lock("caller-a", "fixture-steal").await.unwrap();
    let stolen = client.steal("caller-b", "fixture-steal").await.unwrap();
    assert!(stolen.locked);
    let former = client.unlock("caller-a", "fixture-steal").await.unwrap();
    assert!(former.stolen_before_unlock);
    client.unlock("caller-b", "fixture-steal").await.unwrap();
    assert!(client.unlock("caller-b", "unknown-lock").await.is_err());

    // A subset V1 monitor forwards its declared request and materializes
    // complete current rows without replacing the canonical full topology.
    let mut monitor = client
        .start_protocol_monitor(
            "caller-a",
            "Open_vSwitch",
            "fixture-monitor",
            "monitor",
            serde_json::json!({
                "Bridge":[{"columns":["name","ports"]}]
            }),
        )
        .await
        .unwrap();
    assert_eq!(monitor.initial.method, "monitor");
    monitor.receiver.borrow_and_update();
    client.create_bridge("monitor-added").await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), monitor.receiver.changed())
        .await
        .unwrap()
        .unwrap();
    let protocol = match monitor.receiver.borrow_and_update().clone() {
        monitor::ProtocolObservation::Snapshot(snapshot) => snapshot,
        monitor::ProtocolObservation::Unavailable(reason) => panic!("monitor failed: {reason}"),
    };
    assert!(protocol.tables["Bridge"]
        .values()
        .any(|row| row["name"] == "monitor-added"));
    let cancel = client
        .cancel_monitor("caller-a", "fixture-monitor")
        .await
        .unwrap();
    assert_eq!(cancel, serde_json::json!({}));
    assert!(client
        .cancel_monitor("caller-a", "fixture-monitor")
        .await
        .is_err());

    // monitor_cond uses update2 semantics: an insert that begins matching is
    // materialized and a delete/condition exit removes the row.
    let mut conditional = client
        .start_protocol_monitor(
            "caller-a",
            "Open_vSwitch",
            "fixture-monitor-cond",
            "monitor_cond",
            serde_json::json!({
                "Bridge":[{
                    "columns":["name"],
                    "where":[["name","==","conditional-br"]]
                }]
            }),
        )
        .await
        .unwrap();
    assert!(conditional.initial.tables["Bridge"].is_empty());
    conditional.receiver.borrow_and_update();
    client.create_bridge("conditional-br").await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), conditional.receiver.changed())
        .await
        .unwrap()
        .unwrap();
    let inserted = match conditional.receiver.borrow_and_update().clone() {
        monitor::ProtocolObservation::Snapshot(snapshot) => snapshot,
        monitor::ProtocolObservation::Unavailable(reason) => {
            panic!("monitor_cond failed: {reason}")
        }
    };
    assert_eq!(inserted.tables["Bridge"].len(), 1);
    client.delete_bridge("conditional-br").await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), conditional.receiver.changed())
        .await
        .unwrap()
        .unwrap();
    let deleted = match conditional.receiver.borrow_and_update().clone() {
        monitor::ProtocolObservation::Snapshot(snapshot) => snapshot,
        monitor::ProtocolObservation::Unavailable(reason) => {
            panic!("monitor_cond failed: {reason}")
        }
    };
    assert!(deleted.tables["Bridge"].is_empty());
    client
        .cancel_monitor("caller-a", "fixture-monitor-cond")
        .await
        .unwrap();

    client
        .delete_port("fixture-br", "fixture-int2")
        .await
        .unwrap();
    assert!(!client
        .list_bridge_ports("fixture-br")
        .await
        .unwrap()
        .iter()
        .any(|name| name == "fixture-int2"));
    client.delete_bridge("fixture-br").await.unwrap();
    assert!(!client
        .list_bridges()
        .await
        .unwrap()
        .iter()
        .any(|name| name == "fixture-br"));
}
