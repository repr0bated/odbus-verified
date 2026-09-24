//! Read-only proof of an Incus Btrfs container root, distinct from its API
//! volume record. Incus can retain volume metadata and `backup.yaml` after the
//! actual root subvolume has disappeared.

use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

const BTRFS_STATFS_MAGIC: libc::c_long = 0x9123_683e;
const BTRFS_IOC_FS_INFO: libc::c_ulong = 0x8400_941f;
const BTRFS_IOC_GET_SUBVOL_INFO: libc::c_ulong = 0x81f8_943c;
const MAX_BACKUP_YAML: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootSubvolumeState {
    /// No root entry, or only Incus's narrowly recognized backup.yaml stub.
    Missing,
    /// A true direct-child Btrfs subvolume root is present.
    Present,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MissingRootPreparation {
    AlreadyAbsent,
    Quarantined(QuarantinedRootStub),
}

#[derive(Debug, Clone, Serialize)]
pub struct QuarantinedRootStub {
    pub path: PathBuf,
    pub original_stub_inode: u64,
    pub backup_inode: u64,
    pub backup_sha256: String,
    pub original_stub_mode: u32,
}

/// `incus_dir` is the trusted daemon state directory, e.g. `/var/lib/incus`;
/// `source` must equal the native pool API's pinned `config.source` directory.
/// This query does not create, mount, delete, repair, or select storage.
pub fn inspect_root_subvolume(
    incus_dir: &Path,
    pool_name: &str,
    source: &Path,
    instance_name: &str,
) -> Result<RootSubvolumeState> {
    validate_segment(pool_name, "pool")?;
    validate_segment(instance_name, "instance")?;
    let source_fd = open_absolute_dir(source).context("open pinned Incus pool source")?;
    let state_fd = open_absolute_dir(incus_dir).context("open trusted Incus state")?;
    let pools_fd = open_child_dir(&state_fd, "storage-pools")?
        .context("Incus storage-pools directory is absent")?;
    let pool_fd = open_child_dir(&pools_fd, pool_name)?.context("Incus pool mount is absent")?;

    ensure!(
        mount_id(&pool_fd)? != mount_id(&pools_fd)?,
        "Incus pool has no distinct mounted source"
    );
    let source_info = subvolume_info(&source_fd)?;
    let pool_info = subvolume_info(&pool_fd)?;
    ensure!(
        source_fd.metadata()?.ino() == 256 && pool_fd.metadata()?.ino() == 256,
        "Incus source or pool mount is not a subvolume root"
    );
    ensure!(
        source_info.treeid == pool_info.treeid
            && source_info.uuid == pool_info.uuid
            && fsid(&source_fd)? == fsid(&pool_fd)?,
        "Incus pool mount does not match pinned source"
    );
    let containers_fd =
        open_child_dir(&pool_fd, "containers")?.context("Incus containers directory is absent")?;
    ensure!(
        mount_id(&containers_fd)? == mount_id(&pool_fd)?,
        "Incus containers directory is covered by another mount"
    );
    let Some(root_fd) = open_child_dir_os(&containers_fd, instance_name.as_bytes(), false)? else {
        return Ok(RootSubvolumeState::Missing);
    };
    ensure!(
        mount_id(&root_fd)? == mount_id(&pool_fd)?,
        "container root is covered by another mount"
    );
    let root_meta = root_fd.metadata()?;
    if root_meta.ino() == 256 {
        let root_info = subvolume_info(&root_fd)?;
        ensure!(
            root_info.treeid > 5
                && root_info.parent_id == pool_info.treeid
                && root_info.name == instance_name.as_bytes(),
            "container root subvolume ancestry differs from pinned pool"
        );
        return Ok(RootSubvolumeState::Present);
    }
    ensure!(
        root_meta.uid() == 0
            && root_meta.mode() & 0o7777 == 0o0100
            && root_meta.dev() == containers_fd.metadata()?.dev(),
        "container stub has unexpected ownership, mode, or filesystem"
    );
    let proc_path = format!("/proc/self/fd/{}", root_fd.as_raw_fd());
    let mut entries = fs::read_dir(&proc_path).context("enumerate missing-root stub")?;
    let backup = entries
        .next()
        .transpose()?
        .context("unknown empty Incus root stub")?;
    ensure!(
        backup.file_name().as_bytes() == b"backup.yaml" && entries.next().transpose()?.is_none(),
        "missing-root stub contains unexpected entries"
    );
    let backup_fd = open_child_file(&root_fd, "backup.yaml")?;
    let backup_meta = backup_fd.metadata()?;
    ensure!(
        backup_meta.is_file()
            && backup_meta.uid() == 0
            && backup_meta.nlink() == 1
            && backup_meta.mode() & 0o7777 == 0o0400
            && backup_meta.len() <= MAX_BACKUP_YAML
            && backup_meta.dev() == root_meta.dev(),
        "Incus backup.yaml stub has unsafe metadata"
    );
    Ok(RootSubvolumeState::Missing)
}

/// Move only the exact verified backup.yaml-only Incus stub out of
/// `containers/` before Incus rebuilds its missing root. The owner must have
/// journaled `operation_id` and the resulting deterministic quarantine name
/// before this call. An uncertain result is reconciled, never retried.
pub fn prepare_missing_root_for_rebuild(
    incus_dir: &Path,
    pool_name: &str,
    source: &Path,
    instance_name: &str,
    operation_id: &str,
) -> Result<MissingRootPreparation> {
    validate_segment(pool_name, "pool")?;
    validate_segment(instance_name, "instance")?;
    validate_segment(operation_id, "operation")?;
    let quarantine_name = format!(".opdbus-root-{instance_name}-{operation_id}");
    ensure!(
        quarantine_name.len() <= 255,
        "quarantine name exceeds filesystem limit"
    );
    let source_fd = open_absolute_dir(source).context("open pinned Incus pool source")?;
    let state_fd = open_absolute_dir(incus_dir).context("open trusted Incus state")?;
    let pools_fd = open_child_dir(&state_fd, "storage-pools")?
        .context("Incus storage-pools directory is absent")?;
    let pool_fd = open_child_dir(&pools_fd, pool_name)?.context("Incus pool mount is absent")?;
    ensure!(
        mount_id(&pool_fd)? != mount_id(&pools_fd)?,
        "Incus pool has no distinct mounted source"
    );
    let source_info = subvolume_info(&source_fd)?;
    let pool_info = subvolume_info(&pool_fd)?;
    ensure!(
        source_fd.metadata()?.ino() == 256
            && pool_fd.metadata()?.ino() == 256
            && source_info.treeid == pool_info.treeid
            && source_info.uuid == pool_info.uuid
            && fsid(&source_fd)? == fsid(&pool_fd)?,
        "Incus pool mount does not match pinned source"
    );
    let containers_fd =
        open_child_dir(&pool_fd, "containers")?.context("Incus containers directory is absent")?;
    let pool_mount_id = mount_id(&pool_fd)?;
    ensure!(
        mount_id(&containers_fd)? == pool_mount_id,
        "Incus containers directory is covered by another mount"
    );
    ensure!(
        !entry_exists(&pool_fd, quarantine_name.as_bytes())?,
        "quarantine already exists; reconcile the prior operation"
    );
    let Some(root_fd) = open_child_dir_os(&containers_fd, instance_name.as_bytes(), false)? else {
        return Ok(MissingRootPreparation::AlreadyAbsent);
    };
    let parent_dev = containers_fd.metadata()?.dev();
    let (root_meta, backup_meta, backup_fd) =
        verify_exact_stub(&root_fd, parent_dev, pool_mount_id, 0o100)?;
    let backup_sha256 = hash_pinned_backup(&root_fd, &backup_meta)?;
    let current_root = open_child_dir_os(&containers_fd, instance_name.as_bytes(), false)?
        .context("container stub disappeared before quarantine")?;
    let (current_root_meta, current_backup_meta, current_backup) =
        verify_exact_stub(&current_root, parent_dev, pool_mount_id, 0o100)?;
    ensure!(
        same_file(&root_fd, &current_root)?
            && current_root_meta.ino() == root_meta.ino()
            && current_backup_meta.ino() == backup_meta.ino()
            && same_file(&backup_fd, &current_backup)?
            && hash_pinned_backup(&current_root, &backup_meta)? == backup_sha256,
        "container stub changed before quarantine"
    );
    let old_name = CString::new(instance_name)?;
    let new_name = CString::new(quarantine_name.clone())?;
    // SAFETY: both parents are held, verified directories on one exact Btrfs
    // pool; RENAME_NOREPLACE cannot replace an earlier recovery artifact.
    if unsafe {
        libc::renameat2(
            containers_fd.as_raw_fd(),
            old_name.as_ptr(),
            pool_fd.as_raw_fd(),
            new_name.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(io::Error::last_os_error()).context("quarantine exact missing-root stub");
    }
    ensure!(
        open_child_dir_os(&containers_fd, instance_name.as_bytes(), false)?.is_none(),
        "old container root reappeared after quarantine"
    );
    let quarantined = open_child_dir_os(&pool_fd, quarantine_name.as_bytes(), false)?
        .context("quarantined stub disappeared")?;
    let (quarantined_meta, quarantined_backup_meta, quarantined_backup) =
        verify_exact_stub(&quarantined, parent_dev, pool_mount_id, 0o100)?;
    ensure!(
        same_file(&root_fd, &quarantined)?
            && quarantined_meta.ino() == root_meta.ino()
            && quarantined_backup_meta.ino() == backup_meta.ino()
            && same_file(&backup_fd, &quarantined_backup)?
            && hash_pinned_backup(&quarantined, &backup_meta)? == backup_sha256,
        "quarantined stub or backup changed"
    );
    // The original stub is mode 0100. In its root-only recovery location,
    // permit owner inspection without changing its inode or backup content.
    if unsafe { libc::fchmod(quarantined.as_raw_fd(), 0o700) } != 0 {
        return Err(io::Error::last_os_error()).context("secure quarantined stub directory");
    }
    let (secured_meta, secured_backup_meta, secured_backup) =
        verify_exact_stub(&quarantined, parent_dev, pool_mount_id, 0o700)?;
    ensure!(
        secured_meta.ino() == root_meta.ino()
            && secured_backup_meta.ino() == backup_meta.ino()
            && same_file(&backup_fd, &secured_backup)?
            && hash_pinned_backup(&quarantined, &backup_meta)? == backup_sha256,
        "secured quarantine changed"
    );
    quarantined.sync_all()?;
    containers_fd.sync_all()?;
    pool_fd.sync_all()?;
    Ok(MissingRootPreparation::Quarantined(QuarantinedRootStub {
        path: source.join(quarantine_name),
        original_stub_inode: root_meta.ino(),
        backup_inode: backup_meta.ino(),
        backup_sha256,
        original_stub_mode: root_meta.mode() & 0o7777,
    }))
}

fn entry_exists(parent: &File, name: &[u8]) -> Result<bool> {
    let name = CString::new(name)?;
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: dirfd is pinned; AT_SYMLINK_NOFOLLOW detects any colliding type.
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == 0
    {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::NotFound {
        Ok(false)
    } else {
        Err(error).context("inspect deterministic quarantine target")
    }
}

fn verify_exact_stub(
    root: &File,
    parent_dev: u64,
    expected_mount_id: u64,
    expected_mode: u32,
) -> Result<(fs::Metadata, fs::Metadata, File)> {
    ensure!(
        mount_id(root)? == expected_mount_id,
        "container stub is covered by another mount"
    );
    let root_meta = root.metadata()?;
    ensure!(
        root_meta.ino() != 256
            && root_meta.uid() == 0
            && root_meta.mode() & 0o7777 == expected_mode
            && root_meta.dev() == parent_dev,
        "root is not the exact missing-root stub"
    );
    let proc_path = format!("/proc/self/fd/{}", root.as_raw_fd());
    let mut entries = fs::read_dir(&proc_path).context("enumerate missing-root stub")?;
    ensure!(
        entries
            .next()
            .transpose()?
            .is_some_and(|entry| entry.file_name().as_bytes() == b"backup.yaml")
            && entries.next().transpose()?.is_none(),
        "missing-root stub contains unexpected entries"
    );
    let backup_fd = open_child_file(root, "backup.yaml")?;
    let backup_meta = backup_fd.metadata()?;
    ensure!(
        mount_id(&backup_fd)? == expected_mount_id
            && backup_meta.is_file()
            && backup_meta.uid() == 0
            && backup_meta.nlink() == 1
            && backup_meta.mode() & 0o7777 == 0o0400
            && backup_meta.len() <= MAX_BACKUP_YAML
            && backup_meta.dev() == root_meta.dev(),
        "Incus backup.yaml stub has unsafe metadata"
    );
    Ok((root_meta, backup_meta, backup_fd))
}

fn same_file(left: &File, right: &File) -> Result<bool> {
    let left = left.metadata()?;
    let right = right.metadata()?;
    Ok(left.dev() == right.dev() && left.ino() == right.ino())
}

fn hash_pinned_backup(root: &File, expected: &fs::Metadata) -> Result<String> {
    let name = CString::new("backup.yaml")?;
    // SAFETY: O_NONBLOCK prevents a substituted FIFO from blocking. The held
    // parent FD and O_NOFOLLOW refuse a symlink or path escape.
    let fd = unsafe {
        libc::openat(
            root.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error()).context("read pinned Incus backup.yaml");
    }
    let backup = unsafe { File::from_raw_fd(fd) };
    let actual = backup.metadata()?;
    ensure!(
        actual.is_file()
            && actual.dev() == expected.dev()
            && actual.ino() == expected.ino()
            && actual.len() == expected.len()
            && actual.len() <= MAX_BACKUP_YAML,
        "backup.yaml identity changed"
    );
    let mut bytes = Vec::with_capacity(actual.len() as usize);
    backup.take(MAX_BACKUP_YAML + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 == actual.len(),
        "backup.yaml length changed while hashing"
    );
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn validate_segment(value: &str, label: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 128
            && value != "."
            && value != ".."
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.')),
        "invalid {label} segment"
    );
    Ok(())
}

fn open_absolute_dir(path: &Path) -> Result<File> {
    ensure!(path.is_absolute(), "expected absolute directory path");
    let root = CString::new("/")?;
    // SAFETY: constant NUL-terminated path and checked return value.
    let fd = unsafe {
        libc::open(
            root.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error()).context("open filesystem root");
    }
    // SAFETY: successful open returns an owned descriptor.
    let mut current = unsafe { File::from_raw_fd(fd) };
    for component in path.components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(name) => {
                current = open_child_dir_os(&current, name.as_bytes(), true)?
                    .context("trusted absolute path component is absent")?;
            }
            _ => bail!("trusted absolute path has traversal or prefix"),
        }
    }
    Ok(current)
}

fn open_child_dir(parent: &File, name: &str) -> Result<Option<File>> {
    open_child_dir_os(parent, name.as_bytes(), true)
}

fn open_child_dir_os(parent: &File, name: &[u8], require_root_owner: bool) -> Result<Option<File>> {
    let name = CString::new(name)?;
    // SAFETY: parent is an open directory and name is NUL-terminated.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(error).context("open Incus directory without following symlinks");
    }
    // SAFETY: successful open returns an owned descriptor.
    let file = unsafe { File::from_raw_fd(fd) };
    let meta = file.metadata()?;
    ensure!(
        (!require_root_owner || meta.uid() == 0)
            && (meta.mode() & 0o022 == 0
                || (require_root_owner && meta.uid() == 0 && meta.mode() & 0o1000 != 0)),
        "Incus directory has unexpected ownership or write mode"
    );
    Ok(Some(file))
}

