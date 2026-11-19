//! Canonical JSON and argument hashing.
//!
//! Object keys are sorted recursively and no whitespace is emitted, so the
//! same arguments always hash the same regardless of `serde_json` feature
//! unification (`preserve_order`) elsewhere in the workspace.

use serde_json::Value;
use sha2::{Digest, Sha256};

/// Render `value` as canonical JSON (sorted keys, no whitespace).
pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, &mut out);
    out
}

fn write_value(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&Value::String((*key).clone()).to_string());
                out.push(':');
                write_value(&map[*key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        other => out.push_str(&other.to_string()),
    }
}

/// Lower-case hex SHA-256 of arbitrary bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Hex SHA-256 of the canonical JSON of `args`.
pub fn args_sha256(args: &Value) -> String {
    sha256_hex(canonical_json(args).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn key_order_does_not_change_the_canonical_form() {
        let a: Value =
            serde_json::from_str(r#"{"b":1,"a":{"y":[1,{"d":2,"c":3}],"x":null}}"#).unwrap();
        let b: Value =
            serde_json::from_str(r#"{"a":{"x":null,"y":[1,{"c":3,"d":2}]},"b":1}"#).unwrap();
        assert_eq!(canonical_json(&a), canonical_json(&b));
        assert_eq!(
            canonical_json(&a),
            r#"{"a":{"x":null,"y":[1,{"c":3,"d":2}]},"b":1}"#
        );
        assert_eq!(args_sha256(&a), args_sha256(&b));
    }

    #[test]
    fn strings_are_escaped_and_hash_is_stable() {
        let value = json!({"k":"a\"b\n"});
        assert_eq!(canonical_json(&value), r#"{"k":"a\"b\n"}"#);
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_ne!(args_sha256(&json!({"a":1})), args_sha256(&json!({"a":2})));
    }
}
