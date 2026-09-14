//! Bounded producer preflight for the finite FieldType representation.
//!
//! This is not a second schema. It rejects shapes the state adapters cannot
//! represent instead of silently converting typed dynamic keys into Object({}).
//! Method arguments/results keep their original JSON Schema and do not use it.
use serde_json::Value;

const MAX_DEPTH: usize = 64;
const MAX_EXPANDED_NODES: usize = 65_536;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unsupported plugin state schema: {reason}")]
pub struct StateSchemaShapeError {
    pub reason: &'static str,
}

fn error(reason: &'static str) -> StateSchemaShapeError {
    StateSchemaShapeError { reason }
}

/// Normalize the one finite reference intersection emitted for internally
/// tagged struct variants: an open object reference plus one disjoint required
/// string-constant tag. This is a derived producer view; the authored JSON is
/// untouched. Arbitrary intersections and closed objects remain unsupported.
pub fn prepare_state_schema(root: &Value) -> Result<Value, StateSchemaShapeError> {
    crate::exact_schema::bound_json(root)
        .map_err(|_| error("state schema raw document exceeds bounds"))?;
    let mut prepared = root.clone();
    let mut count = 0;
    normalize_tagged_refs(root, &mut prepared, 0, &mut count)?;
    validate_state_schema(&prepared)?;
    Ok(prepared)
}

fn normalize_tagged_refs(
    root: &Value,
    node: &mut Value,
    depth: usize,
    count: &mut usize,
) -> Result<(), StateSchemaShapeError> {
    *count += 1;
    if depth > MAX_DEPTH || *count > MAX_EXPANDED_NODES {
        return Err(error("normalized state schema exceeds bounds"));
    }
    if node
        .get("x-opdbus-exact-json-schema")
        .and_then(Value::as_bool)
        == Some(true)
    {
        // Close exact fields over the ORIGINAL definitions before preparing
        // finite views. Shared definitions may be normalized elsewhere; their
        // derived representation is never substituted into an exact field.
        *node = crate::exact_schema::self_contained_field_schema(
            node,
            root.get("$defs")
                .or_else(|| root.get("definitions"))
                .and_then(Value::as_object),
        )
        .map_err(|_| error("explicit exact field schema is invalid or unsupported"))?;
        return Ok(());
    }
    if let Some(reference) = node.get("$ref").and_then(Value::as_str) {
        let has_intersection = node.as_object().is_some_and(|object| {
            object
                .keys()
                .any(|key| key != "$ref" && !annotation(key) && !key.starts_with("x-"))
        });
        if has_intersection {
            match merge_tagged_object(root, node, reference) {
                Ok(merged) => *node = merged,
                // Only the reachable finite graph is admitted below. An
                // unsupported intersection in a definition used exclusively
                // by an exact field retains its original semantics there.
                Err(_) => return Ok(()),
            }
        }
    }
    let Some(object) = node.as_object_mut() else {
        return Ok(());
    };
    for key in [
        "properties",
        "$defs",
        "definitions",
        "patternProperties",
        "dependentSchemas",
    ] {
        if let Some(fields) = object.get_mut(key).and_then(Value::as_object_mut) {
            for field in fields.values_mut() {
                normalize_tagged_refs(root, field, depth + 1, count)?;
            }
        }
    }
    for key in ["oneOf", "anyOf", "allOf", "prefixItems"] {
        if let Some(items) = object.get_mut(key).and_then(Value::as_array_mut) {
            for item in items {
                normalize_tagged_refs(root, item, depth + 1, count)?;
            }
        }
    }
    for key in [
        "items",
        "additionalProperties",
        "unevaluatedProperties",
        "contains",
        "not",
        "if",
        "then",
        "else",
        "propertyNames",
    ] {
        if let Some(child) = object.get_mut(key) {
            normalize_tagged_refs(root, child, depth + 1, count)?;
        }
    }
    Ok(())
}

