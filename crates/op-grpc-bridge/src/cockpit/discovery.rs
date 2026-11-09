//! Read-only development discovery (tasks 1.2/1.3). No operation file is opened,
//! and no D-Bus, SHM, container-runtime, grant or lifecycle interface is called.
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::records::*;
use super::safety::{EvidenceDigest, RecordError, RecordGuard};

const MAX_DECLARATION: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCapture {
    pub interface: String,
    pub repository: String,
    pub commit: String,
    pub symbol: SourceSymbol,
    pub callers: Vec<SourceSymbol>,
    pub linked: bool,
}

/// Bytes must be read from the named commit by the development command runner,
/// not from a mutable checkout. Symbol/caller selection remains explicit source
/// research, never an invented automatic call graph.
pub fn capture_source(input: SourceCapture, pinned_bytes: &[u8], guard: &RecordGuard<'_>) -> Result<SourceInterface, RecordError> {
    if input.commit.len() != 40 || !input.commit.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)) {
        return Err(RecordError::Invalid("full source commit pin required"));
    }
    let text = std::str::from_utf8(pinned_bytes).map_err(|_| RecordError::Encoding)?;
    let location = &input.symbol.location;
    if location.first_line == 0 || location.last_line < location.first_line {
        return Err(RecordError::Invalid("invalid source location"));
    }
    let selected = text.lines().skip(location.first_line as usize - 1)
        .take((location.last_line - location.first_line + 1) as usize).collect::<Vec<_>>().join("\n");
    if !selected.contains(&input.symbol.signature) {
        return Err(RecordError::Invalid("signature absent at pinned source location"));
    }
    let source_reference = format!("{}@{}:{}", input.repository, input.commit, location.path);
    let file_digest = EvidenceDigest::from_source(guard, &source_reference, pinned_bytes)?;
    Ok(SourceInterface {
        interface: input.interface, repository: input.repository, commit: input.commit,
        symbol: input.symbol, callers: input.callers, linked: input.linked, file_digest,
    })
}

fn missing<T>(searched: Vec<String>, reason: &str) -> Finding<T> {
    Finding::NotFound { searched, reason: reason.into() }
}
fn observed<T>(value: T, evidence: String) -> Finding<T> {
    Finding::ObservedRuntime { value, evidence: vec![evidence] }
}

