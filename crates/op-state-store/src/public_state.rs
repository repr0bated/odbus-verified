//! Plugin-authored policy for projecting internal state into shared public views.
//!
//! The full state remains authoritative elsewhere. This module only derives a
//! least-privilege outbound view from the exact sealed [`PluginSchema`].

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize, Serializer};
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;
use thiserror::Error;

use crate::plugin_schema::{FieldSchema, FieldType, PluginSchema};

const MAX_PUBLIC_PATHS: usize = 256;
const MAX_PUBLIC_PATH_DEPTH: usize = 32;

/// Explicit state paths a plugin publishes to unaffiliated shared consumers.
///
/// Object-field components use RFC 6901 escaping. A `*` component selects
/// every item of a schema-declared array while retaining only the selected
/// fields within each item. The empty policy publishes no state. A path cannot
/// select the document root, use a numeric array index, or terminate at an
/// opaque [`FieldType::Any`]. Maps require explicit literal keys in the policy;
/// neither a whole map nor a map wildcard can reveal runtime key membership.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PublicStatePolicy {
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        serialize_with = "serialize_paths_sorted"
    )]
    pub paths: Vec<String>,
}

impl PublicStatePolicy {
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
}

fn serialize_paths_sorted<S>(paths: &[String], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut sorted: Vec<&str> = paths.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.serialize(serializer)
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PublicStateProjectionError {
    #[error("invalid public-state policy: {0}")]
    InvalidPolicy(String),
    #[error("internal state does not match the public-state contract: {0}")]
    InvalidState(String),
}

#[derive(Debug, Default)]
struct SelectionNode {
    terminal: bool,
    children: BTreeMap<String, SelectionNode>,
}

impl PluginSchema {
    /// Validate the plugin-authored public state policy without reading state.
    pub fn validate_public_state_policy(&self) -> Result<(), PublicStateProjectionError> {
        self.public_selection_tree().map(|_| ())
    }

    /// Whether the policy publishes any part of one top-level field.
    ///
    /// Consumers use this to distinguish an approved-but-currently-absent
    /// property from a field the plugin did not publish. The complete policy
    /// is validated first, so callers never need their own pointer parser.
    pub fn public_state_exposes_top_level(
        &self,
        field: &str,
    ) -> Result<bool, PublicStateProjectionError> {
        Ok(self.public_selection_tree()?.children.contains_key(field))
    }

    /// Derive the only state view suitable for the shared public namespace.
    ///
    /// `None` means the plugin publishes no public state. A configured policy
    /// returns `Some({})` when its approved fields are currently absent, which
    /// lets consumers distinguish fail-closed policy from field deletion and
    /// issue tombstones against an earlier non-empty view.
    pub fn project_public_state(
        &self,
        state: &Value,
    ) -> Result<Option<Value>, PublicStateProjectionError> {
        if self.public_state.is_empty() {
            return Ok(None);
        }
        let selection = self.public_selection_tree()?;
        let source = state.as_object().ok_or_else(|| {
            PublicStateProjectionError::InvalidState("plugin root must be an object".into())
        })?;
        let projected = project_selected_object(source, &self.fields, &selection, "")?;
        Ok(Some(Value::Object(Box::new(projected))))
    }

    fn public_selection_tree(&self) -> Result<SelectionNode, PublicStateProjectionError> {
        if self.public_state.paths.len() > MAX_PUBLIC_PATHS {
            return Err(invalid_policy(format!(
                "more than {MAX_PUBLIC_PATHS} paths"
            )));
        }
        let mut seen = BTreeSet::new();
        let mut root = SelectionNode::default();
        for pointer in &self.public_state.paths {
            if !seen.insert(pointer.as_str()) {
                return Err(invalid_policy(format!("duplicate path {pointer:?}")));
            }
            let components = decode_pointer(pointer)?;
            validate_schema_path(&self.fields, &components, pointer)?;
            insert_selection(&mut root, &components, pointer)?;
        }
        Ok(root)
    }
}

fn invalid_policy(message: impl Into<String>) -> PublicStateProjectionError {
    PublicStateProjectionError::InvalidPolicy(message.into())
}

fn invalid_state(message: impl Into<String>) -> PublicStateProjectionError {
    PublicStateProjectionError::InvalidState(message.into())
}

fn decode_pointer(pointer: &str) -> Result<Vec<String>, PublicStateProjectionError> {
    if pointer.is_empty() {
        return Err(invalid_policy("the document root cannot be public"));
    }
    if !pointer.starts_with('/') {
        return Err(invalid_policy(format!(
            "path {pointer:?} is not an RFC 6901 pointer"
        )));
    }
    let components = pointer[1..]
        .split('/')
        .map(|component| decode_pointer_component(component, pointer))
        .collect::<Result<Vec<_>, _>>()?;
    if components.len() > MAX_PUBLIC_PATH_DEPTH {
        return Err(invalid_policy(format!(
            "path {pointer:?} exceeds depth {MAX_PUBLIC_PATH_DEPTH}"
        )));
    }
    Ok(components)
}

fn decode_pointer_component(
    component: &str,
    pointer: &str,
) -> Result<String, PublicStateProjectionError> {
    let mut output = String::with_capacity(component.len());
    let mut chars = component.chars();
    while let Some(character) = chars.next() {
        if character != '~' {
            output.push(character);
            continue;
        }
        match chars.next() {
            Some('0') => output.push('~'),
            Some('1') => output.push('/'),
            _ => {
                return Err(invalid_policy(format!(
                    "path {pointer:?} contains invalid RFC 6901 escaping"
                )))
            }
        }
    }
    Ok(output)
}

fn validate_schema_path(
    root_fields: &HashMap<String, FieldSchema>,
    components: &[String],
    pointer: &str,
) -> Result<(), PublicStateProjectionError> {
    let Some((field_name, remaining)) = components.split_first() else {
        return Err(invalid_policy("the document root cannot be public"));
    };
    let field = root_fields.get(field_name).ok_or_else(|| {
        invalid_policy(format!(
            "path {pointer:?} names undeclared field {field_name:?}"
        ))
    })?;
    validate_type_path(&field.field_type, remaining, pointer)
}

fn validate_type_path(
    field_type: &FieldType,
    remaining: &[String],
    pointer: &str,
) -> Result<(), PublicStateProjectionError> {
    let Some((component, tail)) = remaining.split_first() else {
        return if is_closed_public_type(field_type) {
            Ok(())
        } else {
            Err(invalid_policy(format!(
                "path {pointer:?} terminates at an opaque type"
            )))
        };
    };
    match field_type {
        FieldType::Object(fields) => {
            let field = fields.get(component).ok_or_else(|| {
                invalid_policy(format!(
                    "path {pointer:?} names undeclared field {component:?}"
                ))
            })?;
            validate_type_path(&field.field_type, tail, pointer)
        }
        FieldType::Map(item_type) if component != "*" => {
            validate_type_path(item_type, tail, pointer)
        }
        FieldType::Map(_) => Err(invalid_policy(format!(
            "path {pointer:?} must name an explicit map key; wildcards disclose keys"
        ))),
        FieldType::Array(item_type) if component == "*" => {
            validate_type_path(item_type, tail, pointer)
        }
        FieldType::Array(_) => Err(invalid_policy(format!(
            "path {pointer:?} must use '*' for array items"
        ))),
        _ => Err(invalid_policy(format!(
            "path {pointer:?} traverses through a non-container field"
        ))),
    }
}

fn is_closed_public_type(field_type: &FieldType) -> bool {
    match field_type {
        FieldType::Any | FieldType::Map(_) | FieldType::JsonSchema(_) => false,
        FieldType::Array(item) => is_closed_public_type(item),
        FieldType::Object(fields) => fields
            .values()
            .all(|field| is_closed_public_type(&field.field_type)),
        FieldType::OneOf(branches) => {
            !branches.is_empty() && branches.iter().all(is_closed_public_type)
        }
        FieldType::String
        | FieldType::Integer
        | FieldType::Float
        | FieldType::Boolean
        | FieldType::Enum(_) => true,
    }
}

fn insert_selection(
    root: &mut SelectionNode,
    components: &[String],
    pointer: &str,
) -> Result<(), PublicStateProjectionError> {
    let mut current = root;
    for component in components {
        if current.terminal {
            return Err(invalid_policy(format!(
                "path {pointer:?} is already covered by an ancestor"
            )));
        }
        current = current.children.entry(component.clone()).or_default();
    }
    if current.terminal || !current.children.is_empty() {
        return Err(invalid_policy(format!(
            "path {pointer:?} overlaps another public path"
        )));
    }
    current.terminal = true;
    Ok(())
}

fn project_selected_object(
    source: &simd_json::value::owned::Object,
    fields: &HashMap<String, FieldSchema>,
    selection: &SelectionNode,
    parent: &str,
) -> Result<simd_json::value::owned::Object, PublicStateProjectionError> {
    let mut output = simd_json::value::owned::Object::new();
    for (name, selected) in &selection.children {
        let schema = fields.get(name).ok_or_else(|| {
            invalid_policy(format!("selection contains undeclared field {name:?}"))
        })?;
        let Some(value) = source.get(name) else {
            continue;
        };
        if !schema.required && value.is_null() {
            continue;
        }
        let path = format!("{parent}/{}", encode_pointer_component(name));
        let projected = project_selected_value(value, &schema.field_type, selected, &path)?;
        output.insert(name.clone(), projected);
    }
    Ok(output)
}

fn project_selected_value(
    value: &Value,
    field_type: &FieldType,
    selection: &SelectionNode,
    path: &str,
) -> Result<Value, PublicStateProjectionError> {
    if selection.terminal {
        return project_closed_value(value, field_type, path);
    }
    match field_type {
        FieldType::Object(fields) => {
            let source = value
                .as_object()
                .ok_or_else(|| invalid_state(format!("field {path:?} must be an object")))?;
            Ok(Value::Object(Box::new(project_selected_object(
                source, fields, selection, path,
            )?)))
        }
        FieldType::Map(item_type) => {
            let source = value
                .as_object()
                .ok_or_else(|| invalid_state(format!("field {path:?} must be an object map")))?;
            let mut output = simd_json::value::owned::Object::new();
            // Iterate the sealed allowlist, never the private runtime keys.
            for (key, selected) in &selection.children {
                if key == "*" {
                    return Err(invalid_policy("map wildcards are not public"));
                }
                let Some(value) = source.get(key) else {
                    continue;
                };
                let child_path = format!("{path}/{}", encode_pointer_component(key));
                output.insert(
                    key.clone(),
                    project_selected_value(value, item_type, selected, &child_path)?,
                );
            }
            Ok(Value::Object(Box::new(output)))
        }
        FieldType::Array(item_type) => {
            if selection.children.len() != 1 || !selection.children.contains_key("*") {
                return Err(invalid_policy(format!(
                    "path {path:?} has an invalid array selection"
                )));
            }
            let item_selection = &selection.children["*"];
            let items = value
                .as_array()
                .ok_or_else(|| invalid_state(format!("field {path:?} must be an array")))?;
            items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    project_selected_value(
                        item,
                        item_type,
                        item_selection,
                        &format!("{path}/{index}"),
                    )
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array)
        }
        _ => Err(invalid_policy(format!(
            "path {path:?} traverses through a non-container field"
        ))),
    }
}

