//! Derive plugin-specific Rust types only from manifest-pinned sealed PluginSchema.
//! This is a generator, never a second schema authority or a backend transport.
#![allow(dead_code)]
#[path = "../src/model.rs"]
mod model;
#[path = "../src/source.rs"]
mod source;
#[path = "../src/store.rs"]
mod store;

use anyhow::{Context, Result, bail, ensure};
use model::{FieldSchema, FieldType, PluginSchema};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const MAX_GENERATED_BYTES: usize = 64 * 1024 * 1024;
const MAX_DEPTH: usize = 64;
const MAX_TYPES: usize = 16_384;

#[derive(Debug, Serialize)]
struct Issue {
    subject: String,
    reason: String,
}
#[derive(Debug, Serialize)]
struct PluginReport {
    schema_hash: String,
    state: bool,
    public_state: bool,
    methods: Vec<String>,
    unsupported: Vec<Issue>,
    dynamic_slots: Vec<String>,
}
#[derive(Debug, Serialize)]
struct Report {
    authority: &'static str,
    catalog_hash: String,
    catalog_generation: u64,
    status: &'static str,
    plugins: BTreeMap<String, PluginReport>,
}
struct Generated {
    rust: String,
    report: Report,
    use_example: Option<String>,
}

fn append(out: &mut String, text: &str) -> Result<()> {
    ensure!(
        out.len()
            .checked_add(text.len())
            .is_some_and(|size| size <= MAX_GENERATED_BYTES),
        "generated source exceeds 64 MiB"
    );
    out.try_reserve(text.len())
        .context("reserve generated source")?;
    out.push_str(text);
    Ok(())
}
fn literal(value: &str) -> String {
    format!("{value:?}")
}
fn ident(value: &str) -> String {
    // Every non-identifier byte is escaped, including underscore, so distinct
    // authored names cannot collide after sanitization. A prefix avoids keywords.
    let mut out = String::from("f_");
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() {
            out.push(char::from(byte));
        } else {
            write!(out, "_{byte:02x}").expect("String write");
        }
    }
    out
}
fn class(value: &str) -> String {
    let mut out = String::from("T");
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() && byte != b'X' {
            out.push(char::from(byte));
        } else {
            write!(out, "X{byte:02x}").expect("String write");
        }
    }
    out
}

