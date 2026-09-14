//! Derive plugin schemas from `#[derive(schemars::JsonSchema)]` structs instead
//! of hand-building `FieldSchema` maps. This is the **standard** for new plugins
//! (see `docs/schema-from-structs.md`); existing plugins migrate as their config
//! structs get fully typed.
//!
//! The adapter walks the *JSON* of the derived schema rather than schemars'
//! typed API, so it stays robust across schemars 0.8/1.0 and decoupled — a
//! plugin only needs `serde_json::to_value(schemars::schema_for!(T))`.
//!
//! Before (hand-rolled, ~80 lines/plugin, drifts from the struct):
//! ```ignore
//! let mut socket_fields = HashMap::new();
//! socket_fields.insert("path".into(), FieldSchema { field_type: String, required: true, .. });
//! socket_fields.insert("port".into(), FieldSchema {
//!     field_type: Integer,
//!     constraints: vec![Constraint::Min{1.0}, Constraint::Max{65535.0}], .. });
//! PluginSchema::builder("unix_socket").array_field("sockets", Object(socket_fields), ..)
//! ```
//!
//! After (the struct *is* the schema):
//! ```ignore
//! #[derive(Serialize, Deserialize, schemars::JsonSchema)]
//! struct SocketEndpoint {
//!     /// doc comment → field description
//!     path: String,
//!     #[schemars(range(min = 1, max = 65535))] port: u16,
//!     ..
//! }
//! plugin_schema_from_json("unix_socket", "1.0.0", DESC,
//!     &serde_json::to_value(schemars::schema_for!(UnixSocketState)).unwrap())
//! ```

use op_state_store::{Constraint, FieldSchema, FieldType, PluginSchema};
use serde_json::{Map, Value as JVal};
use simd_json::prelude::*;
use simd_json::OwnedValue;
use std::collections::{HashMap, HashSet};

/// Build a `PluginSchema` from the JSON of a schemars-derived schema.
///
/// The root type's top-level properties become the plugin's `fields`; `$ref`s
/// into `$defs`/`definitions` are resolved inline so nested structs (e.g. the
/// `SocketEndpoint` inside `sockets: Vec<SocketEndpoint>`) expand correctly.
pub fn plugin_schema_from_json(
    name: &str,
    version: &str,
    description: &str,
    root: &JVal,
) -> PluginSchema {
    try_plugin_schema_from_json(name, version, description, root).unwrap_or_else(|error| {
        panic!("plugin {name} state schema cannot be represented without loss: {error:?}")
    })
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

    // Root-level `#[schemars(extend("x-oscal-subid" = ...))]` → reserved key.
    // When a plugin declares none, derive it from the field subids it does
    // declare, so every plugin carries a schema subid without each one having
    // to paste the attribute onto its state struct. An explicit declaration
    // always wins.
    // A struct-level `#[schemars(extend("x-oscal-category" = "network"))]` is the
    // plugin stating what it is — the one fact about a schema subid that cannot
    // be derived from anything else. When absent, fall back to inferring it from
    // the field subids the plugin does declare.
    let declared_category = root.get("x-oscal-category").and_then(JVal::as_str);
    match root.get("x-oscal-subid").and_then(JVal::as_str) {
        Some(subid) => {
            subids.insert("__schema__".to_string(), subid.to_string());
        }
        None => {
            let derived = super::plugin_scaffold_helpers::derive_schema_subid(
                name,
                declared_category,
                &subids,
            );
            subids.insert("__schema__".to_string(), derived);
        }
    }

    let mut schema = PluginSchema::builder(name)
        .version(version)
        .description(description)
        .build();
    schema.fields = fields;
    schema.subids = subids;
    // Struct-level `#[schemars(extend("x-immutable-paths" = [...]))]` → immutable_paths.
    if let Some(paths) = root.get("x-immutable-paths").and_then(JVal::as_array) {
        schema.immutable_paths = paths
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }
    // A `mut.*` subid obliges the schema to carry `actor_id`/`capability_id`,
    // `src.*` obliges `source_system`/`source_locator`. Applying that here means
    // declaring the subid is enough — the accountability fields it implies
    // cannot be forgotten separately.
    super::common::oscal::ensure_category_metadata_fields(&mut schema);
    Ok(schema)
}

