use crate::{
    human_principal_dispatch::tests::{
        pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
    },
    identity_sled_dispatch::{tests::write_identity, IdentitySledStore},
    tched_router_runtime::TchedRouterRuntimeClient,
};
use op_state::StatePlugin;
use serde_json::json;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);
impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                // FIXME: Audit that the environment access only happens in single-threaded code.
                Some(value) => unsafe { std::env::set_var(key, value) },
                // FIXME: Audit that the environment access only happens in single-threaded code.
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }
}

fn write_grants(path: &std::path::Path, actor: &str, allowed: bool) {
    let capabilities = if allowed {
        vec!["mcp.write"]
    } else {
        Vec::<&str>::new()
    };
    std::fs::write(
        path,
        serde_json::to_vec(&json!({(actor.to_owned()):{"capabilities":capabilities}})).unwrap(),
    )
    .unwrap();
}

fn toolset_input(expected_generation: u64) -> serde_json::Value {
    json!({
        "expected_generation":expected_generation,
        "toolsets":[{
            "id":"broker_ops",
            "temperature":"warm",
            "provider":"emqx",
            "requires_provider_health":true,
            "tools":["plugin.emqx.get_status"]
        }]
    })
}

async fn call_tool_groups(
    engine: &crate::mutation_engine::MutationEngine,
    actor: &str,
    session: &str,
    genesis: Option<&str>,
    input: serde_json::Value,
) -> anyhow::Result<serde_json::Value> {
    engine
        .dispatch_method_call_with_identity(
            "mcp",
            "configure_tool_groups",
            &input.to_string(),
            Some("mcp.write"),
            actor,
            Some(session),
            genesis,
        )
        .await
}

