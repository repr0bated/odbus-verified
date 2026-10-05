//! Runtime for schema-generated node contracts. This module adds no backend control plane.
#![allow(dead_code)]
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::model::SideEffect;
use crate::mounted::{MountedDiagnostic, MountedNamespace};
use crate::namespace::{MountIdentity, Namespace, NodeKind, ProjectionTarget, encode_component};
use crate::store::ObjectStore;
use crate::typed_bindings::*;

const MAX_DOCUMENT: u64 = 16 * 1024 * 1024;
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

/// Binding-defined conditions preserve source I/O codes as diagnostics, not a bare errno API.
#[derive(Debug, thiserror::Error)]
pub enum NodeError {
    #[error("binding contract: {0}")]
    Contract(String),
    #[error("{operation}: {message} ({kind:?}, errno={errno:?})")]
    Io {
        operation: &'static str,
        condition: FsCondition,
        kind: std::io::ErrorKind,
        errno: Option<i32>,
        message: String,
    },
    #[error("JSON decode: {0}")]
    Decode(String),
    #[error("schema validation: {0}")]
    Schema(String),
    #[error("operation denied: {0:?}")]
    Denied(JsonValue),
    #[error("terminal operation diagnostic ({condition:?}): {diagnostic:?}")]
    Terminal {
        condition: FsCondition,
        diagnostic: OperationDiagnostic,
        raw: JsonValue,
    },
    #[error("call completion is uncertain; do not retry automatically: {0}")]
    Ambiguous(String),
    #[error("route unavailable: {0}")]
    Unavailable(String),
}
impl NodeError {
    pub fn decode(error: serde_json::Error) -> Self {
        Self::Decode(error.to_string())
    }
    fn io(operation: &'static str, error: std::io::Error) -> Self {
        Self::Io {
            operation,
            condition: io_condition(error.raw_os_error()),
            kind: error.kind(),
            errno: error.raw_os_error(),
            message: error.to_string(),
        }
    }
    fn backing(operation: &'static str, error: anyhow::Error) -> Self {
        if let Some(io) = error.downcast_ref::<std::io::Error>() {
            return Self::Io {
                operation,
                condition: io_condition(io.raw_os_error()),
                kind: io.kind(),
                errno: io.raw_os_error(),
                message: error.to_string(),
            };
        }
        Self::Contract(format!("{operation}: {error:#}"))
    }
}
fn ensure(condition: bool, message: impl Into<String>) -> Result<(), NodeError> {
    if condition {
        Ok(())
    } else {
        Err(NodeError::Contract(message.into()))
    }
}
pub fn validate_identifier(value: &str) -> Result<(), NodeError> {
    // Upstream permits arbitrary UTF-8 keys, including empty keys. Use sites
    // validate membership against the pinned catalog/method/field, not a new grammar.
    let _ = value;
    Ok(())
}
pub fn validate_session(value: &str) -> Result<(), NodeError> {
    ensure(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "invalid lungs session selector",
    )
}
pub fn validate_plugin(value: &str) -> Result<(), NodeError> {
    ensure(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
        "invalid canonical captured plugin id",
    )
}
pub fn validate_schema_hash(value: &str) -> Result<(), NodeError> {
    ensure(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f')),
        "schema hash must be 64 lowercase hexadecimal characters",
    )
}
pub fn validate_hash(value: &str) -> Result<(), NodeError> {
    ensure(
        value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()),
        "hash must be 64 ASCII hexadecimal characters",
    )
}
pub fn validate_store_name(value: &str) -> Result<(), NodeError> {
    ensure(
        !value.is_empty()
            && value.len() <= 128
            && value != "."
            && value != ".."
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
        "invalid store name",
    )
}
pub fn validate_pointer(value: &str) -> Result<(), NodeError> {
    ensure(
        value.is_empty() || value.starts_with('/'),
        "JSON pointer must be empty or begin with /",
    )?;
    for part in value.split('/') {
        let mut chars = part.chars();
        while let Some(c) = chars.next() {
            if c == '~' {
                ensure(
                    matches!(chars.next(), Some('0' | '1')),
                    "invalid JSON pointer escape (binding restriction)",
                )?;
            }
        }
    }
    Ok(())
}
pub fn validate_root(value: &str) -> Result<(), NodeError> {
    ensure(
        value.starts_with('/')
            && !value.contains('\0')
            && Path::new(value).components().all(|c| {
                matches!(
                    c,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                )
            }),
        "root must be an absolute path without traversal (binding restriction)",
    )
}

/// Immutable owned decode, not a mapped or zero-copy blob.
#[derive(Debug, Clone)]
pub struct BlobRef<T> {
    value: T,
}
impl<T> BlobRef<T> {
    pub fn get(&self) -> &T {
        &self.value
    }
}
/// Immutable owned public snapshot from canonical committed SHM or FUSE.
#[derive(Debug, Clone)]
pub struct ShmHandle<T> {
    value: T,
}
impl<T> ShmHandle<T> {
    pub fn get(&self) -> &T {
        &self.value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleKind {
    Directory,
    StaticJson,
    ObjectHashFile,
    FrozenValue,
    WorkspaceField,
    LiveValue,
    CallFile,
    Symlink,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub name: String,
    pub kind: HandleKind,
    owner: u64,
    key: String,
    plugin: Option<PluginId>,
    method: Option<MethodId>,
    pointer: Option<JsonPointer>,
    capability: Option<CapabilityId>,
    dependency: Option<DependencyId>,
}
impl DirectoryEntry {
    pub fn plugin(&self) -> Option<&PluginId> {
        self.plugin.as_ref()
    }
    pub fn method(&self) -> Option<&MethodId> {
        self.method.as_ref()
    }
    pub fn pointer(&self) -> Option<&JsonPointer> {
        self.pointer.as_ref()
    }
    pub fn capability(&self) -> Option<&CapabilityId> {
        self.capability.as_ref()
    }
    pub fn dependency(&self) -> Option<&DependencyId> {
        self.dependency.as_ref()
    }
}
/// Resolve declared relative aliases only within the pinned projection inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticLocator {
    pub raw_target: String,
    pub projection_components: Option<Vec<String>>,
    pub external: bool,
    /// Resolved only within this pinned inventory; pass it to TypedFilesystem::node.
    pub target_entry: Option<DirectoryEntry>,
}
/// Method-specific validated input; only its originating declaration accepts it.
#[derive(Debug, Clone)]
pub struct ValidatedArguments {
    plugin: String,
    method: String,
    declaration: MethodDecl,
    value: JsonValue,
}

#[derive(Clone)]
struct Descriptor {
    key: String,
    name: String,
    parent: String,
    kind: HandleKind,
    plugin: Option<String>,
    method: Option<String>,
    pointer: String,
    target: Option<String>,
}
struct Mount {
    path: PathBuf,
    root: File,
    mount_id: String,
    metadata: serde_json::Value,
    view: ViewManifest,
    workspace: bool,
    calls: Option<MountedNamespace>,
}
impl Mount {
    fn open(
        path: &Path,
        session: Option<&str>,
        workspace: Option<&str>,
    ) -> Result<Self, NodeError> {
        let mode = session
            .map(|session| MountIdentity::HostSession {
                session: session.to_owned(),
            })
            .unwrap_or(MountIdentity::Unbound);
        Self::open_for_mode(path, &mode, workspace)
    }

