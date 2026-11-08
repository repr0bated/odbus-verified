//! Direct operation access through the mounted identity namespace. No backend fallback.
#[cfg(test)]
use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

use crate::dispatch;
use crate::model::{MethodDecl, PluginSchema, TypedObjectBlob};
use crate::namespace::{MountIdentity, encode_component};

const MAX_DOCUMENT: u64 = 16 * 1024 * 1024;
const MAX_RECEIPT: u64 = 16 * 1024 * 1024;

type MountProbe = Arc<dyn Fn(&Path, &File) -> Result<String> + Send + Sync>;

/// Preserve the original mounted denial without retrying or rewriting it.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct MountedDiagnostic(pub Value);

pub struct MountedNamespace {
    path: PathBuf,
    root: File,
    mount_id: String,
    metadata: Value,
    view: Value,
    probe: MountProbe,
}

impl MountedNamespace {
    pub fn open(path: &Path, expected_session: Option<&str>) -> Result<Self> {
        Self::open_with_probe(path, expected_session, Arc::new(probe_mount))
    }

    pub fn open_for_mode(path: &Path, expected: &MountIdentity) -> Result<Self> {
        ensure!(
            !matches!(expected, MountIdentity::Unbound),
            "unbound mount has no operation identity"
        );
        Self::open_with_mode_and_probe(path, None, Some(expected), Arc::new(probe_mount))
    }

    fn open_with_probe(
        path: &Path,
        expected_session: Option<&str>,
        probe: MountProbe,
    ) -> Result<Self> {
        Self::open_with_mode_and_probe(path, expected_session, None, probe)
    }

    fn open_with_mode_and_probe(
        path: &Path,
        expected_session: Option<&str>,
        expected_mode: Option<&MountIdentity>,
        probe: MountProbe,
    ) -> Result<Self> {
        let path = std::fs::canonicalize(path).context("resolve mounted namespace")?;
        let root = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .context("open mounted namespace")?;
        let mount_id = probe(&path, &root)?;
        let metadata = read_json(open_relative(&root, ".3tchedfs.json", false)?)?;
        ensure!(
            metadata["format"] == "3tchedfs/1",
            "unsupported mount declaration"
        );
        ensure!(
            metadata["workspace_id"].is_null(),
            "mount must be an identity view"
        );
        let instance = metadata["instance_uuid"]
            .as_str()
            .context("mount instance UUID absent")?;
        ensure!(
            uuid::Uuid::parse_str(instance)?.to_string() == instance,
            "mount instance UUID is noncanonical"
        );
        let mode: MountIdentity = serde_json::from_value(metadata["mount_identity"].clone())
            .context("mounted namespace has no typed identity mode")?;
        if let Some(expected) = expected_mode {
            ensure!(
                &mode == expected,
                "mounted identity mode differs from expected mode"
            );
        }
        match &mode {
            MountIdentity::HostSession { session } => {
                ensure!(
                    !session.is_empty()
                        && metadata["session_id"].as_str() == Some(session.as_str()),
                    "mounted host session declaration is inconsistent"
                );
                if let Some(expected) = expected_session {
                    ensure!(
                        session == expected,
                        "mounted identity session differs from --expected-session"
                    );
                }
            }
            MountIdentity::GuestKernel | MountIdentity::Credential => {
                ensure!(
                    expected_mode.is_some() && metadata["session_id"].is_null(),
                    "mount must declare the expected operation identity mode"
                );
            }
            MountIdentity::Unbound => bail!("unbound mount has no operation identity"),
        }
        let view = read_json(open_relative(&root, "view.json", false)?)?;
        for key in [
            "format",
            "view_id",
            "source_catalog_hash",
            "source_catalog_generation",
        ] {
            ensure!(
                metadata.get(key).is_some() && metadata[key] == view[key],
                "mount/view {key} mismatch"
            );
        }
        ensure!(
            view["objects"].is_object(),
            "mounted view has no object catalog"
        );
        ensure!(
            metadata["object_count"].as_u64()
                == view["objects"]
                    .as_object()
                    .map(|objects| objects.len() as u64),
            "mounted catalog count mismatch"
        );
        let namespace = Self {
            path,
            root,
            mount_id,
            metadata,
            view,
            probe,
        };
        namespace.verify()?;
        Ok(namespace)
    }

