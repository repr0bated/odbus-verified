use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsString;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{FieldSchema, FieldType, PluginSchema, StateValueScope};
use crate::store::{ObjectStore, ViewManifest, Workspace};

pub const ROOT_INO: u64 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MountIdentity {
    HostSession {
        session: String,
    },
    GuestKernel,
    Credential,
    #[default]
    Unbound,
}

#[derive(Debug, Clone)]
pub enum FileSource {
    MountMetadata,
    View,
    Workspace,
    Object { plugin_id: String },
    Schema { plugin_id: String },
    Value { plugin_id: String, pointer: String },
    Capabilities { plugin_id: String },
    Hash { plugin_id: String },
    MethodInput { plugin_id: String, method: String },
    MethodOutput { plugin_id: String, method: String },
    MethodMetadata { plugin_id: String, method: String },
    ThreetchedSchema,
    ThreetchedInput { command: String },
    ThreetchedOutput { command: String },
    ThreetchedMetadata { command: String },
}

#[derive(Debug, Clone)]
pub enum ProjectionTarget {
    View(String),
    Workspace(String),
}

#[derive(Debug, Clone)]
pub struct Namespace {
    pub nodes: HashMap<u64, Node>,
    pub children: HashMap<u64, BTreeMap<OsString, u64>>,
    pub target: ProjectionTarget,
    /// SHM state directory for live reads (`/dev/shm/opdbus/state`).
    /// Only used for view mounts; workspace mounts use frozen COW objects.
    pub state_dir: Option<std::path::PathBuf>,
    /// Lungs mounts belong to one identity and expose a `call` file per
    /// method, so a write is that identity's canonical call.
    pub lungs: bool,
    pub mount_identity: MountIdentity,
    pub instance_uuid: Option<String>,
    pub local_call_owner_uid: Option<u32>,
    pub allowed_operator_uid: Option<u32>,
    pub unavailable_plugins: BTreeSet<String>,
    pub live_generation: Option<u64>,
    pub declared_catalog_dir: std::path::PathBuf,
    pub source_schema_hashes: BTreeMap<String, String>,
    pub threetched_surface: Option<Arc<crate::threetched_surface::ThreetchedSurface>>,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub ino: u64,
    pub parent: u64,
    pub name: OsString,
    pub path: String,
    pub kind: NodeKind,
    pub write_target: Option<WriteTarget>,
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    Directory,
    File(FileSource),
    /// A file whose content is read live from the SHM state tree at open time.
    /// Only used for read-only view mounts. `plugin_id` is a catalog-known
    /// name (validated during capture); `pointer` is a JSON pointer into the
    /// plugin's state object. `schema` is pinned from the sealed blob.
    LiveFile {
        plugin_id: String,
        pointer: String,
    },
    Symlink(String),
    /// Lungs control file. A complete JSON write, flushed on close, is
    /// validated against the sealed method schema and dispatched as the
    /// mount's identity. Nothing is stored in the file itself.
    CallFile {
        plugin_id: String,
        method: String,
    },
}

#[derive(Debug, Clone)]
pub struct WriteTarget {
    pub workspace_id: String,
    pub plugin_id: String,
    pub pointer: String,
    pub expected_object_hash: String,
}

impl Namespace {
    pub fn build(
        store: &ObjectStore,
        target: ProjectionTarget,
        state_dir: Option<&std::path::Path>,
    ) -> Result<Self> {
        Self::build_with(store, target, state_dir, false)
    }

    /// A live view for one identity, with a lungs `call` file per method.
    pub fn build_lungs(
        store: &ObjectStore,
        target: ProjectionTarget,
        state_dir: &std::path::Path,
    ) -> Result<Self> {
        if !matches!(target, ProjectionTarget::View(_)) {
            bail!("a lungs mount projects a pinned view, not a workspace");
        }
        Self::build_with(store, target, Some(state_dir), true)
    }

    pub fn build_with(
        store: &ObjectStore,
        target: ProjectionTarget,
        state_dir: Option<&std::path::Path>,
        lungs: bool,
    ) -> Result<Self> {
        Self::build_with_catalog(
            store,
            target,
            state_dir,
            lungs,
            std::path::Path::new(crate::source::DEFAULT_CATALOG_DIR),
        )
    }

