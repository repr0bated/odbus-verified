//! Create a container volume as a writable snapshot of the shared seed.
//!
//! Incus first creates the (stopped, empty) instance, so it owns the volume
//! record and writes `containers/<name>/backup.yaml`. [`create_from_seed`]
//! then snapshots the seed beside that placeholder, carries `backup.yaml`
//! over, atomically swaps the snapshot into place and destroys the empty
//! placeholder. An interrupted run leaves `containers/.<name>.seed`, which a
//! later call refuses until it is reconciled.

use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::btrfs::{self, SUBVOL_RDONLY, SUBVOL_ROOT_INO};

const MAX_BACKUP_YAML: u64 = 1024 * 1024;

/// Where the pool and its shared seed live.
#[derive(Debug, Clone)]
pub struct SeedLocation {
    /// Incus state directory, normally `/var/lib/incus`.
    pub incus_dir: PathBuf,
    /// Incus storage pool, e.g. `3tched-pool`.
    pub pool: String,
    /// Seed subvolume path inside the pool, e.g. `images/@seed`.
    pub seed_dir: String,
    pub seed_name: String,
    /// Exact authority pins for identity-bound provisioning. Legacy callers
    /// may omit them; the bridge always supplies both.
    pub expected_seed_uuid: Option<uuid::Uuid>,
    pub expected_pool_fsid: Option<uuid::Uuid>,
    pub expected_container_uid: Option<u32>,
    pub expected_seed_treeid: Option<u64>,
    pub expected_metadata_sha256: Option<[u8; 32]>,
}

impl Default for SeedLocation {
    fn default() -> Self {
        Self {
            incus_dir: PathBuf::from("/var/lib/incus"),
            pool: "3tched-pool".into(),
            seed_dir: "images".into(),
            seed_name: "@seed".into(),
            expected_seed_uuid: None,
            expected_pool_fsid: None,
            expected_container_uid: None,
            expected_seed_treeid: None,
            expected_metadata_sha256: None,
        }
    }
}

/// Proof that `containers/<name>` is a writable CoW snapshot of the seed.
#[derive(Debug, Clone, Serialize)]
pub struct SeedContainer {
    pub name: String,
    pub seed_uuid: String,
    pub volume_uuid: String,
    pub volume_treeid: u64,
}

