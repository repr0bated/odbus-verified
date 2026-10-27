//! Explicit base-system payloads. Never infer rootfs writes from overlay inventory.
//!
//! The manifest is graph-pinned and has a separate, deliberately narrow host
//! destination policy. No overlay inventory entry implicitly authorizes a write.
//! Originals are streamed into private durable backups; journal records contain
//! hashes/metadata, never binary contents. An ambiguous recovery keeps evidence.
//! Committed receipts retain backups and the pre-reboot boot ID. Interrupted kit
//! publication may require the authorized bootstrap kit before strict launcher
//! digest gates admit native recovery; those gates are never bypassed here.

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, ffi::CString, fs::File, io::{Seek, SeekFrom},
    os::{fd::{AsFd, AsRawFd, FromRawFd}, unix::{ffi::OsStrExt, fs::MetadataExt}},
    path::{Path, PathBuf}, process::Command};
use super::{fs, inventory::{self, Inventory}, model::*, mounts, preserve, services};

const KIT_MANIFEST: &str = "/usr/local/libexec/opdbus-release/manifest.sha256";
const KIT: [&str; 4] = [
    "/usr/local/libexec/opdbus-release/op-release",
    "/usr/local/libexec/opdbus-release/op-release-launch",
    "/etc/init.d/op-release-activate", KIT_MANIFEST,
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SystemFile {
    pub source: PathBuf, pub path: PathBuf, pub sha256: String, pub mode: u32,
    #[serde(default)] pub install_if_absent: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest { version: u32, files: Vec<SystemFile> }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Fingerprint { sha256: String, mode: u32, uid: u32, gid: u32 }
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub enum Identity {
    Btrfs { uuid: String, subvolume: u64, inode: u64 },
    Other { device: u64, inode: u64 },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileChange {
    entry: SystemFile, before: Option<Fingerprint>, after: Fingerprint,
    backup: PathBuf, temporary: PathBuf, parent: Option<Identity>,
    #[serde(default)]
    backup_parent: Option<Identity>,
    started: bool, reverted: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryChange {
    path: PathBuf, temporary: PathBuf, parent: Option<Identity>,
    identity: Option<Identity>, started: bool, removed: bool,
    #[serde(default = "directory_mode")]
    mode: u32,
}
fn directory_mode() -> u32 { 0o755 }
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Transaction {
    pub manifest_sha256: String, files: Vec<FileChange>, directories: Vec<DirectoryChange>,
    #[serde(default)]
    backup_directory: Option<PathBuf>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub release: String, pub transaction: Transaction,
    pub previous_boot_id: String, pub reboot_requested: bool,
}
pub fn receipt(release: &str, transaction: &Transaction) -> Result<Receipt> {
    let boot_id = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    let boot_id = uuid::Uuid::parse_str(boot_id.trim())?.to_string();
    Ok(Receipt { release: release.into(), transaction: transaction.clone(), previous_boot_id: boot_id, reboot_requested: false })
}
fn backup_fingerprint(before: &Fingerprint) -> Fingerprint {
    Fingerprint { mode: 0o600, ..before.clone() }
}

fn overlaps(a: &Path, b: &Path) -> bool { a.starts_with(b) || b.starts_with(a) }
fn service_member(path: &Path) -> Option<(&str, &str)> {
    let suffix = path.strip_prefix(SERVICES).ok()?.to_str()?;
    let (name, member) = suffix.split_once('/')?;
    if inventory::service_name(name).is_ok() && matches!(member, "run" | "down") { Some((name, member)) } else { None }
}
fn validate_destination(entry: &SystemFile, targets: &[PathBuf], overlays: &[PathBuf]) -> Result<()> {
    inventory::normalized(&entry.path, true)?;
    // Independent bootstrap/authorization policy is never an update payload.
    // The controller kit and fixed declared service dispatchers are the approved
    // base-system surface, not a general root-filesystem extraction facility.
    ensure!(KIT.iter().any(|p| entry.path == Path::new(p)) || service_member(&entry.path).is_some(),
        "system destination is outside the approved base files: {}", entry.path.display());
    ensure!(!inventory::protected(entry.path.to_str().context("non-UTF8 destination")?), "protected system destination");
    ensure!(targets.iter().chain(overlays).all(|p| !overlaps(&entry.path, p)),
        "system destination overlaps an overlay target or mount: {}", entry.path.display());
    ensure!(matches!(entry.mode, 0o600 | 0o644 | 0o700 | 0o755), "unsupported system file mode");
    ensure!(!entry.install_if_absent || service_member(&entry.path).is_some(), "install_if_absent is only supported for service base files");
    Ok(())
}
fn validate_manifest(manifest: &Manifest, targets: &[PathBuf], overlays: &[PathBuf]) -> Result<()> {
    ensure!(manifest.version == 1 && !manifest.files.is_empty() && manifest.files.len() <= 1024, "invalid system manifest version or file count");
    let mut sources = BTreeSet::new();
    for (index, entry) in manifest.files.iter().enumerate() {
        inventory::normalized(&entry.source, false)?;
        ensure!(entry.source.starts_with("system") && entry.source.components().count() > 1, "system source must be below system/");
        ensure!(sources.insert(&entry.source), "duplicate system source");
        ensure!(entry.sha256.len() == 64 && entry.sha256.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)), "invalid system SHA256");
        validate_destination(entry, targets, overlays)?;
        ensure!(manifest.files[..index].iter().all(|other| !overlaps(&entry.path, &other.path)), "duplicate or nested system destination");
        if entry.path == Path::new(KIT_MANIFEST) { ensure!(index + 1 == manifest.files.len(), "controller kit manifest must be applied last"); }
    }
    let kit_count = manifest.files.iter().filter(|e| KIT.iter().any(|p| e.path == Path::new(p))).count();
    ensure!(kit_count == 0 || kit_count == KIT.len(), "controller kit must contain exactly all three files and its manifest");
    Ok(())
}
fn missing(error: &anyhow::Error) -> bool {
    error.downcast_ref::<std::io::Error>().is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
}
fn identity(file: &File) -> Result<Identity> {
    let metadata = file.metadata()?;
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: stat is writable and the borrowed descriptor remains live.
    syscall(unsafe { libc::fstatfs(file.as_raw_fd(), stat.as_mut_ptr()) })?;
    // SAFETY: successful fstatfs initialized stat.
    let stat = unsafe { stat.assume_init() };
    if stat.f_type == 0x9123_683e {
        // Btrfs anonymous device numbers can change across reboot. Persistent
        // filesystem/subvolume/inode identity must survive recovery on a new boot.
        Ok(Identity::Btrfs {
            uuid: btrfs_uapi::filesystem::filesystem_info(file.as_fd())?.uuid.to_string(),
            subvolume: btrfs_uapi::subvolume::subvolume_info(file.as_fd())?.id,
            inode: metadata.ino(),
        })
    } else { Ok(Identity::Other { device: metadata.dev(), inode: metadata.ino() }) }
}
fn check_file(file: &File) -> Result<()> {
    let m = file.metadata()?;
    ensure!(m.is_file() && m.nlink() == 1 && m.uid() == 0 && m.gid() == 0 && m.mode() & 0o7022 == 0,
        "system file must be regular, root:root, single-link and non-writable by other users");
    reject_xattrs(file)
}
fn reject_xattrs(file: &File) -> Result<()> {
    // SAFETY: size query on a live descriptor; metadata we cannot restore is rejected.
    let length = unsafe { libc::flistxattr(file.as_raw_fd(), std::ptr::null_mut(), 0) };
    ensure!(length == 0, "system files with extended attributes (or unreadable attributes) are unsupported");
    Ok(())
}
fn fingerprint(file: &mut File) -> Result<Fingerprint> {
    let m = file.metadata()?;
    ensure!(m.is_file() && m.nlink() == 1, "not a single-link regular system file");
    reject_xattrs(file)?;
    Ok(Fingerprint { sha256: fs::hash_file(file)?, mode: m.mode() & 0o7777, uid: m.uid(), gid: m.gid() })
}
fn verify_digest(file: &mut File, expected: &str) -> Result<()> {
    ensure!(fs::hash_file(file)? == expected, "system payload digest mismatch"); Ok(())
}
fn inspect_at(parent: &File, name: &Path) -> Result<Option<Fingerprint>> {
    match fs::open_beneath(parent.as_fd(), name, libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOATIME) {
        Ok(mut file) => Ok(Some(fingerprint(&mut file)?)),
        Err(e) if missing(&e) => Ok(None), Err(e) => Err(e),
    }
}
fn inspect(path: &Path) -> Result<Option<Fingerprint>> {
    match fs::secure_absolute(path, libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOATIME) {
        Ok(mut file) => { check_file(&file)?; Ok(Some(fingerprint(&mut file)?)) },
        Err(e) if missing(&e) => Ok(None), Err(e) => Err(e),
    }
}
fn existing_parent(path: &Path) -> Result<Option<Identity>> {
    match fs::secure_absolute(path, libc::O_RDONLY | libc::O_DIRECTORY) {
        Ok(file) => {
            ensure!(file.metadata()?.gid() == 0, "system parent must be root:root");
            reject_xattrs(&file)?;
            Ok(Some(identity(&file)?))
        },
        Err(e) if missing(&e) => Ok(None), Err(e) => Err(e),
    }
}
fn overlay_paths() -> Result<Vec<PathBuf>> {
    Ok(mounts::table()?.into_iter().filter(|m| m.kind == "overlay").map(|m| m.path).collect())
}
pub fn plan(state: &State, root: &File, inventory: &Inventory) -> Result<Transaction> {
    let mut manifest_file = fs::open_beneath(root.as_fd(), Path::new("system-files.json"), libc::O_RDONLY | libc::O_NOATIME)?;
    check_file(&manifest_file)?;
    let bytes = fs::read_bounded(&mut manifest_file, 1024 * 1024)?;
    let manifest_sha256 = format!("{:x}", Sha256::digest(&bytes));
    ensure!(inventory.entries.get("system-files.json").and_then(|e| e.sha256.as_ref()) == Some(&manifest_sha256),
        "system manifest is not pinned by the verified release graph");
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    let mut targets: Vec<_> = inventory.targets.iter().map(|t| t.path.clone()).collect();
    if let Some(active) = state.active { targets.extend(state.release(active)?.targets.iter().map(|t| t.path.clone())); }
    validate_manifest(&manifest, &targets, &overlay_paths()?)?;
    if let Some(kit_manifest) = manifest.files.iter().find(|e| e.path == Path::new(KIT_MANIFEST)) {
        let mut expected = String::new();
        for path in &KIT[..3] {
            let entry = manifest.files.iter().find(|e| e.path == Path::new(path)).context("incomplete controller kit")?;
            ensure!(entry.mode == 0o755, "controller executable must be mode 0755");
            expected.push_str(&format!("{}  {path}\n", entry.sha256));
        }
        ensure!(kit_manifest.mode == 0o600, "controller digest manifest must be mode 0600");
        let mut file = fs::open_beneath(root.as_fd(), &kit_manifest.source, libc::O_RDONLY | libc::O_NOATIME)?;
        ensure!(fs::read_bounded(&mut file, 4096)? == expected.as_bytes(), "kit manifest must pin exactly the three declared controller files");
    }
    let transaction = uuid::Uuid::new_v4();
    let mut files = Vec::new(); let mut directories = Vec::new(); let mut missing_dirs = BTreeSet::new();
    for (index, entry) in manifest.files.into_iter().enumerate() {
        let mut source = fs::open_beneath(root.as_fd(), &entry.source, libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOATIME)?;
        check_file(&source)?; verify_digest(&mut source, &entry.sha256)?;
        ensure!(inventory.entries.get(entry.source.to_str().context("source encoding")?).and_then(|e| e.sha256.as_ref()) == Some(&entry.sha256),
            "system payload is not pinned by the verified release graph");
        if let Some((name, member)) = service_member(&entry.path) {
            ensure!(inventory.services.get(name).is_some_and(|s| s.release_owned), "system dispatcher service is not declared release-owned");
            ensure!(entry.install_if_absent, "existing service dispatchers and down markers must never be overwritten");
            let expected = if member == "run" { preserve::stub(name, "run") } else { String::new() };
            ensure!(entry.sha256 == format!("{:x}", Sha256::digest(expected.as_bytes()))
                && entry.mode == if member == "run" { 0o755 } else { 0o644 }, "invalid fixed service launcher or down marker");
            ensure!(inventory.entries.contains_key(&format!("sv/{name}/run")), "dispatcher has no received service implementation");
        }
        let before = inspect(&entry.path)?;
        if let Some((name, "down")) = service_member(&entry.path) {
            // The package is host-independent. An existing run file means this
            // service already has an operator-owned wanted state, even if it
            // intentionally has no down marker.
            if inspect(&Path::new(SERVICES).join(name).join("run"))?.is_some() { continue; }
        }
        if entry.install_if_absent && before.is_some() { continue; }
        let after = Fingerprint { sha256: entry.sha256.clone(), mode: entry.mode, uid: 0, gid: 0 };
        if before.as_ref() == Some(&after) { continue; }
        let parent_path = entry.path.parent().context("system destination has no parent")?;
        let parent = existing_parent(parent_path)?;
        let mut absent = Vec::new(); let mut ancestor = parent_path;
        while existing_parent(ancestor)?.is_none() {
            absent.push(ancestor.to_path_buf()); ancestor = ancestor.parent().context("missing filesystem root")?;
        }
        for path in absent.into_iter().rev() {
            if missing_dirs.insert(path.clone()) {
                directories.push(DirectoryChange { parent: existing_parent(path.parent().unwrap())?, path,
                    temporary: format!(".op-release-{transaction}-dir-{}", directories.len()).into(),
                    identity: None, started: false, removed: false, mode: 0o755 });
            }
        }
        files.push(FileChange { entry, before, after, parent, backup_parent: None,
            backup: format!(".op-release-{transaction}-{index}.before").into(),
            temporary: format!(".op-release-{transaction}-{index}.next").into(), started: false, reverted: false });
    }
    // Installing a down marker must not change an existing service's wanted state.
    for file in &files {
        if let Some((name, "down")) = service_member(&file.entry.path) {
            ensure!(!services::linked(name)? && inspect(&Path::new(SERVICES).join(name).join("run"))?.is_none(),
                "a new down marker is only valid for a new, disabled service");
        }
    }
    let backup_directory = if files.iter().any(|file| file.before.is_some()) {
        let path = Path::new(STATE).parent().unwrap().join(format!("system-backups-{transaction}"));
        let parent = existing_parent(path.parent().unwrap())?.context("release state directory is missing")?;
        ensure!(existing_parent(&path)?.is_none(), "system backup directory already exists");
        directories.push(DirectoryChange { path: path.clone(), temporary: format!(".system-backups-{transaction}").into(),
            parent: Some(parent), identity: None, started: false, removed: false, mode: 0o700 });
        Some(path)
    } else { None };
    Ok(Transaction { manifest_sha256, files, directories, backup_directory })
}
pub fn supplies_dispatcher(transaction: Option<&Transaction>, name: &str) -> bool {
    transaction.is_some_and(|t| t.files.iter().any(|f| f.entry.path == Path::new(SERVICES).join(name).join("run") && f.before.is_none()))
}
pub fn installed_down(transaction: Option<&Transaction>, name: &str) -> bool {
    transaction.is_some_and(|t| t.files.iter().any(|f| f.entry.path == Path::new(SERVICES).join(name).join("down")
        && f.before.is_none() && f.started && !f.reverted))
}
pub fn owns_directory(transaction: Option<&Transaction>, path: &Path) -> bool {
    transaction.is_some_and(|t| t.directories.iter().any(|d| d.path == path))
}
fn name(path: &Path) -> Result<CString> {
    fs::relative(path)?; ensure!(path.components().count() == 1, "single component required");
    Ok(CString::new(path.as_os_str().as_bytes())?)
}
fn syscall(result: i32) -> Result<()> {
    if result < 0 { Err(std::io::Error::last_os_error().into()) } else { Ok(()) }
}
fn stage_copy(parent: &File, destination: &Path, source: &mut File, mode: u32) -> Result<()> {
    let destination = name(destination)?;
    // An unnamed inode prevents partial backups/stages surviving a crash. Only
    // fully written and synced files acquire the transaction-owned stage name.
    // SAFETY: parent is pinned; O_TMPFILE creates a new owned inode.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), c".".as_ptr(), libc::O_TMPFILE | libc::O_RDWR | libc::O_CLOEXEC, 0o600) };
    ensure!(fd >= 0, "create unnamed system stage: {}", std::io::Error::last_os_error());
    // SAFETY: successful openat transfers fd ownership.
    let mut file = unsafe { File::from_raw_fd(fd) };
    // Inherited default ACLs must fail while the inode is still unnamed, so
    // recovery never encounters an unsupported named stage that it cannot inspect.
    reject_xattrs(&file)?;
    let metadata = source.metadata()?;
    source.seek(SeekFrom::Start(0))?;
    std::io::copy(source, &mut file)?;
    // SAFETY: apply exact mode to our owned inode.
    syscall(unsafe { libc::fchmod(file.as_raw_fd(), mode) })?;
    let times = [
        libc::timespec { tv_sec: metadata.atime(), tv_nsec: metadata.atime_nsec() },
        libc::timespec { tv_sec: metadata.mtime(), tv_nsec: metadata.mtime_nsec() },
    ];
    // SAFETY: preserve the source's access/modification times on our new inode.
    syscall(unsafe { libc::futimens(file.as_raw_fd(), times.as_ptr()) })?;
    file.sync_all()?;
    let descriptor_path = CString::new(format!("/proc/self/fd/{}", file.as_raw_fd()))?;
    // SAFETY: this kernel-owned link names our live unnamed descriptor, not a
    // caller-supplied symlink; the destination is beneath the pinned parent.
    syscall(unsafe { libc::linkat(libc::AT_FDCWD, descriptor_path.as_ptr(), parent.as_raw_fd(), destination.as_ptr(), libc::AT_SYMLINK_FOLLOW) })?;
    Ok(parent.sync_all()?)
}
fn replace_at(parent: &File, temporary: &Path, destination: &Path) -> Result<()> {
    let temporary = name(temporary)?; let destination = name(destination)?;
    // SAFETY: both single-component names refer to the same pinned parent.
    syscall(unsafe { libc::renameat(parent.as_raw_fd(), temporary.as_ptr(), parent.as_raw_fd(), destination.as_ptr()) })?;
    Ok(parent.sync_all()?)
}
fn pinned_parent(path: &Path, expected: Option<&Identity>) -> Result<File> {
    let parent = fs::secure_absolute(path.parent().context("system path has no parent")?, libc::O_RDONLY | libc::O_DIRECTORY)?;
    reject_xattrs(&parent)?;
    if let Some(expected) = expected { ensure!(&identity(&parent)? == expected, "system parent identity changed"); }
    Ok(parent)
}
fn backup_parent(transaction: &Transaction, expected: Option<&Identity>) -> Result<File> {
    let path = transaction.backup_directory.as_ref().context("system backup directory is not recorded")?;
    let directory = fs::secure_absolute(path, libc::O_RDONLY | libc::O_DIRECTORY)?;
    reject_xattrs(&directory)?;
    let created = transaction.directories.iter().find(|change| &change.path == path)
        .and_then(|change| change.identity.as_ref()).context("system backup directory creation identity is missing")?;
    ensure!(&identity(&directory)? == created, "system backup directory is not the transaction-created directory");
    let metadata = directory.metadata()?;
    ensure!(metadata.uid() == 0 && metadata.gid() == 0 && metadata.mode() & 0o7777 == 0o700,
        "system backup directory must be private root:root 0700");
    if let Some(expected) = expected { ensure!(&identity(&directory)? == expected, "system backup directory identity changed"); }
    Ok(directory)
}
fn check_overlay(path: &Path) -> Result<()> {
    ensure!(overlay_paths()?.iter().all(|p| !overlaps(path, p)), "system destination became covered by an overlay"); Ok(())
}
pub fn apply(state: &mut State, root: &File) -> Result<()> {
    let Some(transaction) = state.journal.as_ref().and_then(|j| j.system.as_ref()) else { return Ok(()); };
    let count = transaction.directories.len();
    for index in 0..count {
        let change = state.journal.as_ref().unwrap().system.as_ref().unwrap().directories[index].clone();
        check_overlay(&change.path)?;
        let parent = pinned_parent(&change.path, change.parent.as_ref())?;
        ensure!(existing_parent(&change.path)?.is_none(), "system parent appeared after planning");
        {
            let dir = &mut state.journal.as_mut().unwrap().system.as_mut().unwrap().directories[index];
            dir.parent = Some(identity(&parent)?); dir.started = true;
        }
        state.intent(format!("system-directory-stage:{}", change.path.display()))?;
        fs::mkdir_at(parent.as_fd(), &change.temporary, change.mode)?;
        let dir = fs::open_beneath(parent.as_fd(), &change.temporary, libc::O_RDONLY | libc::O_DIRECTORY)?;
        state.journal.as_mut().unwrap().system.as_mut().unwrap().directories[index].identity = Some(identity(&dir)?);
        state.intent(format!("system-directory-publish:{}", change.path.display()))?;
        fs::rename_at(parent.as_fd(), &change.temporary, parent.as_fd(), Path::new(change.path.file_name().unwrap()))?;
    }
    let count = state.journal.as_ref().unwrap().system.as_ref().unwrap().files.len();
    for index in 0..count {
        let change = state.journal.as_ref().unwrap().system.as_ref().unwrap().files[index].clone();
        check_overlay(&change.entry.path)?;
        let parent = pinned_parent(&change.entry.path, change.parent.as_ref())?;
        let basename = Path::new(change.entry.path.file_name().unwrap());
        ensure!(inspect_at(&parent, basename)? == change.before, "system destination changed after planning");
        let backups = if change.before.is_some() {
            Some(backup_parent(state.journal.as_ref().unwrap().system.as_ref().unwrap(), None)?)
        } else { None };
        {
            let file = &mut state.journal.as_mut().unwrap().system.as_mut().unwrap().files[index];
            file.parent = Some(identity(&parent)?); file.started = true;
            file.backup_parent = backups.as_ref().map(identity).transpose()?;
        }
        state.intent(format!("system-file-backup:{}", change.entry.path.display()))?;
        if let Some(before) = &change.before {
            let mut original = fs::open_beneath(parent.as_fd(), basename, libc::O_RDONLY | libc::O_NOATIME)?;
            check_file(&original)?;
            let backups = backups.as_ref().context("missing system backup descriptor")?;
            stage_copy(backups, &change.backup, &mut original, 0o600)?;
            ensure!(inspect_at(backups, &change.backup)? == Some(backup_fingerprint(before)), "system backup differs from preflight");
        }
        let mut source = fs::open_beneath(root.as_fd(), &change.entry.source, libc::O_RDONLY | libc::O_NOATIME)?;
        check_file(&source)?; verify_digest(&mut source, &change.entry.sha256)?;
        stage_copy(&parent, &change.temporary, &mut source, change.entry.mode)?;
        ensure!(inspect_at(&parent, &change.temporary)?.as_ref() == Some(&change.after), "staged system payload differs");
        state.intent(format!("system-file-replace:{}", change.entry.path.display()))?;
        ensure!(inspect_at(&parent, basename)? == change.before, "system destination interfered with before replacement");
        if change.before.is_none() { fs::rename_at(parent.as_fd(), &change.temporary, parent.as_fd(), basename)?; }
        else { replace_at(&parent, &change.temporary, basename)?; }
        state.intent(format!("system-file-applied:{}", change.entry.path.display()))?;
    }
    Ok(())
}
fn remove_matching(parent: &File, path: &Path, expected: &Fingerprint) -> Result<()> {
    if let Some(actual) = inspect_at(parent, path)? {
        ensure!(&actual == expected, "transaction evidence changed: {}", path.display());
        fs::remove_at(parent.as_fd(), path, false)?;
    }
    Ok(())
}
fn restore_file(parent: &File, backups: Option<&File>, change: &FileChange, basename: &Path) -> Result<()> {
    let actual = inspect_at(parent, basename)?;
    ensure!(actual == change.before || actual.as_ref() == Some(&change.after), "system recovery refuses unexpected destination bytes or metadata");
    // A recovery-stage copy can contain the original; a forward stage contains
    // the new payload. Neither may be removed without verifying its contents.
    if let Some(stage) = inspect_at(parent, &change.temporary)? {
        ensure!(Some(&stage) == change.before.as_ref() || stage == change.after, "system temporary evidence changed");
        fs::remove_at(parent.as_fd(), &change.temporary, false)?;
    }
    if actual != change.before {
        if let Some(before) = &change.before {
            let backups = backups.context("system recovery backup directory is missing")?;
            ensure!(inspect_at(backups, &change.backup)? == Some(backup_fingerprint(before)), "system recovery backup missing or modified");
            let mut backup = fs::open_beneath(backups.as_fd(), &change.backup, libc::O_RDONLY | libc::O_NOATIME)?;
            stage_copy(parent, &change.temporary, &mut backup, before.mode)?;
            ensure!(inspect_at(parent, &change.temporary)?.as_ref() == Some(before), "restoration stage differs");
            ensure!(inspect_at(parent, basename)? == actual, "system destination changed during recovery");
            replace_at(parent, &change.temporary, basename)?;
        } else {
            fs::remove_at(parent.as_fd(), basename, false)?;
        }
    }
    if let Some(before) = &change.before {
        remove_matching(backups.context("system recovery backup directory is missing")?, &change.backup, &backup_fingerprint(before))?;
    }
    Ok(())
}
pub fn revert(state: &mut State) -> Result<()> {
    let Some(transaction) = state.journal.as_ref().and_then(|j| j.system.as_ref()) else { return Ok(()); };
    let count = transaction.files.len();
    for index in (0..count).rev() {
        let change = state.journal.as_ref().unwrap().system.as_ref().unwrap().files[index].clone();
        if !change.started || change.reverted { continue; }
        check_overlay(&change.entry.path)?;
        let parent = pinned_parent(&change.entry.path, change.parent.as_ref())?;
        let backups = if change.before.is_some() {
            Some(backup_parent(state.journal.as_ref().unwrap().system.as_ref().unwrap(),
                Some(change.backup_parent.as_ref().context("system backup identity is missing")?))?)
        } else { None };
        state.intent(format!("system-file-restore:{}", change.entry.path.display()))?;
        restore_file(&parent, backups.as_ref(), &change, Path::new(change.entry.path.file_name().unwrap()))?;
        state.journal.as_mut().unwrap().system.as_mut().unwrap().files[index].reverted = true;
        state.save()?;
    }
    Ok(())
}
pub fn cleanup_directories(state: &mut State) -> Result<()> {
    let Some(transaction) = state.journal.as_ref().and_then(|j| j.system.as_ref()) else { return Ok(()); };
    let count = transaction.directories.len();
    for index in (0..count).rev() {
        let change = state.journal.as_ref().unwrap().system.as_ref().unwrap().directories[index].clone();
        if !change.started || change.removed { continue; }
        let parent = pinned_parent(&change.path, change.parent.as_ref())?;
        state.intent(format!("system-directory-remove:{}", change.path.display()))?;
        for candidate in [Path::new(change.path.file_name().unwrap()), change.temporary.as_path()] {
            match fs::open_beneath(parent.as_fd(), candidate, libc::O_RDONLY | libc::O_DIRECTORY) {
                Ok(dir) => {
                    ensure!(change.identity.as_ref() == Some(&identity(&dir)?),
                        "system directory ownership is uncertain; retain journal and directory for operator reconciliation");
                    fs::remove_at(parent.as_fd(), candidate, true)?;
                },
                Err(e) if missing(&e) => {}, Err(e) => return Err(e),
            }
        }
        state.journal.as_mut().unwrap().system.as_mut().unwrap().directories[index].removed = true; state.save()?;
    }
    Ok(())
}
pub fn verify_committed(transaction: &Transaction) -> Result<()> {
    for change in &transaction.files {
        check_overlay(&change.entry.path)?;
        let parent = pinned_parent(&change.entry.path, change.parent.as_ref())?;
        ensure!(inspect_at(&parent, Path::new(change.entry.path.file_name().unwrap()))?.as_ref() == Some(&change.after),
            "committed system file differs: {}", change.entry.path.display());
        if let Some(before) = &change.before {
            let backups = backup_parent(transaction, Some(change.backup_parent.as_ref().context("system backup identity is missing")?))?;
            ensure!(inspect_at(&backups, &change.backup)? == Some(backup_fingerprint(before)), "committed system backup differs");
        }
    }
    Ok(())
}
pub fn reboot(state: &mut State) -> Result<()> {
    ensure!(state.journal.is_none(), "refusing reboot with an incomplete transaction");
    let release = state.release(state.active.context("no active release for reboot")?)?;
    let receipt = state.system_receipts.last().context("no durable system receipt")?;
    ensure!(receipt.release == release.name, "system receipt does not match committed release");
    verify_committed(&receipt.transaction)?;
    let ready = std::fs::read_to_string(READY)?;
    ensure!(ready.starts_with(&format!("complete {} {} ", state.active.unwrap().name(), release.name)), "refusing reboot without complete release readiness");
    ensure!(std::fs::read_to_string("/proc/1/comm")?.trim() == "runit", "graceful system reboot requires runit PID 1");
    ensure!(std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?.trim() == receipt.previous_boot_id,
        "system receipt belongs to another boot; refusing an automatic second reboot");
    // The pinned Artix runit shutdown implementation performs service shutdown
    // and filesystem sync. Never use a forced reboot or the kernel reboot syscall.
    fs::secure_absolute(Path::new("/usr/lib/runit/shutdown"), libc::O_RDONLY)?;
    state.system_receipts.last_mut().unwrap().reboot_requested = true;
    state.save()?;
    services::run(Command::new("/usr/lib/runit/shutdown").args(["-r", "now"]), 30)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn entry(path: &str) -> SystemFile {
        SystemFile { source: "system/payload".into(), path: path.into(), sha256: "0".repeat(64), mode: 0o755, install_if_absent: false }
    }
    #[test]
    fn rejects_protected_overlay_and_ancestor_destinations() {
        for path in ["/etc/shadow", "/var/lib/opdbus/grants", "/root/key", "/proc/test", "/etc/op-dbus/identity.json", "/usr/local/libexec/opdbus-release/policy.json"] {
            assert!(validate_destination(&entry(path), &[], &[]).is_err(), "{path}");
        }
        let path = "/usr/local/libexec/opdbus-release/op-release";
        for overlay in [path, "/usr/local/libexec", "/usr/local/libexec/opdbus-release/op-release/child"] {
            assert!(validate_destination(&entry(path), &[PathBuf::from(overlay)], &[]).is_err());
            assert!(validate_destination(&entry(path), &[], &[PathBuf::from(overlay)]).is_err());
        }
    }
    #[test]
    fn rejects_duplicate_sources_and_noncanonical_payloads() {
        let a = entry("/etc/sv/service-a/run");
        let mut b = entry("/etc/sv/service-b/run");
        assert!(validate_manifest(&Manifest { version: 1, files: vec![a.clone(), b.clone()] }, &[], &[]).is_err());
        b.source = "system/second".into();
        assert!(validate_manifest(&Manifest { version: 1, files: vec![a.clone(), b.clone()] }, &[], &[]).is_ok());
        b.path = a.path.clone();
        assert!(validate_manifest(&Manifest { version: 1, files: vec![a, b.clone()] }, &[], &[]).is_err());
        b.path = "/etc/sv/service-b/run".into();
        for source in ["system/../payload", "other/payload", "/system/payload", "system//payload"] {
            b.source = source.into();
            assert!(validate_manifest(&Manifest { version: 1, files: vec![b.clone()] }, &[], &[]).is_err());
        }
        b.source = "system/second".into(); b.sha256 = "A".repeat(64);
        assert!(validate_manifest(&Manifest { version: 1, files: vec![b] }, &[], &[]).is_err());
    }
    #[test]
    fn descriptor_reads_reject_symlinks_and_wrong_digest() -> Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::write(dir.path().join("payload"), b"real bytes")?;
        symlink("payload", dir.path().join("link"))?;
        let root = File::open(dir.path())?;
        assert!(fs::open_beneath(root.as_fd(), Path::new("link"), libc::O_RDONLY).is_err());
        let mut payload = fs::open_beneath(root.as_fd(), Path::new("payload"), libc::O_RDONLY | libc::O_NOATIME)?;
        assert!(verify_digest(&mut payload, &"0".repeat(64)).is_err());
        Ok(())
    }
    #[test]
    fn interrupted_replacement_restores_original_mode_idempotently() -> Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
        let parent = File::open(dir.path())?;
        let backup_dir = tempfile::tempdir()?;
        let backups = File::open(backup_dir.path())?;
        std::fs::write(dir.path().join("run"), b"old")?;
        std::fs::set_permissions(dir.path().join("run"), std::fs::Permissions::from_mode(0o700))?;
        let before = inspect_at(&parent, Path::new("run"))?.unwrap();
        let mut original = fs::open_beneath(parent.as_fd(), Path::new("run"), libc::O_RDONLY | libc::O_NOATIME)?;
        let times = [libc::timespec { tv_sec: 1_600_000_000, tv_nsec: 42 }; 2];
        // SAFETY: isolated fixture file descriptor and a two-element times array.
        syscall(unsafe { libc::futimens(original.as_raw_fd(), times.as_ptr()) })?;
        stage_copy(&backups, Path::new("backup"), &mut original, 0o600)?;
        std::fs::write(backup_dir.path().join("payload"), b"new")?;
        let mut payload = File::open(backup_dir.path().join("payload"))?;
        stage_copy(&parent, Path::new("stage"), &mut payload, 0o755)?;
        let after = inspect_at(&parent, Path::new("stage"))?.unwrap();
        replace_at(&parent, Path::new("stage"), Path::new("run"))?;
        let change = FileChange { entry: entry("/unused/run"), before: Some(before.clone()), after,
            backup: "backup".into(), temporary: "stage".into(), parent: Some(identity(&parent)?),
            backup_parent: Some(identity(&backups)?), started: true, reverted: false };
        assert!(!dir.path().join("backup").exists());
        assert_eq!(inspect_at(&backups, Path::new("backup"))?, Some(backup_fingerprint(&before)));
        let entries: Vec<_> = std::fs::read_dir(dir.path())?.map(|entry| entry.map(|entry| entry.file_name())).collect::<std::io::Result<_>>()?;
        assert_eq!(entries, vec![std::ffi::OsString::from("run")]);
        restore_file(&parent, Some(&backups), &change, Path::new("run"))?;
        restore_file(&parent, Some(&backups), &change, Path::new("run"))?;
        assert_eq!(inspect_at(&parent, Path::new("run"))?, Some(before));
        let restored = std::fs::metadata(dir.path().join("run"))?;
        assert_eq!((restored.atime(), restored.atime_nsec(), restored.mtime(), restored.mtime_nsec()),
            (1_600_000_000, 42, 1_600_000_000, 42));
        assert_eq!(std::fs::read(dir.path().join("run"))?, b"old");
        std::fs::write(dir.path().join("run"), b"unrelated")?;
        assert!(restore_file(&parent, Some(&backups), &change, Path::new("run")).is_err());
        Ok(())
    }
    #[test]
    fn normal_activation_does_not_apply_rootfs_payload() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let payload = preserve::stub("op-release-test-disabled", "run");
        std::fs::create_dir(dir.path().join("system"))?;
        std::fs::write(dir.path().join("system/payload"), payload.as_bytes())?;
        let file = SystemFile {
            source: "system/payload".into(), path: "/etc/sv/op-release-test-disabled/run".into(),
            sha256: format!("{:x}", Sha256::digest(payload.as_bytes())), mode: 0o755, install_if_absent: true,
        };
        std::fs::write(dir.path().join("system-files.json"),
            serde_json::to_vec(&serde_json::json!({"version": 1, "files": [&file]}))?)?;
        validate_manifest(&Manifest { version: 1, files: vec![file] }, &[], &[])?;
        let root = File::open(dir.path())?;
        let mut state = State::default();
        state.journal = Some(Journal::new("activate", None, Some(Slot::A)));
        apply(&mut state, &root)?;
        assert_eq!(state.journal.as_ref().unwrap().step, "planned");
        assert!(state.journal.as_ref().unwrap().system.is_none());
        assert!(state.system_receipts.is_empty());
        Ok(())
    }
    #[test]
    fn failed_or_uncommitted_activation_cannot_request_reboot() {
        let mut state = State::default();
        assert!(reboot(&mut state).is_err());
        state.journal = Some(Journal::new("activate", None, Some(Slot::A)));
        assert!(reboot(&mut state).is_err());
        assert!(state.system_receipts.is_empty());
    }
    #[test]
    fn inherited_default_acl_fails_without_leaving_a_named_stage() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let parent = File::open(dir.path())?;
        let mut acl = 2u32.to_le_bytes().to_vec();
        // Linux POSIX ACL xattr: version, then tag/permissions/id entries.
        for (tag, permissions, id) in [
            (0x01u16, 7u16, u32::MAX),
            (0x02, 4, unsafe { libc::geteuid() }.saturating_add(1)),
            (0x04, 0, u32::MAX),
            (0x10, 4, u32::MAX),
            (0x20, 0, u32::MAX),
        ] {
            acl.extend_from_slice(&tag.to_le_bytes());
            acl.extend_from_slice(&permissions.to_le_bytes());
            acl.extend_from_slice(&id.to_le_bytes());
        }
        // SAFETY: only this disposable owned directory and the initialized ACL bytes.
        syscall(unsafe { libc::fsetxattr(parent.as_raw_fd(), c"system.posix_acl_default".as_ptr(),
            acl.as_ptr().cast(), acl.len(), 0) })?;
        assert!(reject_xattrs(&parent).is_err());
        let mut source = tempfile::tempfile()?;
        std::io::Write::write_all(&mut source, b"payload")?;
        assert!(stage_copy(&parent, Path::new("stage"), &mut source, 0o644).is_err());
        assert!(std::fs::read_dir(dir.path())?.next().is_none());
        Ok(())
    }
}
