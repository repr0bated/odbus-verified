//! Explicit exact-schema fields, authored once by a plugin and sealed unchanged.
//! No network retrieval, registry discovery, consumer hashing or schema fallback.
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};

use serde_json::Value as Json;
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;

use crate::{FieldSchema, FieldType, PluginSchema};

const MAX_DEPTH: usize = 128;
const MAX_NODES: usize = 262_144;
const MAX_BYTES: usize = 16 * 1024 * 1024;

/// Errors never contain rejected values, schema defaults, map keys or native
/// validator diagnostics. Retained plugin schema remains the trusted evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("exact state schema: {code}")]
pub struct ExactSchemaError {
    pub code: &'static str,
}
fn error(code: &'static str) -> ExactSchemaError {
    ExactSchemaError { code }
}

struct NoExternalResources;
impl jsonschema::Retrieve for NoExternalResources {
    fn retrieve(
        &self,
        _: &jsonschema::Uri<String>,
    ) -> Result<Json, Box<dyn std::error::Error + Send + Sync>> {
        Err(Box::new(error("external_resource_denied")))
    }
}

struct ByteBudget(usize);
impl Write for ByteBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| io::Error::other("size bound"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn bound_json(value: &Json) -> Result<(), ExactSchemaError> {
    let mut pending = vec![(value, 0usize)];
    let mut count = 0usize;
    while let Some((node, depth)) = pending.pop() {
        count += 1;
        if depth > MAX_DEPTH || count + pending.len() > MAX_NODES {
            return Err(error("document_shape_bound"));
        }
        let children: Box<dyn Iterator<Item = &Json> + '_> = match node {
            Json::Array(items) => Box::new(items.iter()),
            Json::Object(fields) => Box::new(fields.values()),
            _ => continue,
        };
        for child in children {
            if count + pending.len() >= MAX_NODES {
                return Err(error("document_shape_bound"));
            }
            pending.push((child, depth + 1));
        }
    }
    serde_json::to_writer(ByteBudget(MAX_BYTES), value).map_err(|_| error("document_byte_bound"))
}

fn bound_simd(value: &Value) -> Result<(), ExactSchemaError> {
    let mut pending = vec![(value, 0usize)];
    let mut count = 0usize;
    while let Some((node, depth)) = pending.pop() {
        count += 1;
        if depth > MAX_DEPTH || count + pending.len() > MAX_NODES {
            return Err(error("document_shape_bound"));
        }
        if let Some(items) = node.as_array() {
            for item in items {
                if count + pending.len() >= MAX_NODES {
                    return Err(error("document_shape_bound"));
                }
                pending.push((item, depth + 1));
            }
        } else if let Some(fields) = node.as_object() {
            for item in fields.values() {
                if count + pending.len() >= MAX_NODES {
                    return Err(error("document_shape_bound"));
                }
                pending.push((item, depth + 1));
            }
        }
    }
    serde_json::to_writer(ByteBudget(MAX_BYTES), value).map_err(|_| error("document_byte_bound"))
}

fn schema_children<'a>(root: &'a Json, node: &'a Json) -> Result<Vec<&'a Json>, ExactSchemaError> {
    if node.is_boolean() {
        return Ok(vec![]);
    }
    let object = node
        .as_object()
        .ok_or_else(|| error("schema_node_invalid"))?;
    if node.get("$dynamicRef").is_some()
        || node.get("$recursiveRef").is_some()
        || node.get("$anchor").is_some()
        || node.get("$dynamicAnchor").is_some()
        || node.get("$recursiveAnchor").is_some()
    {
        return Err(error("dynamic_reference_unsupported"));
    }
    if !std::ptr::eq(root, node) && node.get("$id").is_some() {
        return Err(error("nested_resource_unsupported"));
    }
    let mut children = Vec::new();
    if let Some(reference) = object.get("$ref") {
        let reference = reference
            .as_str()
            .ok_or_else(|| error("local_reference_invalid"))?;
        let pointer = reference
            .strip_prefix('#')
            .filter(|pointer| pointer.is_empty() || pointer.starts_with('/'))
            .ok_or_else(|| error("external_or_anchor_reference_denied"))?;
        for component in pointer.split('/').skip(1) {
            crate::state_schema_shape::decode_component(component)
                .map_err(|_| error("local_reference_escape_invalid"))?;
        }
        // URI percent decoding is deliberately not approximated. Canonical
        // local references use exact RFC6901 components, including ~0 and ~1.
        if pointer.contains('%') {
            return Err(error("percent_reference_unsupported"));
        }
        children.push(
            root.pointer(pointer)
                .ok_or_else(|| error("local_reference_unresolved"))?,
        );
    }
    for key in [
        "$defs",
        "definitions",
        "properties",
        "patternProperties",
        "dependentSchemas",
    ] {
        if let Some(values) = object.get(key) {
            let values = values
                .as_object()
                .ok_or_else(|| error("schema_map_invalid"))?;
            children.extend(values.values());
        }
    }
    for key in [
        "items",
        "additionalProperties",
        "propertyNames",
        "contains",
        "if",
        "then",
        "else",
        "not",
        "unevaluatedProperties",
        "unevaluatedItems",
        "contentSchema",
    ] {
        if let Some(value) = object.get(key) {
            children.push(value)
        }
    }
    for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
        if let Some(values) = object.get(key) {
            children.extend(
                values
                    .as_array()
                    .ok_or_else(|| error("schema_sequence_invalid"))?,
            );
        }
    }
    Ok(children)
}