fn open_child_file(parent: &File, name: &str) -> Result<File> {
    let name = CString::new(name)?;
    // SAFETY: parent is an open directory and name is NUL-terminated.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            // Only metadata is needed. O_PATH never opens a malformed FIFO or
            // device for I/O, so this read-only proof cannot block on it.
            libc::O_PATH | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error()).context("open Incus backup.yaml stub");
    }
    // SAFETY: successful open returns an owned descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn mount_id(file: &File) -> Result<u64> {
    let mut info: libc::statx = unsafe { std::mem::zeroed() };
    let empty = CString::new("")?;
    // SAFETY: statx receives an initialized output struct and pinned fd.
    if unsafe {
        libc::statx(
            file.as_raw_fd(),
            empty.as_ptr(),
            libc::AT_EMPTY_PATH,
            libc::STATX_MNT_ID,
            &mut info,
        )
    } != 0
    {
        return Err(io::Error::last_os_error()).context("read Incus mount identity");
    }
    ensure!(
        info.stx_mask & libc::STATX_MNT_ID != 0,
        "kernel lacks mount identity"
    );
    Ok(info.stx_mnt_id)
}

fn fsid(file: &File) -> Result<[u8; 16]> {
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: fstatfs receives an initialized output struct and pinned fd.
    if unsafe { libc::fstatfs(file.as_raw_fd(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error()).context("read Incus filesystem type");
    }
    ensure!(stat.f_type == BTRFS_STATFS_MAGIC, "Incus pool is not Btrfs");
    let mut raw = [0u8; 1024];
    // SAFETY: output buffer matches btrfs_ioctl_fs_info_args size.
    if unsafe { libc::ioctl(file.as_raw_fd(), BTRFS_IOC_FS_INFO, raw.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error()).context("read Incus Btrfs FSID");
    }
    Ok(raw[16..32].try_into()?)
}

struct SubvolumeInfo {
    treeid: u64,
    parent_id: u64,
    name: Vec<u8>,
    uuid: [u8; 16],
}

fn subvolume_info(file: &File) -> Result<SubvolumeInfo> {
    let mut raw = [0u8; 504];
    // SAFETY: output buffer matches btrfs_ioctl_get_subvol_info_args size.
    if unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            BTRFS_IOC_GET_SUBVOL_INFO,
            raw.as_mut_ptr(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error()).context("read Incus Btrfs subvolume identity");
    }
    let end = raw[8..264]
        .iter()
        .position(|byte| *byte == 0)
        .context("unterminated Btrfs subvolume name")?;
    Ok(SubvolumeInfo {
        treeid: u64::from_ne_bytes(raw[0..8].try_into()?),
        parent_id: u64::from_ne_bytes(raw[264..272].try_into()?),
        name: raw[8..8 + end].to_vec(),
        uuid: raw[296..312].try_into()?,
    })
}
