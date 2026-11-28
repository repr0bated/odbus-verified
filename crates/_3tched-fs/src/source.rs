use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model::{PluginSchema, TypedObjectBlob};

const MAGIC: &[u8; 8] = b"OPBLOB01";
const HEADER_LEN: usize = 64;
const SECTION_ENTRY_LEN: usize = 24;
const SECTION_SCHEMA_JSON: u32 = 1;

pub const DEFAULT_CATALOG_DIR: &str = "/dev/shm/opdbus/plugin-blobs";
pub const DEFAULT_STATE_DIR: &str = "/dev/shm/opdbus/state";
pub const MAX_SNAPSHOT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CATALOG_BYTES: u64 = 1024 * 1024;

/// One committed, freshly decoded public projection. No value is memoized.
#[derive(Debug)]
pub struct CommittedPublicValue {
    pub generation: u64,
    pub value: serde_json::Value,
}

#[derive(Debug)]
pub struct CommittedPublicSnapshot {
    pub generation: u64,
    pub values: BTreeMap<String, serde_json::Value>,
    /// Missing/deleted entries are unavailable, never synthesized values.
    pub unavailable: BTreeSet<String>,
}

/// Reject catalog/policy drift around live reads. Stored declarations are not
/// sufficient authority after resealing: the active sealed schema must still
/// equal the pin. This helper itself also checks catalog stability while blobs
/// are decoded; callers invoke it before and after reading committed state.
pub fn verify_pinned_catalog(
    catalog_dir: &Path,
    catalog_hash: &str,
    catalog_generation: u64,
    schema_hashes: &BTreeMap<String, String>,
    schemas: &BTreeMap<String, PluginSchema>,
) -> Result<()> {
    let source = SourceCatalog::new(catalog_dir, DEFAULT_STATE_DIR);
    let mut remaining = MAX_SNAPSHOT_BYTES;
    let before = source.read_catalog_manifest_bounded(&mut remaining)?;
    ensure!(
        before.catalog_hash == catalog_hash
            && before.generation == catalog_generation
            && schema_hashes
                .iter()
                .all(|(plugin, hash)| before.plugins.get(plugin) == Some(hash)),
        "active sealed catalog differs from pinned view"
    );
    for (plugin_id, schema) in schemas {
        let hash = schema_hashes
            .get(plugin_id)
            .context("schema pin is absent")?;
        let active = source.read_schema_bounded(plugin_id, hash, &mut remaining)?;
        ensure!(
            active == *schema,
            "active sealed public policy/declaration differs from stored pin"
        );
    }
    ensure!(
        source.read_catalog_manifest()? == before,
        "sealed catalog changed during policy verification"
    );
    Ok(())
}

/// Read all pinned schema selections under one canonical snapshot lock, so a
/// directory refresh never mixes generations. Invalid/pending data fails the
/// whole refresh; absent entries are explicitly unavailable.
pub fn read_committed_public_snapshot(
    state_dir: &Path,
    schemas: &BTreeMap<String, PluginSchema>,
    maximum_bytes: u64,
) -> Result<CommittedPublicSnapshot> {
    for (plugin_id, schema) in schemas {
        ensure!(schema.name == *plugin_id, "pinned schema identity mismatch");
    }
    let snapshot = op_core::projection_shm::read_projection_snapshot_bounded_in(
        state_dir
            .to_str()
            .context("state directory must be UTF-8")?,
        &schemas.keys().cloned().collect::<Vec<_>>(),
        maximum_bytes.min(MAX_SNAPSHOT_BYTES),
    )?;
    ensure!(
        snapshot.generation > 0,
        "no committed state generation exists"
    );
    let mut values = BTreeMap::new();
    let mut unavailable = BTreeSet::new();
    for (plugin_id, schema) in schemas {
        let Some(bytes) = snapshot.states.get(plugin_id) else {
            unavailable.insert(plugin_id.clone());
            continue;
        };
        let raw = serde_json::from_slice(bytes)
            .with_context(|| format!("decode committed plugin state '{plugin_id}'"))?;
        values.insert(plugin_id.clone(), schema.project_public_value(&raw)?);
    }
    Ok(CommittedPublicSnapshot {
        generation: snapshot.generation,
        values,
        unavailable,
    })
}

