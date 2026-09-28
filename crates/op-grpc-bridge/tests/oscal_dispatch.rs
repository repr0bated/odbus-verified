//! Sealed-catalog OSCAL dispatch and durable annotation integration.
mod support;

use std::sync::Arc;

use anyhow::{Result, anyhow};
use op_grpc_bridge::MutationEngine;
use op_plugins::state_plugins::oscal_subid_registry::OscalSubidRegistryPlugin;
use op_state::StatePlugin;
use op_state_store::{ChainConfig, EventChain, MethodDecl, PluginSchema, SideEffect};
use serde_json::{Value, json};
use tokio::sync::RwLock;

fn make_engine() -> MutationEngine {
    MutationEngine::new(
        Arc::new(RwLock::new(EventChain::new(ChainConfig::default()))),
        Arc::new(op_network::rovs_proxy::OvsdbDbusClient::new()),
    )
}

fn annotated_fixture_schema() -> PluginSchema {
    let mut schema = PluginSchema::builder("fixture")
        .string_field("status", false, "Fixture status")
        .subid("status", "obs.service.fixture.status@v1")
        .build();
    schema.methods.insert(
        "inspect".into(),
        MethodDecl {
            name: "InspectFixture".into(),
            args: simd_json::json!({"type":"object","additionalProperties":false}),
            returns: Some(simd_json::json!({
                "type":"object",
                "properties":{"status":{"type":"string"}},
                "required":["status"],
                "additionalProperties":false
            })),
            side_effect: SideEffect::Read,
            idempotent: true,
            required_capability: Some("fixture.read".into()),
            subid: "obs.service.fixture.inspect@v1".into(),
        },
    );
    schema
}

async fn call_typed(
    engine: &MutationEngine,
    schema: &PluginSchema,
    method: &str,
    args: Value,
) -> Result<Value> {
    let declaration = schema
        .methods
        .get(method)
        .ok_or_else(|| anyhow!("fixture method {method} is absent"))?;
    let response = engine
        .dispatch_method_call(
            "oscal_subid_registry",
            method,
            &serde_json::to_string(&args)?,
            declaration.required_capability.as_deref(),
            "principal:oscal-fixture",
        )
        .await?;
    let result = response["result"].clone();
    let output_schema = serde_json::to_value(
        declaration
            .returns
            .as_ref()
            .ok_or_else(|| anyhow!("fixture method {method} has no output contract"))?,
    )?;
    jsonschema::validate(&output_schema, &result)
        .map_err(|error| anyhow!("{method} output violates its sealed contract: {error}"))?;
    Ok(result)
}

#[tokio::test]
async fn sealed_dispatch_is_typed_scoped_and_durable_across_engine_recreation() {
    let oscal_schema = OscalSubidRegistryPlugin::new().schema().unwrap();
    let fixture_schema = annotated_fixture_schema();
    let _catalog = support::CatalogScope::new(&[oscal_schema.clone(), fixture_schema]);
    let engine = make_engine();
    let target = "obs.service.fixture.status@v1";

    let wrong_capability = engine
        .dispatch_method_call(
            "oscal_subid_registry",
            "register",
            &json!({
                "subid": target,
                "expected_revision": 0,
                "control_source": "NIST SP 800-53",
                "control_refs": ["CM-3"],
                "statement_refs": []
            })
            .to_string(),
            Some("oscal.read"),
            "principal:oscal-fixture",
        )
        .await;
    assert!(
        wrong_capability.is_err(),
        "wrong capability must fail closed"
    );

    let unknown = engine
        .dispatch_method_call(
            "oscal_subid_registry",
            "register",
            &json!({
                "subid": "obs.service.fixture.missing@v1",
                "expected_revision": 0,
                "control_source": null,
                "control_refs": [],
                "statement_refs": []
            })
            .to_string(),
            Some("oscal.invoke"),
            "principal:oscal-fixture",
        )
        .await;
    assert!(unknown.is_err(), "an annotation cannot mint a new subid");

    let registered = call_typed(
        &engine,
        &oscal_schema,
        "register",
        json!({
            "subid": target,
            "expected_revision": 0,
            "control_source": "NIST SP 800-53",
            "control_refs": ["CM-3", "AC-1"],
            "statement_refs": ["CM-3_smt.a"]
        }),
    )
    .await
    .unwrap();
    assert_eq!(registered["subid"], target);
    assert_eq!(registered["annotation_revision"], 1);
    assert_eq!(registered["binding_count"], 1);

    drop(engine);
    let reopened = make_engine();
    let lookup = call_typed(&reopened, &oscal_schema, "lookup", json!({"subid": target}))
        .await
        .unwrap();
    assert_eq!(lookup["entry"]["annotation"]["revision"], 1);
    assert_eq!(
        lookup["entry"]["annotation"]["control_refs"],
        json!(["AC-1", "CM-3"])
    );
    let encoded = lookup.to_string();
    assert!(!encoded.contains("principal:oscal-fixture"));
    assert!(!encoded.contains("oscal.invoke"));

    let listed = call_typed(
        &reopened,
        &oscal_schema,
        "list_by_category",
        json!({"category":"obs","limit":10}),
    )
    .await
    .unwrap();
    assert!(
        listed["entries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["subid"] == target)
    );

    let resolved = call_typed(
        &reopened,
        &oscal_schema,
        "resolve",
        json!({"plugin_id":"fixture","schema_method_key":"inspect"}),
    )
    .await
    .unwrap();
    assert_eq!(resolved["subid"], "obs.service.fixture.inspect@v1");
    assert_eq!(resolved["binding"]["schema_key"], "inspect");
    assert_eq!(resolved["binding"]["transport_name"], "InspectFixture");

    let exported = call_typed(
        &reopened,
        &oscal_schema,
        "export",
        json!({"plugin_id":"fixture","limit":10}),
    )
    .await
    .unwrap();
    assert!(exported["entries"].as_array().unwrap().iter().all(|entry| {
        entry["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|binding| binding["plugin_id"] == "fixture")
    }));
}