#[tokio::test]
async fn canonical_tool_group_dispatch_commits_file_live_owner_and_member_projection() {
    use crate::mcp_policy::{
        AudiencePolicy, McpPolicyOwner, McpProjectionPolicy, ToolsetManifest, HOT_TOOL_NAMES,
    };
    use op_plugins::state_plugins::{
        emqx::EmqxPlugin, human_principal::HumanPrincipalPlugin, identity_sled::IdentitySledPlugin,
        mcp::McpStatePlugin,
    };

    let _cozo = temp_cozo();
    let mcp = McpStatePlugin::new(Arc::new(op_state_store::MemoryStore::new()), "unused")
        .schema()
        .unwrap();
    let _shm = temp_shm_with_schemas(&[
        HumanPrincipalPlugin::new().schema().unwrap(),
        IdentitySledPlugin::new().schema().unwrap(),
        EmqxPlugin::new().schema().unwrap(),
        mcp.clone(),
    ]);
    let engine = test_engine();
    engine
        .install_identity_sled_store_for_test(IdentitySledStore::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        ))
        .unwrap();
    let key = pk(0xE9);
    register(&key, "mcp-policy-fixture").await.unwrap();
    let sled = write_identity(&engine, &key).await.unwrap();
    let actor = op_identity::session::derive_principal_id(&key);
    let directory = tempfile::tempdir().unwrap();
    let grants = directory.path().join("grants.json");
    let toolsets_path = directory.path().join("toolsets.json");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let _environment = Environment(
        [
            "OP_GRANTS_PATH",
            "TCHED_ROUTER_RUNTIME_ENDPOINT",
            "TCHED_ROUTER_RUNTIME_TOKEN",
        ]
        .into_iter()
        .map(|name| (name, std::env::var_os(name)))
        .collect(),
    );
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_GRANTS_PATH", &grants) };
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe {
        std::env::set_var(
            "TCHED_ROUTER_RUNTIME_ENDPOINT",
            format!("http://{}", listener.local_addr().unwrap()),
        )
    };
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::remove_var("TCHED_ROUTER_RUNTIME_TOKEN") };
    write_grants(&grants, &actor, true);
    let owner = McpPolicyOwner::for_durable_fixture(
        McpProjectionPolicy {
            audience: AudiencePolicy {
                version: 1,
                rotation_epoch: 1,
                singleton_chatbot_principal_id: "87b0decc-8464-5abf-05d8-b52ec88ff9f1".into(),
            },
            toolsets: ToolsetManifest {
                generation: 1,
                hot: HOT_TOOL_NAMES.iter().map(|name| (*name).into()).collect(),
                sets: vec![],
            },
        },
        toolsets_path.clone(),
        crate::sealed_schema_reader::blob_catalog_dir_from_env(),
    )
    .unwrap();
    engine.install_mcp_policy_owner(owner.clone()).unwrap();

    let first = call_tool_groups(
        &engine,
        &actor,
        &sled.session_id,
        sled.genesis.as_deref(),
        toolset_input(1),
    )
    .await
    .unwrap();
    assert_eq!(first["result"]["generation"], 2);
    assert_eq!(owner.snapshot().toolsets.generation, 2);
    let durable: ToolsetManifest =
        serde_json::from_slice(&std::fs::read(&toolsets_path).unwrap()).unwrap();
    assert_eq!(durable.generation, 2);
    let fresh = op_core::projection_shm::read_projection_bytes("mcp").unwrap();
    let fresh: serde_json::Value = serde_json::from_slice(&fresh).unwrap();
    assert_eq!(fresh["toolsets"], first["result"]);
    assert!(fresh.get("config").is_none());

    let pin = crate::sealed_schema_reader::read_manifest_pinned_schema_document(
        &crate::sealed_schema_reader::blob_catalog_dir_from_env(),
        "mcp",
    )
    .unwrap();
    engine
        .process_observed_state(
            "mcp",
            simd_json::serde::to_owned_value(json!({
                "config":{"enabled":true,"deferred_loading":true,"servers":[]},
                "toolsets":first["result"].clone()
            }))
            .unwrap(),
            pin.schema_hash,
        )
        .await
        .unwrap();
    let second = call_tool_groups(
        &engine,
        &actor,
        &sled.session_id,
        sled.genesis.as_deref(),
        toolset_input(2),
    )
    .await
    .unwrap();
    assert_eq!(second["result"]["generation"], 3);
    let projected = op_core::projection_shm::read_projection_bytes("mcp").unwrap();
    let projected: serde_json::Value = serde_json::from_slice(&projected).unwrap();
    assert_eq!(projected["config"]["deferred_loading"], true);
    assert_eq!(projected["toolsets"], second["result"]);

    let before_file = std::fs::read(&toolsets_path).unwrap();
    let before_state = op_core::projection_shm::read_projection_bytes("mcp").unwrap();
    assert!(call_tool_groups(
        &engine,
        &actor,
        &sled.session_id,
        sled.genesis.as_deref(),
        toolset_input(1),
    )
    .await
    .is_err());
    assert_eq!(std::fs::read(&toolsets_path).unwrap(), before_file);
    assert_eq!(owner.snapshot().toolsets.generation, 3);
    assert_eq!(
        op_core::projection_shm::read_projection_bytes("mcp").unwrap(),
        before_state
    );

    write_grants(&grants, &actor, true);
    let held = owner.begin_mutation().await;
    let queued_attempt = owner.mutation_attempts() + 1;
    let queued_engine = engine.clone();
    let queued_actor = actor.clone();
    let queued_session = sled.session_id.clone();
    let queued_genesis = sled.genesis.clone();
    let queued = tokio::spawn(async move {
        call_tool_groups(
            &queued_engine,
            &queued_actor,
            &queued_session,
            queued_genesis.as_deref(),
            toolset_input(3),
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while owner.mutation_attempts() < queued_attempt {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("queued method must pass initial admission and reach the owner lock");
    write_grants(&grants, &actor, false);
    drop(held);
    assert!(queued.await.unwrap().is_err());
    assert_eq!(std::fs::read(&toolsets_path).unwrap(), before_file);
    assert_eq!(owner.snapshot().toolsets.generation, 3);
    assert_eq!(
        op_core::projection_shm::read_projection_bytes("mcp").unwrap(),
        before_state
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn queued_mcp_configuration_rechecks_both_original_grants_and_facade_seal() {
    use op_plugins::state_plugins::{
        human_principal::HumanPrincipalPlugin, identity_sled::IdentitySledPlugin,
        mcp::McpStatePlugin, tched_router::TchedRouterPlugin,
    };
    let _cozo = temp_cozo();
    let mcp = McpStatePlugin::new(Arc::new(op_state_store::MemoryStore::new()), "unused")
        .schema()
        .unwrap();
    let router = TchedRouterPlugin::new().schema().unwrap();
    let _shm = temp_shm_with_schemas(&[
        HumanPrincipalPlugin::new().schema().unwrap(),
        IdentitySledPlugin::new().schema().unwrap(),
        mcp.clone(),
        router.clone(),
    ]);
    let engine = test_engine();
    engine
        .install_identity_sled_store_for_test(IdentitySledStore::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        ))
        .unwrap();
    let key = pk(0xE8);
    register(&key, "mcp-queued-fixture").await.unwrap();
    let sled = write_identity(&engine, &key).await.unwrap();
    let actor = op_identity::session::derive_principal_id(&key);
    let target_cap = router.methods["PatchConfig"]
        .required_capability
        .clone()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let grants = directory.path().join("grants.json");
    let _environment = Environment(
        [
            "OP_GRANTS_PATH",
            "TCHED_ROUTER_RUNTIME_ENDPOINT",
            "TCHED_ROUTER_RUNTIME_TOKEN",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect(),
    );
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::set_var("OP_GRANTS_PATH", &grants) };
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe { std::env::remove_var("TCHED_ROUTER_RUNTIME_TOKEN") };
    let effects = Arc::new(AtomicUsize::new(0));
    let observed = effects.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    // FIXME: Audit that the environment access only happens in single-threaded code.
    unsafe {
        std::env::set_var(
            "TCHED_ROUTER_RUNTIME_ENDPOINT",
            format!("http://{}", listener.local_addr().unwrap()),
        )
    };
    let http = tokio::spawn(async move {
        let app = axum::Router::new().fallback(axum::routing::any(move || {
            observed.fetch_add(1, Ordering::SeqCst);
            async { axum::http::StatusCode::OK }
        }));
        axum::serve(listener, app).await.unwrap();
    });
    for scenario in ["outer_grant", "target_grant", "outer_seal"] {
        let write_grants = |missing: Option<&str>| {
            let allowed = ["mcp.write", target_cap.as_str()]
                .into_iter()
                .filter(|cap| Some(*cap) != missing)
                .collect::<Vec<_>>();
            std::fs::write(
                &grants,
                serde_json::to_vec(&json!({(actor.clone()):{"capabilities":allowed}})).unwrap(),
            )
            .unwrap();
        };
        write_grants(None);
        let mut sealer =
            op_blob::BlobStore::open(crate::sealed_schema_reader::blob_catalog_dir_from_env())
                .unwrap();
        sealer
            .write(&op_blob::blobify_plugin_schema("mcp", mcp.clone()))
            .unwrap();
        let guard = TchedRouterRuntimeClient::selection_guard().await;
        let mut changes = engine.change_tx().subscribe();
        let queued = engine.clone();
        let queued_actor = actor.clone();
        let session = sled.session_id.clone();
        let genesis = sled.genesis.clone();
        let task = tokio::spawn(async move {
            queued.dispatch_method_call_with_identity(
                "mcp", "configure_server", &json!({"operations":[{
                    "op":"replace","path":"/mcp/servers/docs/url","value":"https://docs.example.test/mcp"
                }]}).to_string(), Some("mcp.write"), &queued_actor, Some(&session), genesis.as_deref(),
            ).await
        });
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let event = changes.recv().await.unwrap();
                if event.plugin_id == "tched_router"
                    && event.member_name.as_deref() == Some("PatchConfig")
                {
                    break;
                }
            }
        })
        .await
        .expect("canonical nested target must enter before changing admission");
        match scenario {
            "outer_grant" => write_grants(Some("mcp.write")),
            "target_grant" => write_grants(Some(&target_cap)),
            _ => {
                let mut changed = mcp.clone();
                changed.description.push_str(" changed while queued");
                sealer
                    .write(&op_blob::blobify_plugin_schema("mcp", changed))
                    .unwrap();
            }
        }
        drop(guard);
        assert!(
            tokio::time::timeout(Duration::from_secs(10), task)
                .await
                .unwrap()
                .unwrap()
                .is_err(),
            "{scenario}"
        );
        assert_eq!(effects.load(Ordering::SeqCst), 0, "{scenario}");
    }
    http.abort();
}
