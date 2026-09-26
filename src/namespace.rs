use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use parking_lot::RwLock;
use serde::Serialize;
use serde_json::Value;

use crate::model::{FieldSchema, FieldType, PluginSchema};
use crate::store::{ObjectStore, ViewManifest, Workspace};

pub const ROOT_INO: u64 = 1;

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
    File(Arc<RwLock<Vec<u8>>>),
    /// A file whose content is read live from the SHM state tree at open time.
    /// Only used for read-only view mounts. `plugin_id` is a catalog-known
    /// name (validated during capture); `pointer` is a JSON pointer into the
    /// plugin's state object. `schema` is pinned from the sealed blob.
    LiveFile {
        plugin_id: String,
        pointer: String,
    },
    Symlink(String),
}

#[derive(Debug, Clone)]
pub struct WriteTarget {
    pub workspace_id: String,
    pub plugin_id: String,
    pub pointer: String,
}

impl Namespace {
    pub fn build(
        store: &ObjectStore,
        target: ProjectionTarget,
        state_dir: Option<&std::path::Path>,
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
        let mut builder = NamespaceBuilder::new(target.clone(), state_dir.map(|p| p.to_path_buf()));
        builder.add_root_metadata(&view, workspace.as_ref())?;

        let plugins = builder.add_dir(ROOT_INO, "plugins")?;
        let operations = builder.add_dir(ROOT_INO, "operations")?;
        let relationships = builder.add_dir(ROOT_INO, "relationships")?;
        let dependencies = builder.add_dir(relationships, "dependencies")?;
        let capabilities = builder.add_dir(ROOT_INO, "capabilities")?;

        for plugin_id in view.objects.keys() {
            let object = match &workspace {
                Some(workspace) => store.workspace_object(workspace, plugin_id)?,
                None => store.object_for_view(&view, plugin_id)?,
            };
            let plugin_name = encode_component(plugin_id);
            let plugin_dir = builder.add_dir(plugins, &plugin_name)?;
            builder.add_json(plugin_dir, "object.blob.json", &object, None)?;
            builder.add_json(plugin_dir, "schema.json", &object.schema, None)?;
            if live {
                builder.add_live_file(plugin_dir, "value.json", plugin_id, "")?;
            } else {
                builder.add_json(plugin_dir, "value.json", &object.value, None)?;
            }
            builder.add_json(
                plugin_dir,
                "capabilities.json",
                &object.schema.capabilities(),
                None,
            )?;
            builder.add_text(
                plugin_dir,
                "object.hash",
                format!(
                    "{}\n",
                    object_hash_for_target(&view, workspace.as_ref(), plugin_id)?
                )
                .into_bytes(),
                None,
            )?;

            let data_dir = builder.add_dir(plugin_dir, "data")?;
            let workspace_id = workspace
                .as_ref()
                .map(|workspace| workspace.workspace_id.clone());
            builder.add_value_children(
                data_dir,
                plugin_id,
                "",
                &object.value,
                Some(&object.schema),
                None,
                workspace_id.as_deref(),
                !object.schema.tags.iter().any(|tag| tag == "immutable"),
                live,
            )?;

            let methods_dir = builder.add_dir(plugin_dir, "operations")?;
            let root_methods_dir = builder.add_dir(operations, &plugin_name)?;
            for (method_name, method) in &object.schema.methods {
                let encoded = encode_component(method_name);
                let method_dir = builder.add_dir(methods_dir, &encoded)?;
                builder.add_json(method_dir, "input.schema.json", &method.args, None)?;
                if let Some(returns) = &method.returns {
                    builder.add_json(method_dir, "output.schema.json", returns, None)?;
                }
                builder.add_json(method_dir, "metadata.json", method, None)?;
                builder.add_symlink(
                    root_methods_dir,
                    &encoded,
                    format!("/plugins/{plugin_name}/operations/{encoded}"),
                )?;

                if let Some(capability) = &method.required_capability {
                    let capability_dir =
                        builder.add_dir_idempotent(capabilities, &encode_component(capability))?;
                    builder.add_symlink(
                        capability_dir,
                        &format!("{plugin_name}.{}", encode_component(method_name)),
                        format!("/plugins/{plugin_name}/operations/{encoded}"),
                    )?;
                }
            }

            let relation_dir = builder.add_dir(dependencies, &plugin_name)?;
            for dependency in &object.schema.dependencies {
                builder.add_symlink(
                    relation_dir,
                    &encode_component(dependency),
                    format!("/plugins/{}", encode_component(dependency)),
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
}

struct NamespaceBuilder {
    namespace: Namespace,
    paths: HashMap<String, u64>,
}

#[allow(clippy::too_many_arguments)]
impl NamespaceBuilder {
    fn new(target: ProjectionTarget, state_dir: Option<std::path::PathBuf>) -> Self {
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
            },
            paths: HashMap::from([("/".to_string(), ROOT_INO)]),
        }
    }

    fn add_root_metadata(
        &mut self,
        view: &ViewManifest,
        workspace: Option<&Workspace>,
    ) -> Result<()> {
        #[derive(Serialize)]
        struct MountMetadata<'a> {
            format: &'static str,
            view_id: &'a str,
            workspace_id: Option<&'a str>,
            source_catalog_hash: &'a str,
            source_catalog_generation: u64,
            object_count: usize,
        }
        self.add_json(
            ROOT_INO,
            ".3tchedfs.json",
            &MountMetadata {
                format: "3tchedfs/1",
                view_id: &view.view_id,
                workspace_id: workspace.map(|workspace| workspace.workspace_id.as_str()),
                source_catalog_hash: &view.source_catalog_hash,
                source_catalog_generation: view.source_catalog_generation,
                object_count: view.objects.len(),
            },
            None,
        )?;
        self.add_json(ROOT_INO, "view.json", view, None)?;
        if let Some(workspace) = workspace {
            self.add_json(ROOT_INO, "workspace.json", workspace, None)?;
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
                        self.add_json(parent, ".value.json", value, target)?;
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
                            self.add_json(parent, &encoded, value, target)?;
                        }
                    }
                }
            }
            Value::Array(values) => {
                let target = write_target(workspace_id, plugin_id, pointer, writable);
                if live && target.is_none() {
                    self.add_live_file(parent, ".value.json", plugin_id, pointer)?;
                } else {
                    self.add_json(parent, ".value.json", value, target)?;
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
                            self.add_json(parent, &name, value, target)?;
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

    fn add_json(
        &mut self,
        parent: u64,
        name: &str,
        value: &impl Serialize,
        target: Option<WriteTarget>,
    ) -> Result<u64> {
        let mut bytes = serde_json::to_vec_pretty(value)?;
        bytes.push(b'\n');
        self.add_text(parent, name, bytes, target)
    }

    fn add_text(
        &mut self,
        parent: u64,
        name: &str,
        bytes: Vec<u8>,
        target: Option<WriteTarget>,
    ) -> Result<u64> {
        self.add_node(
            parent,
            name,
            NodeKind::File(Arc::new(RwLock::new(bytes))),
            target,
        )
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

    #[test]
    fn encoding_is_path_safe_and_deterministic() {
        assert_eq!(encode_component("hello_world-1.0"), "hello_world-1.0");
        assert_eq!(encode_component("cap:test@v1"), "cap%3Atest%40v1");
        assert_eq!(encode_component(".."), "%2E2E");
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