    /// Pin the mount instance as well as its declared identity/catalog. Replacing
    /// a mount with identical contents still invalidates this operation context.
    pub fn verify(&self) -> Result<()> {
        ensure!(
            (self.probe)(&self.path, &self.root)? == self.mount_id,
            "3tchedFS mount instance changed"
        );
        let current = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
            .open(&self.path)
            .context("reopen mount root")?;
        let original = self.root.metadata()?;
        let present = current.metadata()?;
        ensure!(
            original.dev() == present.dev() && original.ino() == present.ino(),
            "3tchedFS root changed"
        );
        ensure!(
            read_json(open_relative(&self.root, ".3tchedfs.json", false)?)? == self.metadata,
            "3tchedFS identity or catalog declaration changed"
        );
        ensure!(
            read_json(open_relative(&self.root, "view.json", false)?)? == self.view,
            "3tchedFS view changed"
        );
        Ok(())
    }

    /// Read only the sealed declaration nodes. The mount pins their object
    /// locator and catalog; no public state or object payload is required.
    pub fn schema(&self, plugin: &str) -> Result<PluginSchema> {
        self.verify()?;
        let hash = self.view["objects"]
            .get(plugin)
            .and_then(Value::as_str)
            .context("plugin hash absent from mounted catalog")?;
        let prefix = format!("plugins/{}", encode_component(plugin));
        let schema: PluginSchema = serde_json::from_value(read_json(open_relative(
            &self.root,
            &format!("{prefix}/schema.json"),
            false,
        )?)?)?;
        ensure!(schema.name == plugin, "mounted plugin identity mismatch");
        ensure!(
            hash == read_bounded(
                open_relative(&self.root, &format!("{prefix}/object.hash"), false)?,
                1024
            )?
            .trim(),
            "mounted object hash declaration mismatch"
        );
        self.verify()?;
        Ok(schema)
    }

    pub fn object(&self, plugin: &str) -> Result<TypedObjectBlob> {
        self.verify()?;
        let hash = self.view["objects"]
            .get(plugin)
            .context("plugin absent from mounted catalog")?;
        let prefix = format!("plugins/{}", encode_component(plugin));
        let object: TypedObjectBlob = serde_json::from_value(read_json(open_relative(
            &self.root,
            &format!("{prefix}/object.blob.json"),
            false,
        )?)?)?;
        ensure!(
            object.plugin_id == plugin && object.schema.name == plugin,
            "mounted plugin identity mismatch"
        );
        ensure!(
            hash.as_str()
                == Some(
                    read_bounded(
                        open_relative(&self.root, &format!("{prefix}/object.hash"), false)?,
                        1024
                    )?
                    .trim()
                ),
            "mounted object hash declaration mismatch"
        );
        ensure!(
            read_json(open_relative(
                &self.root,
                &format!("{prefix}/schema.json"),
                false
            )?)? == serde_json::to_value(&object.schema)?,
            "mounted schema differs from pinned object"
        );
        self.verify()?;
        Ok(object)
    }

    pub fn plugins(&self) -> Result<Vec<String>> {
        self.verify()?;
        let plugins = self.view["objects"]
            .as_object()
            .context("missing catalog")?
            .keys()
            .cloned()
            .collect();
        self.verify()?;
        Ok(plugins)
    }

    pub fn view_id(&self) -> Result<&str> {
        self.metadata["view_id"]
            .as_str()
            .context("mounted view id absent")
    }

    /// Public present state served by the mount. A lungs view reads this from
    /// committed SHM inside the filesystem; this process does not open SHM.
    pub fn state(&self, plugin: &str) -> Result<Value> {
        self.verify()?;
        ensure!(
            self.view["objects"].get(plugin).is_some(),
            "plugin absent from mount"
        );
        let state = read_json(open_relative(
            &self.root,
            &format!("plugins/{}/value.json", encode_component(plugin)),
            false,
        )?)?;
        self.verify()?;
        Ok(state)
    }

    pub fn method_decl(&self, plugin: &str, method: &str) -> Result<MethodDecl> {
        self.schema(plugin)?
            .methods
            .get(method)
            .cloned()
            .context("operation is not declared in the mounted catalog")
    }