fn project_closed_value(
    value: &Value,
    field_type: &FieldType,
    path: &str,
) -> Result<Value, PublicStateProjectionError> {
    if !value_matches_type(value, field_type) {
        return Err(invalid_state(format!(
            "field {path:?} has an unexpected type"
        )));
    }
    match field_type {
        FieldType::String
        | FieldType::Integer
        | FieldType::Float
        | FieldType::Boolean
        | FieldType::Enum(_) => Ok(value.clone()),
        FieldType::Array(item_type) => value
            .as_array()
            .expect("type checked")
            .iter()
            .enumerate()
            .map(|(index, item)| project_closed_value(item, item_type, &format!("{path}/{index}")))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        FieldType::Object(fields) => {
            let source = value.as_object().expect("type checked");
            let mut output = simd_json::value::owned::Object::new();
            for (name, field) in fields {
                if let Some(child) = source.get(name) {
                    if !field.required && child.is_null() {
                        continue;
                    }
                    let child_path = format!("{path}/{}", encode_pointer_component(name));
                    output.insert(
                        name.clone(),
                        project_closed_value(child, &field.field_type, &child_path)?,
                    );
                }
            }
            Ok(Value::Object(Box::new(output)))
        }
        FieldType::OneOf(branches) => {
            let branch = branches
                .iter()
                .find(|branch| value_matches_type(value, branch))
                .expect("type checked");
            project_closed_value(value, branch, path)
        }
        FieldType::Any | FieldType::Map(_) | FieldType::JsonSchema(_) => Err(invalid_policy(
            format!("path {path:?} selects an opaque value"),
        )),
    }
}