fn merge_tagged_object(
    root: &Value,
    extension: &Value,
    reference: &str,
) -> Result<Value, StateSchemaShapeError> {
    let invalid = || error("reference intersection is not a disjoint open-object string tag");
    let extension_object = extension.as_object().ok_or_else(invalid)?;
    if extension_object.keys().any(|key| {
        key != "$ref"
            && !annotation(key)
            && !key.starts_with("x-")
            && !matches!(key.as_str(), "type" | "properties" | "required")
    }) || extension.get("type").and_then(Value::as_str) != Some("object")
    {
        return Err(invalid());
    }
    let tags = extension
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(invalid)?;
    if tags.len() != 1 {
        return Err(invalid());
    }
    let (tag, value) = tags.iter().next().ok_or_else(invalid)?;
    if extension.get("required") != Some(&Value::Array(vec![Value::String(tag.clone())]))
        || value.get("type").and_then(Value::as_str) != Some("string")
        || value.get("const").and_then(Value::as_str).is_none()
        || value.as_object().is_none_or(|object| {
            object.keys().any(|key| {
                !matches!(key.as_str(), "type" | "const")
                    && !annotation(key)
                    && !key.starts_with("x-")
            })
        })
    {
        return Err(invalid());
    }
    let tail = reference
        .strip_prefix("#/$defs/")
        .or_else(|| reference.strip_prefix("#/definitions/"))
        .ok_or_else(invalid)?;
    if tail.is_empty() || tail.contains('/') || tail.contains('%') {
        return Err(invalid());
    }
    let key = decode_component(tail)?;
    let target = root
        .get("$defs")
        .or_else(|| root.get("definitions"))
        .and_then(Value::as_object)
        .and_then(|defs| defs.get(&key))
        .ok_or_else(invalid)?;
    let target_object = target.as_object().ok_or_else(invalid)?;
    // Closed targets would reject the new tag in the original intersection.
    // Other constraints/recursive references need an explicit exact field.
    if target.get("type").and_then(Value::as_str) != Some("object")
        || target_object.keys().any(|key| {
            !matches!(key.as_str(), "type" | "properties" | "required")
                && !annotation(key)
                && !key.starts_with("x-")
        })
    {
        return Err(invalid());
    }
    let properties = target
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(invalid)?;
    if properties.contains_key(tag) {
        return Err(invalid());
    }
    let mut required = std::collections::BTreeSet::new();
    if let Some(names) = target.get("required") {
        for name in names.as_array().ok_or_else(invalid)? {
            required.insert(name.as_str().ok_or_else(invalid)?.to_owned());
        }
    }
    required.insert(tag.clone());
    let mut merged = target_object.clone();
    let mut fields = properties.clone();
    fields.insert(tag.clone(), value.clone());
    merged.insert("properties".into(), Value::Object(fields));
    merged.insert(
        "required".into(),
        Value::Array(required.into_iter().map(Value::String).collect()),
    );
    for (key, value) in extension_object {
        if annotation(key) || key.starts_with("x-") {
            merged.insert(key.clone(), value.clone());
        }
    }
    Ok(Value::Object(merged))
}

