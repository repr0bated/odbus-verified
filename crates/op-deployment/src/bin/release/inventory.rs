use anyhow::{Context, Result, bail, ensure};
use op_cozo_store::{GraphSnapshot, GraphProvenanceKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, BTreeSet}, fs::File, os::{fd::{AsFd, AsRawFd}, unix::fs::MetadataExt}, path::{Component, Path, PathBuf}};
use super::{fs, model::{GRAPH, Target}};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Entry {
    pub path: String, pub file_type: String, pub mode: u32, pub uid: u32, pub gid: u32,
    pub size: Option<u64>, pub sha256: Option<String>, pub hash_status: Option<String>,
    pub target: Option<String>, pub release_target: Option<String>, pub consumers: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Service {
    pub release_owned: bool, pub deps: Vec<String>, pub starts: Vec<String>,
    #[serde(default)] pub declarations: Vec<String>,
    #[serde(default)] pub probe: Option<Vec<String>>,
}
pub struct Inventory {
    pub graph_sha256: String, pub tree_sha256: String, pub catalog_sha256: String,
    pub targets: Vec<Target>, pub entries: BTreeMap<String, Entry>, pub services: BTreeMap<String, Service>,
}
pub fn normalized(path: &Path, absolute: bool) -> Result<()> {
    ensure!(path.is_absolute() == absolute && !path.as_os_str().is_empty(), "invalid path {}", path.display());
    let text = path.to_str().context("non-UTF8 path")?;
    ensure!(!text.chars().any(char::is_control) && !text.contains("//") && !text.ends_with('/'), "non-normal path");
    ensure!(path.components().all(|c| matches!(c, Component::Normal(_) | Component::RootDir)), "non-normal path");
    ensure!(!text.split('/').any(|c| c == "." || c == ".."), "dot path component");
    Ok(())
}
pub fn service_name(name: &str) -> Result<()> {
    ensure!(!name.is_empty() && name.len() <= 128 && !name.starts_with('.') && !name.ends_with('.')
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)), "invalid service name"); Ok(())
}
pub fn protected(path: &str) -> bool {
    path.split('/').any(|s| matches!(s, "secrets" | "credentials" | "private"))
        || path.ends_with(".key") || path.ends_with(".env") || path.ends_with("/environment")
}
pub fn validate_targets(targets: &[Target]) -> Result<()> {
    ensure!(!targets.is_empty(), "release has no target directories");
    for (i, t) in targets.iter().enumerate() {
        normalized(&t.path, true)?; normalized(&t.source, false)?;
        for protected in ["/", "/usr", "/usr/bin", "/etc", "/etc/op-dbus", "/etc/opdbus", "/etc/emqx", "/etc/sv", "/etc/runit", "/usr/local/sbin", "/usr/local/libexec/opdbus-release", "/usr/local/libexec/opdbus-storage", "/var/lib/threetched", "/var/lib/opdbus/plugin-blobs"] {
            let protected = Path::new(protected);
            ensure!(!protected.starts_with(&t.path), "target covers protected host content: {}", t.path.display());
            if protected != Path::new("/") && protected != Path::new("/usr") {
                ensure!(!t.path.starts_with(protected), "target lies in protected host content: {}", t.path.display());
            }
        }
        for other in &targets[..i] {
            ensure!(!t.path.starts_with(&other.path) && !other.path.starts_with(&t.path), "nested or duplicate target");
            ensure!(!t.source.starts_with(&other.source) && !other.source.starts_with(&t.source), "nested or duplicate lower directory");
        }
    }
    Ok(())
}
fn read_file(root: &File, path: &str, limit: usize) -> Result<Vec<u8>> {
    fs::read_bounded(&mut fs::open_beneath(root.as_fd(), Path::new(path), libc::O_RDONLY)?, limit)
}
pub fn load(root: &File) -> Result<Inventory> {
    let bytes = read_file(root, GRAPH, 64 * 1024 * 1024)?;
    let graph_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let manifest = String::from_utf8(read_file(root, "MANIFEST", 1024 * 1024)?)?;
    let hashes: Vec<_> = manifest.lines().filter_map(|l| l.strip_prefix("system-graph-sha256: ")).collect();
    ensure!(hashes == [graph_sha256.as_str()], "MANIFEST does not uniquely pin release graph");
    ensure!(manifest.lines().filter(|l| *l == format!("system-graph-path: {GRAPH}")).count() == 1, "MANIFEST graph path mismatch");
    let graph: GraphSnapshot = serde_json::from_slice(&bytes)?;
    ensure!(graph.format_version == 1 && graph.provenance.kind == GraphProvenanceKind::PreparedRelease, "not a prepared-release graph");
    ensure!(graph.nodes.len() <= 50_000 && graph.edges.len() <= 200_000, "graph bound exceeded");
    let mut ids = BTreeSet::new();
    let mut targets = Vec::new(); let mut entries = BTreeMap::new(); let mut services = BTreeMap::new();
    for node in graph.nodes {
        ensure!(ids.insert(node.id.clone()), "duplicate graph node");
        let value = serde_json::to_value(&node.attributes)?;
        match node.kind.as_str() {
            "release_target" => {
                let path = PathBuf::from(value["path"].as_str().context("target path absent")?);
                ensure!(node.id == format!("target:{}", path.display()), "target node ID mismatch");
                targets.push(Target { path, source: PathBuf::from(value["source"].as_str().context("target source absent")?),
                    noexec: value["noexec"].as_bool().context("target noexec absent")?, parent: None });
            }
            "prepared_path" => {
                let entry: Entry = serde_json::from_value(value)?;
                normalized(Path::new(&entry.path), false)?;
                ensure!(node.id == format!("file:{}", entry.path), "prepared node ID mismatch");
                ensure!(entries.insert(entry.path.clone(), entry).is_none(), "duplicate prepared path");
            }
            "runit_service" => {
                let name = node.id.strip_prefix("service:").context("service ID mismatch")?;
                service_name(name)?;
                let service: Service = serde_json::from_value(value)?;
                for dep in service.deps.iter().chain(&service.starts) { service_name(dep)?; }
                if let Some(probe) = &service.probe { ensure!(!probe.is_empty() && Path::new(&probe[0]).is_absolute(), "probe must be an absolute argv"); }
                services.insert(name.to_owned(), service);
            }
            _ => {}
        }
    }
    targets.sort_by(|a,b| a.path.components().count().cmp(&b.path.components().count()).then(a.path.cmp(&b.path)));
    validate_targets(&targets)?;
    for (name, s) in &services {
        for dep in s.deps.iter().chain(&s.starts) { ensure!(services.contains_key(dep), "{name} references undeclared service {dep}"); }
    }
    for entry in entries.values() {
        let t = targets.iter().find(|t| Path::new(&entry.path).starts_with(&t.source));
        ensure!(entry.release_target.as_deref() == t.and_then(|t| t.path.to_str()), "entry target attribution differs: {}", entry.path);
        for consumer in &entry.consumers {
            ensure!(services.get(consumer).is_some_and(|s| s.release_owned), "entry names non-release consumer {consumer}");
        }
        if protected(&entry.path) {
            ensure!(entry.sha256.is_none(), "protected content was hashed");
            if entry.file_type == "regular" {
                ensure!(entry.hash_status.as_deref() == Some("protected_content_not_read"), "protected regular content lacks no-read marker");
            }
            ensure!(entry.release_target.is_none(), "protected content cannot ship in a release target: {}", entry.path);
        }
    }
    let info = btrfs_uapi::subvolume::subvolume_info(root.as_fd())?;
    let device = root.metadata()?.dev();
    for target in &targets {
        let executable = entries.values().any(|e| Path::new(&e.path).starts_with(&target.source)
            && e.file_type == "regular" && e.mode & 0o111 != 0);
        ensure!(target.noexec == !executable, "target noexec does not match inventory executability: {}", target.path.display());
        let lower = fs::open_beneath(root.as_fd(), &target.source, libc::O_RDONLY | libc::O_DIRECTORY)?;
        ensure!(lower.metadata()?.dev() == device && btrfs_uapi::subvolume::subvolume_info(lower.as_fd())?.id == info.id, "lower crosses a filesystem/subvolume");
    }
    let mut actual = BTreeMap::new();
    let mut digest = Sha256::new();
    walk(root, Path::new(""), device, info.id, &mut actual, &mut digest, 0)?;
    for (path, entry) in &entries {
        let observed = actual.remove(path).with_context(|| format!("inventory path missing: {path}"))?;
        ensure!(entry.file_type == observed.file_type && entry.mode == observed.mode && entry.uid == observed.uid && entry.gid == observed.gid,
            "inventory metadata mismatch: {path}");
        if entry.file_type == "regular" && path != "MANIFEST" { ensure!(entry.size == observed.size, "inventory size mismatch: {path}"); }
        ensure!(entry.target == observed.target, "inventory symlink mismatch: {path}");
        if !protected(path) && entry.file_type == "regular" && path != "MANIFEST" {
            ensure!(entry.sha256.is_some() && entry.sha256 == observed.sha256, "inventory digest mismatch: {path}");
        }
    }
    ensure!(actual.is_empty(), "unlisted release paths: {:?}", actual.keys().take(8).collect::<Vec<_>>());
    let catalog_sha256 = fs::hash_file(&mut fs::open_beneath(root.as_fd(), Path::new("usr/share/op-dbus/plugin-blobs/.manifest.json"), libc::O_RDONLY)?)?;
    Ok(Inventory { graph_sha256, tree_sha256: format!("{:x}", digest.finalize()), catalog_sha256, targets, entries, services })
}
fn walk(dir: &File, prefix: &Path, device: u64, subvolume: u64, entries: &mut BTreeMap<String, Entry>, digest: &mut Sha256, depth: usize) -> Result<()> {
    ensure!(depth <= 64 && entries.len() <= 50_000, "release tree bound exceeded");
    fs::reject_overlay_metadata(dir.as_fd())?;
    let base = PathBuf::from(format!("/proc/self/fd/{}", dir.as_raw_fd()));
    let mut names = std::fs::read_dir(&base)?.map(|e| e.map(|e| e.file_name())).collect::<std::io::Result<Vec<_>>>()?;
    names.sort();
    for name in names {
        let rel = prefix.join(&name);
        let path = rel.to_str().context("non-UTF8 release path")?.to_owned();
        ensure!(name != ".wh..wh..opq", "opaque lower-tree marker is not a release artifact");
        normalized(&rel, false)?;
        let meta = std::fs::symlink_metadata(base.join(&name))?;
        ensure!(meta.dev() == device, "release tree crosses device");
        let kind = if meta.is_dir() { "directory" } else if meta.is_file() { "regular" } else if meta.file_type().is_symlink() { "symlink" } else { bail!("special release file: {path}") };
        let mut entry = Entry { path: path.clone(), file_type: kind.into(), mode: meta.mode() & 0o7777,
            uid: meta.uid(), gid: meta.gid(), size: meta.is_file().then_some(meta.len()), sha256: None,
            hash_status: None, target: None, release_target: None, consumers: Vec::new() };
        if meta.file_type().is_symlink() {
            fs::reject_symlink_overlay_metadata(dir.as_fd(), Path::new(&name))?;
            entry.target = Some(std::fs::read_link(base.join(&name))?.to_str().context("non-UTF8 symlink")?.into());
        } else {
            let mut file = fs::open_beneath(dir.as_fd(), Path::new(&name), libc::O_RDONLY | if meta.is_dir() { libc::O_DIRECTORY } else { 0 })?;
            fs::reject_overlay_metadata(file.as_fd())?;
            if meta.is_dir() {
                ensure!(btrfs_uapi::subvolume::subvolume_info(file.as_fd())?.id == subvolume, "nested release subvolume: {path}");
                walk(&file, &rel, device, subvolume, entries, digest, depth + 1)?;
            } else if !protected(&path) { entry.sha256 = Some(fs::hash_file(&mut file)?); }
        }
        digest.update(serde_json::to_vec(&entry)?); digest.update([0]);
        if path != "usr/share/op-dbus/graphs" && !path.starts_with("usr/share/op-dbus/graphs/") { entries.insert(path, entry); }
    }
    Ok(())
}
pub fn changed(old: Option<&Entry>, new: Option<&Entry>) -> bool {
    let (Some(a), Some(b)) = (old, new) else { return true; };
    if a.file_type != b.file_type || a.mode != b.mode || a.uid != b.uid || a.gid != b.gid { return true; }
    match a.file_type.as_str() {
        "directory" => false,
        "symlink" => a.target != b.target,
        _ => a.sha256.is_none() || b.sha256.is_none() || a.size != b.size || a.sha256 != b.sha256,
    }
}
