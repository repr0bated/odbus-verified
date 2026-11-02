use op_blob::{MethodDecl, PluginSchema, SideEffect};
use serde_json::{Value, json};

fn schema(args: Value, returns: Value) -> PluginSchema {
    PluginSchema::builder("alias_fixture")
        .method(MethodDecl {
            name: "Inspect".into(),
            args,
            returns: Some(returns),
            side_effect: SideEffect::Read,
            idempotent: true,
            required_capability: None,
            subid: "obs.service.fixture.inspect@v1".into(),
        })
        .build()
}

fn ambiguous_object() -> Value {
    json!({
        "type": "object",
        "properties": {
            "foo_bar": {"type": "string"},
            "fooBar": {"type": "integer"}
        },
        "additionalProperties": false
    })
}

#[test]
fn checked_producers_report_method_and_schema_side_for_alias_errors() {
    let empty = json!({"type": "object"});
    for (args, returns, side) in [
        (ambiguous_object(), empty.clone(), "args"),
        (empty, ambiguous_object(), "returns"),
    ] {
        let schema = schema(args, returns);
        let canonical = schema.canonical_json();
        for error in [
            op_blob::try_blobify(&schema).unwrap_err(),
            op_blob::try_blobify_with_identity(&schema, None).unwrap_err(),
            op_blob::blob::try_blobify_canonical(&schema, canonical, None).unwrap_err(),
            op_blob::descriptor::try_descriptor_set_for_plugin("alias_fixture", &schema)
                .unwrap_err(),
            op_blob::descriptor::try_method_file_descriptor(
                "alias_fixture",
                &schema.methods["Inspect"],
            )
            .unwrap_err(),
        ] {
            assert!(error.contains("alias_fixture"), "{error}");
            assert!(error.contains("Inspect"), "{error}");
            assert!(error.contains(side), "{error}");
            assert!(error.contains("foo_bar") && error.contains("fooBar"), "{error}");
        }
    }
}

#[test]
fn checked_producer_propagates_alias_errors_through_references_arrays_and_maps() {
    for nested in [
        json!({"type": "array", "items": {"$ref": "#/$defs/Record"}}),
        json!({"type": "object", "additionalProperties": {"$ref": "#/$defs/Record"}}),
    ] {
        let args = json!({
            "type": "object",
            "properties": {"records": nested},
            "$defs": {"Record": ambiguous_object()}
        });
        let error = op_blob::try_blobify(&schema(args, json!({"type": "object"})))
            .unwrap_err();
        assert!(error.contains("args"), "{error}");
        assert!(error.contains("foo_bar") && error.contains("fooBar"), "{error}");
    }
}

#[test]
fn full_workspace_producers_return_alias_errors() {
    let mut workspace_schema = op_state_store::PluginSchema::builder("alias_fixture").build();
    workspace_schema.methods.insert(
        "Inspect".into(),
        serde_json::from_value(
            serde_json::to_value(&schema(ambiguous_object(), json!({})).methods["Inspect"])
                .unwrap(),
        )
        .unwrap(),
    );
    assert!(op_blob::try_blobify_plugin_schema("alias_fixture", workspace_schema.clone()).is_err());
    assert!(
        op_blob::try_blobify_plugin_schema_with_identity("alias_fixture", workspace_schema, None)
            .is_err()
    );
}
