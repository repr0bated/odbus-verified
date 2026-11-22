//! zcall's only source of plugins, methods and argument flags is the
//! identity's mounted 3tchedFS namespace: a live, pinned mirror of the sealed
//! blobs (`view.json`, `plugins/<p>/schema.json`). zcall never reads the blob
//! catalog separately. Nothing here is hand-written per plugin.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use _3tched_fs::model::{MethodDecl, PluginSchema, SideEffect};
use _3tched_fs::mounted::MountedNamespace;
use anyhow::Result;
use serde_json::Value;

pub struct Catalog {
    path: PathBuf,
    namespace: MountedNamespace,
}

/// One sealed method, with its argument schema as plain JSON.
pub struct Method {
    pub name: String,
    pub args: Value,
    pub side_effect: &'static str,
    pub capability: Option<String>,
    pub subid: String,
}

/// One argument property, projected as a CLI flag.
#[derive(Clone)]
pub struct Flag {
    /// The property name in the method's args schema (the JSON key).
    pub key: String,
    /// The spelling typed on the command line.
    pub flag: String,
    pub schema: Value,
    pub required: bool,
}

impl Catalog {
    /// Select and open the mount (see `mount::select`); the same pinned
    /// namespace serves discovery, state reads and calls.
    pub fn open(mount: Option<&str>, identity_mode: Option<&str>) -> Result<Self> {
        let path = crate::mount::select(mount)?;
        let namespace = crate::mount::open(&path, identity_mode)?;
        Ok(Self { path, namespace })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn namespace(&self) -> &MountedNamespace {
        &self.namespace
    }

    /// Plugin ids from the mount's pinned `view.json`.
    pub fn plugins(&self) -> Vec<String> {
        self.namespace.plugins().unwrap_or_default()
    }

    /// The plugin's sealed schema as projected at `plugins/<p>/schema.json`.
    pub fn schema(&self, plugin: &str) -> Option<PluginSchema> {
        self.namespace.schema(plugin).ok()
    }

    /// Resolve a typed plugin name: exact, then `-`/`_` and case insensitive.
    pub fn resolve_plugin(&self, typed: &str) -> Option<String> {
        let plugins = self.plugins();
        if plugins.iter().any(|p| p == typed) {
            return Some(typed.to_owned());
        }
        let want = loose(typed);
        plugins.into_iter().find(|p| loose(p) == want)
    }

    pub fn methods(&self, plugin: &str) -> BTreeMap<String, Method> {
        self.schema(plugin)
            .map(|schema| {
                schema
                    .methods
                    .iter()
                    .map(|(name, decl)| (name.clone(), method(name, decl)))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn resolve_method(&self, plugin: &str, typed: &str) -> Option<Method> {
        let mut methods = self.methods(plugin);
        if let Some(found) = methods.remove(typed) {
            return Some(found);
        }
        let want = loose(typed).replace('_', "");
        let key = methods
            .keys()
            .find(|name| loose(name).replace('_', "") == want)
            .cloned()?;
        methods.remove(&key)
    }
}

fn method(name: &str, decl: &MethodDecl) -> Method {
    Method {
        name: name.to_owned(),
        args: decl.args.clone(),
        side_effect: match decl.side_effect {
            SideEffect::Read => "read",
            SideEffect::Mutation => "mutation",
        },
        capability: decl.required_capability.clone(),
        subid: decl.subid.clone(),
    }
}

/// `-` and `_` are interchangeable and case is ignored when resolving names.
pub fn loose(name: &str) -> String {
    name.replace('-', "_").to_lowercase()
}

impl Method {
    /// Every property of the args schema as a flag. Property names that are
    /// already CLI spellings (captured binaries use `--verbose`, `-v`) are kept
    /// verbatim; other names become `--<name>`.
    pub fn flags(&self) -> Vec<Flag> {
        let required: Vec<&str> = self.args["required"]
            .as_array()
            .map(|list| list.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut flags: Vec<Flag> = self.args["properties"]
            .as_object()
            .map(|props| {
                props
                    .iter()
                    .map(|(key, schema)| Flag {
                        key: key.clone(),
                        flag: if key.starts_with('-') {
                            key.clone()
                        } else {
                            format!("--{key}")
                        },
                        schema: schema.clone(),
                        required: required.contains(&key.as_str()),
                    })
                    .collect()
            })
            .unwrap_or_default();
        flags.sort_by(|a, b| b.required.cmp(&a.required).then(a.flag.cmp(&b.flag)));
        flags
    }

    pub fn find_flag(&self, typed: &str) -> Option<Flag> {
        let flags = self.flags();
        if let Some(flag) = flags.iter().find(|flag| flag.flag == typed) {
            return Some(flag.clone());
        }
        let want = loose(typed);
        flags.into_iter().find(|flag| loose(&flag.flag) == want)
    }

    pub fn summary(&self) -> String {
        let mut text = self.side_effect.to_owned();
        if let Some(capability) = &self.capability {
            text.push_str(" · ");
            text.push_str(capability);
        }
        text
    }
}

/// The JSON type a value schema expects, looking through `oneOf`/`anyOf` and
/// nullable type lists. `None` means the schema accepts any JSON.
pub fn value_type(schema: &Value) -> Option<String> {
    match &schema["type"] {
        Value::String(kind) => return Some(kind.clone()),
        Value::Array(kinds) => {
            if let Some(kind) = kinds.iter().filter_map(Value::as_str).find(|k| *k != "null") {
                return Some(kind.to_owned());
            }
        }
        _ => {}
    }
    if let Some(constant) = schema.get("const") {
        return Some(json_kind(constant).to_owned());
    }
    if schema.get("enum").is_some() {
        return Some("string".to_owned());
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(members) = schema[key].as_array() {
            if let Some(kind) = members.iter().find_map(value_type) {
                return Some(kind);
            }
        }
    }
    None
}

fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
        Value::Null => "null",
    }
}

/// The item schema of an array schema (looking through `oneOf`/`anyOf`).
pub fn item_schema(schema: &Value) -> Option<&Value> {
    if let Some(items) = schema.get("items").filter(|items| items.is_object()) {
        return Some(items);
    }
    if let Some(first) = schema["prefixItems"].as_array().and_then(|list| list.first()) {
        return Some(first);
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(found) = schema[key].as_array().and_then(|m| m.iter().find_map(item_schema)) {
            return Some(found);
        }
    }
    None
}

/// Every closed value a schema allows: `enum`, `const`, booleans, and the same
/// for array items.
pub fn closed_values(schema: &Value) -> Vec<String> {
    let mut values = Vec::new();
    collect_closed(schema, &mut values, 0);
    values.dedup();
    values
}

fn collect_closed(schema: &Value, out: &mut Vec<String>, depth: usize) {
    if depth > 6 {
        return;
    }
    if let Some(list) = schema["enum"].as_array() {
        out.extend(list.iter().filter(|v| !v.is_null()).map(scalar_text));
    }
    if let Some(constant) = schema.get("const").filter(|c| !c.is_boolean()) {
        out.push(scalar_text(constant));
    }
    if value_type(schema).as_deref() == Some("boolean") && schema.get("const").is_none() {
        out.extend(["true".to_owned(), "false".to_owned()]);
    }
    for key in ["oneOf", "anyOf"] {
        for member in schema[key].as_array().into_iter().flatten() {
            collect_closed(member, out, depth + 1);
        }
    }
    if let Some(items) = item_schema(schema) {
        if !std::ptr::eq(items, schema) {
            collect_closed(items, out, depth + 1);
        }
    }
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Short type label for completion descriptions, e.g. `string[]`, `enum`.
pub fn type_label(schema: &Value) -> String {
    let kind = value_type(schema).unwrap_or_else(|| "json".to_owned());
    if kind == "array" {
        let item = item_schema(schema)
            .and_then(value_type)
            .unwrap_or_else(|| "json".to_owned());
        return format!("{item}[]");
    }
    if schema.get("enum").is_some() {
        return "enum".to_owned();
    }
    kind
}

/// A boolean switch takes no value on the command line.
pub fn is_switch(schema: &Value) -> bool {
    value_type(schema).as_deref() == Some("boolean")
}