fn value_matches_type(value: &Value, field_type: &FieldType) -> bool {
    match field_type {
        FieldType::String => value.is_str(),
        FieldType::Integer => value.is_i64() || value.is_u64(),
        FieldType::Float => value.is_f64() || value.is_i64() || value.is_u64(),
        FieldType::Boolean => value.is_bool(),
        FieldType::Array(item_type) => value
            .as_array()
            .is_some_and(|items| items.iter().all(|item| value_matches_type(item, item_type))),
        FieldType::Map(item_type) => value.as_object().is_some_and(|object| {
            object
                .values()
                .all(|item| value_matches_type(item, item_type))
        }),
        FieldType::Object(fields) => value.as_object().is_some_and(|object| {
            fields.iter().all(|(name, field)| {
                object
                    .get(name)
                    .map(|value| {
                        (!field.required && value.is_null())
                            || value_matches_type(value, &field.field_type)
                    })
                    .unwrap_or(!field.required)
            })
        }),
        FieldType::Enum(values) => value
            .as_str()
            .is_some_and(|value| values.iter().any(|candidate| candidate == value)),
        FieldType::OneOf(branches) => branches
            .iter()
            .any(|branch| value_matches_type(value, branch)),
        FieldType::JsonSchema(_) => false,
        FieldType::Any => true,
    }
}

