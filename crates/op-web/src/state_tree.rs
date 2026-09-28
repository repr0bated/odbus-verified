//! Committed public state-tree reader.
//!
//! Plugin membership comes only from the sealer-owned blob manifest. State
//! bytes come from one locked projection transaction selected by that
//! membership. This reader never enumerates the projection directory and never
//! opens a mutable blob store.

use anyhow::{Context, Result, ensure};
use simd_json::OwnedValue as Value;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

const BLOB_CATALOG_DIR: &str = op_blob::catalog::DEFAULT_SHM_DIR;

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommittedPluginState {
    pub schema_hash: String,
    /// Exact schema JSON section from the manifest-pinned sealed blob.
    pub schema_json: String,
    pub schema: op_state_store::PluginSchema,
    pub state: Option<Value>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommittedStateTree {
    pub catalog_hash: String,
    pub projection_generation: u64,
    pub plugins: BTreeMap<String, CommittedPluginState>,
}

struct SealedCatalog {
    catalog_hash: String,
    plugins: BTreeMap<String, String>,
    manifest: Vec<u8>,
}

/// Read a sealed plugin's committed public state. A sealed plugin without a
/// committed projection returns `None`; catalog and projection errors fail.
pub async fn read_plugin(plugin: &str) -> Result<Option<Value>> {
    let plugin = plugin.to_string();
    let state_dir = op_core::projection_shm::shm_state_dir();
    let catalog_dir = blob_catalog_dir();
    tokio::task::spawn_blocking(move || read_plugin_in(&state_dir, &catalog_dir, &plugin))
        .await
        .map_err(|error| anyhow::anyhow!("committed state reader task failed: {error}"))?
}

/// Read every sealed plugin state present in one committed projection snapshot.
/// Sealed plugins without state and state files without a sealed plugin are
/// absent from the result.
pub async fn read_all() -> Result<HashMap<String, Value>> {
    let state_dir = op_core::projection_shm::shm_state_dir();
    let catalog_dir = blob_catalog_dir();
    tokio::task::spawn_blocking(move || read_all_in(&state_dir, &catalog_dir))
        .await
        .map_err(|error| anyhow::anyhow!("committed state reader task failed: {error}"))?
}

/// Read a manifest-pinned catalog and its complete committed public state in
/// one receipt. Callers can use missing `state` entries and the catalog hash to
/// clear retired or not-yet-published values during push-driven hydration.
pub async fn read_committed_tree() -> Result<CommittedStateTree> {
    read_committed_tree_from(op_core::projection_shm::shm_state_dir(), blob_catalog_dir()).await
}

pub(crate) async fn read_committed_tree_from(
    state_dir: String,
    catalog_dir: PathBuf,
) -> Result<CommittedStateTree> {
    tokio::task::spawn_blocking(move || read_committed_tree_in(&state_dir, &catalog_dir))
        .await
        .map_err(|error| anyhow::anyhow!("committed state reader task failed: {error}"))?
}

fn blob_catalog_dir() -> PathBuf {
    std::env::var_os("OP_BLOB_CATALOG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(BLOB_CATALOG_DIR))
}

fn read_plugin_in(state_dir: &str, catalog_dir: &Path, plugin: &str) -> Result<Option<Value>> {
    let sealed = sealed_catalog(catalog_dir)?;
    ensure!(
        sealed.plugins.contains_key(plugin),
        "plugin '{plugin}' is not a current sealed catalog member"
    );
    let schema = validate_sealed_plugin(catalog_dir, plugin)?;
    ensure_manifest_unchanged(catalog_dir, &sealed.manifest)?;

    let plugin_id = plugin.to_string();
    let snapshot = op_core::projection_shm::read_projection_snapshot_in(
        state_dir,
        std::slice::from_ref(&plugin_id),
    )?;
    ensure_manifest_unchanged(catalog_dir, &sealed.manifest)?;
    snapshot
        .states
        .get(plugin)
        .map(|bytes| decode_public_state(plugin, &schema, bytes))
        .transpose()
        .map(Option::flatten)
}

fn read_all_in(state_dir: &str, catalog_dir: &Path) -> Result<HashMap<String, Value>> {
    Ok(read_committed_tree_in(state_dir, catalog_dir)?
        .plugins
        .into_iter()
        .filter_map(|(plugin, entry)| entry.state.map(|state| (plugin, state)))
        .collect())
}

fn read_committed_tree_in(state_dir: &str, catalog_dir: &Path) -> Result<CommittedStateTree> {
    let sealed = sealed_catalog(catalog_dir)?;
    let mut schemas = BTreeMap::new();
    for (plugin, schema_hash) in &sealed.plugins {
        schemas.insert(
            plugin.clone(),
            (
                validate_sealed_plugin(catalog_dir, plugin)?,
                original_schema_json(catalog_dir, plugin, schema_hash)?,
            ),
        );
    }
    ensure_manifest_unchanged(catalog_dir, &sealed.manifest)?;
    let ids: Vec<String> = sealed.plugins.keys().cloned().collect();
    let snapshot = op_core::projection_shm::read_projection_snapshot_in(state_dir, &ids)?;
    ensure_manifest_unchanged(catalog_dir, &sealed.manifest)?;

    let mut plugins = BTreeMap::new();
    for (plugin, schema_hash) in sealed.plugins {
        let (schema, schema_json) = schemas
            .remove(&plugin)
            .context("validated schema disappeared")?;
        let state = snapshot
            .states
            .get(&plugin)
            .map(|bytes| decode_public_state(&plugin, &schema, bytes))
            .transpose()?
            .flatten();
        plugins.insert(
            plugin.clone(),
            CommittedPluginState {
                schema_json,
                schema_hash,
                schema,
                state,
            },
        );
    }
    Ok(CommittedStateTree {
        catalog_hash: sealed.catalog_hash,
        projection_generation: snapshot.generation,
        plugins,
    })
}

fn original_schema_json(catalog_dir: &Path, plugin: &str, schema_hash: &str) -> Result<String> {
    let path = catalog_dir.join(format!("{plugin}.{}.blob", &schema_hash[..16]));
    let bytes = std::fs::read(&path)
        .with_context(|| format!("read sealed plugin blob {}", path.display()))?;
    let blob = op_blob::BlobRef::from_manifest_pinned(&bytes, schema_hash)
        .map_err(|error| anyhow::anyhow!("invalid sealed plugin blob: {error}"))?;
    let manifest = blob
        .manifest()
        .map_err(|error| anyhow::anyhow!("invalid sealed plugin manifest: {error}"))?;
    ensure!(
        manifest.plugin_id == plugin
            && manifest.schema_hash == schema_hash
            && manifest.blob_type == "active_reflection",
        "sealed plugin blob identity does not match catalog"
    );
    Ok(blob.schema_json().to_string())
}

fn sealed_catalog(catalog_dir: &Path) -> Result<SealedCatalog> {
    let manifest_path = catalog_dir.join(op_blob::catalog::MANIFEST_FILENAME);
    let bytes = std::fs::read(&manifest_path)
        .with_context(|| format!("read sealed catalog manifest {}", manifest_path.display()))?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&bytes).context("parse sealed catalog manifest")?;
    let catalog_hash = manifest
        .get("catalog_hash")
        .and_then(serde_json::Value::as_str)
        .filter(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .context("sealed catalog manifest has no valid catalog hash")?
        .to_string();
    let plugins = manifest
        .get("plugins")
        .and_then(serde_json::Value::as_object)
        .context("sealed catalog manifest has no plugins map")?;
    let mut sealed = BTreeMap::new();
    for (id, hash) in plugins {
        ensure!(
            valid_plugin_id(id),
            "sealed catalog contains an invalid plugin id"
        );
        let hash = hash
            .as_str()
            .context("sealed catalog plugin hash is not a string")?;
        ensure!(
            hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "sealed catalog contains a malformed plugin hash"
        );
        sealed.insert(id.clone(), hash.to_string());
    }
    Ok(SealedCatalog {
        catalog_hash,
        plugins: sealed,
        manifest: bytes,
    })
}

fn validate_sealed_plugin(
    catalog_dir: &Path,
    plugin: &str,
) -> Result<op_state_store::PluginSchema> {
    op_blob::catalog::read_plugin_state_store_schema(catalog_dir, plugin)
        .with_context(|| format!("sealed plugin '{plugin}' is missing or invalid"))
}

fn ensure_manifest_unchanged(catalog_dir: &Path, expected: &[u8]) -> Result<()> {
    ensure!(
        std::fs::read(catalog_dir.join(op_blob::catalog::MANIFEST_FILENAME))? == expected,
        "sealed catalog changed during state read"
    );
    Ok(())
}

fn valid_plugin_id(plugin: &str) -> bool {
    !plugin.is_empty()
        && plugin
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

fn decode_public_state(
    plugin: &str,
    schema: &op_state_store::PluginSchema,
    bytes: &[u8],
) -> Result<Option<Value>> {
    let mut bytes = bytes.to_vec();
    let value = simd_json::to_owned_value(&mut bytes)
        .with_context(|| format!("committed state for '{plugin}' is invalid JSON"))?;
    let Some(mut value) = schema.project_public_state(&value)? else {
        return Ok(None);
    };
    if plugin == "identity_sled" {
        redact_identity_credentials(&mut value);
    }
    Ok(Some(value))
}

fn redact_identity_credentials(value: &mut Value) {
    match value {
        Value::Object(object) => {
            object.remove("sealed_id");
            for nested in object.values_mut() {
                redact_identity_credentials(nested);
            }
        }
        Value::Array(values) => {
            for nested in values {
                redact_identity_credentials(nested);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use simd_json::json;

    struct Fixture {
        state: tempfile::TempDir,
        catalog: tempfile::TempDir,
    }

    impl Fixture {
        fn new(plugins: &[&str]) -> Self {
            let state = tempfile::tempdir().unwrap();
            let catalog = tempfile::tempdir().unwrap();
            let mut sealer = op_blob::BlobStore::open(catalog.path()).unwrap();
            for plugin in plugins {
                let schema = if *plugin == "identity_sled" {
                    let mut identity = HashMap::new();
                    identity.insert(
                        "principal".to_string(),
                        op_state_store::FieldSchema {
                            field_type: op_state_store::FieldType::String,
                            required: false,
                            description: String::new(),
                            default: None,
                            example: None,
                            constraints: Vec::new(),
                            read_only: false,
                            read_only_when: None,
                        },
                    );
                    identity.insert(
                        "sealed_id".to_string(),
                        op_state_store::FieldSchema {
                            field_type: op_state_store::FieldType::String,
                            required: false,
                            description: String::new(),
                            default: None,
                            example: None,
                            constraints: Vec::new(),
                            read_only: false,
                            read_only_when: None,
                        },
                    );
                    op_state_store::PluginSchema::builder(plugin)
                        .object_field("nested", identity, false, "public identity")
                        .array_field(
                            "items",
                            op_state_store::FieldType::Object(HashMap::from([(
                                "sealed_id".to_string(),
                                op_state_store::FieldSchema {
                                    field_type: op_state_store::FieldType::String,
                                    required: false,
                                    description: String::new(),
                                    default: None,
                                    example: None,
                                    constraints: Vec::new(),
                                    read_only: false,
                                    read_only_when: None,
                                },
                            )])),
                            false,
                            "identity entries",
                        )
                        .public_state_paths(&["/nested", "/items"])
                        .build()
                } else {
                    op_state_store::PluginSchema::builder(plugin)
                        .string_field("status", false, "public status")
                        .public_state_path("/status")
                        .build()
                };
                sealer
                    .write(&op_blob::blobify_plugin_schema(plugin, schema))
                    .unwrap();
            }
            Self { state, catalog }
        }

        fn commit(&self, plugin: &str, bytes: &[u8]) {
            op_core::projection_shm::update_projection_in(
                self.state.path().to_str().unwrap(),
                plugin,
                |_| Ok(Some(bytes.to_vec())),
            )
            .unwrap();
        }

        fn read_plugin(&self, plugin: &str) -> Result<Option<Value>> {
            read_plugin_in(
                self.state.path().to_str().unwrap(),
                self.catalog.path(),
                plugin,
            )
        }

        fn read_all(&self) -> Result<HashMap<String, Value>> {
            read_all_in(self.state.path().to_str().unwrap(), self.catalog.path())
        }
    }

    #[test]
    fn reads_committed_sealed_state_and_ignores_committed_orphans() {
        let fixture = Fixture::new(&["alpha", "beta"]);
        fixture.commit("alpha", br#"{"status":"ready","runtime_secret":"drop"}"#);
        fixture.commit("orphan", br#"{"secret":true}"#);

        assert_eq!(
            fixture.read_plugin("alpha").unwrap(),
            Some(json!({"status":"ready"}))
        );
        assert_eq!(fixture.read_plugin("beta").unwrap(), None);
        let all = fixture.read_all().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all["alpha"], json!({"status":"ready"}));
        assert!(!all.contains_key("orphan"));

        let receipt = read_committed_tree_in(
            fixture.state.path().to_str().unwrap(),
            fixture.catalog.path(),
        )
        .unwrap();
        assert_eq!(receipt.projection_generation, 2);
        assert_eq!(receipt.catalog_hash.len(), 64);
        assert_eq!(receipt.plugins.len(), 2);
        assert_eq!(receipt.plugins["beta"].state, None);
        assert_eq!(receipt.plugins["alpha"].schema.name, "alpha");
        assert_eq!(receipt.plugins["alpha"].schema_hash.len(), 64);
        let exact: serde_json::Value =
            serde_json::from_str(&receipt.plugins["alpha"].schema_json).unwrap();
        assert_eq!(exact["name"], "alpha");
    }

    #[test]
    fn rejects_unsealed_plugin_even_when_it_has_committed_state() {
        let fixture = Fixture::new(&["alpha"]);
        fixture.commit("orphan", br#"{"secret":true}"#);
        assert!(fixture.read_plugin("orphan").is_err());
    }

    #[test]
    fn malformed_committed_state_is_not_hidden_as_absent() {
        let fixture = Fixture::new(&["alpha"]);
        fixture.commit("alpha", b"{bad-json");
        assert!(fixture.read_plugin("alpha").is_err());
        assert!(fixture.read_all().is_err());
    }

    #[test]
    fn configured_public_field_type_drift_is_an_error() {
        let fixture = Fixture::new(&["alpha"]);
        fixture.commit("alpha", br#"{"status":7}"#);
        assert!(fixture.read_plugin("alpha").is_err());
        assert!(fixture.read_all().is_err());
    }

    #[test]
    fn invalid_public_policy_is_an_error() {
        let fixture = Fixture::new(&["alpha"]);
        let mut invalid = op_state_store::PluginSchema::builder("invalid_policy")
            .string_field("status", false, "status")
            .build();
        invalid.public_state = op_state_store::PublicStatePolicy {
            paths: vec!["/undeclared".into()],
        };
        op_blob::BlobStore::open(fixture.catalog.path())
            .unwrap()
            .write(&op_blob::blobify_plugin_schema("invalid_policy", invalid))
            .unwrap();
        fixture.commit("invalid_policy", br#"{"status":"private"}"#);
        assert!(fixture.read_plugin("invalid_policy").is_err());
        assert!(fixture.read_all().is_err());
    }

    #[test]
    fn identity_credentials_are_recursively_redacted() {
        let fixture = Fixture::new(&["identity_sled"]);
        fixture.commit(
            "identity_sled",
            br#"{"sealed_id":"root","nested":{"principal":"alice","sealed_id":"nested"},"items":[{"sealed_id":"array"}]}"#,
        );
        assert_eq!(
            fixture.read_plugin("identity_sled").unwrap(),
            Some(json!({"nested":{"principal":"alice"},"items":[{}]}))
        );
    }

    #[test]
    fn absent_public_policy_retains_schema_and_withholds_state() {
        let fixture = Fixture::new(&["alpha"]);
        let closed = op_state_store::PluginSchema::builder("closed")
            .string_field("status", false, "private status")
            .build();
        op_blob::BlobStore::open(fixture.catalog.path())
            .unwrap()
            .write(&op_blob::blobify_plugin_schema("closed", closed))
            .unwrap();
        fixture.commit("closed", br#"{"status":"private"}"#);

        let receipt = read_committed_tree_in(
            fixture.state.path().to_str().unwrap(),
            fixture.catalog.path(),
        )
        .unwrap();
        assert_eq!(receipt.plugins["closed"].state, None);
        assert_eq!(receipt.plugins["closed"].schema.name, "closed");
    }

    #[test]
    fn invalid_catalog_or_incomplete_projection_is_an_error() {
        let fixture = Fixture::new(&["alpha"]);
        fixture.commit("alpha", br#"{"status":"old"}"#);
        std::fs::write(
            fixture.state.path().join(".manifest.json"),
            br#"{"generation":2,"publishing":true}"#,
        )
        .unwrap();
        assert!(fixture.read_all().is_err());

        std::fs::write(
            fixture
                .catalog
                .path()
                .join(op_blob::catalog::MANIFEST_FILENAME),
            b"not-json",
        )
        .unwrap();
        assert!(fixture.read_plugin("alpha").is_err());
    }
}
