//! Canonical admission, pinned catalog metadata, and durable policy recovery.
mod support;

use op_grpc_bridge::MutationEngine;
use op_plugins::state_plugins::json_render::JsonRenderPlugin;
use op_state::StatePlugin;
use op_state_store::{ChainConfig, EventChain, PluginSchema};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::RwLock;

fn engine() -> MutationEngine {
    MutationEngine::new(
        Arc::new(RwLock::new(EventChain::new(ChainConfig::default()))),
        Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new()),
    )
}

async fn call(
    engine: &MutationEngine,
    schema: &PluginSchema,
    method: &str,
    args: Value,
) -> anyhow::Result<Value> {
    let response = engine
        .dispatch_method_call(
            "json_render",
            method,
            &serde_json::to_string(&args)?,
            schema.methods[method].required_capability.as_deref(),
            "fixture-principal",
        )
        .await?;
    let result = response["result"].clone();
    let contract = serde_json::to_value(&schema.methods[method].returns)?;
    jsonschema::validate(&contract, &result)
        .map_err(|error| anyhow::anyhow!("response violates contract: {error}"))?;
    Ok(result)
}

#[tokio::test]
async fn current_seal_drives_catalog_and_policy_survives_engine_recreation() {
    let mut schema = JsonRenderPlugin::new().schema().unwrap();
    let scope = support::CatalogScope::new(&[schema.clone()]);
    let engine = engine();
    let before = call(&engine, &schema, "list_components", json!({}))
        .await
        .unwrap();
    let mut components = before["components"].clone();
    components[0]["description"] = json!("changed by the fixture sealer");
    schema.fields.get_mut("components").unwrap().default =
        Some(simd_json::serde::to_owned_value(&components).unwrap());
    let mut sealer = op_blob::BlobStore::open(scope.root().join("blobs")).unwrap();
    sealer
        .write(&op_blob::blobify_plugin_schema(
            "json_render",
            schema.clone(),
        ))
        .unwrap();
    let updated = call(&engine, &schema, "list_components", json!({}))
        .await
        .unwrap();
    assert_eq!(
        updated["components"], components,
        "a live metadata reseal must not use an old adapter snapshot"
    );

    let config = call(&engine, &schema, "get_config", json!({}))
        .await
        .unwrap();
    let mut policy = config["config"].clone();
    policy["expose_catalog"] = json!(false);
    let receipt = call(&engine, &schema, "set_config", json!({"config":policy}))
        .await
        .unwrap();
    assert_eq!(receipt["config"], policy);
    let error = call(&engine, &schema, "list_components", json!({}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("catalog exposure is disabled"));
    let projected = serde_json::to_value(engine.get_state("json_render").await.unwrap()).unwrap();
    assert_eq!(projected["config"], policy);
    assert_eq!(projected["components"], components);

    let recovered = self::engine();
    recovered.seed_missing_plugin_projections().await.unwrap();
    let restored = serde_json::to_value(recovered.get_state("json_render").await.unwrap()).unwrap();
    assert_eq!(restored["config"], policy);
    assert_eq!(restored["components"], components);
    assert_eq!(
        call(&recovered, &schema, "get_config", json!({}))
            .await
            .unwrap()["config"],
        policy
    );
}
