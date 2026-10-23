//! Schema-described projection of a method's arguments (OD-64 / OD-91).
//!
//! The sealed plugin schema is the noise filter for retrieval: only the
//! fields a method's `args` schema *describes* are worth embedding, rendered
//! with the description the schema author wrote; fields marked `x-secret` or
//! `x-opdbus-private` are dropped with their whole subtree; anything the
//! schema does not declare (hashes, transport metadata, stray keys) is left
//! out. The chain block stays lossless — this is the retrieval view of it,
//! not a replacement.

use serde_json::Value;

/// One argument field the schema describes, with the value it carried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescribedField {
    /// Dotted path from the args root, arrays as `[i]`.
    pub path: String,
    /// `description` from the schema (or `title`), empty when absent.
    pub description: String,
    /// `x-oscal-subid` when declared.
    pub subid: Option<String>,
    /// Scalar rendering of the value (bounded by the caller).
    pub value: String,
}

const MAX_DEPTH: usize = 32;
const MAX_FIELDS: usize = 256;

/// Walk `value` under `schema`, emitting described scalar leaves.
pub fn described_fields(schema: &Value, value: &Value) -> Vec<DescribedField> {
    let mut out = Vec::new();
    walk(schema, schema, value, "", &mut out, 0);
    out
}

fn is_secret(schema: &Value) -> bool {
    schema.get("x-secret").and_then(Value::as_bool) == Some(true)
}

/// OD-91: private content stays off shared chain vectors the same way secrets do.
fn is_private(schema: &Value) -> bool {
    schema.get("x-opdbus-private").and_then(Value::as_bool) == Some(true)
}

fn is_redacted(schema: &Value) -> bool {
    is_secret(schema) || is_private(schema)
}

fn resolve<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    if let Some(reference) = schema.get("$ref").and_then(Value::as_str)
        && let Some(pointer) = reference.strip_prefix('#')
        && let Some(target) = root.pointer(pointer)
    {
        return target;
    }
    schema
}