pub fn create_from_seed(location: &SeedLocation, name: &str) -> Result<SeedContainer> {
    for (value, label) in [
        (location.pool.as_str(), "pool"),
        (location.seed_dir.as_str(), "seed directory"),
        (location.seed_name.as_str(), "seed"),
        (name, "container"),
    ] {
        ensure!(
            !value.is_empty()
                && value.len() <= 128
                && value != "."
                && value != ".."
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'@')),
            "invalid {label} name: {value}"
        );
    }
    ensure!(!name.starts_with('.'), "container name must not start with '.'");

    let state = btrfs::open_absolute_dir(&location.incus_dir)?;
    let pools = btrfs::open_dir(&state, b"storage-pools")?.context("Incus storage-pools is absent")?;
    let pool = btrfs::open_dir(&pools, location.pool.as_bytes())?
        .with_context(|| format!("pool {} is not mounted", location.pool))?;
    let pool_mount = btrfs::mount_id(&pool)?;
    ensure!(
        pool_mount != btrfs::mount_id(&pools)? && pool.metadata()?.ino() == SUBVOL_ROOT_INO,
        "pool {} is not a distinct mounted Btrfs filesystem",
        location.pool
    );
    let pool_fsid = btrfs::fsid(&pool)?;
    ensure!(location.expected_pool_fsid.is_none_or(|expected| *expected.as_bytes() == pool_fsid),
        "pool FSID differs from admitted creation intent");
    let pool_tree = btrfs::subvolume_info(&pool)?.treeid;

    // The shared seed: read-only subvolume on this pool, in Incus volume layout.
    let seed_parent = btrfs::open_dir(&pool, location.seed_dir.as_bytes())?
        .context("seed directory is absent")?;
    let seed = btrfs::open_dir(&seed_parent, location.seed_name.as_bytes())?
        .context("seed subvolume is absent")?;
    let seed_info = btrfs::subvolume_info(&seed)?;
    ensure!(
        seed.metadata()?.ino() == SUBVOL_ROOT_INO
            && btrfs::mount_id(&seed)? == pool_mount
            && btrfs::fsid(&seed)? == pool_fsid
            && seed_info.flags & SUBVOL_RDONLY != 0,
        "seed is not a read-only subvolume on pool {}",
        location.pool
    );
    ensure!(location.expected_seed_uuid.is_none_or(|expected| *expected.as_bytes() == seed_info.uuid),
        "seed UUID differs from admitted creation intent");
    verify_seed_metadata(&seed, location, &seed_info)?;
    let seed_entries = btrfs::list(&seed)?;
    ensure!(
        seed_entries.iter().any(|e| e == b"rootfs") && seed_entries.iter().any(|e| e == b"metadata.yaml"),
        "seed lacks the Incus volume layout (metadata.yaml + rootfs/)"
    );

    let rootfs = btrfs::open_dir(&seed, b"rootfs")?.context("seed rootfs absent")?;
    ensure!(rootfs.metadata()?.ino() != SUBVOL_ROOT_INO,
        "seed rootfs is a nested subvolume and would be omitted from the snapshot");
    // The placeholder Incus just created: a writable subvolume holding only
    // backup.yaml and empty layout directories.
    let containers = btrfs::open_dir(&pool, b"containers")?.context("containers directory is absent")?;
    ensure!(btrfs::mount_id(&containers)? == pool_mount, "containers directory is covered by a mount");
    let staging = format!(".{name}.seed");
    ensure!(
        btrfs::open_dir(&containers, staging.as_bytes())?.is_none(),
        "{staging} exists from an interrupted create; reconcile it first"
    );
    let placeholder = btrfs::open_dir(&containers, name.as_bytes())?
        .with_context(|| format!("container {name} has no volume; create the Incus instance first"))?;
    let placeholder_info = btrfs::subvolume_info(&placeholder)?;
    ensure!(
        placeholder.metadata()?.ino() == SUBVOL_ROOT_INO
            && btrfs::mount_id(&placeholder)? == pool_mount
            && placeholder_info.name == name.as_bytes()
            && placeholder_info.parent_id == pool_tree
            && placeholder_info.flags & SUBVOL_RDONLY == 0,
        "container {name} volume is not a writable subvolume on the pool"
    );
    ensure!(
        placeholder_info.parent_uuid != seed_info.uuid,
        "container {name} is already a snapshot of the seed"
    );
    for entry in btrfs::list(&placeholder)? {
        match entry.as_slice() {
            b"backup.yaml" | b"metadata.yaml" => {}
            b"rootfs" | b"templates" => {
                let dir = btrfs::open_dir(&placeholder, &entry)?.context("layout directory vanished")?;
                ensure!(
                    dir.metadata()?.ino() != SUBVOL_ROOT_INO && btrfs::list(&dir)?.is_empty(),
                    "container {name} is not empty; refusing to replace it"
                );
            }
            other => anyhow::bail!(
                "container {name} has unexpected entry {}; refusing to replace it",
                String::from_utf8_lossy(other)
            ),
        }
    }
    let (backup, backup_mode) = read_backup(&placeholder)?;

    // Snapshot the seed beside the placeholder and carry Incus's backup.yaml.
    btrfs::snapshot(&seed, &containers, staging.as_bytes())?;
    let fresh = btrfs::open_dir(&containers, staging.as_bytes())?.context("new snapshot is absent")?;
    if let Some(backup) = backup {
        write_new_file(&fresh, b"backup.yaml", &backup, backup_mode)?;
    }
    btrfs::syncfs(&fresh)?;

    // Swap atomically, then drop the empty placeholder (now at `staging`).
    btrfs::exchange(&containers, staging.as_bytes(), name.as_bytes())?;
    btrfs::destroy_subvolume(&containers, staging.as_bytes())?;
    btrfs::syncfs(&containers)?;

    let volume = btrfs::open_dir(&containers, name.as_bytes())?.context("container volume vanished")?;
    let info = btrfs::subvolume_info(&volume)?;
    ensure!(
        volume.metadata()?.ino() == SUBVOL_ROOT_INO
            && btrfs::mount_id(&volume)? == pool_mount
            && info.parent_id == pool_tree
            && info.parent_uuid == seed_info.uuid
            && info.flags & SUBVOL_RDONLY == 0
            && btrfs::list(&volume)?.iter().any(|e| e == b"rootfs"),
        "container {name} is not a writable snapshot of the seed"
    );
    Ok(SeedContainer {
        name: name.to_string(),
        seed_uuid: uuid::Uuid::from_bytes(seed_info.uuid).to_string(),
        volume_uuid: uuid::Uuid::from_bytes(info.uuid).to_string(),
        volume_treeid: info.treeid,
    })
}

