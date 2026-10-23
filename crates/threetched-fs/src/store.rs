use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::model::{FORMAT_VERSION, TypedObjectBlob};
use crate::source::{SourceCatalog, SourceSnapshot};

pub const DEFAULT_STORE_DIR: &str = "/dev/shm/3tchedfs";
pub const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

#[derive(Debug, thiserror::Error)]
#[error("workspace object conflict: expected {expected_object_hash}, found {actual_object_hash}")]
pub struct StageConflict {
    pub expected_object_hash: String,
    pub actual_object_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ViewManifest {
    pub format: String,
    pub view_id: String,
    pub source_catalog_hash: String,
    pub source_catalog_generation: u64,
    pub parent_view: Option<String>,
    pub objects: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ViewBody {
    format: String,
    source_catalog_hash: String,
    source_catalog_generation: u64,
    parent_view: Option<String>,
    objects: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Workspace {
    pub format: String,
    pub workspace_id: String,
    pub base_view: String,
    pub staged_objects: BTreeMap<String, String>,
    pub committed_view: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ObjectStore {
    root: PathBuf,
}

impl ObjectStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let store = Self { root: root.into() };
        for directory in [
            store.objects_dir(),
            store.views_dir(),
            store.workspaces_dir(),
            store.snapshots_dir(),
        ] {
            fs::create_dir_all(&directory)
                .with_context(|| format!("create store directory {}", directory.display()))?;
        }
        Ok(store)
    }

    pub fn capture(&self, source: &SourceCatalog, activate: bool) -> Result<ViewManifest> {
        let snapshot = source.capture()?;
        self.capture_snapshot(snapshot, activate)
    }

    pub fn capture_snapshot(
        &self,
        snapshot: SourceSnapshot,
        activate: bool,
    ) -> Result<ViewManifest> {
        let _lock = self.lock_exclusive()?;
        let mut objects = BTreeMap::new();
        for (plugin_id, object) in snapshot.objects {
            let hash = self.write_object(&object)?;
            objects.insert(plugin_id, hash);
        }

        if let Some(current) = self.current_view().transpose()?
            && current.source_catalog_hash == snapshot.catalog_hash
            && current.source_catalog_generation == snapshot.catalog_generation
            && current.objects == objects
        {
            return Ok(current);
        }

        let parent_view = self.current_view_id()?;
        let body = ViewBody {
            format: FORMAT_VERSION.to_string(),
            source_catalog_hash: snapshot.catalog_hash,
            source_catalog_generation: snapshot.catalog_generation,
            parent_view,
            objects,
        };
        let view = self.write_view(body)?;
        if activate {
            self.set_current(&view.view_id)?;
        }
        Ok(view)
    }

    pub fn create_workspace(
        &self,
        workspace_id: Option<&str>,
        base_view: Option<&str>,
    ) -> Result<Workspace> {
        let _lock = self.lock_exclusive()?;
        let id = workspace_id
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        validate_name(&id, "workspace")?;
        let base_view = match base_view {
            Some(id) => {
                self.read_view(id)?;
                id.to_string()
            }
            None => self
                .current_view_id()?
                .context("no current view; capture the SHM source first")?,
        };
        let workspace = Workspace {
            format: FORMAT_VERSION.to_string(),
            workspace_id: id.clone(),
            base_view,
            staged_objects: BTreeMap::new(),
            committed_view: None,
        };
        let path = self.workspace_path(&id);
        if path.exists() {
            bail!("workspace '{id}' already exists");
        }
        self.atomic_json(&path, &workspace)?;
        Ok(workspace)
    }

    pub fn read_workspace(&self, id: &str) -> Result<Workspace> {
        validate_name(id, "workspace")?;
        self.read_json(self.workspace_path(id))
    }

    fn stage_state_locked(
        &self,
        mut workspace: Workspace,
        plugin_id: &str,
        value: Value,
    ) -> Result<String> {
        if workspace.committed_view.is_some() {
            bail!(
                "workspace '{}' is already committed",
                workspace.workspace_id
            );
        }
        let current_object = self
            .workspace_object(&workspace, plugin_id)?
            .public_projection()?;
        let base_view = self.read_view(&workspace.base_view)?;
        let immutable_base = self
            .object_for_view(&base_view, plugin_id)?
            .public_projection()?;
        let object = TypedObjectBlob::new_public(
            plugin_id.to_string(),
            current_object.source_schema_hash.clone(),
            current_object.schema.clone(),
            value,
        )?;
        object.validate(Some(&immutable_base.value))?;
        let hash = self.write_object(&object)?;
        workspace
            .staged_objects
            .insert(plugin_id.to_string(), hash.clone());
        self.atomic_json(&self.workspace_path(&workspace.workspace_id), &workspace)?;
        Ok(hash)
    }

    pub fn stage_pointer(
        &self,
        workspace_id: &str,
        plugin_id: &str,
        pointer: &str,
        value: Value,
    ) -> Result<String> {
        self.stage_pointer_with_expected(workspace_id, plugin_id, pointer, None, value)
    }

    pub fn stage_pointer_checked(
        &self,
        workspace_id: &str,
        plugin_id: &str,
        pointer: &str,
        expected_object_hash: &str,
        value: Value,
    ) -> Result<String> {
        validate_hash(expected_object_hash, "expected object")?;
        self.stage_pointer_with_expected(
            workspace_id,
            plugin_id,
            pointer,
            Some(expected_object_hash),
            value,
        )
    }

    fn stage_pointer_with_expected(
        &self,
        workspace_id: &str,
        plugin_id: &str,
        pointer: &str,
        expected: Option<&str>,
        value: Value,
    ) -> Result<String> {
        let _lock = self.lock_exclusive()?;
        let workspace = self.read_workspace(workspace_id)?;
        if let Some(expected) = expected {
            let base = self.read_view(&workspace.base_view)?;
            let actual = workspace
                .staged_objects
                .get(plugin_id)
                .or_else(|| base.objects.get(plugin_id))
                .context("workspace plugin is absent")?;
            if actual != expected {
                return Err(StageConflict {
                    expected_object_hash: expected.to_owned(),
                    actual_object_hash: actual.clone(),
                }
                .into());
            }
        }
        let object = self
            .workspace_object(&workspace, plugin_id)?
            .public_projection()?;
        if object.value_scope == crate::model::StateValueScope::SchemaOnly {
            bail!("schema-only state is unavailable for staging");
        }
        let mut state = object.value;
        set_json_pointer(&mut state, pointer, value)?;
        self.stage_state_locked(workspace, plugin_id, state)
    }

    pub fn commit_workspace(&self, workspace_id: &str, activate: bool) -> Result<ViewManifest> {
        let _lock = self.lock_exclusive()?;
        let mut workspace = self.read_workspace(workspace_id)?;
        if let Some(view_id) = &workspace.committed_view {
            return self.read_view(view_id);
        }
        if activate && self.current_view_id()?.as_deref() != Some(workspace.base_view.as_str()) {
            bail!(
                "workspace base {} is stale; current view changed",
                workspace.base_view
            );
        }
        let base = self.read_view(&workspace.base_view)?;
        let mut objects = base.objects.clone();
        objects.extend(workspace.staged_objects.clone());
        let body = ViewBody {
            format: FORMAT_VERSION.to_string(),
            source_catalog_hash: base.source_catalog_hash,
            source_catalog_generation: base.source_catalog_generation,
            parent_view: Some(base.view_id.clone()),
            objects,
        };
        let view = self.write_view(body)?;
        if activate {
            self.set_current(&view.view_id)?;
        }
        workspace.committed_view = Some(view.view_id.clone());
        self.atomic_json(&self.workspace_path(workspace_id), &workspace)?;
        Ok(view)
    }

    pub fn abort_workspace(&self, workspace_id: &str) -> Result<()> {
        let _lock = self.lock_exclusive()?;
        let workspace = self.read_workspace(workspace_id)?;
        if workspace.committed_view.is_some() {
            bail!("workspace '{workspace_id}' is committed and cannot be aborted");
        }
        fs::remove_file(self.workspace_path(workspace_id))
            .with_context(|| format!("remove workspace '{workspace_id}'"))
    }

    pub fn snapshot(&self, name: &str, view_id: Option<&str>) -> Result<ViewManifest> {
        let _lock = self.lock_exclusive()?;
        validate_name(name, "snapshot")?;
        let view = match view_id {
            Some(view_id) => self.read_view(view_id)?,
            None => self
                .current_view()
                .transpose()?
                .context("no current view")?,
        };
        let path = self.snapshot_path(name);
        if path.exists() {
            bail!("snapshot '{name}' already exists");
        }
        self.atomic_write(&path, format!("{}\n", view.view_id).as_bytes())?;
        Ok(view)
    }

    pub fn read_snapshot(&self, name: &str) -> Result<ViewManifest> {
        validate_name(name, "snapshot")?;
        let id = String::from_utf8(read_bounded(&self.snapshot_path(name), 256)?)
            .with_context(|| format!("read snapshot '{name}'"))?;
        self.read_view(id.trim())
    }

    pub fn current_view_id(&self) -> Result<Option<String>> {
        let path = self.root.join("current");
        match read_bounded(&path, 256) {
            Ok(value) => Ok(Some(String::from_utf8(value)?.trim().to_string())),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                Ok(None)
            }
            Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
        }
    }

    pub fn current_view(&self) -> Option<Result<ViewManifest>> {
        match self.current_view_id() {
            Ok(Some(id)) => Some(self.read_view(&id)),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        }
    }

    pub fn read_view(&self, id: &str) -> Result<ViewManifest> {
        validate_hash(id, "view")?;
        let view: ViewManifest = self.read_json(self.view_path(id))?;
        if view.view_id != id {
            bail!(
                "view file identity mismatch: requested {id}, contains {}",
                view.view_id
            );
        }
        let actual = hash_json(&ViewBody {
            format: view.format.clone(),
            source_catalog_hash: view.source_catalog_hash.clone(),
            source_catalog_generation: view.source_catalog_generation,
            parent_view: view.parent_view.clone(),
            objects: view.objects.clone(),
        })?;
        if actual != id {
            bail!("view {id} failed content-hash verification");
        }
        Ok(view)
    }

    pub fn read_object(&self, hash: &str) -> Result<TypedObjectBlob> {
        validate_hash(hash, "object")?;
        let path = self.object_path(hash);
        let bytes = read_bounded(&path, MAX_DOCUMENT_BYTES)?;
        if hash_bytes(&bytes) != hash {
            bail!("object {hash} failed content-hash verification");
        }
        let object: TypedObjectBlob =
            serde_json::from_slice(&bytes).with_context(|| format!("decode object {hash}"))?;
        object.validate(None)?;
        Ok(object)
    }

    pub fn object_for_view(&self, view: &ViewManifest, plugin_id: &str) -> Result<TypedObjectBlob> {
        let hash = view
            .objects
            .get(plugin_id)
            .with_context(|| format!("plugin '{plugin_id}' is not in view {}", view.view_id))?;
        self.read_object(hash)
    }

    pub fn workspace_object(
        &self,
        workspace: &Workspace,
        plugin_id: &str,
    ) -> Result<TypedObjectBlob> {
        if let Some(hash) = workspace.staged_objects.get(plugin_id) {
            return self.read_object(hash);
        }
        let view = self.read_view(&workspace.base_view)?;
        self.object_for_view(&view, plugin_id)
    }

    pub fn list_views(&self) -> Result<Vec<String>> {
        list_stems(&self.views_dir(), ".json")
    }

    pub fn list_workspaces(&self) -> Result<Vec<String>> {
        list_stems(&self.workspaces_dir(), ".json")
    }

    pub fn list_snapshots(&self) -> Result<Vec<String>> {
        list_stems(&self.snapshots_dir(), ".ref")
    }

    fn write_object(&self, object: &TypedObjectBlob) -> Result<String> {
        object.validate(None)?;
        let bytes = canonical_json(object)?;
        ensure!(
            bytes.len() as u64 <= MAX_DOCUMENT_BYTES,
            "typed object exceeds document byte limit"
        );
        let hash = hash_bytes(&bytes);
        let path = self.object_path(&hash);
        if !path.exists() {
            self.atomic_write(&path, &bytes)?;
        }
        Ok(hash)
    }

    fn write_view(&self, body: ViewBody) -> Result<ViewManifest> {
        let view_id = hash_json(&body)?;
        let view = ViewManifest {
            format: body.format,
            view_id: view_id.clone(),
            source_catalog_hash: body.source_catalog_hash,
            source_catalog_generation: body.source_catalog_generation,
            parent_view: body.parent_view,
            objects: body.objects,
        };
        let path = self.view_path(&view_id);
        if !path.exists() {
            self.atomic_json(&path, &view)?;
        }
        Ok(view)
    }

    fn set_current(&self, view_id: &str) -> Result<()> {
        self.read_view(view_id)?;
        self.atomic_write(
            &self.root.join("current"),
            format!("{view_id}\n").as_bytes(),
        )
    }

    fn atomic_json(&self, path: &Path, value: &impl Serialize) -> Result<()> {
        let bytes = bounded_json(value, MAX_MANIFEST_BYTES, false)?;
        self.atomic_write(path, &bytes)
    }

    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() as u64 <= MAX_DOCUMENT_BYTES,
            "store write exceeds document byte limit"
        );
        let parent = path.parent().context("atomic write path has no parent")?;
        fs::create_dir_all(parent)?;
        let temp = parent.join(format!(
            ".{}.{}.tmp",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("object"),
            Uuid::new_v4()
        ));
        {
            let mut file = File::create(&temp)
                .with_context(|| format!("create temporary file {}", temp.display()))?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        fs::rename(&temp, path)
            .with_context(|| format!("publish {} -> {}", temp.display(), path.display()))?;
        File::open(parent)?.sync_all()?;
        Ok(())
    }

