//! `PluginSchema` — the published single source of truth for a plugin's
//! contract. Faithful port of the shapes in
//! `op-plugins/src/state_plugins/schemars_adapter.rs` and the spec's Layer 2
//! (`op-state-store/src/plugin_schema.rs`), reduced to what the blob pipeline
//! needs and using `serde_json::Value` for embedded schema documents.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub const DIALECT_2020_12: &str = "https://json-schema.org/draft/2020-12/schema";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    String,
    Integer,
    Float,
    Boolean,
    Any,
    Enum(Vec<String>),
    /// Discriminated union (schemars `anyOf`/`oneOf` with >1 non-null branch).
    OneOf(Vec<FieldType>),
    Array(Box<FieldType>),
    Object(HashMap<String, FieldSchema>),
    /// Dynamic string keys with one declared value type. Object retains its
    /// existing named-field representation; map keys are never public implicitly.
    Map(Box<FieldType>),
    /// Explicit, self-contained exact JSON Schema retained from the plugin.
    /// Not a generic fallback; shared public projection denies this boundary.
    JsonSchema(Box<Value>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Constraint {
    Min { value: f64 },
    Max { value: f64 },
    Pattern { regex: String },
    OneOf { values: Vec<Value> },
    RequiresField { field: String },
    Custom { validator: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldSchema {
    pub field_type: FieldType,
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
    pub read_only_when: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffect {
    Read,
    Mutation,
    External,
}

/// A method declaration. `args`/`returns` are full JSON Schema 2020-12
/// documents produced by `schemars::schema_for!` on the input/output structs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MethodDecl {
    pub name: String,
    pub args: Value,
    /// Always `Some` for spec-compliant plugins (REQ-4.4): success/failure-only
    /// methods return `AckOutput`.
    pub returns: Option<Value>,
    pub side_effect: SideEffect,
    pub idempotent: bool,
    pub required_capability: Option<String>,
    /// OSCAL subid, e.g. `mut.network.wireguard.peer-add@v1`.
    pub subid: String,
}

/// The published contract object. Consumers read this — never raw schemars
/// output (spec Key Decision 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginSchema {
    pub name: String,
    pub version: String,
    pub description: String,
    /// From `adapter::plugin_schema_from_json` (schemars-derived state struct).
    pub fields: HashMap<String, FieldSchema>,
    /// Set imperatively in the plugin's schema function.
    pub methods: HashMap<String, MethodDecl>,
    /// From `x-oscal-subid` annotations; `__schema__` is the schema-level subid.
    pub subids: HashMap<String, String>,
    /// From struct-level `x-immutable-paths`.
    pub immutable_paths: Vec<String>,
    pub dialect: String,
}

impl PluginSchema {
    pub fn builder(name: &str) -> PluginSchemaBuilder {
        PluginSchemaBuilder {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            description: String::new(),
            methods: HashMap::new(),
        }
    }

    /// Deterministic canonical JSON of the whole contract. serde_json's Map is
    /// BTreeMap-backed (no `preserve_order` feature), so object keys serialize
    /// sorted; HashMaps pass through `to_value` into sorted maps.
    pub fn canonical_json(&self) -> String {
        let v = serde_json::to_value(self).expect("PluginSchema serializes cleanly");
        serde_json::to_string(&v).expect("canonical value serializes")
    }

    /// sha256 over `canonical_json` — the blob identity hash.
    pub fn schema_hash(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(self.canonical_json().as_bytes());
        h.finalize().into()
    }

    pub fn schema_hash_hex(&self) -> String {
        hex(&self.schema_hash())
    }

    /// Render the state contract as a JSON Schema 2020-12 document — the shape
    /// consumers of `live-schema.json` (and now blob readers) see.
    pub fn to_json_schema(&self) -> Value {
        let mut props = serde_json::Map::new();
        let mut required = Vec::new();
        let mut names: Vec<&String> = self.fields.keys().collect();
        names.sort();
        for name in names {
            let f = &self.fields[name];
            if f.required {
                required.push(Value::String(name.clone()));
            }
            let mut node = field_to_json_schema(f);
            if let (Some(subid), Some(object)) = (self.subids.get(name), node.as_object_mut()) {
                object.insert("x-oscal-subid".into(), Value::String(subid.clone()));
            }
            props.insert(name.clone(), node);
        }
        let mut root = serde_json::Map::new();
        root.insert("$schema".into(), Value::String(self.dialect.clone()));
        root.insert("title".into(), Value::String(self.name.clone()));
        root.insert(
            "description".into(),
            Value::String(self.description.clone()),
        );
        root.insert("type".into(), Value::String("object".into()));
        root.insert("properties".into(), Value::Object(props));
        if !required.is_empty() {
            root.insert("required".into(), Value::Array(required));
        }
        if let Some(subid) = self.subids.get("__schema__") {
            root.insert("x-oscal-subid".into(), Value::String(subid.clone()));
        }
        if !self.immutable_paths.is_empty() {
            root.insert(
                "x-immutable-paths".into(),
                Value::Array(
                    self.immutable_paths
                        .iter()
                        .map(|p| Value::String(p.clone()))
                        .collect(),
                ),
            );
        }
        root.insert(
            "x-plugin-version".into(),
            Value::String(self.version.clone()),
        );
        let mut output = Value::Object(root);
        for (name, field) in &self.fields {
            scope_exact_export(
                &mut output,
                &field.field_type,
                &format!("/properties/{}", escape_pointer(name)),
            );
        }
        output
    }
}

pub struct PluginSchemaBuilder {
    name: String,
    version: String,
    description: String,
    methods: HashMap<String, MethodDecl>,
}

impl PluginSchemaBuilder {
    pub fn version(mut self, v: &str) -> Self {
        self.version = v.to_string();
        self
    }

    pub fn description(mut self, d: &str) -> Self {
        self.description = d.to_string();
        self
    }

    /// Task 0.2 convenience: chainable method registration.
    pub fn method(mut self, decl: MethodDecl) -> Self {
        self.methods.insert(decl.name.clone(), decl);
        self
    }

    pub fn build(self) -> PluginSchema {
        PluginSchema {
            name: self.name,
            version: self.version,
            description: self.description,
            fields: HashMap::new(),
            methods: self.methods,
            subids: HashMap::new(),
            immutable_paths: Vec::new(),
            dialect: DIALECT_2020_12.to_string(),
        }
    }
}

fn field_to_json_schema(f: &FieldSchema) -> Value {
    if let FieldType::JsonSchema(schema) = &f.field_type {
        return schema.as_ref().clone();
    }
    let mut node = type_to_json_schema(&f.field_type);
    let obj = node.as_object_mut().unwrap();
    if !f.description.is_empty() {
        obj.insert("description".into(), Value::String(f.description.clone()));
    }
    if let Some(d) = &f.default {
        obj.insert("default".into(), d.clone());
    }
    if let Some(e) = &f.example {
        obj.insert("examples".into(), Value::Array(vec![e.clone()]));
    }
    if f.read_only {
        obj.insert("readOnly".into(), Value::Bool(true));
    }
    for c in &f.constraints {
        match c {
            Constraint::Min { value } => {
                obj.insert("minimum".into(), serde_json::json!(value));
            }
            Constraint::Max { value } => {
                obj.insert("maximum".into(), serde_json::json!(value));
            }
            Constraint::Pattern { regex } => {
                obj.insert("pattern".into(), Value::String(regex.clone()));
            }
            _ => {}
        }
    }
    node
}

fn type_to_json_schema(t: &FieldType) -> Value {
    match t {
        FieldType::String => serde_json::json!({"type": "string"}),
        FieldType::Integer => serde_json::json!({"type": "integer"}),
        FieldType::Float => serde_json::json!({"type": "number"}),
        FieldType::Boolean => serde_json::json!({"type": "boolean"}),
        FieldType::JsonSchema(schema) => schema.as_ref().clone(),
        FieldType::Any => serde_json::json!({}),
        FieldType::Enum(vs) => serde_json::json!({"type": "string", "enum": vs}),
        FieldType::OneOf(branches) => {
            serde_json::json!({"oneOf": branches.iter().map(type_to_json_schema).collect::<Vec<_>>()})
        }
        FieldType::Map(inner) => {
            serde_json::json!({"type":"object", "additionalProperties": type_to_json_schema(inner)})
        }
        FieldType::Array(inner) => {
            serde_json::json!({"type": "array", "items": type_to_json_schema(inner)})
        }
        FieldType::Object(map) => {
            let mut props = serde_json::Map::new();
            let mut required = Vec::new();
            let mut names: Vec<&String> = map.keys().collect();
            names.sort();
            for name in names {
                let f = &map[name];
                if f.required {
                    required.push(Value::String(name.clone()));
                }
                props.insert(name.clone(), field_to_json_schema(f));
            }
            let mut node = serde_json::Map::new();
            node.insert("type".into(), Value::String("object".into()));
            node.insert("properties".into(), Value::Object(props));
            if !required.is_empty() {
                node.insert("required".into(), Value::Array(required));
            }
            Value::Object(node)
        }
    }
}

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn escape_pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}
fn scope_exact_export(output: &mut Value, kind: &FieldType, pointer: &str) {
    match kind {
        FieldType::JsonSchema(schema) => {
            if let Some(value) = output.pointer_mut(pointer) {
                *value = op_state_store::exact_schema::embed_exact_schema(schema, pointer);
            }
        }
        FieldType::Object(fields) => {
            for (name, field) in fields {
                scope_exact_export(
                    output,
                    &field.field_type,
                    &format!("{pointer}/properties/{}", escape_pointer(name)),
                )
            }
        }
        FieldType::Map(item) => {
            scope_exact_export(output, item, &format!("{pointer}/additionalProperties"))
        }
        FieldType::Array(item) => scope_exact_export(output, item, &format!("{pointer}/items")),
        FieldType::OneOf(items) => {
            for (index, item) in items.iter().enumerate() {
                scope_exact_export(output, item, &format!("{pointer}/oneOf/{index}"))
            }
        }
        _ => {}
    }
}