/// Read-only proof used before reserving a native creation and on replay.
/// It never snapshots, repairs a placeholder or cleans interrupted staging.
pub fn inspect_seed(location: &SeedLocation) -> Result<String> {
    validate_component(&location.pool)?;
    validate_component(&location.seed_dir)?;
    validate_component(&location.seed_name)?;
    let state = btrfs::open_absolute_dir(&location.incus_dir)?;
    let pools = btrfs::open_dir(&state, b"storage-pools")?.context("Incus storage-pools absent")?;
    let pool = btrfs::open_dir(&pools, location.pool.as_bytes())?.context("pool absent")?;
    ensure!(btrfs::mount_id(&pool)? != btrfs::mount_id(&pools)?
        && pool.metadata()?.ino() == SUBVOL_ROOT_INO, "pool is not its own mounted subvolume");
    let fsid = btrfs::fsid(&pool)?;
    ensure!(location.expected_pool_fsid.is_none_or(|expected| *expected.as_bytes() == fsid),
        "pool FSID differs from admitted creation intent");
    let parent = btrfs::open_dir(&pool, location.seed_dir.as_bytes())?.context("seed directory absent")?;
    let seed = btrfs::open_dir(&parent, location.seed_name.as_bytes())?.context("seed absent")?;
    let info = btrfs::subvolume_info(&seed)?;
    ensure!(seed.metadata()?.ino() == SUBVOL_ROOT_INO
        && btrfs::mount_id(&seed)? == btrfs::mount_id(&pool)?
        && btrfs::fsid(&seed)? == fsid && info.flags & SUBVOL_RDONLY != 0
        && location.expected_seed_uuid.is_none_or(|expected| *expected.as_bytes() == info.uuid),
        "seed identity, mount or readonly pin differs");
    let rootfs = btrfs::open_dir(&seed, b"rootfs")?.context("seed rootfs absent")?;
    ensure!(rootfs.metadata()?.ino() != SUBVOL_ROOT_INO
        && btrfs::list(&seed)?.iter().any(|entry| entry == b"metadata.yaml"),
        "seed lacks flat Incus rootfs/metadata layout");
    verify_seed_metadata(&seed, location, &info)?;
    Ok(uuid::Uuid::from_bytes(info.uuid).to_string())
}