    pub fn build_with_catalog(
        store: &ObjectStore,
        target: ProjectionTarget,
        state_dir: Option<&std::path::Path>,
        lungs: bool,
        catalog_dir: &std::path::Path,
    ) -> Result<Self> {
        let (view, workspace) = match &target {
            ProjectionTarget::View(id) => (store.read_view(id)?, None),
            ProjectionTarget::Workspace(id) => {
                let workspace = store.read_workspace(id)?;
                let view = store.read_view(&workspace.base_view)?;
                (view, Some(workspace))
            }
        };
        // View mounts read live state from SHM; workspace mounts use frozen COW.
        let live = workspace.is_none() && state_dir.is_some();
        let mut builder =
            NamespaceBuilder::new(target.clone(), state_dir.map(|p| p.to_path_buf()), lungs);
        builder.namespace.declare_catalog_dir(catalog_dir)?;
        builder.object_hashes = workspace
            .as_ref()
            .map(|workspace| {
                let mut hashes = view.objects.clone();
                hashes.extend(workspace.staged_objects.clone());
                hashes
            })
            .unwrap_or_else(|| view.objects.clone());
        // Live topology is discovered on demand for the addressed plugin.
        // Startup and static traversal never acquire the state transaction lock.
        let live_values: BTreeMap<String, Value> = BTreeMap::new();
        builder.add_root_metadata(&view, workspace.as_ref())?;

        let plugins = builder.add_dir(ROOT_INO, "plugins")?;
        let operations = builder.add_dir(ROOT_INO, "operations")?;
        let relationships = builder.add_dir(ROOT_INO, "relationships")?;
        let dependencies = builder.add_dir(relationships, "dependencies")?;
        let capabilities = builder.add_dir(ROOT_INO, "capabilities")?;

        for plugin_id in view.objects.keys() {
            let raw_object = match &workspace {
                Some(workspace) => store.workspace_object(workspace, plugin_id)?,
                None => store.object_for_view(&view, plugin_id)?,
            };
            let object = if live {
                raw_object
            } else {
                raw_object.public_projection()?
            };
            builder
                .namespace
                .source_schema_hashes
                .insert(plugin_id.clone(), object.source_schema_hash.clone());
            let plugin_name = encode_component(plugin_id);
            let plugin_dir = builder.add_dir(plugins, &plugin_name)?;
            builder.add_file(
                plugin_dir,
                "object.blob.json",
                FileSource::Object {
                    plugin_id: plugin_id.clone(),
                },
                None,
            )?;
            builder.add_file(
                plugin_dir,
                "schema.json",
                FileSource::Schema {
                    plugin_id: plugin_id.clone(),
                },
                None,
            )?;
            if live {
                builder.add_live_file(plugin_dir, "value.json", plugin_id, "")?;
            } else {
                builder.add_file(
                    plugin_dir,
                    "value.json",
                    FileSource::Value {
                        plugin_id: plugin_id.clone(),
                        pointer: String::new(),
                    },
                    None,
                )?;
            }
            builder.add_file(
                plugin_dir,
                "capabilities.json",
                FileSource::Capabilities {
                    plugin_id: plugin_id.clone(),
                },
                None,
            )?;
            builder.add_file(
                plugin_dir,
                "object.hash",
                FileSource::Hash {
                    plugin_id: plugin_id.clone(),
                },
                None,
            )?;

            let data_dir = builder.add_dir(plugin_dir, "data")?;
            let workspace_id = workspace
                .as_ref()
                .map(|workspace| workspace.workspace_id.clone());
            let value = if live {
                live_values.get(plugin_id)
            } else if object.value_scope == StateValueScope::SchemaOnly {
                None
            } else {
                Some(&object.value)
            };
            if let Some(value) = value {
                builder.add_value_children(
                    data_dir,
                    plugin_id,
                    "",
                    value,
                    Some(&object.schema),
                    None,
                    workspace_id.as_deref(),
                    !object.schema.tags.iter().any(|tag| tag == "immutable"),
                    live,
                )?;
            }

            let methods_dir = builder.add_dir(plugin_dir, "operations")?;
            let root_methods_dir = builder.add_dir(operations, &plugin_name)?;
            for (method_name, method) in &object.schema.methods {
                let encoded = encode_component(method_name);
                let method_dir = builder.add_dir(methods_dir, &encoded)?;
                builder.add_file(
                    method_dir,
                    "input.schema.json",
                    FileSource::MethodInput {
                        plugin_id: plugin_id.clone(),
                        method: method_name.clone(),
                    },
                    None,
                )?;
                if method.returns.is_some() {
                    builder.add_file(
                        method_dir,
                        "output.schema.json",
                        FileSource::MethodOutput {
                            plugin_id: plugin_id.clone(),
                            method: method_name.clone(),
                        },
                        None,
                    )?;
                }
                builder.add_file(
                    method_dir,
                    "metadata.json",
                    FileSource::MethodMetadata {
                        plugin_id: plugin_id.clone(),
                        method: method_name.clone(),
                    },
                    None,
                )?;
                if lungs {
                    builder.add_lungs_files(method_dir, plugin_id, method_name)?;
                }
                builder.add_symlink(
                    root_methods_dir,
                    &encoded,
                    format!("../../plugins/{plugin_name}/operations/{encoded}"),
                )?;

                if let Some(capability) = &method.required_capability {
                    let capability_dir =
                        builder.add_dir_idempotent(capabilities, &encode_component(capability))?;
                    builder.add_symlink(
                        capability_dir,
                        &format!("{plugin_name}.{}", encode_component(method_name)),
                        format!("../../plugins/{plugin_name}/operations/{encoded}"),
                    )?;
                }
            }

            let relation_dir = builder.add_dir(dependencies, &plugin_name)?;
            for dependency in &object.schema.dependencies {
                builder.add_symlink(
                    relation_dir,
                    &encode_component(dependency),
                    format!("../../../plugins/{}", encode_component(dependency)),
                )?;
            }
        }

        Ok(builder.finish())
    }

    pub fn node(&self, ino: u64) -> Option<&Node> {
        self.nodes.get(&ino)
    }

    pub fn lookup(&self, parent: u64, name: &std::ffi::OsStr) -> Option<&Node> {
        let ino = self.children.get(&parent)?.get(name)?;
        self.nodes.get(ino)
    }

    pub fn children(&self, parent: u64) -> Option<&BTreeMap<OsString, u64>> {
        self.children.get(&parent)
    }

    /// Public mount declaration, never a credential or inferred identity.
    /// The caller supplies only the socket mount's declared session selector.
    pub fn declare_socket_session(&mut self, session: &str) -> Result<()> {
        self.declare_mount_mode(
            MountIdentity::HostSession {
                session: session.to_owned(),
            },
            None,
        )
    }

