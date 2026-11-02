//! Plugin catalog: the route source of truth.
//!
//! Accepted JSON shapes (all decode to the same catalog):
//! - `{ "plugins": [ <entry>, ... ] }`
//! - `{ "entries": [ <entry>, ... ] }`
//! - `[ <entry>, ... ]`
//!
//! Entry fields: `id` (alias `plugin_id`), `name`, `version`,
//! `description`, `dbus_path`, `interfaces`, `tags`, `schema_hash`,
//! `schema` (JSON Schema value), `methods` (`[{ "name": ... }]` or
//! plain strings).

use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("catalog JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("duplicate plugin id in catalog: {0}")]
    Duplicate(String),
}

/// One callable method declared by a plugin.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PluginMethod {
    pub name: String,
}

impl From<String> for PluginMethod {
    fn from(name: String) -> Self {
        Self { name }
    }
}

/// One plugin in the catalog.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct PluginEntry {
    #[serde(default, alias = "plugin_id")]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub dbus_path: String,
    #[serde(default)]
    pub interfaces: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Hash of this plugin's current schema; stamped on its frames.
    #[serde(default)]
    pub schema_hash: String,
    /// JSON Schema for the plugin, when the catalog carries it.
    #[serde(default)]
    pub schema: Option<serde_json::Value>,
    #[serde(default, deserialize_with = "de_methods")]
    pub methods: Vec<PluginMethod>,
}

fn de_methods<'de, D>(d: D) -> Result<Vec<PluginMethod>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum M {
        Named(PluginMethod),
        Plain(String),
    }
    Ok(Vec::<M>::deserialize(d)?
        .into_iter()
        .map(|m| match m {
            M::Named(p) => p,
            M::Plain(s) => PluginMethod::from(s),
        })
        .collect())
}

/// The whole plugin catalog.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    plugins: Vec<PluginEntry>,
    by_id: BTreeMap<String, usize>,
    hash: String,
}

impl Catalog {
    /// An empty catalog (hash of the empty pair set).
    pub fn empty() -> Self {
        Self::from_entries(Vec::new()).expect("empty catalog is valid")
    }

    /// Parse a catalog from any accepted JSON shape.
    pub fn from_json_str(text: &str) -> Result<Self, CatalogError> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        let entries = value
            .get("plugins")
            .or_else(|| value.get("entries"))
            .cloned()
            .unwrap_or(value);
        let plugins: Vec<PluginEntry> = serde_json::from_value(entries)?;
        Self::from_entries(plugins)
    }

    /// Build a catalog from entries, computing the catalog hash.
    pub fn from_entries(plugins: Vec<PluginEntry>) -> Result<Self, CatalogError> {
        let mut by_id = BTreeMap::new();
        for (i, p) in plugins.iter().enumerate() {
            if by_id.insert(p.id.clone(), i).is_some() {
                return Err(CatalogError::Duplicate(p.id.clone()));
            }
        }
        let mut pairs: Vec<String> = plugins
            .iter()
            .map(|p| format!("{}={}", p.id, p.schema_hash))
            .collect();
        pairs.sort();
        let hash = blake3::hash(pairs.join("\n").as_bytes()).to_hex().to_string();
        Ok(Self { plugins, by_id, hash })
    }

    /// blake3 over the sorted `(plugin_id, schema_hash)` pairs.
    pub fn hash(&self) -> &str {
        &self.hash
    }

    pub fn plugins(&self) -> &[PluginEntry] {
        &self.plugins
    }

    pub fn plugin(&self, id: &str) -> Option<&PluginEntry> {
        self.by_id.get(id).map(|&i| &self.plugins[i])
    }

    /// Does this plugin declare `method`?
    pub fn declares(&self, plugin_id: &str, method: &str) -> bool {
        self.plugin(plugin_id)
            .is_some_and(|p| p.methods.iter().any(|m| m.name == method))
    }
}