    /// Fresh discovery for native clients. These are the mounted declarations,
    /// not a parallel tool schema or an authority to execute the operations.
    pub fn describe(&self, selected_plugin: Option<&str>) -> Result<Value> {
        self.verify()?;
        let objects = self.view["objects"]
            .as_object()
            .context("missing catalog")?;
        if let Some(plugin) = selected_plugin {
            ensure!(
                objects.contains_key(plugin),
                "plugin absent from mounted catalog"
            );
        }
        let mut methods = Vec::new();
        // Account for the envelope and separators as well as declarations.
        let mut remaining = MAX_DOCUMENT as usize - 32;
        for plugin in objects
            .keys()
            .filter(|plugin| selected_plugin.is_none_or(|selected| selected == plugin.as_str()))
        {
            let schema = self.schema(plugin)?;
            let mut declarations: Vec<_> = schema.methods.into_iter().collect();
            declarations.sort_by(|left, right| left.0.cmp(&right.0));
            for (method, declaration) in declarations {
                let descriptor = serde_json::json!({
                    "plugin_id": plugin,
                    "method": method,
                    "path": format!("plugins/{}/operations/{}/call",
                        encode_component(plugin), encode_component(&method)),
                    "declaration": declaration,
                });
                let size = serde_json::to_vec(&descriptor)?.len();
                remaining = remaining
                    .checked_sub(size + 1)
                    .context("mounted operation discovery exceeds 16 MiB")?;
                methods
                    .try_reserve(1)
                    .context("allocate operation discovery")?;
                methods.push(descriptor);
            }
        }
        self.verify()?;
        Ok(serde_json::json!({"methods": methods}))
    }

    pub fn invoke(
        &self,
        path: &str,
        arguments: Value,
        confirm_side_effects: bool,
    ) -> Result<Value> {
        self.invoke_with(
            path,
            arguments,
            confirm_side_effects,
            |plugin, method, decl, args| self.call(plugin, method, decl, args),
        )
    }

    fn invoke_with(
        &self,
        path: &str,
        arguments: Value,
        confirm_side_effects: bool,
        operation: impl FnOnce(&str, &str, &MethodDecl, Value) -> Result<Value>,
    ) -> Result<Value> {
        // Literal syntax preserves the canonical projected path: Path::components
        // would silently discard repeated separators and `.` components.
        let parts: Vec<_> = path.split('/').collect();
        ensure!(
            parts.len() == 5
                && parts[0] == "plugins"
                && parts[2] == "operations"
                && parts[4] == "call"
                && !parts[1].is_empty()
                && !parts[3].is_empty(),
            "expected mount-relative plugins/<encoded-plugin>/operations/<encoded-method>/call"
        );
        self.verify()?;
        let plugin = self.view["objects"]
            .as_object()
            .context("missing catalog")?
            .keys()
            .find(|name| encode_component(name) == parts[1])
            .context("path plugin is absent from pinned catalog")?;
        let schema = self.schema(plugin)?;
        let (method, decl) = schema
            .methods
            .iter()
            .find(|(name, _)| encode_component(name) == parts[3])
            .context("path operation is absent from pinned schema")?;
        dispatch::validate_arguments(decl, &arguments)?;
        crate::store::render_arguments(&arguments)
            .context("mounted operation arguments exceed 1 MiB")?;
        ensure!(
            decl.side_effect == crate::model::SideEffect::Read || confirm_side_effects,
            "operation requires --confirm-side-effects"
        );
        let result = operation(plugin, method, decl, arguments);
        self.verify()
            .context("mount changed during invocation; do not retry automatically")?;
        result
    }

    pub fn call(
        &self,
        plugin: &str,
        method: &str,
        decl: &MethodDecl,
        arguments: Value,
    ) -> Result<Value> {
        self.call_with(plugin, method, decl, arguments, |mut file, bytes| {
            file.write_all(bytes)
                .context("write mounted operation arguments")?;
            // fsync executes the complete call; even a denial retains its
            // exact diagnostic on this same handle. Never retry the write.
            let synced = file.sync_all();
            file.seek(SeekFrom::Start(0))
                .context("seek operation receipt")?;
            let body = read_bounded(file, MAX_RECEIPT).context("read operation receipt")?;
            terminal_receipt(&body, synced, plugin, method, decl)
        })
    }

