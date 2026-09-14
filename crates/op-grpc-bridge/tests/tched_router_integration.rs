// Integration tests for the tched_router Axum HTTP/gRPC-Web host.
//
// These tests spin up the TCP side of the server on an ephemeral port and
// exercise the generated TchedRouterService through a native tonic client. The
// server is configured with `tonic_web::enable`, so the same listener also
// accepts gRPC-Web/HTTP/1.1 clients.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{extract::Query, routing::get, Json, Router};
use op_grpc_bridge::mcp_policy::{
    parse_audience_policy, parse_toolset_manifest, McpProjectionPolicy,
};
use op_grpc_bridge::proto::tched_router::{
    tched_router_service_client::TchedRouterServiceClient, GetSchemaRequest, WatchSchemaRequest,
};
use op_grpc_bridge::schema_loader::SchemaLoader;
use op_grpc_bridge::server::build_axum_app_with_policy;
use op_state::StatePlugin;
use tokio::time::timeout;
use tonic::metadata::MetadataValue;
mod support;

const TEST_TRACE_HEADER: &str = "x-ghostbridge-trace-id";
const TEST_GENESIS_HEADER: &str = "x-ghostbridge-genesis";

struct RuntimeFixture {
    task: tokio::task::JoinHandle<()>,
    environment: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl Drop for RuntimeFixture {
    fn drop(&mut self) {
        self.task.abort();
        for (key, value) in &self.environment {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

async fn start_runtime_fixture() -> RuntimeFixture {
    async fn health() -> Json<serde_json::Value> {
        Json(serde_json::json!({"status":"ok"}))
    }
    async fn full() -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "agents":{"dashboard":{"model_provider":"ollama.default"}},
            "providers":{"models":{"ollama":{"default":{"model":"ollama-model"}}}}
        }))
    }
    async fn list() -> Json<serde_json::Value> {
        Json(serde_json::json!({
            "entries":[{
                "path":"providers.models.ollama.default.model",
                "value":"ollama-model"
            }]
        }))
    }
    async fn prop(
        Query(query): Query<std::collections::HashMap<String, String>>,
    ) -> Json<serde_json::Value> {
        let value = match query.get("path").map(String::as_str) {
            Some("agents.dashboard.model_provider") => "ollama.default",
            Some("providers.models.ollama.default.model") => "ollama-model",
            _ => "",
        };
        Json(serde_json::json!({"value":value}))
    }
    async fn models() -> Json<serde_json::Value> {
        Json(serde_json::json!({"models":["ollama-model"],"live":true}))
    }

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/config", get(full))
        .route("/api/config/list", get(list))
        .route("/api/config/prop", get(prop))
        .route("/api/config/catalog/models", get(models));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let environment = [
        "TCHED_ROUTER_RUNTIME_ENDPOINT",
        "TCHED_ROUTER_RUNTIME_AGENT",
        "TCHED_ROUTER_RUNTIME_TOKEN",
        "OP_GRANTS_PATH",
        "OP_HUMAN_PRINCIPAL_COZO_DB_PATH",
        "OP_LOCAL_HUMAN_WIREGUARD_PUBKEY",
        "OP_LOCAL_HUMAN_SESSION_ID",
        "OP_LOCAL_HUMAN_PRINCIPAL_ID",
    ]
    .into_iter()
    .map(|key| (key, std::env::var_os(key)))
    .collect();
    std::env::set_var("TCHED_ROUTER_RUNTIME_ENDPOINT", format!("http://{address}"));
    std::env::set_var("TCHED_ROUTER_RUNTIME_AGENT", "dashboard");
    std::env::remove_var("TCHED_ROUTER_RUNTIME_TOKEN");
    RuntimeFixture { task, environment }
}

fn write_schema(dir: &std::path::Path, version: &str, description: &str) {
    let schema = op_state_store::PluginSchema::builder("tched_router")
        .version(version)
        .description(description)
        .build();
    let blob = op_blob::blobify_plugin_schema("tched_router", schema);
    op_blob::catalog::BlobStore::open(dir)
        .unwrap()
        .write(&blob)
        .unwrap();
}

fn test_mcp_policy() -> McpProjectionPolicy {
    let audience = parse_audience_policy(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/config/mcp-audience-policy.json"
    )))
    .unwrap();
    let toolsets = parse_toolset_manifest(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/config/mcp-toolsets.json"
    )))
    .unwrap();
    McpProjectionPolicy { audience, toolsets }
}

async fn start_test_server() -> (SocketAddr, Arc<SchemaLoader>, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();
    write_schema(&path, "1.0.0", "test schema");

    let loader = Arc::new(SchemaLoader::new(&path).unwrap());

    // Build the intercepted plugin service exactly as `run_tched_router_server`
    // does, so the Axum app mounts both the tched_router service and the plugin
    // service behind the Ghostbridge interceptor.
    let event_chain = Arc::new(tokio::sync::RwLock::new(op_state_store::EventChain::new(
        op_state_store::ChainConfig::default(),
    )));
    let ovsdb = Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new());
    let mutation_engine = Arc::new(op_grpc_bridge::MutationEngine::new(event_chain, ovsdb));
    let operation_server = op_grpc_bridge::grpc_server::OperationGrpcServer::new(mutation_engine);
    let app = build_axum_app_with_policy(loader.clone(), operation_server, test_mcp_policy());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        // Keep the fixture directory alive for the server and the caller's
        // reload path. Dropping TempDir here would remove the schema before
        // the watch test rewrites it.
        let _dir = dir;
        axum::serve(listener, app).await.unwrap();
    });

    // Give the server a moment to start accepting connections.
    tokio::time::sleep(Duration::from_millis(100)).await;

    (addr, loader, path)
}

