//! Check actual generated and freshly sealed transport descriptors. Their field
//! numbers and message layouts differ; the plugin's output contract owns value
//! semantics. Fixtures never read or reseal a live catalog.
use op_plugins::state_plugins::tched_router::{
    tched_router_plugin_schema, ConfigPropertyKind, ConfigPropertyOutput,
};
use op_state_store::PluginSchema;
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, Kind, MessageDescriptor, SerializeOptions};
use serde::de::DeserializeSeed;
use serde_json::{json, Value};
use std::sync::OnceLock;

struct Contracts {
    schema: PluginSchema,
    native: DescriptorPool,
    sealed: DescriptorPool,
    manifest: op_blob::BlobManifest,
}

fn contracts() -> &'static Contracts {
    static CONTRACTS: OnceLock<Contracts> = OnceLock::new();
    CONTRACTS.get_or_init(|| {
        let schema = tched_router_plugin_schema();
        let blob = op_blob::blobify_plugin_schema("tched_router", schema.clone());
        let bytes = blob.seal();
        let pinned =
            op_blob::BlobRef::from_manifest_pinned(&bytes, &blob.manifest.schema_hash).unwrap();
        let restored: PluginSchema = serde_json::from_str(pinned.schema_json()).unwrap();
        assert_eq!(
            serde_json::to_value(&restored).unwrap(),
            serde_json::to_value(&schema).unwrap()
        );
        Contracts {
            schema: restored,
            native: DescriptorPool::decode(op_grpc_bridge::proto::FILE_DESCRIPTOR_SET).unwrap(),
            sealed: DescriptorPool::decode(pinned.descriptor_set()).unwrap(),
            manifest: pinned.manifest().unwrap(),
        }
    })
}

impl Contracts {
    fn output_schema(&self, method: &str) -> Value {
        serde_json::to_value(self.schema.methods[method].returns.as_ref().unwrap()).unwrap()
    }

    fn outputs(&self, method: &str, native_name: &str) -> [MessageDescriptor; 2] {
        let manifest = self
            .manifest
            .methods
            .iter()
            .find(|entry| entry.schema_name == method)
            .expect("canonical method key in sealed manifest");
        [
            self.native
                .get_message_by_name(&format!("operation.plugin.v1.{native_name}"))
                .unwrap(),
            self.sealed
                .get_message_by_name(&manifest.output_message)
                .unwrap(),
        ]
    }
}

fn assert_local_refs_resolve(root: &Value, value: &Value) {
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                let pointer = reference
                    .strip_prefix('#')
                    .expect("method output contracts use only local references");
                assert!(
                    root.pointer(pointer).is_some(),
                    "unresolved output {reference}"
                );
            }
            for child in object.values() {
                assert_local_refs_resolve(root, child);
            }
        }
        Value::Array(array) => {
            for child in array {
                assert_local_refs_resolve(root, child);
            }
        }
        _ => {}
    }
}

fn roundtrip(descriptor: &MessageDescriptor, input: &Value) -> Value {
    let text = serde_json::to_string(input).unwrap();
    let message = descriptor
        .clone()
        .deserialize(&mut serde_json::Deserializer::from_str(&text))
        .unwrap();
    let decoded =
        DynamicMessage::decode(descriptor.clone(), message.encode_to_vec().as_slice()).unwrap();
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
        (Value::Number(a), Value::Number(b)) => {
            // Struct/Value carry doubles. These fixtures deliberately stay in
            // the exact integer range; this is not a uint64-fidelity claim.
            assert!(a.as_f64().unwrap().abs() <= 9_007_199_254_740_992.0);
            assert_eq!(a.as_f64(), b.as_f64());
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.len(), b.len());
            for (key, value) in a {
                assert_json_preserved(value, b.get(key).unwrap());
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                assert_json_preserved(a, b);
            }
        }
        _ => assert_eq!(expected, actual),
    }
}