/// Reads only pinned declaration filenames. O_NOFOLLOW covers every path
/// component, not just the final name; the open directory pins the mount root.
fn declaration(root: &File, name: &str) -> Result<Value, RecordError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let name = std::ffi::CString::new(name).map_err(|_| RecordError::Invalid("declaration filename invalid"))?;
    let fd = unsafe { libc::openat(root.as_raw_fd(), name.as_ptr(), libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
    if fd < 0 { return Err(std::io::Error::last_os_error().into()); }
    let file = unsafe { File::from_raw_fd(fd) };
    let mut bytes = Vec::new();
    file.take(MAX_DECLARATION + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_DECLARATION { return Err(RecordError::Invalid("declaration exceeds byte bound")); }
    serde_json::from_slice(&bytes).map_err(|_| RecordError::Encoding)
}

fn root_without_links(path: &Path) -> Result<File, RecordError> {
    use std::os::fd::{AsRawFd, FromRawFd};
    if !path.is_absolute() { return Err(RecordError::Invalid("absolute mount root required")); }
    let mut root = OpenOptions::new().read(true).custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC).open("/")?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(part) => {
                use std::os::unix::ffi::OsStrExt;
                let name = std::ffi::CString::new(part.as_bytes()).map_err(|_| RecordError::Invalid("invalid mount root"))?;
                let fd = unsafe { libc::openat(root.as_raw_fd(), name.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
                if fd < 0 { return Err(std::io::Error::last_os_error().into()); }
                root = unsafe { File::from_raw_fd(fd) };
            }
            _ => return Err(RecordError::Invalid("noncanonical mount root")),
        }
    }
    Ok(root)
}

fn mount_source(path: &str) -> Finding<String> {
    let observation = (|| -> Option<String> {
        let mut file = File::open("/proc/self/mountinfo").ok()?.take(MAX_DECLARATION);
        let mut text = String::new();
        file.read_to_string(&mut text).ok()?;
        let mut sources = text.lines().filter_map(|line| {
            let (before, after) = line.split_once(" - ")?;
            let mount = before.split_whitespace().nth(4)?;
            let mut fields = after.split_whitespace();
            let filesystem = fields.next()?;
            let source = fields.next()?;
            (mount == path && filesystem == "fuse.3tchedFS").then(|| source.to_owned())
        });
        let source = sources.next()?;
        sources.next().is_none().then_some(source)
    })();
    match observation {
        Some(source) => observed(source, "/proc/self/mountinfo".into()),
        None => missing(vec!["/proc/self/mountinfo".into()], "no unique matching fuse.3tchedFS mount observed"),
    }
}

fn mount_owner(path: &str) -> Finding<String> {
    let observation = (|| -> Option<String> {
        let mut candidates = Vec::new();
        for entry in std::fs::read_dir("/proc").ok()? {
            let Ok(entry) = entry else { continue; };
            let name = entry.file_name();
            let Some(pid) = name.to_str().filter(|name| name.bytes().all(|c| c.is_ascii_digit())) else { continue; };
            let Ok(mut command) = File::open(entry.path().join("cmdline")) else { continue; };
            let mut bytes = Vec::new();
            if command.by_ref().take(64 * 1024).read_to_end(&mut bytes).is_err() { continue; }
            let args: Vec<_> = bytes.split(|c| *c == 0).collect();
            let executable = args.first().and_then(|name| std::str::from_utf8(name).ok())
                .and_then(|name| Path::new(name).file_name()).and_then(|name| name.to_str());
            if executable == Some("3tched-fs") && args.iter().any(|arg| *arg == path.as_bytes()) {
                // Process arguments establish a candidate, not ownership of the
                // kernel mount. Never upgrade this to observed ownership.
                candidates.push(format!("pid:{pid}"));
            }
        }
        (candidates.len() == 1).then(|| candidates.remove(0))
    })();
    match observation {
        Some(candidate) => Finding::SourceDefinitionOnly { value: candidate,
            evidence: vec!["/proc/*/cmdline (mount-path candidate only; kernel ownership unproven)".into()] },
        None => missing(vec!["/proc/*/cmdline".into()], "no unique readable filesystem process candidate"),
    }
}

pub fn collect_runtime(header: RecordHeader) -> RuntimeRecord {
    let mut findings = Vec::new();
    let views = [ViewKind::ThreeTched, ViewKind::Threetched].into_iter().map(|view| {
        let path = view.mount_path();
        let attempt = (|| -> Result<MountedViewObservation, RecordError> {
            let root = root_without_links(Path::new(path))?;
            let metadata = declaration(&root, ".3tchedfs.json")?;
            let catalog = declaration(&root, "view.json")?;
            if metadata["format"] != "3tchedfs/1" || metadata["workspace_id"] != Value::Null
                || metadata["view_id"] != catalog["view_id"]
                || metadata["source_catalog_hash"] != catalog["source_catalog_hash"]
                || metadata["source_catalog_generation"] != catalog["source_catalog_generation"] {
                return Err(RecordError::Invalid("mount declaration and catalog disagree"));
            }
            let hash = metadata["source_catalog_hash"].as_str().ok_or(RecordError::Invalid("catalog hash absent"))?;
            let generation = metadata["source_catalog_generation"].as_u64().ok_or(RecordError::Invalid("catalog generation absent"))?;
            findings.push(RuntimeFinding { id: format!("{view:?}-mounted-projection"), class: EvidenceClass::MountedProjection,
                view: Some(view), observation: observed("declaration and catalog present; not identity or operation proof".into(), format!("{path}/.3tchedfs.json;{path}/view.json")) });
            let mode = metadata["mount_identity"]["kind"].as_str().ok_or(RecordError::Invalid("mount identity mode absent"))?;
            // guest_kernel is a declared mode, not proof that a call was bound.
            let identity = missing(vec![format!("{path}/.3tchedfs.json")],
                if mode == "guest_kernel" { "guest-kernel mode declared; no authenticated tracked identity read observed" }
                else { "accessible mount does not declare guest-kernel mode; no cockpit authority inferred" });
            Ok(MountedViewObservation {
                view, mount_path: path.into(),
                kernel_mount_source: mount_source(path),
                owning_process: mount_owner(path),
                catalog_pin: observed(CatalogPin { hash: hash.into(), generation }, format!("{path}/.3tchedfs.json")),
                guest_kernel_identity: identity,
            })
        })();
        match attempt {
            Ok(value) => observed(value, format!("{path}/.3tchedfs.json;{path}/view.json")),
            Err(_) => missing(vec![format!("{path}/.3tchedfs.json"), format!("{path}/view.json")], "mounted declarations absent, denied, inconsistent or invalid; no alternate path attempted"),
        }
    }).collect();
    let absent = || missing(vec!["identity-bound cockpit passive projections".into()], "not established for the cockpit; host declarations are not cockpit evidence");
    RuntimeRecord {
        header,
        preservation: PreservationObservation {
            cockpit_name: absent(), session: absent(), native_uuid: absent(), principal_id: absent(),
            grants_identity: absent(), credential_identity: absent(), seed: absent(), leaf: absent(),
            btrfs_members: missing(vec!["cockpit passive projections".into()], "membership unobserved"),
            subvolumes: missing(vec!["cockpit passive projections".into()], "seven-subvolume set unobserved"),
            host_channels: missing(vec!["cockpit passive projections".into()], "guest channels unobserved"),
            listeners: missing(vec!["cockpit passive projections".into()], "guest listeners unobserved"),
        },
        views, findings, tool_calls: Vec::new(),
    }
}