    fn call_with(
        &self,
        plugin: &str,
        method: &str,
        decl: &MethodDecl,
        arguments: Value,
        operation: impl FnOnce(File, &[u8]) -> Result<Value>,
    ) -> Result<Value> {
        dispatch::validate_arguments(decl, &arguments)?;
        let bytes = crate::store::render_arguments(&arguments)
            .context("mounted operation arguments exceed 1 MiB")?;
        self.verify()?;
        ensure!(
            self.view["objects"].get(plugin).is_some(),
            "plugin absent from mount"
        );
        let prefix = format!(
            "plugins/{}/operations/{}",
            encode_component(plugin),
            encode_component(method)
        );
        let mounted_decl: MethodDecl = serde_json::from_value(read_json(open_relative(
            &self.root,
            &format!("{prefix}/metadata.json"),
            false,
        )?)?)?;
        ensure!(
            mounted_decl == *decl,
            "mounted operation declaration changed"
        );
        ensure!(
            read_json(open_relative(
                &self.root,
                &format!("{prefix}/input.schema.json"),
                false
            )?)? == decl.args,
            "mounted operation argument schema changed"
        );
        let file = open_relative(&self.root, &format!("{prefix}/call"), true)?;
        self.verify()?;
        let result = operation(file, &bytes);
        // An effect may already have occurred: mismatch is an explicit failure,
        // never justification for automatically submitting the call again.
        self.verify()
            .context("mount changed during operation; do not retry automatically")?;
        result
    }
}

fn terminal_receipt(
    body: &str,
    synced: std::io::Result<()>,
    plugin: &str,
    method: &str,
    decl: &MethodDecl,
) -> Result<Value> {
    let value: Value =
        serde_json::from_str(body).context("missing or malformed terminal operation receipt")?;
    if value.get("error").is_some() {
        ensure!(
            value["plugin"] == plugin && value["method"] == method,
            "operation diagnostic attribution mismatch"
        );
        return Err(MountedDiagnostic(value).into());
    }
    ensure!(
        value["plugin_id"] == plugin && value["method"] == method,
        "operation receipt attribution mismatch"
    );
    synced.context("operation fsync failed without a terminal diagnostic")?;
    ensure!(
        value["subid"] == decl.subid
            && value["required_capability"] == serde_json::to_value(&decl.required_capability)?
            && value["side_effect"] == serde_json::to_value(decl.side_effect)?
            && value["idempotent"] == decl.idempotent,
        "operation receipt contract mismatch"
    );
    ensure!(
        value.get("result").is_some(),
        "terminal operation receipt has no result"
    );
    dispatch::validate_engine_receipt(plugin, method, decl, &value["result"])?;
    Ok(value)
}

fn read_json(file: File) -> Result<Value> {
    serde_json::from_str(&read_bounded(file, MAX_DOCUMENT)?).context("decode mounted JSON")
}

fn read_bounded(reader: impl Read, limit: u64) -> Result<String> {
    let mut bytes = Vec::new();
    reader.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "mounted document exceeds {limit} byte limit"
    );
    String::from_utf8(bytes).context("mounted document is not UTF-8")
}

/// Walk only existing projected nodes relative to the pinned root descriptor.
/// Do not follow symlinks or create files on a missing/unmounted namespace.
fn open_relative(root: &File, relative: &str, write: bool) -> Result<File> {
    let components: Vec<_> = Path::new(relative).components().collect();
    ensure!(!components.is_empty(), "empty projected path");
    let mut parent = root.try_clone()?;
    let mount_id = fd_mount_id(root)?;
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            bail!("unsafe projected path");
        };
        use std::os::unix::ffi::OsStrExt;
        let name = CString::new(name.as_bytes())?;
        let last = index + 1 == components.len();
        let flags = libc::O_CLOEXEC
            | libc::O_NOFOLLOW
            | if last {
                if write { libc::O_RDWR } else { libc::O_RDONLY }
            } else {
                libc::O_RDONLY | libc::O_DIRECTORY
            };
        // SAFETY: parent is a live directory descriptor; name is NUL terminated.
        let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error())
                .with_context(|| format!("open mounted {relative}"));
        }
        // SAFETY: openat returned a newly owned descriptor.
        parent = unsafe { File::from_raw_fd(fd) };
        ensure!(
            fd_mount_id(&parent)? == mount_id,
            "projected path crosses the pinned mount boundary"
        );
    }
    Ok(parent)
}

fn fd_mount_id(file: &File) -> Result<String> {
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", file.as_raw_fd()))?;
    info.lines()
        .find_map(|line| line.strip_prefix("mnt_id:").map(str::trim))
        .map(str::to_owned)
        .context("kernel file mount identity unavailable")
}