    fn open_for_mode(
        path: &Path,
        expected_mode: &MountIdentity,
        workspace: Option<&str>,
    ) -> Result<Self, NodeError> {
        let session = match expected_mode {
            MountIdentity::HostSession { session } => Some(session.as_str()),
            _ => None,
        };
        let path =
            std::fs::canonicalize(path).map_err(|e| NodeError::io("canonicalize mount", e))?;
        let root = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(|e| NodeError::io("open mount", e))?;
        let mount_id = fd_mount_id(&root)?;
        verify_fuse(&mount_id, &path)?;
        let metadata = read_json(open_relative(&root, ".3tchedfs.json", false)?)?;
        let view: ViewManifest =
            serde_json::from_value(read_json(open_relative(&root, "view.json", false)?)?)
                .map_err(NodeError::decode)?;
        ensure(
            metadata["format"] == "3tchedfs/1",
            "unsupported mount format",
        )?;
        let instance = metadata["instance_uuid"]
            .as_str()
            .ok_or_else(|| NodeError::Contract("mount instance UUID is missing".into()))?;
        uuid::Uuid::parse_str(instance).map_err(|error| {
            NodeError::Contract(format!("invalid mount instance UUID: {error}"))
        })?;
        for key in [
            "format",
            "view_id",
            "source_catalog_hash",
            "source_catalog_generation",
        ] {
            ensure(
                metadata[key] == serde_json::to_value(&view).map_err(NodeError::decode)?[key],
                format!("mount/view {key} mismatch"),
            )?;
        }
        ensure(
            metadata["object_count"].as_u64() == Some(view.objects.len() as u64),
            "mount catalog count mismatch",
        )?;
        let is_workspace = workspace.is_some();
        if let Some(id) = workspace {
            ensure(
                metadata["workspace_id"].as_str() == Some(id),
                "workspace declaration mismatch",
            )?;
        } else {
            ensure(
                metadata["workspace_id"].is_null(),
                "live mount must be an identity view",
            )?;
            ensure(
                !matches!(expected_mode, MountIdentity::Unbound),
                "live factory requires an admitted mount mode",
            )?;
            ensure(
                metadata["mount_identity"]
                    == serde_json::to_value(expected_mode).map_err(NodeError::decode)?,
                "mount identity mode differs from typed factory",
            )?;
            if session.is_some() {
                ensure(
                    metadata["session_id"].as_str() == session,
                    "identity session mismatch",
                )?;
            } else {
                ensure(
                    metadata["session_id"].is_null(),
                    "guest/credential mount must not declare a host session",
                )?;
            }
        }
        let calls = if is_workspace {
            None
        } else {
            Some(
                MountedNamespace::open_for_mode(&path, expected_mode)
                    .map_err(|e| NodeError::backing("pin mounted calls", e))?,
            )
        };
        let mount = Self {
            path,
            root,
            mount_id,
            metadata,
            view,
            workspace: is_workspace,
            calls,
        };
        mount.verify()?;
        Ok(mount)
    }
    fn verify(&self) -> Result<(), NodeError> {
        ensure(
            fd_mount_id(&self.root)? == self.mount_id,
            "pinned mount FD changed",
        )?;
        verify_fuse(&self.mount_id, &self.path)?;
        let current = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&self.path)
            .map_err(|e| NodeError::io("verify mount root", e))?;
        let original = self
            .root
            .metadata()
            .map_err(|e| NodeError::io("pinned root metadata", e))?;
        let present = current
            .metadata()
            .map_err(|e| NodeError::io("current root metadata", e))?;
        ensure(
            fd_mount_id(&current)? == self.mount_id
                && original.dev() == present.dev()
                && original.ino() == present.ino(),
            "mount instance replaced",
        )?;
        ensure(
            read_json(open_relative(&self.root, ".3tchedfs.json", false)?)? == self.metadata,
            "mount identity/catalog declaration changed",
        )?;
        ensure(
            read_json(open_relative(&self.root, "view.json", false)?)?
                == serde_json::to_value(&self.view).map_err(NodeError::decode)?,
            "pinned view changed",
        )?;
        if let Some(calls) = &self.calls {
            calls
                .verify()
                .map_err(|e| NodeError::backing("verify call context", e))?;
        }
        Ok(())
    }
    fn read(&self, key: &str) -> Result<Vec<u8>, NodeError> {
        self.verify()?;
        let result = read_bounded(open_relative(&self.root, key, false)?, MAX_DOCUMENT);
        self.verify()?;
        result
    }
}
enum Backend {
    Direct {
        store: ObjectStore,
        view: ViewManifest,
    },
    DirectShm {
        store: ObjectStore,
        view: ViewManifest,
        catalog: PathBuf,
        state: PathBuf,
    },
    Fuse(Mount),
}
struct Context {
    owner: u64,
    backend: Backend,
    inventory: RwLock<BTreeMap<String, Descriptor>>,
    gate: Mutex<()>,
}
/// Factories pin a mode and locator. No route fallback occurs after a denial or missing node.
pub struct TypedFilesystem {
    context: Arc<Context>,
}
impl TypedFilesystem {
    pub fn verify_schema_pin(
        &self,
        plugin: &PluginId,
        schema: &SchemaHash,
    ) -> Result<(), NodeError> {
        let handle = self.handle(&format!(
            "plugins/{}/schema.json",
            encode_component(plugin.as_str())
        ))?;
        let object = handle.object()?;
        ensure(
            object.source_schema_hash == schema.as_str(),
            "specialized plugin schema pin differs from captured declaration",
        )?;
        handle.verify_policy()
    }
    pub fn invoke_typed<A: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self,
        plugin: PluginId,
        method: MethodId,
        schema: SchemaHash,
        args: &A,
        confirm_side_effects: bool,
    ) -> Result<R, NodeError> {
        self.verify_schema_pin(&plugin, &schema)?;
        let mut call = self.call_file(plugin.clone(), method)?;
        let bytes = crate::store::render_arguments(args)
            .map_err(|e| NodeError::backing("serialize bounded typed arguments", e))?;
        let args =
            JsonValue::from_json(serde_json::from_slice(&bytes).map_err(NodeError::decode)?)?;
        let validated = call.0.validate_arguments(args)?;
        let receipt = call.0.invoke(validated, confirm_side_effects)?;
        self.verify_schema_pin(&plugin, &schema).map_err(|e| {
            NodeError::Ambiguous(format!(
                "call completed but current schema pin verification failed: {e}"
            ))
        })?;
        serde_json::from_value(receipt.result.result).map_err(|e| {
            NodeError::Ambiguous(format!(
                "validated domain output does not decode as specialized result: {e}"
            ))
        })
    }
    pub fn plugin_state_typed<R: serde::de::DeserializeOwned>(
        &self,
        plugin: PluginId,
        schema: SchemaHash,
    ) -> Result<R, NodeError> {
        self.verify_schema_pin(&plugin, &schema)?;
        let value = self
            .plugin_live_value(plugin.clone())?
            .0
            .live_value()?
            .value
            .into_json()?;
        self.verify_schema_pin(&plugin, &schema)?;
        serde_json::from_value(value).map_err(NodeError::decode)
    }
    pub fn plugin_live_value(&self, plugin: PluginId) -> Result<LiveValue, NodeError> {
        let handle = self.handle(&format!(
            "plugins/{}/value.json",
            encode_component(plugin.as_str())
        ))?;
        ensure(
            handle.kind() == HandleKind::LiveValue,
            "factory does not expose live public state",
        )?;
        Ok(LiveValue(handle))
    }
    /// Fresh locked public-state reads, with active sealed policy checked before
    /// and after each read. This is an owned decode, not POSIX shm_open or mmap.
    pub fn direct_shm(
        root: StoreRoot,
        view: ViewId,
        catalog: CatalogRoot,
        state: StateDirectory,
    ) -> Result<Self, NodeError> {
        for member in ["objects", "views", "workspaces", "snapshots"] {
            ensure(
                Path::new(root.as_str()).join(member).is_dir(),
                "direct SHM requires an existing capture store",
            )?;
        }
        let store = ObjectStore::open(root.as_str())
            .map_err(|e| NodeError::backing("open pinned capture store", e))?;
        let view = store
            .read_view(view.as_str())
            .map_err(|e| NodeError::backing("read pinned view", e))?;
        let catalog = PathBuf::from(catalog.as_str());
        let state = PathBuf::from(state.as_str());
        let namespace = Namespace::build_with_catalog(
            &store,
            ProjectionTarget::View(view.view_id.clone()),
            Some(&state),
            false,
            &catalog,
        )
        .map_err(|e| NodeError::backing("build direct public namespace", e))?;
        Ok(Self::new(
            Backend::DirectShm {
                store,
                view,
                catalog,
                state,
            },
            inventory_for_namespace(&namespace)?,
        ))
    }
    /// Open only an existing capture store. This route never reads live SHM or dispatches.
    pub fn direct_blob_at(root: StoreRoot, view: ViewId) -> Result<Self, NodeError> {
        for member in ["objects", "views", "workspaces", "snapshots"] {
            ensure(
                Path::new(root.as_str()).join(member).is_dir(),
                "direct blob factory requires an existing complete capture store",
            )?;
        }
        let store = ObjectStore::open(root.as_str())
            .map_err(|error| NodeError::backing("open existing capture store", error))?;
        Self::direct_blob(store, view)
    }

    pub fn direct_blob(store: ObjectStore, view: ViewId) -> Result<Self, NodeError> {
        let view = store
            .read_view(view.as_str())
            .map_err(|e| NodeError::backing("read pinned view", e))?;
        for plugin in view.objects.keys() {
            validate_plugin(plugin)?;
        }
        let namespace =
            Namespace::build(&store, ProjectionTarget::View(view.view_id.clone()), None)
                .map_err(|e| NodeError::backing("build frozen namespace", e))?;
        let mut inventory = BTreeMap::new();
        for node in namespace.nodes.values() {
            let key = node.path.trim_start_matches('/').to_owned();
            let kind = match node.kind {
                NodeKind::Directory => HandleKind::Directory,
                NodeKind::Symlink(_) => HandleKind::Symlink,
                _ => classify(&key, false, false),
            };
            let target = match &node.kind {
                NodeKind::Symlink(value) => Some(value.clone()),
                _ => None,
            };
            let mut desc = descriptor(key, kind)?;
            desc.target = target;
            inventory.insert(desc.key.clone(), desc);
        }
        for plugin in view.objects.keys() {
            let object = store
                .object_for_view(&view, plugin)
                .and_then(|object| object.public_projection())
                .map_err(|e| NodeError::backing("derive frozen locators", e))?;
            let prefix = format!("plugins/{}/data", encode_component(plugin));
            let mut values = BTreeMap::new();
            projection_values(&object.value, &prefix, "", &mut values);
            for (path, pointer) in values {
                if let Some(desc) = inventory.get_mut(&path) {
                    desc.pointer = pointer;
                }
            }
        }
        Ok(Self::new(Backend::Direct { store, view }, inventory))
    }
    pub fn via_fuse(root: MountRoot, session: SessionId) -> Result<Self, NodeError> {
        Self::mounted(Mount::open(
            Path::new(root.as_str()),
            Some(session.as_str()),
            None,
        )?)
    }
    pub fn via_workspace_fuse(root: MountRoot, workspace: WorkspaceId) -> Result<Self, NodeError> {
        Self::mounted(Mount::open(
            Path::new(root.as_str()),
            None,
            Some(workspace.as_str()),
        )?)
    }
    pub fn via_guest_fuse(root: MountRoot) -> Result<Self, NodeError> {
        Self::mounted(Mount::open_for_mode(
            Path::new(root.as_str()),
            &MountIdentity::GuestKernel,
            None,
        )?)
    }
    pub fn via_credential_fuse(root: MountRoot) -> Result<Self, NodeError> {
        Self::mounted(Mount::open_for_mode(
            Path::new(root.as_str()),
            &MountIdentity::Credential,
            None,
        )?)
    }
    fn mounted(mount: Mount) -> Result<Self, NodeError> {
        let inventory = inventory_for_mount(&mount)?;
        Ok(Self::new(Backend::Fuse(mount), inventory))
    }
    fn new(backend: Backend, inventory: BTreeMap<String, Descriptor>) -> Self {
        Self {
            context: Arc::new(Context {
                owner: NEXT_OWNER.fetch_add(1, Ordering::Relaxed),
                backend,
                inventory: RwLock::new(inventory),
                gate: Mutex::new(()),
            }),
        }
    }
    pub fn root(&self) -> Directory {
        Directory(self.handle("").expect("factory always constructs root"))
    }
    pub fn node(&self, entry: DirectoryEntry) -> Result<TypedNode, NodeError> {
        ensure(
            entry.owner == self.context.owner,
            "entry belongs to a different pinned factory",
        )?;
        let handle = self.handle(&entry.key)?;
        ensure(
            handle.kind() == entry.kind,
            "directory entry changed since enumeration",
        )?;
        Ok(TypedNode::from_handle(handle))
    }
    pub fn call_file(&self, plugin: PluginId, method: MethodId) -> Result<CallFile, NodeError> {
        let key = format!(
            "plugins/{}/operations/{}/call",
            encode_component(plugin.as_str()),
            encode_component(method.as_str())
        );
        let handle = self.handle(&key)?;
        ensure(
            handle.kind() == HandleKind::CallFile,
            "not a declared call node",
        )?;
        Ok(CallFile(handle))
    }
    fn handle(&self, key: &str) -> Result<NodeHandle, NodeError> {
        let desc = self
            .context
            .inventory
            .read()
            .get(key)
            .cloned()
            .ok_or_else(|| NodeError::Unavailable("node absent from pinned namespace".into()))?;
        Ok(NodeHandle {
            context: self.context.clone(),
            desc,
        })
    }
}
fn inventory_for_namespace(
    namespace: &Namespace,
) -> Result<BTreeMap<String, Descriptor>, NodeError> {
    namespace
        .nodes
        .values()
        .map(|node| {
            let key = node.path.trim_start_matches('/').to_owned();
            let kind = match &node.kind {
                NodeKind::Directory => HandleKind::Directory,
                NodeKind::Symlink(_) => HandleKind::Symlink,
                NodeKind::LiveFile { .. } => HandleKind::LiveValue,
                NodeKind::CallFile { .. } => HandleKind::CallFile,
                _ => classify(&key, false, false),
            };
            let mut desc = descriptor(key, kind)?;
            match &node.kind {
                NodeKind::LiveFile { plugin_id, pointer }
                | NodeKind::File(crate::namespace::FileSource::Value { plugin_id, pointer }) => {
                    desc.plugin = Some(plugin_id.clone());
                    desc.pointer = pointer.clone();
                }
                NodeKind::Symlink(target) => desc.target = Some(target.clone()),
                _ => {}
            }
            Ok((desc.key.clone(), desc))
        })
        .collect()
}
fn inventory_for_mount(mount: &Mount) -> Result<BTreeMap<String, Descriptor>, NodeError> {
    let mut inventory = BTreeMap::new();
    inventory.insert(
        String::new(),
        descriptor(String::new(), HandleKind::Directory)?,
    );
    walk(mount, &mount.root, "", &mut inventory, 0, None)?;
    // Determine actual workspace write exposure from canonical pinned schema and captured values.
    if mount.workspace {
        for plugin in mount.view.objects.keys() {
            let key = format!("plugins/{}/object.blob.json", encode_component(plugin));
            let object: TypedObject =
                serde_json::from_slice(&mount.read(&key)?).map_err(NodeError::decode)?;
            ensure(
                object.plugin_id == *plugin && object.schema.name == *plugin,
                "workspace object plugin mismatch",
            )?;
            let mut values = BTreeMap::new();
            projection_values(
                &object.value,
                &format!("plugins/{}/data", encode_component(plugin)),
                "",
                &mut values,
            );
            for (path, pointer) in values {
                if let Some(desc) = inventory.get_mut(&path) {
                    desc.pointer = pointer;
                }
            }
            for desc in inventory.values_mut().filter(|d| {
                d.plugin.as_deref() == Some(plugin) && d.kind == HandleKind::FrozenValue
            }) {
                // FUSE reports only existing workspace targets with write mode; backend revalidates schema on put.
                let file = open_relative(&mount.root, &desc.key, false)?;
                if file
                    .metadata()
                    .map_err(|e| NodeError::io("workspace mode", e))?
                    .mode()
                    & 0o222
                    != 0
                {
                    desc.kind = HandleKind::WorkspaceField;
                }
            }
        }
    }
    mount.verify()?;
    Ok(inventory)
}
pub struct NodeHandle {
    context: Arc<Context>,
    desc: Descriptor,
}
impl NodeHandle {
    fn refresh_mounted_data(&self, mount: &Mount) -> Result<(), NodeError> {
        let Some(base) = data_base(&self.desc.key) else {
            return mount.verify();
        };
        let prefix = format!("{base}/");
        let read = || -> Result<BTreeMap<String, Descriptor>, NodeError> {
            mount.verify()?;
            let mut fresh = BTreeMap::new();
            walk(
                mount,
                &open_relative(&mount.root, &base, false)?,
                &base,
                &mut fresh,
                0,
                Some(&base),
            )?;
            let plugin = self
                .desc
                .plugin
                .as_deref()
                .ok_or_else(|| NodeError::Contract("data locator has no plugin".into()))?;
            let value = serde_json::from_slice(
                &mount.read(&format!("plugins/{}/value.json", encode_component(plugin)))?,
            )
            .map_err(NodeError::decode)?;
            let mut pointers = BTreeMap::new();
            projection_values(&value, &base, "", &mut pointers);
            for (path, pointer) in pointers {
                if let Some(desc) = fresh.get_mut(&path) {
                    desc.pointer = pointer;
                }
            }
            mount.verify()?;
            Ok(fresh)
        };
        let fresh = read();
        let mut inventory = self.context.inventory.write();
        inventory.retain(|key, _| !key.starts_with(&prefix));
        let fresh = fresh?;
        ensure(
            inventory.len().saturating_add(fresh.len()) <= 100_000,
            "projection exceeds binding node-count bound",
        )?;
        inventory.extend(fresh);
        Ok(())
    }
    fn direct_public_namespace(
        &self,
        store: &ObjectStore,
        view: &ViewManifest,
        catalog: &Path,
        state: &Path,
    ) -> Result<Namespace, NodeError> {
        let mut namespace = Namespace::build_with_catalog(
            store,
            ProjectionTarget::View(view.view_id.clone()),
            Some(state),
            false,
            catalog,
        )
        .map_err(|e| NodeError::backing("build direct public locators", e))?;
        let parts: Vec<_> = self.desc.key.split('/').collect();
        if parts.first() != Some(&"plugins") || parts.get(2) != Some(&"data") {
            return Ok(namespace);
        }
        let base = parts[..3].join("/");
        let path = format!("/{base}");
        let ino = namespace
            .nodes
            .values()
            .find(|node| node.path == path)
            .ok_or_else(|| NodeError::Unavailable("public data directory is absent".into()))?
            .ino;
        let refreshed = namespace.refresh_for_inode(store, ino);
        let prefix = format!("{base}/");
        let fresh = inventory_for_namespace(&namespace)?;
        let mut inventory = self.context.inventory.write();
        // Retain unrelated structural locators. Failed reads remove stale
        // descendants and propagate the authoritative error, never an empty pass.
        inventory.retain(|key, _| !key.starts_with(&prefix));
        let additions: Vec<_> = fresh
            .into_iter()
            .filter(|(key, _)| key.starts_with(&prefix))
            .collect();
        ensure(
            inventory.len().saturating_add(additions.len()) <= 100_000,
            "projection exceeds binding node-count bound",
        )?;
        inventory.extend(additions);
        drop(inventory);
        refreshed.map_err(|e| NodeError::backing("refresh addressed public data", e))?;
        Ok(namespace)
    }
    fn verify_policy(&self) -> Result<(), NodeError> {
        let (view, catalog) = match &self.context.backend {
            Backend::Direct { .. } => return Ok(()),
            Backend::DirectShm { view, catalog, .. } => (view, catalog.as_path()),
            Backend::Fuse(mount) if !mount.workspace => (
                &mount.view,
                Path::new(
                    mount.metadata["declared_catalog_dir"]
                        .as_str()
                        .unwrap_or(crate::source::DEFAULT_CATALOG_DIR),
                ),
            ),
            Backend::Fuse(_) => return Ok(()),
        };
        let Some(plugin) = &self.desc.plugin else {
            return Ok(());
        };
        let object = self.object()?;
        let schemas = BTreeMap::from([(plugin.clone(), object.schema)]);
        let hashes = BTreeMap::from([(plugin.clone(), object.source_schema_hash)]);
        crate::source::verify_pinned_catalog(
            catalog,
            &view.source_catalog_hash,
            view.source_catalog_generation,
            &hashes,
            &schemas,
        )
        .map_err(|e| NodeError::backing("verify active sealed public policy", e))
    }
    pub fn kind(&self) -> HandleKind {
        self.desc.kind
    }
    pub fn require_route(&self, route: &str) -> Result<(), NodeError> {
        ensure(
            matches!(
                (&self.context.backend, route),
                (Backend::Direct { .. }, "direct-blob")
                    | (Backend::Fuse(_), "via-fuse")
                    | (Backend::DirectShm { .. }, "direct-shm")
            ),
            "node route differs from requested implementation",
        )
    }
    fn verify(&self) -> Result<(), NodeError> {
        ensure(
            self.context
                .inventory
                .read()
                .get(&self.desc.key)
                .is_some_and(|d| d.kind == self.desc.kind),
            "node was removed or changed class",
        )?;
        match &self.context.backend {
            Backend::Direct { store, view, .. } | Backend::DirectShm { store, view, .. } => {
                store
                    .read_view(&view.view_id)
                    .map_err(|e| NodeError::backing("verify frozen view", e))?;
                Ok(())
            }
            Backend::Fuse(mount) => mount.verify(),
        }
    }
    fn bytes(&self) -> Result<Vec<u8>, NodeError> {
        self.verify()?;
        match &self.context.backend {
            Backend::Fuse(mount) => mount.read(&self.desc.key),
            Backend::DirectShm {
                store,
                view,
                catalog,
                state,
            } => {
                let namespace = self.direct_public_namespace(store, view, catalog, state)?;
                let node = namespace
                    .nodes
                    .values()
                    .find(|node| node.path.trim_start_matches('/') == self.desc.key)
                    .ok_or_else(|| {
                        NodeError::Unavailable("public locator is absent or unavailable".to_owned())
                    })?;
                namespace
                    .file_bytes(store, node)
                    .map_err(|e| NodeError::backing("read direct public locator", e))
            }
            Backend::Direct { store, view } => {
                // Build only this operation's temporary canonical projection. Never
                // retain decoded payloads or byte buffers for later reads.
                let namespace =
                    Namespace::build(store, ProjectionTarget::View(view.view_id.clone()), None)
                        .map_err(|error| {
                            NodeError::backing("render fresh frozen projection", error)
                        })?;
                let node = namespace
                    .nodes
                    .values()
                    .find(|n| n.path.trim_start_matches('/') == self.desc.key)
                    .ok_or_else(|| {
                        NodeError::Contract("node no longer in frozen namespace".into())
                    })?;
                if let NodeKind::File(_) = &node.kind {
                    namespace
                        .file_bytes(store, node)
                        .map_err(|error| NodeError::backing("read fresh frozen file", error))
                } else {
                    Err(NodeError::Contract(
                        "node does not have a byte payload".into(),
                    ))
                }
            }
        }
    }
    pub fn list(&self) -> Result<Vec<DirectoryEntry>, NodeError> {
        ensure(self.kind() == HandleKind::Directory, "expected directory")?;
        self.verify()?;
        if let Backend::Fuse(mount) = &self.context.backend {
            if mount.workspace {
                *self.context.inventory.write() = inventory_for_mount(mount)?;
            } else {
                self.refresh_mounted_data(mount)?;
            }
            self.verify()?;
        }
        if let Backend::DirectShm {
            store,
            view,
            catalog,
            state,
        } = &self.context.backend
        {
            self.direct_public_namespace(store, view, catalog, state)?;
            self.verify()?;
        }
        self.context
            .inventory
            .read()
            .values()
            .filter(|d| !d.key.is_empty() && d.parent == self.desc.key)
            .map(|d| typed_entry(d, self.context.owner))
            .collect::<Result<Vec<_>, _>>()
    }
    fn object(&self) -> Result<TypedObject, NodeError> {
        let plugin = self
            .desc
            .plugin
            .as_deref()
            .ok_or_else(|| NodeError::Contract("node has no plugin locator".into()))?;
        match &self.context.backend {
            Backend::Direct { store, view, .. } => store
                .object_for_view(view, plugin)
                .and_then(|object| object.public_projection())
                .map_err(|e| NodeError::backing("read pinned object", e)),
            Backend::DirectShm {
                store,
                view,
                catalog,
                state,
            } => {
                let namespace = Namespace::build_with_catalog(
                    store,
                    ProjectionTarget::View(view.view_id.clone()),
                    Some(state),
                    false,
                    catalog,
                )
                .map_err(|e| NodeError::backing("refresh direct public object", e))?;
                let node = namespace
                    .nodes
                    .values()
                    .find(|node| {
                        node.path
                            == format!("/plugins/{}/object.blob.json", encode_component(plugin))
                    })
                    .ok_or_else(|| NodeError::Unavailable("public object is absent".to_owned()))?;
                serde_json::from_slice(
                    &namespace
                        .file_bytes(store, node)
                        .map_err(|e| NodeError::backing("read direct public object", e))?,
                )
                .map_err(NodeError::decode)
            }
            Backend::Fuse(mount) => {
                if let Some(calls) = &mount.calls {
                    calls
                        .object(plugin)
                        .map_err(|e| NodeError::backing("read mounted object", e))
                } else {
                    serde_json::from_slice(&mount.read(&format!(
                        "plugins/{}/object.blob.json",
                        encode_component(plugin)
                    ))?)
                    .map_err(NodeError::decode)
                }
            }
        }
    }
    pub fn static_payload(&self) -> Result<StaticPayload, NodeError> {
        ensure(
            self.kind() == HandleKind::StaticJson,
            "expected static JSON node",
        )?;
        self.verify()?;
        let value: serde_json::Value =
            serde_json::from_slice(&self.bytes()?).map_err(NodeError::decode)?;
        macro_rules! decode {
            ($variant:ident,$ty:ty) => {
                Ok(StaticPayload::$variant(
                    serde_json::from_value::<$ty>(value).map_err(NodeError::decode)?,
                ))
            };
        }
        match self.desc.name.as_str() {
            ".3tchedfs.json" => decode!(MountMetadata, MountMetadata),
            "view.json" => decode!(View, ViewManifest),
            "workspace.json" => decode!(Workspace, Workspace),
            "object.blob.json" => decode!(Object, TypedObject),
            "schema.json" => decode!(Schema, PluginSchema),
            "capabilities.json" => decode!(Capabilities, Capabilities),
            "input.schema.json" => Ok(StaticPayload::Arguments(JsonValue::from_json(value)?)),
            "output.schema.json" => Ok(StaticPayload::Returns(JsonValue::from_json(value)?)),
            "metadata.json" => decode!(MethodMetadata, MethodDecl),
            _ => Err(NodeError::Contract("unclassified static payload".into())),
        }
    }
    pub fn object_hash(&self) -> Result<ObjectHash, NodeError> {
        ensure(
            self.kind() == HandleKind::ObjectHashFile,
            "expected hash node",
        )?;
        let bytes = self.bytes()?;
        let text = std::str::from_utf8(&bytes).map_err(|e| NodeError::Decode(e.to_string()))?;
        let hash = ObjectHash::new(text.trim())?;
        let plugin = self
            .desc
            .plugin
            .as_deref()
            .ok_or_else(|| NodeError::Contract("missing hash plugin".into()))?;
        match &self.context.backend {
            Backend::Direct { store, view, .. } | Backend::DirectShm { store, view, .. } => {
                ensure(
                    view.objects.get(plugin).map(String::as_str) == Some(hash.as_str()),
                    "pinned object hash mismatch",
                )?;
                store
                    .read_object(hash.as_str())
                    .map_err(|e| NodeError::backing("verify hash object", e))?;
            }
            Backend::Fuse(mount) if !mount.workspace => {
                ensure(
                    mount.view.objects.get(plugin).map(String::as_str) == Some(hash.as_str()),
                    "mounted object hash differs from pinned view",
                )?;
            }
            Backend::Fuse(_) => { /* Workspace object hashes may differ from their immutable base view. */
            }
        }
        Ok(hash)
    }
    pub fn frozen_value(&self) -> Result<BlobRef<JsonValue>, NodeError> {
        ensure(
            matches!(
                self.kind(),
                HandleKind::FrozenValue | HandleKind::WorkspaceField
            ),
            "expected frozen/workspace value",
        )?;
        let value = match &self.context.backend {
            Backend::Direct { .. } => {
                let object = self.object()?;
                if object.value_scope == crate::model::StateValueScope::SchemaOnly {
                    return Err(NodeError::Unavailable(
                        "frozen state is unavailable in schema-only capture".to_owned(),
                    ));
                }
                resolve_frozen_value(&object.value, &self.desc.pointer)?.clone()
            }
            Backend::Fuse(_) => {
                serde_json::from_slice(&self.bytes()?).map_err(NodeError::decode)?
            }
            Backend::DirectShm { .. } => {
                return Err(NodeError::Unavailable(
                    "direct SHM does not expose frozen values".to_owned(),
                ));
            }
        };
        Ok(BlobRef {
            value: JsonValue::from_json(value)?,
        })
    }
    pub fn live_value(&self) -> Result<ShmHandle<JsonValue>, NodeError> {
        ensure(
            self.kind() == HandleKind::LiveValue,
            "expected mounted live value",
        )?;
        ensure(
            matches!(
                self.context.backend,
                Backend::Fuse(_) | Backend::DirectShm { .. }
            ),
            "live public state requires direct SHM or mounted identity route",
        )?;
        self.verify_policy()?;
        let value = serde_json::from_slice(&self.bytes()?).map_err(NodeError::decode)?;
        self.verify_policy()?;
        Ok(ShmHandle {
            value: JsonValue::from_json(value)?,
        })
    }
    pub fn put_workspace(&mut self, value: JsonValue) -> Result<BlobRef<JsonValue>, NodeError> {
        ensure(
            self.kind() == HandleKind::WorkspaceField,
            "expected writable workspace field",
        )?;
        let Backend::Fuse(mount) = &self.context.backend else {
            return Err(NodeError::Unavailable(
                "workspace write only through mounted workspace".into(),
            ));
        };
        ensure(mount.workspace, "write requires workspace mode")?;
        let _guard = self
            .context
            .gate
            .lock()
            .map_err(|_| NodeError::Contract("factory operation lock poisoned".into()))?;
        mount.verify()?;
        validate_json_tree(&value, 0, &mut 0)?;
        let bytes = crate::store::render_arguments(&value)
            .map_err(|e| NodeError::backing("serialize bounded workspace input", e))?;
        let mut file = open_relative(&mount.root, &self.desc.key, true)?;
        file.write_all(&bytes)
            .map_err(|e| NodeError::Ambiguous(format!("partial workspace write: {e}")))?;
        // fsync may fail after a partial or committed write. Never resubmit automatically.
        file.sync_all()
            .map_err(|e| NodeError::Ambiguous(format!("workspace fsync: {e}")))?;
        drop(file);
        mount.verify()?;
        *self.context.inventory.write() = inventory_for_mount(mount).map_err(|e| {
            NodeError::Ambiguous(format!(
                "workspace committed but inventory refresh failed: {e}"
            ))
        })?;
        let object = self.object().map_err(|e| {
            NodeError::Ambiguous(format!("workspace committed but object read failed: {e}"))
        })?;
        let observed = JsonValue::from_json(
            object
                .value
                .pointer(&self.desc.pointer)
                .cloned()
                .ok_or_else(|| {
                    NodeError::Ambiguous("workspace committed but pointer disappeared".into())
                })?,
        )?;
        ensure(
            observed == value,
            "workspace resulting value differs from submitted value",
        )?;
        Ok(BlobRef { value: observed })
    }
    fn declaration(&self) -> Result<MethodDecl, NodeError> {
        let method = self
            .desc
            .method
            .as_deref()
            .ok_or_else(|| NodeError::Contract("missing method locator".into()))?;
        self.object()?
            .schema
            .methods
            .get(method)
            .cloned()
            .ok_or_else(|| NodeError::Contract("method absent from pinned schema".into()))
    }
    pub fn validate_arguments(
        &self,
        arguments: JsonValue,
    ) -> Result<ValidatedArguments, NodeError> {
        ensure(self.kind() == HandleKind::CallFile, "expected call file")?;
        self.verify_policy()?;
        validate_json_tree(&arguments, 0, &mut 0)?;
        let decl = self.declaration()?;
        self.verify_policy()?;
        let value = arguments.clone().into_json()?;
        crate::dispatch::validate_arguments(&decl, &value)
            .map_err(|e| NodeError::Schema(e.to_string()))?;
        crate::store::render_arguments(&value)
            .map_err(|e| NodeError::backing("measure bounded arguments", e))?;
        Ok(ValidatedArguments {
            plugin: self.desc.plugin.clone().unwrap(),
            method: self.desc.method.clone().unwrap(),
            declaration: decl,
            value: arguments,
        })
    }
    pub fn invoke(
        &mut self,
        arguments: ValidatedArguments,
        confirm_side_effects: bool,
    ) -> Result<CallReceipt, NodeError> {
        ensure(self.kind() == HandleKind::CallFile, "expected call file")?;
        let Backend::Fuse(mount) = &self.context.backend else {
            return Err(NodeError::Unavailable(
                "call requires mounted identity route".into(),
            ));
        };
        let calls = mount.calls.as_ref().ok_or_else(|| {
            NodeError::Unavailable("workspace cannot dispatch identity calls".into())
        })?;
        let _guard = self
            .context
            .gate
            .lock()
            .map_err(|_| NodeError::Contract("factory operation lock poisoned".into()))?;
        let decl = self.declaration()?;
        self.verify_policy()?;
        ensure(
            Some(arguments.plugin.as_str()) == self.desc.plugin.as_deref()
                && Some(arguments.method.as_str()) == self.desc.method.as_deref()
                && arguments.declaration == decl,
            "validated arguments belong to a different pinned method",
        )?;
        ensure(
            decl.side_effect == SideEffect::Read || confirm_side_effects,
            "mutation requires explicit side-effect confirmation",
        )?;
        let path = &self.desc.key;
        let value = calls
            .invoke(path, arguments.value.into_json()?, confirm_side_effects)
            .map_err(|error| {
                if let Some(denial) = error.downcast_ref::<MountedDiagnostic>() {
                    terminal_diagnostic(denial.0.clone())
                } else {
                    NodeError::Ambiguous(format!("{error:#}"))
                }
            })?;
        self.verify_policy().map_err(|error| {
            NodeError::Ambiguous(format!(
                "terminal receipt received but active sealed pin changed: {error}"
            ))
        })?;
        // MountedNamespace validates the canonical engine receipt, its terminal
        // audit proof and the pinned method-specific return contract before decode.
        serde_json::from_value(value)
            .map_err(|error| NodeError::Ambiguous(format!("receipt decode: {error}")))
    }
    pub fn symlink_target(&self) -> Result<SemanticLocator, NodeError> {
        ensure(self.kind() == HandleKind::Symlink, "expected symlink")?;
        self.verify()?;
        let target = self
            .desc
            .target
            .clone()
            .ok_or_else(|| NodeError::Contract("missing symlink target".into()))?;
        let projection = semantic_projection_target(&self.desc.parent, &target);
        Ok(SemanticLocator {
            raw_target: target,
            external: projection.is_none(),
            target_entry: projection
                .as_ref()
                .and_then(|parts| self.context.inventory.read().get(&parts.join("/")).cloned())
                .map(|desc| typed_entry(&desc, self.context.owner))
                .transpose()?,
            projection_components: projection,
        })
    }
}
fn semantic_projection_target(parent: &str, target: &str) -> Option<Vec<String>> {
    if target.starts_with('/') || target.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = parent
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    for part in target.split('/') {
        match part {
            ".." => {
                parts.pop()?;
            }
            "." => {}
            "" => return None,
            part => parts.push(part.to_owned()),
        }
    }
    Some(parts)
}
fn classify(key: &str, workspace: bool, identity: bool) -> HandleKind {
    let name = key.rsplit('/').next().unwrap_or("");
    let parts: Vec<_> = key.split('/').collect();
    if parts.len() == 3 && parts[0] == "plugins" && name == "object.hash" {
        HandleKind::ObjectHashFile
    } else if parts.len() == 5 && parts[0] == "plugins" && parts[2] == "operations" && identity {
        match name {
            "call" => HandleKind::CallFile,
            _ => HandleKind::StaticJson,
        }
    } else if parts.len() >= 3
        && parts[0] == "plugins"
        && (parts[2] == "data" || name == "value.json")
    {
        if identity && !workspace {
            HandleKind::LiveValue
        } else {
            HandleKind::FrozenValue
        }
    } else {
        HandleKind::StaticJson
    }
}
fn descriptor(key: String, kind: HandleKind) -> Result<Descriptor, NodeError> {
    let parts: Vec<_> = key.split('/').collect();
    let name = parts.last().copied().unwrap_or("").to_owned();
    let parent = key.rsplit_once('/').map(|v| v.0).unwrap_or("").to_owned();
    let plugin = if parts.len() >= 2 && (parts[0] == "plugins" || parts[0] == "operations") {
        Some(decode_component(parts[1])?)
    } else if parts.len() >= 3 && parts[0] == "relationships" && parts[1] == "dependencies" {
        Some(decode_component(parts[2])?)
    } else {
        None
    };
    let method = if parts.len() >= 4 && parts[0] == "plugins" && parts[2] == "operations" {
        Some(decode_component(parts[3])?)
    } else if parts.len() >= 3 && parts[0] == "operations" {
        Some(decode_component(parts[2])?)
    } else {
        None
    };
    let mut pointer = String::new();
    if parts.len() > 3 && parts[0] == "plugins" && parts[2] == "data" {
        for component in &parts[3..] {
            if *component == ".value.json" {
                continue;
            }
            let decoded = decode_component(component)?;
            // Canonical array names are padded indexes; a JSON object key that looks like one is
            // resolved below against the actual value rather than guessed from name alone.
            pointer.push('/');
            pointer.push_str(&decoded.replace('~', "~0").replace('/', "~1"));
        }
    }
    Ok(Descriptor {
        key,
        name,
        parent,
        kind,
        plugin,
        method,
        pointer,
        target: None,
    })
}
fn decode_component(value: &str) -> Result<String, NodeError> {
    if value == "%" {
        return Ok(String::new());
    }
    if value == "%2E2E" {
        return Ok("..".into());
    }
    if value == "%2E" {
        return Ok(".".into());
    }
    let bytes = value.as_bytes();
    let mut decoded = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            ensure(i + 2 < bytes.len(), "incomplete encoded component")?;
            let digits = std::str::from_utf8(&bytes[i + 1..i + 3])
                .map_err(|e| NodeError::Decode(e.to_string()))?;
            decoded.push(
                u8::from_str_radix(digits, 16).map_err(|e| NodeError::Decode(e.to_string()))?,
            );
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(decoded).map_err(|e| NodeError::Decode(e.to_string()))
}
fn data_base(key: &str) -> Option<String> {
    let parts: Vec<_> = key.split('/').collect();
    (parts.first() == Some(&"plugins") && parts.get(2) == Some(&"data"))
        .then(|| parts[..3].join("/"))
}
fn is_data_root(key: &str) -> bool {
    data_base(key).as_deref() == Some(key)
}
fn walk(
    mount: &Mount,
    dir: &File,
    prefix: &str,
    inventory: &mut BTreeMap<String, Descriptor>,
    depth: usize,
    selected_data: Option<&str>,
) -> Result<(), NodeError> {
    ensure(depth <= 128, "projection exceeds binding depth bound")?;
    for entry in std::fs::read_dir(format!("/proc/self/fd/{}", dir.as_raw_fd()))
        .map_err(|e| NodeError::io("enumerate mounted directory", e))?
    {
        let entry = entry.map_err(|e| NodeError::io("read directory entry", e))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| NodeError::Contract("projection name is not UTF-8".into()))?;
        ensure(
            !name.contains('/') && name != "." && name != "..",
            "invalid directory entry",
        )?;
        let key = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        let ty = entry
            .file_type()
            .map_err(|e| NodeError::io("entry type", e))?;
        let kind = if ty.is_dir() {
            HandleKind::Directory
        } else if ty.is_symlink() {
            HandleKind::Symlink
        } else {
            classify(&key, mount.workspace, !mount.workspace)
        };
        let mut desc = descriptor(key.clone(), kind)?;
        if ty.is_symlink() {
            let target = std::fs::read_link(entry.path())
                .map_err(|e| NodeError::io("read semantic symlink", e))?;
            desc.target = Some(
                target
                    .into_os_string()
                    .into_string()
                    .map_err(|_| NodeError::Contract("symlink target is not UTF-8".into()))?,
            );
        }
        inventory.insert(key.clone(), desc);
        ensure(
            inventory.len() <= 100_000,
            "projection exceeds binding node-count bound",
        )?;
        if ty.is_dir()
            && (mount.workspace || !is_data_root(&key) || selected_data == Some(key.as_str()))
        {
            walk(
                mount,
                &open_relative(&mount.root, &key, false)?,
                &key,
                inventory,
                depth + 1,
                selected_data,
            )?;
        }
    }
    Ok(())
}
fn fd_mount_id(file: &File) -> Result<String, NodeError> {
    let text = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", file.as_raw_fd()))
        .map_err(|e| NodeError::io("read FD mount identity", e))?;
    text.lines()
        .find_map(|l| l.strip_prefix("mnt_id:").map(str::trim))
        .map(str::to_owned)
        .ok_or_else(|| NodeError::Contract("FD mount identity unavailable".into()))
}
fn verify_fuse(id: &str, path: &Path) -> Result<(), NodeError> {
    let info = std::fs::read_to_string("/proc/self/mountinfo")
        .map_err(|e| NodeError::io("read mount table", e))?;
    let line = info
        .lines()
        .find(|l| l.split_whitespace().next() == Some(id))
        .ok_or_else(|| NodeError::Contract("pinned mount detached".into()))?;
    let (left, right) = line
        .split_once(" - ")
        .ok_or_else(|| NodeError::Contract("malformed mount record".into()))?;
    let declared = left
        .split_whitespace()
        .nth(4)
        .ok_or_else(|| NodeError::Contract("missing mountpoint".into()))?;
    let declared = declared
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\");
    ensure(
        Path::new(&declared) == path,
        "root must be the actual pinned mountpoint",
    )?;
    ensure(
        right.split_whitespace().next() == Some("fuse.3tchedFS"),
        "root must be a fuse.3tchedFS namespace",
    )
}
fn open_relative(root: &File, relative: &str, write: bool) -> Result<File, NodeError> {
    ensure(!relative.is_empty(), "empty file locator")?;
    let parts: Vec<_> = relative.split('/').collect();
    let mount = fd_mount_id(root)?;
    let mut parent = root
        .try_clone()
        .map_err(|e| NodeError::io("clone pinned root", e))?;
    for (index, part) in parts.iter().enumerate() {
        ensure(
            !part.is_empty() && *part != "." && *part != "..",
            "unsafe projected component",
        )?;
        let name = CString::new(part.as_bytes())
            .map_err(|_| NodeError::Contract("NUL in projected component".into()))?;
        let last = index + 1 == parts.len();
        let flags = libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | if last {
                if write {
                    libc::O_RDWR | libc::O_TRUNC
                } else {
                    libc::O_RDONLY
                }
            } else {
                libc::O_RDONLY | libc::O_DIRECTORY
            };
        // SAFETY: parent is an owned live descriptor, name is NUL terminated, no creation flags.
        let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(NodeError::io(
                "open projected node",
                std::io::Error::last_os_error(),
            ));
        }
        // SAFETY: successful openat returns a newly owned descriptor.
        parent = unsafe { File::from_raw_fd(fd) };
        ensure(
            fd_mount_id(&parent)? == mount,
            "projected locator crosses mount boundary",
        )?;
    }
    Ok(parent)
}
fn read_bounded(mut file: File, limit: u64) -> Result<Vec<u8>, NodeError> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let count = file
            .read(&mut chunk)
            .map_err(|e| NodeError::io("read projected payload", e))?;
        if count == 0 {
            break;
        }
        ensure(
            (bytes.len() as u64)
                .checked_add(count as u64)
                .is_some_and(|size| size <= limit),
            "projected payload exceeds binding bound",
        )?;
        bytes.try_reserve_exact(count).map_err(|e| {
            NodeError::Contract(format!("bounded payload allocation unavailable: {e}"))
        })?;
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(bytes)
}
fn read_json(file: File) -> Result<serde_json::Value, NodeError> {
    serde_json::from_slice(&read_bounded(file, MAX_DOCUMENT)?).map_err(NodeError::decode)
}
fn io_condition(errno: Option<i32>) -> FsCondition {
    match errno {
        Some(libc::ENOENT) => FsCondition::MissingNode,
        Some(libc::EROFS) => FsCondition::ReadOnly,
        Some(libc::EINVAL) => FsCondition::InvalidValue,
        Some(libc::EACCES | libc::EPERM) => FsCondition::PermissionDenied,
        Some(libc::EIO) => FsCondition::BackendUnavailable,
        Some(libc::EISDIR) => FsCondition::IsDirectory,
        Some(libc::ESTALE) => FsCondition::StaleVersion,
        Some(libc::EFBIG) => FsCondition::TooLarge,
        Some(libc::ENOMEM) => FsCondition::ResourceExhausted,
        Some(libc::EBUSY) => FsCondition::Busy,
        Some(libc::EBADF) => FsCondition::InvalidHandle,
        Some(libc::EAGAIN) => FsCondition::InFlight,
        _ => FsCondition::OtherIo,
    }
}
fn terminal_diagnostic(value: serde_json::Value) -> NodeError {
    let raw = match JsonValue::from_json(value.clone()) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let diagnostic: OperationDiagnostic = match serde_json::from_value(value) {
        Ok(value) => value,
        Err(error) => {
            return NodeError::Ambiguous(format!("malformed terminal diagnostic: {error}"));
        }
    };
    let condition = match diagnostic.stage {
        DiagnosticStage::Parse | DiagnosticStage::Validate => FsCondition::InvalidValue,
        DiagnosticStage::Credential | DiagnosticStage::Register | DiagnosticStage::Call => {
            FsCondition::PermissionDenied
        }
        DiagnosticStage::Transport => FsCondition::BackendUnavailable,
        DiagnosticStage::Contract => FsCondition::InvalidValue,
        DiagnosticStage::Limits => FsCondition::TooLarge,
        DiagnosticStage::Handle => FsCondition::InvalidHandle,
        DiagnosticStage::Commit | DiagnosticStage::Fsync | DiagnosticStage::OutcomeUnknown => {
            FsCondition::BackendUnavailable
        }
    };
    NodeError::Terminal {
        condition,
        diagnostic,
        raw,
    }
}