fn assert_kind_preserved(kind: Kind, expected: &Value, actual: &Value) {
    match kind {
        Kind::Message(message) if !message.full_name().starts_with("google.protobuf.") => {
            assert_message_preserved(&message, expected, actual);
        }
        _ => assert_json_preserved(expected, actual),
    }
}

fn assert_message_preserved(descriptor: &MessageDescriptor, expected: &Value, actual: &Value) {
    let expected = expected.as_object().unwrap();
    let actual = actual.as_object().unwrap();
    for (name, value) in expected {
        let field = descriptor.get_field_by_name(name).unwrap();
        let actual = actual
            .get(name)
            .unwrap_or_else(|| panic!("missing field {name}"));
        if field.is_map() {
            let Kind::Message(entry) = field.kind() else {
                panic!("map entry")
            };
            let kind = entry.get_field_by_name("value").unwrap().kind();
            let expected = value.as_object().unwrap();
            let actual = actual.as_object().unwrap();
            assert_eq!(expected.len(), actual.len(), "stable map keys");
            for (key, value) in expected {
                assert_kind_preserved(kind.clone(), value, actual.get(key).unwrap());
            }
        } else if field.is_list() {
            let expected = value.as_array().unwrap();
            let actual = actual.as_array().unwrap();
            assert_eq!(expected.len(), actual.len(), "array cardinality");
            for (value, actual) in expected.iter().zip(actual) {
                assert_kind_preserved(field.kind(), value, actual);
            }
        } else {
            assert_kind_preserved(field.kind(), value, actual);
        }
    }
    // Protobuf repeated/map fields have no presence. Match the bridge's JSON
    // serializer, allowing only those declared empty defaults to be added;
    // absent scalar/nullable properties must never become invented values.
    for (name, value) in actual
        .iter()
        .filter(|(name, _)| !expected.contains_key(*name))
    {
        let field = descriptor.get_field_by_name(name).unwrap();
        assert!(
            (field.is_list() && value == &json!([])) || (field.is_map() && value == &json!({})),
            "unexpected field {name} in {}: {value}",
            descriptor.full_name()
        );
    }
}

#[test]
fn native_config_outputs_survive_generated_and_sealed_descriptors() {
    let contracts = contracts();
    for method in std::iter::once("GetConfig").chain(
        op_plugins::state_plugins::tched_router_config_surface::SECTIONS
            .iter()
            .map(|(method, _, _)| *method),
    ) {
        let output = contracts.output_schema(method);
        assert_local_refs_resolve(&output, &output);
        jsonschema::Validator::new(&output).expect("sealed method output compiles");
    }

    let gateway = json!({"port":0,"allow_public_bind":false,"path_prefix":null,"tls":null});
    let agents = json!({"fixture-agent":{"enabled":false,"cron_jobs":["fixture-job"],
        "identity":{"format":"aieos","aieos_inline":null,"aieos_path":null}}});
    let routes = json!([{"hint":"semantic","model_provider":"openai.fixture", "model":"fixture-model", "dimensions":0,"api_key":null}]);
    for (method, native_name, value) in [
        (
            "GetConfig",
            "TchedRouterGetConfigResponse",
            json!({"config":{}}),
        ),
        (
            "GetConfig",
            "TchedRouterGetConfigResponse",
            json!({"config":{"gateway":gateway.clone(),"agents":agents.clone(),"embedding_routes":routes.clone()}}),
        ),
        (
            "GetGatewayConfig",
            "TchedRouterGetGatewayConfigResponse",
            json!({"config":gateway}),
        ),
        (
            "GetAgentsConfig",
            "TchedRouterGetAgentsConfigResponse",
            json!({"config":agents}),
        ),
        (
            "GetEmbeddingRoutesConfig",
            "TchedRouterGetEmbeddingRoutesConfigResponse",
            json!({"config":routes}),
        ),
    ] {
        let contract = contracts.output_schema(method);
        jsonschema::validate(&contract, &value).expect("native-contract-valid fixture");
        for descriptor in contracts.outputs(method, native_name) {
            let actual = roundtrip(&descriptor, &value);
            assert_message_preserved(&descriptor, &value, &actual);
            jsonschema::validate(&contract, &actual)
                .expect("wire result still satisfies sealed output");
        }
    }

    // Shape-preserving protobuf transport cannot replace the declared native
    // uint16 constraint. Validate against the same resolved wrapper afterward.
    let output = contracts.output_schema("GetGatewayConfig");
    let invalid = json!({"config":{"port":65536}});
    for descriptor in contracts.outputs("GetGatewayConfig", "TchedRouterGetGatewayConfigResponse") {
        assert!(jsonschema::validate(&output, &roundtrip(&descriptor, &invalid)).is_err());
    }
}