#[tokio::test]
async fn should_serve_schema_over_grpc_web() {
    let (addr, _loader, _path) = start_test_server().await;

    let endpoint = format!("http://{}", addr);
    let mut client = TchedRouterServiceClient::connect(endpoint).await.unwrap();

    let mut request = tonic::Request::new(GetSchemaRequest {});
    request.metadata_mut().insert(
        TEST_TRACE_HEADER,
        MetadataValue::from_static("integration-test-trace"),
    );
    request.metadata_mut().insert(
        TEST_GENESIS_HEADER,
        MetadataValue::from_static("integration-test-genesis"),
    );

    let response = client.get_schema(request).await.unwrap();
    let inner = response.into_inner();

    let parsed: serde_json::Value = serde_json::from_str(&inner.schema_json).unwrap();
    assert_eq!(parsed["name"], "tched_router");
    assert_eq!(parsed["version"], "1.0.0");
    assert_eq!(inner.trace_id, "integration-test-trace");
    assert_eq!(inner.session_genesis, "integration-test-genesis");
}

#[tokio::test]
async fn should_stream_reload_on_sighup() {
    let (addr, loader, path) = start_test_server().await;

    let endpoint = format!("http://{}", addr);
    let mut client = TchedRouterServiceClient::connect(endpoint).await.unwrap();

    let mut stream = client
        .watch_schema(tonic::Request::new(WatchSchemaRequest {}))
        .await
        .unwrap()
        .into_inner();

    let initial = timeout(Duration::from_secs(5), stream.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(initial.event_type, "initial");

    // Simulate a SIGHUP-triggered reload by reloading the file and broadcasting
    // the event through the loader's channel. Tests do not send real SIGHUP to
    // the process to avoid interfering with other tests.
    write_schema(&path, "2.0.0", "reloaded schema");
    loader.load().unwrap();
    let _ = loader
        .reload_tx()
        .send(op_grpc_bridge::schema_loader::SchemaReloadEvent {
            event_type: "reload".to_string(),
        });

    let event = timeout(Duration::from_secs(5), stream.message())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(event.event_type, "reload");

    let parsed: serde_json::Value = serde_json::from_str(&event.schema_json).unwrap();
    assert_eq!(parsed["version"], "2.0.0");
}

#[tokio::test]
async fn list_models_uses_the_audited_tched_router_method_dispatcher() {
    let router_schema = op_plugins::state_plugins::tched_router::TchedRouterPlugin::new()
        .schema()
        .unwrap();
    let _catalog = support::CatalogScope::new(&[
        op_plugins::state_plugins::human_principal::HumanPrincipalPlugin::new()
            .schema()
            .unwrap(),
        op_plugins::state_plugins::identity_sled::IdentitySledPlugin::new()
            .schema()
            .unwrap(),
        router_schema.clone(),
    ]);
    let _runtime = start_runtime_fixture().await;
    let event_chain = Arc::new(tokio::sync::RwLock::new(op_state_store::EventChain::new(
        op_state_store::ChainConfig::default(),
    )));
    let ovsdb = Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new());
    let engine = op_grpc_bridge::MutationEngine::new(event_chain.clone(), ovsdb);
    let identity_cozo = op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap();
    engine
        .configure_identity_store(identity_cozo.clone())
        .unwrap();
    use base64::Engine as _;
    let public_key = base64::engine::general_purpose::STANDARD.encode([0xD2; 32]);
    std::env::set_var(
        "OP_HUMAN_PRINCIPAL_COZO_DB_PATH",
        _catalog.root().join("human-cozo"),
    );
    std::env::set_var("OP_LOCAL_HUMAN_WIREGUARD_PUBKEY", &public_key);
    std::env::remove_var("OP_LOCAL_HUMAN_SESSION_ID");
    std::env::remove_var("OP_LOCAL_HUMAN_PRINCIPAL_ID");
    let actor = op_identity::session::derive_principal_id(&public_key);
    let capability = router_schema.methods["ListModels"]
        .required_capability
        .clone()
        .unwrap();
    let grants = _catalog.root().join("grants.json");
    std::fs::write(
        &grants,
        serde_json::to_vec(&serde_json::json!({
            (actor.clone()): {"capabilities":[capability.clone()]}
        }))
        .unwrap(),
    )
    .unwrap();
    std::env::set_var("OP_GRANTS_PATH", grants);
    support::provision_identity_record(&identity_cozo, _catalog.root(), &public_key).unwrap();
    engine.recover_configured_identity_store().await.unwrap();
    let caller = engine
        .bootstrap_configured_local_human_identity()
        .await
        .unwrap()
        .expect("configured fixture identity");

    let result = engine
        .dispatch_method_call_with_identity(
            "tched_router",
            "ListModels",
            r#"{"provider":"ollama"}"#,
            Some(&capability),
            &actor,
            Some(&caller.session_id),
            Some(&caller.genesis_hex),
        )
        .await
        .unwrap();

    assert_eq!(result["success"], true);
    assert_eq!(result["plugin_id"], "tched_router");
    assert_eq!(result["method"], "ListModels");
    let routes = result["result"]["model_routes"].as_array().unwrap();
    assert!(!routes.is_empty());
    assert!(routes
        .iter()
        .all(|route| { route["provider"] == "ollama" || route["upstream_provider"] == "ollama" }));

    let chain = event_chain.read().await;
    let event = chain.events().last().unwrap();
    assert_eq!(event.plugin_id, "tched_router");
    assert_eq!(event.method_name.as_deref(), Some("ListModels"));
    assert_eq!(
        event.capability_id.as_deref(),
        Some("cap.software.3tched-router.models.read@v1")
    );
    assert_eq!(event.actor_id, actor);
    assert_eq!(
        event.input_payload.as_ref().expect("payload")["args"]["provider"],
        "ollama"
    );
}
