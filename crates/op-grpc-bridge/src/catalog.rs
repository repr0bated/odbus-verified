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
use std::path::Path;

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
    /// Read one consistent manifest-pinned snapshot of the sealed plugin catalog.
    /// The catalog identity comes from the sealer's manifest, never a consumer hash.
    pub fn from_sealed(dir: &Path) -> anyhow::Result<Self> {
        let manifest_path = dir.join(op_blob::catalog::MANIFEST_FILENAME);
        let manifest_bytes = std::fs::read(&manifest_path)?;
        let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)?;
        let catalog_hash = manifest["catalog_hash"]
            .as_str()
            .filter(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| anyhow::anyhow!("invalid sealed catalog hash"))?
            .to_owned();
        let entries: BTreeMap<String, String> =
            serde_json::from_value(manifest["plugins"].clone())?;
        let mut plugins = Vec::with_capacity(entries.len());
        for (id, schema_hash) in entries {
            let schema = op_blob::catalog::read_plugin_state_store_schema(dir, &id)
                .ok_or_else(|| anyhow::anyhow!("invalid sealed plugin {id}"))?;
            anyhow::ensure!(
                schema_hash.len() == 64 && schema_hash.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid schema hash for {id}"
            );
            let bytes = std::fs::read(dir.join(format!("{id}.{}.blob", &schema_hash[..16])))?;
            let blob = op_blob::BlobRef::from_manifest_pinned(&bytes, &schema_hash)
                .map_err(|error| anyhow::anyhow!("invalid sealed plugin {id}: {error}"))?;
            let raw_schema = serde_json::from_str(blob.schema_json())?;
            let mut methods: Vec<_> = schema
                .methods
                .keys()
                .cloned()
                .map(PluginMethod::from)
                .collect();
            methods.sort_by(|a, b| a.name.cmp(&b.name));
            plugins.push(PluginEntry {
                id: id.clone(),
                name: schema.name.clone(),
                version: schema.version.clone(),
                description: schema.description.clone(),
                dbus_path: format!("/org/opdbus/v1/plugins/{id}"),
                tags: schema.tags.clone(),
                schema_hash,
                schema: Some(raw_schema),
                methods,
                ..Default::default()
            });
        }
        anyhow::ensure!(
            std::fs::read(manifest_path)? == manifest_bytes,
            "sealed catalog changed during read"
        );
        let by_id = plugins
            .iter()
            .enumerate()
            .map(|(index, plugin)| (plugin.id.clone(), index))
            .collect();
        Ok(Self {
            plugins,
            by_id,
            hash: catalog_hash,
        })
    }

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
        let hash = blake3::hash(pairs.join("\n").as_bytes())
            .to_hex()
            .to_string();
        Ok(Self {
            plugins,
            by_id,
            hash,
        })
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

#[cfg(test)]
mod sealed_tests {
    use super::*;

    #[test]
    fn sealed_catalog_uses_manifest_identity_and_declared_methods() {
        let dir = tempfile::tempdir().unwrap();
        let schema = op_state_store::PluginSchema::builder("fixture")
            .version("1")
            .method(op_state_store::MethodDecl {
                name: "Read".into(),
                args: simd_json::json!({"type": "object"}),
                returns: None,
                side_effect: op_state_store::SideEffect::Read,
                idempotent: true,
                required_capability: None,
                subid: "obs.service.fixture.read@v1".into(),
            })
            .build();
        let blob = op_blob::blobify_plugin_schema(&schema.name, schema.clone());
        op_blob::BlobStore::open(dir.path())
            .unwrap()
            .write(&blob)
            .unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(
            &std::fs::read(dir.path().join(op_blob::catalog::MANIFEST_FILENAME)).unwrap(),
        )
        .unwrap();

        let catalog = Catalog::from_sealed(dir.path()).unwrap();
        assert_eq!(catalog.hash(), manifest["catalog_hash"].as_str().unwrap());
        assert_eq!(catalog.plugins().len(), 1);
        let plugin = catalog.plugin("fixture").unwrap();
        assert_eq!(plugin.schema_hash, blob.manifest.schema_hash);
        assert_eq!(plugin.schema.as_ref().unwrap()["name"], "fixture");
        assert!(catalog.declares("fixture", "Read"));

        std::fs::remove_file(dir.path().join(op_blob::catalog::MANIFEST_FILENAME)).unwrap();
        assert!(Catalog::from_sealed(dir.path()).is_err());
    }
}
