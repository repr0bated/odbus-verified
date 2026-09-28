//! Real relay effects through authenticated canonical dispatch and sealed SHM.
use super::*;
use crate::human_principal_dispatch::tests::{
    pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
};
use crate::identity_sled_dispatch::{IdentitySledStore, tests::write_identity};
use op_plugins::state_plugins::{
    human_principal::HumanPrincipalPlugin, identity_sled::IdentitySledPlugin,
    proxy_server::ProxyServerPlugin, runtime_documents::RuntimeDocuments,
};
use op_state::StatePlugin;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

struct GrantsEnvironment(Option<std::ffi::OsString>);
impl Drop for GrantsEnvironment {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            // FIXME: Audit that the environment access only happens in single-threaded code.
            unsafe { std::env::set_var("OP_GRANTS_PATH", path) };
        } else {
            // FIXME: Audit that the environment access only happens in single-threaded code.
            unsafe { std::env::remove_var("OP_GRANTS_PATH") };
        }
    }
}

#[tokio::test]
async fn three_proxy_methods_require_identity_and_publish_actual_committed_state() {
    let _cozo = temp_cozo();
    let schema = ProxyServerPlugin::new().schema().unwrap();
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
    let documents = Arc::new(RuntimeDocuments::new(
        op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
    ));
    assert!(
        engine
            .proxy_methods
            .set(Arc::new(crate::proxy_methods::ProxyMethods::new(documents)))
            .is_ok()
    );
    let public_key = pk(0xD4);
    register(&public_key, "proxy-fixture").await.unwrap();
    let identity = write_identity(&engine, &public_key).await.unwrap();
    let principal = op_identity::session::derive_principal_id(&public_key);
    let root = tempfile::tempdir().unwrap();
    let grant_path = root.path().join("grants.json");
    let _grant_environment = GrantsEnvironment(std::env::var_os("OP_GRANTS_PATH"));
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_GRANTS_PATH", &grant_path) };
    let capabilities = schema
        .methods
        .values()
        .filter_map(|method| method.required_capability.clone())
        .collect::<Vec<_>>();
    std::fs::write(
        &grant_path,
        serde_json::to_vec(&json!({(principal.clone()): {"capabilities":capabilities}})).unwrap(),
    )
    .unwrap();
    let upstream = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = upstream.local_addr().unwrap();
    let responder = tokio::spawn(async move {
        let (mut stream, _) = upstream.accept().await.unwrap();
        let mut request = [0; 4];
        stream.read_exact(&mut request).await.unwrap();
        assert_eq!(&request, b"ping");
        stream.write_all(b"pong").await.unwrap();
    });
    let available = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = available.local_addr().unwrap().port();
    drop(available);
    let configure = json!({"name":"fixture","enabled":true,"port":port,"upstream":upstream_address.to_string(),"expected_revision":0});
    let capability = schema.methods["configure"].required_capability.as_deref();
    assert!(
        engine
            .dispatch_method_call_with_identity(
                "proxy_server",
                "configure",
                &configure.to_string(),
                capability,
                &principal,
                None,
                None
            )
            .await
            .is_err()
    );
    assert!(op_core::projection_shm::read_projection_bytes("proxy_server").is_none());
    for (method, arguments, revision, running) in [
        ("configure", configure, 1, false),
        (
            "start_proxy",
            json!({"name":"fixture","expected_revision":1}),
            2,
            true,
        ),
        (
            "stop_proxy",
            json!({"name":"fixture","expected_revision":2}),
            3,
            false,
        ),
    ] {
        let result = engine
            .dispatch_method_call_with_identity(
                "proxy_server",
                method,
                &arguments.to_string(),
                schema.methods[method].required_capability.as_deref(),
                &principal,
                Some(&identity.session_id),
                identity.genesis.as_deref(),
            )
            .await
            .unwrap();
        let result = &result["result"];
        let contract =
            serde_json::to_value(schema.methods[method].returns.as_ref().unwrap()).unwrap();
        jsonschema::Validator::new(&contract)
            .unwrap()
            .validate(result)
            .unwrap();
        let bytes = op_core::projection_shm::read_projection_bytes("proxy_server").unwrap();
        let state: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(state["revision"], revision);
        assert_eq!(state["proxies"]["fixture"]["running"], running);
        assert_eq!(state["proxies"]["fixture"], result["status"]);
        if running {
            let mut peer = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .unwrap();
            peer.write_all(b"ping").await.unwrap();
            let mut response = [0; 4];
            tokio::time::timeout(
                std::time::Duration::from_secs(3),
                peer.read_exact(&mut response),
            )
            .await
            .unwrap()
            .unwrap();
            assert_eq!(&response, b"pong");
        } else {
            assert!(
                TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                    .await
                    .is_err()
            );
        }
    }
    responder.await.unwrap();
}
