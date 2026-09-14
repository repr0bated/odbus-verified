//! Exact retained fields preserve local refs through whole-schema export/sealing.
use op_state_store::{FieldSchema, FieldType, MethodDecl, PluginSchema, SideEffect};
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, SerializeOptions};
use serde::de::DeserializeSeed;
use serde_json::json;

#[test]
fn exact_field_original_schema_and_rebased_referenced_wire_both_survive_sealing() {
    let document = json!({"$schema":op_state_store::DEFAULT_SCHEMA_DIALECT,"$defs":{"Row":{
        "type":"object","properties":{"revision":{"type":"integer","format":"uint64","minimum":0},
        "label":{"type":["string","null"]},"children":{"type":"object","additionalProperties":{"type":"boolean"}}},
        "required":["revision","label","children"],"additionalProperties":false}},
        "type":"object","properties":{"jobs":{"type":"object","additionalProperties":{"$ref":"#/$defs/Row"}}},"required":["jobs"],"additionalProperties":false});
    let mut schema = PluginSchema::builder("exact_fixture")
        .version("2.0.0")
        .field(
            "native",
            FieldSchema {
                field_type: FieldType::exact_json_schema(document.clone()).unwrap(),
                required: true,
                description: "Internal source contract".into(),
                default: None,
                example: None,
                constraints: vec![],
                read_only: true,
                read_only_when: None,
            },
        )
        .build();
    let returns = schema.to_json_schema();
    schema.methods.insert(
        "Read".into(),
        MethodDecl {
            name: "Read".into(),
            args: simd_json::json!({"type":"object","properties":{}}),
            returns: Some(returns),
            side_effect: SideEffect::Read,
            idempotent: true,
            required_capability: Some("fixture.read".into()),
            subid: "obs.service.fixture.read@v2".into(),
        },
    );
    let blob = op_blob::blobify_plugin_schema("exact_fixture", schema.clone());
    let bytes = blob.seal();
    let pinned =
        op_blob::BlobRef::from_manifest_pinned(&bytes, &blob.manifest.schema_hash).unwrap();
    let decoded: PluginSchema = serde_json::from_str(pinned.schema_json()).unwrap();
    assert_eq!(decoded.fields, schema.fields);
    assert_eq!(
        serde_json::to_value(&decoded.fields["native"].field_type).unwrap()["json_schema"],
        document
    );
    let mirror: op_blob::schema::PluginSchema = serde_json::from_str(pinned.schema_json()).unwrap();
    let export = mirror.to_json_schema();
    assert_eq!(
        export["properties"]["native"]["properties"]["jobs"]["additionalProperties"]["$ref"],
        "#/properties/native/$defs/Row"
    );
    let input = json!({"native":{"jobs":{"stable_key":{"revision":u64::MAX,"label":null,"children":{"stable_grandchild":false}}}}});
    assert!(jsonschema::Validator::new(&export)
        .unwrap()
        .is_valid(&input));
    let pool = DescriptorPool::decode(pinned.descriptor_set()).unwrap();
    let descriptor = pool
        .get_message_by_name(&blob.manifest.methods[0].output_message)
        .unwrap();
    let bytes = input.to_string();
    let message = descriptor
        .clone()
        .deserialize(&mut serde_json::Deserializer::from_str(&bytes))
        .unwrap();
    let decoded = DynamicMessage::decode(descriptor, message.encode_to_vec().as_slice()).unwrap();
    let mut encoded = Vec::new();
    decoded
        .serialize_with_options(
            &mut serde_json::Serializer::new(&mut encoded),
            &SerializeOptions::new()
                .use_proto_field_name(true)
                .stringify_64_bit_integers(false)
                .skip_default_fields(false),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&encoded).unwrap(),
        input
    );
    // The exact field is internal, regardless of the method output fixture.
    assert_eq!(
        schema
            .project_public_state(&simd_json::serde::to_owned_value(input).unwrap())
            .unwrap(),
        None
    );
}
