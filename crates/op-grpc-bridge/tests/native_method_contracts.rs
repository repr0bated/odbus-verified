//! Backend fixtures live with their plugins; these cases cover the independent
//! generated protobuf boundary, where a valid domain result must not lose data.
use op_plugins::default_registry::DefaultPluginRegistry;
use op_state_store::MemoryStore;
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage, SerializeOptions};
use serde::de::DeserializeSeed;
use serde_json::{json, Value};
use std::sync::Arc;

fn pascal(name: &str) -> String {
    name.split('_')
        .map(|part| format!("{}{}", part[..1].to_uppercase(), &part[1..]))
        .collect()
}

fn assert_preserved(expected: &Value, actual: &Value, path: &str) {
    match (expected, actual) {
        (Value::Number(a), Value::Number(b)) => {
            if a.is_u64() && b.is_u64() {
                assert_eq!(a.as_u64(), b.as_u64(), "{path}");
            } else if a.is_i64() && b.is_i64() {
                assert_eq!(a.as_i64(), b.as_i64(), "{path}");
            } else {
                assert_eq!(a.as_f64(), b.as_f64(), "{path}");
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                assert_preserved(a, b, &format!("{path}[{index}]"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                assert_preserved(value, &b[key], &format!("{path}.{key}"));
            }
        }
        _ => assert_eq!(expected, actual, "{path}"),
    }
}

#[tokio::test]
async fn required_scalar_presence_survives_native_wire() {
    let registry = DefaultPluginRegistry::new(Arc::new(MemoryStore::new()));
    let schema = registry
        .load_plugin("login1")
        .await
        .unwrap()
        .schema()
        .unwrap();
    let contract = serde_json::to_value(&schema.methods["set_user_linger"].args).unwrap();
    let pool = DescriptorPool::decode(op_grpc_bridge::proto::FILE_DESCRIPTOR_SET).unwrap();
    let descriptor = pool
        .get_message_by_name("operation.plugin.v1.Login1SetUserLingerRequest")
        .unwrap();
    for (input, valid) in [
        (json!({"uid":0,"enable":false,"interactive":false}), true),
        (json!({"uid":0,"interactive":false}), false),
        (json!({"enable":false,"interactive":false}), false),
    ] {
        let encoded = serde_json::to_string(&input).unwrap();
        let mut deserializer = serde_json::Deserializer::from_str(&encoded);
        let message = descriptor.clone().deserialize(&mut deserializer).unwrap();
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
        let actual: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            input, actual,
            "explicit zero/false and missing fields must remain distinct"
        );
        assert_eq!(jsonschema::validate(&contract, &actual).is_ok(), valid);
    }
}

#[tokio::test]
async fn native_results_survive_generated_protobuf() {
    let cases = [
        (
            "embedding_model",
            "embed",
            json!({"vectors":[[0.25,0.75],[0.5,0.5]],"dimensions":2,"model":"fixture-model","provider_model":null,"provider":"openai_compatible","config_revision":9007199254740993u64}),
        ),
        (
            "embedding_model",
            "get_config",
            json!({"config":{"provider":"ollama","model_id":"fixture","endpoint":"http://127.0.0.1:11434/api/embed","dimensions":2,"request_dimensions":false,"timeout_ms":30000},"revision":9007199254740993u64}),
        ),
        (
            "config",
            "get_value",
            json!({"value":{"nested":[true,null,{"key":"📚"}]},"found":true}),
        ),
        ("config", "get_value", json!({"value":null,"found":true})),
        (
            "config",
            "export_config",
            json!({"config":{"provider":"chosen","nested":[1,null]}}),
        ),
        (
            "endpoint",
            "add_endpoint",
            json!({"endpoint":"unix:///run/test.sock","present":true,"revision":9007199254740993u64}),
        ),
        (
            "agent_config",
            "update_config",
            json!({"config":{"name":"reader","enabled":true,"model":"custom/model","tools":["inspect"]},"revision":17}),
        ),
        (
            "keyring",
            "list_collections",
            json!({"collections":[{"path":"/collection/default","label":"Personal","locked":true,"created":12,"modified":23}]}),
        ),
        (
            "login1",
            "list_users",
            json!({"users":[{"uid":1001,"user":"operator","path":"/user/_1001"}]}),
        ),
        (
            "login1",
            "create_session",
            json!({"session_id":"c2","object_path":"/session/c2","runtime_path":"/run/user/1001","uid":1001,"seat_id":"seat0","vtnr":2,"existing":false}),
        ),
        (
            "login1",
            "release_session",
            json!({"operation":"ReleaseSession","disposition":"accepted"}),
        ),
        (
            "dnsresolver",
            "resolve_hostname",
            json!({"addresses":[{"ifindex":2,"family":2,"address":[192,0,2,1]}],"canonical":"test.invalid","flags":9007199254740993u64}),
        ),
        (
            "dnsresolver",
            "resolve_record",
            json!({"records":[{"ifindex":2,"class":1,"record_type":16,"data":[3,102,111,111]}],"flags":0}),
        ),
        (
            "dnsresolver",
            "resolve_service",
            json!({"srv_data":[{"priority":1,"weight":2,"port":443,"hostname":"service.invalid","addresses":[{"ifindex":2,"family":2,"address":[192,0,2,2]}],"canonical":"service.invalid"}],"txt_data":[[3,102,111,111],[0,255]],"canonical_name":"service","canonical_type":"_https._tcp","canonical_domain":"invalid","flags":1}),
        ),
        (
            "qdrant",
            "search_points",
            json!({"results":[{"id":23,"score":0.75,"payload":{"text":"evidence","tags":["a",null]}}]}),
        ),
        (
            "qdrant",
            "scroll_points",
            json!({"points":[{"id":"abc","payload":{"value":3}}],"next_page_offset":"next-id"}),
        ),
        (
            "qdrant",
            "upsert_points",
            json!({"status":"ok","operation_id":9007199254740993u64,"operation_status":"completed"}),
        ),
        (
            "incus",
            "exec_instance",
            json!({"name":"fixture","completed":true,"exit_code":3,"stdout":"message\n","stderr":"error\n","response":{"metadata":{"return":3}}}),
        ),
        (
            "packagekit",
            "resolve",
            json!({"transaction":"/org/freedesktop/PackageKit/transactions/t1","packages":["pkg;1;x86_64;installed"],"package_details":[{"info":1,"package_id":"pkg;1;x86_64;installed","summary":"Fixture"}],"exit_code":1,"runtime_ms":50}),
        ),
        (
            "rtnetlink",
            "add_link",
            json!({"interface":"veth-fixture","disposition":"accepted"}),
        ),
        ("memory", "get_entry", json!({"entry":null})),
        (
            "memory",
            "update_entry",
            json!({"entry":{"id":"fixture-id","namespace":"notes","key":"k","value":{"items":[null,true,"📚"]},"created_at":"2026-09-13T00:00:00Z","updated_at":"2026-09-13T00:00:01Z"},"previous_value":null}),
        ),
        (
            "memory",
            "compact",
            json!({"scope":"shared_cozo_database","compacted_at":"2026-09-13T00:00:00Z","reclaimed_bytes":null}),
        ),
        (
            "gcloud_adc",
            "check_auth_status",
            json!({"provider":"google_application_default_credentials","configured":true,"authenticated":false,"validation":"rejected","failure":{"kind":"provider_rejected","retryable":false,"message":"Provider rejected the credential"}}),
        ),
        (
            "keypair",
            "list_keypairs",
            json!({"source":"ssh_agent","keypairs":[]}),
        ),
        (
            "shared_unix_socket",
            "list_registrations",
            json!({"registrations":[],"revision":9007199254740993u64}),
        ),
        (
            "unix_socket",
            "accept",
            json!({"event":null,"observed_generation":"fixture-generation","cursor":9007199254740993u64,"timed_out":false}),
        ),
        (
            "unix_socket",
            "close",
            json!({"path":"/run/fixture.sock","protocol":"grpc","phase":"closed","generation":"fixture-generation","binding_generation":3,"requested_backlog":null,"active_connections":0,"accepted_connections":9007199254740993u64}),
        ),
    ];
    let registry = DefaultPluginRegistry::new(Arc::new(MemoryStore::new()));
    let pool = DescriptorPool::decode(op_grpc_bridge::proto::FILE_DESCRIPTOR_SET).unwrap();
    for (plugin_id, method, result) in cases {
        let plugin = registry.load_plugin(plugin_id).await.unwrap();
        let schema = plugin.schema().unwrap();
        let contract = schema
            .methods
            .get(method)
            .unwrap_or_else(|| panic!("missing {plugin_id}.{method}"));
        let returns = serde_json::to_value(contract.returns.as_ref().unwrap()).unwrap();
        jsonschema::validate(&returns, &result)
            .unwrap_or_else(|error| panic!("{plugin_id}.{method}: {error}"));
        let name = format!(
            "operation.plugin.v1.{}{}Response",
            pascal(plugin_id),
            pascal(method)
        );
        let descriptor = pool.get_message_by_name(&name).unwrap();
        let source = serde_json::to_string(&result).unwrap();
        let mut input = serde_json::Deserializer::from_str(&source);
        let message = descriptor
            .clone()
            .deserialize(&mut input)
            .unwrap_or_else(|error| panic!("{name} encoding: {error}"));
        input.end().unwrap();
        let decoded =
            DynamicMessage::decode(descriptor, message.encode_to_vec().as_slice()).unwrap();
        let mut output = Vec::new();
        decoded
            .serialize_with_options(
                &mut serde_json::Serializer::new(&mut output),
                &SerializeOptions::new()
                    .use_proto_field_name(true)
                    .stringify_64_bit_integers(false)
                    .skip_default_fields(false),
            )
            .unwrap();
        let actual: Value = serde_json::from_slice(&output).unwrap();
        assert_preserved(&result, &actual, &name);
    }
}
