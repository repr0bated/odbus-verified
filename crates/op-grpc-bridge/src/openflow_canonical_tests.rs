use crate::human_principal_dispatch::tests::{
    pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
};
use crate::identity_sled_dispatch::{IdentitySledStore, tests::write_identity};
use op_plugins::state_plugins::{
    human_principal::HumanPrincipalPlugin,
    identity_sled::IdentitySledPlugin,
    openflow::{FlowAction, FlowEntry, OpenFlowPlugin},
};
use op_state::StatePlugin;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::Mutex;

#[derive(Clone, Default)]
struct ControllerFixture {
    effects: Arc<AtomicUsize>,
    flows: Arc<Mutex<Vec<FlowEntry>>>,
}

#[zbus::interface(name = "org.opdbus.v1.plugins.openflow")]
impl ControllerFixture {
    async fn compare_exchange_flow(
        &self,
        bridge: String,
        flow_json: String,
        expected_json: String,
        delete: bool,
    ) -> zbus::fdo::Result<String> {
        if bridge != "fixture0" {
            return Err(zbus::fdo::Error::Failed("unknown bridge".into()));
        }
        let desired: FlowEntry = serde_json::from_str(&flow_json)
            .map_err(|_| zbus::fdo::Error::InvalidArgs("invalid flow".into()))?;
        let expected = if expected_json.is_empty() {
            None
        } else {
            Some(
                serde_json::from_str::<FlowEntry>(&expected_json)
                    .map_err(|_| zbus::fdo::Error::InvalidArgs("invalid expected flow".into()))?,
            )
        };
        let mut flows = self.flows.lock().await;
        match expected.as_ref() {
            Some(expected) => {
                let position = flows
                    .iter()
                    .position(|flow| flow == expected)
                    .ok_or_else(|| zbus::fdo::Error::Failed("CAS conflict".into()))?;
                flows.remove(position);
            }
            None if flows.contains(&desired) => {
                return Err(zbus::fdo::Error::Failed("flow already exists".into()));
            }
            None => {}
        }
        if !delete {
            flows.push(desired.clone());
        }
        self.effects.fetch_add(1, Ordering::SeqCst);
        serde_json::to_string(&json!({
            "verified":true,
            "action":if delete { "delete" } else { "add" },
            "flow":desired
        }))
        .map_err(|_| zbus::fdo::Error::Failed("serialize receipt".into()))
    }
}

struct GrantsEnvironment(Option<std::ffi::OsString>);

impl Drop for GrantsEnvironment {
    fn drop(&mut self) {
        match self.0.take() {
            // FIXME: Audit that the environment access only happens in single-threaded code.
            Some(value) => unsafe { std::env::set_var("OP_GRANTS_PATH", value) },
            // FIXME: Audit that the environment access only happens in single-threaded code.
            None => unsafe { std::env::remove_var("OP_GRANTS_PATH") },
        }
    }
}

fn flow(action: FlowAction) -> FlowEntry {
    FlowEntry {
        table: 4,
        priority: 200,
        match_fields: HashMap::from([("in_port".into(), "uplink".into())]),
        actions: vec![action],
        cookie: Some(42),
        idle_timeout: 0,
        hard_timeout: 0,
    }
}

#[tokio::test]
async fn all_three_flow_methods_cross_canonical_admission_native_cas_and_child_fold() {
    let _cozo = temp_cozo();
    let schema = OpenFlowPlugin::new().schema().unwrap();
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

    let controller = ControllerFixture::default();
    let (server_stream, client_stream) = tokio::net::UnixStream::pair().unwrap();
    let server = zbus::connection::Builder::unix_stream(server_stream)
        .server(zbus::Guid::generate())
        .unwrap()
        .p2p()
        .serve_at("/org/opdbus/v1/plugins/openflow", controller.clone())
        .unwrap()
        .build();
    let client = zbus::connection::Builder::unix_stream(client_stream)
        .p2p()
        .build();
    let (_server, client) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        tokio::try_join!(server, client)
    })
    .await
    .unwrap()
    .unwrap();
    engine
        .install_openflow_method_connection_for_test(client)
        .unwrap();

    let key = pk(0xC7);
    register(&key, "openflow-canonical-fixture").await.unwrap();
    let identity = write_identity(&engine, &key).await.unwrap();
    let principal = op_identity::session::derive_principal_id(&key);
    let directory = tempfile::tempdir().unwrap();
    let grants = directory.path().join("grants.json");
    let _environment = GrantsEnvironment(std::env::var_os("OP_GRANTS_PATH"));
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_GRANTS_PATH", &grants) };
    std::fs::write(
        &grants,
        serde_json::to_vec(&json!({
            (principal.clone()):{"capabilities":["openflow.write"]}
        }))
        .unwrap(),
    )
    .unwrap();

    let sibling = flow(FlowAction::Normal);
    let initial = json!({
        "bridges":[{"name":"authored","flows":[],"socket_ports":null}],
        "observed_flows":{"other":{"sibling":{"flow":sibling,"installed":true}}},
        "controller_endpoint":"tcp:127.0.0.1:6653",
        "auto_discover_containers":false,
        "enable_security_flows":true,
        "obfuscation_level":1
    });
    op_core::projection_shm::write_projection("openflow", &serde_json::to_vec(&initial).unwrap())
        .unwrap();

    let original = flow(FlowAction::Normal);
    let replacement = flow(FlowAction::Output {
        port: "LOCAL".into(),
    });
    let calls = [
        (
            "add_flow",
            json!({"bridge":"fixture0","flow":original.clone()}),
        ),
        (
            "modify_flow",
            json!({"bridge":"fixture0","expected":original,"replacement":replacement.clone()}),
        ),
        (
            "delete_flow",
            json!({"bridge":"fixture0","expected":replacement}),
        ),
    ];
    for (method, arguments) in calls {
        let declaration = &schema.methods[method];
        let response = engine
            .dispatch_method_call_with_identity(
                "openflow",
                method,
                &arguments.to_string(),
                declaration.required_capability.as_deref(),
                &principal,
                Some(&identity.session_id),
                identity.genesis.as_deref(),
            )
            .await
            .unwrap();
        let output_schema = serde_json::to_value(declaration.returns.as_ref().unwrap()).unwrap();
        jsonschema::Validator::new(&output_schema)
            .unwrap()
            .validate(&response["result"])
            .unwrap();
        assert_eq!(response["result"]["verified"], true);
    }
    assert_eq!(controller.effects.load(Ordering::SeqCst), 3);
    assert!(controller.flows.lock().await.is_empty());

    let committed: Value = serde_json::from_slice(
        &op_core::projection_shm::read_projection_bytes("openflow").unwrap(),
    )
    .unwrap();
    assert_eq!(committed["bridges"], initial["bridges"]);
    assert_eq!(
        committed["observed_flows"]["other"],
        initial["observed_flows"]["other"]
    );
    let fixture_members = committed["observed_flows"]["fixture0"].as_object().unwrap();
    assert_eq!(fixture_members.len(), 1);
    assert_eq!(fixture_members.values().next().unwrap()["installed"], false);

    std::fs::write(&grants, "{}").unwrap();
    let denied = json!({"bridge":"fixture0","flow":flow(FlowAction::Normal)});
    assert!(
        engine
            .dispatch_method_call_with_identity(
                "openflow",
                "add_flow",
                &denied.to_string(),
                Some("openflow.write"),
                &principal,
                Some(&identity.session_id),
                identity.genesis.as_deref(),
            )
            .await
            .is_err()
    );
    assert_eq!(controller.effects.load(Ordering::SeqCst), 3);
}