    pub fn declare_mount_mode(
        &mut self,
        identity: MountIdentity,
        instance_uuid: Option<&str>,
    ) -> Result<()> {
        if let MountIdentity::HostSession { session } = &identity {
            if session.is_empty()
                || session.len() > 128
                || !session
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                bail!("invalid declared mount session");
            }
        }
        if let Some(instance) = instance_uuid {
            uuid::Uuid::parse_str(instance).context("invalid mount instance UUID")?;
        }
        self.mount_identity = identity;
        self.instance_uuid = instance_uuid.map(str::to_owned);
        Ok(())
    }

    pub fn refresh(&self, store: &ObjectStore) -> Result<Self> {
        let mut fresh = Self::build_with_catalog(
            store,
            self.target.clone(),
            self.state_dir.as_deref(),
            self.lungs,
            &self.declared_catalog_dir,
        )?;
        fresh.mount_identity = self.mount_identity.clone();
        fresh.instance_uuid = self.instance_uuid.clone();
        fresh.local_call_owner_uid = self.local_call_owner_uid;
        fresh.allowed_operator_uid = self.allowed_operator_uid;
        #[cfg(test)]
        if let Some(surface) = self.threetched_surface.clone() {
            fresh.graft_shared_threetched(surface)?;
        }
        Ok(fresh)
    }

    /// Add the `threetched` tree beside the sealed catalog objects.
    /// The bytes come from a captured binary surface, not from a plugin blob.
    #[cfg(test)]
    pub fn graft_threetched(
        &mut self,
        surface: crate::threetched_surface::ThreetchedSurface,
    ) -> Result<()> {
        self.graft_shared_threetched(Arc::new(surface))
    }

    #[cfg(test)]
    fn graft_shared_threetched(
        &mut self,
        surface: Arc<crate::threetched_surface::ThreetchedSurface>,
    ) -> Result<()> {
        let plugins = *self
            .children
            .get(&ROOT_INO)
            .and_then(|children| children.get(&OsString::from("plugins")))
            .context("plugins directory is missing")?;
        let operations = *self
            .children
            .get(&ROOT_INO)
            .and_then(|children| children.get(&OsString::from("operations")))
            .context("operations directory is missing")?;
        let lungs = self.lungs;
        let mut builder = NamespaceBuilder::rehydrate(self.clone());
        builder.attach_threetched(plugins, operations, surface, lungs)?;
        *self = builder.finish();
        Ok(())
    }

    /// Refresh discovery for an addressed live data subtree only. The index
    /// holds locators, never source schemas or rendered state documents.
    pub fn refresh_for_inode(&mut self, store: &ObjectStore, ino: u64) -> Result<()> {
        if self.state_dir.is_none() || !matches!(self.target, ProjectionTarget::View(_)) {
            *self = self.refresh(store)?;
            return Ok(());
        }
        let Some(path) = self.nodes.get(&ino).map(|node| node.path.clone()) else {
            return Ok(());
        };
        let ProjectionTarget::View(id) = &self.target else {
            unreachable!()
        };
        let view = store.read_view(id)?;
        let Some(plugin) = view
            .objects
            .keys()
            .find(|plugin| {
                let base = format!("/plugins/{}/data", encode_component(plugin));
                path == base || path.starts_with(&format!("{base}/"))
            })
            .cloned()
        else {
            return Ok(());
        };
        let base = format!("/plugins/{}/data", encode_component(&plugin));
        let parent = inode_for_path(&base);
        let descendants = format!("{base}/");
        // Remove stale discovery even when the new authoritative read fails.
        let removed: BTreeSet<_> = self
            .nodes
            .values()
            .filter(|node| node.path.starts_with(&descendants))
            .map(|node| node.ino)
            .collect();
        self.nodes.retain(|ino, _| !removed.contains(ino));
        self.children.retain(|ino, _| !removed.contains(ino));
        self.children
            .get_mut(&parent)
            .context("data directory absent")?
            .clear();
        let object = store.object_for_view(&view, &plugin)?;
        let snapshot = match self.read_public_for_view(&view, &plugin, &object.schema) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.unavailable_plugins.insert(plugin);
                return Err(error);
            }
        };
        let mut builder = NamespaceBuilder {
            paths: self
                .nodes
                .values()
                .map(|node| (node.path.clone(), node.ino))
                .collect(),
            namespace: self.clone(),
            object_hashes: view.objects.clone(),
        };
        builder.add_value_children(
            parent,
            &plugin,
            "",
            &snapshot.value,
            Some(&object.schema),
            None,
            None,
            false,
            true,
        )?;
        builder.namespace.unavailable_plugins.remove(&plugin);
        builder.namespace.live_generation = Some(snapshot.generation);
        *self = builder.finish();
        Ok(())
    }

    pub fn declare_local_owner(&mut self, uid: u32, operator_uid: Option<u32>) {
        self.local_call_owner_uid = Some(uid);
        self.allowed_operator_uid = operator_uid;
    }

    pub fn declare_catalog_dir(&mut self, catalog: &std::path::Path) -> Result<()> {
        let value = catalog
            .to_str()
            .context("catalog directory must be UTF-8")?;
        if !catalog.is_absolute()
            || catalog == std::path::Path::new("/")
            || value.split('/').any(|part| part == "." || part == "..")
        {
            bail!("catalog directory must be absolute and traversal-free");
        }
        self.declared_catalog_dir = catalog.to_path_buf();
        Ok(())
    }

    fn metadata_value(&self, view: &ViewManifest, workspace: Option<&Workspace>) -> Value {
        let mut value = serde_json::json!({
            "format":"3tchedfs/1", "view_id":view.view_id,
            "workspace_id":workspace.map(|workspace| &workspace.workspace_id),
            "source_catalog_hash":view.source_catalog_hash,
            "source_catalog_generation":view.source_catalog_generation,
            "object_count":view.objects.len(),
            "mount_identity":self.mount_identity, "instance_uuid":self.instance_uuid,
            "local_call_owner_uid":self.local_call_owner_uid,"allowed_operator_uid":self.allowed_operator_uid,
            "declared_catalog_dir":self.declared_catalog_dir
        });
        if view.objects.contains_key("tched_fs") {
            value["invocation"] =
                Value::String("/plugins/tched_fs/data/invocation/.value.json".to_owned());
        }
        if let MountIdentity::HostSession { session } = &self.mount_identity {
            value["session_id"] = Value::String(session.clone());
        }
        value
    }

    /// Fresh bounded rendering from authoritative locators. Namespace owns no
    /// rendered payloads, and returned bytes belong only to this read/open.
    pub fn file_bytes(&self, store: &ObjectStore, node: &Node) -> Result<Vec<u8>> {
        if let NodeKind::LiveFile { plugin_id, pointer } = &node.kind {
            let ProjectionTarget::View(id) = &self.target else {
                bail!("live locator is not a pinned view")
            };
            let view = store.read_view(id)?;
            let schema = store.object_for_view(&view, plugin_id)?.schema;
            let snapshot = self.read_public_for_view(&view, plugin_id, &schema)?;
            let value = snapshot
                .value
                .pointer(pointer)
                .context("committed public pointer is absent")?;
            return crate::store::render_document(value);
        }
        let NodeKind::File(source) = &node.kind else {
            bail!("node is not a synthetic/frozen file")
        };
        let (view, workspace) = match &self.target {
            ProjectionTarget::View(id) => (store.read_view(id)?, None),
            ProjectionTarget::Workspace(id) => {
                let workspace = store.read_workspace(id)?;
                (store.read_view(&workspace.base_view)?, Some(workspace))
            }
        };
        let object = |plugin_id: &str| -> Result<crate::model::TypedObjectBlob> {
            match &workspace {
                Some(workspace) => store.workspace_object(workspace, plugin_id),
                None => store.object_for_view(&view, plugin_id),
            }
        };
        let public_object = |plugin_id: &str| -> Result<crate::model::TypedObjectBlob> {
            let raw = object(plugin_id)?;
            if workspace.is_none() && self.state_dir.is_some() {
                return match self.read_public_for_view(&view, plugin_id, &raw.schema) {
                    Ok(snapshot) => crate::model::TypedObjectBlob::new_public(
                        raw.plugin_id,
                        raw.source_schema_hash,
                        raw.schema,
                        snapshot.value,
                    ),
                    Err(_) => crate::model::TypedObjectBlob::schema_only(
                        raw.plugin_id,
                        raw.source_schema_hash,
                        raw.schema,
                    ),
                };
            }
            raw.public_projection()
        };
        let value = match source {
            FileSource::MountMetadata => self.metadata_value(&view, workspace.as_ref()),
            FileSource::View => serde_json::to_value(&view)?,
            FileSource::Workspace => serde_json::to_value(
                workspace
                    .as_ref()
                    .context("workspace locator used in view mount")?,
            )?,
            FileSource::Object { plugin_id } => serde_json::to_value(public_object(plugin_id)?)?,
            FileSource::Schema { plugin_id } => serde_json::to_value(object(plugin_id)?.schema)?,
            FileSource::Value { plugin_id, pointer } => {
                let object = public_object(plugin_id)?;
                if object.value_scope == StateValueScope::SchemaOnly {
                    bail!("state value is unavailable in schema-only projection");
                }
                object
                    .value
                    .pointer(pointer)
                    .context("public value pointer is absent")?
                    .clone()
            }
            FileSource::Capabilities { plugin_id } => {
                serde_json::to_value(object(plugin_id)?.schema.required_capabilities())?
            }
            FileSource::Hash { plugin_id } => {
                return Ok(format!(
                    "{}\n",
                    object_hash_for_target(&view, workspace.as_ref(), plugin_id)?
                )
                .into_bytes());
            }
            FileSource::MethodInput { plugin_id, method } => object(plugin_id)?
                .schema
                .methods
                .get(method)
                .context("method absent from pinned schema")?
                .args
                .clone(),
            FileSource::MethodOutput { plugin_id, method } => object(plugin_id)?
                .schema
                .methods
                .get(method)
                .context("method absent from pinned schema")?
                .returns
                .clone()
                .context("method declares no return")?,
            FileSource::MethodMetadata { plugin_id, method } => serde_json::to_value(
                object(plugin_id)?
                    .schema
                    .methods
                    .get(method)
                    .context("method absent from pinned schema")?,
            )?,
            FileSource::ThreetchedSchema => {
                let surface = self
                    .threetched_surface
                    .as_ref()
                    .context("threetched surface is not attached")?;
                return Ok(surface.schema_bytes().to_vec());
            }
            FileSource::ThreetchedInput { command } => {
                let surface = self
                    .threetched_surface
                    .as_ref()
                    .context("threetched surface is not attached")?;
                surface.input_schema(command)?
            }
            FileSource::ThreetchedOutput { command } => {
                let surface = self
                    .threetched_surface
                    .as_ref()
                    .context("threetched surface is not attached")?;
                surface.output_schema(command)?
            }
            FileSource::ThreetchedMetadata { command } => {
                let surface = self
                    .threetched_surface
                    .as_ref()
                    .context("threetched surface is not attached")?;
                surface.metadata(command)?
            }
        };
        crate::store::render_document(&value)
    }

    fn read_public_for_view(
        &self,
        view: &ViewManifest,
        plugin: &str,
        schema: &PluginSchema,
    ) -> Result<crate::source::CommittedPublicValue> {
        let schemas = BTreeMap::from([(plugin.to_owned(), schema.clone())]);
        let hashes = BTreeMap::from([(
            plugin.to_owned(),
            self.source_schema_hashes
                .get(plugin)
                .context("missing pinned schema hash")?
                .clone(),
        )]);
        crate::source::verify_pinned_catalog(
            &self.declared_catalog_dir,
            &view.source_catalog_hash,
            view.source_catalog_generation,
            &hashes,
            &schemas,
        )?;
        let snapshot = crate::source::read_committed_public_value(
            self.state_dir
                .as_deref()
                .context("live state directory is absent")?,
            plugin,
            schema,
            crate::source::MAX_SNAPSHOT_BYTES,
        )?;
        crate::source::verify_pinned_catalog(
            &self.declared_catalog_dir,
            &view.source_catalog_hash,
            view.source_catalog_generation,
            &hashes,
            &schemas,
        )?;
        Ok(snapshot)
    }
}

