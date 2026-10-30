use op_state_store::{FieldSchema, FieldType, MethodDecl, PluginSchema, SideEffect};

fn simd_value(value: serde_json::Value) -> simd_json::OwnedValue {
    serde_json::from_value(value).expect("JSON value deserializes into simd-json")
}

fn args_schema(reverse_keys: bool, reverse_required: bool) -> serde_json::Value {
    let properties = if reverse_keys {
        serde_json::json!({
            "zeta": {"type": "string"},
            "alpha": {"type": "integer"}
        })
    } else {
        serde_json::json!({
            "alpha": {"type": "integer"},
            "zeta": {"type": "string"}
        })
    };
    let required = if reverse_required {
        vec!["zeta", "alpha"]
    } else {
        vec!["alpha", "zeta"]
    };

    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required
    })
}

fn method_schema(reverse_keys: bool, reverse_required: bool) -> PluginSchema {
    let mut schema = PluginSchema::builder("canonical_fixture")
        .version("1.0.0")
        .build();
    schema.methods.insert(
        "Inspect".to_string(),
        MethodDecl {
            name: "Inspect".to_string(),
            args: simd_value(args_schema(reverse_keys, reverse_required)),
            returns: Some(simd_value(serde_json::json!({
                "type": "object",
                "properties": {
                    "result": {"type": "boolean"}
                }
            }))),
            side_effect: SideEffect::Read,
            idempotent: true,
            required_capability: Some("fixture.read@v1".to_string()),
            subid: "obs.service.fixture.inspect@v1".to_string(),
        },
    );

    // The metadata section includes this exact schema. Its nested object-key
    // permutations must be canonicalized by the same producer rule.
    let exact_schema = if reverse_keys {
        serde_json::json!({
            "type": "object",
            "properties": {
                "zeta": {"const": 2},
                "alpha": {"enum": ["first", "second"]}
            }
        })
    } else {
        serde_json::json!({
            "type": "object",
            "properties": {
                "alpha": {"enum": ["first", "second"]},
                "zeta": {"const": 2}
            }
        })
    };
    schema.fields.insert(
        "exact".to_string(),
        FieldSchema {
            field_type: FieldType::JsonSchema(Box::new(simd_value(exact_schema))),
            required: true,
            description: String::new(),
            default: None,
            example: None,
            constraints: Vec::new(),
            read_only: false,
            read_only_when: None,
        },
    );
    schema
}

#[test]
fn equivalent_object_key_permutations_produce_identical_sealed_blobs() {
    let first_args = args_schema(false, false);
    let permuted_args = args_schema(true, false);
    assert_eq!(first_args, permuted_args);
    assert_ne!(first_args.to_string(), permuted_args.to_string());

    let first = op_blob::blobify_plugin_schema("canonical_fixture", method_schema(false, false));
    let repeated = op_blob::blobify_plugin_schema("canonical_fixture", method_schema(false, false));
    let permuted = op_blob::blobify_plugin_schema("canonical_fixture", method_schema(true, false));

    assert_eq!(first.schema_json, repeated.schema_json);
    assert_eq!(first.meta_json, repeated.meta_json);
    assert_eq!(first.seal(), repeated.seal());

    assert_eq!(first.schema_json, permuted.schema_json);
    assert_eq!(first.manifest.schema_hash, permuted.manifest.schema_hash);
    assert_eq!(first.meta_json, permuted.meta_json);
    assert_eq!(first.seal(), permuted.seal());
}

#[test]
fn reduced_schema_canonicalization_sorts_embedded_objects_but_preserves_arrays() {
    let make_schema = |reverse_keys, reverse_required| {
        let mut schema: op_blob::PluginSchema = serde_json::from_value(
            serde_json::to_value(method_schema(reverse_keys, reverse_required)).unwrap(),
        )
        .unwrap();
        // Keep insertion order in serde_json directly. The dev-dependency
        // enables preserve_order; production canonicalization cannot rely on it.
        schema.methods.get_mut("Inspect").unwrap().args =
            args_schema(reverse_keys, reverse_required);
        schema
    };
    let first = make_schema(false, false);
    let permuted = make_schema(true, false);
    let reordered_array = make_schema(false, true);
    assert_ne!(
        first.methods["Inspect"].args.to_string(),
        permuted.methods["Inspect"].args.to_string()
    );
    assert_eq!(first.canonical_json(), permuted.canonical_json());
    assert_eq!(first.schema_hash(), permuted.schema_hash());
    assert_eq!(
        op_blob::try_blobify(&first).unwrap().seal(),
        op_blob::try_blobify(&permuted).unwrap().seal()
    );
    assert_ne!(first.schema_hash(), reordered_array.schema_hash());
}

#[test]
fn canonicalization_preserves_array_order() {
    let first = op_blob::blobify_plugin_schema("canonical_fixture", method_schema(false, false));
    let reordered_array =
        op_blob::blobify_plugin_schema("canonical_fixture", method_schema(false, true));

    assert_ne!(first.schema_json, reordered_array.schema_json);
    assert_ne!(
        first.manifest.schema_hash,
        reordered_array.manifest.schema_hash
    );
}

#[test]
fn jsonschema_const_and_enum_compare_nested_objects_without_reordering_arrays() {
    let const_schema = serde_json::json!({
        "const": {
            "outer": {
                "items": [
                    {"alpha": 1, "beta": [true, false]},
                    "tail"
                ]
            }
        }
    });
    let enum_schema = serde_json::json!({
        "enum": [{
            "outer": {
                "items": [
                    {"alpha": 1, "beta": [true, false]},
                    "tail"
                ]
            }
        }]
    });
    let equivalent_instance = serde_json::json!({
        "outer": {
            "items": [
                {"beta": [true, false], "alpha": 1},
                "tail"
            ]
        }
    });
    let reordered_array_instance = serde_json::json!({
        "outer": {
            "items": [
                "tail",
                {"alpha": 1, "beta": [true, false]}
            ]
        }
    });

    assert!(jsonschema::is_valid(&const_schema, &equivalent_instance));
    assert!(jsonschema::is_valid(&enum_schema, &equivalent_instance));
    assert!(!jsonschema::is_valid(
        &const_schema,
        &reordered_array_instance
    ));
    assert!(!jsonschema::is_valid(
        &enum_schema,
        &reordered_array_instance
    ));
}