/// The only live-state reader used by 3tchedFS. A human-bound mount does not
/// authorize disclosure of private state, so the sealed public policy applies
/// even when the mount can admit separately authorized operation files.
pub fn read_committed_public_value(
    state_dir: &Path,
    plugin_id: &str,
    schema: &PluginSchema,
    maximum_bytes: u64,
) -> Result<CommittedPublicValue> {
    ensure!(schema.name == plugin_id, "pinned schema identity mismatch");
    let snapshot = op_core::projection_shm::read_projection_snapshot_bounded_in(
        state_dir
            .to_str()
            .context("state directory must be UTF-8")?,
        &[plugin_id.to_owned()],
        maximum_bytes.min(MAX_SNAPSHOT_BYTES),
    )?;
    ensure!(
        snapshot.generation > 0,
        "no committed state generation exists"
    );
    let bytes = snapshot
        .states
        .get(plugin_id)
        .context("committed plugin state is absent or deleted")?;
    let raw: serde_json::Value =
        serde_json::from_slice(bytes).context("decode committed plugin state")?;
    Ok(CommittedPublicValue {
        generation: snapshot.generation,
        value: schema.project_public_value(&raw)?,
    })
}

#[derive(Debug, Clone)]
pub struct SourceCatalog {
    pub(crate) catalog_dir: PathBuf,
    pub(crate) state_dir: PathBuf,
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

impl SourceCatalog {
    pub fn new(catalog_dir: impl Into<PathBuf>, state_dir: impl Into<PathBuf>) -> Self {
        Self {
            catalog_dir: catalog_dir.into(),
            state_dir: state_dir.into(),
        }
    }

    pub fn capture(&self) -> Result<SourceSnapshot> {
        self.capture_bounded(MAX_SNAPSHOT_BYTES)
    }

    /// Boot dependency path: capture only manifest-pinned declarations, without
    /// reading live state or fabricating schema/default values.
    pub fn capture_schema_only(&self) -> Result<SourceSnapshot> {
        for _ in 0..4 {
            let mut remaining = MAX_SNAPSHOT_BYTES;
            let before = self.read_catalog_manifest_bounded(&mut remaining)?;
            let mut objects = BTreeMap::new();
            for (plugin_id, schema_hash) in &before.plugins {
                let schema = self.read_schema_bounded(plugin_id, schema_hash, &mut remaining)?;
                objects.insert(
                    plugin_id.clone(),
                    TypedObjectBlob::schema_only(plugin_id.clone(), schema_hash.clone(), schema)?,
                );
            }
            let after = self.read_catalog_manifest()?;
            if before == after {
                return Ok(SourceSnapshot {
                    catalog_hash: after.catalog_hash,
                    catalog_generation: after.generation,
                    objects,
                });
            }
        }
        bail!("sealed catalog changed during four schema-only capture attempts")
    }

