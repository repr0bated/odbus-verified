//! Actual state FieldType map -> sealed contract -> reflected protobuf boundary.
use op_state_store::{FieldSchema, FieldType, MethodDecl, PluginSchema, SideEffect};
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, SerializeOptions};
use serde::de::DeserializeSeed;
use std::collections::HashMap;
fn field(field_type: FieldType) -> FieldSchema {
    FieldSchema {
        field_type,
        required: true,
        description: String::new(),
        default: None,
        example: None,
        constraints: vec![],
        read_only: false,
        read_only_when: None,
    }
}
#[test]
fn sealed_typed_map_state_keeps_children_contract_and_real_wire_keys() {
    let leaf = FieldType::Object(HashMap::from([("ready".into(), field(FieldType::Boolean))]));
    let item = FieldType::Object(HashMap::from([(
        "children".into(),
        field(FieldType::Map(Box::new(leaf))),
    )]));
    let mut schema = PluginSchema::builder("map_fixture")
        .version("2.0.0")
        .field("jobs", field(FieldType::Map(Box::new(item))))
        .public_state_path("/jobs/approved/children/first/ready")
        .build();
    let output = schema.to_json_schema();
    schema.methods.insert(
        "Inspect".into(),
        MethodDecl {
            name: "Inspect".into(),
            args: simd_json::json!({"type":"object","properties":{}}),
            returns: Some(output),
            side_effect: SideEffect::Read,
            idempotent: true,
            required_capability: Some("fixture.read".into()),
            subid: "obs.service.fixture.inspect@v2".into(),
        },
    );
    let blob = op_blob::blobify_plugin_schema("map_fixture", schema.clone());
    let bytes = blob.seal();
    let pinned =
        op_blob::BlobRef::from_manifest_pinned(&bytes, &blob.manifest.schema_hash).unwrap();
    let decoded: PluginSchema = serde_json::from_str(pinned.schema_json()).unwrap();
    assert_eq!(decoded.fields, schema.fields);
    let private = simd_json::json!({"jobs":{"approved":{"children":{"first":{"ready":false},"private_child":{"ready":true}}},"private_job":{"children":{}}}});
    assert_eq!(
        decoded.project_public_state(&private).unwrap(),
        Some(simd_json::json!({"jobs":{"approved":{"children":{"first":{"ready":false}}}}}))
    );
    let pool = DescriptorPool::decode(pinned.descriptor_set()).unwrap();
    let descriptor = pool
        .get_message_by_name(&blob.manifest.methods[0].output_message)
        .unwrap();
    let jobs = descriptor.get_field_by_name("jobs").unwrap();
    assert!(jobs.is_map());
    let input = serde_json::json!({"jobs":{"stable_job":{"children":{"stable_grandchild":{"ready":false}}}}});
    let serialized = input.to_string();
    let dynamic = descriptor
        .clone()
        .deserialize(&mut serde_json::Deserializer::from_str(&serialized))
        .unwrap();
    let decoded = DynamicMessage::decode(descriptor, dynamic.encode_to_vec().as_slice()).unwrap();
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
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
        input
    );
}
