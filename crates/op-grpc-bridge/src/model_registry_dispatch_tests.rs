use std::{
    collections::HashMap,
    os::unix::fs::{MetadataExt, PermissionsExt},
    sync::Mutex,
};

use op_plugins::state_plugins::model_registry::{bounded_error, ModelRef};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

/// Canned model lists per provider; records the key each call received.
struct FakeLister {
    lists: HashMap<String, std::result::Result<Vec<&'static str>, &'static str>>,
    keys: Mutex<Vec<(String, Option<String>)>>,
}

impl FakeLister {
    fn new(lists: &[(&str, std::result::Result<Vec<&'static str>, &'static str>)]) -> Self {
        Self {
            lists: lists
                .iter()
                .map(|(id, list)| (id.to_string(), list.clone()))
                .collect(),
            keys: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl ModelLister for FakeLister {
    async fn list(
        &self,
        provider: &ProviderConfig,
        key: Option<&str>,
    ) -> Result<Vec<ObservedModel>> {
        self.keys
            .lock()
            .unwrap()
            .push((provider.id.clone(), key.map(str::to_string)));
        match self.lists.get(&provider.id) {
            Some(Ok(ids)) => Ok(ids
                .iter()
                .map(|id| ObservedModel {
                    model_id: id.to_string(),
                    name: Some(format!("{id} model")),
                    context_length: None,
                    prompt_price: None,
                    completion_price: None,
                })
                .collect()),
            Some(Err(reason)) => bail!("{reason}"),
            None => bail!("no canned list"),
        }
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    env: Environment,
    store: Arc<CozoGraphShuttle>,
}

const PROVIDERS: &str = r#"
[[providers]]
id = "openrouter"
name = "OpenRouter"
models_url = "https://openrouter.ai/api/v1/models"
format = "openai"
auth = "none"

[[providers]]
id = "opencode_go"
name = "OpenCode Go"
models_url = "https://opencode.ai/zen/go/v1/models"
format = "openai"
auth = "bearer"
key_file = "/etc/op-dbus/secrets/opencode-api-key"

[[providers]]
id = "cheaper_inference"
name = "Cheaper Inference"
models_url = "https://api.cheaperinference.com/v1/models"
format = "openai"
auth = "bearer"
key_file = "/etc/op-dbus/secrets/cheaper-inference-api-key"

[[providers]]
id = "parked"
name = "Parked"
models_url = "https://parked.test/v1/models"
format = "openai"
auth = "none"
enabled = false
"#;

fn write(path: &Path, text: &str, mode: u32) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

fn fixture(providers: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let providers_path = dir.path().join("model-providers.toml");
    write(&providers_path, providers, 0o644);
    let secrets_dir = dir.path().join("secrets");
    std::fs::create_dir(&secrets_dir).unwrap();
    write(&secrets_dir.join("opencode-api-key"), "go-key-123\n", 0o600);
    let required_uid = std::fs::metadata(&providers_path).unwrap().uid();
    let store = CozoGraphShuttle::new_in_memory().unwrap();
    store.initialize_model_registry_store().unwrap();
    Fixture {
        _dir: dir,
        env: Environment {
            providers_path,
            secrets_dir,
            required_uid,
        },
        store: Arc::new(store),
    }
}

fn by_id<'a>(output: &'a RefreshOutput, id: &str) -> &'a ProviderRefresh {
    output
        .providers
        .iter()
        .find(|provider| provider.provider_id == id)
        .unwrap_or_else(|| panic!("{id} missing from {output:?}"))
}

#[tokio::test]
async fn refresh_folds_every_enabled_provider_into_the_catalog() {
    let fixture = fixture(PROVIDERS);
    let lister = FakeLister::new(&[
        (
            "openrouter",
            Ok(vec!["anthropic/claude-fable-5-1", "openai/gpt-6-luna"]),
        ),
        ("opencode_go", Ok(vec!["glm-5.3-flash"])),
        ("cheaper_inference", Ok(vec!["never-called"])),
    ]);
    let output = refresh(
        &fixture.env,
        &lister,
        fixture.store.clone(),
        RefreshInput::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        output.providers.len(),
        3,
        "the disabled provider is skipped"
    );
    assert_eq!(by_id(&output, "openrouter").available_models, 2);
    assert_eq!(by_id(&output, "opencode_go").status, "ok");
    // Cheaper Inference has no key file here: an error, never a guess.
    let cheaper = by_id(&output, "cheaper_inference");
    assert_eq!(cheaper.status, "error");
    assert!(cheaper
        .error
        .as_deref()
        .unwrap()
        .contains("key file is unavailable"));
    assert!(output.revision >= 3);

    let keys = lister.keys.lock().unwrap().clone();
    assert!(keys.contains(&("openrouter".into(), None)));
    assert!(keys.contains(&("opencode_go".into(), Some("go-key-123".into()))));
    assert!(!keys
        .iter()
        .any(|(id, _)| id == "cheaper_inference" || id == "parked"));

    let providers = list_providers(&fixture.env, fixture.store.clone())
        .await
        .unwrap();
    let parked = providers
        .providers
        .iter()
        .find(|p| p.provider_id == "parked")
        .unwrap();
    assert_eq!((parked.enabled, parked.status.as_str()), (false, "never"));
    let go = providers
        .providers
        .iter()
        .find(|p| p.provider_id == "opencode_go")
        .unwrap();
    assert_eq!(go.available_models, 1);
    assert!(go.succeeded_at.is_some());

    let models = list_models(
        &fixture.env,
        fixture.store.clone(),
        ListModelsInput::default(),
    )
    .await
    .unwrap();
    let ids: Vec<_> = models
        .models
        .iter()
        .map(|m| (m.provider_id.as_str(), m.model_id.as_str()))
        .collect();
    assert_eq!(
        ids,
        [
            ("opencode_go", "glm-5.3-flash"),
            ("openrouter", "anthropic/claude-fable-5-1"),
            ("openrouter", "openai/gpt-6-luna"),
        ]
    );
}

#[tokio::test]
async fn later_refreshes_add_drop_and_retire() {
    let fixture = fixture(PROVIDERS);
    let first = FakeLister::new(&[
        ("openrouter", Ok(vec!["a", "b"])),
        ("opencode_go", Ok(vec!["glm"])),
        ("cheaper_inference", Err("unused")),
    ]);
    refresh(
        &fixture.env,
        &first,
        fixture.store.clone(),
        RefreshInput::default(),
    )
    .await
    .unwrap();

    // OpenRouter changed its list; OpenCode Go is down and keeps its list.
    let second = FakeLister::new(&[
        ("openrouter", Ok(vec!["b", "c"])),
        ("opencode_go", Err("HTTP 503")),
    ]);
    let output = refresh(
        &fixture.env,
        &second,
        fixture.store.clone(),
        RefreshInput::default(),
    )
    .await
    .unwrap();
    let openrouter = by_id(&output, "openrouter");
    assert_eq!((openrouter.added, openrouter.dropped), (1, 1));
    let go = by_id(&output, "opencode_go");
    assert_eq!(go.status, "error");
    assert_eq!(go.error.as_deref(), Some("HTTP 503"));
    assert_eq!(go.available_models, 1);

    let unavailable = list_models(
        &fixture.env,
        fixture.store.clone(),
        ListModelsInput {
            provider_id: Some("openrouter".into()),
            include_unavailable: Some(true),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let a = unavailable
        .models
        .iter()
        .find(|m| m.model_id == "a")
        .unwrap();
    assert!(!a.available);

    // OpenRouter leaves the provider list: its models become unavailable.
    let without = PROVIDERS.replacen(
        "[[providers]]\nid = \"openrouter\"\nname = \"OpenRouter\"\nmodels_url = \"https://openrouter.ai/api/v1/models\"\nformat = \"openai\"\nauth = \"none\"\n",
        "",
        1,
    );
    assert_ne!(without, PROVIDERS);
    write(&fixture.env.providers_path, &without, 0o644);
    let third = FakeLister::new(&[("opencode_go", Ok(vec!["glm"]))]);
    let output = refresh(
        &fixture.env,
        &third,
        fixture.store.clone(),
        RefreshInput::default(),
    )
    .await
    .unwrap();
    let removed = by_id(&output, "openrouter");
    assert_eq!((removed.status.as_str(), removed.dropped), ("removed", 2));
    let providers = list_providers(&fixture.env, fixture.store.clone())
        .await
        .unwrap();
    let gone = providers
        .providers
        .iter()
        .find(|p| p.provider_id == "openrouter")
        .unwrap();
    assert!(!gone.configured);
    assert_eq!(gone.status, "removed");
    let available = list_models(
        &fixture.env,
        fixture.store.clone(),
        ListModelsInput {
            provider_id: Some("openrouter".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(available.models.is_empty());
}

#[tokio::test]
async fn single_provider_refresh_is_exact() {
    let fixture = fixture(PROVIDERS);
    let lister = FakeLister::new(&[("openrouter", Ok(vec!["x"]))]);
    let output = refresh(
        &fixture.env,
        &lister,
        fixture.store.clone(),
        RefreshInput {
            provider_id: Some("openrouter".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(output.providers.len(), 1);
    for (id, why) in [
        ("parked", "disabled"),
        ("unknown", "not in the provider list"),
        ("Bad-Id", "invalid provider_id"),
    ] {
        let error = refresh(
            &fixture.env,
            &lister,
            fixture.store.clone(),
            RefreshInput {
                provider_id: Some(id.into()),
            },
        )
        .await
        .unwrap_err();
        assert!(format!("{error:#}").contains(why), "{id}: {error:#}");
    }
}

#[tokio::test]
async fn provider_list_and_keys_must_be_protected() {
    let fixture = fixture(PROVIDERS);
    std::fs::set_permissions(
        &fixture.env.providers_path,
        std::fs::Permissions::from_mode(0o666),
    )
    .unwrap();
    let error = load_providers(&fixture.env).unwrap_err();
    assert!(format!("{error:#}").contains("group or world writable"));
    std::fs::set_permissions(
        &fixture.env.providers_path,
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let other = Environment {
        providers_path: fixture.env.providers_path.clone(),
        secrets_dir: fixture.env.secrets_dir.clone(),
        required_uid: fixture.env.required_uid + 1,
    };
    assert!(format!("{:#}", load_providers(&other).unwrap_err()).contains("owned by root"));

    // A readable key is refused, and the refresh records why.
    let key = fixture.env.secrets_dir.join("opencode-api-key");
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
    let lister = FakeLister::new(&[("opencode_go", Ok(vec!["glm"]))]);
    let output = refresh(
        &fixture.env,
        &lister,
        fixture.store.clone(),
        RefreshInput {
            provider_id: Some("opencode_go".into()),
        },
    )
    .await
    .unwrap();
    let go = by_id(&output, "opencode_go");
    assert_eq!(go.status, "error");
    assert!(go
        .error
        .as_deref()
        .unwrap()
        .contains("key file is unavailable"));
    assert!(
        lister.keys.lock().unwrap().is_empty(),
        "no call without a key"
    );

    // A symlinked key is refused too.
    std::fs::remove_file(&key).unwrap();
    let target = fixture.env.secrets_dir.join("real-key");
    write(&target, "k", 0o600);
    std::os::unix::fs::symlink(&target, &key).unwrap();
    assert!(read_key(&fixture.env, "/etc/op-dbus/secrets/opencode-api-key").is_err());
    assert!(read_key(&fixture.env, "/etc/op-dbus/secrets/real-key").is_ok());
    assert!(read_key(&fixture.env, "/etc/op-dbus/other/real-key").is_err());
}

#[tokio::test]
async fn list_models_pages_filters_and_searches() {
    let fixture = fixture(PROVIDERS);
    let ids: Vec<&'static str> = vec!["alpha-1", "alpha-2", "beta", "Gamma-Pro", "delta"];
    let lister = FakeLister::new(&[("openrouter", Ok(ids))]);
    refresh(
        &fixture.env,
        &lister,
        fixture.store.clone(),
        RefreshInput {
            provider_id: Some("openrouter".into()),
        },
    )
    .await
    .unwrap();
    let page = |after: Option<&str>, query: Option<&str>| {
        let store = fixture.store.clone();
        let input = ListModelsInput {
            after: after.map(|model_id| ModelRef {
                provider_id: "openrouter".into(),
                model_id: model_id.into(),
            }),
            query: query.map(str::to_string),
            limit: Some(2),
            ..Default::default()
        };
        list_models(&fixture.env, store, input)
    };
    let first = page(None, None).await.unwrap();
    assert_eq!(
        first
            .models
            .iter()
            .map(|m| m.model_id.as_str())
            .collect::<Vec<_>>(),
        ["Gamma-Pro", "alpha-1"]
    );
    assert!(first.more);
    let last = page(Some("beta"), None).await.unwrap();
    assert_eq!(last.models.len(), 1);
    assert!(!last.more);
    let search = page(None, Some("gamma")).await.unwrap();
    assert_eq!(search.models[0].model_id, "Gamma-Pro");
    let by_name = page(None, Some("ALPHA-2 MODEL")).await.unwrap();
    assert_eq!(by_name.models.len(), 1);
    assert!(list_models(
        &fixture.env,
        fixture.store.clone(),
        ListModelsInput {
            limit: Some(0),
            ..Default::default()
        }
    )
    .await
    .is_err());
}

/// Serve one HTTP response on loopback; resolves to the raw request.
async fn serve_once(response: String) -> (String, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/v1/models", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        while !request.windows(4).any(|w| w == b"\r\n\r\n") {
            let read = socket.read(&mut buffer).await.unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
        }
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.shutdown().await.ok();
        String::from_utf8_lossy(&request).to_string()
    });
    (url, handle)
}

fn http(status: &str, body: &str, extra: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n{extra}\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn fetch_sends_the_key_only_as_declared_and_bounds_the_answer() {
    let body = r#"{"data":[{"id":"glm-5.3-flash"},{"id":"grok-4.7"}]}"#;
    let (url, request) = serve_once(http("200 OK", body, "")).await;
    let models = fetch_models(
        &url,
        ModelListFormat::Openai,
        ProviderAuth::Bearer,
        Some("sk-1"),
    )
    .await
    .unwrap();
    assert_eq!(models.len(), 2);
    let request = request.await.unwrap().to_lowercase();
    assert!(request.starts_with("get /v1/models "));
    assert!(request.contains("authorization: bearer sk-1"));

    let (url, request) = serve_once(http("200 OK", r#"{"data":[]}"#, "")).await;
    fetch_models(
        &url,
        ModelListFormat::Anthropic,
        ProviderAuth::XApiKey,
        Some("ak"),
    )
    .await
    .unwrap();
    let request = request.await.unwrap().to_lowercase();
    assert!(request.starts_with("get /v1/models?limit=1000 "));
    assert!(request.contains("x-api-key: ak"));
    assert!(request.contains("anthropic-version: 2023-06-01"));
    assert!(!request.contains("authorization"));

    let (url, request) = serve_once(http("200 OK", r#"{"data":[]}"#, "")).await;
    fetch_models(&url, ModelListFormat::Openai, ProviderAuth::None, None)
        .await
        .unwrap();
    assert!(!request
        .await
        .unwrap()
        .to_lowercase()
        .contains("authorization"));

    let (url, _) = serve_once(http("401 Unauthorized", r#"{"error":"no"}"#, "")).await;
    let error = fetch_models(
        &url,
        ModelListFormat::Openai,
        ProviderAuth::Bearer,
        Some("k"),
    )
    .await
    .unwrap_err();
    assert_eq!(format!("{error:#}"), "HTTP 401");

    let (url, _) = serve_once(http(
        "302 Found",
        "",
        "location: http://127.0.0.1:9/elsewhere\r\n",
    ))
    .await;
    let error = fetch_models(
        &url,
        ModelListFormat::Openai,
        ProviderAuth::Bearer,
        Some("k"),
    )
    .await
    .unwrap_err();
    assert_eq!(
        format!("{error:#}"),
        "HTTP 302",
        "redirects are not followed"
    );

    let oversize = format!(
        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        MAX_MODELS_RESPONSE_BYTES + 1
    );
    let (url, _) = serve_once(oversize).await;
    let error = fetch_models(&url, ModelListFormat::Openai, ProviderAuth::None, None)
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("exceeds"));

    assert!(
        fetch_models(
            "http://127.0.0.1:9/models",
            ModelListFormat::Openai,
            ProviderAuth::Bearer,
            None
        )
        .await
        .is_err(),
        "a keyed provider is never called without its key"
    );
}

#[test]
fn stored_errors_never_carry_a_second_line() {
    assert_eq!(bounded_error("HTTP 503\nretry"), "HTTP 503 retry");
}

#[tokio::test]
async fn set_model_profile_records_only_an_offered_control_plane_profile() {
    let fixture = fixture(
        r#"
[[providers]]
id = "opencode_go"
name = "OpenCode Go"
models_url = "https://opencode.ai/zen/go/v1/models"
format = "openai"
auth = "bearer"
key_file = "/etc/op-dbus/secrets/opencode-api-key"
control_plane = { profile = "opencode.go_control_plane", alternatives = ["opencode.go_control_plane_responses"] }
"#,
    );
    let lister = FakeLister::new(&[("opencode_go", Ok(vec!["glm-5.3-flash"]))]);
    refresh(
        &fixture.env,
        &lister,
        fixture.store.clone(),
        RefreshInput::default(),
    )
    .await
    .unwrap();

    let refused = set_model_profile(
        &fixture.env,
        fixture.store.clone(),
        SetModelProfileInput {
            provider_id: "opencode_go".into(),
            model_id: "glm-5.3-flash".into(),
            profile: Some("openai.control_plane".into()),
        },
    )
    .await
    .unwrap_err();
    assert!(format!("{refused:#}").contains("not one this provider offers"));

    let recorded = set_model_profile(
        &fixture.env,
        fixture.store.clone(),
        SetModelProfileInput {
            provider_id: "opencode_go".into(),
            model_id: "glm-5.3-flash".into(),
            profile: Some("opencode.go_control_plane_responses".into()),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        recorded.model.recorded_profile.as_deref(),
        Some("opencode.go_control_plane_responses")
    );
    assert_eq!(
        recorded.model.control_plane_profile.as_deref(),
        Some("opencode.go_control_plane_responses")
    );

    refresh(
        &fixture.env,
        &lister,
        fixture.store.clone(),
        RefreshInput::default(),
    )
    .await
    .unwrap();
    let models = list_models(
        &fixture.env,
        fixture.store.clone(),
        ListModelsInput::default(),
    )
    .await
    .unwrap();
    assert_eq!(
        models.models[0].control_plane_profile.as_deref(),
        Some("opencode.go_control_plane_responses")
    );

    let cleared = set_model_profile(
        &fixture.env,
        fixture.store.clone(),
        SetModelProfileInput {
            provider_id: "opencode_go".into(),
            model_id: "glm-5.3-flash".into(),
            profile: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        cleared.model.control_plane_profile.as_deref(),
        Some("opencode.go_control_plane")
    );
    assert!(cleared.model.recorded_profile.is_none());
}