    /// Capture is bounded across the catalog, selected schema blobs and the
    /// complete raw snapshot. Pending publication and invalid state fail closed.
    pub fn capture_bounded(&self, maximum_bytes: u64) -> Result<SourceSnapshot> {
        let maximum_bytes = maximum_bytes.min(MAX_SNAPSHOT_BYTES);
        for _ in 0..4 {
            let mut remaining = maximum_bytes;
            let catalog_before = self.read_catalog_manifest_bounded(&mut remaining)?;
            let mut schemas = BTreeMap::new();
            for (plugin_id, schema_hash) in &catalog_before.plugins {
                schemas.insert(
                    plugin_id.clone(),
                    self.read_schema_bounded(plugin_id, schema_hash, &mut remaining)?,
                );
            }
            let snapshot = op_core::projection_shm::read_projection_snapshot_bounded_in(
                self.state_dir
                    .to_str()
                    .context("state directory must be UTF-8")?,
                &catalog_before.plugins.keys().cloned().collect::<Vec<_>>(),
                remaining,
            )?;
            ensure!(
                snapshot.generation > 0,
                "no committed state generation exists"
            );
            let mut objects = BTreeMap::new();
            for (plugin_id, schema_hash) in &catalog_before.plugins {
                let schema = schemas
                    .remove(plugin_id)
                    .context("selected schema missing")?;
                let bytes = snapshot.states.get(plugin_id).with_context(|| {
                    format!("committed plugin state '{plugin_id}' is absent or deleted")
                })?;
                let raw = serde_json::from_slice(bytes)
                    .with_context(|| format!("decode committed plugin state '{plugin_id}'"))?;
                let value = schema
                    .project_public_value(&raw)
                    .with_context(|| format!("project committed plugin state '{plugin_id}'"))?;
                let object = TypedObjectBlob::new_public(
                    plugin_id.clone(),
                    schema_hash.clone(),
                    schema,
                    value,
                )
                .with_context(|| format!("validate typed source object '{plugin_id}'"))?;
                objects.insert(plugin_id.clone(), object);
            }

            let catalog_after = self.read_catalog_manifest()?;
            if catalog_before == catalog_after {
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
        let mut remaining = MAX_CATALOG_BYTES;
        self.read_catalog_manifest_bounded(&mut remaining)
    }
    fn read_catalog_manifest_bounded(&self, remaining: &mut u64) -> Result<CatalogManifest> {
        let bytes = read_bounded(
            self.catalog_dir.join(".manifest.json"),
            (*remaining).min(MAX_CATALOG_BYTES),
        )?;
        *remaining = remaining
            .checked_sub(bytes.len() as u64)
            .context("catalog exceeds capture budget")?;
        serde_json::from_slice(&bytes).context("decode sealed catalog manifest")
    }

    #[cfg(test)]
    fn read_schema(&self, plugin_id: &str, expected_hash: &str) -> Result<PluginSchema> {
        let mut remaining = MAX_SNAPSHOT_BYTES;
        self.read_schema_bounded(plugin_id, expected_hash, &mut remaining)
    }

    fn read_schema_bounded(
        &self,
        plugin_id: &str,
        expected_hash: &str,
        remaining: &mut u64,
    ) -> Result<PluginSchema> {
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
        let bytes = read_bounded(&file, *remaining)?;
        *remaining = remaining
            .checked_sub(bytes.len() as u64)
            .context("schema blobs exceed capture budget")?;
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

fn read_bounded(path: impl AsRef<Path>, maximum_bytes: u64) -> Result<Vec<u8>> {
    let path = path.as_ref();
    let file: File = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .with_context(|| format!("open bounded source {}", path.display()))?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() <= maximum_bytes,
        "source {} is not a bounded regular file",
        path.display()
    );
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(
            usize::try_from(metadata.len()).context("source allocation size overflow")?,
        )
        .context("reserve bounded source buffer")?;
    let mut reader = file;
    let mut chunk = [0_u8; 8192];
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        ensure!(
            (bytes.len() as u64) + count as u64 <= metadata.len(),
            "source grew during bounded read"
        );
        bytes.extend_from_slice(&chunk[..count]);
    }
    ensure!(
        bytes.len() as u64 == metadata.len(),
        "source changed during bounded read"
    );
    Ok(bytes)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn fixture_schema() -> PluginSchema {
        crate::model::from_sealed(serde_json::from_value(json!({
            "name":"demo", "version":"1", "description":"fixture",
            "fields": {
                "shown":{"field_type":"boolean","required":true},
                "secret":{"field_type":"string","required":true,"default":"never use default"}
            },
            "public_state":{"paths":["/shown"]},
            "methods":{"read":{"name":"read","args":{"type":"object"},"side_effect":"read","idempotent":true,"subid":"obs.demo.read@v1"}}
        })).unwrap()).unwrap()
    }

    fn publish(directory: &Path, state: Value) {
        let bytes = serde_json::to_vec(&state).unwrap();
        op_core::projection_shm::update_projection_in(directory.to_str().unwrap(), "demo", |_| {
            Ok(Some(bytes))
        })
        .unwrap();
    }

    pub(crate) fn fixture_catalog(directory: &Path, schema: &PluginSchema) -> SourceCatalog {
        let catalog = directory.join("catalog");
        let state = directory.join("state");
        std::fs::create_dir(&catalog).unwrap();
        let payload = serde_json::to_vec(schema).unwrap();
        let hash = format!("{:x}", Sha256::digest(&payload));
        let offset = HEADER_LEN + SECTION_ENTRY_LEN;
        let mut blob = vec![0; offset];
        blob[..8].copy_from_slice(MAGIC);
        blob[8..10].copy_from_slice(&1_u16.to_le_bytes());
        blob[10..12].copy_from_slice(&1_u16.to_le_bytes());
        blob[16..48].copy_from_slice(&Sha256::digest(&payload));
        blob[HEADER_LEN..HEADER_LEN + 4].copy_from_slice(&SECTION_SCHEMA_JSON.to_le_bytes());
        blob[HEADER_LEN + 8..HEADER_LEN + 16].copy_from_slice(&(offset as u64).to_le_bytes());
        blob[HEADER_LEN + 16..HEADER_LEN + 24]
            .copy_from_slice(&(payload.len() as u64).to_le_bytes());
        blob.extend(payload);
        std::fs::write(catalog.join(format!("demo.{}.blob", &hash[..16])), blob).unwrap();
        std::fs::write(
            catalog.join(".manifest.json"),
            serde_json::to_vec(
                &json!({"catalog_hash":"fixture","generation":1,"plugins":{"demo":hash}}),
            )
            .unwrap(),
        )
        .unwrap();
        SourceCatalog::new(catalog, state)
    }

    #[test]
    fn committed_reader_validates_full_state_and_publishes_only_public_members() {
        let directory = tempfile::tempdir().unwrap();
        let schema = fixture_schema();
        publish(directory.path(), json!({"shown":true,"secret":"private"}));
        let snapshot =
            read_committed_public_value(directory.path(), "demo", &schema, MAX_SNAPSHOT_BYTES)
                .unwrap();
        assert_eq!(snapshot.generation, 1);
        assert_eq!(snapshot.value, json!({"shown":true}));
        assert!(schema.validate_value(&snapshot.value, None).is_err());
        schema.validate_public_value(&snapshot.value).unwrap();
        assert!(schema.methods.contains_key("read"));
        publish(directory.path(), json!({"shown":true}));
        assert!(
            read_committed_public_value(directory.path(), "demo", &schema, MAX_SNAPSHOT_BYTES)
                .is_err()
        );
    }

    #[test]
    fn absent_invalid_deleted_and_uncommitted_state_never_falls_back() {
        let directory = tempfile::tempdir().unwrap();
        let schema = fixture_schema();
        assert!(
            read_committed_public_value(directory.path(), "demo", &schema, MAX_SNAPSHOT_BYTES)
                .is_err()
        );
        std::fs::write(
            directory.path().join("demo.json"),
            b"{\"shown\":true,\"secret\":\"private\"}",
        )
        .unwrap();
        assert!(
            read_committed_public_value(directory.path(), "demo", &schema, MAX_SNAPSHOT_BYTES)
                .is_err()
        );
        std::fs::remove_file(directory.path().join("demo.json")).unwrap();
        publish(
            directory.path(),
            json!({"shown":"wrong","secret":"private"}),
        );
        assert!(
            read_committed_public_value(directory.path(), "demo", &schema, MAX_SNAPSHOT_BYTES)
                .is_err()
        );
        op_core::projection_shm::update_projection_in(
            directory.path().to_str().unwrap(),
            "demo",
            |_| Ok(Some(b"{invalid".to_vec())),
        )
        .unwrap();
        assert!(
            read_committed_public_value(directory.path(), "demo", &schema, MAX_SNAPSHOT_BYTES)
                .is_err()
        );
        op_core::projection_shm::update_projection_in(
            directory.path().to_str().unwrap(),
            "demo",
            |_| Ok(None),
        )
        .unwrap();
        assert!(
            read_committed_public_value(directory.path(), "demo", &schema, MAX_SNAPSHOT_BYTES)
                .is_err()
        );
    }

    #[test]
    fn interrupted_and_pending_publications_fail_closed() {
        for gate in [
            json!({"publishing":true}),
            json!({"document_pending":{"plugin_id":"demo","operation_id":"fixture"}}),
            json!({"credential_pending":{"plugin_id":"demo","operation_id":"fixture"}}),
        ] {
            let directory = tempfile::tempdir().unwrap();
            publish(directory.path(), json!({"shown":true,"secret":"private"}));
            let mut manifest = gate;
            manifest["generation"] = json!(1);
            std::fs::write(
                directory.path().join(".manifest.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            assert!(
                read_committed_public_value(
                    directory.path(),
                    "demo",
                    &fixture_schema(),
                    MAX_SNAPSHOT_BYTES
                )
                .is_err()
            );
        }
    }

    #[test]
    fn committed_reader_is_bounded_and_empty_policy_exposes_no_state() {
        let directory = tempfile::tempdir().unwrap();
        publish(directory.path(), json!({"shown":true,"secret":"private"}));
        assert!(
            read_committed_public_value(directory.path(), "demo", &fixture_schema(), 1).is_err()
        );
        let mut private = fixture_schema();
        private.public_state = json!({"paths":[]});
        assert_eq!(
            read_committed_public_value(directory.path(), "demo", &private, MAX_SNAPSHOT_BYTES)
                .unwrap()
                .value,
            json!({})
        );
        private.public_state = json!({"paths":[""]});
        assert!(
            read_committed_public_value(directory.path(), "demo", &private, MAX_SNAPSHOT_BYTES)
                .is_err()
        );
    }

    #[test]
    fn capture_uses_committed_public_objects_and_aggregate_budget() {
        let directory = tempfile::tempdir().unwrap();
        let source = fixture_catalog(directory.path(), &fixture_schema());
        assert!(source.capture().is_err());
        publish(&source.state_dir, json!({"shown":true,"secret":"private"}));
        let captured = source.capture().unwrap();
        let object = &captured.objects["demo"];
        assert_eq!(object.value, json!({"shown":true}));
        assert_eq!(
            object.value_scope,
            crate::model::StateValueScope::PublicProjection
        );
        assert!(object.schema.fields.contains_key("secret"));
        assert!(object.schema.methods.contains_key("read"));
        assert!(source.capture_bounded(1).is_err());
        let catalog_bytes = std::fs::metadata(source.catalog_dir.join(".manifest.json"))
            .unwrap()
            .len();
        let blob_path = std::fs::read_dir(&source.catalog_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "blob")
            })
            .unwrap();
        let schema_bytes = std::fs::metadata(blob_path).unwrap().len();
        let state_bytes = std::fs::metadata(source.state_dir.join("demo.json"))
            .unwrap()
            .len();
        assert!(
            source
                .capture_bounded(catalog_bytes + schema_bytes + state_bytes - 1)
                .is_err()
        );
        publish(&source.state_dir, json!({"shown":true}));
        assert!(source.capture().is_err());
    }

    #[test]
    fn boot_capture_is_schema_only_even_without_committed_state() {
        let directory = tempfile::tempdir().unwrap();
        let source = fixture_catalog(directory.path(), &fixture_schema());
        let captured = source.capture_schema_only().unwrap();
        assert_eq!(
            captured.objects["demo"].value_scope,
            crate::model::StateValueScope::SchemaOnly
        );
        assert!(captured.objects["demo"].value.is_null());
        assert!(captured.objects["demo"].schema.methods.contains_key("read"));
        assert!(!source.state_dir.exists());
        assert!(source.capture().is_err());
    }

    #[test]
    fn snapshot_reports_deletion_and_enforces_one_aggregate_budget() {
        let directory = tempfile::tempdir().unwrap();
        let mut other = fixture_schema();
        other.name = "other".to_owned();
        let schemas = BTreeMap::from([
            ("demo".to_owned(), fixture_schema()),
            ("other".to_owned(), other),
        ]);
        publish(directory.path(), json!({"shown":true,"secret":"private"}));
        let first =
            read_committed_public_snapshot(directory.path(), &schemas, MAX_SNAPSHOT_BYTES).unwrap();
        assert_eq!(first.generation, 1);
        assert!(first.unavailable.contains("other"));
        assert!(!first.values.contains_key("other"));
        let bytes = serde_json::to_vec(&json!({"shown":false,"secret":"private"})).unwrap();
        op_core::projection_shm::update_projection_in(
            directory.path().to_str().unwrap(),
            "other",
            |_| Ok(Some(bytes)),
        )
        .unwrap();
        let second =
            read_committed_public_snapshot(directory.path(), &schemas, MAX_SNAPSHOT_BYTES).unwrap();
        assert_eq!(second.generation, 2);
        assert!(second.unavailable.is_empty());
        let single_size = std::fs::metadata(directory.path().join("demo.json"))
            .unwrap()
            .len();
        assert!(read_committed_public_snapshot(directory.path(), &schemas, single_size).is_err());
    }

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
