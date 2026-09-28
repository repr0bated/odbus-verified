use super::*;
use crate::human_principal_dispatch::tests::{
    pk, register, temp_cozo, temp_shm_with_schemas, test_engine,
};
use crate::identity_sled_dispatch::{IdentitySledStore, tests::write_identity};
use op_llm::antigravity::record_usage_with_documents;
use op_llm::provider::TokenUsage;
use op_plugins::state_plugins::{
    antigravity::{AntigravityPlugin, AntigravityState},
    antigravity_chat::{AntigravityChatPlugin, AntigravityChatState},
    human_principal::HumanPrincipalPlugin,
    identity_sled::IdentitySledPlugin,
    runtime_documents::RuntimeDocuments,
};
use op_state::StatePlugin;
use op_state_store::PluginSchema;
use serde_json::{Value, json};
use std::sync::Arc;

struct GrantsEnvironment(Option<std::ffi::OsString>);
impl Drop for GrantsEnvironment {
    fn drop(&mut self) {
        match self.0.take() {
            // FIXME: Audit that the environment access only happens in single-threaded code.
            Some(path) => unsafe { std::env::set_var("OP_GRANTS_PATH", path) },
            // FIXME: Audit that the environment access only happens in single-threaded code.
            None => unsafe { std::env::remove_var("OP_GRANTS_PATH") },
        }
    }
}

struct Fixture {
    engine: Arc<MutationEngine>,
    antigravity: PluginSchema,
    chat: PluginSchema,
    principal: String,
    session: String,
    genesis: String,
    documents: Arc<RuntimeDocuments>,
    _grants_environment: GrantsEnvironment,
    _root: tempfile::TempDir,
    _shm: crate::human_principal_dispatch::tests::TempShm,
    _cozo: crate::human_principal_dispatch::tests::TempCozo,
}