struct Types<'a> {
    root: &'a Value,
    prefix: String,
    counter: usize,
    definitions: String,
    refs: BTreeSet<String>,
    scopes: Vec<Value>,
    dynamic: Vec<String>,
}
impl<'a> Types<'a> {
    fn new(root: &'a Value, prefix: &str) -> Self {
        Self {
            root,
            prefix: prefix.into(),
            counter: 0,
            definitions: String::new(),
            refs: BTreeSet::new(),
            scopes: Vec::new(),
            dynamic: Vec::new(),
        }
    }
    fn name(&mut self) -> Result<String> {
        self.counter += 1;
        ensure!(
            self.counter <= MAX_TYPES,
            "more than 16384 derived types in one declaration"
        );
        Ok(format!("{}Part{}", self.prefix, self.counter))
    }
    fn ty(&mut self, schema: &Value, depth: usize) -> Result<String> {
        let scoped = schema.get("$defs").is_some()
            || schema.get("definitions").is_some()
            || schema.get("$id").is_some();
        if scoped {
            self.scopes.push(schema.clone());
        }
        let result = self.ty_inner(schema, depth);
        if scoped {
            self.scopes.pop();
        }
        result
    }
    fn ty_inner(&mut self, schema: &Value, depth: usize) -> Result<String> {
        ensure!(depth <= MAX_DEPTH, "schema nesting exceeds 64");
        if schema == &Value::Bool(false) {
            bail!("unsatisfiable false schema has no inhabited static type");
        }
        if schema == &Value::Bool(true) {
            self.dynamic
                .push("explicit true schema retains arbitrary JSON".into());
            return Ok("serde_json::Value".into());
        }
        let object = schema
            .as_object()
            .context("schema is not an object or boolean")?;
        let allowed = [
            "$schema",
            "$id",
            "$defs",
            "definitions",
            "$ref",
            "title",
            "description",
            "default",
            "examples",
            "type",
            "properties",
            "required",
            "additionalProperties",
            "items",
            "enum",
            "const",
            "oneOf",
            "anyOf",
            "allOf",
            "minimum",
            "maximum",
            "exclusiveMinimum",
            "exclusiveMaximum",
            "multipleOf",
            "minLength",
            "maxLength",
            "pattern",
            "format",
            "minItems",
            "maxItems",
            "uniqueItems",
            "minProperties",
            "maxProperties",
            "readOnly",
            "writeOnly",
            "deprecated",
        ];
        for key in object.keys() {
            ensure!(
                allowed.contains(&key.as_str()) || key.starts_with("x-"),
                "unsupported/custom schema keyword {key}"
            );
        }
        if let Some(reference) = object.get("$ref") {
            let reference = reference.as_str().context("$ref is not a string")?;
            ensure!(
                reference.starts_with("#/"),
                "external/nonlocal reference {reference}"
            );
            ensure!(
                object.keys().all(|key| [
                    "$ref",
                    "$defs",
                    "definitions",
                    "title",
                    "description",
                    "default",
                    "examples",
                    "$schema",
                    "$id"
                ]
                .contains(&key.as_str())
                    || key.starts_with("x-")),
                "validation siblings of $ref are not specialized"
            );
            ensure!(
                self.refs.insert(reference.into()),
                "recursive reference {reference} requires unsupported recursive specialization"
            );
            let target = self
                .scopes
                .last()
                .unwrap_or(self.root)
                .pointer(&reference[1..])
                .with_context(|| format!("unresolved local reference {reference}"))?
                .clone();
            let result = self.ty(&target, depth + 1);
            self.refs.remove(reference);
            return result;
        }
        if object.contains_key("allOf") {
            bail!("allOf intersections are reported rather than inferred");
        }
        if let Some(branches) = object.get("oneOf") {
            ensure!(
                !object.contains_key("type")
                    && !object.contains_key("properties")
                    && !object.contains_key("anyOf"),
                "oneOf with structural siblings requires unsupported intersection specialization"
            );
            return self.sum(
                branches.as_array().context("oneOf is not an array")?,
                depth + 1,
            );
        }
        if let Some(branches) = object.get("anyOf") {
            let branches = branches.as_array().context("anyOf is not an array")?;
            ensure!(
                branches.len() == 2
                    && branches
                        .iter()
                        .filter(|branch| branch["type"] == "null")
                        .count()
                        == 1,
                "general anyOf is not an exactly-one sum; only nullable pairs are specialized"
            );
            let other = branches
                .iter()
                .find(|branch| branch["type"] != "null")
                .expect("nullable pair");
            let ty = self.ty(other, depth + 1)?;
            return Ok(format!("Nullable<{ty}>"));
        }
        if let Some(variants) = object.get("enum").and_then(Value::as_array) {
            ensure!(!variants.is_empty(), "enum has no variants");
            if variants.iter().all(Value::is_string) {
                let name = self.name()?;
                let mut body = String::new();
                let mut seen = BTreeSet::new();
                for (index, variant) in variants.iter().enumerate() {
                    let variant = variant.as_str().expect("string enum");
                    ensure!(seen.insert(variant), "duplicate enum value");
                    writeln!(
                        body,
                        "    #[serde(rename={})]\n    Variant{index},",
                        literal(variant)
                    )?;
                }
                append(
                    &mut self.definitions,
                    &format!(
                        "#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]\npub enum {name} {{\n{body}}}\n"
                    ),
                )?;
                return Ok(name);
            }
            self.dynamic.push("non-string enum retains declared scalar shape; exact membership is validated against sealed schema".into());
        }
        if let Some(types) = object.get("type").and_then(Value::as_array) {
            ensure!(
                types.len() == 2 && types.iter().filter(|ty| **ty == "null").count() == 1,
                "only a concrete type plus null is specialized from a type array"
            );
            let other = types
                .iter()
                .find(|ty| **ty != "null")
                .expect("nullable type")
                .clone();
            let mut schema = schema.clone();
            schema["type"] = other;
            let ty = self.ty(&schema, depth + 1)?;
            return Ok(format!("Nullable<{ty}>"));
        }
        let kind = object.get("type").and_then(Value::as_str).or_else(|| {
            object.get("const").and_then(|value| match value {
                Value::Null => Some("null"),
                Value::Bool(_) => Some("boolean"),
                Value::String(_) => Some("string"),
                Value::Number(n) if n.is_i64() || n.is_u64() => Some("integer"),
                Value::Number(_) => Some("number"),
                _ => None,
            })
        });
        match kind {
            Some("null") => Ok("Null".into()),
            Some("boolean") => Ok("bool".into()),
            Some("string") => Ok("String".into()),
            Some("integer") => Ok("Integer".into()),
            Some("number") => Ok("serde_json::Number".into()),
            Some("array") => {
                let items = object.get("items").cloned().unwrap_or(Value::Bool(true));
                ensure!(!items.is_array(), "tuple array items are not specialized");
                Ok(format!("Vec<{}>", self.ty(&items, depth + 1)?))
            }
            Some("object") => self.object(schema, depth + 1),
            None if object.is_empty()
                || object.keys().all(|key| {
                    [
                        "$schema",
                        "$id",
                        "$defs",
                        "definitions",
                        "title",
                        "description",
                        "default",
                        "examples",
                    ]
                    .contains(&key.as_str())
                        || key.starts_with("x-")
                }) =>
            {
                self.dynamic
                    .push("unconstrained authored schema retains arbitrary JSON".into());
                Ok("serde_json::Value".into())
            }
            None => bail!("schema lacks a concrete structural type; no type is inferred"),
            Some(other) => bail!("unsupported/custom type {other}"),
        }
    }
    fn object(&mut self, schema: &Value, depth: usize) -> Result<String> {
        let properties = schema
            .get("properties")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().context("required member is not a string"))
                    .collect::<Result<BTreeSet<_>>>()
            })
            .transpose()?
            .unwrap_or_default();
        ensure!(
            required.iter().all(|name| properties.contains_key(*name)),
            "required keys without declared properties are not inferred"
        );
        let additional = schema
            .get("additionalProperties")
            .cloned()
            .unwrap_or(Value::Bool(true));
        if properties.is_empty() && additional != Value::Bool(false) {
            return Ok(format!(
                "std::collections::BTreeMap<String,{}>",
                self.ty(&additional, depth + 1)?
            ));
        }
        let name = self.name()?;
        let mut body = String::new();
        for (key, property) in properties {
            let ty = self.ty(&property, depth + 1)?;
            if required.contains(key.as_str()) {
                writeln!(
                    body,
                    "    #[serde(rename={})]\n    pub {}: {ty},",
                    literal(&key),
                    ident(&key)
                )?;
            } else {
                writeln!(
                    body,
                    "    #[serde(rename={},default,skip_serializing_if=\"Presence::is_missing\")]\n    pub {}: Presence<{ty}>,",
                    literal(&key),
                    ident(&key)
                )?;
            }
        }
        let serde_attr = if additional == Value::Bool(false) {
            "#[serde(deny_unknown_fields)]\n"
        } else {
            let ty = self.ty(&additional, depth + 1)?;
            writeln!(
                body,
                "    #[serde(flatten)]\n    pub extra: std::collections::BTreeMap<String,{ty}>,"
            )?;
            ""
        };
        append(
            &mut self.definitions,
            &format!(
                "#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]\n{serde_attr}pub struct {name} {{\n{body}}}\n"
            ),
        )?;
        Ok(name)
    }
}
impl Types<'_> {
    fn sum(&mut self, branches: &[Value], depth: usize) -> Result<String> {
        ensure!(!branches.is_empty(), "oneOf has no branches");
        let name = self.name()?;
        let mut variants = String::new();
        let mut serialize = String::new();
        let mut deserialize = String::new();
        let mut documents = Vec::new();
        for (index, branch) in branches.iter().enumerate() {
            let ty = self.ty(branch, depth + 1)?;
            let mut document = branch.clone();
            for key in ["$defs", "definitions"] {
                if document.is_object()
                    && let Some(definitions) = self.scopes.last().unwrap_or(self.root).get(key)
                {
                    document[key] = definitions.clone();
                }
            }
            let document = serde_json::to_string(&document)?;
            documents.push(literal(&document));
            writeln!(variants, "    Branch{index}({ty}),")?;
            writeln!(
                serialize,
                "            Self::Branch{index}(value) => ({index}, to_json(value, DOCUMENT_LIMIT).map_err(serde::ser::Error::custom)?),"
            )?;
            writeln!(
                deserialize,
                "            {index} => serde_json::from_value(value).map(Self::Branch{index}).map_err(serde::de::Error::custom),"
            )?;
        }
        let docs = documents.join(",");
        append(
            &mut self.definitions,
            &format!(
                r#"
#[derive(Debug, Clone, PartialEq)]
pub enum {name} {{
{variants}}}
impl serde::Serialize for {name} {{
    fn serialize<S:serde::Serializer>(&self, serializer:S)->Result<S::Ok,S::Error> {{
        let (selected,value)=match self {{
{serialize}        }};
        let actual=sum_choice(&value,&[{docs}]).map_err(serde::ser::Error::custom)?;
        if actual!=selected {{ return Err(serde::ser::Error::custom("oneOf variant does not match its declared branch")); }}
        serde::Serialize::serialize(&value,serializer)
    }}
}}
impl<'de> serde::Deserialize<'de> for {name} {{
    fn deserialize<D:serde::Deserializer<'de>>(deserializer:D)->Result<Self,D::Error> {{
        let value=<serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
        match sum_choice(&value,&[{docs}]).map_err(serde::de::Error::custom)? {{
{deserialize}            _=>Err(serde::de::Error::custom("unreachable oneOf branch")),
        }}
    }}
}}
"#
            ),
        )?;
        Ok(name)
    }
}

