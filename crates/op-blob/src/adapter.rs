//! Translator layer: schemars-derived JSON Schema 2020-12 -> `PluginSchema`.
//!
//! Port of `op-plugins/src/state_plugins/schemars_adapter.rs` (values as
//! `serde_json::Value` instead of `simd_json::OwnedValue`). Schemars is the
//! seed; it disappears at this boundary — no consumer ever reads schemars
//! output directly (spec Key Decision 1/2).

use crate::schema::{Constraint, FieldSchema, FieldType, MethodDecl, PluginSchema, SideEffect};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as JVal};
use std::collections::{HashMap, HashSet};

/// Build a `PluginSchema` from the JSON of a schemars-derived schema.
///
/// The root type's top-level properties become the plugin's `fields`; `$ref`s
/// into `$defs`/`definitions` are resolved inline so nested structs expand
/// correctly. Root/field `x-oscal-subid` extensions populate `subids`;
/// `x-immutable-paths` populates `immutable_paths`.
pub fn plugin_schema_from_json(
    name: &str,
    version: &str,
    description: &str,
    root: &JVal,
) -> PluginSchema {
    try_plugin_schema_from_json(name, version, description, root)
        .expect("plugin state schema cannot be represented without loss")
}

/// Fallible producer boundary. Rejects recursive references, mixed dynamic /
/// named objects, and unsupported map-value constraints before conversion.
/// This never reads a catalog or establishes runtime plugin existence.
pub fn try_plugin_schema_from_json(
    name: &str,
    version: &str,
    description: &str,
    root: &JVal,
) -> Result<PluginSchema, op_state_store::state_schema_shape::StateSchemaShapeError> {
    let prepared = op_state_store::state_schema_shape::prepare_state_schema(root)?;
    let root = &prepared;
    let defs = root
        .get("$defs")
        .or_else(|| root.get("definitions"))
        .and_then(JVal::as_object);

    let top = resolve(root, defs);
    let required = required_set(top);

    let mut fields = HashMap::new();
    let mut subids = HashMap::new();
    if let Some(props) = top.get("properties").and_then(JVal::as_object) {
        for (fname, fnode) in props {
            let req = required.contains(fname.as_str());
            fields.insert(fname.clone(), field_schema(fnode, defs, req));
            if let Some(subid) = fnode.get("x-oscal-subid").and_then(JVal::as_str) {
                subids.insert(fname.clone(), subid.to_string());
            }
        }
    }

    // Root-level `#[schemars(extend("x-oscal-subid" = ...))]` -> reserved key.
    if let Some(subid) = root.get("x-oscal-subid").and_then(JVal::as_str) {
        subids.insert("__schema__".to_string(), subid.to_string());
    }

    let mut schema = PluginSchema::builder(name)
        .version(version)
        .description(description)
        .build();
    schema.fields = fields;
    schema.subids = subids;
    if let Some(paths) = root.get("x-immutable-paths").and_then(JVal::as_array) {
        schema.immutable_paths = paths
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    Ok(schema)
}

/// Follow a single `$ref` into `$defs`/`definitions`, recursively.
fn resolve<'a>(node: &'a JVal, defs: Option<&'a Map<String, JVal>>) -> &'a JVal {
    if let (Some(r), Some(d)) = (node.get("$ref").and_then(JVal::as_str), defs)
        && let Some(key) = r.rsplit('/').next()
    {
        let key = op_state_store::state_schema_shape::decode_component(key)
            .expect("producer preflight checked the local reference");
        if let Some(target) = d.get(&key) {
            return resolve(target, defs);
        }
    }
    node
}

fn required_set(node: &JVal) -> HashSet<String> {
    node.get("required")
        .and_then(JVal::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn field_type(node: &JVal, defs: Option<&Map<String, JVal>>) -> FieldType {
    // Preserve an explicit field opt-in even when its body is a local $ref.
    let node = if node
        .get("x-opdbus-exact-json-schema")
        .and_then(JVal::as_bool)
        == Some(true)
    {
        node
    } else {
        resolve(node, defs)
    };
    if node
        .get("x-opdbus-exact-json-schema")
        .and_then(JVal::as_bool)
        == Some(true)
    {
        let document = op_state_store::exact_schema::self_contained_field_schema(node, defs)
            .expect("producer preflight admitted the explicit exact field");
        return FieldType::JsonSchema(Box::new(document));
    }

    // `anyOf`/`oneOf`: Option<T> collapses to T; multi-branch unions become a
    // real discriminated `OneOf` so all variants render.
    if let Some(alternatives) = node
        .get("anyOf")
        .or_else(|| node.get("oneOf"))
        .and_then(JVal::as_array)
    {
        let non_null: Vec<&JVal> = alternatives
            .iter()
            .filter(|a| type_str(a) != Some("null"))
            .collect();
        match non_null.as_slice() {
            [] => {}
            [single] => return field_type(single, defs),
            many => {
                return FieldType::OneOf(many.iter().map(|a| field_type(a, defs)).collect());
            }
        }
    }

    if let Some(value) = node.get("const").and_then(JVal::as_str) {
        return FieldType::Enum(vec![value.to_owned()]);
    }

    if let Some(en) = node.get("enum").and_then(JVal::as_array) {
        return FieldType::Enum(
            en.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
        );
    }

    match type_str(node) {
        Some("string") => FieldType::String,
        Some("integer") => FieldType::Integer,
        Some("number") => FieldType::Float,
        Some("boolean") => FieldType::Boolean,
        Some("array") => {
            let item = node
                .get("items")
                .map(|i| field_type(i, defs))
                .unwrap_or(FieldType::Any);
            FieldType::Array(Box::new(item))
        }
        Some("object") => {
            let named = node.get("properties").and_then(JVal::as_object);
            if named.is_none_or(|properties| properties.is_empty())
                && let Some(additional) = node.get("additionalProperties")
                && additional != &JVal::Bool(false)
            {
                return FieldType::Map(Box::new(field_type(additional, defs)));
            }
            let req = required_set(node);
            let mut m = HashMap::new();
            if let Some(props) = node.get("properties").and_then(JVal::as_object) {
                for (k, v) in props {
                    m.insert(k.clone(), field_schema(v, defs, req.contains(k.as_str())));
                }
            }
            FieldType::Object(m)
        }
        _ => FieldType::Any,
    }
}

/// schemars emits `"type": "string"` or, for `Option<T>`, `"type": ["string","null"]`.
fn type_str(node: &JVal) -> Option<&str> {
    match node.get("type") {
        Some(JVal::String(s)) => Some(s.as_str()),
        Some(JVal::Array(a)) => a.iter().filter_map(JVal::as_str).find(|t| *t != "null"),
        _ => None,
    }
}

fn field_schema(node: &JVal, defs: Option<&Map<String, JVal>>, required: bool) -> FieldSchema {
    let meta = node;
    FieldSchema {
        field_type: field_type(node, defs),
        required,
        description: meta
            .get("description")
            .and_then(JVal::as_str)
            .unwrap_or_default()
            .to_string(),
        default: meta.get("default").cloned(),
        example: example_of(meta).cloned(),
        constraints: constraints(constraint_node(meta, defs)),
        read_only: meta
            .get("readOnly")
            .and_then(JVal::as_bool)
            .unwrap_or(false),
        read_only_when: None,
    }
}

// The finite producer preflight admits only one non-null optional branch
// when a shared FieldSchema must carry its constraints.
fn constraint_node<'a>(node: &'a JVal, defs: Option<&'a Map<String, JVal>>) -> &'a JVal {
    let resolved = resolve(node, defs);
    if let Some(alternatives) = resolved
        .get("anyOf")
        .or_else(|| resolved.get("oneOf"))
        .and_then(JVal::as_array)
    {
        let mut concrete = alternatives
            .iter()
            .filter(|node| type_str(node) != Some("null"));
        if let (Some(single), None) = (concrete.next(), concrete.next()) {
            return constraint_node(single, defs);
        }
    }
    resolved
}

fn example_of(node: &JVal) -> Option<&JVal> {
    if let Some(first) = node
        .get("examples")
        .and_then(JVal::as_array)
        .and_then(|a| a.first())
    {
        return Some(first);
    }
    node.get("example")
}

fn constraints(node: &JVal) -> Vec<Constraint> {
    let mut out = Vec::new();
    if let Some(m) = node.get("minimum").and_then(JVal::as_f64) {
        out.push(Constraint::Min { value: m });
    }
    if let Some(m) = node.get("maximum").and_then(JVal::as_f64) {
        out.push(Constraint::Max { value: m });
    }
    if let Some(p) = node.get("pattern").and_then(JVal::as_str) {
        out.push(Constraint::Pattern {
            regex: p.to_string(),
        });
    }
    out
}

/// Recursively overwrite field defaults with values from the serialized
/// default state, so the schema reflects the typed state's custom `Default`
/// impl (which schemars' derive cannot always see for nested structs).
pub fn apply_state_defaults(schema: &mut PluginSchema, state: &JVal) {
    fn set_field_defaults(field: &mut FieldSchema, value: &JVal) {
        field.default = Some(value.clone());
        if let FieldType::Object(ref mut inner) = field.field_type
            && let Some(obj) = value.as_object()
        {
            for (k, v) in obj.iter() {
                if let Some(f) = inner.get_mut(k.as_str()) {
                    set_field_defaults(f, v);
                }
            }
        }
    }

    if let Some(obj) = state.as_object() {
        for (k, v) in obj.iter() {
            if let Some(f) = schema.fields.get_mut(k.as_str()) {
                set_field_defaults(f, v);
            }
        }
    }
}

/// REQ-4.1: the only sanctioned way to build a `MethodDecl` — typed input AND
/// typed output structs, both schemars-derived. The deprecated input-only
/// variant is deliberately not ported.
pub fn method_decl_from_schemars_with_output<
    Input: schemars::JsonSchema,
    Output: schemars::JsonSchema,
>(
    name: &str,
    side_effect: SideEffect,
    idempotent: bool,
    required_capability: &str,
    subid: &str,
) -> MethodDecl {
    let args = serde_json::to_value(schemars::schema_for!(Input))
        .expect("input schema serializes cleanly");
    let returns = serde_json::to_value(schemars::schema_for!(Output))
        .expect("output schema serializes cleanly");
    MethodDecl {
        name: name.to_string(),
        args,
        returns: Some(returns),
        side_effect,
        idempotent,
        required_capability: Some(required_capability.to_string()),
        subid: subid.to_string(),
    }
}

/// Standard acknowledgment output for success/failure-only methods (REQ-4.3).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AckOutput {
    pub success: bool,
}

/// Input for methods that take no arguments.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EmptyInput {}

/// Full-fidelity drift diff between a reference schema and a derived schema
/// (REQ-2.5 drift-guard tests). Empty result ⇒ exact reproduction.
pub fn schema_diffs(reference: &PluginSchema, derived: &PluginSchema) -> Vec<String> {
    let mut d = Vec::new();
    if reference.name != derived.name {
        d.push(format!("name: {:?} -> {:?}", reference.name, derived.name));
    }
    if reference.version != derived.version {
        d.push(format!(
            "version: {:?} -> {:?}",
            reference.version, derived.version
        ));
    }
    if reference.description != derived.description {
        d.push(format!(
            "description: {:?} -> {:?}",
            reference.description, derived.description
        ));
    }
    let (mut ir, mut id) = (
        reference.immutable_paths.clone(),
        derived.immutable_paths.clone(),
    );
    ir.sort();
    id.sort();
    if ir != id {
        d.push(format!("immutable_paths: {ir:?} -> {id:?}"));
    }
    if reference.subids != derived.subids {
        d.push(format!(
            "subids: {:?} -> {:?}",
            reference.subids, derived.subids
        ));
    }
    let keys: std::collections::BTreeSet<&String> = reference
        .fields
        .keys()
        .chain(derived.fields.keys())
        .collect();
    for k in keys {
        match (reference.fields.get(k), derived.fields.get(k)) {
            (Some(r), Some(v)) => field_diffs(&mut d, k, r, v),
            (Some(_), None) => d.push(format!("{k}: DROPPED (in reference, not derived)")),
            (None, Some(_)) => d.push(format!("{k}: ADDED (in derived, not reference)")),
            (None, None) => {}
        }
    }
    d
}

fn field_diffs(d: &mut Vec<String>, prefix: &str, r: &FieldSchema, v: &FieldSchema) {
    if type_tag(&r.field_type) != type_tag(&v.field_type) {
        d.push(format!(
            "{prefix}.type: {} -> {}",
            type_tag(&r.field_type),
            type_tag(&v.field_type)
        ));
    }
    if r.required != v.required {
        d.push(format!(
            "{prefix}.required: {} -> {}",
            r.required, v.required
        ));
    }
    if r.description != v.description {
        d.push(format!(
            "{prefix}.description: {:?} -> {:?}",
            r.description, v.description
        ));
    }
    if r.default != v.default {
        d.push(format!(
            "{prefix}.default: {:?} -> {:?}",
            r.default, v.default
        ));
    }
    if r.example != v.example {
        d.push(format!(
            "{prefix}.example: {:?} -> {:?}",
            r.example, v.example
        ));
    }
    if constraint_tags(&r.constraints) != constraint_tags(&v.constraints) {
        d.push(format!(
            "{prefix}.constraints: {:?} -> {:?}",
            constraint_tags(&r.constraints),
            constraint_tags(&v.constraints)
        ));
    }
    if r.read_only != v.read_only {
        d.push(format!(
            "{prefix}.read_only: {} -> {}",
            r.read_only, v.read_only
        ));
    }
    field_type_diffs(d, prefix, &r.field_type, &v.field_type);
}

fn field_type_diffs(d: &mut Vec<String>, prefix: &str, r: &FieldType, v: &FieldType) {
    match (r, v) {
        (FieldType::JsonSchema(left), FieldType::JsonSchema(right)) if left != right => {
            d.push(format!("{prefix}: exact schema changed"));
        }
        (FieldType::Map(r_inner), FieldType::Map(v_inner)) => {
            field_type_diffs(d, &format!("{prefix}{{value}}"), r_inner, v_inner);
        }
        (FieldType::Array(r_inner), FieldType::Array(v_inner)) => {
            field_type_diffs(d, &format!("{prefix}[]"), r_inner, v_inner);
        }
        (FieldType::Object(r_map), FieldType::Object(v_map)) => {
            let keys: std::collections::BTreeSet<&String> =
                r_map.keys().chain(v_map.keys()).collect();
            for k in keys {
                match (r_map.get(k), v_map.get(k)) {
                    (Some(rf), Some(vf)) => field_diffs(d, &format!("{prefix}.{k}"), rf, vf),
                    (Some(_), None) => {
                        d.push(format!("{prefix}.{k}: DROPPED (in reference, not derived)"))
                    }
                    (None, Some(_)) => {
                        d.push(format!("{prefix}.{k}: ADDED (in derived, not reference)"))
                    }
                    (None, None) => {}
                }
            }
        }
        (FieldType::OneOf(r_branches), FieldType::OneOf(v_branches)) => {
            if r_branches.len() != v_branches.len() {
                d.push(format!(
                    "{prefix}: one_of branch count differs ({} vs {})",
                    r_branches.len(),
                    v_branches.len()
                ));
            }
            for (i, (rb, vb)) in r_branches.iter().zip(v_branches.iter()).enumerate() {
                field_type_diffs(d, &format!("{prefix}|{i}"), rb, vb);
            }
        }
        _ => {}
    }
}

fn type_tag(t: &FieldType) -> String {
    match t {
        FieldType::String => "string".into(),
        FieldType::Integer => "integer".into(),
        FieldType::Float => "float".into(),
        FieldType::Boolean => "boolean".into(),
        FieldType::JsonSchema(_) => "json_schema".into(),
        FieldType::Any => "any".into(),
        FieldType::Enum(_) => "enum".into(),
        FieldType::OneOf(branches) => {
            let mut tags: Vec<String> = branches.iter().map(type_tag).collect();
            tags.sort();
            format!("one_of<{}>", tags.join("|"))
        }
        FieldType::Map(i) => format!("map<string,{}>", type_tag(i)),
        FieldType::Array(i) => format!("array<{}>", type_tag(i)),
        FieldType::Object(m) => {
            let mut entries: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("{k}:{}", type_tag(&v.field_type)))
                .collect();
            entries.sort();
            format!("object{{{}}}", entries.join(","))
        }
    }
}

fn constraint_tags(cs: &[Constraint]) -> Vec<String> {
    let mut v: Vec<String> = cs
        .iter()
        .map(|c| match c {
            Constraint::Min { value } => format!("min:{value}"),
            Constraint::Max { value } => format!("max:{value}"),
            Constraint::Pattern { regex } => format!("pattern:{regex}"),
            Constraint::OneOf { .. } => "oneof".into(),
            Constraint::RequiresField { field } => format!("requires:{field}"),
            Constraint::Custom { validator } => format!("custom:{validator}"),
        })
        .collect();
    v.sort();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walks_array_of_objects_with_constraints() {
        // Mirrors what schemars derives for UnixSocketState { sockets: Vec<SocketEndpoint> }.
        let root: JVal = serde_json::from_str(
            r##"{
            "type":"object",
            "properties":{"sockets":{"type":"array","description":"Declared endpoints",
                "items":{"$ref":"#/$defs/SocketEndpoint"}}},
            "required":["sockets"],
            "$defs":{"SocketEndpoint":{"type":"object",
                "properties":{
                    "path":{"type":"string","description":"socket path"},
                    "port":{"type":"integer","minimum":1,"maximum":65535}},
                "required":["path","port"]}}
        }"##,
        )
        .unwrap();
        let s = plugin_schema_from_json("unix_socket", "1.0.0", "desc", &root);

        let sockets = s.fields.get("sockets").expect("sockets field");
        assert!(sockets.required);
        match &sockets.field_type {
            FieldType::Array(inner) => match inner.as_ref() {
                FieldType::Object(props) => {
                    assert!(props.contains_key("path"));
                    let port = props.get("port").unwrap();
                    assert!(port.required);
                    assert_eq!(port.constraints.len(), 2); // Min + Max
                }
                other => panic!("expected object items, got {other:?}"),
            },
            other => panic!("expected array, got {other:?}"),
        }
    }

    #[test]
    fn ingests_root_and_field_subids() {
        let schema = plugin_schema_from_json(
            "subid_test",
            "1.0.0",
            "desc",
            &serde_json::json!({
                "type": "object",
                "x-oscal-subid": "sch.software.test-schema.describe@v1",
                "properties": {
                    "field_a": {
                        "type": "string",
                        "x-oscal-subid": "exp.software.test-field.render@v1"
                    }
                },
                "required": ["field_a"]
            }),
        );
        assert_eq!(
            schema.subids.get("__schema__"),
            Some(&"sch.software.test-schema.describe@v1".to_string())
        );
        assert_eq!(
            schema.subids.get("field_a"),
            Some(&"exp.software.test-field.render@v1".to_string())
        );
    }

    #[test]
    fn reports_nested_mismatch() {
        let mk = |inner_type: &str| {
            plugin_schema_from_json(
                "nested_test",
                "1.0.0",
                "desc",
                &serde_json::json!({
                    "type": "object",
                    "properties": {
                        "wrapper": {
                            "type": "object",
                            "properties": {
                                "inner": { "type": inner_type, "description": "inner field" }
                            },
                            "required": ["inner"]
                        }
                    },
                    "required": ["wrapper"]
                }),
            )
        };
        let diffs = schema_diffs(&mk("string"), &mk("integer"));
        assert!(
            diffs
                .iter()
                .any(|d| d.contains("wrapper.inner") && d.contains("type")),
            "expected nested inner.type diff, got: {diffs:#?}"
        );
    }
}