    fn read_json<T: serde::de::DeserializeOwned>(&self, path: PathBuf) -> Result<T> {
        let bytes = read_bounded(&path, MAX_MANIFEST_BYTES)?;
        serde_json::from_slice(&bytes).with_context(|| format!("decode {}", path.display()))
    }

    fn lock_exclusive(&self) -> Result<StoreLock> {
        let path = self.root.join(".lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .with_context(|| format!("open store lock {}", path.display()))?;
        file.lock_exclusive()
            .with_context(|| format!("lock store {}", self.root.display()))?;
        Ok(StoreLock(file))
    }

    fn objects_dir(&self) -> PathBuf {
        self.root.join("objects")
    }

    fn views_dir(&self) -> PathBuf {
        self.root.join("views")
    }

    fn workspaces_dir(&self) -> PathBuf {
        self.root.join("workspaces")
    }

    fn snapshots_dir(&self) -> PathBuf {
        self.root.join("snapshots")
    }

    fn object_path(&self, hash: &str) -> PathBuf {
        self.objects_dir().join(format!("{hash}.blob"))
    }

    fn view_path(&self, id: &str) -> PathBuf {
        self.views_dir().join(format!("{id}.json"))
    }

    fn workspace_path(&self, id: &str) -> PathBuf {
        self.workspaces_dir().join(format!("{id}.json"))
    }

    fn snapshot_path(&self, name: &str) -> PathBuf {
        self.snapshots_dir().join(format!("{name}.ref"))
    }
}

fn read_bounded(path: &Path, maximum_bytes: u64) -> Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .with_context(|| format!("open bounded store file {}", path.display()))?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() <= maximum_bytes,
        "store file {} is not a bounded regular file",
        path.display()
    );
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(
            usize::try_from(metadata.len()).context("store allocation size overflow")?,
        )
        .context("reserve bounded store buffer")?;
    let mut reader = file;
    let mut chunk = [0_u8; 8192];
    loop {
        let count = reader.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        ensure!(
            (bytes.len() as u64) + count as u64 <= metadata.len(),
            "store file grew during bounded read"
        );
        bytes.extend_from_slice(&chunk[..count]);
    }
    ensure!(
        bytes.len() as u64 == metadata.len(),
        "store file changed during bounded read"
    );
    Ok(bytes)
}

