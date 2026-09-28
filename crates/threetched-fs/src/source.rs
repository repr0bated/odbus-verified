use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model::{PluginSchema, TypedObjectBlob};

const MAGIC: &[u8; 8] = b"OPBLOB01";
const HEADER_LEN: usize = 64;
const SECTION_ENTRY_LEN: usize = 24;
const SECTION_SCHEMA_JSON: u32 = 1;

pub const DEFAULT_CATALOG_DIR: &str = "/dev/shm/opdbus/plugin-blobs";
pub const DEFAULT_STATE_DIR: &str = "/dev/shm/opdbus/state";

#[derive(Debug, Clone)]
pub struct SourceCatalog {
    catalog_dir: PathBuf,
    state_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SourceSnapshot {
    pub catalog_hash: String,
    pub catalog_generation: u64,
    pub objects: BTreeMap<String, TypedObjectBlob>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct CatalogManifest {
    catalog_hash: String,
    generation: u64,
    plugins: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct StateManifest {
    #[serde(default)]
    generation: u64,
}

impl SourceCatalog {
    pub fn new(catalog_dir: impl Into<PathBuf>, state_dir: impl Into<PathBuf>) -> Self {
        Self {
            catalog_dir: catalog_dir.into(),
            state_dir: state_dir.into(),
        }
    }

    pub fn capture(&self) -> Result<SourceSnapshot> {
        for _ in 0..4 {
            let catalog_before = self.read_catalog_manifest()?;
            let state_before = self.read_state_generation()?;
            let mut objects = BTreeMap::new();

            for (plugin_id, schema_hash) in &catalog_before.plugins {
                let schema = self.read_schema(plugin_id, schema_hash)?;
                let value = self.read_state(plugin_id, &schema);
                let object =
                    TypedObjectBlob::new(plugin_id.clone(), schema_hash.clone(), schema, value)
                        .with_context(|| format!("validate typed source object '{plugin_id}'"))?;
                objects.insert(plugin_id.clone(), object);
            }

            let catalog_after = self.read_catalog_manifest()?;
            let state_after = self.read_state_generation()?;
            if catalog_before == catalog_after && state_before == state_after {
                return Ok(SourceSnapshot {
                    catalog_hash: catalog_after.catalog_hash,
                    catalog_generation: catalog_after.generation,
                    objects,
                });
            }
        }

        bail!("SHM source changed during four capture attempts")
    }

    fn read_catalog_manifest(&self) -> Result<CatalogManifest> {
        read_json(self.catalog_dir.join(".manifest.json"))
    }

    fn read_state_generation(&self) -> Result<u64> {
        let path = self.state_dir.join(".manifest.json");
        if !path.exists() {
            return Ok(0);
        }
        Ok(read_json::<StateManifest>(path)?.generation)
    }

    /// Read a plugin's live state from `/dev/shm/opdbus/state/<plugin>.json`.
    /// Falls back to schema-derived defaults when the file is missing,
    /// unreadable, or does not validate against the sealed schema.
    fn read_state(&self, plugin_id: &str, schema: &PluginSchema) -> serde_json::Value {
        let path = self.state_dir.join(format!("{plugin_id}.json"));
        match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(value) => match schema.validate_value(&value, None) {
                    Ok(()) => value,
                    Err(e) => {
                        if std::env::var("THREETCHEDFS_DEBUG").is_ok() {
                            eprintln!("3tchedfs: state fallback for {plugin_id}: {e}");
                        }
                        schema.template()
                    }
                },
                Err(e) => {
                    if std::env::var("THREETCHEDFS_DEBUG").is_ok() {
                        eprintln!("3tchedfs: state parse error for {plugin_id}: {e}");
                    }
                    schema.template()
                }
            },
            Err(e) => {
                if std::env::var("THREETCHEDFS_DEBUG").is_ok() {
                    eprintln!("3tchedfs: state read error for {plugin_id}: {e}");
                }
                schema.template()
            }
        }
    }

    fn read_schema(&self, plugin_id: &str, expected_hash: &str) -> Result<PluginSchema> {
        if plugin_id.is_empty()
            || plugin_id.len() > 128
            || !plugin_id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            bail!("invalid canonical plugin id '{plugin_id}'");
        }
        if expected_hash.len() != 64
            || !expected_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            bail!("invalid schema hash for '{plugin_id}'");
        }
        let file = self
            .catalog_dir
            .join(format!("{plugin_id}.{}.blob", &expected_hash[..16]));
        let bytes =
            fs::read(&file).with_context(|| format!("read source blob {}", file.display()))?;
        let blob = OpBlobRef::new(&bytes)
            .with_context(|| format!("decode source blob {}", file.display()))?;
        let actual_hash = blob.schema_hash_hex();
        if actual_hash != expected_hash {
            bail!(
                "manifest hash mismatch for '{plugin_id}': expected {expected_hash}, got {actual_hash}"
            );
        }
        // The schema's own authority decodes it, so this projection reads
        // exactly what was sealed; `from_sealed` then fails to compile, not to
        // capture, whenever the authority's shape changes.
        let sealed: op_state_store::PluginSchema = serde_json::from_slice(blob.schema_json())
            .with_context(|| format!("decode sealed PluginSchema for '{plugin_id}'"))?;
        crate::model::from_sealed(sealed)
            .with_context(|| format!("convert sealed PluginSchema for '{plugin_id}'"))
    }
}