/// Check only the schema-bearing graph, never defaults/examples or live state.
/// Recursive schemas (including recursive definitions) are outside FieldType's
/// finite tree contract. Bounded expansion catches them before recursive adapters.
/// A map's keys cannot carry pattern/property-count constraints in Map(Box<_>),
/// and its immediate value has no FieldSchema metadata slot; reject such losses.
pub fn validate_state_schema(root: &Value) -> Result<(), StateSchemaShapeError> {
    // node, expansion depth, inside map, has FieldSchema metadata slot
    let mut pending = vec![(root, 0usize, false, true)];
    let mut visited = 0usize;
    while let Some((node, depth, in_map, field_slot)) = pending.pop() {
        visited += 1;
        if depth > MAX_DEPTH || visited > MAX_EXPANDED_NODES {
            return Err(error("recursive or excessively expanded state type"));
        }
        if node
            .get("x-opdbus-exact-json-schema")
            .and_then(Value::as_bool)
            == Some(true)
        {
            crate::exact_schema::self_contained_field_schema(
                node,
                root.get("$defs")
                    .or_else(|| root.get("definitions"))
                    .and_then(Value::as_object),
            )
            .map_err(|_| error("explicit exact field schema is invalid or unsupported"))?;
            continue;
        }
        if node == &Value::Bool(true) {
            continue;
        }
        if node == &Value::Bool(false) {
            return Err(error("false schema has no FieldType representation"));
        }
        let object = node
            .as_object()
            .ok_or_else(|| error("schema node is not an object or boolean"))?;
        if let Some(reference) = node.get("$ref") {
            let reference = reference
                .as_str()
                .ok_or_else(|| error("invalid local reference"))?;
            let tail = reference
                .strip_prefix("#/$defs/")
                .or_else(|| reference.strip_prefix("#/definitions/"))
                .ok_or_else(|| error("only local definition references are supported"))?;
            if tail.contains('/') || tail.contains('%') || tail.is_empty() {
                return Err(error("unsupported definition pointer"));
            }
            let key = decode_component(tail)?;
            let target = root
                .get("$defs")
                .or_else(|| root.get("definitions"))
                .and_then(Value::as_object)
                .and_then(|defs| defs.get(&key))
                .ok_or_else(|| error("unresolved local reference"))?;
            if object
                .keys()
                .any(|key| key != "$ref" && !annotation(key) && !key.starts_with("x-"))
            {
                return Err(error("reference siblings change the referenced type"));
            }
            pending.push((target, depth + 1, in_map, field_slot));
            continue;
        }
        // Numeric-key maps emit this even with additionalProperties:false.
        if object.contains_key("patternProperties") {
            return Err(error(
                "pattern-keyed objects require an explicit exact field schema",
            ));
        }
        if let Some(constant) = node.get("const") {
            if !constant.is_string() || node.get("type").and_then(Value::as_str) != Some("string") {
                return Err(error(
                    "non-string const requires an explicit exact field schema",
                ));
            }
            if node.get("enum").is_some_and(|values| {
                values
                    .as_array()
                    .is_none_or(|values| !values.contains(constant))
            }) {
                return Err(error(
                    "conflicting const and enum require an explicit exact field schema",
                ));
            }
        }
        let properties = node.get("properties").and_then(Value::as_object);
        let additional = node.get("additionalProperties");
        let dynamic = additional.is_some_and(|value| value != &Value::Bool(false));
        if dynamic && properties.is_some_and(|fields| !fields.is_empty()) {
            return Err(error(
                "mixed named properties and additionalProperties are not representable",
            ));
        }
        let is_map = dynamic && properties.is_none_or(|fields| fields.is_empty());
        if is_map
            && object.keys().any(|key| {
                matches!(
                    key.as_str(),
                    "propertyNames"
                        | "patternProperties"
                        | "minProperties"
                        | "maxProperties"
                        | "dependentRequired"
                        | "dependentSchemas"
                        | "unevaluatedProperties"
                )
            })
        {
            return Err(error(
                "map key or cardinality constraints are not representable",
            ));
        }
        if in_map {
            if node.get("type").is_some_and(|kind| kind == "null") && !field_slot {
                return Err(error(
                    "null map values or array items are not representable",
                ));
            }
            if node
                .get("enum")
                .and_then(Value::as_array)
                .is_some_and(|values| values.iter().any(|value| !value.is_string()))
            {
                return Err(error("non-string map enums are not representable"));
            }
            for key in object.keys() {
                if annotation(key) || key.starts_with("x-") {
                    continue;
                }
                if matches!(
                    key.as_str(),
                    "type"
                        | "properties"
                        | "required"
                        | "additionalProperties"
                        | "items"
                        | "enum"
                        | "const"
                        | "anyOf"
                        | "oneOf"
                ) {
                    continue;
                }
                if field_slot && matches!(key.as_str(), "minimum" | "maximum" | "pattern") {
                    continue;
                }
                return Err(error(
                    "constraint inside map has no faithful FieldType representation",
                ));
            }
            // Optional named fields can omit null. A map entry or array item has
            // no required/nullable bit; silently removing its null branch is wrong.
            if !field_slot
                && (node
                    .get("type")
                    .and_then(Value::as_array)
                    .is_some_and(|types| types.iter().any(|kind| kind == "null"))
                    || ["anyOf", "oneOf"].iter().any(|key| {
                        node.get(key)
                            .and_then(Value::as_array)
                            .is_some_and(|branches| {
                                branches.iter().any(|branch| {
                                    branch.get("type").is_some_and(|kind| kind == "null")
                                })
                            })
                    }))
            {
                return Err(error(
                    "nullable map values or array items are not representable",
                ));
            }
        }
        if let Some(properties) = properties {
            for value in properties.values() {
                pending.push((value, depth + 1, in_map, true));
            }
        }
        if let Some(value) = additional.filter(|_| dynamic) {
            pending.push((value, depth + 1, true, false));
        }
        if let Some(value) = node.get("items") {
            if value.is_array() {
                return Err(error("tuple array items are not representable"));
            }
            pending.push((value, depth + 1, in_map, false));
        }
        for key in ["anyOf", "oneOf"] {
            if let Some(branches) = node.get(key).and_then(Value::as_array) {
                for branch in branches {
                    // Optional named null branch is already handled by field.required.
                    if field_slot && branch.get("type").is_some_and(|kind| kind == "null") {
                        continue;
                    }
                    let concrete = branches
                        .iter()
                        .filter(|branch| !branch.get("type").is_some_and(|kind| kind == "null"))
                        .count();
                    pending.push((branch, depth + 1, in_map, field_slot && concrete == 1));
                }
            }
        }
    }
    Ok(())
}

fn annotation(key: &str) -> bool {
    matches!(
        key,
        "title"
            | "description"
            | "default"
            | "examples"
            | "example"
            | "readOnly"
            | "writeOnly"
            | "deprecated"
            | "$comment"
            | "$schema"
            | "$id"
            | "$defs"
            | "definitions"
    )
}

