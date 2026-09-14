//! Exercise the actual sealed descriptor bytes as a protobuf JSON boundary.
//! Fixtures seal locally; consumers neither synthesize nor rehash a schema.
use op_state_store::{MethodDecl, PluginSchema, SideEffect};
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor, SerializeOptions};
use serde::de::DeserializeSeed;
use serde_json::{json, Value};

#[test]
fn public_state_policy_survives_the_actual_sealed_blob_boundary() {
    let child = PluginSchema::builder("child")
        .string_field("status", true, "Shared status")
        .build();
    let schema = PluginSchema::builder("policy_fixture")
        .object_field("child", child.fields, false, "Child status")
        .string_field("private_value", false, "Internal value")
        .public_state_path("/child")
        .build();
    let blob = op_blob::blobify_plugin_schema("policy_fixture", schema);
    let bytes = blob.seal();
    let pinned =
        op_blob::BlobRef::from_manifest_pinned(&bytes, &blob.manifest.schema_hash).unwrap();
    let decoded: PluginSchema = serde_json::from_str(pinned.schema_json()).unwrap();
    let state = simd_json::json!({
        "child": {"status": "ready", "undeclared_secret": "hidden"},
        "private_value": "hidden"
    });
    assert_eq!(decoded.public_state.paths, vec!["/child"]);
    assert_eq!(
        decoded.project_public_state(&state).unwrap(),
        Some(simd_json::json!({"child": {"status": "ready"}}))
    );
}

fn sealed_contract(contract: &Value) -> MessageDescriptor {
    let schema = PluginSchema::builder("descriptor_fixture")
        .version("2.0.0")
        .method(MethodDecl {
            name: "Inspect".into(),
            args: simd_json::serde::to_owned_value(contract).unwrap(),
            returns: Some(simd_json::serde::to_owned_value(contract).unwrap()),
            side_effect: SideEffect::Read,
            idempotent: true,
            required_capability: Some("fixture.read".into()),
            subid: "obs.service.fixture.inspect@v2".into(),
        })
        .build();
    let blob = op_blob::blobify_plugin_schema("descriptor_fixture", schema);
    let sealed = blob.seal();
    let pinned =
        op_blob::BlobRef::from_manifest_pinned(&sealed, &blob.manifest.schema_hash).unwrap();
    let decoded_schema: Value = serde_json::from_str(pinned.schema_json()).unwrap();
    assert_eq!(decoded_schema["methods"]["Inspect"]["args"], *contract);
    let pool = DescriptorPool::decode(pinned.descriptor_set()).unwrap();
    // Both directions derive from the same supplied contract in this fixture.
    let input = pool
        .get_message_by_name(&blob.manifest.methods[0].input_message)
        .unwrap();
    let output = pool
        .get_message_by_name(&blob.manifest.methods[0].output_message)
        .unwrap();
    assert_eq!(input.fields().count(), output.fields().count());
    output
}

