mod support;

use op_grpc_bridge::mutation_engine::MutationEngine;
use op_state::StatePlugin;
use op_state_store::{ChainConfig, EventChain, PluginSchema};
use std::{net::TcpListener, sync::Arc};

struct Environment(Vec<(&'static str, Option<std::ffi::OsString>)>);

impl Environment {
    fn new() -> Self {
        Self(
            ["OP_GRANTS_PATH", "TCHED_ROUTER_RUNTIME_ENDPOINT"]
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect(),
        )
    }
}

impl Drop for Environment {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn facade() -> PluginSchema {
    op_plugins::state_plugins::large_language_model::LargeLanguageModelPlugin::new()
        .schema()
        .unwrap()
}

fn tched() -> PluginSchema {
    op_plugins::state_plugins::tched_router::TchedRouterPlugin::new()
        .schema()
        .unwrap()
}

fn engine() -> MutationEngine {
    MutationEngine::new(
        Arc::new(tokio::sync::RwLock::new(EventChain::new(
            ChainConfig::default(),
        ))),
        Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new()),
    )
}

async fn invoke(engine: &MutationEngine, capability: &str) -> anyhow::Result<serde_json::Value> {
    invoke_method(engine, "large_language_model", "list_providers", capability).await
}

async fn invoke_method(
    engine: &MutationEngine,
    plugin: &str,
    method: &str,
    capability: &str,
) -> anyhow::Result<serde_json::Value> {
    let genesis = "ab".repeat(32);
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        engine.dispatch_method_call_with_identity(
            plugin,
            method,
            "{}",
            Some(capability),
            "fixture-principal",
            Some("00000000-0000-4000-8000-000000000001"),
            Some(&genesis),
        ),
    )
    .await
    .map_err(|_| anyhow::anyhow!("canonical facade exceeded fixture deadline"))?
}

fn endpoint_without_server() -> TcpListener {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    std::env::set_var(
        "TCHED_ROUTER_RUNTIME_ENDPOINT",
        format!("http://{}", listener.local_addr().unwrap()),
    );
    listener
}

fn assert_no_http(listener: &TcpListener) {
    assert!(matches!(
        listener.accept(),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
    ));
}

#[tokio::test]
async fn nested_target_absence_or_drift_stops_before_http() {
    let target_capability = tched().methods["ListProviders"]
        .required_capability
        .clone()
        .unwrap();
    {
        let _catalog = support::CatalogScope::new(&[facade()]);
        let _environment = Environment::new();
        let listener = endpoint_without_server();
        let error = invoke(&engine(), &target_capability).await.unwrap_err();
        assert!(error.to_string().contains("tched_router"));
        assert_no_http(&listener);
    }
    {
        let mut stale = tched();
        stale.version = "0.0.0".into();
        let _catalog = support::CatalogScope::new(&[facade(), stale]);
        let _environment = Environment::new();
        let listener = endpoint_without_server();
        let error = invoke(&engine(), &target_capability).await.unwrap_err();
        assert!(error.to_string().contains("compiled/sealed"));
        assert_no_http(&listener);
    }
}

#[tokio::test]
async fn missing_grant_or_invalid_session_stops_before_http() {
    let _catalog = support::CatalogScope::new(&[facade(), tched()]);
    let _environment = Environment::new();
    let capability = tched().methods["ListProviders"]
        .required_capability
        .clone()
        .unwrap();
    let grants = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(grants.path(), b"{}").unwrap();
    std::env::set_var("OP_GRANTS_PATH", grants.path());
    let listener = endpoint_without_server();
    let error = invoke(&engine(), &capability).await.unwrap_err();
    assert!(error.to_string().contains("exact nested"));
    assert_no_http(&listener);

    std::fs::write(
        grants.path(),
        serde_json::to_vec(&serde_json::json!({
            "fixture-principal": {"capabilities": [capability.clone()]}
        }))
        .unwrap(),
    )
    .unwrap();
    let error = invoke(&engine(), &capability).await.unwrap_err();
    let error_chain = format!("{error:#}");
    assert!(
        error_chain.contains("identity credential projection is unavailable"),
        "unexpected failure: {error_chain}"
    );
    assert_no_http(&listener);
}