pub fn decode_component(component: &str) -> Result<String, StateSchemaShapeError> {
    let mut output = String::with_capacity(component.len());
    let mut chars = component.chars();
    while let Some(c) = chars.next() {
        if c != '~' {
            output.push(c);
            continue;
        }
        output.push(match chars.next() {
            Some('0') => '~',
            Some('1') => '/',
            _ => return Err(error("invalid definition pointer escape")),
        });
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tagged_object_normalization_retains_intersection_and_authored_source() {
        let original = json!({"type":"object","properties":{
            "device":{"$ref":"#/$defs/Base","type":"object","properties":{"type":{"type":"string","const":"nic"}},"required":["type"],"description":"native tagged device"}
        },"$defs":{"Base":{"type":"object","properties":{"parent":{"type":"string"}},"required":["parent"]}}});
        let preserved = original.clone();
        let prepared = prepare_state_schema(&original).unwrap();
        assert_eq!(original, preserved);
        assert!(prepared["properties"]["device"].get("$ref").is_none());
        assert_eq!(
            prepared["properties"]["device"]["properties"]["type"]["const"],
            "nic"
        );
        let before = jsonschema::validator_for(&original).unwrap();
        let after = jsonschema::validator_for(&prepared).unwrap();
        for value in [
            json!({"device":{"type":"nic","parent":"ovsbr0"}}),
            json!({"device":{"type":"nic","parent":"ovsbr0","arbitrary_open_key":5}}),
            json!({"device":{"type":"nic"}}),
            json!({"device":{"type":"disk","parent":"ovsbr0"}}),
            json!({"device":{"parent":"ovsbr0"}}),
        ] {
            assert_eq!(before.is_valid(&value), after.is_valid(&value));
        }
    }

    #[test]
    fn tagged_normalization_rejects_closed_intersecting_or_extra_constraints() {
        let original = json!({"type":"object","properties":{
            "device":{"$ref":"#/$defs/Base","type":"object","properties":{"type":{"type":"string","const":"nic"}},"required":["type"]}
        },"$defs":{"Base":{"type":"object","properties":{"parent":{"type":"string"}}}}});
        let mut closed = original.clone();
        closed["$defs"]["Base"]["additionalProperties"] = json!(false);
        assert!(prepare_state_schema(&closed).is_err());
        let mut overlapping = original.clone();
        overlapping["$defs"]["Base"]["properties"]["type"] = json!({"type":"string"});
        assert!(prepare_state_schema(&overlapping).is_err());
        let mut constraint = original.clone();
        constraint["properties"]["device"]["maxProperties"] = json!(2);
        assert!(prepare_state_schema(&constraint).is_err());
        let mut numeric = original.clone();
        numeric["properties"]["device"]["properties"]["type"] = json!({"type":"integer","const":1});
        assert!(prepare_state_schema(&numeric).is_err());
        let scalar = json!({"type":"object","properties":{"value":{"$ref":"#/$defs/N","minimum":4}},"$defs":{"N":{"type":"integer"}}});
        assert!(prepare_state_schema(&scalar).is_err());
    }

    #[test]
    fn finite_typed_maps_allow_nested_fields_but_not_lost_constraints() {
        let valid = json!({"type":"object","additionalProperties":{"type":"object","properties":{
            "revision":{"type":"integer","minimum":0},
            "nested":{"type":"object","additionalProperties":{"type":"boolean"}}
        },"required":["revision"]}});
        validate_state_schema(&valid).unwrap();
        for invalid in [
            json!({"type":"object","properties":{"tag":{"type":"string","const":"nic","enum":["disk"]}}}),
            json!({"type":"object","patternProperties":{"^\\d+$":{"type":"string"}},"additionalProperties":false}),
            json!({"type":"object","properties":{"fixed":{"type":"string"}},"additionalProperties":{"type":"boolean"}}),
            json!({"type":"object","additionalProperties":{"type":"integer","minimum":0}}),
            json!({"type":"object","additionalProperties":{"type":["string","null"]}}),
            json!({"type":"object","additionalProperties":{"type":"object","properties":{"x":{"type":"string","minLength":1}}}}),
            json!({"type":"object","additionalProperties":{"type":"string"},"propertyNames":{"pattern":"a"}}),
        ] {
            assert!(validate_state_schema(&invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn cycles_unresolved_and_reference_constraints_fail_before_expansion() {
        for invalid in [
            json!({"$ref":"#/$defs/Cycle","$defs":{"Cycle":{"$ref":"#/$defs/Cycle"}}}),
            json!({"type":"object","properties":{"x":{"$ref":"#/$defs/Cycle"}},"$defs":{"Cycle":{"type":"object","additionalProperties":{"$ref":"#/$defs/Cycle"}}}}),
            json!({"type":"object","properties":{"x":{"$ref":"#/$defs/Missing"}}}),
            json!({"$ref":"https://example.invalid/schema"}),
            json!({"$ref":"#/$defs/T","minimum":3,"$defs":{"T":{"type":"integer"}}}),
        ] {
            assert!(validate_state_schema(&invalid).is_err());
        }
    }
}