fn state_branch_needs_context(ty: &FieldType) -> bool {
    match ty {
        // JSON Schema integer equality admits 0.0 while the source FieldType
        // integer accepts only i64/u64 tokens. Do not misclassify a state sum.
        FieldType::Integer => true,
        FieldType::Object(fields) => fields.values().any(|field| {
            !field.constraints.is_empty() || state_branch_needs_context(&field.field_type)
        }),
        FieldType::Array(item) | FieldType::Map(item) => state_branch_needs_context(item),
        FieldType::OneOf(branches) => branches.iter().any(state_branch_needs_context),
        _ => false,
    }
}

fn field_schema(field: &FieldSchema, public: bool) -> Value {
    let mut schema = field_type(&field.field_type, public);
    // The PluginSchema's own validator owns constraints and public policy.
    // This shape mapping never duplicates conditional/read-only semantics.
    if !field.required
        && schema != Value::Bool(true)
        && schema["type"] != "null"
        && !schema["type"]
            .as_array()
            .is_some_and(|types| types.iter().any(|ty| ty == "null"))
    {
        schema = json!({"anyOf":[schema,{"type":"null"}]});
    }
    schema
}
fn field_type(ty: &FieldType, public: bool) -> Value {
    match ty {
        FieldType::String => json!({"type":"string"}),
        FieldType::Integer => json!({"type":"integer"}),
        FieldType::Float => json!({"type":"number"}),
        FieldType::Boolean => json!({"type":"boolean"}),
        FieldType::Array(item) => json!({"type":"array","items":field_type(item,public)}),
        FieldType::Map(item) => {
            json!({"type":"object","additionalProperties":field_type(item,public)})
        }
        FieldType::JsonSchema(schema) => (**schema).clone(),
        FieldType::Enum(variants) => json!({"type":"string","enum":variants}),
        FieldType::OneOf(branches) if branches.iter().any(state_branch_needs_context) => {
            json!({"type":"unsupported_context_dependent_state_one_of"})
        }
        FieldType::OneOf(branches) => {
            json!({"oneOf":branches.iter().map(|branch|field_type(branch,public)).collect::<Vec<_>>()})
        }
        FieldType::Any => Value::Bool(true),
        FieldType::Object(fields) => json!({
            "type":"object",
            "properties":fields.iter().map(|(key,field)|(key.clone(),field_schema(field,public))).collect::<BTreeMap<_,_>>(),
            "required":if public { Vec::<String>::new() } else { fields.iter().filter(|(_,field)|field.required).map(|(key,_)|key.clone()).collect() },
            "additionalProperties":true
        }),
    }
}