fn encode_pointer_component(component: &str) -> String {
    component.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin_schema::{Constraint, FieldSchema};
    use simd_json::json;

    fn field(field_type: FieldType, required: bool) -> FieldSchema {
        FieldSchema {
            field_type,
            required,
            description: String::new(),
            default: None,
            example: None,
            constraints: Vec::<Constraint>::new(),
            read_only: false,
            read_only_when: None,
        }
    }

    fn schema(fields: HashMap<String, FieldSchema>, paths: &[&str]) -> PluginSchema {
        let mut builder = PluginSchema::builder("fixture");
        for (name, field) in fields {
            builder = builder.field(&name, field);
        }
        builder.public_state_paths(paths).build()
    }

    #[test]
    fn absent_policy_exposes_no_state() {
        let schema = PluginSchema::builder("fixture")
            .string_field("status", false, "")
            .build();
        assert_eq!(
            schema
                .project_public_state(&json!({"status":"ok","secret":"no"}))
                .unwrap(),
            None
        );
    }

    #[test]
    fn typed_parent_drops_unknown_nested_and_top_level_fields() {
        let nested = HashMap::from([
            ("enabled".into(), field(FieldType::Boolean, false)),
            ("label".into(), field(FieldType::String, false)),
        ]);
        let schema = schema(
            HashMap::from([
                ("config".into(), field(FieldType::Object(nested), false)),
                ("private".into(), field(FieldType::String, false)),
            ]),
            &["/config"],
        );
        let projected = schema
            .project_public_state(&json!({
                "config":{"enabled":true,"label":"public","token":"hidden"},
                "private":"hidden",
                "unknown":{"credential":"hidden"}
            }))
            .unwrap();
        assert_eq!(
            projected,
            Some(json!({"config":{"enabled":true,"label":"public"}}))
        );
    }

    #[test]
    fn nested_path_selects_only_that_declared_child() {
        let nested = HashMap::from([
            ("status".into(), field(FieldType::String, false)),
            ("details".into(), field(FieldType::Any, false)),
        ]);
        let schema = schema(
            HashMap::from([("provider".into(), field(FieldType::Object(nested), false))]),
            &["/provider/status"],
        );
        assert_eq!(
            schema
                .project_public_state(&json!({
                    "provider":{"status":"ready","details":{"token":"hidden"}}
                }))
                .unwrap(),
            Some(json!({"provider":{"status":"ready"}}))
        );
    }

    #[test]
    fn opaque_terminal_and_overlapping_rules_are_rejected() {
        let opaque = schema(
            HashMap::from([("config".into(), field(FieldType::Any, false))]),
            &["/config"],
        );
        assert!(matches!(
            opaque.validate_public_state_policy(),
            Err(PublicStateProjectionError::InvalidPolicy(_))
        ));

        let nested = HashMap::from([("status".into(), field(FieldType::String, false))]);
        let overlapping = schema(
            HashMap::from([("provider".into(), field(FieldType::Object(nested), false))]),
            &["/provider/status", "/provider"],
        );
        assert!(matches!(
            overlapping.validate_public_state_policy(),
            Err(PublicStateProjectionError::InvalidPolicy(_))
        ));
    }

    #[test]
    fn runtime_type_drift_fails_closed_without_echoing_value() {
        let schema = schema(
            HashMap::from([("status".into(), field(FieldType::String, false))]),
            &["/status"],
        );
        let error = schema
            .project_public_state(&json!({"status":{"token":"must-not-appear"}}))
            .unwrap_err();
        assert!(matches!(error, PublicStateProjectionError::InvalidState(_)));
        assert!(!error.to_string().contains("must-not-appear"));
    }

    #[test]
    fn configured_but_absent_fields_produce_empty_public_object() {
        let schema = schema(
            HashMap::from([("status".into(), field(FieldType::String, false))]),
            &["/status"],
        );
        assert_eq!(
            schema.project_public_state(&json!({})).unwrap(),
            Some(json!({}))
        );
        assert!(schema.public_state_exposes_top_level("status").unwrap());
        assert!(!schema.public_state_exposes_top_level("private").unwrap());
    }

    #[test]
    fn optional_null_is_withheld_instead_of_causing_or_widening_projection() {
        let schema = schema(
            HashMap::from([("status".into(), field(FieldType::String, false))]),
            &["/status"],
        );
        assert_eq!(
            schema
                .project_public_state(&json!({"status":null,"other":"hidden"}))
                .unwrap(),
            Some(json!({}))
        );
    }

    #[test]
    fn typed_arrays_filter_unknown_object_members() {
        let item = FieldType::Object(HashMap::from([(
            "name".into(),
            field(FieldType::String, true),
        )]));
        let schema = schema(
            HashMap::from([(
                "items".into(),
                field(FieldType::Array(Box::new(item)), false),
            )]),
            &["/items"],
        );
        assert_eq!(
            schema
                .project_public_state(&json!({
                    "items":[{"name":"one","credential":"hidden"}]
                }))
                .unwrap(),
            Some(json!({"items":[{"name":"one"}]}))
        );
    }

    #[test]
    fn array_item_rules_preserve_order_length_and_drop_unselected_fields() {
        let item = FieldType::Object(HashMap::from([
            ("name".into(), field(FieldType::String, false)),
            ("status".into(), field(FieldType::String, false)),
            ("details".into(), field(FieldType::Any, false)),
        ]));
        let schema = schema(
            HashMap::from([(
                "items".into(),
                field(FieldType::Array(Box::new(item)), false),
            )]),
            &["/items/*/name", "/items/*/status"],
        );
        assert_eq!(
            schema
                .project_public_state(&json!({
                    "items":[
                        {"name":"one","status":"up","details":{"token":"hidden"},"unknown":"hidden"},
                        {"name":"two","details":{"password":"hidden"}}
                    ]
                }))
                .unwrap(),
            Some(json!({
                "items":[
                    {"name":"one","status":"up"},
                    {"name":"two"}
                ]
            }))
        );
    }

    #[test]
    fn nested_arrays_project_only_selected_typed_leaves() {
        let child = FieldType::Object(HashMap::from([
            ("id".into(), field(FieldType::String, false)),
            ("opaque".into(), field(FieldType::Any, false)),
        ]));
        let parent = FieldType::Object(HashMap::from([(
            "children".into(),
            field(FieldType::Array(Box::new(child)), false),
        )]));
        let schema = schema(
            HashMap::from([(
                "parents".into(),
                field(FieldType::Array(Box::new(parent)), false),
            )]),
            &["/parents/*/children/*/id"],
        );
        assert_eq!(
            schema
                .project_public_state(&json!({
                    "parents":[{"children":[{"id":"c1","opaque":{"secret":"hidden"}}]}]
                }))
                .unwrap(),
            Some(json!({"parents":[{"children":[{"id":"c1"}]}]}))
        );
    }

    #[test]
    fn array_rule_rejects_indexes_and_type_drift_before_returning_a_view() {
        let item = FieldType::Object(HashMap::from([(
            "name".into(),
            field(FieldType::String, false),
        )]));
        let indexed = schema(
            HashMap::from([(
                "items".into(),
                field(FieldType::Array(Box::new(item.clone())), false),
            )]),
            &["/items/0/name"],
        );
        assert!(matches!(
            indexed.validate_public_state_policy(),
            Err(PublicStateProjectionError::InvalidPolicy(_))
        ));

        let selected = schema(
            HashMap::from([(
                "items".into(),
                field(FieldType::Array(Box::new(item)), false),
            )]),
            &["/items/*/name"],
        );
        let error = selected
            .project_public_state(&json!({
                "items":[{"name":"ok"},{"name":{"secret":"must-not-appear"}}]
            }))
            .unwrap_err();
        assert!(matches!(error, PublicStateProjectionError::InvalidState(_)));
        assert!(!error.to_string().contains("must-not-appear"));
    }

    #[test]
    fn star_is_a_literal_name_when_traversing_an_object() {
        let schema = schema(
            HashMap::from([("*".into(), field(FieldType::String, false))]),
            &["/*"],
        );
        assert_eq!(
            schema
                .project_public_state(&json!({"*":"visible","other":"hidden"}))
                .unwrap(),
            Some(json!({"*":"visible"}))
        );
    }

    #[test]
    fn rfc6901_escaped_field_names_are_supported() {
        let schema = schema(
            HashMap::from([("a/b~c".into(), field(FieldType::String, false))]),
            &["/a~1b~0c"],
        );
        assert_eq!(
            schema
                .project_public_state(&json!({"a/b~c":"visible"}))
                .unwrap(),
            Some(json!({"a/b~c":"visible"}))
        );
    }

    #[test]
    fn policy_is_plugin_schema_owned_and_roundtrips_canonically() {
        let withheld = PluginSchema::builder("withheld")
            .string_field("status", false, "")
            .build();
        let withheld_json = serde_json::to_value(&withheld).unwrap();
        assert!(withheld_json.get("public_state").is_none());
        let withheld_roundtrip: PluginSchema = serde_json::from_value(withheld_json).unwrap();
        assert!(withheld_roundtrip.public_state.is_empty());

        let exposed = PluginSchema::builder("exposed")
            .string_field("status", false, "")
            .string_field("alpha", false, "")
            .public_state_paths(&["/status", "/alpha"])
            .build();
        let first = serde_json::to_vec(&exposed).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&first).unwrap()["public_state"]["paths"],
            serde_json::json!(["/alpha", "/status"])
        );
        let roundtrip: PluginSchema = serde_json::from_slice(&first).unwrap();
        assert_eq!(
            roundtrip.public_state,
            PublicStatePolicy {
                paths: vec!["/alpha".into(), "/status".into()]
            }
        );
        assert_eq!(
            serde_json::to_vec(&exposed.public_state).unwrap(),
            br#"{"paths":["/alpha","/status"]}"#
        );
        assert_eq!(
            serde_json::to_value(&exposed).unwrap(),
            serde_json::to_value(&roundtrip).unwrap()
        );
    }
}