pub fn inspect_container(location: &SeedLocation, name: &str) -> Result<SeedContainer> {
    validate_component(name)?;
    ensure!(!name.starts_with('.'), "container name must not start with dot");
    let seed_uuid = inspect_seed(location)?;
    let state = btrfs::open_absolute_dir(&location.incus_dir)?;
    let pools = btrfs::open_dir(&state, b"storage-pools")?.context("Incus storage-pools absent")?;
    let pool = btrfs::open_dir(&pools, location.pool.as_bytes())?.context("pool absent")?;
    let containers = btrfs::open_dir(&pool, b"containers")?.context("containers absent")?;
    ensure!(btrfs::mount_id(&containers)? == btrfs::mount_id(&pool)?, "containers covered by a mount");
    ensure!(btrfs::open_dir(&containers, format!(".{name}.seed").as_bytes())?.is_none(),
        "interrupted seed creation requires exact original-operation reconciliation");
    let volume = btrfs::open_volume(&containers, name.as_bytes(), location.expected_container_uid)?.context("container volume absent")?;
    let info = btrfs::subvolume_info(&volume)?;
    ensure!(volume.metadata()?.ino() == SUBVOL_ROOT_INO
        && btrfs::mount_id(&volume)? == btrfs::mount_id(&pool)?
        && btrfs::fsid(&volume)? == btrfs::fsid(&pool)?
        && info.parent_uuid == *uuid::Uuid::parse_str(&seed_uuid)?.as_bytes()
        && info.flags & SUBVOL_RDONLY == 0,
        "container is not the pinned writable Golden seed snapshot");
    let rootfs = btrfs::open_dir(&volume, b"rootfs")?.context("container rootfs absent")?;
    ensure!(rootfs.metadata()?.ino() != SUBVOL_ROOT_INO, "container rootfs is nested subvolume");
    Ok(SeedContainer { name: name.into(), seed_uuid,
        volume_uuid: uuid::Uuid::from_bytes(info.uuid).to_string(), volume_treeid: info.treeid })
}

fn validate_component(value: &str) -> Result<()> {
    ensure!(!value.is_empty() && value.len() <= 128 && value != "." && value != ".."
        && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'@')),
        "unsafe native storage component");
    Ok(())
}

pub(crate) fn read_backup(dir: &File) -> Result<(Option<Vec<u8>>, u32)> {
    let name = CString::new("backup.yaml")?;
    // SAFETY: held directory fd; O_NOFOLLOW|O_NONBLOCK refuse symlinks and FIFOs.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            return Ok((None, 0o400));
        }
        return Err(error).context("open Incus backup.yaml");
    }
    // SAFETY: successful openat returns an owned descriptor.
    let file = unsafe { File::from_raw_fd(fd) };
    let meta = file.metadata()?;
    ensure!(
        meta.is_file() && meta.uid() == 0 && meta.len() <= MAX_BACKUP_YAML,
        "Incus backup.yaml has unsafe metadata"
    );
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(MAX_BACKUP_YAML + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 == meta.len(), "backup.yaml changed while reading");
    Ok((Some(bytes), meta.mode() & 0o7777))
}

pub(crate) fn write_new_file(dir: &File, name: &[u8], bytes: &[u8], mode: u32) -> Result<()> {
    let c_name = CString::new(name)?;
    // SAFETY: held directory fd; O_EXCL|O_NOFOLLOW create only a new regular file.
    let fd = unsafe {
        libc::openat(
            dir.as_raw_fd(),
            c_name.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            mode as libc::c_uint,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error()).context("write backup.yaml into the new volume");
    }
    // SAFETY: successful openat returns an owned descriptor.
    let mut file = unsafe { File::from_raw_fd(fd) };
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn verify_seed_metadata(seed: &File, location: &SeedLocation, info: &btrfs::SubvolumeInfo) -> Result<()> {
    use sha2::{Digest as _, Sha256};
    ensure!(location.expected_seed_treeid.is_none_or(|id| id == info.treeid), "seed subvolume ID changed");
    let Some(expected) = location.expected_metadata_sha256 else { return Ok(()) };
    let name = CString::new("metadata.yaml")?;
    // SAFETY: held seed FD; fixed filename; refuse links, devices and FIFOs.
    let fd = unsafe { libc::openat(seed.as_raw_fd(), name.as_ptr(),
        libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK) };
    ensure!(fd >= 0, "seed metadata absent");
    // SAFETY: successful openat returned a fresh owned descriptor.
    let file = unsafe { File::from_raw_fd(fd) };
    ensure!(file.metadata()?.is_file() && file.metadata()?.len() <= MAX_BACKUP_YAML, "unsafe seed metadata");
    let mut bytes = Vec::new();
    file.take(MAX_BACKUP_YAML + 1).read_to_end(&mut bytes)?;
    let actual: [u8; 32] = Sha256::digest(&bytes).into();
    ensure!(bytes.len() as u64 <= MAX_BACKUP_YAML && actual == expected,
        "seed metadata hash differs from admitted Golden provenance");
    Ok(())
}