fn public_shape(schema: &PluginSchema) -> Result<Value> {
    let paths = schema
        .public_state
        .get("paths")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let paths = paths
        .iter()
        .map(|path| -> Result<Vec<String>> {
            let path = path
                .as_str()
                .context("public policy path is not a string")?;
            ensure!(
                path.starts_with('/'),
                "public policy path must be an authored JSON pointer"
            );
            Ok(path[1..]
                .split('/')
                .map(|part| part.replace("~1", "/").replace("~0", "~"))
                .collect())
        })
        .collect::<Result<Vec<_>>>()?;
    selected_shape(&FieldType::Object(schema.fields.clone()), &paths)
}
fn selected_shape(ty: &FieldType, paths: &[Vec<String>]) -> Result<Value> {
    if paths.iter().any(Vec::is_empty) {
        return Ok(field_type(ty, false));
    }
    let FieldType::Object(fields) = ty else {
        bail!(
            "nested public policy through map/array/custom schema requires unsupported specialization; no partial shape is inferred"
        );
    };
    let mut grouped = BTreeMap::<String, Vec<Vec<String>>>::new();
    for path in paths {
        let (head, tail) = path.split_first().context("empty policy branch")?;
        grouped.entry(head.clone()).or_default().push(tail.to_vec());
    }
    let mut properties = BTreeMap::new();
    let mut required = Vec::new();
    for (name, paths) in grouped {
        let field = fields
            .get(&name)
            .with_context(|| format!("public path member {name:?} has no declared field"))?;
        let mut shape = selected_shape(&field.field_type, &paths)?;
        if field.required {
            required.push(name.clone());
        } else if shape != Value::Bool(true)
            && shape["type"] != "null"
            && !shape["type"]
                .as_array()
                .is_some_and(|types| types.iter().any(|ty| ty == "null"))
        {
            shape = json!({"anyOf":[shape,{"type":"null"}]});
        }
        properties.insert(name, shape);
    }
    Ok(
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
    )
}

const SUPPORT: &str = r#"
use crate::typed_runtime::{NodeError,TypedFilesystem};
use crate::typed_bindings::{PluginId,MethodId,SchemaHash};
const ARGUMENT_LIMIT:usize=1024*1024;
const DOCUMENT_LIMIT:usize=16*1024*1024;
#[derive(Debug,Clone,PartialEq,serde::Serialize,serde::Deserialize)]
#[serde(transparent)]
pub struct Null(pub ());
#[derive(Debug,Clone,PartialEq,serde::Serialize,serde::Deserialize)]
#[serde(untagged)]
pub enum Nullable<T> { Null(Null),Value(T) }
#[derive(Debug,Clone,PartialEq)]
pub enum Presence<T> { Missing,Present(T) }
impl<T> Default for Presence<T> { fn default()->Self { Self::Missing } }
impl<T> Presence<T> { pub fn is_missing(&self)->bool { matches!(self,Self::Missing) } }
impl<T:serde::Serialize> serde::Serialize for Presence<T> {
    fn serialize<S:serde::Serializer>(&self,serializer:S)->Result<S::Ok,S::Error> {
        match self { Self::Present(value)=>value.serialize(serializer),Self::Missing=>Err(serde::ser::Error::custom("Missing may only occur in a skipped optional field")) }
    }
}
impl<'de,T:serde::Deserialize<'de>> serde::Deserialize<'de> for Presence<T> {
    fn deserialize<D:serde::Deserializer<'de>>(deserializer:D)->Result<Self,D::Error> { T::deserialize(deserializer).map(Self::Present) }
}
/// Lossless integer representation, including canonical JSON integers beyond i64.
/// The sealed schema decides numeric bounds; no float conversion is stored.
#[derive(Debug,Clone,PartialEq,serde::Serialize,serde::Deserialize)]
#[serde(transparent)]
pub struct Integer(pub serde_json::Number);
impl Integer {
    pub fn from_i64(value:i64)->Self { Self(value.into()) }
    pub fn from_u64(value:u64)->Self { Self(value.into()) }
}
fn schema_validator(document:&str)->Result<jsonschema::Validator,NodeError> {
    let schema:serde_json::Value=serde_json::from_str(document).map_err(NodeError::decode)?;
    jsonschema::Validator::new(&schema).map_err(|error|NodeError::Schema(error.to_string()))
}
fn validate_document(document:&str,value:&serde_json::Value)->Result<(),NodeError> {
    schema_validator(document)?.validate(value).map_err(|error|NodeError::Schema(error.to_string()))
}
fn sum_choice(value:&serde_json::Value,documents:&[&str])->Result<usize,NodeError> {
    let mut selected=None;
    for (index,document) in documents.iter().enumerate() {
        if schema_validator(document)?.is_valid(value) {
            if selected.is_some() { return Err(NodeError::Schema("oneOf matched more than one branch".into())); }
            selected=Some(index);
        }
    }
    selected.ok_or_else(||NodeError::Schema("oneOf matched no branch".into()))
}
fn validate_state(document:&str,value:&serde_json::Value,public:bool)->Result<(),NodeError> {
    let schema:crate::model::PluginSchema=serde_json::from_str(document).map_err(NodeError::decode)?;
    let result=if public { schema.validate_public_value(value) } else { schema.validate_value(value,None) };
    result.map_err(|error|NodeError::Schema(error.to_string()))
}
fn to_json(value:&impl serde::Serialize,maximum:usize)->Result<serde_json::Value,NodeError> {
    struct Bound(usize);
    impl std::io::Write for Bound {
        fn write(&mut self,bytes:&[u8])->std::io::Result<usize> {
            self.0=self.0.checked_sub(bytes.len()).ok_or_else(||std::io::Error::other("typed document exceeds byte bound"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self)->std::io::Result<()> { Ok(()) }
    }
    serde_json::to_writer(Bound(maximum),value).map_err(NodeError::decode)?;
    serde_json::to_value(value).map_err(NodeError::decode)
}
"#;

fn wrapper(
    name: &str,
    shape: &Value,
    validation: &str,
    maximum: &str,
) -> Result<(String, String, Vec<String>)> {
    let mut types = Types::new(shape, &format!("{name}Value"));
    let ty = types.ty(shape, 0)?;
    let body = format!(
        r#"
#[derive(Debug,Clone,PartialEq,serde::Serialize)]
#[serde(transparent)]
pub struct {name}({ty});
impl {name} {{
    pub fn new(value:{ty})->Result<Self,NodeError> {{
        let raw=to_json(&value,{maximum})?;
        {validation}
        Ok(Self(value))
    }}
    pub fn get(&self)->&{ty} {{ &self.0 }}
    pub fn into_inner(self)->{ty} {{ self.0 }}
    pub fn validate(&self)->Result<(),NodeError> {{
        let raw=to_json(&self.0,{maximum})?;
        {validation}
        Ok(())
    }}
}}
impl<'de> serde::Deserialize<'de> for {name} {{
    fn deserialize<D:serde::Deserializer<'de>>(deserializer:D)->Result<Self,D::Error> {{
        let raw=<serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
        {validation_deserialize}
        let value=serde_json::from_value(raw).map_err(serde::de::Error::custom)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }}
}}
"#,
        validation_deserialize = validation.replace("?;", ".map_err(serde::de::Error::custom)?;")
    );
    append(&mut types.definitions, &body)?;
    Ok((types.definitions, ty, types.dynamic))
}