#[cfg(test)]
mod typed_map_privacy_tests {
    use super::*;
    use simd_json::json;
    fn field(field_type: FieldType) -> FieldSchema {
        FieldSchema {
            field_type,
            required: false,
            description: String::new(),
            default: None,
            example: None,
            constraints: vec![],
            read_only: false,
            read_only_when: None,
        }
    }
    fn schema(paths: &[&str]) -> PluginSchema {
        let item = FieldType::Object(HashMap::from([
            ("status".into(), field(FieldType::String)),
            ("secret".into(), field(FieldType::Any)),
            (
                "children".into(),
                field(FieldType::Map(Box::new(FieldType::Object(HashMap::from(
                    [
                        ("ready".into(), field(FieldType::Boolean)),
                        ("private".into(), field(FieldType::Any)),
                    ],
                ))))),
            ),
        ]));
        PluginSchema::builder("fixture")
            .field("jobs", field(FieldType::Map(Box::new(item))))
            .public_state_paths(paths)
            .build()
    }
    #[test]
    fn explicit_key_and_grandchild_projection_never_enumerates_private_keys() {
        let schema = schema(&["/jobs/public/status", "/jobs/public/children/a~1b~0c/ready"]);
        let state = json!({"jobs":{
            "public":{"status":"ok","secret":"HIDDEN","children":{"a/b~c":{"ready":false,"private":"HIDDEN"},"PRIVATE_CHILD":{"private":"HIDDEN"}}},
            "PRIVATE_JOB":false
        }});
        assert_eq!(
            schema.project_public_state(&state).unwrap(),
            Some(json!({"jobs":{"public":{"status":"ok","children":{"a/b~c":{"ready":false}}}}}))
        );
        assert_eq!(
            schema
                .project_public_state(&json!({"jobs":{"PRIVATE_JOB":false}}))
                .unwrap(),
            Some(json!({"jobs":{}}))
        );
        assert!(schema
            .project_public_state(&json!({"jobs":{"public":{"status":false}}}))
            .is_err());
    }
    #[test]
    fn whole_maps_wildcards_and_opaque_values_are_not_public() {
        for paths in [
            vec!["/jobs"],
            vec!["/jobs/*/status"],
            vec!["/jobs/public"],
            vec!["/jobs/public/secret"],
            vec!["/jobs/public/children/*/ready"],
        ] {
            assert!(schema(&paths).validate_public_state_policy().is_err());
        }
        // Even a closed scalar map cannot disclose all keys by selecting its parent.
        let scalar = PluginSchema::builder("fixture")
            .field("labels", field(FieldType::Map(Box::new(FieldType::String))))
            .public_state_path("/labels")
            .build();
        assert!(scalar.validate_public_state_policy().is_err());
    }
}
