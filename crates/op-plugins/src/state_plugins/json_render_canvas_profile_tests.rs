use super::*;
use serde_json::json;

fn validator() -> PinnedCanvasValidator {
    let blob = op_blob::blobify_plugin_schema("json_render", super::super::json_render_schema());
    let bytes = blob.seal();
    let parsed =
        op_blob::BlobRef::from_manifest_pinned(&bytes, &blob.manifest.schema_hash).unwrap();
    let original = parsed.schema_json();
    let validator = PinnedCanvasValidator::from_manifest_pinned_schema(
        Arc::from(original),
        blob.manifest.schema_hash.clone(),
    )
    .unwrap();
    assert_eq!(validator.original_schema_json(), original);
    assert_eq!(validator.schema_hash(), blob.manifest.schema_hash);
    validator
}

fn text() -> Value {
    json!({"root":"root","elements":{"root":{"type":"text","props":{"text":"hello","className":null},"children":[]}}})
}

#[test]
fn actual_seal_retains_source_checked_profile_and_required_nullable_props() {
    let validator = validator();
    assert_eq!(validator.profile.evidence.assurance, "source_checked");
    assert_eq!(validator.profile.evidence.core_version, "0.20.0");
    assert_eq!(validator.profile.components.len(), 8);
    assert_eq!(validator.profile.actions.len(), 1);
    assert_eq!(validator.profile.actions[0].name, "setState");
    validator.validate(&text()).unwrap();
    let mut missing = text();
    missing["elements"]["root"]["props"]
        .as_object_mut()
        .unwrap()
        .remove("className");
    assert!(validator.validate(&missing).is_err());
    let mut unknown = text();
    unknown["elements"]["root"]["props"]["secret"] = json!("PRIVATE_SENTINEL");
    let error = validator.validate(&unknown).unwrap_err();
    assert!(!format!("{error:?}").contains("PRIVATE_SENTINEL"));
}

#[test]
fn rejects_reference_catalog_and_hidden_shell_recursion_and_runtime_features() {
    let validator = validator();
    for component in [
        "Card",
        "Text",
        "antigravityChatContainer",
        "tchedRouterPicker",
        "pageHeader",
        "chatMessage",
    ] {
        let mut value = text();
        value["elements"]["root"]["type"] = json!(component);
        assert!(validator.validate(&value).is_err(), "{component}");
    }
    for (key, value) in [
        ("on", json!({"resize":{"action":"setState"}})),
        ("repeat", json!({"statePath":"/secret"})),
        ("watch", json!({})),
        ("visible", json!({"$state":"/secret"})),
        ("slots", json!({"default":["root"]})),
    ] {
        let mut spec = text();
        spec["elements"]["root"][key] = value;
        assert!(validator.validate(&spec).is_err(), "{key}");
    }
    let mut expression = text();
    expression["elements"]["root"]["props"]["text"] = json!({"$state":"/streams/private/messages"});
    assert!(validator.validate(&expression).is_err());
    let mut state = text();
    state["state"] = json!({"selected":false});
    validator.validate(&state).unwrap();
}

#[test]
fn set_state_event_is_resolved_from_the_pinned_binding() {
    let validator = validator();
    let spec = json!({
        "root":"board",
        "state":{"size":"small"},
        "elements":{
            "board":{"type":"autoTileItem","props":{"size":"small","contentType":null,"tileId":"tile","className":null},"children":[],
                "on":{"resize":{"action":"setState","params":{"statePath":"/size","value":"large"}}}}
        }
    });
    let changed = validator.apply_event(&spec, "board", "resize").unwrap();
    assert_eq!(changed["state"]["size"], "large");
    assert_eq!(changed["elements"], spec["elements"]);
    assert!(validator.apply_event(&spec, "board", "delete").is_err());
}

#[test]
fn all_static_component_contracts_accept_exact_implemented_props() {
    let validator = validator();
    for (name, props) in [
        ("container", json!({"className":null})),
        ("row", json!({"className":null})),
        ("text", json!({"text":"hello","className":null})),
        ("heading", json!({"text":"title","level":2})),
        ("badge", json!({"text":null,"label":"ok","tone":"ok"})),
        (
            "stat",
            json!({"label":"count","value":"3","unit":null,"trend":"flat"}),
        ),
        (
            "autoTile",
            json!({"columns":null,"gap":null,"rowHeight":null,"boardId":null,"reorderable":null,"fit":null,"className":null}),
        ),
        (
            "autoTileItem",
            json!({"size":null,"contentType":null,"tileId":null,"className":null}),
        ),
    ] {
        let spec =
            json!({"root":"root","elements":{"root":{"type":name,"props":props,"children":[]}}});
        validator.validate(&spec).unwrap();
    }
    let bad = json!({"root":"root","elements":{"root":{"type":"heading","props":{"text":"title","level":4},"children":[]}}});
    assert!(validator.validate(&bad).is_err());
}

#[test]
fn graph_and_json_limits_apply_before_unbounded_render_work() {
    let validator = validator();
    let mut cycle = text();
    cycle["elements"]["root"] =
        json!({"type":"container","props":{"className":null},"children":["root"]});
    assert!(
        validator
            .validate(&cycle)
            .unwrap_err()
            .to_string()
            .contains("cycle")
    );
    let mut graph = serde_json::Map::new();
    for index in 0..14 {
        graph.insert(
            format!("n{index}"),
            json!({"type":"container","props":{"className":null},
            "children":if index == 13 { vec![] } else { vec![format!("n{}",index+1);2] }}),
        );
    }
    let amplified = json!({"root":"n0","elements":graph});
    assert!(
        validator
            .validate(&amplified)
            .unwrap_err()
            .to_string()
            .contains("expanded")
    );
    let mut deep = Value::Null;
    for _ in 0..70 {
        deep = json!([deep]);
    }
    assert!(
        validator
            .validate(&deep)
            .unwrap_err()
            .to_string()
            .contains("depth")
    );
}