/// Merge `allOf` branches' properties view: the first branch that declares
/// the property wins; combinators are searched for property declarations.
fn property_schema<'a>(root: &'a Value, schema: &'a Value, key: &str) -> Option<&'a Value> {
    let schema = resolve(root, schema);
    if let Some(found) = schema.get("properties").and_then(|p| p.get(key)) {
        return Some(found);
    }
    for combinator in ["allOf", "anyOf", "oneOf"] {
        if let Some(branches) = schema.get(combinator).and_then(Value::as_array) {
            for branch in branches {
                if let Some(found) = property_schema(root, branch, key) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Treat secrecy / privacy as a conservative annotation across every
/// applicable schema branch. JSON Schema `allOf` combines constraints, so
/// selecting only the first property declaration must not discard a later
/// `x-secret: true` or `x-opdbus-private: true`. Applying the same rule to
/// alternatives avoids exposing a value when any accepted shape redacts it.
fn property_is_redacted(root: &Value, schema: &Value, key: &str) -> bool {
    let schema = resolve(root, schema);
    if schema
        .get("properties")
        .and_then(|properties| properties.get(key))
        .is_some_and(|property| is_redacted(resolve(root, property)))
    {
        return true;
    }

    ["allOf", "anyOf", "oneOf"].into_iter().any(|combinator| {
        schema
            .get(combinator)
            .and_then(Value::as_array)
            .is_some_and(|branches| {
                branches
                    .iter()
                    .any(|branch| property_is_redacted(root, branch, key))
            })
    })
}

fn description_of(root: &Value, schema: &Value) -> (String, Option<String>) {
    let schema = resolve(root, schema);
    let description = schema
        .get("description")
        .or_else(|| schema.get("title"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let subid = schema
        .get("x-oscal-subid")
        .and_then(Value::as_str)
        .map(str::to_string);
    (description, subid)
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

fn walk(
    root: &Value,
    schema: &Value,
    value: &Value,
    path: &str,
    out: &mut Vec<DescribedField>,
    depth: usize,
) {
    if depth > MAX_DEPTH || out.len() >= MAX_FIELDS {
        return;
    }
    let schema = resolve(root, schema);
    if is_redacted(schema) {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                // Undeclared keys are noise: not described, not embedded.
                let Some(child_schema) = property_schema(root, schema, key) else {
                    continue;
                };
                if property_is_redacted(root, schema, key) {
                    continue;
                }
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                walk(root, child_schema, child, &child_path, out, depth + 1);
            }
        }
        Value::Array(items) => {
            let Some(item_schema) = schema.get("items") else {
                return;
            };
            for (index, item) in items.iter().enumerate() {
                walk(
                    root,
                    item_schema,
                    item,
                    &format!("{path}[{index}]"),
                    out,
                    depth + 1,
                );
            }
        }
        scalar => {
            if let Some(text) = scalar_text(scalar) {
                let (description, subid) = description_of(root, schema);
                out.push(DescribedField {
                    path: path.to_string(),
                    description,
                    subid,
                    value: text,
                });
            }
        }
    }
}

/// OD-91: replace `x-opdbus-private` leaves with blake3 digests so the chain
/// stays hash-verifiable without storing private text. Secrets stay omitted.
pub fn commit_with_private_digests(schema: &Value, value: &Value) -> Value {
    digest_walk(schema, schema, value, 0)
}

fn digest_walk(root: &Value, schema: &Value, value: &Value, depth: usize) -> Value {
    if depth > MAX_DEPTH {
        return Value::Null;
    }
    let schema = resolve(root, schema);
    if is_secret(schema) {
        return Value::Null;
    }
    if is_private(schema) {
        return Value::String(format!(
            "blake3:{}",
            blake3::hash(value.to_string().as_bytes())
        ));
    }
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, child) in map {
                let Some(child_schema) = property_schema(root, schema, key) else {
                    // Preserve undeclared keys as-is for audit completeness of
                    // non-schema noise; callers that want projection use
                    // `described_fields` instead.
                    out.insert(key.clone(), child.clone());
                    continue;
                };
                if property_is_secret_only(root, schema, key) {
                    continue;
                }
                if property_is_private_only(root, schema, key) {
                    out.insert(
                        key.clone(),
                        Value::String(format!(
                            "blake3:{}",
                            blake3::hash(child.to_string().as_bytes())
                        )),
                    );
                    continue;
                }
                out.insert(
                    key.clone(),
                    digest_walk(root, child_schema, child, depth + 1),
                );
            }
            Value::Object(out)
        }
        Value::Array(items) => {
            let Some(item_schema) = schema.get("items") else {
                return value.clone();
            };
            Value::Array(
                items
                    .iter()
                    .map(|item| digest_walk(root, item_schema, item, depth + 1))
                    .collect(),
            )
        }
        other => other.clone(),
    }
}

fn property_is_secret_only(root: &Value, schema: &Value, key: &str) -> bool {
    let schema = resolve(root, schema);
    if schema
        .get("properties")
        .and_then(|properties| properties.get(key))
        .is_some_and(|property| is_secret(resolve(root, property)))
    {
        return true;
    }
    ["allOf", "anyOf", "oneOf"].into_iter().any(|combinator| {
        schema
            .get(combinator)
            .and_then(Value::as_array)
            .is_some_and(|branches| {
                branches
                    .iter()
                    .any(|branch| property_is_secret_only(root, branch, key))
            })
    })
}

fn property_is_private_only(root: &Value, schema: &Value, key: &str) -> bool {
    let schema = resolve(root, schema);
    if schema
        .get("properties")
        .and_then(|properties| properties.get(key))
        .is_some_and(|property| {
            let resolved = resolve(root, property);
            is_private(resolved) && !is_secret(resolved)
        })
    {
        return true;
    }
    ["allOf", "anyOf", "oneOf"].into_iter().any(|combinator| {
        schema
            .get(combinator)
            .and_then(Value::as_array)
            .is_some_and(|branches| {
                branches
                    .iter()
                    .any(|branch| property_is_private_only(root, branch, key))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_described_non_secret_fields_survive_with_their_descriptions() {
        let schema = json!({
            "type": "object",
            "properties": {
                "message": {"type": "string", "description": "What the user asked", "x-oscal-subid": "obs.x.message@v1"},
                "api_key": {"type": "string", "x-secret": true, "description": "never"},
                "route": {"$ref": "#/$defs/Route"},
                "tags": {"type": "array", "items": {"type": "string", "description": "A tag"}},
                "nested": {"allOf": [{"properties": {"depth": {"type": "integer", "title": "Depth"}}}]}
            },
            "$defs": {
                "Route": {"type": "object", "properties": {
                    "provider": {"type": "string", "description": "Upstream provider"},
                    "token": {"type": "string", "x-secret": true}
                }}
            }
        });
        let value = json!({
            "message": "hello",
            "api_key": "sk-live-123",
            "route": {"provider": "anthropic", "token": "t0k"},
            "tags": ["a", "b"],
            "nested": {"depth": 3},
            "event_hash": "deadbeef",
            "extra": {"stray": true}
        });
        let fields = described_fields(&schema, &value);
        let rendered: Vec<String> = fields
            .iter()
            .map(|f| format!("{}={} ({})", f.path, f.value, f.description))
            .collect();
        assert_eq!(
            rendered,
            // Sorted key order: deterministic on origin and replica alike.
            vec![
                "message=hello (What the user asked)",
                "nested.depth=3 (Depth)",
                "route.provider=anthropic (Upstream provider)",
                "tags[0]=a (A tag)",
                "tags[1]=b (A tag)",
            ]
        );
        assert_eq!(fields[0].subid.as_deref(), Some("obs.x.message@v1"));
        let joined = rendered.join("\n");
        assert!(!joined.contains("sk-live"));
        assert!(!joined.contains("t0k"));
        assert!(!joined.contains("deadbeef"));
        assert!(!joined.contains("stray"));
    }

    #[test]
    fn later_all_of_secret_annotation_suppresses_the_property() {
        let schema = json!({
            "type": "object",
            "allOf": [
                {
                    "properties": {
                        "token": {
                            "type": "string",
                            "description": "First declaration"
                        }
                    }
                },
                {
                    "properties": {
                        "token": {
                            "type": "string",
                            "x-secret": true
                        }
                    }
                }
            ]
        });

        let fields = described_fields(&schema, &json!({"token": "do-not-embed"}));
        assert!(fields.is_empty());
    }

    #[test]
    fn opdbus_private_fields_are_dropped_like_secrets() {
        let schema = json!({
            "type": "object",
            "properties": {
                "outcome": {"type": "string", "description": "Episode outcome"},
                "goal_text": {
                    "type": "string",
                    "description": "Prompt [PII]",
                    "x-opdbus-private": true
                },
                "nested": {
                    "type": "object",
                    "properties": {
                        "decision_output": {
                            "type": "string",
                            "x-opdbus-private": true,
                            "description": "Reply"
                        },
                        "actor_role": {"type": "string", "description": "Actor"}
                    }
                }
            }
        });
        let value = json!({
            "outcome": "config_set",
            "goal_text": "secret prompt text",
            "nested": {
                "decision_output": "secret reply",
                "actor_role": "human"
            }
        });
        let fields = described_fields(&schema, &value);
        let rendered: Vec<String> = fields
            .iter()
            .map(|f| format!("{}={}", f.path, f.value))
            .collect();
        assert_eq!(
            rendered,
            vec!["nested.actor_role=human", "outcome=config_set"]
        );
        let joined = rendered.join("\n");
        assert!(!joined.contains("secret prompt"));
        assert!(!joined.contains("secret reply"));
    }

    #[test]
    fn private_fields_become_digests_for_chain_commit() {
        let schema = json!({
            "type": "object",
            "properties": {
                "outcome": {"type": "string"},
                "goal_text": {"type": "string", "x-opdbus-private": true},
                "api_key": {"type": "string", "x-secret": true}
            }
        });
        let value = json!({
            "outcome": "config_set",
            "goal_text": "do not store plaintext",
            "api_key": "sk-live"
        });
        let committed = commit_with_private_digests(&schema, &value);
        assert_eq!(committed["outcome"], json!("config_set"));
        assert!(committed.get("api_key").is_none());
        let digest = committed["goal_text"].as_str().unwrap();
        assert!(digest.starts_with("blake3:"));
        assert!(!digest.contains("plaintext"));
        let expected = format!(
            "blake3:{}",
            blake3::hash(json!("do not store plaintext").to_string().as_bytes())
        );
        assert_eq!(digest, expected);
    }
}
