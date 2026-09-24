//! Canonical dispatch tested against an isolated upstream HTTP fixture.
mod support;

use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use op_grpc_bridge::mutation_engine::MutationEngine;
use op_state_store::{ChainConfig, EventChain};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, OnceLock,
    },
};
use tokio::sync::Mutex as AsyncMutex;

#[derive(Clone)]
struct Fixture {
    inner: Arc<AsyncMutex<FixtureState>>,
    requests: Arc<AtomicUsize>,
}
struct FixtureState {
    provider: String,
    models: HashMap<String, String>,
    write_fail: bool,
    chats: usize,
    writes: usize,
    ignore_writes: bool,
    race_provider_before_patch: Option<String>,
    race_model_before_patch: Option<(String, String)>,
    change_provider_on_catalog: Option<String>,
    change_model_on_catalog: Option<(String, String)>,
    oversized_config_list: bool,
    omit_reference_from_list: Option<String>,
    catalog_live: bool,
    patch_redirect: Option<String>,
    projection_block_after_patch: Option<std::path::PathBuf>,
    mcp_url: String,
    complete_native_config: bool,
    expected_sid1: Option<String>,
}

async fn fixture() -> (String, Fixture, tokio::task::JoinHandle<()>) {
    let fixture = Fixture {
        requests: Arc::new(AtomicUsize::new(0)),
        inner: Arc::new(AsyncMutex::new(FixtureState {
            provider: "ollama.main".into(),
            models: HashMap::from([
                ("ollama.main".into(), "old-model".into()),
                ("openai.main".into(), "new-model".into()),
                ("custom.default".into(), "unlisted-model".into()),
            ]),
            write_fail: false,
            chats: 0,
            writes: 0,
            ignore_writes: false,
            race_provider_before_patch: None,
            race_model_before_patch: None,
            change_provider_on_catalog: None,
            change_model_on_catalog: None,
            oversized_config_list: false,
            omit_reference_from_list: None,
            catalog_live: true,
            patch_redirect: None,
            projection_block_after_patch: None,
            mcp_url: "https://docs.example.test/mcp".into(),
            complete_native_config: false,
            expected_sid1: None,
        })),
    };
    let app = Router::new()
        .route("/health", get(health_fixture))
        .route("/api/config", get(full_config).patch(config_patch))
        .route("/api/config/list", get(config_list))
        .route("/api/config/prop", get(config_prop))
        .route("/api/config/catalog/models", get(catalog_models))
        .route("/api/tools", get(tools_fixture))
        .route("/a2a/:alias", post(chat_fixture))
        .with_state(fixture.clone())
        .layer(axum::middleware::from_fn_with_state(
            fixture.clone(),
            count_request,
        ));
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{address}"), fixture, task)
}

async fn count_request(
    State(fixture): State<Fixture>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    fixture.requests.fetch_add(1, Ordering::SeqCst);
    next.run(request).await
}