fn probe_mount(path: &Path, root: &File) -> Result<String> {
    let fdinfo = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", root.as_raw_fd()))?;
    let id = fdinfo
        .lines()
        .find_map(|line| line.strip_prefix("mnt_id:").map(str::trim))
        .context("kernel mount identity unavailable")?;
    let info = std::fs::read_to_string("/proc/self/mountinfo")?;
    let line = info
        .lines()
        .find(|line| line.split_whitespace().next() == Some(id))
        .context("pinned mount no longer attached")?;
    let (before, after) = line
        .split_once(" - ")
        .context("invalid kernel mount record")?;
    let fields: Vec<_> = before.split_whitespace().collect();
    let mountpoint = fields.get(4).context("missing kernel mountpoint")?;
    let decoded = mountpoint
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\");
    ensure!(
        Path::new(&decoded) == path && after.split_whitespace().next() == Some("fuse.3tchedFS"),
        "--mount must name an actual fuse.3tchedFS mount root"
    );
    Ok(id.to_owned())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::{PluginSchema, SideEffect};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub(crate) fn fixture() -> (tempfile::TempDir, MountedNamespace, MethodDecl) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let operation = root.join("plugins/demo/operations/get_status");
        std::fs::create_dir_all(&operation).unwrap();
        let decl = MethodDecl {
            name: "get_status".into(),
            args: json!({"type":"object","properties":{"event":{"type":"string"}},"required":["event"],"additionalProperties":false}),
            returns: Some(
                json!({"type":"object","required":["answer"],"properties":{"answer":{"type":"integer"}},"additionalProperties":false}),
            ),
            side_effect: SideEffect::Read,
            idempotent: true,
            required_capability: Some("cap.demo@v1".into()),
            subid: "demo.get_status@v1".into(),
        };
        let object = TypedObjectBlob::new(
            "demo".into(),
            "a".repeat(64),
            PluginSchema {
                name: "demo".into(),
                version: "1".into(),
                methods: BTreeMap::from([("get_status".into(), decl.clone())]),
                ..Default::default()
            },
            json!({"mode":"sealed"}),
        )
        .unwrap();
        for (path, value) in [
            (
                ".3tchedfs.json",
                json!({"format":"3tchedfs/1","view_id":"view","workspace_id":null,"session_id":"human","mount_identity":{"kind":"host_session","session":"human"},"instance_uuid":"00000000-0000-4000-8000-000000000001","source_catalog_hash":"catalog","source_catalog_generation":1,"object_count":1}),
            ),
            (
                "view.json",
                json!({"format":"3tchedfs/1","view_id":"view","source_catalog_hash":"catalog","source_catalog_generation":1,"objects":{"demo":"object-hash"}}),
            ),
            (
                "plugins/demo/object.blob.json",
                serde_json::to_value(&object).unwrap(),
            ),
            (
                "plugins/demo/schema.json",
                serde_json::to_value(&object.schema).unwrap(),
            ),
            (
                "plugins/demo/value.json",
                json!({"mode":"mounted","nested":{"n":7}}),
            ),
            (
                "plugins/demo/operations/get_status/metadata.json",
                serde_json::to_value(&decl).unwrap(),
            ),
            (
                "plugins/demo/operations/get_status/input.schema.json",
                decl.args.clone(),
            ),
        ] {
            std::fs::write(root.join(path), serde_json::to_vec(&value).unwrap()).unwrap();
        }
        std::fs::write(root.join("plugins/demo/object.hash"), "object-hash\n").unwrap();
        std::fs::write(operation.join("call"), "").unwrap();
        let mount = MountedNamespace::open_with_probe(
            root,
            Some("human"),
            Arc::new(|_, _| Ok("mount-1".into())),
        )
        .unwrap();
        (directory, mount, decl)
    }

    fn receipt(decl: &MethodDecl) -> Value {
        json!({"plugin_id":"demo","method":"get_status","subid":decl.subid,"required_capability":decl.required_capability,
            "side_effect":decl.side_effect,"idempotent":decl.idempotent,"result":{
                "success":true,"operation_id":"00000000-0000-4000-8000-000000000001","outcome":"completed",
                "event_id":1,"event_hash":"a".repeat(32),"terminal_event_id":2,"terminal_event_hash":"b".repeat(32),
                "plugin_id":"demo","method":"get_status","result":{"answer":42}}})
    }

    #[test]
    fn mounted_catalog_and_state_never_use_captured_values() {
        let (_directory, mount, _) = fixture();
        assert_eq!(mount.object("demo").unwrap().value["mode"], "sealed");
        assert_eq!(mount.state("demo").unwrap()["mode"], "mounted");
    }

    #[test]
    fn declarations_do_not_require_object_or_state_payloads() {
        let (directory, mount, decl) = fixture();
        let plugin = directory.path().join("plugins/demo");
        std::fs::remove_file(plugin.join("object.blob.json")).unwrap();
        std::fs::remove_file(plugin.join("value.json")).unwrap();
        assert_eq!(mount.schema("demo").unwrap().name, "demo");
        assert_eq!(mount.method_decl("demo", "get_status").unwrap(), decl);
        assert_eq!(
            mount.describe(None).unwrap()["methods"][0]["method"],
            "get_status"
        );
        assert_eq!(
            mount
                .invoke_with(
                    "plugins/demo/operations/get_status/call",
                    json!({"event":"read"}),
                    false,
                    |_, _, declaration, _| {
                        assert_eq!(declaration, &decl);
                        Ok(json!("declared"))
                    },
                )
                .unwrap(),
            json!("declared")
        );
        assert!(mount.object("demo").is_err());
        assert!(mount.state("demo").is_err());
    }

    #[test]
    fn declarations_reject_plugin_hash_and_catalog_drift() {
        let (directory, mount, _) = fixture();
        assert!(mount.schema("missing").is_err());
        let plugin = directory.path().join("plugins/demo");
        std::fs::write(plugin.join("object.hash"), "wrong-hash\n").unwrap();
        assert!(mount.schema("demo").is_err());
        std::fs::write(plugin.join("object.hash"), "object-hash\n").unwrap();
        let schema = mount.schema("demo").unwrap();
        let mut wrong = schema.clone();
        wrong.name = "other".into();
        std::fs::write(
            plugin.join("schema.json"),
            serde_json::to_vec(&wrong).unwrap(),
        )
        .unwrap();
        assert!(mount.schema("demo").is_err());
        std::fs::write(
            plugin.join("schema.json"),
            serde_json::to_vec(&schema).unwrap(),
        )
        .unwrap();
        let mut metadata = mount.metadata.clone();
        metadata["session_id"] = json!("other");
        std::fs::write(
            directory.path().join(".3tchedfs.json"),
            metadata.to_string(),
        )
        .unwrap();
        assert!(mount.schema("demo").is_err());
    }

    #[test]
    fn invocation_rejects_noncanonical_paths_before_submission() {
        let (_directory, mount, _) = fixture();
        let calls = AtomicUsize::new(0);
        for path in [
            "/plugins/demo/operations/get_status/call",
            "operations/demo/get_status/call",
            "plugins/demo/operations/get_status/result",
            "plugins/demo/operations/get_status/call/",
            "plugins/demo/../get_status/call",
            "plugins/./demo/operations/get_status/call",
            "plugins/demo//operations/get_status/call",
            "plugins/%64emo/operations/get_status/call",
            "plugins/demo/operations/missing/call",
        ] {
            assert!(
                mount
                    .invoke_with(path, json!({"event":"read"}), true, |_, _, _, _| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(Value::Null)
                    })
                    .is_err(),
                "accepted {path}"
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn one_target_lookup_does_not_open_unrelated_catalog_members() {
        let (directory, mut mount, decl) = fixture();
        mount.view["objects"]["unrelated_missing"] = json!("other-hash");
        mount.metadata["object_count"] = json!(2);
        std::fs::write(directory.path().join("view.json"), mount.view.to_string()).unwrap();
        std::fs::write(
            directory.path().join(".3tchedfs.json"),
            mount.metadata.to_string(),
        )
        .unwrap();
        assert_eq!(mount.method_decl("demo", "get_status").unwrap(), decl);
        assert!(mount.object("unrelated_missing").is_err());
        let calls = AtomicUsize::new(0);
        let expected = receipt(&decl);
        let actual = mount
            .invoke_with(
                "plugins/demo/operations/get_status/call",
                json!({"event":"read"}),
                false,
                |plugin, method, selected, args| {
                    mount.call_with(plugin, method, selected, args, |_, _| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        terminal_receipt(&expected.to_string(), Ok(()), plugin, method, selected)
                    })
                },
            )
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(actual, expected);
    }

    #[test]
    fn invocation_preserves_denial_and_rejects_malformed_arguments_without_retry() {
        let (_directory, mount, _) = fixture();
        let calls = AtomicUsize::new(0);
        let diagnostic = json!({"plugin":"demo","method":"get_status","stage":"call","error":"missing cap.demo@v1 for human"});
        let error = mount
            .invoke_with(
                "plugins/demo/operations/get_status/call",
                json!({"event":"read"}),
                false,
                |plugin, method, decl, args| {
                    mount.call_with(plugin, method, decl, args, |_, _| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        terminal_receipt(
                            &diagnostic.to_string(),
                            Err(std::io::Error::from_raw_os_error(libc::EACCES)),
                            plugin,
                            method,
                            decl,
                        )
                    })
                },
            )
            .unwrap_err();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            error.downcast_ref::<MountedDiagnostic>().unwrap().0,
            diagnostic
        );
        assert!(
            mount
                .invoke_with(
                    "plugins/demo/operations/get_status/call",
                    json!({"event":7}),
                    false,
                    |_, _, _, _| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(Value::Null)
                    }
                )
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn invocation_keeps_mutation_confirmation() {
        let (directory, mount, _) = fixture();
        let mut object = mount.object("demo").unwrap();
        object
            .schema
            .methods
            .get_mut("get_status")
            .unwrap()
            .side_effect = SideEffect::Mutation;
        std::fs::write(
            directory.path().join("plugins/demo/object.blob.json"),
            serde_json::to_vec(&object).unwrap(),
        )
        .unwrap();
        std::fs::write(
            directory.path().join("plugins/demo/schema.json"),
            serde_json::to_vec(&object.schema).unwrap(),
        )
        .unwrap();
        let calls = AtomicUsize::new(0);
        let operation = |_: &str, _: &str, _: &MethodDecl, _: Value| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!("confirmed"))
        };
        assert!(
            mount
                .invoke_with(
                    "plugins/demo/operations/get_status/call",
                    json!({"event":"write"}),
                    false,
                    operation
                )
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            mount
                .invoke_with(
                    "plugins/demo/operations/get_status/call",
                    json!({"event":"write"}),
                    true,
                    operation
                )
                .unwrap(),
            json!("confirmed")
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn mounted_identity_modes_require_exact_explicit_expectation() {
        let (directory, mount, _) = fixture();
        for mode in [MountIdentity::GuestKernel, MountIdentity::Credential] {
            let mut metadata = mount.metadata.clone();
            metadata["mount_identity"] = serde_json::to_value(&mode).unwrap();
            metadata.as_object_mut().unwrap().remove("session_id");
            std::fs::write(
                directory.path().join(".3tchedfs.json"),
                metadata.to_string(),
            )
            .unwrap();
            assert!(
                MountedNamespace::open_with_mode_and_probe(
                    directory.path(),
                    None,
                    Some(&mode),
                    mount.probe.clone()
                )
                .is_ok()
            );
            assert!(
                MountedNamespace::open_with_probe(directory.path(), None, mount.probe.clone())
                    .is_err()
            );
            let other = if mode == MountIdentity::GuestKernel {
                MountIdentity::Credential
            } else {
                MountIdentity::GuestKernel
            };
            assert!(
                MountedNamespace::open_with_mode_and_probe(
                    directory.path(),
                    None,
                    Some(&other),
                    mount.probe.clone()
                )
                .is_err()
            );
            metadata["session_id"] = json!("human");
            std::fs::write(
                directory.path().join(".3tchedfs.json"),
                metadata.to_string(),
            )
            .unwrap();
            assert!(
                MountedNamespace::open_with_mode_and_probe(
                    directory.path(),
                    None,
                    Some(&mode),
                    mount.probe.clone()
                )
                .is_err()
            );
        }
    }

    #[test]
    fn identity_catalog_and_mount_replacement_fail_closed() {
        let (directory, mut mount, _) = fixture();
        assert!(
            MountedNamespace::open_with_probe(directory.path(), Some("other"), mount.probe.clone())
                .is_err()
        );
        mount.probe = Arc::new(|_, _| Ok("mount-2".into()));
        assert!(
            mount
                .verify()
                .unwrap_err()
                .to_string()
                .contains("instance changed")
        );
        mount.probe = Arc::new(|_, _| Ok("mount-1".into()));
        let mut metadata = mount.metadata.clone();
        metadata["session_id"] = json!("other");
        std::fs::write(
            directory.path().join(".3tchedfs.json"),
            metadata.to_string(),
        )
        .unwrap();
        assert!(
            mount
                .verify()
                .unwrap_err()
                .to_string()
                .contains("declaration changed")
        );
        std::fs::write(
            directory.path().join(".3tchedfs.json"),
            mount.metadata.to_string(),
        )
        .unwrap();
        let mut view = mount.view.clone();
        view["source_catalog_generation"] = json!(2);
        std::fs::write(directory.path().join("view.json"), view.to_string()).unwrap();
        assert!(mount.verify().is_err());
    }

    #[test]
    fn malformed_arguments_do_not_open_or_execute_call() {
        let (directory, mount, decl) = fixture();
        std::fs::write(
            directory
                .path()
                .join("plugins/demo/operations/get_status/call"),
            "untouched",
        )
        .unwrap();
        let calls = AtomicUsize::new(0);
        assert!(
            mount
                .call_with("demo", "get_status", &decl, json!({"event":5}), |_, _| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(Value::Null)
                })
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            std::fs::read_to_string(
                directory
                    .path()
                    .join("plugins/demo/operations/get_status/call")
            )
            .unwrap(),
            "untouched"
        );
    }

    #[test]
    fn receipts_require_exact_contract_and_preserve_denials_without_retry() {
        let (_directory, mount, decl) = fixture();
        let valid = receipt(&decl);
        assert_eq!(
            terminal_receipt(&valid.to_string(), Ok(()), "demo", "get_status", &decl).unwrap(),
            valid
        );
        for body in ["", "{}", "{bad"] {
            assert!(terminal_receipt(body, Ok(()), "demo", "get_status", &decl).is_err());
        }
        let mut wrong = valid.clone();
        wrong["method"] = json!("other");
        assert!(terminal_receipt(&wrong.to_string(), Ok(()), "demo", "get_status", &decl).is_err());
        assert!(
            terminal_receipt(
                &valid.to_string(),
                Err(std::io::Error::from_raw_os_error(libc::EIO)),
                "demo",
                "get_status",
                &decl
            )
            .is_err()
        );
        let diagnostic = json!({"plugin":"demo","method":"get_status","stage":"call","error":"missing cap.demo@v1 for human"});
        let calls = AtomicUsize::new(0);
        let error = mount
            .call_with(
                "demo",
                "get_status",
                &decl,
                json!({"event":"read"}),
                |_, _| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    terminal_receipt(
                        &diagnostic.to_string(),
                        Err(std::io::Error::from_raw_os_error(libc::EACCES)),
                        "demo",
                        "get_status",
                        &decl,
                    )
                },
            )
            .unwrap_err();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(error.to_string().contains("missing cap.demo@v1 for human"));
        assert!(error.to_string().contains("\"stage\":\"call\""));
    }

    #[test]
    fn plain_directory_cannot_masquerade_as_fuse_and_symlinks_fail() {
        let (directory, _mount, _) = fixture();
        assert!(MountedNamespace::open(directory.path(), None).is_err());
        std::os::unix::fs::symlink("value.json", directory.path().join("plugins/demo/alias"))
            .unwrap();
        let root = File::open(directory.path()).unwrap();
        assert!(open_relative(&root, "plugins/demo/alias", false).is_err());
        // /proc is a different kernel mount and cannot stand in for a
        // projected node, even when a descriptor walk remains beneath /.
        let host_root = File::open("/").unwrap();
        assert!(open_relative(&host_root, "proc/self/status", false).is_err());
    }

    #[test]
    fn post_effect_mount_change_is_unknown_outcome_and_never_retried() {
        let (directory, mount, decl) = fixture();
        let calls = AtomicUsize::new(0);
        let error = mount
            .call_with(
                "demo",
                "get_status",
                &decl,
                json!({"event":"read"}),
                |_, _| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    std::fs::write(directory.path().join("view.json"), "{}").unwrap();
                    Ok(receipt(&decl))
                },
            )
            .unwrap_err();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(error.to_string().contains("do not retry automatically"));
    }
}
