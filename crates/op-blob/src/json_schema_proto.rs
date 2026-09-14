//! Shared JSON Schema field mapping for sealing, build and runtime gRPC adapters.
//! Contracts are supplied by PluginSchema; this module owns no declarations.
//!
//! Dynamic Value/Struct/ListValue numbers use protobuf's IEEE-754 doubles.
//! Declared integer fields use int64/uint64 and retain their full range.
#[cfg(test)]
use serde_json::json;
use serde_json::Value;

/// Proto3 scalar presence distinguishes an omitted argument from an explicit
/// false, zero, or empty string before validating the owning plugin schema.
pub fn needs_scalar_presence(proto_type: &str, repeated: bool) -> bool {
    !repeated && !proto_type.starts_with("google.protobuf.")
}

pub fn json_schema_type_to_proto(schema: &Value) -> (String, bool) {
    classify(schema, 0)
}

fn classify(schema: &Value, depth: usize) -> (String, bool) {
    if depth >= 64 {
        return ("google.protobuf.Value".into(), false);
    }
    if let Some(alternatives) = schema
        .get("anyOf")
        .or_else(|| schema.get("oneOf"))
        .and_then(Value::as_array)
    {
        // Value preserves null and heterogeneous unions without inventing a
        // default scalar, or discarding one branch of the plugin's contract.
        if alternatives.len() != 1 {
            return ("google.protobuf.Value".into(), false);
        }
        return classify(&alternatives[0], depth + 1);
    }
    if let Some(kinds) = schema.get("type").and_then(Value::as_array) {
        if kinds.len() != 1 || kinds[0] == "null" {
            return ("google.protobuf.Value".into(), false);
        }
        let mut single = schema.clone();
        single["type"] = kinds[0].clone();
        return classify(&single, depth + 1);
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return if values.iter().all(Value::is_string) {
            ("string".into(), false)
        } else {
            ("google.protobuf.Value".into(), false)
        };
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("string") => ("string".into(), false),
        Some("boolean") => ("bool".into(), false),
        Some("integer") => {
            let unsigned = schema
                .get("format")
                .and_then(Value::as_str)
                .is_some_and(|format| format.starts_with("uint"))
                || schema
                    .get("minimum")
                    .and_then(Value::as_f64)
                    .is_some_and(|minimum| minimum >= 0.0);
            (if unsigned { "uint64" } else { "int64" }.into(), false)
        }
        Some("number") => ("double".into(), false),
        Some("array") => {
            let (kind, repeated) = schema
                .get("items")
                .map(|item| classify(item, depth + 1))
                .unwrap_or_else(|| ("google.protobuf.Value".into(), false));
            // Protobuf has no repeated repeated scalar. Each inner array is
            // represented by ListValue so matrix boundaries survive the wire.
            (
                if repeated {
                    "google.protobuf.ListValue".into()
                } else {
                    kind
                },
                true,
            )
        }
        Some("object") => ("google.protobuf.Struct".into(), false),
        _ if schema.get("properties").is_some() => ("google.protobuf.Struct".into(), false),
        _ => ("google.protobuf.Value".into(), false),
    }
}

#[test]
fn preserves_matrix_null_and_unsigned_contracts() {
    assert_eq!(
        json_schema_type_to_proto(
            &json!({"type":"array","items":{"type":"array","items":{"type":"number"}}})
        ),
        ("google.protobuf.ListValue".into(), true)
    );
    assert_eq!(
        json_schema_type_to_proto(&json!({"type":["string","null"]})),
        ("google.protobuf.Value".into(), false)
    );
    assert_eq!(
        json_schema_type_to_proto(&json!({"type":"integer","format":"uint64","minimum":0})),
        ("uint64".into(), false)
    );
    assert_eq!(
        json_schema_type_to_proto(&json!({"type":"integer"})),
        ("int64".into(), false)
    );
    assert_eq!(
        json_schema_type_to_proto(&json!({"enum":[1,2]})),
        ("google.protobuf.Value".into(), false)
    );
}