struct NamespaceBuilder {
    namespace: Namespace,
    paths: HashMap<String, u64>,
    object_hashes: BTreeMap<String, String>,
}

#[allow(clippy::too_many_arguments)]
impl NamespaceBuilder {
    fn rehydrate(namespace: Namespace) -> Self {
        let paths = namespace
            .nodes
            .values()
            .map(|node| (node.path.clone(), node.ino))
            .collect();
        Self {
            namespace,
            paths,
            object_hashes: BTreeMap::new(),
        }
    }

    fn new(target: ProjectionTarget, state_dir: Option<std::path::PathBuf>, lungs: bool) -> Self {
        let root = Node {
            ino: ROOT_INO,
            parent: ROOT_INO,
            name: OsString::from("/"),
            path: "/".to_string(),
            kind: NodeKind::Directory,
            write_target: None,
        };
        Self {
            namespace: Namespace {
                nodes: HashMap::from([(ROOT_INO, root)]),
                children: HashMap::from([(ROOT_INO, BTreeMap::new())]),
                target,
                state_dir,
                lungs,
                mount_identity: MountIdentity::Unbound,
                instance_uuid: None,
                local_call_owner_uid: None,
                allowed_operator_uid: None,
                unavailable_plugins: BTreeSet::new(),
                live_generation: None,
                declared_catalog_dir: std::path::PathBuf::from(crate::source::DEFAULT_CATALOG_DIR),
                source_schema_hashes: BTreeMap::new(),
                threetched_surface: None,
            },
            paths: HashMap::from([("/".to_string(), ROOT_INO)]),
            object_hashes: BTreeMap::new(),
        }
    }