fn generate(
    snapshot: &source::SourceSnapshot,
    selection: &BTreeSet<String>,
    example_module: &Path,
) -> Result<Generated> {
    let mut rust = String::from(
        "// @generated by typed_specialize from manifest-pinned sealed PluginSchema.\n// Derived types are not a schema authority. Never edit them to change a plugin.\n#![allow(dead_code)]\n",
    );
    append(&mut rust, SUPPORT)?;
    let mut report = Report {
        authority: "manifest-pinned OPBLOB01 PluginSchema via SourceCatalog::capture_schema_only",
        catalog_hash: snapshot.catalog_hash.clone(),
        catalog_generation: snapshot.catalog_generation,
        status: "generated source; compilation/runtime verification is separate",
        plugins: BTreeMap::new(),
    };
    let mut example = None;
    for (plugin_id, object) in &snapshot.objects {
        if !selection.is_empty() && !selection.contains(plugin_id) {
            continue;
        }
        ensure!(
            object.plugin_id == *plugin_id && object.schema.name == *plugin_id,
            "sealed plugin identity mismatch"
        );
        let schema = &object.schema;
        let module = format!("plugin_{}", ident(plugin_id));
        let mut body = String::new();
        let mut methods = String::new();
        let mut entry = PluginReport {
            schema_hash: object.source_schema_hash.clone(),
            state: false,
            public_state: false,
            methods: Vec::new(),
            unsupported: Vec::new(),
            dynamic_slots: Vec::new(),
        };
        let document = literal(&serde_json::to_string(schema)?);
        for (name, public) in [("State", false), ("PublicState", true)] {
            let shape = if public {
                match public_shape(schema) {
                    Ok(shape) => shape,
                    Err(error) => {
                        entry.unsupported.push(Issue {
                            subject: name.into(),
                            reason: error.to_string(),
                        });
                        continue;
                    }
                }
            } else {
                field_type(&FieldType::Object(schema.fields.clone()), false)
            };
            let validation = format!("validate_state({document},&raw,{public})?;");
            match wrapper(name, &shape, &validation, "DOCUMENT_LIMIT") {
                Ok((definitions, _, dynamic)) => {
                    append(&mut body, &definitions)?;
                    entry.dynamic_slots.extend(
                        dynamic
                            .into_iter()
                            .map(|detail| format!("{name}: {detail}")),
                    );
                    if public {
                        entry.public_state = true;
                    } else {
                        entry.state = true;
                    }
                }
                Err(error) => entry.unsupported.push(Issue {
                    subject: name.into(),
                    reason: error.to_string(),
                }),
            }
        }
        if entry.public_state {
            append(
                &mut methods,
                "    pub fn public_state(&self)->Result<PublicState,NodeError> { self.filesystem.plugin_state_typed(PluginId::new(PLUGIN_ID)?,SchemaHash::new(SCHEMA_HASH)?) }\n",
            )?;
        }
        let mut example_shapes = BTreeMap::new();
        for (method_id, method) in &schema.methods {
            let Some(result) = &method.returns else {
                entry.unsupported.push(Issue {subject:method_id.clone(),reason:"missing sealed result contract; no binding or effect admission is generated".into()});
                continue;
            };
            let prefix = class(method_id);
            let args_name = format!("{prefix}Args");
            let result_name = format!("{prefix}Result");
            let args_validation = format!(
                "validate_document({},&raw)?;",
                literal(&serde_json::to_string(&method.args)?)
            );
            let result_validation = format!(
                "validate_document({},&raw)?;",
                literal(&serde_json::to_string(result)?)
            );
            let derived = || -> Result<_> {
                jsonschema::Validator::new(&method.args)
                    .context("invalid sealed arguments schema")?;
                jsonschema::Validator::new(result).context("invalid sealed result schema")?;
                let args = wrapper(&args_name, &method.args, &args_validation, "ARGUMENT_LIMIT")?;
                let returns = wrapper(&result_name, result, &result_validation, "DOCUMENT_LIMIT")?;
                Ok((args, returns))
            };
            match derived() {
                Ok(((args, args_shape, args_dynamic), (returns, _, result_dynamic))) => {
                    append(&mut body, &args)?;
                    append(&mut body, &returns)?;
                    entry.dynamic_slots.extend(
                        args_dynamic
                            .into_iter()
                            .chain(result_dynamic)
                            .map(|detail| format!("{method_id}: {detail}")),
                    );
                    let mutation = method.side_effect != model::SideEffect::Read;
                    let parameter = if mutation {
                        ",confirm_side_effects:bool"
                    } else {
                        ""
                    };
                    let confirm = if mutation {
                        "confirm_side_effects"
                    } else {
                        "false"
                    };
                    append(
                        &mut methods,
                        &format!(
                            "    pub fn {}(&self,args:&{args_name}{parameter})->Result<{result_name},NodeError> {{ args.validate()?; self.filesystem.invoke_typed(PluginId::new(PLUGIN_ID)?,MethodId::new({})?,SchemaHash::new(SCHEMA_HASH)?,args,{confirm}) }}\n",
                            ident(method_id),
                            literal(method_id)
                        ),
                    )?;
                    entry.methods.push(method_id.clone());
                    example_shapes.insert(
                        method_id.clone(),
                        (args_name, args_shape, result_name, mutation),
                    );
                }
                Err(error) => entry.unsupported.push(Issue {
                    subject: method_id.clone(),
                    reason: error.to_string(),
                }),
            }
        }
        append(
            &mut rust,
            &format!(
                "\npub mod {module} {{\nuse super::*;\npub const PLUGIN_ID:&str={};\npub const SCHEMA_HASH:&str={};\n{body}\npub struct Plugin<'a> {{ filesystem:&'a TypedFilesystem }}\nimpl<'a> Plugin<'a> {{\n    pub fn bind(filesystem:&'a TypedFilesystem)->Result<Self,NodeError> {{ filesystem.verify_schema_pin(&PluginId::new(PLUGIN_ID)?,&SchemaHash::new(SCHEMA_HASH)?)?; Ok(Self {{filesystem}}) }}\n{methods}}}\n}}\n",
                literal(plugin_id),
                literal(&object.source_schema_hash)
            ),
        )?;
        if plugin_id == "threetched_fs"
            && let (Some(protocol), Some(receipt)) = (
                example_shapes.get("get_protocol"),
                example_shapes.get("get_receipt"),
            )
        {
            ensure!(
                !protocol.3 && !receipt.3,
                "fixture operations must be sealed read operations"
            );
            example = Some(usage_example(example_module, &module, protocol, receipt)?);
        }
        report.plugins.insert(plugin_id.clone(), entry);
    }
    for selected in selection {
        ensure!(
            report.plugins.contains_key(selected),
            "selected plugin {selected} is absent from pinned sealed catalog"
        );
    }
    Ok(Generated {
        rust,
        report,
        use_example: example,
    })
}