async fn full_config(State(f): State<Fixture>) -> Json<Value> {
    let state = f.inner.lock().await;
    let mut families = serde_json::Map::new();
    for (reference, model) in &state.models {
        let (family, alias) = reference
            .split_once('.')
            .expect("fixture provider references are family.alias");
        families
            .entry(family.to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()))
            .as_object_mut()
            .expect("fixture provider family is an object")
            .insert(alias.to_string(), json!({"model": model}));
    }
    let mut config = if state.complete_native_config {
        native_config_defaults()
    } else {
        json!({})
    };
    config.as_object_mut().unwrap().extend(
        json!({
            "agents": {"dashboard": {"model_provider": state.provider}},
            "providers": {"models": Value::Object(families)},
            "mcp": {"enabled":true,"servers":[{
                "name":"docs","transport":"http","url":state.mcp_url,
                "env":{}, "headers":{"Authorization":"fixture-private-mcp-token"}
            }]}
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    Json(config)
}

// Fixture data comes from the one imported native contract. It is served by
// the private HTTP owner; no schema or runtime catalog is materialized here.
fn native_config_defaults() -> Value {
    use op_plugins::state_plugins::tched_router_contract::document;
    static DEFAULTS: OnceLock<Value> = OnceLock::new();
    DEFAULTS
        .get_or_init(|| {
            let fields = document()["properties"].as_object().unwrap();
            let config = Value::Object(
                fields
                    .iter()
                    .map(|(name, field)| {
                        let value = field.get("default").cloned().unwrap_or_else(|| {
                            if field["type"] == "array" {
                                json!([])
                            } else {
                                json!({})
                            }
                        });
                        (name.clone(), value)
                    })
                    .collect(),
            );
            jsonschema::validate(document(), &config)
                .expect("native default fixture satisfies the import");
            config
        })
        .clone()
}

async fn config_list(State(f): State<Fixture>) -> Json<Value> {
    let s = f.inner.lock().await;
    if s.oversized_config_list {
        return Json(json!({"entries":[], "padding":"x".repeat(4 * 1024 * 1024 + 1)}));
    }
    Json(json!({
        "entries": s.models.iter()
            .filter(|(reference, _)| Some(reference.as_str()) != s.omit_reference_from_list.as_deref())
            .map(|(r,m)| json!({"path":format!("providers.models.{r}.model"),"value":m}))
            .collect::<Vec<_>>()
    }))
}
async fn health_fixture() -> Json<Value> {
    Json(json!({"status":"ok"}))
}

async fn tools_fixture() -> Json<Value> {
    Json(json!({"tools":[{
        "name":"fixture_lookup", "description":"Look up a fixture record",
        "parameters":{"type":"object","required":["id"],"properties":{"id":{"type":"string"}}}
    }]}))
}
#[derive(Deserialize)]
struct PropQuery {
    path: String,
}
async fn config_prop(State(f): State<Fixture>, Query(q): Query<PropQuery>) -> Json<Value> {
    let s = f.inner.lock().await;
    let value = if q.path == "agents.dashboard.model_provider" {
        s.provider.clone()
    } else if q.path == "mcp.servers.docs.url" {
        s.mcp_url.clone()
    } else {
        s.models
            .get(
                q.path
                    .strip_prefix("providers.models.")
                    .unwrap_or_default()
                    .strip_suffix(".model")
                    .unwrap_or_default(),
            )
            .cloned()
            .unwrap_or_default()
    };
    Json(json!({"path":q.path,"value": value}))
}
async fn catalog_models(
    State(f): State<Fixture>,
    Query(q): Query<HashMap<String, String>>,
) -> Json<Value> {
    let mut state = f.inner.lock().await;
    if let Some(provider) = state.change_provider_on_catalog.take() {
        state.provider = provider;
    }
    if let Some((reference, model)) = state.change_model_on_catalog.take() {
        state.models.insert(reference, model);
    }
    let models = match q.get("model_provider").map(String::as_str) {
        Some("ollama.main") => vec!["old-model", "old-alt"],
        Some("ollama.secondary") => vec!["secondary-model"],
        Some("openai.main") => vec!["new-model"],
        _ => Vec::new(),
    };
    Json(json!({"models":models,"live":state.catalog_live}))
}
fn patch_receipt(operations: &[Value]) -> Value {
    let results = operations
        .iter()
        .map(|operation| {
            let pointer = operation["path"].as_str().unwrap_or_default();
            let path = pointer.trim_start_matches('/').replace('/', ".");
            let value = operation.get("value").cloned().unwrap_or(Value::Null);
            json!({"op": operation["op"], "path": path, "value": value})
        })
        .collect::<Vec<_>>();
    json!({"saved": true, "results": results, "warnings": []})
}

async fn config_patch(State(f): State<Fixture>, Json(body): Json<Value>) -> Response {
    let mut state = f.inner.lock().await;
    if let Some(location) = &state.patch_redirect {
        return (
            StatusCode::TEMPORARY_REDIRECT,
            [(header::LOCATION, location.clone())],
        )
            .into_response();
    }
    if state.write_fail {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    if let Some(provider) = state.race_provider_before_patch.take() {
        state.provider = provider;
    }
    if let Some((reference, model)) = state.race_model_before_patch.take() {
        state.models.insert(reference, model);
    }
    let Some(operations) = body.as_array() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    // Model validation of this request's test operations before its writes.
    // This fixture lock does not imply cross-handler CAS in the native daemon.
    for op in operations.iter().filter(|op| op["op"] == "test") {
        let Some(path) = op["path"].as_str() else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let observed = if path == "/agents/dashboard/model_provider" {
            Some(&state.provider)
        } else if path == "/mcp/servers/docs/url" {
            Some(&state.mcp_url)
        } else {
            path.strip_prefix("/providers/models/")
                .and_then(|p| p.strip_suffix("/model"))
                .and_then(|key| state.models.get(&key.replace('/', ".")))
        };
        if observed.map(String::as_str) != op["value"].as_str() {
            return StatusCode::CONFLICT.into_response();
        }
    }
    state.writes += 1;
    if state.ignore_writes {
        return Json(patch_receipt(operations)).into_response();
    }
    for op in operations.iter().filter(|op| op["op"] == "replace") {
        let Some(path) = op["path"].as_str() else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let Some(value) = op["value"].as_str().map(str::to_string) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if path == "/agents/dashboard/model_provider" {
            state.provider = value;
        } else if path == "/mcp/servers/docs/url" {
            state.mcp_url = value;
        } else {
            let Some(key) = path
                .strip_prefix("/providers/models/")
                .and_then(|p| p.strip_suffix("/model"))
            else {
                return StatusCode::BAD_REQUEST.into_response();
            };
            state.models.insert(key.replace('/', "."), value);
        }
    }
    if let Some(blocked) = state.projection_block_after_patch.take() {
        // Model a provider effect that completes immediately before the bridge's
        // authoritative SHM publication. The test restores this process-global
        // fixture path before making another admitted call.
        std::env::set_var("OP_SHM_STATE_DIR", blocked);
    }
    Json(patch_receipt(operations)).into_response()
}

async fn redirected_patch_sink(State(hits): State<Arc<AtomicUsize>>) -> StatusCode {
    hits.fetch_add(1, Ordering::SeqCst);
    StatusCode::NO_CONTENT
}

async fn redirect_sink() -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route("/capture", patch(redirected_patch_sink))
        .with_state(hits.clone());
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{address}/capture"), hits, task)
}
async fn chat_fixture(
    State(f): State<Fixture>,
    headers: axum::http::HeaderMap,
    Json(_body): Json<Value>,
) -> Response {
    let mut state = f.inner.lock().await;
    let authenticated = state.expected_sid1.as_deref().is_some_and(|expected| {
        headers
            .get("x-opdbus-sealed-id-bin")
            .and_then(|value| value.to_str().ok())
            == Some(expected)
    });
    if !authenticated {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    state.chats += 1;
    let mut response =
        Json(json!({"result":{"artifacts":[{"parts":[{"kind":"text","text":"fixture"}]}]}}))
            .into_response();
    response.headers_mut().insert(
        "x-opdbus-identity-authenticated",
        axum::http::HeaderValue::from_static("SID1"),
    );
    response
}

struct FixtureScope {
    task: tokio::task::JoinHandle<()>,
    extra_tasks: Vec<tokio::task::JoinHandle<()>>,
    environment: Vec<(&'static str, Option<std::ffi::OsString>)>,
}
impl Drop for FixtureScope {
    fn drop(&mut self) {
        self.task.abort();
        for task in &self.extra_tasks {
            task.abort();
        }
        for (key, value) in &self.environment {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

async fn dispatch(
    engine: &MutationEngine,
    caller: &op_grpc_bridge::mutation_engine::SessionContext,
    method: &str,
    args: Value,
) -> anyhow::Result<Value> {
    use op_state::StatePlugin;
    let schema = op_plugins::state_plugins::tched_router::TchedRouterPlugin::new()
        .schema()
        .unwrap();
    engine
        .dispatch_method_call_with_identity(
            "tched_router",
            method,
            &args.to_string(),
            schema.methods[method].required_capability.as_deref(),
            &op_identity::session::derive_principal_id(&caller.wireguard_pubkey),
            Some(&caller.session_id),
            Some(&caller.genesis_hex),
        )
        .await
}

async fn dispatch_mcp(
    engine: &MutationEngine,
    caller: &op_grpc_bridge::mutation_engine::SessionContext,
    method: &str,
    args: Value,
) -> anyhow::Result<Value> {
    use op_state::StatePlugin;
    let schema = op_plugins::state_plugins::mcp::McpStatePlugin::new(
        Arc::new(op_state_store::MemoryStore::new()),
        "unused",
    )
    .schema()
    .unwrap();
    engine
        .dispatch_method_call_with_identity(
            "mcp",
            method,
            &args.to_string(),
            schema.methods[method].required_capability.as_deref(),
            &op_identity::session::derive_principal_id(&caller.wireguard_pubkey),
            Some(&caller.session_id),
            Some(&caller.genesis_hex),
        )
        .await
}

async fn dispatch_llm(
    engine: &MutationEngine,
    caller: &op_grpc_bridge::mutation_engine::SessionContext,
    method: &str,
    args: Value,
) -> anyhow::Result<Value> {
    use op_state::StatePlugin;
    let schema = op_plugins::state_plugins::large_language_model::LargeLanguageModelPlugin::new()
        .schema()
        .unwrap();
    engine
        .dispatch_method_call_with_identity(
            "large_language_model",
            method,
            &args.to_string(),
            schema.methods[method].required_capability.as_deref(),
            &op_identity::session::derive_principal_id(&caller.wireguard_pubkey),
            Some(&caller.session_id),
            Some(&caller.genesis_hex),
        )
        .await
}

#[test]
fn gemma_route_uses_the_exact_tched_router_resolve_contract() {
    use op_state::StatePlugin;
    let gemma = op_plugins::state_plugins::gemma_brain::GemmaBrainPlugin::new()
        .schema()
        .unwrap();
    let router = op_plugins::state_plugins::tched_router::TchedRouterPlugin::new()
        .schema()
        .unwrap();
    let facade = &gemma.methods["route"];
    let target = &router.methods["ResolveRoute"];
    assert_eq!(facade.args, target.args);
    assert_eq!(facade.returns, target.returns);
    assert_eq!(facade.side_effect, target.side_effect);
    assert_eq!(facade.idempotent, target.idempotent);
    assert_eq!(facade.subid, "obs.service.gemma.route@v2");
    assert_eq!(facade.required_capability.as_deref(), Some("gemma.invoke"));
}

#[tokio::test]
async fn canonical_selection_commits_verified_upstream_and_preserves_projection_on_failure() {
    use op_state::StatePlugin;
    let root = tempfile::tempdir().unwrap();
    let (endpoint, fixture, task) = fixture().await;
    let (redirect_target, redirect_hits, redirect_task) = redirect_sink().await;
    let _scope = FixtureScope {
        task,
        extra_tasks: vec![redirect_task],
        environment: [
            "TCHED_ROUTER_RUNTIME_ENDPOINT",
            "TCHED_ROUTER_RUNTIME_AGENT",
            "TCHED_ROUTER_RUNTIME_TOKEN",
            "OP_SHM_STATE_DIR",
            "DBUS_SESSION_BUS_ADDRESS",
            "OP_BLOB_CATALOG_DIR",
            "OP_GRANTS_PATH",
            "OP_HUMAN_PRINCIPAL_COZO_DB_PATH",
            "OP_LOCAL_HUMAN_WIREGUARD_PUBKEY",
            "OP_LOCAL_HUMAN_SESSION_ID",
            "OP_LOCAL_HUMAN_PRINCIPAL_ID",
            "OP_IDENTITY_PROJECTION_GROUP",
            "OP_PLUGIN_RUNTIME_COZO_PATH",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect(),
    };
    std::env::set_var("TCHED_ROUTER_RUNTIME_ENDPOINT", endpoint);
    std::env::set_var("TCHED_ROUTER_RUNTIME_AGENT", "dashboard");
    std::env::set_var("TCHED_ROUTER_RUNTIME_TOKEN", "fixture-secret");
    std::env::set_var("OP_SHM_STATE_DIR", root.path().join("state"));
    std::env::set_var(
        "OP_PLUGIN_RUNTIME_COZO_PATH",
        root.path().join("runtime-documents"),
    );
    let catalog = root.path().join("blobs");
    let schema = op_plugins::state_plugins::tched_router::TchedRouterPlugin::new()
        .schema()
        .unwrap();
    let gemma_schema = op_plugins::state_plugins::gemma_brain::GemmaBrainPlugin::new()
        .schema()
        .unwrap();
    let schemas = [
        op_plugins::state_plugins::human_principal::HumanPrincipalPlugin::new()
            .schema()
            .unwrap(),
        op_plugins::state_plugins::identity_sled::IdentitySledPlugin::new()
            .schema()
            .unwrap(),
        schema.clone(),
        gemma_schema.clone(),
        op_plugins::state_plugins::large_language_model::LargeLanguageModelPlugin::new()
            .schema()
            .unwrap(),
        op_plugins::state_plugins::ctl_plane_chatbot::CtlPlaneChatbotPlugin::new()
            .schema()
            .unwrap(),
        op_plugins::state_plugins::mcp::McpStatePlugin::new(
            Arc::new(op_state_store::MemoryStore::new()),
            "unused",
        )
        .schema()
        .unwrap(),
    ];
    let mut sealer = op_blob::BlobStore::open(&catalog).unwrap();
    for plugin_schema in schemas {
        let plugin_name = plugin_schema.name.clone();
        sealer
            .write(&op_blob::blobify_plugin_schema(&plugin_name, plugin_schema))
            .unwrap();
    }
    std::env::set_var("OP_BLOB_CATALOG_DIR", &catalog);
    std::env::set_var(
        "DBUS_SESSION_BUS_ADDRESS",
        format!("unix:path={}/absent-bus", root.path().display()),
    );
    let engine = MutationEngine::new(
        Arc::new(tokio::sync::RwLock::new(EventChain::new(
            ChainConfig::default(),
        ))),
        Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new()),
    );
    let identity_cozo = op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap();
    let episode_cozo = Arc::new(op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap());
    engine
        .configure_chatbot_episode_store(episode_cozo.clone())
        .unwrap();
    engine
        .configure_chatbot_episode_projection(
            op_core::projection_shm::PrivateProjectionDirectory::prepare(
                &root.path().join("private-episodes"),
            )
            .unwrap(),
            catalog,
        )
        .unwrap();
    engine
        .configure_identity_store(identity_cozo.clone())
        .unwrap();
    use base64::Engine as _;
    let public_key = base64::engine::general_purpose::STANDARD.encode([0xC8; 32]);
    std::env::set_var(
        "OP_HUMAN_PRINCIPAL_COZO_DB_PATH",
        root.path().join("human-cozo"),
    );
    std::env::set_var("OP_LOCAL_HUMAN_WIREGUARD_PUBKEY", &public_key);
    std::env::remove_var("OP_LOCAL_HUMAN_SESSION_ID");
    std::env::remove_var("OP_LOCAL_HUMAN_PRINCIPAL_ID");
    std::env::set_var(
        "OP_IDENTITY_PROJECTION_GROUP",
        nix::unistd::Group::from_gid(nix::unistd::Gid::current())
            .unwrap()
            .expect("test process group")
            .name,
    );
    let actor = op_identity::session::derive_principal_id(&public_key);
    let grants = root.path().join("grants.json");
    let mut capabilities = schema.capabilities.keys().cloned().collect::<Vec<_>>();
    capabilities.push("gemma.invoke".to_owned());
    capabilities.extend(["mcp.read".to_owned(), "mcp.write".to_owned()]);
    std::fs::write(
        &grants,
        serde_json::to_vec(&serde_json::json!({(actor): {"capabilities": capabilities}})).unwrap(),
    )
    .unwrap();
    std::env::set_var("OP_GRANTS_PATH", &grants);
    support::provision_identity_record(&identity_cozo, root.path(), &public_key).unwrap();
    engine.recover_configured_identity_store().await.unwrap();
    let caller = engine
        .bootstrap_configured_local_human_identity()
        .await
        .unwrap()
        .expect("configured fixture identity");
    fixture.inner.lock().await.expected_sid1 = Some(
        op_identity::session_projection::resolve_identity_credential_session(Some(
            &caller.session_id,
        ))
        .unwrap()
        .mcp_sealed_id_header()
        .unwrap()
        .to_owned(),
    );

    // A first read has no committed projection to borrow defaults from. The
    // selected agent provider and that provider's model come from the exact
    // paths declared by the pinned v0.8.4 configuration schema. Configured
    // aliases sharing one family are represented once.
    {
        let mut upstream = fixture.inner.lock().await;
        upstream.provider = "ollama.main".into();
        upstream
            .models
            .insert("ollama.secondary".into(), "secondary-model".into());
    }
    let fresh = dispatch(&engine, &caller, "GetState", json!({}))
        .await
        .unwrap();
    let observed = &fresh["result"]["state"];
    assert_eq!(observed["selected_provider"], "ollama");
    assert_eq!(observed["selected_model"], "old-model");
    assert!(observed.get("model_assignments").is_none());
    let providers = observed["providers"].as_array().unwrap();
    let old = providers
        .iter()
        .find(|provider| provider["id"] == "ollama")
        .unwrap();
    assert_eq!(old["aliases"].as_array().unwrap().len(), 2);
    let routes = observed["model_routes"].as_array().unwrap();
    assert!(routes
        .iter()
        .any(|route| route["provider"] == "ollama" && route["model"] == "old-model"));
    assert!(routes
        .iter()
        .any(|route| { route["provider"] == "ollama" && route["model"] == "secondary-model" }));
    assert!(providers.iter().any(|provider| {
        provider["id"] == "custom" && provider["aliases"] == json!(["custom.default"])
    }));
    assert!(!routes.iter().any(|route| route["provider"] == "custom"));

    // An empty catalog is valid for an inactive configured alias, but cannot
    // become the selected provider's represented model inventory.
    fixture.inner.lock().await.provider = "custom.default".into();
    let selected_empty = dispatch(&engine, &caller, "GetState", json!({})).await;
    assert!(format!("{:#}", selected_empty.unwrap_err())
        .contains("selected 'custom.default' model catalog is empty"));
    fixture.inner.lock().await.provider = "ollama.main".into();

    // Family projection contains both aliases, but mutation admission must
    // validate against the exact alias being selected.
    let writes_before_cross_alias = fixture.inner.lock().await.writes;
    assert!(dispatch(
        &engine,
        &caller,
        "SetModel",
        json!({"model_id":"secondary-model"}),
    )
    .await
    .is_err());
    assert!(dispatch(
        &engine,
        &caller,
        "SetSelection",
        json!({"provider_id":"ollama.secondary","model_id":"old-model"}),
    )
    .await
    .is_err());
    assert_eq!(fixture.inner.lock().await.writes, writes_before_cross_alias);

    // Gemma's retained route facade must reach the same admitted router
    // resolver and preserve its typed provider/model selection.
    let principal_id = op_identity::session::derive_principal_id(&caller.wireguard_pubkey);
    let route = engine
        .dispatch_method_call_with_identity(
            "gemma_brain",
            "route",
            &json!({"hint":"old-model"}).to_string(),
            Some("gemma.invoke"),
            &principal_id,
            Some(&caller.session_id),
            Some(&caller.genesis_hex),
        )
        .await
        .unwrap();
    assert_eq!(route["result"]["route"]["hint"], "runtime");
    assert_eq!(route["result"]["route"]["provider"], "ollama");
    assert_eq!(route["result"]["route"]["model"], "old-model");
    let native_route = dispatch(
        &engine,
        &caller,
        "ResolveRoute",
        json!({"hint":"old-model"}),
    )
    .await
    .unwrap();
    assert_eq!(route["result"], native_route["result"]);

    // External MCP configuration is owned by the same native router and its
    // committed SHM projection, not by a second file-based plugin manager.
    let mcp = dispatch_mcp(&engine, &caller, "get_config", json!({}))
        .await
        .unwrap();
    assert_eq!(
        mcp["result"]["config"]["servers"][0]["url"],
        "https://docs.example.test/mcp"
    );
    assert_eq!(
        mcp["result"]["config"]["servers"][0]["headers"]["Authorization"],
        "***MASKED***"
    );
    assert!(!mcp.to_string().contains("fixture-private-mcp-token"));
    let updated_url = "https://updated.example.test/mcp";
    let configure = json!({"operations":[{
        "op":"replace", "path":"/mcp/servers/docs/url", "value":updated_url
    }]});
    let receipt = dispatch_mcp(&engine, &caller, "configure_server", configure.clone())
        .await
        .unwrap();
    assert_eq!(receipt["result"]["saved"], true);
    assert_eq!(fixture.inner.lock().await.mcp_url, updated_url);
    let mcp = dispatch_mcp(&engine, &caller, "get_config", json!({}))
        .await
        .unwrap();
    assert_eq!(mcp["result"]["config"]["servers"][0]["url"], updated_url);
    let owner = dispatch(&engine, &caller, "GetMcpConfig", json!({}))
        .await
        .unwrap();
    assert_eq!(mcp["result"], owner["result"]);
    for forbidden in [
        "/agents/dashboard/model_provider",
        "/mcp/servers_other/docs/url",
        "/mcp/servers/docs/url/",
        "/mcp/servers/docs/a~1b",
    ] {
        let before = fixture.requests.load(Ordering::SeqCst);
        assert!(dispatch_mcp(
            &engine,
            &caller,
            "configure_server",
            json!({"operations":[{
                "op":"replace", "path":forbidden, "value":"forbidden"
            }]})
        )
        .await
        .is_err());
        assert_eq!(fixture.requests.load(Ordering::SeqCst), before);
    }
    let target_write_grant = schema.methods["PatchConfig"]
        .required_capability
        .as_ref()
        .unwrap();
    for missing in ["mcp.write", target_write_grant.as_str()] {
        let allowed = capabilities
            .iter()
            .filter(|grant| grant.as_str() != missing)
            .cloned()
            .collect::<Vec<_>>();
        std::fs::write(
            &grants,
            serde_json::to_vec(&json!({
                (principal_id.clone()): {"capabilities":allowed}
            }))
            .unwrap(),
        )
        .unwrap();
        let before = fixture.requests.load(Ordering::SeqCst);
        assert!(
            dispatch_mcp(&engine, &caller, "configure_server", configure.clone())
                .await
                .is_err()
        );
        assert_eq!(fixture.requests.load(Ordering::SeqCst), before);
    }
    std::fs::write(
        &grants,
        serde_json::to_vec(&json!({
            (principal_id.clone()): {"capabilities":capabilities}
        }))
        .unwrap(),
    )
    .unwrap();
    let before_mcp_failure =
        op_core::projection_shm::read_projection_bytes("tched_router").unwrap();
    fixture.inner.lock().await.write_fail = true;
    assert!(dispatch_mcp(&engine, &caller, "configure_server", json!({"operations":[{
        "op":"replace", "path":"/mcp/servers/docs/url", "value":"https://rejected.example.test/mcp"
    }]})).await.is_err());
    assert_eq!(fixture.inner.lock().await.mcp_url, updated_url);
    assert_eq!(
        op_core::projection_shm::read_projection_bytes("tched_router").unwrap(),
        before_mcp_failure
    );
    fixture.inner.lock().await.write_fail = false;

    let error = engine
        .dispatch_method_call_with_identity(
            "gemma_brain",
            "route",
            &json!({"hint":"not-an-observed-native-route"}).to_string(),
            Some("gemma.invoke"),
            &principal_id,
            Some(&caller.session_id),
            Some(&caller.genesis_hex),
        )
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("not declared by the native runtime"));

    // Removing the nested router grant stops before the native target.
    let before_denial = fixture.requests.load(Ordering::SeqCst);
    let without_nested = capabilities
        .iter()
        .filter(|capability| *capability != "cap.software.3tched-router.route.resolve@v1")
        .cloned()
        .collect::<Vec<_>>();
    std::fs::write(
        &grants,
        serde_json::to_vec(
            &serde_json::json!({(principal_id.clone()): {"capabilities": without_nested}}),
        )
        .unwrap(),
    )
    .unwrap();
    let error = engine
        .dispatch_method_call_with_identity(
            "gemma_brain",
            "route",
            &json!({"hint":"old-model"}).to_string(),
            Some("gemma.invoke"),
            &principal_id,
            Some(&caller.session_id),
            Some(&caller.genesis_hex),
        )
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("exact nested"));
    assert_eq!(fixture.requests.load(Ordering::SeqCst), before_denial);
    for (outer_granted, session, args) in [
        (
            false,
            Some(caller.session_id.as_str()),
            json!({"hint":"old-model"}),
        ),
        (true, None, json!({"hint":"old-model"})),
        (true, Some(caller.session_id.as_str()), json!({"hint":""})),
        (
            true,
            Some(caller.session_id.as_str()),
            json!({"hint":"old-model", "unknown":true}),
        ),
    ] {
        let current_grants = capabilities
            .iter()
            .filter(|capability| outer_granted || capability.as_str() != "gemma.invoke")
            .collect::<Vec<_>>();
        std::fs::write(
            &grants,
            serde_json::to_vec(&json!({
                (principal_id.clone()): {"capabilities":current_grants}
            }))
            .unwrap(),
        )
        .unwrap();
        let result = engine
            .dispatch_method_call_with_identity(
                "gemma_brain",
                "route",
                &args.to_string(),
                Some("gemma.invoke"),
                &principal_id,
                session,
                Some(&caller.genesis_hex),
            )
            .await;
        assert!(
            result.is_err(),
            "invalid or unauthorized Gemma route reached its target"
        );
        assert_eq!(fixture.requests.load(Ordering::SeqCst), before_denial);
    }
    std::fs::write(
        &grants,
        serde_json::to_vec(
            &serde_json::json!({(principal_id.clone()): {"capabilities": capabilities}}),
        )
        .unwrap(),
    )
    .unwrap();
    // A selected property is not sufficient authority when its profile is
    // absent from the configured provider enumeration.
    fixture.inner.lock().await.omit_reference_from_list = Some("ollama.main".into());
    let error = dispatch(&engine, &caller, "GetState", json!({}))
        .await
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("absent from the configured provider list"),
        "unexpected error: {error:#}"
    );
    fixture.inner.lock().await.omit_reference_from_list = None;

    // Catalog reads are a multi-request observation. Reject a selection that
    // changes before the final ref/model recheck instead of publishing a mixed
    // catalog and selection.
    fixture.inner.lock().await.change_provider_on_catalog = Some("openai.main".into());
    let error = dispatch(&engine, &caller, "GetState", json!({}))
        .await
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("selection changed during catalog observation"),
        "unexpected error: {error:#}"
    );
    fixture.inner.lock().await.provider = "ollama.main".into();

    fixture.inner.lock().await.change_model_on_catalog =
        Some(("ollama.main".into(), "old-alt".into()));
    let error = dispatch(&engine, &caller, "GetState", json!({}))
        .await
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("selection changed during catalog observation"),
        "unexpected error: {error:#}"
    );
    fixture
        .inner
        .lock()
        .await
        .models
        .insert("ollama.main".into(), "old-model".into());

    fixture.inner.lock().await.oversized_config_list = true;
    let error = dispatch(&engine, &caller, "GetState", json!({}))
        .await
        .unwrap_err();
    assert!(
        format!("{error:#}").contains("response exceeds 4194304 bytes"),
        "unexpected error: {error:#}"
    );
    fixture.inner.lock().await.oversized_config_list = false;

    // A cached upstream catalog remains an observed catalog. Its entries are
    // represented as unavailable/catalogued instead of rejecting the read or
    // claiming live availability.
    fixture.inner.lock().await.catalog_live = false;
    let cached = dispatch(&engine, &caller, "GetState", json!({}))
        .await
        .unwrap();
    assert!(cached["result"]["state"]["model_routes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|route| route["available"] == false && route["status"] == "catalogued"));
    fixture.inner.lock().await.catalog_live = true;

    {
        let mut upstream = fixture.inner.lock().await;
        upstream.provider = "ollama.main".into();
        upstream.models.remove("ollama.secondary");
    }
    // Role assignments have a durable RuntimeDocuments owner. Create one
    // through its admitted canonical method, then prove a native GetState
    // observation carries that independent sibling into committed SHM.
    let assignment = dispatch(
        &engine,
        &caller,
        "SetOvsRoutingModel",
        json!({"model_id":"old-model"}),
    )
    .await
    .unwrap();
    assert_eq!(assignment["result"]["ovs_routing"], "old-model");
    let observed = dispatch(&engine, &caller, "GetState", json!({}))
        .await
        .unwrap();
    assert_eq!(observed["result"]["state"]["selected_provider"], "ollama");
    assert_eq!(observed["result"]["state"]["selected_model"], "old-model");
    assert!(observed["result"]["state"]["providers"]
        .as_array()
        .is_some_and(|providers| !providers.is_empty()));
    let committed: Value = serde_json::from_slice(
        &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(),
    )
    .unwrap();
    assert_eq!(committed["model_assignments"]["ovs_routing"], "old-model");
    let initial = engine.get_state("tched_router").await.unwrap();
    let writes_before_invalid = fixture.inner.lock().await.writes;
    let invalid = dispatch(
        &engine,
        &caller,
        "SetSelection",
        json!({"provider_id":"openai","model_id":"old-model"}),
    )
    .await;
    assert!(invalid.is_err());
    assert_eq!(fixture.inner.lock().await.writes, writes_before_invalid);
    assert_eq!(engine.get_state("tched_router").await.unwrap(), initial);

    // The native loopback client never follows an upstream redirect. In
    // particular, a config mutation body and bearer credential cannot be
    // forwarded to a second peer.
    fixture.inner.lock().await.patch_redirect = Some(redirect_target);
    let redirected = dispatch(
        &engine,
        &caller,
        "SetProvider",
        json!({"provider_id":"openai"}),
    )
    .await;
    assert!(redirected.is_err());
    assert_eq!(redirect_hits.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.inner.lock().await.writes, writes_before_invalid);
    fixture.inner.lock().await.patch_redirect = None;

    // SetProvider tests the provider value in the same upstream JSON Patch as
    // its replacement. A gateway edit between observation and PATCH causes a
    // conflict and no bridge-authored write.
    let writes_before_race = fixture.inner.lock().await.writes;
    fixture.inner.lock().await.race_provider_before_patch = Some("openai.main".into());
    let raced_provider = dispatch(
        &engine,
        &caller,
        "SetProvider",
        json!({"provider_id":"openai"}),
    )
    .await;
    assert!(raced_provider.is_err());
    {
        let mut upstream = fixture.inner.lock().await;
        assert_eq!(upstream.writes, writes_before_race);
        assert_eq!(upstream.provider, "openai.main");
        upstream.provider = "ollama.main".into();
    }

    // SetModel tests both the selected provider and its current model before
    // replacing the model. A concurrent model edit is preserved.
    let writes_before_race = fixture.inner.lock().await.writes;
    fixture.inner.lock().await.race_model_before_patch =
        Some(("ollama.main".into(), "external-model".into()));
    let raced_model = dispatch(&engine, &caller, "SetModel", json!({"model_id":"old-alt"})).await;
    assert!(raced_model.is_err());
    {
        let mut upstream = fixture.inner.lock().await;
        assert_eq!(upstream.writes, writes_before_race);
        assert_eq!(upstream.models["ollama.main"], "external-model");
        upstream
            .models
            .insert("ollama.main".into(), "old-model".into());
    }

    // An HTTP success that failed to change upstream config must never become
    // a successful mutation or a new projected selection.
    fixture.inner.lock().await.ignore_writes = true;
    assert!(dispatch(
        &engine,
        &caller,
        "SetSelection",
        json!({"provider_id":"openai","model_id":"new-model"})
    )
    .await
    .is_err());
    assert_eq!(engine.get_state("tched_router").await.unwrap(), initial);
    fixture.inner.lock().await.ignore_writes = false;

    // A second admitted assignment updates the durable sibling after the read
    // populated the mirror. Selection readback must carry both assignments
    // into the same authoritative observation commit.
    dispatch(
        &engine,
        &caller,
        "SetVectorizationModel",
        json!({"model_id":"old-alt"}),
    )
    .await
    .unwrap();
    let before_generation =
        op_core::projection_shm::read_projection_snapshot(&["tched_router".into()])
            .unwrap()
            .generation;

    let result = dispatch(
        &engine,
        &caller,
        "SetSelection",
        json!({"provider_id":"openai","model_id":"new-model"}),
    )
    .await
    .unwrap();
    assert_eq!(
        result["result"],
        json!({"selected_provider":"openai","selected_model":"new-model"})
    );
    let selected: Value =
        serde_json::to_value(engine.get_state("tched_router").await.unwrap()).unwrap();
    assert_eq!(selected["selected_provider"], "openai");
    assert_eq!(selected["selected_model"], "new-model");
    assert_eq!(selected["model_assignments"]["ovs_routing"], "old-model");
    assert_eq!(selected["model_assignments"]["vectorization"], "old-alt");
    assert_eq!(
        op_core::projection_shm::read_projection_snapshot(&["tched_router".into()])
            .unwrap()
            .generation,
        before_generation + 1,
        "provider and model must be committed together"
    );
    assert_eq!(fixture.inner.lock().await.provider, "openai.main");

    assert!(dispatch(
        &engine,
        &caller,
        "Chat",
        json!({"message":"hi", "model":"stale"})
    )
    .await
    .is_err());
    assert_eq!(fixture.inner.lock().await.chats, 0);
    let chat = dispatch(
        &engine,
        &caller,
        "Chat",
        json!({"message":"hi", "conversation_id":"native-fixture"}),
    )
    .await
    .unwrap();
    assert_eq!(chat["result"]["provider"], "openai");
    assert_eq!(chat["result"]["model"], "new-model");
    assert_eq!(fixture.inner.lock().await.chats, 1);
    let episode_scope = op_cozo_store::EpisodeScope {
        principal_id: principal_id.clone(),
        session_id: caller.session_id.clone(),
        session_genesis: caller.genesis_hex.clone(),
    };
    let recorded = episode_cozo
        .snapshot_scoped_episodes(&episode_scope, 32)
        .unwrap();
    assert_eq!(
        recorded.episode_count, 1,
        "rejected native calls create no episodes"
    );
    let episode = &recorded.episodes[0].episode;
    assert_eq!(episode["principal_id"], principal_id);
    assert_eq!(episode["on_behalf_of"], principal_id);
    assert_eq!(episode["conversation_id"], "native-fixture");
    assert_eq!(episode["goal_text"], "hi");
    assert_eq!(episode["decision_output"], chat["result"]["content"]);
    assert_eq!(episode["reasoning_summary"], Value::Null);
    assert_eq!(episode["pii_flagged"], true);
    assert!(
        engine.get_state("ctl_plane_chatbot").await.is_none(),
        "private episode must not enter the public state cache"
    );
    let mut other_scope = episode_scope.clone();
    other_scope.session_genesis.push_str("-other");
    assert_eq!(
        episode_cozo
            .snapshot_scoped_episodes(&other_scope, 32)
            .unwrap()
            .episode_count,
        0
    );

    let before_failed_write = engine.get_state("tched_router").await.unwrap();
    fixture.inner.lock().await.write_fail = true;
    assert!(dispatch(
        &engine,
        &caller,
        "SetProvider",
        json!({"provider_id":"ollama"})
    )
    .await
    .is_err());
    assert_eq!(
        engine.get_state("tched_router").await.unwrap(),
        before_failed_write
    );
    fixture.inner.lock().await.write_fail = false;
    let changed = dispatch(
        &engine,
        &caller,
        "SetProvider",
        json!({"provider_id":"ollama"}),
    )
    .await
    .unwrap();
    assert_eq!(changed["result"], json!({"selected_provider":"ollama"}));
    let selected: Value =
        serde_json::to_value(engine.get_state("tched_router").await.unwrap()).unwrap();
    assert_eq!(selected["selected_model"], "old-model");
    let changed_model = dispatch(&engine, &caller, "SetModel", json!({"model_id":"old-alt"}))
        .await
        .unwrap();
    assert_eq!(changed_model["result"], json!({"selected_model":"old-alt"}));

    // An unreadable committed store invalidates the caller's credential
    // projection at the first admission boundary, before any upstream mutation;
    // a process-local identity or router mirror cannot stand in for SHM.
    let before_publication_failure = engine.get_state("tched_router").await.unwrap();
    let before_shm = op_core::projection_shm::read_projection_bytes("tched_router").unwrap();
    let blocked = root.path().join("blocked-state");
    std::fs::write(&blocked, b"not a directory").unwrap();
    std::env::set_var("OP_SHM_STATE_DIR", &blocked);
    let error = dispatch(
        &engine,
        &caller,
        "SetProvider",
        json!({"provider_id":"openai"}),
    )
    .await
    .unwrap_err();
    let error_chain = format!("{error:#}");
    assert!(
        error_chain.contains("identity credential projection is unavailable"),
        "unexpected failure: {error_chain}"
    );
    assert_eq!(fixture.inner.lock().await.provider, "ollama.main");
    assert_eq!(
        engine.get_state("tched_router").await.unwrap(),
        before_publication_failure
    );
    std::env::set_var("OP_SHM_STATE_DIR", root.path().join("state"));
    assert_eq!(
        op_core::projection_shm::read_projection_bytes("tched_router").unwrap(),
        before_shm
    );

    // Separately fault SHM only after the native PATCH has applied. This must
    // report an indeterminate completed-provider outcome, retain the old
    // committed projection/mirror, and reconcile from native observation once
    // the authoritative projection path is restored.
    let before_post_effect = engine.get_state("tched_router").await.unwrap();
    let before_post_effect_shm =
        op_core::projection_shm::read_projection_bytes("tched_router").unwrap();
    let blocked_after_effect = root.path().join("blocked-after-native-effect");
    std::fs::write(&blocked_after_effect, b"not a directory").unwrap();
    fixture.inner.lock().await.projection_block_after_patch = Some(blocked_after_effect);
    let error = dispatch(
        &engine,
        &caller,
        "SetProvider",
        json!({"provider_id":"openai"}),
    )
    .await
    .unwrap_err();
    let error_chain = format!("{error:#}");
    assert!(
        error_chain.contains(
            "native tched_router selection completed but its authoritative observation could not be published"
        ),
        "unexpected post-effect failure: {error_chain}"
    );
    assert_eq!(fixture.inner.lock().await.provider, "openai.main");
    assert_eq!(
        engine.get_state("tched_router").await.unwrap(),
        before_post_effect
    );
    std::env::set_var("OP_SHM_STATE_DIR", root.path().join("state"));
    assert_eq!(
        op_core::projection_shm::read_projection_bytes("tched_router").unwrap(),
        before_post_effect_shm
    );
    let reconciled = dispatch(&engine, &caller, "GetState", json!({}))
        .await
        .unwrap();
    assert_eq!(reconciled["result"]["state"]["selected_provider"], "openai");
    assert_eq!(
        engine.get_state("tched_router").await.unwrap()["selected_provider"],
        "openai"
    );
    for (method, field, model) in [
        ("SetObfuscationModel", "obfuscation", "new-model"),
        ("SetQdrantRetrievalModel", "qdrant_retrieval", "old-alt"),
        ("SetCozoRetrievalModel", "cozo_retrieval", "old-model"),
    ] {
        let before: Value = serde_json::from_slice(
            &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(),
        )
        .unwrap();
        let output = dispatch(&engine, &caller, method, json!({"model_id":model}))
            .await
            .unwrap();
        assert_eq!(output["result"][field], model);
        let after: Value = serde_json::from_slice(
            &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(),
        )
        .unwrap();
        assert_eq!(after["model_assignments"][field], model);
        for (key, value) in before["model_assignments"].as_object().unwrap() {
            if key != field {
                assert_eq!(&after["model_assignments"][key], value);
            }
        }
    }
    let routes = dispatch(&engine, &caller, "GetModelRoutes", json!({}))
        .await
        .unwrap();
    assert!(routes["result"]["model_routes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|route| route["provider"] == "openai" && route["model"] == "new-model"));
    let providers = dispatch(&engine, &caller, "GetProviderCatalog", json!({}))
        .await
        .unwrap();
    assert!(providers["result"]["providers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|provider| provider["id"] == "ollama"));
    let tools = dispatch(&engine, &caller, "GetTools", json!({}))
        .await
        .unwrap();
    let tool = &tools["result"]["tools"][0];
    assert_eq!(tool["name"], "fixture_lookup");
    assert_eq!(tool["description"], "Look up a fixture record");
    assert_eq!(tool["parameters"]["required"], json!(["id"]));
    let reconciled_shm: Value = serde_json::from_slice(
        &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(),
    )
    .unwrap();
    assert_eq!(reconciled_shm["selected_provider"], "openai");
    assert_eq!(
        reconciled_shm["model_assignments"]["ovs_routing"],
        "old-model"
    );
    assert_eq!(
        reconciled_shm["model_assignments"]["vectorization"],
        "old-alt"
    );

    // Every unary LLM facade must execute the native router owner. Check
    // selection effects and their canonical SHM readback as well as the reads.
    let selected = dispatch_llm(
        &engine,
        &caller,
        "set_provider",
        json!({"provider_id":"ollama"}),
    )
    .await
    .unwrap();
    assert_eq!(selected["result"]["selected_provider"], "ollama");
    assert_eq!(fixture.inner.lock().await.provider, "ollama.main");
    let selection = dispatch_llm(
        &engine,
        &caller,
        "update_config",
        json!({"provider_id":"openai","model_id":"new-model"}),
    )
    .await
    .unwrap();
    assert_eq!(
        selection["result"],
        json!({"selected_provider":"openai","selected_model":"new-model"})
    );
    let providers = dispatch_llm(&engine, &caller, "list_providers", json!({}))
        .await
        .unwrap();
    assert_eq!(
        providers["result"],
        dispatch(&engine, &caller, "ListProviders", json!({}))
            .await
            .unwrap()["result"]
    );
    assert!(providers["result"]["providers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == "openai"));
    let provider = dispatch_llm(&engine, &caller, "get_provider", json!({}))
        .await
        .unwrap();
    assert_eq!(provider["result"]["selected_provider"], "openai");
    assert_eq!(provider["result"]["provider"]["id"], "openai");
    let models = dispatch_llm(
        &engine,
        &caller,
        "list_models",
        json!({"provider":"openai"}),
    )
    .await
    .unwrap();
    assert_eq!(
        models["result"],
        dispatch(&engine, &caller, "ListModels", json!({"provider":"openai"}))
            .await
            .unwrap()["result"]
    );
    assert!(models["result"]["model_routes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|m| m["model"] == "new-model"));
    let config = dispatch_llm(&engine, &caller, "get_config", json!({}))
        .await
        .unwrap();
    assert_eq!(config["result"]["config"], selection["result"]);
    let chats = fixture.inner.lock().await.chats;
    let generated = dispatch_llm(
        &engine,
        &caller,
        "generate",
        json!({"message":"facade fixture request"}),
    )
    .await
    .unwrap();
    assert_eq!(generated["result"]["content"], "fixture");
    assert_eq!(generated["result"]["provider"], "openai");
    assert_eq!(generated["result"]["model"], "new-model");
    assert_eq!(fixture.inner.lock().await.chats, chats + 1);
    let after_facades: Value = serde_json::from_slice(
        &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(),
    )
    .unwrap();
    assert_eq!(after_facades["selected_provider"], "openai");
    assert_eq!(after_facades["selected_model"], "new-model");
    assert_eq!(
        after_facades["model_assignments"],
        reconciled_shm["model_assignments"]
    );

    // Exercise EVERY generated native getter through authenticated canonical
    // dispatch, native HTTP observation, MutationEngine commit and SHM readback.
    // Declaring/mapping a method is not counted as execution.
    fixture.inner.lock().await.complete_native_config = true;
    let complete = dispatch(&engine, &caller, "GetConfig", json!({}))
        .await
        .unwrap();
    let native = complete["result"]["config"].clone();
    assert_eq!(
        native["mcp"]["servers"][0]["headers"]["Authorization"],
        "***MASKED***"
    );
    assert!(!native.to_string().contains("fixture-private-mcp-token"));
    use op_plugins::state_plugins::tched_router_config_surface::{
        config_method_backing, ConfigMethodBacking,
    };
    let mut tested = std::collections::BTreeSet::new();
    for method in schema.methods.keys() {
        let expected = match config_method_backing(method) {
            Some(ConfigMethodBacking::WholeNativeConfig) => native.clone(),
            Some(ConfigMethodBacking::NativeSection(section)) => native
                .get(section)
                .unwrap_or_else(|| panic!("native fixture lacks {section} for {method}"))
                .clone(),
            _ => continue,
        };
        let requests = fixture.requests.load(Ordering::SeqCst);
        let response = dispatch(&engine, &caller, method, json!({}))
            .await
            .unwrap_or_else(|error| panic!("{method} failed canonical execution: {error:#}"));
        assert_eq!(response["result"], json!({"config":expected}), "{method}");
        assert!(
            fixture.requests.load(Ordering::SeqCst) > requests,
            "{method} never reached native HTTP"
        );
        let committed: Value = serde_json::from_slice(
            &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(),
        )
        .unwrap();
        assert_eq!(
            committed["native_config"], native,
            "{method} did not commit complete native state"
        );
        assert_eq!(
            committed["model_assignments"], reconciled_shm["model_assignments"],
            "{method} lost sibling state"
        );
        assert!(tested.insert(method.clone()));
    }
    assert_eq!(
        tested.len(),
        82,
        "the native getter inventory changed; review coverage"
    );
    let contract = dispatch(&engine, &caller, "GetConfigSchema", json!({}))
        .await
        .unwrap();
    assert_eq!(
        contract["result"]["config_schema"],
        *op_plugins::state_plugins::tched_router_contract::document()
    );

    // Local descriptors have a distinct committed-state backing. Seed a
    // typed fixture observation through MutationEngine, then require each
    // reader to use those SHM values without fabricating native observations.
    // This tests configured reads; it does not establish a production producer
    // for every local descriptor.
    let mut local = serde_json::to_value(
        op_plugins::state_plugins::tched_router::TchedRouterPlugin::current_state(),
    )
    .unwrap();
    let observed: Value = serde_json::from_slice(
        &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(),
    )
    .unwrap();
    local
        .as_object_mut()
        .unwrap()
        .extend(observed.as_object().unwrap().clone());
    engine
        .process_authoritative_change(
            "tched_router".into(),
            "/org/opdbus/v1/plugins/tched_router".into(),
            op_grpc_bridge::mutation_engine::ChangeType::ObjectAdded,
            None,
            None,
            simd_json::serde::to_owned_value(&local).unwrap(),
            vec!["fixture-descriptors".into()],
            "fixture-observation-owner".into(),
            None,
            op_grpc_bridge::mutation_engine::ChangeSource::Internal,
        )
        .await
        .unwrap();
    let committed: Value = serde_json::from_slice(
        &op_core::projection_shm::read_projection_bytes("tched_router").unwrap(),
    )
    .unwrap();
    let requests = fixture.requests.load(Ordering::SeqCst);
    let mut local_count = 0;
    for method in schema.methods.keys() {
        if let Some(ConfigMethodBacking::CommittedPluginStateSection(field)) =
            config_method_backing(method)
        {
            let output = dispatch(&engine, &caller, method, json!({})).await.unwrap();
            assert_eq!(
                output["result"],
                json!({"config":committed[field]}),
                "{method}"
            );
            local_count += 1;
        }
    }
    assert_eq!(local_count, 6);
    for (method, field) in [
        ("GetRouter", "router"),
        ("ListUiSurfaces", "ui_surfaces"),
        ("GetStructuredOutput", "structured_output"),
    ] {
        let output = dispatch(&engine, &caller, method, json!({})).await.unwrap();
        assert_eq!(output["result"][field], committed[field], "{method}");
    }
    assert_eq!(fixture.requests.load(Ordering::SeqCst), requests);
}