/// Validate bounds, local reference closure and acyclic expansion before the
/// upstream validator compiles. Draft2020-12 is the supported exact dialect;
/// recursive schemas, nested resources and dynamic anchors reject explicitly.
/// These restrictions do not alter or simplify accepted contracts.
pub fn validate_exact_schema_document(schema: &Json) -> Result<(), ExactSchemaError> {
    compile_exact_schema(schema).map(|_| ())
}

fn compile_exact_schema(schema: &Json) -> Result<jsonschema::Validator, ExactSchemaError> {
    bound_json(schema)?;
    if let Some(dialect) = schema.get("$schema")
        && dialect.as_str() != Some(crate::DEFAULT_SCHEMA_DIALECT)
    {
        return Err(error("dialect_unsupported"));
    }
    enum Step<'a> {
        Enter(&'a Json, usize),
        Exit(&'a Json, Vec<&'a Json>),
    }
    let mut stack = vec![Step::Enter(schema, 0)];
    let mut active = HashSet::new();
    let mut heights = HashMap::<usize, usize>::new();
    while let Some(step) = stack.pop() {
        match step {
            Step::Enter(node, depth) => {
                let id = node as *const Json as usize;
                if depth > MAX_DEPTH {
                    return Err(error("reference_expansion_bound"));
                }
                if let Some(height) = heights.get(&id) {
                    if depth + height > MAX_DEPTH {
                        return Err(error("reference_expansion_bound"));
                    }
                    continue;
                }
                if !active.insert(id) {
                    return Err(error("recursive_reference_unsupported"));
                }
                if active.len() + heights.len() > MAX_NODES {
                    return Err(error("reference_expansion_bound"));
                }
                let children = schema_children(schema, node)?;
                stack.push(Step::Exit(node, children.clone()));
                for child in children.into_iter().rev() {
                    stack.push(Step::Enter(child, depth + 1));
                }
            }
            Step::Exit(node, children) => {
                let id = node as *const Json as usize;
                let height = children
                    .iter()
                    .map(|child| heights[&(*child as *const Json as usize)] + 1)
                    .max()
                    .unwrap_or(0);
                if height > MAX_DEPTH {
                    return Err(error("reference_expansion_bound"));
                }
                active.remove(&id);
                heights.insert(id, height);
            }
        }
    }
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .with_retriever(NoExternalResources)
        .build(schema)
        .map_err(|_| error("schema_invalid"))
}

pub(crate) fn validate_value(schema: &Value, value: &Value) -> Result<(), ExactSchemaError> {
    // Deserialized sealed fields receive the same checks as explicit producers.
    bound_simd(schema)?;
    bound_simd(value)?;
    let schema = serde_json::to_value(schema).map_err(|_| error("schema_conversion"))?;
    let validator = compile_exact_schema(&schema)?;
    let value = serde_json::to_value(value).map_err(|_| error("state_conversion"))?;
    if validator.is_valid(&value) {
        Ok(())
    } else {
        Err(error("state_rejected"))
    }
}

impl FieldType {
    /// Plugin-only explicit opt-in. The original complete document is retained
    /// in the canonical field, including definitions and constraints.
    pub fn exact_json_schema(schema: Json) -> Result<Self, ExactSchemaError> {
        validate_exact_schema_document(&schema)?;
        Ok(Self::JsonSchema(Box::new(
            simd_json::serde::to_owned_value(schema).map_err(|_| error("schema_conversion"))?,
        )))
    }
}