struct StoreLock(File);

impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn canonical_json(value: &impl Serialize) -> Result<Vec<u8>> {
    bounded_json(value, MAX_DOCUMENT_BYTES, false)
}

struct BoundedBuffer {
    bytes: Vec<u8>,
    maximum: u64,
}
impl Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let total = (self.bytes.len() as u64)
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "JSON byte budget overflow")
            })?;
        if total > self.maximum {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "JSON document exceeds byte budget",
            ));
        }
        self.bytes
            .try_reserve_exact(bytes.len())
            .map_err(std::io::Error::other)?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn bounded_json(value: &impl Serialize, maximum: u64, pretty: bool) -> Result<Vec<u8>> {
    let mut writer = BoundedBuffer {
        bytes: Vec::new(),
        maximum,
    };
    if pretty {
        serde_json::to_writer_pretty(&mut writer, value)?;
        writer.write_all(b"\n")?;
    } else {
        serde_json::to_writer(&mut writer, value)?;
    }
    Ok(writer.bytes)
}
pub fn render_document(value: &impl Serialize) -> Result<Vec<u8>> {
    bounded_json(value, MAX_DOCUMENT_BYTES, true)
}
pub fn render_arguments(value: &impl Serialize) -> Result<Vec<u8>> {
    bounded_json(value, 1024 * 1024, false)
}

