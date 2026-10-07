//! Real native OVSDB effects must return the sealed acknowledgment contract.
//! The database/socket are private temporary fixtures; no live ports are changed.
use super::*;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct NativeFixture {
    server: Child,
    previous_socket: Option<std::ffi::OsString>,
    _directory: tempfile::TempDir,
}

impl Drop for NativeFixture {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
        match self.previous_socket.take() {
            Some(value) => unsafe { std::env::set_var("OVSDB_SOCKET_ADDR", value) },
            None => unsafe { std::env::remove_var("OVSDB_SOCKET_ADDR") },
        }
    }
}

async fn native_fixture() -> NativeFixture {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let database = root.join("db");
    let socket = root.join("db.sock");
    let created = Command::new("/usr/bin/ovsdb-tool")
        .args([
            "create".into(),
            database.display().to_string(),
            "/usr/share/openvswitch/vswitch.ovsschema".into(),
        ])
        .status()
        .unwrap();
    assert!(created.success(), "private OVSDB creation failed");
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
    let previous_socket = std::env::var_os("OVSDB_SOCKET_ADDR");
    let fixture = NativeFixture {
        server,
        previous_socket,
        _directory: directory,
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        while !socket.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("private OVSDB socket absent");
    let initialized = Command::new("/usr/bin/ovs-vsctl")
        .args([
            format!("--db=unix:{}", socket.display()),
            "--no-wait".into(),
            "init".into(),
        ])
        .status()
        .unwrap();
    assert!(initialized.success(), "private OVSDB initialization failed");
    unsafe { std::env::set_var("OVSDB_SOCKET_ADDR", format!("unix:{}", socket.display())) };
    fixture
}

#[tokio::test]
async fn native_port_add_remove_results_satisfy_sealed_ack_after_real_effects() {
    // Share the existing bridge environment lock with other owner fixtures.
    let _environment = crate::human_principal_dispatch::tests::test_registry_guard();
    let _fixture = native_fixture().await;
    let ovsdb = OvsdbDbusClient::new();
    ovsdb.create_bridge("ack-fixture").await.unwrap();
    let schema = op_plugins::state_plugins::rovs_commands::RovsCommandsPlugin::schema().unwrap();
    let added = dispatch_rovs_commands_method(
        &ovsdb,
        "add_port",
        &simd_json::json!({
            "bridge_name":"ack-fixture", "port_name":"ack-fixture-port", "interface_type":"system"
        }),
    )
    .await
    .unwrap();
    crate::method_contract_admission::validate_admitted_output(&schema.methods["add_port"], &added)
        .expect("successful native add must return declared AckOutput");
    assert_eq!(added["success"], true);
    assert_eq!(added["added"], "ack-fixture-port");
    assert_eq!(added["to"], "ack-fixture");
    assert_eq!(added["interface_type"], "system");
    assert!(
        ovsdb
            .list_bridge_ports("ack-fixture")
            .await
            .unwrap()
            .contains(&"ack-fixture-port".into())
    );
    let removed = dispatch_rovs_commands_method(
        &ovsdb,
        "remove_port",
        &simd_json::json!({
            "bridge_name":"ack-fixture", "port_name":"ack-fixture-port"
        }),
    )
    .await
    .unwrap();
    crate::method_contract_admission::validate_admitted_output(
        &schema.methods["remove_port"],
        &removed,
    )
    .expect("successful native removal must return declared AckOutput");
    assert_eq!(removed["success"], true);
    assert_eq!(removed["removed"], "ack-fixture-port");
    assert_eq!(removed["from"], "ack-fixture");
    assert!(
        !ovsdb
            .list_bridge_ports("ack-fixture")
            .await
            .unwrap()
            .contains(&"ack-fixture-port".into())
    );
    // Failure must never be coerced into a successful acknowledgment.
    assert!(
        dispatch_rovs_commands_method(
            &ovsdb,
            "remove_port",
            &simd_json::json!({
                "bridge_name":"ack-fixture", "port_name":"ack-fixture-port"
            })
        )
        .await
        .is_err()
    );
}