    /// Materialize the `threetched` plugin tree from a captured binary surface.
    #[cfg(test)]
    pub fn attach_threetched(
        &mut self,
        plugins_parent: u64,
        operations_parent: u64,
        surface: Arc<crate::threetched_surface::ThreetchedSurface>,
        lungs: bool,
    ) -> Result<()> {
        self.namespace.threetched_surface = Some(surface);
        let commands: Vec<String> = self
            .namespace
            .threetched_surface
            .as_ref()
            .context("threetched surface missing after attach")?
            .commands()
            .map(|command| command.to_owned())
            .collect();
        let plugin_id = "threetched";
        let plugin_name = encode_component(plugin_id);
        let plugin_dir = self.add_dir(plugins_parent, &plugin_name)?;
        self.add_file(
            plugin_dir,
            "schema.json",
            FileSource::ThreetchedSchema,
            None,
        )?;
        let methods_dir = self.add_dir(plugin_dir, "operations")?;
        let root_methods_dir = self.add_dir(operations_parent, &plugin_name)?;
        for command in commands {
            let encoded = encode_component(&command);
            let method_dir = self.add_dir(methods_dir, &encoded)?;
            self.add_file(
                method_dir,
                "input.schema.json",
                FileSource::ThreetchedInput {
                    command: command.clone(),
                },
                None,
            )?;
            self.add_file(
                method_dir,
                "output.schema.json",
                FileSource::ThreetchedOutput {
                    command: command.clone(),
                },
                None,
            )?;
            self.add_file(
                method_dir,
                "metadata.json",
                FileSource::ThreetchedMetadata {
                    command: command.clone(),
                },
                None,
            )?;
            if lungs {
                self.add_lungs_files(method_dir, plugin_id, &command)?;
            }
            self.add_symlink(
                root_methods_dir,
                &encoded,
                format!("../../plugins/{plugin_name}/operations/{encoded}"),
            )?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn build_with_threetched_surface(
        view_id: String,
        surface: crate::threetched_surface::ThreetchedSurface,
        lungs: bool,
    ) -> Result<Namespace> {
        let mut builder = NamespaceBuilder::new(ProjectionTarget::View(view_id), None, lungs);
        let plugins = builder.add_dir(ROOT_INO, "plugins")?;
        let operations = builder.add_dir(ROOT_INO, "operations")?;
        builder.attach_threetched(plugins, operations, Arc::new(surface), lungs)?;
        Ok(builder.namespace)
    }

    fn add_root_metadata(
        &mut self,
        view: &ViewManifest,
        workspace: Option<&Workspace>,
    ) -> Result<()> {
        let _ = view;
        self.add_file(ROOT_INO, ".3tchedfs.json", FileSource::MountMetadata, None)?;
        self.add_file(ROOT_INO, "view.json", FileSource::View, None)?;
        if workspace.is_some() {
            self.add_file(ROOT_INO, "workspace.json", FileSource::Workspace, None)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn add_value_children(
        &mut self,
        parent: u64,
        plugin_id: &str,
        pointer: &str,
        value: &Value,
        plugin_schema: Option<&PluginSchema>,
        field_schema: Option<&FieldSchema>,
        workspace_id: Option<&str>,
        inherited_writable: bool,
        live: bool,
    ) -> Result<()> {
        let writable = inherited_writable
            && !pointer_is_immutable(plugin_schema, pointer, field_schema, value);
        match value {
            Value::Object(values) => {
                if !pointer.is_empty() {
                    let target = write_target(workspace_id, plugin_id, pointer, writable);
                    if live && target.is_none() {
                        self.add_live_file(parent, ".value.json", plugin_id, pointer)?;
                    } else {
                        self.add_file(
                            parent,
                            ".value.json",
                            FileSource::Value {
                                plugin_id: plugin_id.to_owned(),
                                pointer: pointer.to_owned(),
                            },
                            target,
                        )?;
                    }
                }
                for (name, value) in values {
                    let child_pointer = push_pointer(pointer, name);
                    let child_schema = field_for_pointer(plugin_schema, field_schema, name);
                    let encoded = encode_component(name);
                    if value.is_object() || value.is_array() {
                        let directory = self.add_dir(parent, &encoded)?;
                        self.add_value_children(
                            directory,
                            plugin_id,
                            &child_pointer,
                            value,
                            plugin_schema,
                            child_schema,
                            workspace_id,
                            writable,
                            live,
                        )?;
                    } else {
                        let child_writable = writable
                            && !pointer_is_immutable(
                                plugin_schema,
                                &child_pointer,
                                child_schema,
                                value,
                            );
                        let target =
                            write_target(workspace_id, plugin_id, &child_pointer, child_writable);
                        if live && target.is_none() {
                            self.add_live_file(parent, &encoded, plugin_id, &child_pointer)?;
                        } else {
                            self.add_file(
                                parent,
                                &encoded,
                                FileSource::Value {
                                    plugin_id: plugin_id.to_owned(),
                                    pointer: child_pointer.clone(),
                                },
                                target,
                            )?;
                        }
                    }
                }
            }
            Value::Array(values) => {
                let target = write_target(workspace_id, plugin_id, pointer, writable);
                if live && target.is_none() {
                    self.add_live_file(parent, ".value.json", plugin_id, pointer)?;
                } else {
                    self.add_file(
                        parent,
                        ".value.json",
                        FileSource::Value {
                            plugin_id: plugin_id.to_owned(),
                            pointer: pointer.to_owned(),
                        },
                        target,
                    )?;
                }
                for (index, value) in values.iter().enumerate() {
                    let name = format!("{index:08}");
                    let child_pointer = push_pointer(pointer, &index.to_string());
                    if value.is_object() || value.is_array() {
                        let directory = self.add_dir(parent, &name)?;
                        self.add_value_children(
                            directory,
                            plugin_id,
                            &child_pointer,
                            value,
                            plugin_schema,
                            field_schema,
                            workspace_id,
                            writable,
                            live,
                        )?;
                    } else {
                        let target =
                            write_target(workspace_id, plugin_id, &child_pointer, writable);
                        if live && target.is_none() {
                            self.add_live_file(parent, &name, plugin_id, &child_pointer)?;
                        } else {
                            self.add_file(
                                parent,
                                &name,
                                FileSource::Value {
                                    plugin_id: plugin_id.to_owned(),
                                    pointer: child_pointer.clone(),
                                },
                                target,
                            )?;
                        }
                    }
                }
            }
            _ => bail!("add_value_children requires object or array"),
        }
        Ok(())
    }

    fn add_live_file(
        &mut self,
        parent: u64,
        name: &str,
        plugin_id: &str,
        pointer: &str,
    ) -> Result<u64> {
        self.add_node(
            parent,
            name,
            NodeKind::LiveFile {
                plugin_id: plugin_id.to_string(),
                pointer: pointer.to_string(),
            },
            None,
        )
    }

    fn add_lungs_files(&mut self, method_dir: u64, plugin_id: &str, method: &str) -> Result<()> {
        self.add_node(
            method_dir,
            "call",
            NodeKind::CallFile {
                plugin_id: plugin_id.to_string(),
                method: method.to_string(),
            },
            None,
        )?;
        Ok(())
    }

    fn add_dir(&mut self, parent: u64, name: &str) -> Result<u64> {
        self.add_node(parent, name, NodeKind::Directory, None)
    }

    fn add_dir_idempotent(&mut self, parent: u64, name: &str) -> Result<u64> {
        if let Some(ino) = self
            .namespace
            .children
            .get(&parent)
            .and_then(|children| children.get(&OsString::from(name)))
        {
            return Ok(*ino);
        }
        self.add_dir(parent, name)
    }

    fn add_file(
        &mut self,
        parent: u64,
        name: &str,
        source: FileSource,
        mut target: Option<WriteTarget>,
    ) -> Result<u64> {
        if let Some(target) = &mut target {
            target.expected_object_hash = self
                .object_hashes
                .get(&target.plugin_id)
                .context("workspace write object hash missing")?
                .clone();
        }
        self.add_node(parent, name, NodeKind::File(source), target)
    }

    fn add_symlink(&mut self, parent: u64, name: &str, target: String) -> Result<u64> {
        self.add_node(parent, name, NodeKind::Symlink(target), None)
    }

    fn add_node(
        &mut self,
        parent: u64,
        name: &str,
        kind: NodeKind,
        write_target: Option<WriteTarget>,
    ) -> Result<u64> {
        if self.namespace.nodes.len() >= 100_000 {
            bail!("namespace exceeds topology node budget");
        }
        let parent_node = self
            .namespace
            .nodes
            .get(&parent)
            .with_context(|| format!("parent inode {parent} does not exist"))?;
        if !matches!(parent_node.kind, NodeKind::Directory) {
            bail!("parent {} is not a directory", parent_node.path);
        }
        let path = if parent_node.path == "/" {
            format!("/{name}")
        } else {
            format!("{}/{}", parent_node.path, name)
        };
        if self.paths.contains_key(&path) {
            bail!("duplicate namespace path {path}");
        }
        let ino = inode_for_path(&path);
        if let Some(existing) = self.namespace.nodes.get(&ino) {
            bail!("inode collision between {} and {path}", existing.path);
        }
        let node = Node {
            ino,
            parent,
            name: OsString::from(name),
            path: path.clone(),
            kind,
            write_target,
        };
        if matches!(node.kind, NodeKind::Directory) {
            self.namespace.children.insert(ino, BTreeMap::new());
        }
        self.namespace
            .children
            .entry(parent)
            .or_default()
            .insert(node.name.clone(), ino);
        self.namespace.nodes.insert(ino, node);
        self.paths.insert(path, ino);
        Ok(ino)
    }

    fn finish(self) -> Namespace {
        self.namespace
    }
}

fn object_hash_for_target(
    view: &ViewManifest,
    workspace: Option<&Workspace>,
    plugin_id: &str,
) -> Result<String> {
    workspace
        .and_then(|workspace| workspace.staged_objects.get(plugin_id))
        .or_else(|| view.objects.get(plugin_id))
        .cloned()
        .with_context(|| format!("missing object hash for plugin '{plugin_id}'"))
}

fn write_target(
    workspace_id: Option<&str>,
    plugin_id: &str,
    pointer: &str,
    writable: bool,
) -> Option<WriteTarget> {
    if !writable {
        return None;
    }
    Some(WriteTarget {
        workspace_id: workspace_id?.to_string(),
        plugin_id: plugin_id.to_string(),
        pointer: pointer.to_string(),
        expected_object_hash: String::new(),
    })
}

fn pointer_is_immutable(
    schema: Option<&PluginSchema>,
    pointer: &str,
    field: Option<&FieldSchema>,
    _root_value: &Value,
) -> bool {
    let Some(schema) = schema else {
        return false;
    };
    if field.is_some_and(|field| field.read_only) {
        return true;
    }
    // Conditional writability depends on a sibling in the full plugin value.
    // Keep such leaves read-only in the direct file projection; whole-state
    // staging can still evaluate the condition accurately.
    if field.is_some_and(|field| field.read_only_when.is_some()) {
        return true;
    }
    schema
        .immutable_paths
        .iter()
        .any(|immutable| pointer == immutable || pointer.starts_with(&format!("{immutable}/")))
}

fn field_for_pointer<'a>(
    plugin_schema: Option<&'a PluginSchema>,
    parent: Option<&'a FieldSchema>,
    name: &str,
) -> Option<&'a FieldSchema> {
    fn object_field<'a>(field_type: &'a FieldType, name: &str) -> Option<&'a FieldSchema> {
        match field_type {
            FieldType::Object(fields) => fields.get(name),
            FieldType::Array(item) => object_field(item, name),
            _ => None,
        }
    }