fn validate_json_tree(value: &JsonValue, depth: usize, count: &mut usize) -> Result<(), NodeError> {
    *count += 1;
    ensure(
        depth <= 128 && *count <= 100_000,
        "JSON exceeds binding depth/node bound",
    )?;
    match value {
        JsonValue::Array(values) => {
            for value in values {
                validate_json_tree(value, depth + 1, count)?;
            }
        }
        JsonValue::Object(values) => {
            for value in values.values() {
                validate_json_tree(value, depth + 1, count)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn resolve_frozen_value<'a>(
    root: &'a serde_json::Value,
    pointer: &str,
) -> Result<&'a serde_json::Value, NodeError> {
    if pointer.is_empty() {
        return Ok(root);
    }
    let mut value = root;
    for segment in pointer.split('/').skip(1) {
        let segment = segment.replace("~1", "/").replace("~0", "~");
        value = match value {
            serde_json::Value::Object(map) => map.get(&segment),
            serde_json::Value::Array(array) => {
                segment.parse::<usize>().ok().and_then(|i| array.get(i))
            }
            _ => None,
        }
        .ok_or_else(|| NodeError::Contract("frozen value locator absent".into()))?;
    }
    Ok(value)
}

fn projection_values(
    value: &serde_json::Value,
    path: &str,
    pointer: &str,
    values: &mut BTreeMap<String, String>,
) {
    match value {
        serde_json::Value::Object(object) => {
            values.insert(path.into(), pointer.into());
            if !pointer.is_empty() {
                values.insert(format!("{path}/.value.json"), pointer.into());
            }
            for (key, value) in object {
                let path = format!("{path}/{}", encode_component(key));
                let pointer = format!("{pointer}/{}", key.replace('~', "~0").replace('/', "~1"));
                if value.is_object() || value.is_array() {
                    projection_values(value, &path, &pointer, values);
                } else {
                    values.insert(path, pointer);
                }
            }
        }
        serde_json::Value::Array(array) => {
            values.insert(path.into(), pointer.into());
            values.insert(format!("{path}/.value.json"), pointer.into());
            for (index, value) in array.iter().enumerate() {
                let path = format!("{path}/{index:08}");
                let pointer = format!("{pointer}/{index}");
                if value.is_object() || value.is_array() {
                    projection_values(value, &path, &pointer, values);
                } else {
                    values.insert(path, pointer);
                }
            }
        }
        _ => {
            values.insert(path.into(), pointer.into());
        }
    }
}

fn typed_entry(desc: &Descriptor, owner: u64) -> Result<DirectoryEntry, NodeError> {
    let components: Vec<_> = desc.key.split('/').collect();
    let pointer = if desc.plugin.is_some()
        && components.len() >= 3
        && (components[2] == "data" || components[2] == "value.json")
    {
        Some(JsonPointer::new(desc.pointer.clone())?)
    } else {
        None
    };
    Ok(DirectoryEntry {
        name: desc.name.clone(),
        kind: desc.kind,
        owner,
        key: desc.key.clone(),
        plugin: desc.plugin.clone().map(PluginId::new).transpose()?,
        method: desc.method.clone().map(MethodId::new).transpose()?,
        pointer,
        capability: if components.len() >= 2 && components[0] == "capabilities" {
            Some(CapabilityId::new(decode_component(components[1])?)?)
        } else {
            None
        },
        dependency: if components.len() >= 4
            && components[0] == "relationships"
            && components[1] == "dependencies"
        {
            Some(DependencyId::new(decode_component(components[3])?)?)
        } else {
            None
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn mounted_static_inventory_skips_data_until_explicitly_selected() {
        let directory = tempfile::tempdir().unwrap();
        let base = directory.path().join("plugins/demo/data");
        std::fs::create_dir_all(base.join("hidden")).unwrap();
        std::fs::write(base.join("hidden/value"), b"1").unwrap();
        let operation = directory.path().join("plugins/demo/operations/read");
        std::fs::create_dir_all(&operation).unwrap();
        std::fs::write(operation.join("call"), b"").unwrap();
        let mount = Mount {
            path: directory.path().to_path_buf(),
            root: File::open(directory.path()).unwrap(),
            mount_id: String::new(),
            metadata: serde_json::json!({}),
            view: ViewManifest {
                format: "3tchedfs/1".into(),
                view_id: String::new(),
                source_catalog_hash: String::new(),
                source_catalog_generation: 1,
                parent_view: None,
                objects: BTreeMap::new(),
            },
            workspace: false,
            calls: None,
        };
        let mut static_inventory = BTreeMap::new();
        walk(&mount, &mount.root, "", &mut static_inventory, 0, None).unwrap();
        assert!(static_inventory.contains_key("plugins/demo/operations/read/call"));
        assert!(static_inventory.contains_key("plugins/demo/data"));
        assert!(!static_inventory.contains_key("plugins/demo/data/hidden"));
        let mut selected = BTreeMap::new();
        walk(
            &mount,
            &File::open(base).unwrap(),
            "plugins/demo/data",
            &mut selected,
            0,
            Some("plugins/demo/data"),
        )
        .unwrap();
        assert!(selected.contains_key("plugins/demo/data/hidden/value"));
        assert!(!selected.contains_key("plugins/demo/operations/read/call"));
    }
    #[test]
    fn direct_public_data_is_discovered_on_demand_and_failed_reads_remove_stale_locators() {
        let directory = tempfile::tempdir().unwrap();
        let sealed: op_state_store::PluginSchema = serde_json::from_value(serde_json::json!({
            "name":"demo","version":"1","description":"fixture",
            "fields":{"items":{"field_type":{"array":"integer"},"required":true}},
            "public_state":{"paths":["/items"]}
        }))
        .unwrap();
        let schema = crate::model::from_sealed(sealed).unwrap();
        let source = crate::source::tests::fixture_catalog(directory.path(), &schema);
        let store = ObjectStore::open(directory.path().join("store")).unwrap();
        let view = store
            .capture_snapshot(source.capture_schema_only().unwrap(), false)
            .unwrap();
        let filesystem = TypedFilesystem::direct_shm(
            StoreRoot::new(directory.path().join("store").to_str().unwrap()).unwrap(),
            ViewId::new(view.view_id).unwrap(),
            CatalogRoot::new(source.catalog_dir.to_str().unwrap()).unwrap(),
            StateDirectory::new(source.state_dir.to_str().unwrap()).unwrap(),
        )
        .unwrap();
        assert!(filesystem.handle("plugins/demo/data/items").is_err());
        let publish = |items: Value| {
            let bytes = serde_json::to_vec(&serde_json::json!({"items":items})).unwrap();
            op_core::projection_shm::update_projection_in(
                source.state_dir.to_str().unwrap(),
                "demo",
                |_| Ok(Some(bytes)),
            )
            .unwrap();
        };
        publish(serde_json::json!([1]));
        let data = filesystem.handle("plugins/demo/data").unwrap();
        assert!(!data.list().unwrap().is_empty());
        let leaf = filesystem
            .handle("plugins/demo/data/items/00000000")
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&leaf.bytes().unwrap()).unwrap(),
            serde_json::json!(1)
        );
        filesystem.root().0.list().unwrap();
        assert!(
            filesystem
                .handle("plugins/demo/data/items/00000000")
                .is_ok()
        );
        publish(serde_json::json!([2]));
        assert_eq!(
            serde_json::from_slice::<Value>(&leaf.bytes().unwrap()).unwrap(),
            serde_json::json!(2)
        );
        publish(serde_json::json!([]));
        assert!(leaf.bytes().is_err());
        assert!(
            filesystem
                .handle("plugins/demo/data/items/00000000")
                .is_err()
        );
        std::fs::write(source.state_dir.join(".manifest.json"), b"invalid").unwrap();
        assert!(data.list().is_err());
        assert!(filesystem.handle("plugins/demo/data/items").is_err());
        assert!(filesystem.root().0.list().is_ok());
    }
    #[test]
    fn terminal_diagnostics_preserve_stage_and_exact_document() {
        for (stage, expected) in [
            ("parse", FsCondition::InvalidValue),
            ("validate", FsCondition::InvalidValue),
            ("credential", FsCondition::PermissionDenied),
            ("register", FsCondition::PermissionDenied),
            ("call", FsCondition::PermissionDenied),
            ("transport", FsCondition::BackendUnavailable),
        ] {
            let document = serde_json::json!({"plugin":"demo","method":"Read","stage":stage,"error":"original error"});
            let NodeError::Terminal {
                condition,
                diagnostic,
                raw,
            } = terminal_diagnostic(document.clone())
            else {
                panic!("expected typed terminal diagnostic");
            };
            assert_eq!(condition, expected);
            assert_eq!(diagnostic.error, "original error");
            assert_eq!(raw.into_json().unwrap(), document);
        }
        assert!(matches!(
            terminal_diagnostic(serde_json::json!({"error":"unknown"})),
            NodeError::Ambiguous(_)
        ));
    }
    #[test]
    fn nested_json_bound_rejects_before_serialization_or_submission() {
        let mut value = JsonValue::Null;
        for _ in 0..130 {
            value = JsonValue::Array(vec![value]);
        }
        assert!(validate_json_tree(&value, 0, &mut 0).is_err());
    }
}
