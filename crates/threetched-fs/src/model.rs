use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const FORMAT_VERSION: &str = "3tchedfs/1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TypedObjectBlob {
    pub format: String,
    pub plugin_id: String,
    pub source_schema_hash: String,
    pub schema: PluginSchema,
    pub value: Value,
}

impl TypedObjectBlob {
    pub fn new(
        plugin_id: String,
        source_schema_hash: String,
        schema: PluginSchema,
        value: Value,
    ) -> Result<Self> {
        let blob = Self {
            format: FORMAT_VERSION.to_string(),
            plugin_id,
            source_schema_hash,
            schema,
            value,
        };
        blob.validate(None)?;
        Ok(blob)
    }

    pub fn validate(&self, base: Option<&Value>) -> Result<()> {
        if self.format != FORMAT_VERSION {
            bail!("unsupported typed object format '{}'", self.format);
        }
        if self.schema.name != self.plugin_id {
            bail!(
                "schema name '{}' does not match plugin id '{}'",
                self.schema.name,
                self.plugin_id
            );
        }
        self.schema.validate_value(&self.value, base)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginSchema {
    pub name: String,
    #[serde(default)]
    pub category: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub fields: BTreeMap<String, FieldSchema>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub immutable_paths: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub dialect: String,
    #[serde(default)]
    pub mutation_index: Option<u64>,
    #[serde(default)]
    pub subids: BTreeMap<String, String>,
    #[serde(default)]
    pub org: Option<String>,
    #[serde(default)]
    pub methods: BTreeMap<String, MethodDecl>,
    #[serde(default)]
    pub signals: Vec<SignalDecl>,
    #[serde(default)]
    pub guarantees: PluginGuarantees,
}

impl PluginSchema {
    pub fn validate_value(&self, root: &Value, base: Option<&Value>) -> Result<()> {
        let object = root
            .as_object()
            .with_context(|| format!("{} state must be an object", self.name))?;

        for (name, field) in &self.fields {
            match object.get(name) {
                Some(value) => field
                    .validate(value, root)
                    .with_context(|| format!("field /{name}"))?,
                None if field.required => bail!("required field /{name} is missing"),
                None => {}
            }
        }

        if let Some(base) = base {
            let base_object = base
                .as_object()
                .with_context(|| format!("base {} state must be an object", self.name))?;
            validate_object_immutability(&self.fields, object, base_object, "")?;

            for pointer in &self.immutable_paths {
                if root.pointer(pointer) != base.pointer(pointer) {
                    bail!("immutable path {pointer} changed");
                }
            }

            if self.tags.iter().any(|tag| tag == "immutable") && root != base {
                bail!("plugin {} is immutable", self.name);
            }
        }

        Ok(())
    }

    pub fn template(&self) -> Value {
        let values = self
            .fields
            .iter()
            .map(|(name, schema)| (name.clone(), schema.template()))
            .collect();
        Value::Object(values)
    }

    pub fn capabilities(&self) -> BTreeSet<&str> {
        self.methods
            .values()
            .filter_map(|method| method.required_capability.as_deref())
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldSchema {
    pub field_type: FieldType,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub example: Option<Value>,
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub read_only_when: Option<ReadOnlyCondition>,
}

impl FieldSchema {
    pub fn validate(&self, value: &Value, root: &Value) -> Result<()> {
        // PluginSchema represents `Option<T>` as a non-required T while live
        // state may preserve an explicit JSON null for that option.
        if value.is_null() && !self.required {
            return Ok(());
        }
        self.field_type.validate(value, root)?;
        for constraint in &self.constraints {
            constraint.validate(value, root)?;
        }
        Ok(())
    }

    pub fn template(&self) -> Value {
        self.default
            .clone()
            .or_else(|| self.example.clone())
            .unwrap_or_else(|| self.field_type.template())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    String,
    Integer,
    Float,
    Boolean,
    Array(Box<FieldType>),
    Object(BTreeMap<String, FieldSchema>),
    /// Dynamic string keys with one declared value type. Wire form matches
    /// op-state-store: `{"map": <FieldType>}`.
    Map(Box<FieldType>),
    /// Explicit exact JSON Schema document retained from the sealed plugin.
    /// Wire form matches op-state-store: `{"json_schema": <document>}`.
    JsonSchema(Box<Value>),
    Enum(Vec<String>),
    OneOf(Vec<FieldType>),
    Any,
}

impl FieldType {
    pub fn validate(&self, value: &Value, root: &Value) -> Result<()> {
        match self {
            Self::String if !value.is_string() => bail!("expected string"),
            Self::Integer if !value.is_i64() && !value.is_u64() => bail!("expected integer"),
            Self::Float if !value.is_number() => bail!("expected number"),
            Self::Boolean if !value.is_boolean() => bail!("expected boolean"),
            Self::Array(item_type) => {
                let values = value.as_array().context("expected array")?;
                for (index, value) in values.iter().enumerate() {
                    item_type
                        .validate(value, root)
                        .with_context(|| format!("array element {index}"))?;
                }
            }
            Self::Object(fields) => {
                let object = value.as_object().context("expected object")?;
                for (name, field) in fields {
                    match object.get(name) {
                        Some(value) => field
                            .validate(value, root)
                            .with_context(|| format!("object field {name}"))?,
                        None if field.required => bail!("required object field {name} is missing"),
                        None => {}
                    }
                }
            }
            Self::Map(item_type) => {
                let object = value.as_object().context("expected object map")?;
                for (index, value) in object.values().enumerate() {
                    // Dynamic keys may be private; keep errors key-agnostic.
                    item_type
                        .validate(value, root)
                        .with_context(|| format!("map value {index}"))?;
                }
            }
            Self::JsonSchema(schema) => {
                jsonschema::Validator::new(schema)
                    .context("compile embedded json_schema field")?
                    .validate(value)
                    .map_err(|error| anyhow::anyhow!("value failed json_schema: {error}"))?;
            }
            Self::Enum(variants) => {
                let value = value.as_str().context("expected enum string")?;
                if !variants.iter().any(|variant| variant == value) {
                    bail!("'{value}' is not an allowed enum value");
                }
            }
            Self::OneOf(branches) => {
                let matches = branches
                    .iter()
                    .filter(|branch| branch.validate(value, root).is_ok())
                    .count();
                // The current schemars adapter reduces some discriminated
                // variants to the same FieldType shape. Require at least one
                // matching branch; the sealed JSON Schema remains available
                // in the object for stricter discriminator-aware consumers.
                if matches == 0 {
                    bail!("value does not match any union branch");
                }
            }
            Self::Any | Self::String | Self::Integer | Self::Float | Self::Boolean => {}
        }
        Ok(())
    }

    pub fn template(&self) -> Value {
        match self {
            Self::String => Value::String(String::new()),
            Self::Integer => Value::from(0),
            Self::Float => Value::from(0.0),
            Self::Boolean => Value::Bool(false),
            Self::Array(_) => Value::Array(Vec::new()),
            Self::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(name, field)| (name.clone(), field.template()))
                    .collect(),
            ),
            Self::Map(_) => Value::Object(serde_json::Map::new()),
            Self::JsonSchema(schema) => json_schema_template(schema),
            Self::Any => Value::Null,
            Self::Enum(values) => values
                .first()
                .cloned()
                .map(Value::String)
                .unwrap_or(Value::Null),
            Self::OneOf(branches) => branches.first().map(Self::template).unwrap_or(Value::Null),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReadOnlyCondition {
    pub property: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Constraint {
    Min { value: f64 },
    Max { value: f64 },
    Pattern { regex: String },
    OneOf { values: Vec<Value> },
    RequiresField { field: String },
    Custom { validator: String },
}

impl Constraint {
    fn validate(&self, value: &Value, root: &Value) -> Result<()> {
        match self {
            Self::Min { value: minimum } => {
                let actual = measure(value).context("min constraint requires number or length")?;
                if actual < *minimum {
                    bail!("{actual} is below minimum {minimum}");
                }
            }
            Self::Max { value: maximum } => {
                let actual = measure(value).context("max constraint requires number or length")?;
                if actual > *maximum {
                    bail!("{actual} exceeds maximum {maximum}");
                }
            }
            Self::Pattern { regex } => {
                let actual = value
                    .as_str()
                    .context("pattern constraint requires string")?;
                if !Regex::new(regex)
                    .with_context(|| format!("invalid schema regex '{regex}'"))?
                    .is_match(actual)
                {
                    bail!("value does not match pattern '{regex}'");
                }
            }
            Self::OneOf { values } if !values.contains(value) => {
                bail!("value is not in the allowed set");
            }
            Self::RequiresField { field } if root.get(field).is_none() => {
                bail!("required peer field '{field}' is missing");
            }
            // A custom validator names plugin code that this standalone
            // projection cannot execute. The plugin dispatch path remains the
            // final authority when a staged value is applied.
            Self::Custom { .. } | Self::OneOf { .. } | Self::RequiresField { .. } => {}
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MethodDecl {
    pub name: String,
    pub args: Value,
    #[serde(default)]
    pub returns: Option<Value>,
    pub side_effect: SideEffect,
    pub idempotent: bool,
    #[serde(default)]
    pub required_capability: Option<String>,
    pub subid: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SideEffect {
    Read,
    Mutation,
    External,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignalDecl {
    pub name: String,
    #[serde(default)]
    pub payload: Option<Value>,
    pub subid: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginGuarantees {
    #[serde(default)]
    pub supports_rollback: bool,
    #[serde(default)]
    pub supports_checkpoints: bool,
    #[serde(default)]
    pub supports_verification: bool,
    #[serde(default)]
    pub atomic_operations: bool,
}

fn measure(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().map(|value| value.chars().count() as f64))
        .or_else(|| value.as_array().map(|value| value.len() as f64))
        .or_else(|| value.as_object().map(|value| value.len() as f64))
}

fn json_scalar_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}

fn validate_object_immutability(
    fields: &BTreeMap<String, FieldSchema>,
    current: &serde_json::Map<String, Value>,
    base: &serde_json::Map<String, Value>,
    parent_pointer: &str,
) -> Result<()> {
    for (name, field) in fields {
        let pointer = push_pointer(parent_pointer, name);
        let current_value = current.get(name);
        let base_value = base.get(name);
        if field.read_only && current_value != base_value {
            bail!("read-only field {pointer} changed");
        }
        if let Some(condition) = &field.read_only_when {
            let trigger = base
                .get(&condition.property)
                .map(json_scalar_text)
                .as_deref()
                == Some(condition.value.as_str());
            if trigger && current_value != base_value {
                bail!("conditionally read-only field {pointer} changed");
            }
        }
        if let (Some(current_value), Some(base_value)) = (current_value, base_value) {
            validate_type_immutability(&field.field_type, current_value, base_value, &pointer)?;
        }
    }
    Ok(())
}

fn validate_type_immutability(
    field_type: &FieldType,
    current: &Value,
    base: &Value,
    pointer: &str,
) -> Result<()> {
    match field_type {
        FieldType::Object(fields) => {
            if let (Some(current), Some(base)) = (current.as_object(), base.as_object()) {
                validate_object_immutability(fields, current, base, pointer)?;
            }
        }
        FieldType::Map(item_type) => {
            if let (Some(current), Some(base)) = (current.as_object(), base.as_object()) {
                for (key, current_value) in current {
                    if let Some(base_value) = base.get(key) {
                        validate_type_immutability(
                            item_type,
                            current_value,
                            base_value,
                            &push_pointer(pointer, key),
                        )?;
                    }
                }
            }
        }
        FieldType::Array(item_type) => {
            if let (Some(current), Some(base)) = (current.as_array(), base.as_array()) {
                for (index, (current, base)) in current.iter().zip(base).enumerate() {
                    validate_type_immutability(
                        item_type,
                        current,
                        base,
                        &push_pointer(pointer, &index.to_string()),
                    )?;
                }
            }
        }
        FieldType::OneOf(branches) => {
            for branch in branches {
                if branch.validate(current, current).is_ok() && branch.validate(base, base).is_ok()
                {
                    validate_type_immutability(branch, current, base, pointer)?;
                }
            }
        }
        FieldType::String
        | FieldType::Integer
        | FieldType::Float
        | FieldType::Boolean
        | FieldType::Enum(_)
        | FieldType::JsonSchema(_)
        | FieldType::Any => {}
    }
    Ok(())
}

fn push_pointer(parent: &str, segment: &str) -> String {
    let escaped = segment.replace('~', "~0").replace('/', "~1");
    format!("{parent}/{escaped}")
}

/// A value that satisfies `schema` when no live state file exists.
/// Missing plugin state is filled from this template and then validated.
fn json_schema_template(schema: &Value) -> Value {
    if let Some(const_value) = schema.get("const") {
        return const_value.clone();
    }
    if let Some(first) = schema
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
    {
        return first.clone();
    }
    match json_schema_type_name(schema) {
        Some("object") => {
            let mut object = serde_json::Map::new();
            let properties = schema.get("properties").and_then(Value::as_object);
            if let Some(required) = schema.get("required").and_then(Value::as_array) {
                for name in required.iter().filter_map(Value::as_str) {
                    let child = properties
                        .and_then(|fields| fields.get(name))
                        .unwrap_or(&Value::Null);
                    object.insert(name.to_string(), json_schema_template(child));
                }
            }
            Value::Object(object)
        }
        Some("array") => Value::Array(Vec::new()),
        Some("string") => Value::String(String::new()),
        Some("integer") => schema
            .get("minimum")
            .and_then(Value::as_i64)
            .map(Value::from)
            .unwrap_or_else(|| Value::from(0)),
        Some("number") => Value::from(0.0),
        Some("boolean") => Value::Bool(false),
        Some("null") => Value::Null,
        _ => schema
            .get("oneOf")
            .or_else(|| schema.get("anyOf"))
            .and_then(Value::as_array)
            .and_then(|branches| branches.first())
            .map(json_schema_template)
            .unwrap_or(Value::Null),
    }
}

fn json_schema_type_name(schema: &Value) -> Option<&str> {
    match schema.get("type") {
        Some(Value::String(name)) => Some(name.as_str()),
        Some(Value::Array(names)) => names
            .iter()
            .filter_map(Value::as_str)
            .find(|name| *name != "null"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> PluginSchema {
        PluginSchema {
            name: "demo".into(),
            category: "test".into(),
            version: "1".into(),
            description: String::new(),
            display_name: None,
            fields: BTreeMap::from([
                (
                    "name".into(),
                    FieldSchema {
                        field_type: FieldType::String,
                        required: true,
                        description: String::new(),
                        default: None,
                        example: None,
                        constraints: vec![Constraint::Min { value: 1.0 }],
                        read_only: false,
                        read_only_when: None,
                    },
                ),
                (
                    "id".into(),
                    FieldSchema {
                        field_type: FieldType::Integer,
                        required: true,
                        description: String::new(),
                        default: Some(Value::from(1)),
                        example: None,
                        constraints: Vec::new(),
                        read_only: true,
                        read_only_when: None,
                    },
                ),
            ]),
            dependencies: Vec::new(),
            immutable_paths: Vec::new(),
            tags: Vec::new(),
            dialect: String::new(),
            mutation_index: None,
            subids: BTreeMap::new(),
            org: None,
            methods: BTreeMap::new(),
            signals: Vec::new(),
            guarantees: PluginGuarantees::default(),
        }
    }

    #[test]
    fn validates_typed_state_and_read_only_fields() {
        let schema = schema();
        let base = serde_json::json!({"name": "before", "id": 1});
        schema
            .validate_value(&serde_json::json!({"name": "after", "id": 1}), Some(&base))
            .unwrap();
        let error = schema
            .validate_value(&serde_json::json!({"name": "after", "id": 2}), Some(&base))
            .unwrap_err();
        assert!(error.to_string().contains("read-only field /id"));
    }

    #[test]
    fn validates_nested_read_only_fields() {
        let mut schema = schema();
        schema.fields.insert(
            "nested".into(),
            FieldSchema {
                field_type: FieldType::Object(BTreeMap::from([(
                    "fixed".into(),
                    FieldSchema {
                        field_type: FieldType::Integer,
                        required: true,
                        description: String::new(),
                        default: None,
                        example: None,
                        constraints: Vec::new(),
                        read_only: true,
                        read_only_when: None,
                    },
                )])),
                required: true,
                description: String::new(),
                default: None,
                example: None,
                constraints: Vec::new(),
                read_only: false,
                read_only_when: None,
            },
        );
        let base = serde_json::json!({"name": "before", "id": 1, "nested": {"fixed": 1}});
        let error = schema
            .validate_value(
                &serde_json::json!({"name": "after", "id": 1, "nested": {"fixed": 2}}),
                Some(&base),
            )
            .unwrap_err();
        assert!(error.to_string().contains("read-only field /nested/fixed"));
    }

    #[test]
    fn parses_producer_map_and_json_schema_field_types() {
        // Wire forms from op-state-store FieldType (externally tagged,
        // rename_all = snake_case): {"map": <FieldType>} and
        // {"json_schema": <document>}.
        let map: FieldType = serde_json::from_value(serde_json::json!({"map": "string"})).unwrap();
        assert_eq!(map, FieldType::Map(Box::new(FieldType::String)));

        let nested_map: FieldType = serde_json::from_value(serde_json::json!({
            "map": {
                "object": {
                    "ready": {
                        "field_type": "boolean",
                        "required": true
                    }
                }
            }
        }))
        .unwrap();
        match nested_map {
            FieldType::Map(inner) => match *inner {
                FieldType::Object(fields) => {
                    assert_eq!(fields["ready"].field_type, FieldType::Boolean);
                    assert!(fields["ready"].required);
                }
                other => panic!("expected object value type, got {other:?}"),
            },
            other => panic!("expected map, got {other:?}"),
        }

        let document = serde_json::json!({
            "type": "object",
            "properties": {
                "revision": {"type": "integer", "minimum": 2}
            },
            "required": ["revision"],
            "additionalProperties": false
        });
        let exact: FieldType = serde_json::from_value(serde_json::json!({
            "json_schema": document
        }))
        .unwrap();
        match &exact {
            FieldType::JsonSchema(schema) => assert_eq!(schema.as_ref(), &document),
            other => panic!("expected json_schema, got {other:?}"),
        }

        exact
            .validate(&serde_json::json!({"revision": 2}), &Value::Null)
            .unwrap();
        assert!(
            exact
                .validate(&serde_json::json!({"revision": 1}), &Value::Null)
                .is_err()
        );
        let templated = exact.template();
        exact.validate(&templated, &Value::Null).unwrap();
        assert_eq!(templated, serde_json::json!({"revision": 2}));

        let open_object = FieldType::JsonSchema(Box::new(serde_json::json!({
            "type": "object",
            "additionalProperties": {"type": "object"}
        })));
        assert_eq!(open_object.template(), serde_json::json!({}));
        open_object
            .validate(&open_object.template(), &Value::Null)
            .unwrap();

        FieldType::Map(Box::new(FieldType::Integer))
            .validate(&serde_json::json!({"a": 1, "b": 2}), &Value::Null)
            .unwrap();
        assert!(
            FieldType::Map(Box::new(FieldType::Integer))
                .validate(&serde_json::json!({"a": "no"}), &Value::Null)
                .is_err()
        );
    }
}
