//! Read-only MCP tools over the sealed plugin-blob catalog.
//!
//! The catalog in `/dev/shm/opdbus/plugin-blobs` is the source of truth.
//! These tools read `.manifest.json` and blob sections. They never re-hash
//! and never write.

use crate::server::{ToolExecutor, ToolInfo};
use anyhow::{anyhow, Context, Result};
use op_blob::catalog::{read_manifest_plugin_ids, DEFAULT_SHM_DIR, MANIFEST_FILENAME};
use op_blob::BlobRef;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::path::{Path, PathBuf};

/// MCP tool executor that serves sealed blob schema/manifest/methods.
pub struct BlobSchemaExecutor {
    catalog_dir: PathBuf,
}

impl BlobSchemaExecutor {
    pub fn shm() -> Self {
        Self {
            catalog_dir: PathBuf::from(DEFAULT_SHM_DIR),
        }
    }

    pub fn with_dir(dir: impl Into<PathBuf>) -> Self {
        Self {
            catalog_dir: dir.into(),
        }
    }

    fn tool_defs() -> Vec<ToolInfo> {
        vec![
            ToolInfo {
                name: "blob_catalog".into(),
                description:
                    "List sealed plugin blobs: catalog_hash, generation, plugin id + schema_hash."
                        .into(),
                input_schema: json!({"type": "object", "properties": {}}),
                annotations: None,
            },
            ToolInfo {
                name: "blob_schema".into(),
                description:
                    "Return the sealed PluginSchema JSON for one plugin (section 1, as sealed)."
                        .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "plugin_id": {
                            "type": "string",
                            "description": "Plugin id, e.g. tched_router or unix_socket"
                        }
                    },
                    "required": ["plugin_id"]
                }),
                annotations: None,
            },
            ToolInfo {
                name: "blob_manifest".into(),
                description: "Return the sealed BlobManifest JSON (D-Bus/gRPC identity + methods)."
                    .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "plugin_id": {"type": "string"}
                    },
                    "required": ["plugin_id"]
                }),
                annotations: None,
            },
            ToolInfo {
                name: "blob_methods".into(),
                description:
                    "List declared methods for a plugin: name, capability, subid, side_effect."
                        .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "plugin_id": {"type": "string"}
                    },
                    "required": ["plugin_id"]
                }),
                annotations: None,
            },
            ToolInfo {
                name: "blob_search".into(),
                description:
                    "Search plugin ids, descriptions, and method names in the sealed catalog."
                        .into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": {"type": "string"},
                        "limit": {"type": "integer", "minimum": 1, "default": 20}
                    },
                    "required": ["query"]
                }),
                annotations: None,
            },
        ]
    }

    fn require_plugin_id(arguments: &Value) -> Result<String> {
        arguments
            .get("plugin_id")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow!("plugin_id is required"))
    }

    fn read_manifest_value(&self) -> Result<Value> {
        let path = self.catalog_dir.join(MANIFEST_FILENAME);
        let mut bytes = std::fs::read(&path)
            .with_context(|| format!("read catalog manifest {}", path.display()))?;
        simd_json::to_owned_value(&mut bytes).context("parse catalog .manifest.json")
    }

    fn find_blob_path(&self, plugin_id: &str) -> Result<PathBuf> {
        let prefix = format!("{plugin_id}.");
        let dir = std::fs::read_dir(&self.catalog_dir)
            .with_context(|| format!("read blob catalog dir {}", self.catalog_dir.display()))?;
        for entry in dir {
            let path = entry?.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if name.starts_with(&prefix) && name.ends_with(".blob") {
                return Ok(path);
            }
        }
        Err(anyhow!(
            "no sealed blob for plugin '{plugin_id}' in {}",
            self.catalog_dir.display()
        ))
    }

    fn load_blob_bytes(&self, plugin_id: &str) -> Result<Vec<u8>> {
        let path = self.find_blob_path(plugin_id)?;
        std::fs::read(&path).with_context(|| format!("read {}", path.display()))
    }

    fn parse_json_owned(raw: &str) -> Result<Value> {
        let mut buf = raw.as_bytes().to_vec();
        simd_json::to_owned_value(&mut buf).map_err(|e| anyhow!("json parse: {e}"))
    }

    fn catalog(&self) -> Result<Value> {
        self.read_manifest_value()
    }

    fn schema(&self, plugin_id: &str) -> Result<Value> {
        let bytes = self.load_blob_bytes(plugin_id)?;
        let blob = BlobRef::new(&bytes).map_err(|e| anyhow!("blob {plugin_id}: {e}"))?;
        let schema = Self::parse_json_owned(blob.schema_json())?;
        Ok(json!({
            "plugin_id": plugin_id,
            "schema_hash": blob.schema_hash_hex(),
            "schema": schema
        }))
    }

    fn manifest(&self, plugin_id: &str) -> Result<Value> {
        let bytes = self.load_blob_bytes(plugin_id)?;
        let blob = BlobRef::new(&bytes).map_err(|e| anyhow!("blob {plugin_id}: {e}"))?;
        let manifest = Self::parse_json_owned(blob.manifest_json())?;
        Ok(json!({
            "plugin_id": plugin_id,
            "schema_hash": blob.schema_hash_hex(),
            "manifest": manifest
        }))
    }

    fn methods(&self, plugin_id: &str) -> Result<Value> {
        let bytes = self.load_blob_bytes(plugin_id)?;
        let blob = BlobRef::new(&bytes).map_err(|e| anyhow!("blob {plugin_id}: {e}"))?;
        let schema = blob
            .state_store_schema()
            .map_err(|e| anyhow!("schema {plugin_id}: {e}"))?;
        let mut methods = Vec::new();
        let mut names: Vec<_> = schema.methods.keys().cloned().collect();
        names.sort();
        for name in names {
            let Some(method) = schema.methods.get(&name) else {
                continue;
            };
            methods.push(json!({
                "name": method.name,
                "subid": method.subid,
                "side_effect": format!("{:?}", method.side_effect).to_ascii_lowercase(),
                "idempotent": method.idempotent,
                "required_capability": method.required_capability,
                "args": method.args,
                "returns": method.returns
            }));
        }
        Ok(json!({
            "plugin_id": plugin_id,
            "schema_hash": blob.schema_hash_hex(),
            "method_count": methods.len(),
            "methods": methods
        }))
    }

    fn search(&self, query: &str, limit: usize) -> Result<Value> {
        let q = query.to_ascii_lowercase();
        let ids = read_manifest_plugin_ids(&self.catalog_dir).unwrap_or_default();
        let mut hits = Vec::new();
        for id in ids {
            if hits.len() >= limit {
                break;
            }
            let id_hit = id.to_ascii_lowercase().contains(&q);
            let bytes = match self.load_blob_bytes(&id) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let Ok(blob) = BlobRef::new(&bytes) else {
                continue;
            };
            let Ok(schema) = blob.state_store_schema() else {
                continue;
            };
            let desc_hit = schema.description.to_ascii_lowercase().contains(&q);
            let mut method_hits: Vec<String> = schema
                .methods
                .keys()
                .filter(|name| name.to_ascii_lowercase().contains(&q))
                .cloned()
                .collect();
            method_hits.sort();
            if id_hit || desc_hit || !method_hits.is_empty() {
                hits.push(json!({
                    "plugin_id": id,
                    "schema_hash": blob.schema_hash_hex(),
                    "description": schema.description,
                    "matched_methods": method_hits
                }));
            }
        }
        Ok(json!({
            "query": query,
            "count": hits.len(),
            "hits": hits
        }))
    }
}

