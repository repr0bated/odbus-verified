//! Canonical mail admission, local D-Bus ownership and complete SHM folding.

use super::*;
use crate::human_principal_dispatch::tests::{
    pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
};
use crate::identity_sled_dispatch::{tests::write_identity, IdentitySledStore};
use op_plugins::state_plugins::{
    human_principal::HumanPrincipalPlugin,
    identity_sled::IdentitySledPlugin,
    mail_server::{
        methods, native_owner::MailAdminService, native_owner::NativePaths, MailServerPlugin,
    },
    runtime_documents::RuntimeDocuments,
};
use op_state::StatePlugin;
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex as StdMutex,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
    sync::Notify,
};

struct Environment {
    values: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl Environment {
    fn capture(keys: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            values: keys
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect(),
        }
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in &self.values {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

struct NativeFixture {
    root: tempfile::TempDir,
    paths: NativePaths,
    socket: std::path::PathBuf,
    accepted: Arc<AtomicUsize>,
    qmgr_requests: Arc<StdMutex<Vec<Vec<u8>>>>,
    qmgr_request: Arc<Notify>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for NativeFixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl NativeFixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("etc");
        let queue = root.path().join("queue");
        std::fs::create_dir_all(&config).unwrap();
        std::fs::create_dir_all(&queue).unwrap();
        let paths = NativePaths {
            domains: config.join("domains"),
            mailboxes: config.join("vmailbox"),
            aliases: config.join("virtual"),
            dovecot_users: config.join("users"),
            dovecot_users_gid: None,
            mail_root: root.path().join("mail"),
            mail_uid: None,
            mail_gid: None,
            showq_socket: queue.join("showq"),
            qmgr_socket: queue.join("qmgr"),
            postfix_master_pid: None,
        };
        std::fs::write(
            &paths.mailboxes,
            "owner@example.com example.com/owner/Maildir/\n",
        )
        .unwrap();
        std::fs::write(
            &paths.dovecot_users,
            "owner@example.com:{SHA512-CRYPT}$6$fixture::::::\n",
        )
        .unwrap();
        std::fs::write(&paths.aliases, "").unwrap();

        let showq = UnixListener::bind(&paths.showq_socket).unwrap();
        let showq_task = tokio::spawn(async move {
            let response = b"protocol\0mail_queue_list_protocol\0\0queue_name\0deferred\0queue_id\0Q1\0time\x001700000000\0size\x001234\0forced_expire\x000\0sender\0sender@example.net\0original_recipient\0\0recipient\0owner@example.com\0log_class\0defer\0reason\0temporary failure\0\0\0";
            while let Ok((mut stream, _)) = showq.accept().await {
                let _ = stream.write_all(response).await;
                let _ = stream.shutdown().await;
            }
        });

        let qmgr = UnixListener::bind(&paths.qmgr_socket).unwrap();
        let qmgr_requests = Arc::new(StdMutex::new(Vec::new()));
        let requests = qmgr_requests.clone();
        let qmgr_request = Arc::new(Notify::new());
        let request_ready = qmgr_request.clone();
        let qmgr_task = tokio::spawn(async move {
            while let Ok((mut stream, _)) = qmgr.accept().await {
                let mut request = Vec::new();
                let _ = stream.read_to_end(&mut request).await;
                requests.lock().unwrap().push(request);
                request_ready.notify_one();
            }
        });

        let service = MailAdminService::new(
            RuntimeDocuments::new(op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap()),
            paths.clone(),
        );
        service.adopt_or_reconcile().await.unwrap();

        let socket = root.path().join("admin.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let accepted = Arc::new(AtomicUsize::new(0));
        let accepted_connections = accepted.clone();
        let dbus_task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                accepted_connections.fetch_add(1, Ordering::SeqCst);
                let service = service.clone();
                tokio::spawn(async move {
                    let connection = zbus::connection::Builder::unix_stream(stream)
                        .server(zbus::Guid::generate())
                        .unwrap()
                        .p2p()
                        .serve_at(methods::OBJECT_PATH, service)
                        .unwrap()
                        .build()
                        .await
                        .unwrap();
                    connection.closed().await;
                });
            }
        });

        Self {
            root,
            paths,
            socket,
            accepted,
            qmgr_requests,
            qmgr_request,
            tasks: vec![showq_task, qmgr_task, dbus_task],
        }
    }

