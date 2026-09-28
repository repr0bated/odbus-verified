use std::collections::BTreeMap;
use std::sync::Arc;

use op_plugins::state_plugins::{
    hardware::HardwareState,
    incus::IncusState,
    json_render::JsonRenderState,
    net::NetworkConfig,
    procfs::ProcfsState,
    schema_renderer::SchemaRendererState,
    tched_router::{TchedRouterPlugin, TchedRouterState},
};
use op_plugins::{DefaultPluginRegistry, PluginSchema};
use op_state_store::MemoryStore;
use serde::Serialize;
use simd_json::OwnedValue;
use simd_json::prelude::*;

fn owned<T: Serialize>(value: &T) -> OwnedValue {
    simd_json::serde::to_owned_value(value).expect("typed state serializes")
}

async fn registered_schemas() -> BTreeMap<String, PluginSchema> {
    let registry = DefaultPluginRegistry::new(Arc::new(MemoryStore::new()));
    registry
        .load_all_plugins()
        .await
        .expect("registered plugins load")
        .into_iter()
        .filter_map(|plugin| {
            plugin
                .schema()
                .map(|schema| (plugin.name().to_string(), schema))
        })
        .collect()
}

fn assert_withheld(projected: &OwnedValue, keys: &[&str]) {
    let object = projected.as_object().expect("projected root is an object");
    for key in keys {
        assert!(!object.contains_key(*key), "private field {key} leaked");
    }
}

#[tokio::test]
async fn every_registered_public_policy_validates_against_its_own_schema() {
    let schemas = registered_schemas().await;
    let configured = schemas
        .values()
        .filter(|schema| !schema.public_state.paths.is_empty())
        .collect::<Vec<_>>();
    assert!(
        !configured.is_empty(),
        "expected explicit public state policies"
    );

    for schema in configured {
        schema
            .validate_public_state_policy()
            .unwrap_or_else(|error| {
                panic!(
                    "{} has an invalid public state policy: {error}",
                    schema.name
                )
            });
    }
}

#[tokio::test]
async fn typed_seed_and_default_states_project_for_each_annotated_plugin() {
    let schemas = registered_schemas().await;
    let fixtures = [
        ("procfs", owned(&ProcfsState::default())),
        ("hardware", owned(&HardwareState::default())),
        ("net", owned(&NetworkConfig::default())),
        (
            "incus",
            owned(&IncusState {
                instances: Vec::new(),
            }),
        ),
        ("json_render", owned(&JsonRenderState::default())),
        ("tched_router", owned(&TchedRouterPlugin::current_state())),
    ];

    for (plugin_id, state) in fixtures {
        let schema = &schemas[plugin_id];
        schema
            .project_public_state(&state)
            .unwrap_or_else(|error| panic!("{plugin_id} default state drifted: {error}"))
            .unwrap_or_else(|| panic!("{plugin_id} unexpectedly withholds its public state"));
    }

    // The stateless renderer has no observed state to publish.
    let renderer = &schemas["schema_renderer"];
    assert!(renderer.fields.is_empty());
    assert!(
        renderer
            .project_public_state(&owned(&SchemaRendererState::default()))
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn net_and_incus_public_views_exclude_private_topology_fields() {
    let schemas = registered_schemas().await;
    let net: NetworkConfig = serde_json::from_value(serde_json::json!({
        "interfaces": [{
            "name": "eth0",
            "type": "ethernet",
            "driver": "rtnetlink",
            "ports": ["uplink"],
            "l3_driver": "rtnetlink",
            "controller": "ssl:private.example:6653",
            "properties": {"secret-token": "do-not-publish"},
            "property_schema": ["secret-token"]
        }]
    }))
    .expect("typed network fixture");
    let projected = schemas["net"]
        .project_public_state(&owned(&net))
        .expect("valid net policy")
        .expect("net public view");
    let interface = &projected["interfaces"][0];
    assert_eq!(interface["name"], "eth0");
    assert_withheld(
        interface,
        &["l3_driver", "controller", "properties", "property_schema"],
    );

    let incus_state: IncusState = serde_json::from_value(serde_json::json!({
        "instances": [{
            "name": "private-session-name",
            "status": "Running",
            "type": "container",
            "profiles": ["private-profile"],
            "image": "private-image",
            "project": "private-project",
            "config": {"environment.SECRET": "do-not-publish"},
            "devices": []
        }]
    }))
    .expect("typed Incus fixture");
    let projected = schemas["incus"]
        .project_public_state(&owned(&incus_state))
        .expect("valid Incus policy")
        .expect("Incus public view");
    let instance = &projected["instances"][0];
    assert_eq!(instance["status"], "Running");
    assert_withheld(
        instance,
        &["name", "profiles", "image", "project", "config", "devices"],
    );
}

#[tokio::test]
async fn router_and_json_render_views_exclude_private_catalog_surfaces() {
    let schemas = registered_schemas().await;
    let router: TchedRouterState = TchedRouterPlugin::current_state();
    let projected = schemas["tched_router"]
        .project_public_state(&owned(&router))
        .expect("valid router policy")
        .expect("router public view");
    assert_eq!(
        projected["selected_model"].as_str(),
        Some(router.selected_model.as_str())
    );
    assert_withheld(
        &projected,
        &[
            "transport",
            "registration_service",
            "user_container",
            "identity_chain",
            "memory_namespaces",
            "privacy_policy",
            "providers",
            "models",
        ],
    );

    let render = JsonRenderState::default();
    let projected = schemas["json_render"]
        .project_public_state(&owned(&render))
        .expect("valid json-render policy")
        .expect("json-render public view");
    assert_eq!(projected["status"], "catalog_snapshot");
    assert_withheld(
        &projected,
        &[
            "spec_contract",
            "packages",
            "core_exports",
            "renderers",
            "components",
            "actions",
            "validation_checks",
            "directives",
        ],
    );
}