fn usage_example(
    generated: &Path,
    module: &str,
    protocol: &(String, String, String, bool),
    receipt: &(String, String, String, bool),
) -> Result<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut example = String::from(
        "//! Generated typed read example; uses only the identity's existing mounted filesystem.\n#![allow(dead_code)]\n",
    );
    for name in [
        "dispatch",
        "fuse_buffers",
        "fuse_process",
        "model",
        "mounted",
        "namespace",
        "source",
        "store",
        "typed_bindings",
        "typed_runtime",
    ] {
        let path = root.join("src").join(format!("{name}.rs"));
        writeln!(
            example,
            "#[path={}] mod {name};",
            literal(path.to_str().context("source path is not UTF-8")?)
        )?;
    }
    writeln!(
        example,
        "#[path={}] mod threetched_surface;",
        literal(
            root.join("src/threetched-surface.rs")
                .to_str()
                .context("surface source path is not UTF-8")?
        )
    )?;
    writeln!(
        example,
        "#[path={}] mod plugins;",
        literal(generated.to_str().context("generated path is not UTF-8")?)
    )?;
    append(
        &mut example,
        &format!(
            r#"
use anyhow::{{Context,Result}};
use typed_bindings::{{MountRoot,SessionId}};
use typed_runtime::TypedFilesystem;
use plugins::{module}::*;
fn main()->Result<()> {{
    let mut args=std::env::args().skip(1);
    let root=MountRoot::new(args.next().context("mount root required")?)?;
    let session=SessionId::new(args.next().context("declared host session required")?)?;
    let operation_id=args.next().context("operation UUID to recover required")?;
    let filesystem=TypedFilesystem::via_fuse(root,session)?;
    let plugin=Plugin::bind(&filesystem)?;
    let request={protocol_args}::new({protocol_shape} {{}})?;
    let protocol=plugin.{protocol_method}(&request)?;
    println!("{{:?}}",protocol.get());
    let request={receipt_args}::new({receipt_shape} {{ {operation_field}:operation_id }})?;
    let receipt=plugin.{receipt_method}(&request)?;
    println!("{{:?}}",receipt.get());
    Ok(())
}}
"#,
            protocol_args = protocol.0,
            protocol_shape = protocol.1,
            protocol_method = ident("get_protocol"),
            receipt_args = receipt.0,
            receipt_shape = receipt.1,
            receipt_method = ident("get_receipt"),
            operation_field = ident("operation_id")
        ),
    )?;
    Ok(example)
}
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("output has no parent directory")?;
    std::fs::create_dir_all(parent)?;
    let staged = parent.join(format!(".typed-specialize-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        std::fs::write(&staged, bytes)?;
        std::fs::rename(&staged, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(staged);
    }
    result
}
fn absolute(path: PathBuf) -> Result<PathBuf> {
    Ok(if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    })
}
fn main() -> Result<()> {
    let mut catalog = PathBuf::from(source::DEFAULT_CATALOG_DIR);
    let mut output = PathBuf::from("crates/threetched-fs/generated/plugin_types.rs");
    let mut report = PathBuf::from("crates/threetched-fs/docs/typed-specialization-report.json");
    let mut example = None;
    let mut plugins = BTreeSet::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--catalog" => catalog = args.next().context("--catalog requires a path")?.into(),
            "--out-file" => output = args.next().context("--out-file requires a path")?.into(),
            "--report" => report = args.next().context("--report requires a path")?.into(),
            "--use-example" => {
                example = Some(PathBuf::from(
                    args.next().context("--use-example requires a path")?,
                ))
            }
            "--plugin" => {
                plugins.insert(
                    args.next()
                        .context("--plugin requires a sealed plugin id")?,
                );
            }
            _ => bail!(
                "unknown argument {arg}; expected --catalog/--out-file/--report/--use-example/--plugin"
            ),
        }
    }
    let output = absolute(output)?;
    let snapshot =
        source::SourceCatalog::new(catalog, source::DEFAULT_STATE_DIR).capture_schema_only()?;
    let generated = generate(&snapshot, &plugins, &output)?;
    write_atomic(&output, generated.rust.as_bytes())?;
    write_atomic(
        &absolute(report)?,
        &store::render_document(&generated.report)?,
    )?;
    if example.is_some() {
        ensure!(
            generated.use_example.is_some(),
            "sealed threetched_fs get_protocol/get_receipt cannot both be specialized; consult emitted report"
        );
    }
    if let Some(path) = example {
        write_atomic(
            &absolute(path)?,
            generated
                .use_example
                .as_ref()
                .expect("checked example")
                .as_bytes(),
        )?;
    }
    println!(
        "Wrote sealed-derived Rust types and explicit specialization report; compilation and mounted verification have not run."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(schema: PluginSchema) -> source::SourceSnapshot {
        let plugin = schema.name.clone();
        let object =
            model::TypedObjectBlob::schema_only(plugin.clone(), "a".repeat(64), schema).unwrap();
        source::SourceSnapshot {
            catalog_hash: "test-only".into(),
            catalog_generation: 1,
            objects: BTreeMap::from([(plugin, object)]),
        }
    }
    fn read(args: Value, result: Option<Value>) -> model::MethodDecl {
        model::MethodDecl {
            name: "read".into(),
            args,
            returns: result,
            side_effect: model::SideEffect::Read,
            idempotent: true,
            required_capability: Some("cap.test.read@v1".into()),
            subid: "obs.test.read@v1".into(),
        }
    }
    fn fixture(methods: BTreeMap<String, model::MethodDecl>) -> PluginSchema {
        PluginSchema {
            name: "demo".into(),
            version: "1".into(),
            methods,
            ..Default::default()
        }
    }
    #[test]
    fn missing_null_presence_maps_and_enums_are_preserved() {
        let schema = json!({"type":"object","properties":{
            "required":{"type":["string","null"]},
            "optional":{"type":"string"},
            "nullable_optional":{"anyOf":[{"type":"string"},{"type":"null"}]},
            "map":{"type":"object","additionalProperties":{"type":"array","items":{"enum":["a","b"]}}}
        },"required":["required","map"],"additionalProperties":false});
        let (rust, _, _) = wrapper(
            "Arguments",
            &schema,
            &format!("validate_document({},&raw)?;", literal(&schema.to_string())),
            "ARGUMENT_LIMIT",
        )
        .unwrap();
        assert!(rust.contains("Presence<String>"));
        assert!(rust.contains("Presence<Nullable<String>>"));
        assert!(rust.contains("pub f_required: Nullable<String>"));
        assert!(rust.contains("BTreeMap<String,Vec<"));
        assert!(rust.contains("deny_unknown_fields"));
        assert!(rust.contains("serde::de::Error::custom"));
    }
    #[test]
    fn one_of_is_an_exact_sum_on_both_decode_and_encode() {
        let schema = json!({"oneOf":[{"const":"first"},{"const":"second"}]});
        let mut types = Types::new(&schema, "Choice");
        types.ty(&schema, 0).unwrap();
        assert!(types.definitions.contains("sum_choice(&value"));
        assert!(types.definitions.contains("actual!=selected"));
        assert!(!types.definitions.contains("#[serde(untagged)]"));
        assert!(SUPPORT.contains("oneOf matched more than one branch"));
        let ambiguous = json!({"oneOf":[{"type":"integer"},{"type":"number"}]});
        let validator = jsonschema::Validator::new(&ambiguous).unwrap();
        assert!(!validator.is_valid(&json!(2)));
    }
    #[test]
    fn local_refs_are_resolved_but_recursive_external_and_custom_cases_report() {
        let supported = json!({"$defs":{"Value":{"type":"string"}},"type":"array","items":{"$ref":"#/$defs/Value"}});
        assert_eq!(
            Types::new(&supported, "Ref").ty(&supported, 0).unwrap(),
            "Vec<String>"
        );
        let recursive = json!({"$defs":{"Loop":{"type":"array","items":{"$ref":"#/$defs/Loop"}}},"$ref":"#/$defs/Loop"});
        assert!(Types::new(&recursive, "Ref").ty(&recursive, 0).is_err());
        for schema in [
            json!({"$ref":"https://example.invalid/schema"}),
            json!({"type":"object","unevaluatedProperties":false}),
            json!({"allOf":[{"type":"string"}]}),
        ] {
            assert!(Types::new(&schema, "Unsupported").ty(&schema, 0).is_err());
        }
    }

    #[test]
    fn embedded_local_definition_scope_is_used_for_types_and_sum_validation() {
        let schema = json!({"type":"object","additionalProperties":false,
            "properties":{"nested":{"$id":"urn:fixture:nested",
                "$defs":{"Choice":{"type":"string","const":"nested"}},
                "oneOf":[{"$ref":"#/$defs/Choice"},{"type":"integer"}]}},
            "required":["nested"]});
        let mut types = Types::new(&schema, "Nested");
        types.ty(&schema, 0).unwrap();
        assert!(types.definitions.contains("Branch0(String)"));
        assert!(types.definitions.contains("Branch1(Integer)"));
        assert!(types.definitions.contains("Choice"));
        assert!(types.definitions.contains("nested"));
        assert!(types.scopes.is_empty());
    }
    #[test]
    fn generation_is_deterministic_and_never_emits_missing_contract_methods() {
        let schema = fixture(BTreeMap::from([
            (
                "supported".into(),
                read(
                    json!({"type":"object","additionalProperties":false}),
                    Some(json!({"type":"integer","minimum":0})),
                ),
            ),
            ("missing".into(), read(json!({"type":"object"}), None)),
            (
                "custom".into(),
                read(
                    json!({"type":"object"}),
                    Some(json!({"type":"object","dependentSchemas":{"a":{"required":["b"]}}})),
                ),
            ),
        ]));
        let captured = snapshot(schema);
        let a = generate(&captured, &BTreeSet::new(), Path::new("/tmp/fixture.rs")).unwrap();
        let b = generate(&captured, &BTreeSet::new(), Path::new("/tmp/fixture.rs")).unwrap();
        assert_eq!(a.rust, b.rust);
        assert_eq!(
            serde_json::to_value(&a.report).unwrap(),
            serde_json::to_value(&b.report).unwrap()
        );
        assert_eq!(a.report.plugins["demo"].methods, ["supported"]);
        assert_eq!(a.report.plugins["demo"].unsupported.len(), 2);
        assert!(!a.rust.contains("pub fn f_missing"));
        assert!(a.rust.contains("verify_schema_pin"));
        assert!(a.rust.contains("invoke_typed"));
    }
    #[test]
    fn exact_numeric_bounds_stay_in_declaration_validation() {
        let schema = json!({"type":"integer","minimum":0,"maximum":18446744073709551615_u64});
        let validator = jsonschema::Validator::new(&schema).unwrap();
        assert!(validator.is_valid(&json!(18446744073709551615_u64)));
        assert!(validator.is_valid(&json!(0.0)));
        assert!(!validator.is_valid(&json!(-1)));
        let (rust, ty, _) = wrapper(
            "BoundedInteger",
            &schema,
            &format!("validate_document({},&raw)?;", literal(&schema.to_string())),
            "DOCUMENT_LIMIT",
        )
        .unwrap();
        assert_eq!(ty, "Integer");
        assert!(rust.contains("18446744073709551615"));
        assert!(!rust.contains("f64"));
    }
    #[test]
    fn public_state_has_no_required_private_peer_fields() {
        let field = FieldSchema {
            field_type: FieldType::String,
            required: true,
            description: String::new(),
            default: None,
            example: None,
            constraints: Vec::new(),
            read_only: false,
            read_only_when: None,
        };
        let schema = PluginSchema {
            name: "demo".into(),
            version: "1".into(),
            fields: BTreeMap::from([("public".into(), field.clone()), ("private".into(), field)]),
            public_state: json!({"paths":["/public"]}),
            ..Default::default()
        };
        let shape = public_shape(&schema).unwrap();
        assert_eq!(shape["required"], json!(["public"]));
        let (rust, _, _) = wrapper(
            "PublicState",
            &shape,
            "validate_state(\"{}\",&raw,true)?;",
            "DOCUMENT_LIMIT",
        )
        .unwrap();
        assert!(!rust.contains("f_private"));
        assert!(rust.contains("pub f_public: String"));
        assert!(rust.contains("validate_state"));
    }
    #[test]
    fn generated_read_usage_is_selected_from_declared_methods() {
        // Disposable shape fixtures exercise the emitter. The actual example
        // is emitted from live/persisted sealed capture, never these fixtures.
        let mut schema = fixture(BTreeMap::from([
            (
                "get_protocol".into(),
                read(
                    json!({"type":"object","additionalProperties":false}),
                    Some(json!({"type":"object","additionalProperties":true})),
                ),
            ),
            (
                "get_receipt".into(),
                read(
                    json!({"type":"object","properties":{"operation_id":{"type":"string"}},"required":["operation_id"],"additionalProperties":false}),
                    Some(json!({"type":"object","additionalProperties":true})),
                ),
            ),
        ]));
        schema.name = "threetched_fs".into();
        let generated = generate(
            &snapshot(schema),
            &BTreeSet::new(),
            Path::new("/tmp/plugin_types.rs"),
        )
        .unwrap();
        let example = generated.use_example.unwrap();
        assert!(example.contains("Plugin::bind"));
        assert!(example.contains("f_get_5fprotocol"));
        assert!(example.contains("f_get_5freceipt"));
        assert!(example.contains("mod fuse_buffers;"));
        assert!(example.contains("mod fuse_process;"));
        assert!(example.contains("mod threetched_surface;"));
        assert!(example.contains("threetched-surface.rs"));
        assert!(!example.contains("mod fuse_fs;"));
        assert!(!example.contains("read_to_end"));
        assert!(!example.contains("serde_json::from"));
        assert!(!example.contains("operations/"));
        // Canonical owner-owned recovery wire fields remain data, not an
        // inferred transport envelope or a copied private domain result.
        let recovery = op_state_store::method_receipt::MethodReceiptRecovery {
            operation_id: "00000000-0000-4000-8000-000000000001".into(),
            status: op_state_store::method_receipt::MethodRecoveryStatus::NotFound,
            plugin_id: None,
            method: None,
            attempt: None,
            terminal: None,
            result_digest: None,
        };
        assert_eq!(
            serde_json::to_value(recovery).unwrap()["status"],
            "not_found"
        );
    }
    #[test]
    fn identifiers_never_merge_distinct_authored_names() {
        assert_ne!(ident("a-b"), ident("a_2db"));
        assert_ne!(class("a-b"), class("aX2db"));
        assert_ne!(ident("type"), "type");
    }
}