struct OpBlobRef<'a> {
    bytes: &'a [u8],
    schema_range: std::ops::Range<usize>,
}

impl<'a> OpBlobRef<'a> {
    fn new(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() < HEADER_LEN || &bytes[..8] != MAGIC {
            bail!("not an OPBLOB01 blob");
        }
        let version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if version != 1 {
            bail!("unsupported OPBLOB format version {version}");
        }
        let count = u16::from_le_bytes([bytes[10], bytes[11]]) as usize;
        let table_end = HEADER_LEN
            .checked_add(
                count
                    .checked_mul(SECTION_ENTRY_LEN)
                    .context("section count overflow")?,
            )
            .context("section table overflow")?;
        if table_end > bytes.len() {
            bail!("section table overruns blob");
        }

        let mut schema_range = None;
        for index in 0..count {
            let entry = HEADER_LEN + index * SECTION_ENTRY_LEN;
            let tag = u32::from_le_bytes(bytes[entry..entry + 4].try_into()?);
            let offset = u64::from_le_bytes(bytes[entry + 8..entry + 16].try_into()?) as usize;
            let length = u64::from_le_bytes(bytes[entry + 16..entry + 24].try_into()?) as usize;
            let end = offset
                .checked_add(length)
                .context("section range overflow")?;
            if end > bytes.len() {
                bail!("section {tag} overruns blob");
            }
            if tag == SECTION_SCHEMA_JSON && schema_range.replace(offset..end).is_some() {
                bail!("duplicate schema section");
            }
        }

        let schema_range = schema_range.context("missing schema section")?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes[schema_range.clone()]);
        if hasher.finalize().as_slice() != &bytes[16..48] {
            bail!("schema hash mismatch: blob is corrupt or tampered");
        }

        Ok(Self {
            bytes,
            schema_range,
        })
    }

    fn schema_json(&self) -> &'a [u8] {
        &self.bytes[self.schema_range.clone()]
    }

    fn schema_hash_hex(&self) -> String {
        self.bytes[16..48]
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> Result<T> {
    let path = path.as_ref();
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("decode {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_blob_bytes() {
        assert!(OpBlobRef::new(b"not a blob").is_err());
    }

    #[test]
    fn rejects_unsafe_catalog_identity_before_file_access() {
        let source = SourceCatalog::new("/does/not/matter", "/does/not/matter");
        assert!(source.read_schema("../escape", &"a".repeat(64)).is_err());
        assert!(source.read_schema("safe", "not-a-hash").is_err());
    }
}