fn roundtrip(descriptor: MessageDescriptor, input: &Value) -> Value {
    let serialized = serde_json::to_string(input).unwrap();
    let mut parser = serde_json::Deserializer::from_str(&serialized);
    let message = descriptor.clone().deserialize(&mut parser).unwrap();
    let decoded = DynamicMessage::decode(descriptor, message.encode_to_vec().as_slice()).unwrap();
    let mut bytes = Vec::new();
    decoded
        .serialize_with_options(
            &mut serde_json::Serializer::new(&mut bytes),
            &SerializeOptions::new()
                .use_proto_field_name(true)
                .stringify_64_bit_integers(false)
                .skip_default_fields(false),
        )
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn assert_json_preserved(expected: &Value, actual: &Value) {
    match (expected, actual) {
        (Value::Number(expected), Value::Number(actual)) => {
            // JSON has one numeric type. Standard Value uses double, so small
            // fixture integers may reappear as 1.0. Large typed integers are
            // asserted exactly in their separate test, never via a float cast.
            assert!(expected.as_f64().unwrap().abs() <= 9_007_199_254_740_992.0);
            assert_eq!(expected.as_f64(), actual.as_f64());
        }
        (Value::Object(expected), Value::Object(actual)) => {
            assert_eq!(expected.len(), actual.len());
            for (key, value) in expected {
                assert_json_preserved(value, actual.get(key).unwrap());
            }
        }
        (Value::Array(expected), Value::Array(actual)) => {
            assert_eq!(expected.len(), actual.len());
            for (expected, actual) in expected.iter().zip(actual) {
                assert_json_preserved(expected, actual);
            }
        }
        _ => assert_eq!(expected, actual),
    }
}

#[test]
fn null_unions_any_and_heterogeneous_json_are_values_not_json_strings() {
    let contract = json!({
        "type":"object", "properties":{
            "any":{}, "null_only":{"type":"null"},
            "nullable":{"type":["string","null"]},
            "union":{"oneOf":[{"type":"string"},{"type":"object"}]},
            "map":{"type":"object", "additionalProperties":true},
            "enum_value":{"enum":[1,false,null]}
        }, "required":["any","null_only","nullable","union","map","enum_value"],
        "additionalProperties":false
    });
    let descriptor = sealed_contract(&contract);
    let input = json!({
        "any":[true,null,{"text":"📚","count":3}], "null_only":null,
        "nullable":null, "union":{"selected":true},
        "map":{"nested":[0,null,{"flag":false}]}, "enum_value":false
    });
    let actual = roundtrip(descriptor.clone(), &input);
    assert_json_preserved(&input, &actual);
    assert!(jsonschema::validate(&contract, &actual).is_ok());
    let mut second = input;
    second["any"] = Value::Null;
    second["nullable"] = json!("present");
    second["union"] = json!("selected");
    second["enum_value"] = Value::Null;
    assert_json_preserved(&second, &roundtrip(descriptor, &second));
}

#[test]
fn matrices_higher_arrays_and_maps_of_lists_keep_json_boundaries() {
    let contract = json!({"type":"object", "properties":{
        "matrix":{"type":"array","items":{"type":"array","items":{"type":"number"}}},
        "tensor":{"type":"array","items":{"type":"array","items":{"type":"array","items":{}}}},
        "lists":{"type":"object","additionalProperties":{"type":"array","items":{"type":"integer"}}},
        "maps":{"type":"array","items":{"type":"object","additionalProperties":{"type":"boolean"}}},
        "nested_maps":{"type":"object","additionalProperties":{"type":"object","additionalProperties":{"type":"boolean"}}}
    }});
    let input = json!({
        "matrix":[[],[0.25,0.75],[2]], "tensor":[[[true,null,{"v":3}],[]],[]],
        "lists":{"first":[0,1],"empty":[]}, "maps":[{"a":true},{"b":false}],
        "nested_maps":{"outer":{"inner":false}}
    });
    assert_json_preserved(&input, &roundtrip(sealed_contract(&contract), &input));
}

#[test]
fn declared_uint64_nested_records_and_typed_maps_preserve_maximum_exactly() {
    let contract = json!({"type":"object", "properties":{
        "maximum":{"type":"integer","format":"uint64","minimum":0},
        "signed":{"type":"integer","format":"int64"},
        "record":{"$ref":"#/$defs/Record"},
        "records":{"type":"array","items":{"$ref":"#/$defs/Record"}},
        "ids":{"type":"array","items":{"type":"integer","minimum":0}},
        "by_name":{"type":"object","additionalProperties":{"type":"integer","minimum":0}},
        "by_record":{"type":"object","additionalProperties":{"$ref":"#/$defs/Record"}}
    }, "$defs":{"Record":{"type":"object", "properties":{
        "id":{"type":"integer","format":"uint64","minimum":0}
    },"required":["id"],"additionalProperties":false}}});
    let input = json!({
        "maximum":u64::MAX, "signed":i64::MIN, "record":{"id":u64::MAX},
        "records":[{"id":u64::MAX}], "ids":[0,u64::MAX],
        "by_name":{"maximum":u64::MAX,"zero":0}, "by_record":{"maximum":{"id":u64::MAX}}
    });
    let actual = roundtrip(sealed_contract(&contract), &input);
    assert_eq!(actual, input);
    assert_eq!(actual["maximum"].as_u64(), Some(u64::MAX));
    assert_eq!(actual["record"]["id"].as_u64(), Some(u64::MAX));
    assert_eq!(actual["by_name"]["maximum"].as_u64(), Some(u64::MAX));
    assert_eq!(
        actual["by_record"]["maximum"]["id"].as_u64(),
        Some(u64::MAX)
    );
    assert!(jsonschema::validate(&contract, &actual).is_ok());
}

#[test]
fn false_zero_empty_string_and_omission_have_distinct_presence() {
    let contract = json!({"type":"object", "properties":{
        "enable":{"type":"boolean"}, "name":{"type":"string"},
        "uid":{"type":"integer","minimum":0}
    }, "required":["enable","name","uid"],"additionalProperties":false});
    let descriptor = sealed_contract(&contract);
    for (input, valid) in [
        (json!({"enable":false,"name":"","uid":0}), true),
        (json!({"name":"","uid":0}), false),
        (json!({"enable":false,"uid":0}), false),
        (json!({"enable":false,"name":""}), false),
        (json!({}), false),
    ] {
        let actual = roundtrip(descriptor.clone(), &input);
        assert_eq!(actual, input);
        assert_eq!(jsonschema::validate(&contract, &actual).is_ok(), valid);
    }
    // Existing producer field numbers remain name-sorted and sequential.
    assert_eq!(descriptor.get_field_by_name("enable").unwrap().number(), 1);
    assert_eq!(descriptor.get_field_by_name("name").unwrap().number(), 2);
    assert_eq!(descriptor.get_field_by_name("uid").unwrap().number(), 3);
    assert!(descriptor.fields().all(|field| field.supports_presence()));
}

#[test]
fn recursive_refs_alias_cycles_and_unknown_refs_are_bounded_structured_values() {
    let contracts = [
        json!({"type":"object","properties":{"child":{"$ref":"#"}}}),
        json!({"type":"object","properties":{"child":{"$ref":"#/$defs/A"}},
            "$defs":{"A":{"$ref":"#/$defs/B"},"B":{"$ref":"#/$defs/A"}}}),
        json!({"type":"object","properties":{"child":{"$ref":"https://example.invalid/schema"}}}),
        json!({"type":"object","properties":{"child":{"$ref":"#/$defs/missing"}}}),
    ];
    let input = json!({"child":{"child":{"child":[null,true,{"value":7}]}}});
    for contract in contracts {
        assert_json_preserved(&input, &roundtrip(sealed_contract(&contract), &input));
    }
    let arrays = json!({"type":"object","properties":{"child":{"$ref":"#/$defs/Array"}},
        "$defs":{"Array":{"type":"array","items":{"$ref":"#/$defs/Array"}}}});
    let input = json!({"child":[[[[null]]],[]]});
    assert_json_preserved(&input, &roundtrip(sealed_contract(&arrays), &input));
}

#[test]
fn local_json_pointer_refs_resolve_escaped_keys_and_bound_long_alias_chains() {
    let escaped = json!({"type":"object","properties":{"id":{"$ref":"#/$defs/id~1kind~0"}},
        "$defs":{"id/kind~":{"type":"integer","format":"uint64","minimum":0}}});
    let input = json!({"id":u64::MAX});
    assert_eq!(roundtrip(sealed_contract(&escaped), &input), input);
    let mut definitions = serde_json::Map::new();
    for index in 0..96 {
        definitions.insert(
            format!("N{index}"),
            json!({"$ref":format!("#/$defs/N{}",index+1)}),
        );
    }
    definitions.insert("N96".into(), json!({"type":"string"}));
    let long =
        json!({"type":"object","properties":{"value":{"$ref":"#/$defs/N0"}},"$defs":definitions});
    let input = json!({"value":"bounded fallback keeps a JSON value"});
    assert_eq!(roundtrip(sealed_contract(&long), &input), input);
}

#[test]
fn dynamic_value_numeric_precision_is_explicitly_limited_to_protobuf_double() {
    let contract = json!({"type":"object","properties":{"value":{}}});
    let input = json!({"value":9_007_199_254_740_993u64});
    let actual = roundtrip(sealed_contract(&contract), &input);
    // A standard google.protobuf.Value has number_value: double. This is an
    // explicit protocol boundary, not a claim that arbitrary JSON integers
    // have the exact preservation guaranteed for declared uint64 above.
    assert_ne!(actual["value"], input["value"]);
    assert_eq!(actual["value"].as_f64(), Some(9_007_199_254_740_992.0));
}