fn hash_json(value: &impl Serialize) -> Result<String> {
    Ok(hash_bytes(&bounded_json(value, MAX_MANIFEST_BYTES, false)?))
}

fn hash_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

fn validate_hash(value: &str, kind: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid {kind} hash '{value}'");
    }
    Ok(())
}

fn validate_name(value: &str, kind: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || value == "."
        || value == ".."
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        bail!("invalid {kind} name '{value}'");
    }
    Ok(())
}

fn list_stems(directory: &Path, suffix: &str) -> Result<Vec<String>> {
    let mut values = Vec::new();
    for entry in fs::read_dir(directory)
        .with_context(|| format!("list directory {}", directory.display()))?
    {
        let entry = entry?;
        if let Some(name) = entry.file_name().to_str()
            && let Some(stem) = name.strip_suffix(suffix)
        {
            values.push(stem.to_string());
        }
    }
    values.sort();
    Ok(values)
}

fn set_json_pointer(root: &mut Value, pointer: &str, value: Value) -> Result<()> {
    if pointer.is_empty() {
        *root = value;
        return Ok(());
    }
    if !pointer.starts_with('/') {
        bail!("JSON pointer must be empty or start with '/'");
    }
    let mut segments = pointer
        .split('/')
        .skip(1)
        .map(|segment| segment.replace("~1", "/").replace("~0", "~"))
        .peekable();
    let mut current = root;
    while let Some(segment) = segments.next() {
        let is_last = segments.peek().is_none();
        match current {
            Value::Object(object) if is_last => {
                object.insert(segment, value);
                return Ok(());
            }
            Value::Object(object) => {
                current = object.get_mut(&segment).with_context(|| {
                    format!("JSON pointer component '{segment}' does not exist")
                })?;
            }
            Value::Array(array) => {
                let index: usize = segment
                    .parse()
                    .with_context(|| format!("array pointer '{segment}' is not an index"))?;
                let slot = array
                    .get_mut(index)
                    .with_context(|| format!("array pointer index {index} is out of bounds"))?;
                if is_last {
                    *slot = value;
                    return Ok(());
                }
                current = slot;
            }
            _ => bail!("JSON pointer traverses a scalar at '{segment}'"),
        }
    }
    bail!("invalid JSON pointer '{pointer}'")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::tempdir;

    use super::*;
    use crate::model::{FieldSchema, FieldType, PluginGuarantees, PluginSchema};

    fn object(value: Value) -> TypedObjectBlob {
        let fields = value
            .as_object()
            .unwrap()
            .keys()
            .map(|name| {
                (
                    name.clone(),
                    FieldSchema {
                        field_type: FieldType::Integer,
                        required: true,
                        description: String::new(),
                        default: None,
                        example: None,
                        constraints: Vec::new(),
                        read_only: false,
                        read_only_when: None,
                    },
                )
            })
            .collect();
        let public_state = serde_json::json!({"paths":value.as_object().unwrap().keys().map(|name| format!("/{}",name.replace('~',"~0").replace('/',"~1"))).collect::<Vec<_>>()});
        TypedObjectBlob::new(
            "demo".into(),
            "a".repeat(64),
            PluginSchema {
                name: "demo".into(),
                category: "test".into(),
                version: "1".into(),
                description: String::new(),
                display_name: None,
                fields,
                public_state,
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
                ..Default::default()
            },
            value,
        )
        .unwrap()
    }

    #[test]
    fn workspace_commit_is_copy_on_write() {
        let directory = tempdir().unwrap();
        let store = ObjectStore::open(directory.path()).unwrap();
        let snapshot = SourceSnapshot {
            catalog_hash: "catalog".into(),
            catalog_generation: 1,
            objects: BTreeMap::from([("demo".into(), object(serde_json::json!({"x": 1})))]),
        };
        let base = store.capture_snapshot(snapshot, true).unwrap();
        store.create_workspace(Some("edit"), None).unwrap();
        store
            .stage_pointer("edit", "demo", "/x", Value::from(2))
            .unwrap();
        let committed = store.commit_workspace("edit", true).unwrap();

        assert_ne!(base.view_id, committed.view_id);
        assert_eq!(
            committed.parent_view.as_deref(),
            Some(base.view_id.as_str())
        );
        assert_eq!(
            store
                .object_for_view(&base, "demo")
                .unwrap()
                .value
                .pointer("/x"),
            Some(&Value::from(1))
        );
        assert_eq!(
            store
                .object_for_view(&committed, "demo")
                .unwrap()
                .value
                .pointer("/x"),
            Some(&Value::from(2))
        );
    }

    #[test]
    fn staged_writes_remain_bound_to_immutable_base() {
        let directory = tempdir().unwrap();
        let store = ObjectStore::open(directory.path()).unwrap();
        let mut typed = object(serde_json::json!({"mutable": 1, "fixed": 1}));
        typed.schema.fields = BTreeMap::from([
            (
                "mutable".into(),
                FieldSchema {
                    field_type: FieldType::Integer,
                    required: true,
                    description: String::new(),
                    default: None,
                    example: None,
                    constraints: Vec::new(),
                    read_only: false,
                    read_only_when: None,
                },
            ),
            (
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
            ),
        ]);
        let snapshot = SourceSnapshot {
            catalog_hash: "catalog".into(),
            catalog_generation: 1,
            objects: BTreeMap::from([("demo".into(), typed)]),
        };
        store.capture_snapshot(snapshot, true).unwrap();
        store.create_workspace(Some("edit"), None).unwrap();

        store
            .stage_pointer("edit", "demo", "/mutable", Value::from(2))
            .unwrap();
        store
            .stage_pointer("edit", "demo", "/mutable", Value::from(3))
            .unwrap();
        assert!(
            store
                .stage_pointer("edit", "demo", "/fixed", Value::from(2))
                .is_err()
        );

        let workspace = store.read_workspace("edit").unwrap();
        let value = store.workspace_object(&workspace, "demo").unwrap().value;
        assert_eq!(value.pointer("/mutable"), Some(&Value::from(3)));
        assert_eq!(value.pointer("/fixed"), Some(&Value::from(1)));
    }

    #[test]
    fn stale_opened_object_version_rejects_before_staging() {
        let directory = tempdir().unwrap();
        let store = ObjectStore::open(directory.path()).unwrap();
        let view = store
            .capture_snapshot(
                SourceSnapshot {
                    catalog_hash: "fixture".into(),
                    catalog_generation: 1,
                    objects: BTreeMap::from([("demo".into(), object(serde_json::json!({"x":1})))]),
                },
                true,
            )
            .unwrap();
        store.create_workspace(Some("edit"), None).unwrap();
        let expected = &view.objects["demo"];
        let winner = store
            .stage_pointer_checked("edit", "demo", "/x", expected, Value::from(2))
            .unwrap();
        let error = store
            .stage_pointer_checked("edit", "demo", "/x", expected, Value::from(3))
            .unwrap_err();
        assert!(error.downcast_ref::<StageConflict>().is_some());
        let workspace = store.read_workspace("edit").unwrap();
        assert_eq!(workspace.staged_objects["demo"], winner);
        assert_eq!(
            store.workspace_object(&workspace, "demo").unwrap().value["x"],
            Value::from(2)
        );
    }

    #[test]
    fn oversized_store_documents_and_manifests_fail_before_decode() {
        let directory = tempdir().unwrap();
        let store = ObjectStore::open(directory.path()).unwrap();
        let hash = "a".repeat(64);
        File::create(store.object_path(&hash))
            .unwrap()
            .set_len(MAX_DOCUMENT_BYTES + 1)
            .unwrap();
        assert!(store.read_object(&hash).is_err());
        File::create(store.workspace_path("fixture"))
            .unwrap()
            .set_len(MAX_MANIFEST_BYTES + 1)
            .unwrap();
        assert!(store.read_workspace("fixture").is_err());
    }
}