impl Fixture {
    async fn new() -> Self {
        let cozo = temp_cozo();
        let antigravity = AntigravityPlugin::new().schema().unwrap();
        let chat = AntigravityChatPlugin::new().schema().unwrap();
        let shm = temp_shm_with_schemas(&[
            HumanPrincipalPlugin::new().schema().unwrap(),
            IdentitySledPlugin::new().schema().unwrap(),
            antigravity.clone(),
            chat.clone(),
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
        engine
            .antigravity_documents
            .set(documents.clone())
            .unwrap_or_else(|_| panic!("install isolated Antigravity documents"));
        let public_key = pk(0xD7);
        register(&public_key, "antigravity-fixture").await.unwrap();
        let identity = write_identity(&engine, &public_key).await.unwrap();
        let principal = op_identity::session::derive_principal_id(&public_key);
        let root = tempfile::tempdir().unwrap();
        let grants = root.path().join("grants.json");
        let grants_environment = GrantsEnvironment(std::env::var_os("OP_GRANTS_PATH"));
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_GRANTS_PATH", &grants) };
        let capabilities = antigravity
            .methods
            .values()
            .chain(chat.methods.values())
            .filter_map(|method| method.required_capability.clone())
            .collect::<std::collections::BTreeSet<_>>();
        std::fs::write(
            &grants,
            serde_json::to_vec(&json!({
                (principal.clone()): {"capabilities": capabilities}
            }))
            .unwrap(),
        )
        .unwrap();
        Self {
            engine,
            antigravity,
            chat,
            principal,
            session: identity.session_id,
            genesis: identity.genesis.unwrap(),
            documents,
            _grants_environment: grants_environment,
            _root: root,
            _shm: shm,
            _cozo: cozo,
        }
    }

    async fn call(
        &self,
        plugin: &str,
        schema: &PluginSchema,
        method: &str,
        arguments: Value,
    ) -> anyhow::Result<Value> {
        let declaration = &schema.methods[method];
        let input_schema = serde_json::to_value(&declaration.args)?;
        let input_validator = jsonschema::Validator::new(&input_schema)
            .map_err(|error| anyhow::anyhow!("invalid {plugin}.{method} input schema: {error}"))?;
        input_validator.validate(&arguments).map_err(|error| {
            anyhow::anyhow!("{plugin}.{method} fixture input violates its schema: {error}")
        })?;
        let response = self
            .engine
            .dispatch_method_call_with_identity(
                plugin,
                method,
                &arguments.to_string(),
                declaration.required_capability.as_deref(),
                &self.principal,
                Some(&self.session),
                Some(&self.genesis),
            )
            .await?;
        let output = response["result"].clone();
        let output_schema = serde_json::to_value(
            declaration
                .returns
                .as_ref()
                .expect("Antigravity methods have typed outputs"),
        )?;
        let output_validator = jsonschema::Validator::new(&output_schema)
            .map_err(|error| anyhow::anyhow!("invalid {plugin}.{method} output schema: {error}"))?;
        output_validator.validate(&output).map_err(|error| {
            anyhow::anyhow!("{plugin}.{method} output violates its schema: {error}")
        })?;
        let committed = op_core::projection_shm::read_projection_bytes(plugin)
            .ok_or_else(|| anyhow::anyhow!("{plugin} SHM state is absent"))?;
        match plugin {
            "antigravity" => {
                serde_json::from_slice::<AntigravityState>(&committed)?;
            }
            "antigravity_chat" => {
                serde_json::from_slice::<AntigravityChatState>(&committed)?;
            }
            _ => anyhow::bail!("unexpected fixture plugin"),
        }
        Ok(output)
    }
}

#[tokio::test]
async fn six_methods_pass_canonical_admission_native_refresh_and_shm_publication() {
    let fixture = Fixture::new().await;
    let received = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let requests = received.clone();
    let token_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let token_uri = format!("http://{}/token", token_listener.local_addr().unwrap());
    let app = axum::Router::new().route("/token", axum::routing::post(
        move |axum::Json(payload): axum::Json<Value>| {
            let requests = requests.clone();
            async move {
                requests.lock().unwrap().push(payload);
                axum::Json(json!({"access_token":"PRIVATE_ACCESS", "token_type":"Bearer", "expires_in":3600}))
            }
        }
    ));
    let token_server = tokio::spawn(async move { axum::serve(token_listener, app).await.unwrap() });
    // The SDK starts its own refresh worker when credentials are constructed.
    // Prove rejection before that worker exists, so unrelated SDK cache work
    // cannot be mistaken for a side effect of the rejected canonical call.
    assert!(
        fixture
            .engine
            .dispatch_method_call_with_identity(
                "antigravity_chat",
                "refresh_token",
                "{}",
                fixture.chat.methods["refresh_token"]
                    .required_capability
                    .as_deref(),
                &fixture.principal,
                None,
                None,
            )
            .await
            .is_err()
    );
    assert!(received.lock().unwrap().is_empty());
    assert!(fixture.engine.antigravity_credentials.get().is_none());
    assert!(op_core::projection_shm::read_projection_bytes("antigravity_chat").is_none());
    let scopes = vec!["scope-a".to_owned()];
    let sdk = google_cloud_auth::credentials::user_account::Builder::new(json!({
        "type":"authorized_user", "client_id":"PRIVATE_CLIENT",
        "client_secret":"PRIVATE_SECRET", "refresh_token":"PRIVATE_REFRESH"
    }))
    .with_token_uri(token_uri)
    .with_scopes(scopes.clone())
    .build_access_token_credentials()
    .unwrap();
    assert!(
        fixture
            .engine
            .antigravity_credentials
            .set(Arc::new(
                op_llm::antigravity::GoogleCredentialOwner::from_credentials(sdk, scopes)
            ))
            .is_ok()
    );

    let configured = fixture
        .call(
            "antigravity_chat",
            &fixture.chat,
            "configure",
            json!({
                "expected_revision": 0,
                "provider": "gemini",
                "base_url": "https://generativelanguage.googleapis.com/v1beta",
                "selected_model": "fixture-model"
            }),
        )
        .await
        .unwrap();
    assert_eq!(configured["revision"], 1);

    let safety = fixture
        .call(
            "antigravity",
            &fixture.antigravity,
            "configure_safety",
            json!({
                "expected_revision": 1,
                "settings":[{
                    "category":"HARM_CATEGORY_HARASSMENT",
                    "threshold":"BLOCK_ONLY_HIGH"
                }]
            }),
        )
        .await
        .unwrap();
    assert_eq!(safety["revision"], 2);
    let refreshed = fixture
        .call(
            "antigravity_chat",
            &fixture.chat,
            "refresh_token",
            json!({}),
        )
        .await
        .unwrap();
    assert_eq!(refreshed["token"]["token_available"], true);
    assert!(!refreshed.to_string().contains("PRIVATE_"));
    let requests = received.lock().unwrap().clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["grant_type"], "refresh_token");
    assert_eq!(requests[0]["client_id"], "PRIVATE_CLIENT");
    assert_eq!(requests[0]["client_secret"], "PRIVATE_SECRET");
    assert_eq!(requests[0]["refresh_token"], "PRIVATE_REFRESH");
    assert!(
        !String::from_utf8(
            op_core::projection_shm::read_projection_bytes("antigravity_chat").unwrap()
        )
        .unwrap()
        .contains("PRIVATE_")
    );
    record_usage_with_documents(
        fixture.documents.clone(),
        "fixture-model".into(),
        TokenUsage {
            prompt_tokens: 7,
            completion_tokens: 3,
            total_tokens: 10,
        },
    )
    .await
    .unwrap();

    let auth = fixture
        .call(
            "antigravity",
            &fixture.antigravity,
            "get_auth_status",
            json!({}),
        )
        .await
        .unwrap();
    let auth_wire = serde_json::to_string(&auth).unwrap();
    assert!(!auth_wire.contains("access_token"));
    assert!(!auth_wire.contains("refresh_token"));

    let usage = fixture
        .call(
            "antigravity",
            &fixture.antigravity,
            "get_usage_report",
            json!({"model":"fixture-model"}),
        )
        .await
        .unwrap();
    assert_eq!(usage["usage"]["total_tokens"], 10);
    assert!(usage["usage"].get("quota").is_none());
    assert!(usage["usage"].get("billing").is_none());

    let reserved = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let bridge = format!("http://{}", reserved.local_addr().unwrap());
    drop(reserved);
    fixture
        .call(
            "antigravity_chat",
            &fixture.chat,
            "configure",
            json!({
                "expected_revision": 2,
                "provider": "openai_compatible",
                "base_url": bridge,
                "selected_model": "bridge-model"
            }),
        )
        .await
        .unwrap();
    let status = fixture
        .call(
            "antigravity_chat",
            &fixture.chat,
            "get_bridge_status",
            json!({}),
        )
        .await
        .unwrap();
    assert_eq!(status["bridge"]["reachable"], false);
    assert_eq!(status["bridge"]["http_status"], 0);
    assert!(status["bridge"].get("version").is_none());
    token_server.abort();
}