/// Follow a single `$ref` into `$defs`/`definitions`, recursively.
fn resolve<'a>(node: &'a JVal, defs: Option<&'a Map<String, JVal>>) -> &'a JVal {
    if let (Some(r), Some(d)) = (node.get("$ref").and_then(JVal::as_str), defs) {
        if let Some(key) = r.rsplit('/').next() {
            let key = op_state_store::state_schema_shape::decode_component(key)
                .expect("producer preflight checked the local reference");
            if let Some(target) = d.get(&key) {
                return resolve(target, defs);
            }
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
    // An explicit field opt-in can accompany a local reference. Its prepared
    // self-contained document must not be replaced by the finite target view.
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
        return FieldType::exact_json_schema(document).expect("preflight admitted exact field");
    }

    // `anyOf`/`oneOf` covers two distinct schemars patterns:
    //   * `Option<T>` (and other single-real-branch unions) emit one non-null
    //     alternative plus a `null` alternative -> collapse to the concrete type.
    //   * A `#[serde(tag = "...")]` enum emits one object alternative per
    //     variant -> build a real `FieldType::OneOf` discriminated union so all
    //     variants render instead of silently dropping all but the first.
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
            if named.is_none_or(|properties| properties.is_empty()) {
                if let Some(additional) = node.get("additionalProperties") {
                    if additional != &JVal::Bool(false) {
                        return FieldType::Map(Box::new(field_type(additional, defs)));
                    }
                }
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
/// Collapse to the first non-null type.
fn type_str(node: &JVal) -> Option<&str> {
    match node.get("type") {
        Some(JVal::String(s)) => Some(s.as_str()),
        Some(JVal::Array(a)) => a.iter().filter_map(JVal::as_str).find(|t| *t != "null"),
        _ => None,
    }
}

fn field_schema(node: &JVal, defs: Option<&Map<String, JVal>>, required: bool) -> FieldSchema {
    // Metadata such as `description`, `examples`, `readOnly`, and `default` are
    // declared on the field node itself; only the type/reference needs to be
    // resolved through `$defs`.
    let meta = node;
    FieldSchema {
        field_type: field_type(node, defs),
        required,
        description: meta
            .get("description")
            .and_then(JVal::as_str)
            .unwrap_or_default()
            .to_string(),
        default: meta.get("default").map(to_simd),
        example: example_of(meta).map(to_simd),
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

/// `serde_json::Value` → `simd_json::OwnedValue` (the `Value` op_state_store uses).
fn to_simd(v: &JVal) -> OwnedValue {
    let mut bytes = serde_json::to_vec(v).unwrap_or_default();
    simd_json::to_owned_value(&mut bytes).unwrap_or_else(|_| simd_json::json!(null))
}

/// Recursively overwrite field defaults in `schema` with the values from the
/// serialized default state. This ensures the schema's defaults reflect the
/// typed state's custom `Default` impl, which schemars' derive macro cannot
/// always see for nested structs.
pub(crate) fn apply_state_defaults(schema: &mut PluginSchema, state: &OwnedValue) {
    fn set_field_defaults(field: &mut FieldSchema, value: &OwnedValue) {
        field.default = Some(value.clone());
        if let FieldType::Object(ref mut inner) = field.field_type {
            if let Some(obj) = value.as_object() {
                for (k, v) in obj.iter() {
                    if let Some(f) = inner.get_mut(k.as_str()) {
                        set_field_defaults(f, v);
                    }
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

/// Test-only **full-fidelity** diff: lists every way `derived` departs from the
/// golden hand-rolled `reference` — field set, recursive types, `required`,
/// descriptions, defaults, examples, constraints, `read_only`, immutable paths,
/// and OSCAL subids at every nesting level.
/// Empty result ⇒ the derived schema reproduces the reference exactly. Any entry
/// must be consciously resolved (annotate the struct to preserve it, or document
/// it as a deliberate correction of a hand-rolled bug).
#[cfg(test)]
pub(crate) fn schema_diffs(reference: &PluginSchema, derived: &PluginSchema) -> Vec<String> {
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

#[cfg(test)]
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
    if val_str(&r.default) != val_str(&v.default) {
        d.push(format!(
            "{prefix}.default: {} -> {}",
            val_str(&r.default),
            val_str(&v.default)
        ));
    }
    if val_str(&r.example) != val_str(&v.example) {
        d.push(format!(
            "{prefix}.example: {} -> {}",
            val_str(&r.example),
            val_str(&v.example)
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

    // Recurse into nested container types so nested field-level diffs are reported.
    field_type_diffs(d, prefix, &r.field_type, &v.field_type);
}

#[cfg(test)]
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

#[cfg(test)]
fn val_str(v: &Option<OwnedValue>) -> String {
    match v {
        Some(x) => format!("{x:?}"),
        None => "∅".into(),
    }
}

#[cfg(test)]
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

#[cfg(test)]
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
    use crate::state_plugins::common::oscal::validate_subid;

    #[test]
    fn walks_array_of_objects_with_constraints() {
        // Minimal hand-written JSON mirroring what schemars derives for
        // `UnixSocketState { sockets: Vec<SocketEndpoint> }`.
        let bytes = br##"{
            "type":"object",
            "properties":{"sockets":{"type":"array","description":"Declared endpoints",
                "items":{"$ref":"#/$defs/SocketEndpoint"}}},
            "required":["sockets"],
            "$defs":{"SocketEndpoint":{"type":"object",
                "properties":{
                    "path":{"type":"string","description":"socket path"},
                    "port":{"type":"integer","minimum":1,"maximum":65535}},
                "required":["path","port"]}}
        }"##
        .to_vec();
        let root: JVal = serde_json::from_slice(&bytes).unwrap();
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
    fn reports_nested_mismatch() {
        let hand = plugin_schema_from_json(
            "nested_test",
            "1.0.0",
            "desc",
            &serde_json::json!({
                "type": "object",
                "properties": {
                    "wrapper": {
                        "type": "object",
                        "properties": {
                            "inner": { "type": "string", "description": "inner field" }
                        },
                        "required": ["inner"]
                    }
                },
                "required": ["wrapper"]
            }),
        );
        let derived = plugin_schema_from_json(
            "nested_test",
            "1.0.0",
            "desc",
            &serde_json::json!({
                "type": "object",
                "properties": {
                    "wrapper": {
                        "type": "object",
                        "properties": {
                            "inner": { "type": "integer", "description": "inner field" }
                        },
                        "required": ["inner"]
                    }
                },
                "required": ["wrapper"]
            }),
        );
        let diffs = schema_diffs(&hand, &derived);
        assert!(
            !diffs.is_empty(),
            "expected a non-empty diff for nested mismatch"
        );
        assert!(
            diffs
                .iter()
                .any(|d| d.contains("wrapper.inner") && d.contains("type")),
            "expected nested inner.type diff, got: {:#?}",
            diffs
        );
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
    fn validate_subid_accepts_valid_and_rejects_invalid() {
        assert!(validate_subid("mut.service.state-sync.apply-patch@v1").is_ok());
        assert!(validate_subid("exp.software.cognitive-memory.render").is_ok());
        assert!(validate_subid("obs.service.plugin-projection.query").is_ok());
        assert!(validate_subid("bad-category.software.foo.bar").is_err());
        assert!(validate_subid("mut.bad-type.foo.bar").is_err());
        assert!(validate_subid("mut.software.foo").is_err());
    }
}

#[cfg(test)]
mod typed_map_adapter_tests {
    use super::*;
    use std::collections::BTreeMap;
    #[derive(schemars::JsonSchema)]
    #[allow(dead_code)]
    struct Leaf {
        ready: bool,
    }
    #[derive(schemars::JsonSchema)]
    #[allow(dead_code)]
    struct Job {
        children: BTreeMap<String, Leaf>,
    }
    #[derive(schemars::JsonSchema)]
    #[allow(dead_code)]
    struct Workspace {
        jobs: BTreeMap<String, Job>,
    }
    #[test]
    fn actual_schemars_maps_keep_stable_typed_children_and_match_blob_adapter() {
        let source = serde_json::to_value(schemars::schema_for!(Workspace)).unwrap();
        let schema = try_plugin_schema_from_json("workspace", "2.0.0", "", &source).unwrap();
        let blob_view =
            op_blob::adapter::try_plugin_schema_from_json("workspace", "2.0.0", "", &source)
                .unwrap();
        assert_eq!(
            serde_json::to_value(&schema.fields).unwrap(),
            serde_json::to_value(&blob_view.fields).unwrap()
        );
        let FieldType::Map(job) = &schema.fields["jobs"].field_type else {
            panic!("map lost")
        };
        let FieldType::Object(job) = job.as_ref() else {
            panic!("job type lost")
        };
        let FieldType::Map(leaf) = &job["children"].field_type else {
            panic!("child map lost")
        };
        let FieldType::Object(leaf) = leaf.as_ref() else {
            panic!("leaf type lost")
        };
        assert_eq!(leaf["ready"].field_type, FieldType::Boolean);
        let exported = serde_json::to_value(schema.to_json_schema()).unwrap();
        let rebuilt = try_plugin_schema_from_json("workspace", "2.0.0", "", &exported).unwrap();
        assert_eq!(schema.fields, rebuilt.fields);
        let mixed = serde_json::json!({"type":"object","properties":{"jobs":{"type":"object","properties":{"fixed":{"type":"string"}},"additionalProperties":{"type":"boolean"}}}});
        assert!(try_plugin_schema_from_json("fixture", "1", "", &mixed).is_err());
    }
}

#[cfg(test)]
mod explicit_exact_record_tests {
    use super::*;
    use std::collections::BTreeMap;
    #[derive(schemars::JsonSchema)]
    #[schemars(extend("x-opdbus-exact-json-schema" = true))]
    #[allow(dead_code)]
    struct Record {
        revision: u64,
        label: Option<String>,
    }
    #[derive(schemars::JsonSchema)]
    #[allow(dead_code)]
    struct Workspace {
        jobs: BTreeMap<String, Record>,
    }
    #[test]
    fn exact_reference_preserves_authored_definitions_beside_finite_tagged_view() {
        let source = serde_json::json!({"type":"object","properties":{
            "finite":{"$ref":"#/$defs/Device"},
            "exact":{"$ref":"#/$defs/Device","x-opdbus-exact-json-schema":true}
        },"$defs":{
            "Device":{"oneOf":[
                {"$ref":"#/$defs/Base","type":"object","properties":{"type":{"type":"string","const":"nic"}},"required":["type"]},
                {"type":"object","properties":{"type":{"type":"string","const":"none"}},"required":["type"]}
            ]},
            "Base":{"type":"object","properties":{"parent":{"type":"string"}},"required":["parent"]}
        }});
        let original = source.clone();
        let schema = try_plugin_schema_from_json("fixture", "2", "", &source).unwrap();
        let mirror =
            op_blob::adapter::try_plugin_schema_from_json("fixture", "2", "", &source).unwrap();
        assert_eq!(
            serde_json::to_value(&schema.fields).unwrap(),
            serde_json::to_value(&mirror.fields).unwrap()
        );
        assert_eq!(source, original);
        let FieldType::OneOf(variants) = &schema.fields["finite"].field_type else {
            panic!("finite tagged view lost")
        };
        let FieldType::Object(nic) = &variants[0] else {
            panic!("finite object lost")
        };
        assert_eq!(nic["type"].field_type, FieldType::Enum(vec!["nic".into()]));
        let FieldType::JsonSchema(document) = &schema.fields["exact"].field_type else {
            panic!("exact reference marker lost")
        };
        let retained = serde_json::to_value(document.as_ref()).unwrap();
        assert_eq!(retained["$ref"], "#/$defs/Device");
        assert_eq!(retained["$defs"], original["$defs"]);
        let exported = serde_json::to_value(schema.to_json_schema()).unwrap();
        let validator = jsonschema::validator_for(&exported).unwrap();
        assert!(validator.is_valid(&serde_json::json!({"exact":{"type":"nic","parent":"ovsbr0"}})));
        assert!(!validator.is_valid(&serde_json::json!({"exact":{"type":"nic"}})));

        // Exact-only constraints must not be rejected by normalizing unrelated
        // finite definitions. No finite field refers to this closed base.
        let mut exact_only = source.clone();
        exact_only["properties"]
            .as_object_mut()
            .unwrap()
            .remove("finite");
        exact_only["$defs"]["Base"]["additionalProperties"] = serde_json::json!(false);
        let exact_schema = try_plugin_schema_from_json("fixture", "2", "", &exact_only).unwrap();
        let FieldType::JsonSchema(retained) = &exact_schema.fields["exact"].field_type else {
            panic!("exact-only contract lost")
        };
        assert_eq!(
            serde_json::to_value(retained.as_ref()).unwrap()["$defs"],
            exact_only["$defs"]
        );
    }
    #[test]
    fn opted_in_map_record_preserves_real_schemars_format_and_nullable_children() {
        let source = serde_json::to_value(schemars::schema_for!(Workspace)).unwrap();
        let schema = try_plugin_schema_from_json("workspace", "2", "", &source).unwrap();
        let mirror =
            op_blob::adapter::try_plugin_schema_from_json("workspace", "2", "", &source).unwrap();
        assert_eq!(
            serde_json::to_value(&schema.fields).unwrap(),
            serde_json::to_value(&mirror.fields).unwrap()
        );
        let FieldType::Map(record) = &schema.fields["jobs"].field_type else {
            panic!("map keys lost")
        };
        let FieldType::JsonSchema(record) = record.as_ref() else {
            panic!("exact child constraints lost")
        };
        let retained = serde_json::to_value(record.as_ref()).unwrap();
        assert_eq!(retained["properties"]["revision"]["format"], "uint64");
        assert!(retained["properties"]["label"]["type"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("null")));
        let valid = simd_json::json!({"jobs":{"stable":{"revision":u64::MAX,"label":null}}});
        assert!(schema.validate(&valid).valid);
        schema.validate_exact_schema_fields(&valid).unwrap();
        assert!(schema
            .validate_exact_schema_fields(
                &simd_json::json!({"jobs":{"stable":{"revision":-1,"label":null}}})
            )
            .is_err());
        assert!(schema.public_state.is_empty());
    }
}