#[async_trait::async_trait]
impl ToolExecutor for BlobSchemaExecutor {
    async fn list_tools(&self) -> Result<Vec<ToolInfo>> {
        Ok(Self::tool_defs())
    }

    async fn execute_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        match name {
            "blob_catalog" => self.catalog(),
            "blob_schema" => self.schema(&Self::require_plugin_id(&arguments)?),
            "blob_manifest" => self.manifest(&Self::require_plugin_id(&arguments)?),
            "blob_methods" => self.methods(&Self::require_plugin_id(&arguments)?),
            "blob_search" => {
                let query = arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                if query.is_empty() {
                    return Err(anyhow!("query is required"));
                }
                let limit = arguments
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(20)
                    .clamp(1, 200) as usize;
                self.search(query, limit)
            }
            other => Err(anyhow!("unknown blob-schema tool: {other}")),
        }
    }

    async fn get_tool_schema(&self, name: &str) -> Result<Option<Value>> {
        Ok(Self::tool_defs()
            .into_iter()
            .find(|t| t.name == name)
            .map(|t| t.input_schema))
    }

    async fn search_tools(&self, query: &str, limit: usize) -> Result<Vec<ToolInfo>> {
        let q = query.to_ascii_lowercase();
        Ok(Self::tool_defs()
            .into_iter()
            .filter(|t| {
                t.name.to_ascii_lowercase().contains(&q)
                    || t.description.to_ascii_lowercase().contains(&q)
            })
            .take(limit)
            .collect())
    }
}

/// Read sealed schema JSON for `blob://<plugin_id>` MCP resources.
pub fn read_schema_resource(dir: &Path, plugin_id: &str) -> Option<String> {
    let prefix = format!("{plugin_id}.");
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with(&prefix) && name.ends_with(".blob") {
            let bytes = std::fs::read(&path).ok()?;
            let blob = BlobRef::new(&bytes).ok()?;
            return Some(blob.schema_json().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn require_plugin_id_rejects_blank() {
        let args = json!({"plugin_id": "  "});
        assert!(BlobSchemaExecutor::require_plugin_id(&args).is_err());
    }

    #[tokio::test]
    async fn catalog_reads_manifest_without_rehash() {
        let dir = tempfile_dir();
        let manifest = r#"{
            "catalog_hash": "abc123",
            "generation": 7,
            "plugins": {"unix_socket": "deadbeef"}
        }"#;
        std::fs::write(dir.path().join(MANIFEST_FILENAME), manifest).unwrap();
        let exec = BlobSchemaExecutor::with_dir(dir.path());
        let value = exec.catalog().unwrap();
        assert_eq!(
            value.get("catalog_hash").and_then(|v| v.as_str()),
            Some("abc123")
        );
        assert_eq!(value.get("generation").and_then(|v| v.as_u64()), Some(7));
    }

    #[tokio::test]
    async fn missing_plugin_is_an_error() {
        let dir = tempfile_dir();
        std::fs::write(dir.path().join(MANIFEST_FILENAME), r#"{"plugins":{}}"#).unwrap();
        let exec = BlobSchemaExecutor::with_dir(dir.path());
        let err = exec.schema("nope").unwrap_err();
        assert!(err.to_string().contains("no sealed blob"), "{err}");
    }

    #[tokio::test]
    async fn lists_five_blob_tools() {
        let exec = BlobSchemaExecutor::with_dir("/tmp");
        let tools = exec.list_tools().await.unwrap();
        assert_eq!(tools.len(), 5);
        assert!(tools.iter().any(|t| t.name == "blob_schema"));
    }

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn tempfile_dir() -> TempDir {
        let path = std::env::temp_dir().join(format!(
            "op-mcp-blob-schema-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        TempDir { path }
    }
}
