//! Isolated native OVS userspace datapath. Every socket and database is private;
//! no command addresses the host OVS database or kernel network interfaces.
use super::*;
use rcgen::{BasicConstraints, CertificateParams, IsCa, KeyPair};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
};

struct NativeChild(Child);
impl Drop for NativeChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct OvsEnvironment(Vec<(&'static str, Option<std::ffi::OsString>)>);

impl OvsEnvironment {
    fn set(root: &Path) -> Self {
        let mut previous = Vec::new();
        for key in ["OVS_RUNDIR", "OVS_DBDIR", "OVS_LOGDIR"] {
            previous.push((key, std::env::var_os(key)));
            // FIXME: Audit that the environment access only happens in single-threaded code.
            unsafe { std::env::set_var(key, root) };
        }
        Self(previous)
    }
}

impl Drop for OvsEnvironment {
    fn drop(&mut self) {
        for (key, value) in self.0.drain(..) {
            match value {
                // FIXME: Audit that the environment access only happens in single-threaded code.
                Some(value) => unsafe { std::env::set_var(key, value) },
                // FIXME: Audit that the environment access only happens in single-threaded code.
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }
}

struct TlsEnvironment(Vec<(&'static str, Option<std::ffi::OsString>)>);

impl TlsEnvironment {
    fn set(ca: &Path, cert: &Path, key: &Path) -> Self {
        let mut previous = Vec::new();
        for (name, path) in [(TLS_CA_ENV, ca), (TLS_CERT_ENV, cert), (TLS_KEY_ENV, key)] {
            previous.push((name, std::env::var_os(name)));
            // FIXME: Audit that the environment access only happens in single-threaded code.
            unsafe { std::env::set_var(name, path) };
        }
        Self(previous)
    }
}

impl Drop for TlsEnvironment {
    fn drop(&mut self) {
        for (name, value) in self.0.drain(..) {
            match value {
                // FIXME: Audit that the environment access only happens in single-threaded code.
                Some(value) => unsafe { std::env::set_var(name, value) },
                // FIXME: Audit that the environment access only happens in single-threaded code.
                None => unsafe { std::env::remove_var(name) },
            }
        }
    }
}

struct TlsFixture {
    ca: std::path::PathBuf,
    server_cert: std::path::PathBuf,
    server_key: std::path::PathBuf,
    switch_cert: std::path::PathBuf,
    switch_key: std::path::PathBuf,
}

impl TlsFixture {
    fn new(root: &Path) -> Self {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let ca_key = KeyPair::generate().unwrap();
        let mut ca_params = CertificateParams::new(vec![]).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let server_key = KeyPair::generate().unwrap();
        let server_cert = CertificateParams::new(vec!["127.0.0.1".into()])
            .unwrap()
            .signed_by(&server_key, &ca, &ca_key)
            .unwrap();
        let switch_key = KeyPair::generate().unwrap();
        let switch_cert = CertificateParams::new(vec!["switch.fixture".into()])
            .unwrap()
            .signed_by(&switch_key, &ca, &ca_key)
            .unwrap();
        let fixture = Self {
            ca: root.join("openflow-ca.pem"),
            server_cert: root.join("controller.pem"),
            server_key: root.join("controller-key.pem"),
            switch_cert: root.join("switch.pem"),
            switch_key: root.join("switch-key.pem"),
        };
        std::fs::write(&fixture.ca, ca.pem()).unwrap();
        std::fs::write(&fixture.server_cert, server_cert.pem()).unwrap();
        std::fs::write(&fixture.server_key, server_key.serialize_pem()).unwrap();
        std::fs::write(&fixture.switch_cert, switch_cert.pem()).unwrap();
        std::fs::write(&fixture.switch_key, switch_key.serialize_pem()).unwrap();
        fixture
    }
}

fn command(binary: &str, arguments: &[String], root: &Path) -> Command {
    let mut command = Command::new(binary);
    command
        .args(arguments)
        .env("OVS_RUNDIR", root)
        .env("OVS_LOGDIR", root)
        .env("OVS_DBDIR", root)
        .stdin(Stdio::null());
    command
}

fn run(binary: &str, arguments: &[String], root: &Path) {
    let result = command(binary, arguments, root).output().unwrap();
    assert!(
        result.status.success(),
        "{binary}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

async fn wait_socket(path: &Path) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !path.exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("private OVS socket did not appear");
}

#[tokio::test]
#[ignore = "requires installed OVS binaries; runs private dummy datapath and ephemeral mTLS fixture"]
async fn native_ovs_exact_add_modify_delete_and_recovery_readback() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path();
    let _ovs_environment = OvsEnvironment::set(path);
    let tls = TlsFixture::new(path);
    let _tls_environment = TlsEnvironment::set(&tls.ca, &tls.server_cert, &tls.server_key);
    let database = path.join("db");
    let socket = path.join("db.sock");
    run(
        "/usr/bin/ovsdb-tool",
        &[
            "create".into(),
            database.display().to_string(),
            "/usr/share/openvswitch/vswitch.ovsschema".into(),
        ],
        path,
    );
    let _database = NativeChild(
        command(
            "/usr/bin/ovsdb-server",
            &[
                format!("--remote=punix:{}", socket.display()),
                format!("--pidfile={}", path.join("db.pid").display()),
                format!("--unixctl={}", path.join("db.ctl").display()),
                database.display().to_string(),
            ],
            path,
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap(),
    );
    wait_socket(&socket).await;
    let db = format!("--db=unix:{}", socket.display());
    run(
        "/usr/bin/ovs-vsctl",
        &[db.clone(), "--no-wait".into(), "init".into()],
        path,
    );
    run(
        "/usr/bin/ovs-vsctl",
        &[
            db.clone(),
            "set-ssl".into(),
            tls.switch_key.display().to_string(),
            tls.switch_cert.display().to_string(),
            tls.ca.display().to_string(),
        ],
        path,
    );

    let _switch = NativeChild(
        command(
            "/usr/bin/ovs-vswitchd",
            &[
                "--enable-dummy".into(),
                format!("--pidfile={}", path.join("switch.pid").display()),
                format!("--unixctl={}", path.join("switch.ctl").display()),
                format!("unix:{}", socket.display()),
            ],
            path,
        )
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap(),
    );
    wait_socket(&path.join("switch.ctl")).await;
    let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = reservation.local_addr().unwrap();
    drop(reservation);
    let mut controller = OpenFlowController::new(address);
    controller.static_fdb.clear();
    let handle = controller.handle();
    let controller = tokio::spawn(controller.run());
    run(
        "/usr/bin/ovs-vsctl",
        &[
            db.clone(),
            "--timeout=10".into(),
            "--".into(),
            "add-br".into(),
            "fixture0".into(),
            "--".into(),
            "set".into(),
            "Bridge".into(),
            "fixture0".into(),
            "datapath_type=dummy".into(),
            "protocols=OpenFlow15".into(),
            "--".into(),
            "set-controller".into(),
            "fixture0".into(),
            format!("ssl:{address}"),
        ],
        path,
    );
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if handle.active_conn.lock().unwrap().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("private OVS did not complete controller handshake");

    // Exercise every retained OpenFlow control method against this private
    // OVSDB/ovs-vswitchd pair and verify the resulting owner state.
    crate::datapath_safe::set_fail_mode("fixture0", "standalone")
        .await
        .unwrap();
    let health = crate::datapath_safe::get_datapath_health("fixture0")
        .await
        .unwrap();
    assert_eq!(health.bridge, "fixture0");
    assert_eq!(health.fail_mode, "standalone");
    crate::datapath_safe::ensure_fallback_normal("fixture0")
        .await
        .unwrap();
    assert!(
        crate::datapath_safe::fallback_present("fixture0")
            .await
            .unwrap()
    );
    crate::datapath_safe::del_controller("fixture0")
        .await
        .unwrap();
    let controllers = Command::new("/usr/bin/ovs-vsctl")
        .args([&db, "get-controller", "fixture0"])
        .output()
        .unwrap();
    assert!(controllers.status.success());
    assert!(
        String::from_utf8_lossy(&controllers.stdout)
            .trim()
            .is_empty()
    );
    crate::datapath_safe::set_controller("fixture0", &format!("ssl:{address}"))
        .await
        .unwrap();
    let attached =
        crate::datapath_safe::attach_controller_safe("fixture0", &format!("ssl:{address}"))
            .await
            .unwrap();
    assert_eq!(attached.bridge, "fixture0");
    assert_eq!(attached.fail_mode, "standalone");
    assert!(attached.fallback_normal);
    assert!(!attached.disable_in_band);
    let original = serde_json::json!({"table":7,"priority":123,"cookie":0x4f42_0000_0000_0001u64,
        "match_fields":{"ip":""},"actions":[{"type":"set_field","field":"nw_ttl","value":"64"},{"type":"normal"}],"idle_timeout":0,"hard_timeout":0}).to_string();
    assert!(
        handle
            .verify_flow("fixture0".into(), original.clone(), true)
            .await
            .unwrap()
    );
    handle
        .compare_exchange_flow("fixture0".into(), original.clone(), None, false)
        .await
        .unwrap();
    assert!(
        handle
            .verify_flow("fixture0".into(), original.clone(), false)
            .await
            .unwrap()
    );
    let mut changed: serde_json::Value = serde_json::from_str(&original).unwrap();
    changed["actions"][0]["value"] = "63".into();
    let changed = changed.to_string();
    assert!(
        handle
            .compare_exchange_flow("fixture0".into(), changed.clone(), None, false)
            .await
            .is_err()
    );
    handle
        .compare_exchange_flow("fixture0".into(), changed.clone(), Some(original), false)
        .await
        .unwrap();
    assert!(
        handle
            .verify_flow("fixture0".into(), changed.clone(), false)
            .await
            .unwrap()
    );
    handle
        .compare_exchange_flow(
            "fixture0".into(),
            changed.clone(),
            Some(changed.clone()),
            true,
        )
        .await
        .unwrap();
    assert!(
        handle
            .verify_flow("fixture0".into(), changed, true)
            .await
            .unwrap()
    );
    controller.abort();
    let _ = controller.await;
}