#[test]
fn config_property_kind_path_and_nullable_fields_survive_both_descriptors() {
    let contracts = contracts();
    let output = contracts.output_schema("config_get");
    assert_local_refs_resolve(&output, &output);
    let kind_ref = output["properties"]["kind"]["$ref"].as_str().unwrap();
    assert_eq!(
        output.pointer(kind_ref.strip_prefix('#').unwrap()).unwrap()["enum"],
        json!(["value", "secret"])
    );
    let [native, sealed] = contracts.outputs("config_get", "TchedRouterConfigGetResponse");
    // The native mapper preserves unresolved refs as Value. The sealed mapper
    // resolves this local enum as string; neither creates a second enum owner.
    let Kind::Message(kind) = native.get_field_by_name("kind").unwrap().kind() else {
        panic!("native enum carrier")
    };
    assert_eq!(kind.full_name(), "google.protobuf.Value");
    assert_eq!(
        sealed.get_field_by_name("kind").unwrap().kind(),
        Kind::String
    );
    for descriptor in [&native, &sealed] {
        let path = descriptor.get_field_by_name("path").unwrap();
        assert_eq!(path.kind(), Kind::String);
        assert!(path.supports_presence());
        for field in ["value", "populated"] {
            let Kind::Message(kind) = descriptor.get_field_by_name(field).unwrap().kind() else {
                panic!("nullable carrier")
            };
            assert_eq!(kind.full_name(), "google.protobuf.Value");
        }
        assert!(descriptor.get_field_by_name("warnings").unwrap().is_list());
        for value in [
            json!({"kind":"value","path":"gateway.port","value":"0","warnings":[]}),
            json!({"kind":"value","path":"","value":null,"populated":null,"warnings":[]}),
            json!({"kind":"secret","path":"providers.models.openai.fixture.api_key","populated":false,"warnings":[]}),
            json!({"kind":"secret","path":"providers.models.openai.fixture.api_key","populated":true,"warnings":[{"code":"fixture","path":"providers.models.openai.fixture.api_key","message":"fixture warning"}]}),
        ] {
            jsonschema::validate(&output, &value).unwrap();
            let actual = roundtrip(descriptor, &value);
            assert_json_preserved(&value, &actual);
            jsonschema::validate(&output, &actual).unwrap();
        }
        for invalid in [
            json!({"kind":"invented","path":"gateway.port","warnings":[]}),
            json!({"kind":"value","warnings":[]}),
        ] {
            let actual = roundtrip(descriptor, &invalid);
            assert_json_preserved(&invalid, &actual);
            assert!(
                jsonschema::validate(&output, &actual).is_err(),
                "enum and required path stay schema-enforced"
            );
        }
    }

    for value in [
        json!({"kind":"value","path":"gateway.port","value":"8084"}),
        json!({"kind":"secret","path":"providers.models.openai.fixture.api_key","populated":true}),
    ] {
        let typed: ConfigPropertyOutput = serde_json::from_value(value.clone()).unwrap();
        assert!(matches!(
            typed.kind,
            ConfigPropertyKind::Value | ConfigPropertyKind::Secret
        ));
        assert_eq!(serde_json::to_value(typed).unwrap(), value);
    }
}