impl PluginSchema {
    /// Validate whole present exact-schema boundaries after folding children.
    /// This is independent of public exposure and must precede SHM publication.
    /// It establishes no method grant, durable transaction or catalog authority.
    pub fn validate_exact_schema_fields(&self, state: &Value) -> Result<(), ExactSchemaError> {
        let mut has_exact = false;
        for field in self.fields.values() {
            has_exact |= contains_exact(&field.field_type)?;
        }
        if !has_exact {
            return Ok(());
        }
        bound_simd(state)?;
        let mut queue = Vec::new();
        for (name, field) in &self.fields {
            if contains_exact(&field.field_type)? {
                let object = state
                    .as_object()
                    .ok_or_else(|| error("state_root_invalid"))?;
                if let Some(value) = object.get(name) {
                    if value.is_null() && !field.required {
                        continue;
                    }
                    queue.push((value, &field.field_type, 0usize));
                } else if field.required {
                    return Err(error("required_exact_field_absent"));
                }
            }
        }
        while let Some((value, kind, depth)) = queue.pop() {
            if depth > MAX_DEPTH {
                return Err(error("state_shape_bound"));
            }
            match kind {
                FieldType::JsonSchema(schema) => validate_value(schema, value)?,
                FieldType::Object(fields) => {
                    let values = value
                        .as_object()
                        .ok_or_else(|| error("exact_field_parent_invalid"))?;
                    for (name, field) in fields {
                        if !contains_exact(&field.field_type)? {
                            continue;
                        }
                        if let Some(value) = values.get(name) {
                            if value.is_null() && !field.required {
                                continue;
                            }
                            queue.push((value, &field.field_type, depth + 1));
                        } else if field.required {
                            return Err(error("required_exact_field_absent"));
                        }
                    }
                }
                FieldType::Map(item) => {
                    let values = value
                        .as_object()
                        .ok_or_else(|| error("exact_field_parent_invalid"))?;
                    queue.extend(
                        values
                            .values()
                            .map(|value| (value, item.as_ref(), depth + 1)),
                    );
                }
                FieldType::Array(item) => {
                    let values = value
                        .as_array()
                        .ok_or_else(|| error("exact_field_parent_invalid"))?;
                    queue.extend(values.iter().map(|value| (value, item.as_ref(), depth + 1)));
                }
                // Finite union branch selection requires the entire branch to
                // validate, not merely accepting whichever exact leaf is easiest.
                FieldType::OneOf(_) => {
                    super::plugin_schema::validate_value_against_type("exact union", value, kind)
                        .map_err(|_| error("state_rejected"))?
                }
                _ => {}
            }
            if queue.len() > MAX_NODES {
                return Err(error("state_shape_bound"));
            }
        }
        Ok(())
    }
}

pub(crate) fn contains_exact(kind: &FieldType) -> Result<bool, ExactSchemaError> {
    let mut queue = vec![(kind, 0usize)];
    let mut count = 0;
    while let Some((kind, depth)) = queue.pop() {
        count += 1;
        if depth > MAX_DEPTH || count > MAX_NODES {
            return Err(error("field_type_bound"));
        }
        match kind {
            FieldType::JsonSchema(_) => return Ok(true),
            FieldType::Object(fields) => {
                queue.extend(fields.values().map(|f| (&f.field_type, depth + 1)))
            }
            FieldType::Map(item) | FieldType::Array(item) => queue.push((item.as_ref(), depth + 1)),
            FieldType::OneOf(items) => queue.extend(items.iter().map(|item| (item, depth + 1))),
            _ => {}
        }
    }
    Ok(false)
}

