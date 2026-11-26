//! Sealed-schema tests for the `openflow` plugin: the `flow` child type
//! (rovs-openflow tasks 5.1) and the removal of `ensure_fallback_normal` (6.3).

use super::*;
use crate::state_plugins::oscal_subid_registry_methods::{
    SealedCatalogSnapshot, SealedSchemaDocument,
};

const FLOW_FIELDS: [&str; 12] = [
    "bridge",
    "table",
    "priority",
    "cookie",
    "match",
    "actions",
    "flags",
    "idle_timeout",
    "hard_timeout",
    "packet_count",
    "byte_count",
    "duration_sec",
];

fn expected_subid(field: &str) -> String {
    format!("obs.network.openflow.flow.{field}@v1")
}

fn all_flow_fields() -> Vec<&'static str> {
    FLOW_FIELDS.to_vec()
}

/// Depth-first search for the JSON object that describes the flow child: the
/// first object whose `properties` contain `cookie` and `byte_count`.
fn find_flow_object(value: &serde_json::Value) -> Option<&serde_json::Value> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(props) = map.get("properties").and_then(|p| p.as_object()) {
                if props.contains_key("cookie") && props.contains_key("byte_count") {
                    return Some(value);
                }
            }
            map.values().find_map(find_flow_object)
        }
        serde_json::Value::Array(items) => items.iter().find_map(find_flow_object),
        _ => None,
    }
}

fn sealed_flow_object() -> serde_json::Value {
    let schema = openflow_schema();
    let field = schema
        .fields
        .get("flows")
        .expect("sealed openflow schema must carry the `flows` child field");
    let encoded = serde_json::to_value(field).expect("field serializes");
    find_flow_object(&encoded)
        .expect("flow child object schema must be present in the sealed field")
        .clone()
}

#[test]
fn flow_child_type_is_in_the_sealed_schema_with_every_field() {
    let flow = sealed_flow_object();
    let props = flow["properties"].as_object().unwrap();
    let mut got: Vec<&str> = props.keys().map(String::as_str).collect();
    got.sort_unstable();
    let mut want = all_flow_fields();
    want.sort_unstable();
    assert_eq!(got, want);
}

#[test]
fn every_flow_child_field_carries_its_oscal_subid() {
    let flow = sealed_flow_object();
    let props = flow["properties"].as_object().unwrap();
    for field in all_flow_fields() {
        assert_eq!(
            props[field]["x-oscal-subid"].as_str(),
            Some(expected_subid(field).as_str()),
            "field {field} must carry x-oscal-subid"
        );
    }
}

#[test]
fn flow_child_cookie_is_a_hex_string_with_the_3tched_prefix_shape() {
    let flow = sealed_flow_object();
    let cookie = &flow["properties"]["cookie"];
    assert_eq!(cookie["type"], "string");
    let pattern = cookie["pattern"].as_str().expect("cookie pattern");
    assert!(pattern.contains("0x33444348"), "pattern was {pattern}");
}

#[test]
fn flow_child_subids_are_registered_in_the_sealed_catalog_snapshot() {
    let schema = openflow_schema();
    let document = SealedSchemaDocument {
        plugin_id: "openflow".to_string(),
        schema_hash: "0".repeat(64),
        schema_json: serde_json::to_string(&schema).unwrap(),
    };
    let snapshot = SealedCatalogSnapshot::from_documents("1".repeat(64), vec![document])
        .expect("registry accepts the openflow schema (valid, bounded subids)");
    let dump = format!("{snapshot:?}");
    for field in all_flow_fields() {
        let subid = expected_subid(field);
        assert!(
            dump.contains(&format!("subid: \"{subid}\"")),
            "registry has no entry for {subid}"
        );
    }
    assert!(
        !dump.contains("mut.network.openflow.datapath.ensure-fallback-normal@v1"),
        "retired subid must not be active in the registry"
    );
}

#[test]
fn ensure_fallback_normal_is_gone_from_the_sealed_schema() {
    let schema = openflow_schema();
    assert!(!schema.methods.contains_key("ensure_fallback_normal"));
    assert!(
        schema.methods.values().all(
            |m| m.name != "EnsureFallbackNormal" && !m.subid.contains("ensure-fallback-normal")
        )
    );
    assert!(
        schema
            .subids
            .values()
            .all(|s| !s.contains("ensure-fallback-normal"))
    );
    let json = serde_json::to_string(&schema).unwrap();
    assert!(!json.contains("ensure-fallback-normal"));
    assert!(!json.to_lowercase().contains("ensurefallbacknormal"));
    assert!(!json.contains("ensure_fallback_normal"));
}

#[test]
fn retained_methods_keep_their_subids() {
    let schema = openflow_schema();
    let mut names: Vec<&str> = schema.methods.keys().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "add_flow",
            "attach_controller_safe",
            "del_controller",
            "delete_flow",
            "get_datapath_health",
            "modify_flow",
            "set_controller",
            "set_fail_mode",
        ]
    );
    assert!(schema.validate_capability_closure().ok());
}

#[tokio::test]
async fn dispatch_rejects_ensure_fallback_normal_as_unknown() {
    let err = dispatch_openflow_method(
        "ensure_fallback_normal",
        &serde_json::json!({"bridge": "ovsbr0"}),
    )
    .await
    .unwrap_err();
    assert!(
        err.to_string().contains("unknown openflow method"),
        "got: {err}"
    );
}