    async fn wait_for_flush(&self) {
        if self.qmgr_requests.lock().unwrap().is_empty() {
            tokio::time::timeout(
                std::time::Duration::from_secs(3),
                self.qmgr_request.notified(),
            )
            .await
            .expect("qmgr fixture received flush request");
        }
    }
}

fn validate_json(schema: &impl serde::Serialize, value: &Value) {
    let schema = serde_json::to_value(schema).unwrap();
    jsonschema::Validator::new(&schema)
        .unwrap()
        .validate(value)
        .unwrap();
}

#[tokio::test]
async fn all_nine_mail_methods_cross_mutation_engine_dbus_and_commit_complete_state() {
    let _cozo = temp_cozo();
    let schema = MailServerPlugin::new().schema().unwrap();
    let _shm = temp_shm_with_schemas(&[
        HumanPrincipalPlugin::new().schema().unwrap(),
        IdentitySledPlugin::new().schema().unwrap(),
        schema.clone(),
    ]);
    let engine = test_engine();
    engine
        .install_identity_sled_store_for_test(IdentitySledStore::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        ))
        .unwrap();

    let public_key = pk(0xDA);
    register(&public_key, "mail-fixture").await.unwrap();
    let identity = write_identity(&engine, &public_key).await.unwrap();
    let principal = op_identity::session::derive_principal_id(&public_key);

    let fixture = NativeFixture::new().await;
    let grant_path = fixture.root.path().join("grants.json");
    let _environment = Environment::capture(["OP_GRANTS_PATH", "OP_MAIL_ADMIN_SOCKET"]);
    std::env::set_var("OP_GRANTS_PATH", &grant_path);
    std::env::set_var("OP_MAIL_ADMIN_SOCKET", &fixture.socket);
    let capabilities = schema
        .methods
        .values()
        .filter_map(|method| method.required_capability.clone())
        .collect::<Vec<_>>();
    assert_eq!(schema.methods.len(), 9);
    std::fs::write(
        &grant_path,
        serde_json::to_vec(&json!({(principal.clone()): {"capabilities":capabilities}})).unwrap(),
    )
    .unwrap();

    let mut initial = MailServerPlugin::declared_state();
    initial.container_name = "fixture-mail".into();
    initial.container_status = "Running".into();
    initial.domain = "preserved.example".into();
    initial.healthy = true;
    engine
        .publish_plugin_projection_state(
            "mail_server",
            simd_json::serde::to_owned_value(serde_json::to_value(&initial).unwrap()).unwrap(),
            ChangeType::PropertySet,
            true,
        )
        .await
        .unwrap();
    let before = op_core::projection_shm::read_projection_bytes("mail_server").unwrap();

    let read_capability = schema.methods["get_queue"].required_capability.as_deref();
    assert!(engine
        .dispatch_method_call_with_identity(
            "mail_server",
            "get_queue",
            "{}",
            read_capability,
            &principal,
            None,
            None,
        )
        .await
        .is_err());
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 0);
    assert_eq!(
        op_core::projection_shm::read_projection_bytes("mail_server").unwrap(),
        before
    );

    let calls = [
        ("add_domain", json!({"domain":"example.net"})),
        (
            "add_mailbox",
            json!({"domain":"example.net","user":"alice","password_hash":"{SHA512-CRYPT}$6$fixture"}),
        ),
        (
            "set_quota",
            json!({"domain":"example.net","user":"alice","quota_mb":256}),
        ),
        (
            "add_alias",
            json!({"domain":"example.net","alias":"info","destination":"alice@example.net"}),
        ),
        ("get_queue", json!({})),
        ("flush_queue", json!({})),
        (
            "remove_alias",
            json!({"domain":"example.net","alias":"info"}),
        ),
        (
            "remove_mailbox",
            json!({"domain":"example.net","user":"alice"}),
        ),
        ("remove_domain", json!({"domain":"example.net"})),
    ];
    for (method, arguments) in calls {
        let declaration = &schema.methods[method];
        validate_json(&declaration.args, &arguments);
        let response = engine
            .dispatch_method_call_with_identity(
                "mail_server",
                method,
                &arguments.to_string(),
                declaration.required_capability.as_deref(),
                &principal,
                Some(&identity.session_id),
                identity.genesis.as_deref(),
            )
            .await
            .unwrap_or_else(|error| panic!("mail_server.{method}: {error:#}"));
        validate_json(declaration.returns.as_ref().unwrap(), &response["result"]);

        let bytes = op_core::projection_shm::read_projection_bytes("mail_server").unwrap();
        let state: Value = serde_json::from_slice(&bytes).unwrap();
        let mut exact_bytes = bytes.clone();
        let exact = simd_json::to_owned_value(&mut exact_bytes).unwrap();
        schema.validate_exact_schema_fields(&exact).unwrap();
        assert_eq!(state["container_name"], "fixture-mail");
        assert_eq!(state["container_status"], "Running");
        assert_eq!(state["domain"], "preserved.example");
        assert_eq!(state["healthy"], true);
        assert!(state["administration"]["domains"]
            .as_array()
            .unwrap()
            .contains(&json!("example.com")));
        let administration = &state["administration"];
        match method {
            "add_domain" => assert!(administration["domains"]
                .as_array()
                .unwrap()
                .contains(&json!("example.net"))),
            "add_mailbox" => assert!(administration["mailboxes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|mailbox| mailbox["address"] == "alice@example.net")),
            "set_quota" => {
                assert!(administration["mailboxes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|mailbox| {
                        mailbox["address"] == "alice@example.net"
                            && mailbox["quota_bytes"] == 256 * 1024 * 1024
                    }))
            }
            "add_alias" => {
                assert!(administration["aliases"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|alias| {
                        alias["address"] == "info@example.net"
                            && alias["destination"] == "alice@example.net"
                    }))
            }
            "get_queue" => {
                assert_eq!(response["result"]["summary"]["messages"], 1);
                assert_eq!(response["result"]["entries"][0]["queue_id"], "Q1");
            }
            "flush_queue" => assert_eq!(response["result"]["accepted"], true),
            "remove_alias" => assert!(administration["aliases"]
                .as_array()
                .unwrap()
                .iter()
                .all(|alias| alias["address"] != "info@example.net")),
            "remove_mailbox" => assert!(administration["mailboxes"]
                .as_array()
                .unwrap()
                .iter()
                .all(|mailbox| mailbox["address"] != "alice@example.net")),
            "remove_domain" => assert!(!administration["domains"]
                .as_array()
                .unwrap()
                .contains(&json!("example.net"))),
            _ => unreachable!("fixture lists every canonical mail method"),
        }
    }

    fixture.wait_for_flush().await;
    assert_eq!(fixture.accepted.load(Ordering::SeqCst), 9);
    assert_eq!(
        fixture.qmgr_requests.lock().unwrap().as_slice(),
        [b"FADI\0"]
    );
    let final_state: Value = serde_json::from_slice(
        &op_core::projection_shm::read_projection_bytes("mail_server").unwrap(),
    )
    .unwrap();
    assert_eq!(
        final_state["administration"]["domains"],
        json!(["example.com"])
    );
    assert_eq!(
        final_state["administration"]["mailboxes"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        final_state["administration"]["mailboxes"][0]["address"],
        "owner@example.com"
    );
    assert_eq!(final_state["administration"]["aliases"], json!([]));
    assert_eq!(final_state["administration"]["queue"]["messages"], 1);
    assert_eq!(final_state["administration"]["queue"]["bytes"], 1234);

    assert_eq!(
        std::fs::read_to_string(&fixture.paths.domains).unwrap(),
        "example.com\tOK\n"
    );
    assert_eq!(
        std::fs::read_to_string(&fixture.paths.mailboxes).unwrap(),
        "owner@example.com\texample.com/owner/Maildir/\n"
    );
    assert_eq!(std::fs::read_to_string(&fixture.paths.aliases).unwrap(), "");
    assert_eq!(
        std::fs::read_to_string(&fixture.paths.dovecot_users).unwrap(),
        "owner@example.com:{SHA512-CRYPT}$6$fixture::::::\n"
    );
    assert!(fixture
        .paths
        .mail_root
        .join("example.net/alice/Maildir")
        .is_dir());
}