/// Rebase local references only in a derived embedding. Original sealed field
/// bytes stay intact. Removing the root resource ID makes the prefixed pointers
/// refer to the enclosing exported document; nested IDs were rejected at admission.
pub fn embed_exact_schema(schema: &Json, pointer: &str) -> Json {
    validate_exact_schema_document(schema).expect("invalid exact schema cannot be exported");
    let mut output = schema.clone();
    if pointer.is_empty() {
        return output;
    }
    if let Some(object) = output.as_object_mut() {
        object.remove("$id");
    }
    fn rebase(node: &mut Json, prefix: &str) {
        let Some(object) = node.as_object_mut() else {
            return;
        };
        if let Some(Json::String(reference)) = object.get_mut("$ref")
            && let Some(suffix) = reference.strip_prefix('#')
        {
            *reference = format!("#{prefix}{suffix}");
        }
        for key in [
            "$defs",
            "definitions",
            "properties",
            "patternProperties",
            "dependentSchemas",
        ] {
            if let Some(values) = object.get_mut(key).and_then(Json::as_object_mut) {
                for value in values.values_mut() {
                    rebase(value, prefix)
                }
            }
        }
        for key in [
            "items",
            "additionalProperties",
            "propertyNames",
            "contains",
            "if",
            "then",
            "else",
            "not",
            "unevaluatedProperties",
            "unevaluatedItems",
            "contentSchema",
        ] {
            if let Some(value) = object.get_mut(key) {
                rebase(value, prefix)
            }
        }
        for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
            if let Some(values) = object.get_mut(key).and_then(Json::as_array_mut) {
                for value in values {
                    rebase(value, prefix)
                }
            }
        }
    }
    rebase(&mut output, pointer);
    output
}