    match parent.and_then(|field| object_field(&field.field_type, name)) {
        Some(field) => Some(field),
        None if parent.is_none() => plugin_schema.and_then(|schema| schema.fields.get(name)),
        None => None,
    }
}

fn push_pointer(parent: &str, segment: &str) -> String {
    let escaped = segment.replace('~', "~0").replace('/', "~1");
    format!("{parent}/{escaped}")
}

pub fn encode_component(value: &str) -> String {
    if value == ".value.json" {
        return "%2Evalue.json".to_owned();
    }
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    if output == "." || output == ".." || output.is_empty() {
        format!("%{}", hex_bytes(value.as_bytes()))
    } else {
        output
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

fn inode_for_path(path: &str) -> u64 {
    let bytes = blake3::hash(path.as_bytes()).as_bytes()[..8]
        .try_into()
        .expect("eight-byte slice");
    let mut ino = u64::from_le_bytes(bytes) | (1 << 63);
    if ino == ROOT_INO {
        ino += 1;
    }
    ino
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_store() -> (
        tempfile::TempDir,
        ObjectStore,
        ViewManifest,
        std::path::PathBuf,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(directory.path().join("store")).unwrap();
        let sealed:op_state_store::PluginSchema = serde_json::from_value(serde_json::json!({
            "name":"demo","version":"1","description":"fixture",
            "fields": {
                "items":{"field_type":{"one_of":[{"array":"integer"},{"object":{"shown":{"field_type":"boolean","required":true}}}]},"required":true},
                "private":{"field_type":"string","required":true}
            },
            "public_state":{"paths":["/items"]}
        })).unwrap();
        let schema = crate::model::from_sealed(sealed).unwrap();
        let catalog = crate::source::tests::fixture_catalog(directory.path(), &schema);
        let mut snapshot = catalog.capture_schema_only().unwrap();
        let object = crate::model::TypedObjectBlob::new(
            "demo".to_owned(),
            snapshot.objects["demo"].source_schema_hash.clone(),
            schema,
            serde_json::json!({"items":[1],"private":"secret"}),
        )
        .unwrap();
        snapshot.objects.insert("demo".to_owned(), object);
        let view = store.capture_snapshot(snapshot, false).unwrap();
        let state = catalog.state_dir;
        (directory, store, view, state)
    }

    #[test]
    fn locators_render_fresh_and_do_not_reuse_previous_payload() {
        let (_directory, store, view, _state) = fixture_store();
        let namespace =
            Namespace::build(&store, ProjectionTarget::View(view.view_id.clone()), None).unwrap();
        let node = namespace
            .nodes
            .values()
            .find(|node| node.path == "/plugins/demo/value.json")
            .unwrap();
        assert!(matches!(
            node.kind,
            NodeKind::File(FileSource::Value { .. })
        ));
        let owned = namespace.file_bytes(&store, node).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&owned).unwrap(),
            serde_json::json!({"items":[1]})
        );
        let objectpath = _directory
            .path()
            .join("store/objects")
            .join(format!("{}.blob", view.objects["demo"]));
        std::fs::write(objectpath, b"corrupt").unwrap();
        assert!(namespace.file_bytes(&store, node).is_err());
        assert_eq!(
            serde_json::from_slice::<Value>(&owned).unwrap(),
            serde_json::json!({"items":[1]})
        );
    }

    #[test]
    fn live_refresh_tracks_shape_changes_and_discards_unavailable_topology() {
        let (_directory, store, view, state) = fixture_store();
        let mut namespace = Namespace::build_with_catalog(
            &store,
            ProjectionTarget::View(view.view_id),
            Some(&state),
            true,
            &_directory.path().join("catalog"),
        )
        .unwrap();
        namespace
            .declare_mount_mode(
                MountIdentity::GuestKernel,
                Some("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"),
            )
            .unwrap();
        namespace.declare_local_owner(1000, Some(0));
        assert!(
            namespace
                .children(inode_for_path("/plugins/demo/data"))
                .unwrap()
                .is_empty()
        );
        let publish = |items: Value| {
            let bytes =
                serde_json::to_vec(&serde_json::json!({"items":items,"private":"secret"})).unwrap();
            op_core::projection_shm::update_projection_in(state.to_str().unwrap(), "demo", |_| {
                Ok(Some(bytes))
            })
            .unwrap();
        };
        publish(serde_json::json!([1, 2]));
        let _ = namespace.refresh_for_inode(&store, inode_for_path("/plugins/demo/data"));
        assert!(namespace.unavailable_plugins.is_empty());
        assert!(
            namespace
                .nodes
                .values()
                .any(|node| node.path == "/plugins/demo/data/items/00000001")
        );
        publish(serde_json::json!({"shown":true}));
        let _ = namespace.refresh_for_inode(&store, inode_for_path("/plugins/demo/data"));
        assert!(
            namespace
                .nodes
                .values()
                .any(|node| node.path == "/plugins/demo/data/items/shown")
        );
        assert!(
            !namespace
                .nodes
                .values()
                .any(|node| node.path == "/plugins/demo/data/items/00000001")
        );
        std::fs::write(
            state.join(".manifest.json"),
            b"{\"generation\":3,\"publishing\":true}",
        )
        .unwrap();
        let _ = namespace.refresh_for_inode(&store, inode_for_path("/plugins/demo/data"));
        assert!(namespace.unavailable_plugins.contains("demo"));
        assert!(
            !namespace
                .nodes
                .values()
                .any(|node| node.path.starts_with("/plugins/demo/data/items"))
        );
        assert_eq!(namespace.mount_identity, MountIdentity::GuestKernel);
        assert_eq!(namespace.local_call_owner_uid, Some(1000));
        assert_eq!(namespace.allowed_operator_uid, Some(0));
    }

    #[test]
    fn targeted_static_discovery_does_not_require_committed_state() {
        let (directory, store, view, state) = fixture_store();
        let mut namespace = Namespace::build_with_catalog(
            &store,
            ProjectionTarget::View(view.view_id),
            Some(&state),
            true,
            &directory.path().join("catalog"),
        )
        .unwrap();
        std::fs::create_dir_all(&state).unwrap();
        std::fs::write(state.join(".manifest.json"), b"invalid manifest").unwrap();
        let plugin = inode_for_path("/plugins/demo");
        namespace.refresh_for_inode(&store, plugin).unwrap();
        assert!(
            namespace
                .lookup(plugin, std::ffi::OsStr::new("schema.json"))
                .is_some()
        );
        let data = inode_for_path("/plugins/demo/data");
        assert!(namespace.refresh_for_inode(&store, data).is_err());
        assert!(namespace.children(data).unwrap().is_empty());
        assert!(namespace.node(plugin).is_some());
    }

    #[test]
    fn invocation_reference_requires_the_plugin_in_the_pinned_view() {
        let mut view = ViewManifest {
            format: "3tchedfs/1".into(),
            view_id: "test-view".into(),
            source_catalog_hash: "test-catalog".into(),
            source_catalog_generation: 1,
            parent_view: None,
            objects: BTreeMap::new(),
        };
        let metadata = |view: &ViewManifest| {
            let mut builder =
                NamespaceBuilder::new(ProjectionTarget::View(view.view_id.clone()), None, false);
            builder.add_root_metadata(view, None).unwrap();
            let ino = builder.namespace.children[&ROOT_INO][&OsString::from(".3tchedfs.json")];
            let NodeKind::File(FileSource::MountMetadata) = &builder.namespace.nodes[&ino].kind
            else {
                panic!("mount metadata must be a projected file");
            };
            builder.namespace.metadata_value(view, None)
        };
        assert!(metadata(&view).get("invocation").is_none());
        view.objects
            .insert("tched_fs".into(), "sealed-object-hash".into());
        assert_eq!(
            metadata(&view)["invocation"],
            "/plugins/tched_fs/data/invocation/.value.json"
        );
        view.objects.remove("tched_fs");
        view.objects
            .insert("unrelated".into(), "other-object-hash".into());
        assert!(metadata(&view).get("invocation").is_none());
    }

    #[test]
    fn threetched_attach_materializes_plugin_tree_from_fixture() {
        use crate::source::SourceSnapshot;
        use crate::threetched_surface::ThreetchedSurface;

        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path().join("store")).unwrap();
        let snapshot = SourceSnapshot {
            catalog_hash: "threetched-fixture".into(),
            catalog_generation: 1,
            objects: BTreeMap::new(),
        };
        let view = store.capture_snapshot(snapshot, false).unwrap();
        let binary = crate::threetched_surface::test_fixture_binary(dir.path());
        let surface = ThreetchedSurface::capture(&binary).unwrap();
        let namespace =
            NamespaceBuilder::build_with_threetched_surface(view.view_id, surface, true).unwrap();

        let schema_node = namespace
            .nodes
            .values()
            .find(|node| node.path == "/plugins/threetched/schema.json")
            .unwrap();
        let schema_bytes = namespace.file_bytes(&store, schema_node).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&schema_bytes).unwrap(),
            serde_json::json!({"type":"object","title":"fixture-config","properties":{"model_routes":{"type":"array"}}})
        );

        for leaf in ["call", "input.schema.json", "metadata.json"] {
            let path = format!("/plugins/threetched/operations/self-test/{leaf}");
            assert!(
                namespace.nodes.values().any(|node| node.path == path),
                "missing {path}"
            );
        }
        let metadata_node = namespace
            .nodes
            .values()
            .find(|node| node.path == "/plugins/threetched/operations/self-test/metadata.json")
            .unwrap();
        let metadata =
            serde_json::from_slice::<Value>(&namespace.file_bytes(&store, metadata_node).unwrap())
                .unwrap();
        assert_eq!(metadata["name"], "self-test");
    }

    #[test]
    fn encoding_is_path_safe_and_deterministic() {
        assert_eq!(encode_component("hello_world-1.0"), "hello_world-1.0");
        assert_eq!(encode_component("cap:test@v1"), "cap%3Atest%40v1");
        assert_eq!(encode_component(".."), "%2E2E");
        assert_eq!(encode_component(".value.json"), "%2Evalue.json");
    }

    #[test]
    fn path_inodes_are_stable() {
        assert_eq!(
            inode_for_path("/plugins/demo"),
            inode_for_path("/plugins/demo")
        );
        assert_ne!(
            inode_for_path("/plugins/demo"),
            inode_for_path("/plugins/other")
        );
    }
}