pub(crate) fn scope_field_exports(
    output: &mut Value,
    fields: &HashMap<String, FieldSchema>,
    prefix: &str,
) {
    if !fields
        .values()
        .any(|field| contains_exact(&field.field_type).expect("bounded field type"))
    {
        return;
    }
    bound_simd(output).expect("bounded schema export");
    let mut converted = serde_json::to_value(&*output).expect("JSON schema serializes");
    for (name, field) in fields {
        scope_type_export_json(
            &mut converted,
            &field.field_type,
            &format!("{prefix}/properties/{}", escape(name)),
        );
    }
    *output = simd_json::serde::to_owned_value(converted).expect("JSON schema converts");
}
pub(crate) fn scope_type_export(output: &mut Value, kind: &FieldType, pointer: &str) {
    if !contains_exact(kind).expect("bounded field type") {
        return;
    }
    bound_simd(output).expect("bounded schema export");
    let mut converted = serde_json::to_value(&*output).expect("JSON schema serializes");
    scope_type_export_json(&mut converted, kind, pointer);
    *output = simd_json::serde::to_owned_value(converted).expect("JSON schema converts");
}
fn scope_type_export_json(output: &mut Json, kind: &FieldType, pointer: &str) {
    match kind {
        FieldType::JsonSchema(schema) => {
            bound_simd(schema).expect("bounded exact field schema");
            let schema =
                serde_json::to_value(schema.as_ref()).expect("JSON field schema serializes");
            if let Some(value) = output.pointer_mut(pointer) {
                *value = embed_exact_schema(&schema, pointer);
            }
        }
        FieldType::Object(fields) => {
            for (name, field) in fields {
                scope_type_export_json(
                    output,
                    &field.field_type,
                    &format!("{pointer}/properties/{}", escape(name)),
                )
            }
        }
        FieldType::Map(item) => {
            scope_type_export_json(output, item, &format!("{pointer}/additionalProperties"))
        }
        FieldType::Array(item) => scope_type_export_json(output, item, &format!("{pointer}/items")),
        FieldType::OneOf(items) => {
            for (index, item) in items.iter().enumerate() {
                scope_type_export_json(output, item, &format!("{pointer}/oneOf/{index}"))
            }
        }
        _ => {}
    }
}
fn escape(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn document(minimum: i64) -> Json {
        json!({"$schema":crate::DEFAULT_SCHEMA_DIALECT,"$defs":{"Entry":{"type":"object","properties":{
            "revision":{"type":"integer","minimum":minimum},
            "label":{"type":["string","null"]},
            "children":{"type":"object","additionalProperties":{"type":"integer","minimum":0}}
        },"required":["revision","label","children"],"additionalProperties":false}},
        "type":"object","properties":{"jobs":{"type":"object","additionalProperties":{"$ref":"#/$defs/Entry"}}},"required":["jobs"],"additionalProperties":false})
    }
    fn field(schema: Json) -> FieldSchema {
        FieldSchema {
            field_type: FieldType::exact_json_schema(schema).unwrap(),
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
    fn exact_field_preserves_nullable_maps_constraints_and_schema_equality() {
        let original = document(2);
        let schema = PluginSchema::builder("exact")
            .field("native", field(original.clone()))
            .build();
        let encoded = serde_json::to_value(&schema).unwrap();
        assert_eq!(
            encoded["fields"]["native"]["field_type"]["json_schema"],
            original
        );
        let decoded: PluginSchema = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.fields, schema.fields);
        let valid = simd_json::json!({"native":{"jobs":{"stable":{"revision":2,"label":null,"children":{"grandchild":0}}}}});
        assert!(decoded.validate(&valid).valid);
        decoded.validate_exact_schema_fields(&valid).unwrap();
        // Same full document after a child write; no public policy is involved.
        for invalid in [
            simd_json::json!({"native":{"jobs":{"stable":{"revision":1,"label":null,"children":{"grandchild":0}}}}}),
            simd_json::json!({"native":{"jobs":{"stable":{"revision":2,"label":null,"children":{"grandchild":-1}}}}}),
            simd_json::json!({"native":{"jobs":{"stable":{"revision":2,"children":{}}}}}),
        ] {
            assert!(decoded.validate_exact_schema_fields(&invalid).is_err());
        }
        assert_eq!(decoded.project_public_state(&valid).unwrap(), None);
        let mut public = decoded.clone();
        public.public_state.paths = vec!["/native/jobs/stable/revision".into()];
        assert!(public.validate_public_state_policy().is_err());
        public.public_state.paths = vec!["/native".into()];
        assert!(public.validate_public_state_policy().is_err());
        assert_eq!(
            serde_json::to_value(decoded.field_input_schema("native").unwrap()).unwrap(),
            original
        );
        let changed = PluginSchema::builder("exact")
            .field("native", field(document(3)))
            .build();
        assert_ne!(changed.fields, decoded.fields); // existing compiled/sealed comparison catches this
    }
    #[test]
    fn exported_exact_fields_rebase_local_definitions_without_colliding() {
        let schema = PluginSchema::builder("exact")
            .field("first", field(document(1)))
            .field("second/a~b", field(document(5)))
            .build();
        let exported = serde_json::to_value(schema.to_json_schema()).unwrap();
        assert_eq!(
            exported["properties"]["first"]["properties"]["jobs"]["additionalProperties"]["$ref"],
            "#/properties/first/$defs/Entry"
        );
        assert_eq!(
            exported["properties"]["second/a~b"]["properties"]["jobs"]["additionalProperties"]
                ["$ref"],
            "#/properties/second~1a~0b/$defs/Entry"
        );
        let validator = jsonschema::options()
            .with_retriever(NoExternalResources)
            .build(&exported)
            .unwrap();
        assert!(validator.is_valid(&json!({"first":{"jobs":{"key":{"revision":1,"label":null,"children":{}}}},"second/a~b":{"jobs":{"key":{"revision":5,"label":null,"children":{}}}}})));
        assert!(!validator.is_valid(&json!({"first":{"jobs":{"key":{"revision":1,"label":null,"children":{}}}},"second/a~b":{"jobs":{"key":{"revision":4,"label":null,"children":{}}}}})));
        assert_eq!(
            serde_json::to_value(&schema.fields["first"].field_type).unwrap()["json_schema"],
            document(1)
        );
    }
    #[test]
    fn exact_checks_reject_remote_recursive_invalid_and_private_value_errors() {
        for schema in [
            json!({"$ref":"https://example.invalid/private"}),
            json!({"$ref":"#/$defs/Cycle","$defs":{"Cycle":{"$ref":"#/$defs/Cycle"}}}),
            json!({"$defs":{"Inner":{"$id":"urn:nested","type":"string"}},"$ref":"#/$defs/Inner"}),
            json!({"$dynamicRef":"#node"}),
            json!({"$ref":"#/$defs/Missing"}),
        ] {
            assert!(FieldType::exact_json_schema(schema).is_err());
        }
        let schema = PluginSchema::builder("exact")
            .field("native", field(document(0)))
            .build();
        let value = simd_json::json!({"native":{"jobs":{"PRIVATE_KEY":{"revision":"PRIVATE_SENTINEL","label":null,"children":{}}}}});
        let err = schema.validate_exact_schema_fields(&value).unwrap_err();
        assert!(!format!("{err}\n{err:?}").contains("PRIVATE"));
        let mut deep = json!(true);
        for _ in 0..(MAX_DEPTH + 1) {
            deep = json!({"not":deep});
        }
        assert!(FieldType::exact_json_schema(deep).is_err());
    }
    #[test]
    #[should_panic(expected = "exact 2020-12 fields cannot use legacy draft07 export")]
    #[allow(deprecated)]
    fn legacy_dialect_cannot_silently_weaken_exact_contract() {
        let schema = PluginSchema::builder("exact")
            .field("native", field(document(1)))
            .build();
        schema.to_json_schema_draft07();
    }
}

/// Explicit schemars record opt-in. Keep the exact record body and only its
/// reachable local definitions; do not copy unrelated plugin fields/defaults.
/// This is producer extraction from the one authored DTO, never a reader's
/// substitute catalog. No constraints or nullable branches are normalized.
pub fn self_contained_field_schema(
    node: &Json,
    definitions: Option<&serde_json::Map<String, Json>>,
) -> Result<Json, ExactSchemaError> {
    bound_json(node)?;
    if node.get("$defs").is_some() || node.get("definitions").is_some() {
        validate_exact_schema_document(node)?;
        return Ok(node.clone());
    }
    let mut output = node.clone();
    let mut collected = serde_json::Map::new();
    let mut seen = HashSet::new();
    let mut pending = vec![node];
    let mut count = 0usize;
    let mut namespace: Option<&str> = None;
    while let Some(value) = pending.pop() {
        count += 1;
        if count + pending.len() > MAX_NODES {
            return Err(error("field_definition_bound"));
        }
        let Some(object) = value.as_object() else {
            continue;
        };
        if let Some(reference) = object.get("$ref") {
            let reference = reference
                .as_str()
                .ok_or_else(|| error("local_reference_invalid"))?;
            let (space, pointer) = if let Some(tail) = reference.strip_prefix("#/$defs/") {
                ("$defs", tail)
            } else if let Some(tail) = reference.strip_prefix("#/definitions/") {
                ("definitions", tail)
            } else {
                return Err(error("field_reference_not_definition"));
            };
            if namespace.is_some_and(|old| old != space) {
                return Err(error("mixed_definition_namespaces"));
            }
            namespace = Some(space);
            let component = pointer
                .split('/')
                .next()
                .ok_or_else(|| error("local_reference_invalid"))?;
            let key = crate::state_schema_shape::decode_component(component)
                .map_err(|_| error("local_reference_escape_invalid"))?;
            if seen.insert(key.clone()) {
                let target = definitions
                    .and_then(|defs| defs.get(&key))
                    .ok_or_else(|| error("local_reference_unresolved"))?;
                bound_json(target)?;
                collected.insert(key, target.clone());
                pending.push(target);
            }
        }
        for key in ["properties", "patternProperties", "dependentSchemas"] {
            if let Some(values) = object.get(key).and_then(Json::as_object) {
                pending.extend(values.values());
            }
        }
        for key in [
            "items",
            "additionalProperties",
            "propertyNames",
            "contains",
            "if",
            "then",
            "else",
            "not",
            "unevaluatedProperties",
            "unevaluatedItems",
            "contentSchema",
        ] {
            if let Some(value) = object.get(key) {
                pending.push(value)
            }
        }
        for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
            if let Some(values) = object.get(key).and_then(Json::as_array) {
                pending.extend(values)
            }
        }
    }
    if !collected.is_empty() {
        output
            .as_object_mut()
            .ok_or_else(|| error("schema_node_invalid"))?
            .insert(namespace.unwrap_or("$defs").into(), Json::Object(collected));
    }
    validate_exact_schema_document(&output)?;
    Ok(output)
}

#[cfg(test)]
mod dialect_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn exported_2020_keyword_remains_enforced_after_reference_relocation() {
        let original = json!({"$schema":crate::DEFAULT_SCHEMA_DIALECT,"$defs":{"Base":{"type":"object","properties":{"known":{"type":"integer"}}}},"allOf":[{"$ref":"#/$defs/Base"}],"unevaluatedProperties":false});
        let schema = PluginSchema::builder("dialect")
            .field(
                "native",
                FieldSchema {
                    field_type: FieldType::exact_json_schema(original).unwrap(),
                    required: true,
                    description: String::new(),
                    default: None,
                    example: None,
                    constraints: vec![],
                    read_only: false,
                    read_only_when: None,
                },
            )
            .build();
        let exported = serde_json::to_value(schema.to_json_schema()).unwrap();
        assert_eq!(exported["$schema"], crate::DEFAULT_SCHEMA_DIALECT);
        let validator = jsonschema::options()
            .with_retriever(NoExternalResources)
            .build(&exported)
            .unwrap();
        assert!(validator.is_valid(&json!({"native":{"known":1}})));
        assert!(!validator.is_valid(&json!({"native":{"known":1,"unknown":true}})));
    }
}
