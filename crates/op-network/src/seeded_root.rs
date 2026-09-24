//! Fail-closed mount gate for a prepared Btrfs seed and writable sprout.
//!
//! A mount gate for an existing sprout and a distinct, exclusive provisioning
//! path for a new blank leaf. Neither path formats an existing filesystem.

use std::collections::BTreeSet;
use std::ffi::CString;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::FileExt;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const SUPER_OFFSET: u64 = 64 * 1024;
const BTRFS_MAGIC: u64 = 0x4d5f_5366_5248_425f;
const BTRFS_SEEDING_FLAG: u64 = 1 << 32;
const BTRFS_STATFS_MAGIC: libc::c_long = 0x9123_683e;
const BTRFS_IOC_FS_INFO: libc::c_ulong = 0x8400_941f;
const BTRFS_IOC_DEV_INFO: libc::c_ulong = 0xd000_941e;
const BTRFS_IOC_GET_SUBVOL_INFO: libc::c_ulong = 0x81f8_943c;
const BTRFS_IOC_SUBVOL_GETFLAGS: libc::c_ulong = 0x8008_9419;
const BTRFS_IOC_SUBVOL_CREATE: libc::c_ulong = 0x5000_940e;
const LOOP_SET_FD: libc::c_ulong = 0x4c00;
const LOOP_CLR_FD: libc::c_ulong = 0x4c01;
const LOOP_GET_STATUS64: libc::c_ulong = 0x4c05;
const LOOP_CTL_GET_FREE: libc::c_ulong = 0x4c82;
const LO_FLAGS_READ_ONLY: u32 = 1;
const LO_FLAGS_AUTOCLEAR: u32 = 4;

/// A trusted, prepared pool root. `pool_subvolume` is one direct Btrfs
/// subvolume under `mount_point`, not a path or the target instance root.
pub struct SeededRootDescriptor<'a> {
    pub seed_image: &'a Path,
    pub seed_sha256: &'a str,
    pub seed_fsid: Uuid,
    pub leaf_image: &'a Path,
    pub sprout_fsid: Uuid,
    pub mount_point: &'a Path,
    pub pool_subvolume: &'a str,
}

/// Provisioning is a distinct, one-shot operation. `new_leaf_image` must not
/// exist: the kernel will claim only the blank file created by this call.
pub struct SeededProvisionDescriptor<'a> {
    pub seed_image: &'a Path,
    pub seed_sha256: &'a str,
    pub seed_fsid: Uuid,
    /// Exact image cache allowed in the sealed core before any identity root.
    pub image_fingerprint: &'a str,
    pub new_leaf_image: &'a Path,
    pub leaf_size_bytes: u64,
    pub mount_point: &'a Path,
}

/// Persist this receipt with the identity owner's journal before selecting
/// the new filesystem for an Incus instance or committing its Cozo binding.
#[derive(Debug, Clone)]
pub struct SeededProvisionReceipt {
    pub seed_image: PathBuf,
    pub seed_sha256: String,
    pub seed_fsid: Uuid,
    pub leaf_image: PathBuf,
    pub sprout_fsid: Uuid,
    pub seed_loop: PathBuf,
    pub leaf_loop: PathBuf,
    pub mount_point: PathBuf,
    pub journal_path: PathBuf,
}

/// Holds the one-leaf lock and exact mount while the owner records the receipt
/// and prepares the pool/persist subvolumes. Dropping it does not unmount.
pub struct SeededProvisionGuard {
    pub receipt: SeededProvisionReceipt,
    seed_file: File,
    leaf_file: File,
    leaf_lock: File,
    mount_file: File,
    journal_file: File,
}

/// Keep this guard alive while applying the corresponding Incus target action.
/// The loop mappings and mount intentionally remain after the guard drops.
pub struct SeededRootMount {
    pub seed_loop: PathBuf,
    pub leaf_loop: PathBuf,
    pub mount_point: PathBuf,
    pub pool_path: PathBuf,
    pub mounted_now: bool,
    seed_image: PathBuf,
    leaf_image: PathBuf,
    seed_sha256: String,
    seed_fsid: Uuid,
    sprout_fsid: Uuid,
    pool_subvolume: String,
    _seed_file: File,
    _leaf_file: File,
    _leaf_lock: File,
    _mount_file: File,
}

#[repr(C)]
struct FsInfo {
    max_id: u64,
    num_devices: u64,
    fsid: [u8; 16],
    nodesize: u32,
    sectorsize: u32,
    clone_alignment: u32,
    csum_type: u16,
    csum_size: u16,
    flags: u64,
    generation: u64,
    metadata_uuid: [u8; 16],
    reserved: [u8; 944],
}

#[repr(C)]
struct DevInfo {
    devid: u64,
    uuid: [u8; 16],
    bytes_used: u64,
    total_bytes: u64,
    fsid: [u8; 16],
    unused: [u64; 377],
    path: [u8; 1024],
}

#[repr(C)]
struct VolArgs {
    fd: i64,
    name: [u8; 4088],
}

const _: () = assert!(std::mem::size_of::<VolArgs>() == 4096);

#[repr(C)]
struct LoopInfo64 {
    lo_device: u64,
    lo_inode: u64,
    lo_rdevice: u64,
    lo_offset: u64,
    lo_sizelimit: u64,
    lo_number: u32,
    lo_encrypt_type: u32,
    lo_encrypt_key_size: u32,
    lo_flags: u32,
    lo_file_name: [u8; 64],
    lo_crypt_name: [u8; 64],
    lo_encrypt_key: [u8; 32],
    lo_init: [u64; 2],
}

const _: () = assert!(std::mem::size_of::<FsInfo>() == 1024);
const _: () = assert!(std::mem::size_of::<DevInfo>() == 4096);
const _: () = assert!(std::mem::size_of::<LoopInfo64>() == 232);

struct Backings {
    seed: File,
    leaf: File,
    leaf_lock: File,
    seed_meta: Metadata,
    leaf_meta: Metadata,
}

struct LoopMapping {
    path: PathBuf,
    file: File,
    created: bool,
}

fn c_path(path: &Path) -> Result<CString> {
    CString::new(path.as_os_str().as_bytes()).context("path contains NUL")
}

fn secure_ancestors(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "prepared path must be absolute");
    for ancestor in path.ancestors().skip(1) {
        let meta = fs::symlink_metadata(ancestor)
            .with_context(|| format!("inspect ancestor {}", ancestor.display()))?;
        ensure!(
            meta.is_dir() && meta.uid() == 0,
            "prepared path ancestor must be a root-owned directory"
        );
        // A root-owned sticky temporary directory is safe only when every
        // descendant leading to the prepared object is root-owned as well.
        let sticky_root = meta.mode() & 0o1000 != 0 && meta.mode() & 0o002 != 0;
        ensure!(
            meta.mode() & 0o022 == 0 || sticky_root,
            "prepared path ancestor is writable by another principal"
        );
    }
    Ok(())
}

fn root_owned_regular(path: &Path, writable: bool) -> Result<(File, Metadata)> {
    ensure!(path.is_absolute(), "backing image path must be absolute");
    secure_ancestors(path)?;
    ensure!(
        fs::canonicalize(path).context("canonicalize backing image")? == path,
        "backing image path must be canonical and contain no symlink"
    );
    let link_meta = fs::symlink_metadata(path).context("inspect backing image path")?;
    ensure!(
        link_meta.file_type().is_file(),
        "backing must be a regular image"
    );
    ensure!(link_meta.uid() == 0, "backing image must be root-owned");
    ensure!(
        link_meta.mode() & 0o022 == 0,
        "backing image must not be group or world writable"
    );
    let file = OpenOptions::new()
        .read(true)
        .write(writable)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .with_context(|| format!("open prepared backing {}", path.display()))?;
    let meta = file.metadata()?;
    ensure!(meta.is_file(), "opened backing is not a regular image");
    ensure!(
        meta.uid() == 0 && meta.mode() & 0o022 == 0,
        "opened backing is not root-owned and write-protected"
    );
    ensure!(
        meta.dev() == link_meta.dev() && meta.ino() == link_meta.ino(),
        "backing image changed while opening"
    );
    Ok((file, meta))
}

fn read_exact_at(file: &File, mut offset: u64, mut out: &mut [u8]) -> Result<()> {
    while !out.is_empty() {
        let count = file.read_at(out, offset)?;
        ensure!(count > 0, "short backing image read");
        offset += count as u64;
        out = &mut out[count..];
    }
    Ok(())
}

fn superblock_identity(file: &File, expected_fsid: Uuid, must_seed: bool) -> Result<()> {
    let mut header = [0u8; 80];
    read_exact_at(file, SUPER_OFFSET, &mut header)?;
    let fsid: [u8; 16] = header[32..48].try_into()?;
    let bytenr = u64::from_le_bytes(header[48..56].try_into()?);
    let flags = u64::from_le_bytes(header[56..64].try_into()?);
    let magic = u64::from_le_bytes(header[64..72].try_into()?);
    ensure!(
        magic == BTRFS_MAGIC && bytenr == SUPER_OFFSET,
        "invalid Btrfs superblock"
    );
    ensure!(
        fsid == *expected_fsid.as_bytes(),
        "backing filesystem FSID mismatch"
    );
    ensure!(
        (flags & BTRFS_SEEDING_FLAG != 0) == must_seed,
        "backing seed flag mismatch"
    );
    Ok(())
}

fn sha256_file(file: &File, size: u64) -> Result<String> {
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    let mut offset = 0u64;
    while offset < size {
        let length = ((size - offset) as usize).min(buffer.len());
        let count = file.read_at(&mut buffer[..length], offset)?;
        ensure!(count > 0, "short seed image read during digest");
        digest.update(&buffer[..count]);
        offset += count as u64;
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn prepare_backings(descriptor: &SeededRootDescriptor<'_>) -> Result<Backings> {
    ensure!(
        descriptor.seed_sha256.len() == 64
            && descriptor
                .seed_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit()),
        "invalid pinned seed SHA-256"
    );
    ensure!(
        descriptor.pool_subvolume != "."
            && descriptor.pool_subvolume != ".."
            && !descriptor.pool_subvolume.is_empty()
            && !descriptor.pool_subvolume.contains('/')
            && !descriptor.pool_subvolume.contains('\0'),
        "pool subvolume must be one direct path component"
    );
    ensure!(
        descriptor.seed_fsid != descriptor.sprout_fsid,
        "seed and sprout FSIDs must differ"
    );
    let (seed, seed_meta) = root_owned_regular(descriptor.seed_image, false)?;
    let (leaf, leaf_meta) = root_owned_regular(descriptor.leaf_image, true)?;
    ensure!(
        seed_meta.dev() != leaf_meta.dev() || seed_meta.ino() != leaf_meta.ino(),
        "seed and writable leaf must be distinct images"
    );
    let leaf_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(descriptor.leaf_image)
        .context("open independent writable leaf lock")?;
    let lock_meta = leaf_lock.metadata()?;
    ensure!(
        lock_meta.dev() == leaf_meta.dev()
            && lock_meta.ino() == leaf_meta.ino()
            && lock_meta.uid() == 0
            && lock_meta.mode() & 0o022 == 0,
        "writable leaf lock path changed"
    );
    // Serializes this native owner across repair/start actions. A caller must
    // retain the returned guard until the corresponding Incus action finishes.
    // Do not lock the FD passed to LOOP_SET_FD: the kernel retains that open
    // file description, which would otherwise hold the flock after our exit.
    if unsafe { libc::flock(leaf_lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::last_os_error()).context("writable leaf is busy");
    }
    superblock_identity(&seed, descriptor.seed_fsid, true)?;
    superblock_identity(&leaf, descriptor.sprout_fsid, false)?;
    ensure!(
        sha256_file(&seed, seed_meta.len())?.eq_ignore_ascii_case(descriptor.seed_sha256),
        "sealed seed digest mismatch"
    );
    recheck_path(descriptor.seed_image, &seed_meta)?;
    recheck_path(descriptor.leaf_image, &leaf_meta)?;
    Ok(Backings {
        seed,
        leaf,
        leaf_lock,
        seed_meta,
        leaf_meta,
    })
}

fn recheck_path(path: &Path, opened: &Metadata) -> Result<()> {
    secure_ancestors(path)?;
    let current = fs::symlink_metadata(path).context("recheck prepared image path")?;
    ensure!(
        current.is_file()
            && current.dev() == opened.dev()
            && current.ino() == opened.ino()
            && current.len() == opened.len()
            && current.uid() == 0
            && current.mode() & 0o022 == 0,
        "prepared backing path changed during verification"
    );
    Ok(())
}

fn mount_id(path: &Path) -> Result<u64> {
    let path = c_path(path)?;
    // SAFETY: statx receives an initialized output struct and NUL-terminated path.
    let mut info: libc::statx = unsafe { std::mem::zeroed() };
    let rc = unsafe {
        libc::statx(
            libc::AT_FDCWD,
            path.as_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
            libc::STATX_MNT_ID,
            &mut info,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error()).context("statx mount boundary");
    }
    ensure!(
        info.stx_mask & libc::STATX_MNT_ID != 0,
        "kernel lacks mount ID readback"
    );
    Ok(info.stx_mnt_id)
}

fn mount_id_fd(file: &File) -> Result<u64> {
    let mut info: libc::statx = unsafe { std::mem::zeroed() };
    let empty = CString::new("")?;
    let rc = unsafe {
        libc::statx(
            file.as_raw_fd(),
            empty.as_ptr(),
            libc::AT_EMPTY_PATH,
            libc::STATX_MNT_ID,
            &mut info,
        )
    };
    if rc != 0 {
        return Err(io::Error::last_os_error()).context("statx pinned mount");
    }
    ensure!(
        info.stx_mask & libc::STATX_MNT_ID != 0,
        "kernel lacks mount ID readback"
    );
    Ok(info.stx_mnt_id)
}

fn exact_mount_present(path: &Path) -> Result<bool> {
    ensure!(path.is_absolute(), "mount path must be absolute");
    secure_ancestors(path)?;
    ensure!(
        fs::canonicalize(path)? == path,
        "mount path must be canonical and unsymlinked"
    );
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_dir() && meta.uid() == 0,
        "mount target must be a root-owned directory"
    );
    ensure!(
        meta.mode() & 0o022 == 0,
        "mount target must not be group or world writable"
    );
    let parent = path.parent().context("mount path has no parent")?;
    Ok(mount_id(path)? != mount_id(parent)?)
}

fn private_empty_target(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_dir() && meta.uid() == 0,
        "mount target must be a root-owned directory"
    );
    ensure!(
        meta.mode() & 0o077 == 0,
        "unmounted target must be private to root"
    );
    ensure!(
        fs::read_dir(path)?.next().is_none(),
        "unmounted target is not empty"
    );
    Ok(())
}

fn cleanup_created_mount(path: &Path, pinned: File, pinned_id: u64) -> Result<()> {
    secure_ancestors(path)?;
    ensure!(
        fs::canonicalize(path)? == path,
        "new mount path changed before cleanup"
    );
    let parent = path.parent().context("mount path has no parent")?;
    let on_path = mount_id(path)?;
    ensure!(
        on_path == pinned_id
            && mount_id_fd(&pinned)? == pinned_id
            && mount_id(parent)? != pinned_id,
        "new mount identity changed before cleanup"
    );
    let at_path = fs::metadata(path)?;
    let held = pinned.metadata()?;
    ensure!(
        at_path.dev() == held.dev() && at_path.ino() == held.ino(),
        "new mount root changed before cleanup"
    );
    let target = c_path(path)?;
    // A held directory FD makes a normal unmount busy. Release only our FD
    // after the exact mount and root inode checks, then unmount the private
    // root-owned target. A failed unmount retains both loop mappings.
    drop(pinned);
    // SAFETY: the exact mount ID and pinned root have just been revalidated.
    if unsafe { libc::umount2(target.as_ptr(), 0) } != 0 {
        return Err(io::Error::last_os_error()).context("unmount newly created sprout");
    }
    Ok(())
}

fn loop_status(file: &File) -> Result<LoopInfo64> {
    // SAFETY: loop_info64 is a fixed-size integer-only kernel ABI struct.
    let mut status: LoopInfo64 = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(file.as_raw_fd(), LOOP_GET_STATUS64, &mut status) } != 0 {
        return Err(io::Error::last_os_error()).context("query loop mapping");
    }
    Ok(status)
}

fn backing_matches(status: &LoopInfo64, meta: &Metadata) -> bool {
    status.lo_device == meta.dev()
        && status.lo_inode == meta.ino()
        && status.lo_rdevice == meta.rdev()
}

fn validate_loop(status: &LoopInfo64, meta: &Metadata, readonly: bool) -> Result<()> {
    ensure!(
        backing_matches(status, meta),
        "loop backing identity mismatch"
    );
    ensure!(
        status.lo_offset == 0 && status.lo_sizelimit == 0,
        "partial loop mapping refused"
    );
    ensure!(
        status.lo_flags & LO_FLAGS_AUTOCLEAR == 0,
        "autoclear loop mapping refused"
    );
    ensure!(
        (status.lo_flags & LO_FLAGS_READ_ONLY != 0) == readonly,
        "loop read-only mode mismatch"
    );
    Ok(())
}

fn open_loop(path: &Path, writable: bool) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(writable)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    ensure!(
        file.metadata()?.file_type().is_block_device(),
        "loop path is not a block device"
    );
    Ok(file)
}

fn existing_loop(meta: &Metadata, readonly: bool) -> Result<Option<LoopMapping>> {
    let mut found = None;
    for entry in fs::read_dir("/dev")? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(digits) = name.strip_prefix("loop") else {
            continue;
        };
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if digits.parse::<u32>().ok().filter(|n| *n < 4096).is_none() {
            continue;
        }
        let file = match open_loop(&entry.path(), false) {
            Ok(file) => file,
            Err(_) => continue,
        };
        let status = match loop_status(&file) {
            Ok(status) => status,
            Err(_) => continue,
        };
        if !backing_matches(&status, meta) {
            continue;
        }
        validate_loop(&status, meta, readonly)?;
        ensure!(
            found.is_none(),
            "multiple loop mappings reference one prepared image"
        );
        found = Some(LoopMapping {
            path: entry.path(),
            file,
            created: false,
        });
    }
    Ok(found)
}

fn detach_created(mapping: &LoopMapping) -> Result<()> {
    if mapping.created && unsafe { libc::ioctl(mapping.file.as_raw_fd(), LOOP_CLR_FD) } != 0 {
        return Err(io::Error::last_os_error()).context("detach newly allocated loop");
    }
    Ok(())
}

fn allocate_loop(backing: &File, meta: &Metadata, readonly: bool) -> Result<LoopMapping> {
    let control = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/loop-control")
        .context("open loop control")?;
    for _ in 0..16 {
        // SAFETY: LOOP_CTL_GET_FREE takes no pointer argument and returns an index.
        let index = unsafe { libc::ioctl(control.as_raw_fd(), LOOP_CTL_GET_FREE) };
        if index < 0 {
            return Err(io::Error::last_os_error()).context("allocate free loop index");
        }
        let path = PathBuf::from(format!("/dev/loop{index}"));
        let file = open_loop(&path, !readonly)?;
        // SAFETY: Both FDs are open; the kernel retains the backing file.
        if unsafe { libc::ioctl(file.as_raw_fd(), LOOP_SET_FD, backing.as_raw_fd()) } != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EBUSY) {
                continue;
            }
            return Err(error).context("attach prepared image to new loop");
        }
        let mapping = LoopMapping {
            path,
            file,
            created: true,
        };
        if let Err(error) =
            loop_status(&mapping.file).and_then(|status| validate_loop(&status, meta, readonly))
        {
            detach_created(&mapping).context("clean up invalid newly allocated loop")?;
            return Err(error);
        }
        return Ok(mapping);
    }
    bail!("loop allocation remained busy after bounded retries")
}

fn get_loop(
    backing: &File,
    meta: &Metadata,
    readonly: bool,
    allow_allocate: bool,
) -> Result<LoopMapping> {
    if let Some(existing) = existing_loop(meta, readonly)? {
        return Ok(existing);
    }
    ensure!(
        allow_allocate,
        "prepared image has no registered loop mapping"
    );
    allocate_loop(backing, meta, readonly)
}

fn mounted_fs_info(root: &File, expected: Uuid) -> Result<FsInfo> {
    // SAFETY: fstatfs writes to the initialized fixed-size C struct.
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatfs(root.as_raw_fd(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error()).context("stat mounted filesystem");
    }
    ensure!(
        stat.f_type == BTRFS_STATFS_MAGIC,
        "target is not mounted Btrfs"
    );
    // SAFETY: ioctl writes to the correctly sized kernel ABI struct.
    let mut info: FsInfo = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(root.as_raw_fd(), BTRFS_IOC_FS_INFO, &mut info) } != 0 {
        return Err(io::Error::last_os_error()).context("query mounted Btrfs identity");
    }
    ensure!(
        info.fsid == *expected.as_bytes(),
        "mounted sprout FSID mismatch"
    );
    ensure!(
        info.num_devices == 1,
        "sprout must have exactly one writable device"
    );
    ensure!(
        info.max_id > 0 && info.max_id <= 16,
        "unexpected Btrfs device ID range"
    );
    Ok(info)
}

fn verify_member(root: &File, id: u64) -> Result<Option<DevInfo>> {
    // SAFETY: ioctl writes to the correctly sized kernel ABI struct.
    let mut dev: DevInfo = unsafe { std::mem::zeroed() };
    dev.devid = id;
    if unsafe { libc::ioctl(root.as_raw_fd(), BTRFS_IOC_DEV_INFO, &mut dev) } != 0 {
        let error = io::Error::last_os_error();
        if matches!(
            error.raw_os_error(),
            Some(libc::ENODEV) | Some(libc::ENOENT) | Some(libc::EINVAL)
        ) {
            return Ok(None);
        }
        return Err(error).context("enumerate mounted Btrfs member");
    }
    Ok(Some(dev))
}

fn verify_members(
    root: &File,
    info: &FsInfo,
    backings: &Backings,
    seed: &LoopMapping,
    leaf: &LoopMapping,
    descriptor: &SeededRootDescriptor<'_>,
) -> Result<()> {
    let seed_rdev = seed.file.metadata()?.rdev();
    let leaf_rdev = leaf.file.metadata()?.rdev();
    ensure!(seed_rdev != leaf_rdev, "seed and leaf loops are identical");
    let mut saw_seed = false;
    let mut saw_leaf = false;
    let mut count = 0;
    for id in 1..=info.max_id {
        let Some(dev) = verify_member(root, id)? else {
            continue;
        };
        count += 1;
        let end = dev
            .path
            .iter()
            .position(|b| *b == 0)
            .context("unterminated Btrfs device path")?;
        let path = Path::new(std::str::from_utf8(&dev.path[..end])?);
        let member = open_loop(path, false).context("open Btrfs member loop")?;
        let rdev = member.metadata()?.rdev();
        if rdev == seed_rdev && dev.fsid == *descriptor.seed_fsid.as_bytes() {
            ensure!(!saw_seed, "duplicate seed member");
            validate_loop(&loop_status(&member)?, &backings.seed_meta, true)?;
            saw_seed = true;
        } else if rdev == leaf_rdev && dev.fsid == *descriptor.sprout_fsid.as_bytes() {
            ensure!(!saw_leaf, "duplicate writable leaf member");
            validate_loop(&loop_status(&member)?, &backings.leaf_meta, false)?;
            saw_leaf = true;
        } else {
            bail!("mounted sprout has an unexpected Btrfs member");
        }
    }
    ensure!(
        count == 2 && saw_seed && saw_leaf,
        "mounted sprout ancestry is incomplete"
    );
    Ok(())
}

fn subvolume_identity(file: &File) -> Result<(u64, u64, Vec<u8>)> {
    let mut raw = [0u8; 504];
    // SAFETY: the output buffer matches btrfs_ioctl_get_subvol_info_args size.
    if unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            BTRFS_IOC_GET_SUBVOL_INFO,
            raw.as_mut_ptr(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error()).context("query Btrfs subvolume identity");
    }
    let treeid = u64::from_ne_bytes(raw[0..8].try_into()?);
    let parent_id = u64::from_ne_bytes(raw[264..272].try_into()?);
    let name_end = raw[8..264]
        .iter()
        .position(|b| *b == 0)
        .context("unterminated subvolume name")?;
    Ok((treeid, parent_id, raw[8..8 + name_end].to_vec()))
}

fn verify_pool_subvolume(
    descriptor: &SeededRootDescriptor<'_>,
    mount_id_root: u64,
) -> Result<PathBuf> {
    let path = descriptor.mount_point.join(descriptor.pool_subvolume);
    ensure!(
        fs::canonicalize(&path)? == path,
        "pool path is not canonical"
    );
    let meta = fs::symlink_metadata(&path)?;
    ensure!(
        meta.is_dir() && meta.ino() == 256,
        "pool is not a Btrfs subvolume root"
    );
    ensure!(
        mount_id(&path)? == mount_id_root,
        "pool subvolume is covered by another mount"
    );
    let file = File::open(&path)?;
    let (treeid, parent_id, name) = subvolume_identity(&file)?;
    ensure!(
        treeid > 5 && parent_id == 5,
        "pool is not directly under the Btrfs root"
    );
    ensure!(
        name == descriptor.pool_subvolume.as_bytes(),
        "pool subvolume name mismatch"
    );
    Ok(path)
}

fn direct_names(path: &Path) -> Result<BTreeSet<String>> {
    fs::read_dir(path)?
        .map(|entry| {
            let name = entry?.file_name();
            Ok(name
                .to_str()
                .context("non-UTF-8 core seed entry")?
                .to_owned())
        })
        .collect()
}

fn verify_clean_core_layout(
    mount_point: &Path,
    expected_mount_id: u64,
    image_fingerprint: &str,
    allow_persist: bool,
) -> Result<()> {
    ensure!(
        image_fingerprint.len() == 64
            && image_fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "invalid pinned core image fingerprint"
    );
    let expected_root = if allow_persist {
        BTreeSet::from(["pool".to_owned(), "persist".to_owned()])
    } else {
        BTreeSet::from(["pool".to_owned()])
    };
    ensure!(
        direct_names(mount_point)? == expected_root,
        "core seed has an unexpected root entry"
    );
    let pool = mount_point.join("pool");
    let pool_meta = fs::symlink_metadata(&pool)?;
    ensure!(
        pool_meta.is_dir() && pool_meta.ino() == 256 && mount_id(&pool)? == expected_mount_id,
        "core pool is not a direct subvolume on the verified mount"
    );
    let pool_file = File::open(&pool)?;
    let (pool_id, parent_id, pool_name) = subvolume_identity(&pool_file)?;
    ensure!(
        pool_id > 5 && parent_id == 5 && pool_name == b"pool",
        "core pool ancestry differs from sealed root"
    );
    let standard = BTreeSet::from([
        "buckets".to_owned(),
        "containers".to_owned(),
        "containers-snapshots".to_owned(),
        "custom".to_owned(),
        "custom-snapshots".to_owned(),
        "images".to_owned(),
        "virtual-machines".to_owned(),
        "virtual-machines-snapshots".to_owned(),
    ]);
    ensure!(
        direct_names(&pool)? == standard,
        "core pool has an unexpected volume category"
    );
    for category in &standard {
        let category_path = pool.join(category);
        let meta = fs::symlink_metadata(&category_path)?;
        ensure!(
            meta.is_dir() && mount_id(&category_path)? == expected_mount_id,
            "core pool category is not a directory on the verified mount"
        );
        if category != "images" {
            ensure!(
                direct_names(&category_path)?.is_empty(),
                "core seed contains a pre-existing container or other volume"
            );
        }
    }
    let images = pool.join("images");
    ensure!(
        direct_names(&images)? == BTreeSet::from([image_fingerprint.to_owned()]),
        "core seed image cache differs from pinned image"
    );
    let image = images.join(image_fingerprint);
    let image_meta = fs::symlink_metadata(&image)?;
    ensure!(
        image_meta.is_dir() && image_meta.ino() == 256 && mount_id(&image)? == expected_mount_id,
        "pinned image cache is not a direct Btrfs subvolume"
    );
    let image_file = File::open(&image)?;
    let (image_id, image_parent, image_name) = subvolume_identity(&image_file)?;
    ensure!(
        image_id > 5 && image_parent == pool_id && image_name == image_fingerprint.as_bytes(),
        "pinned image cache ancestry differs from core pool"
    );
    let mut image_flags = 0u64;
    if unsafe {
        libc::ioctl(
            image_file.as_raw_fd(),
            BTRFS_IOC_SUBVOL_GETFLAGS,
            &mut image_flags,
        )
    } != 0
    {
        return Err(io::Error::last_os_error()).context("query core image subvolume flags");
    }
    ensure!(
        image_flags & (1 << 1) != 0,
        "core image cache is not read-only"
    );
    ensure!(
        direct_names(&image)?
            == BTreeSet::from([
                "metadata.yaml".to_owned(),
                "rootfs".to_owned(),
                "templates".to_owned(),
            ]),
        "pinned image cache layout differs from Incus"
    );
    ensure!(
        fs::symlink_metadata(image.join("metadata.yaml"))?.is_file()
            && fs::symlink_metadata(image.join("rootfs"))?.is_dir()
            && fs::symlink_metadata(image.join("templates"))?.is_dir()
            && image.join("rootfs/usr/sbin/init").exists(),
        "pinned image cache lacks required Ubuntu rootfs"
    );
    Ok(())
}

fn verify_mount(
    descriptor: &SeededRootDescriptor<'_>,
    backings: &Backings,
    seed: &LoopMapping,
    leaf: &LoopMapping,
) -> Result<(File, PathBuf)> {
    ensure!(
        exact_mount_present(descriptor.mount_point)?,
        "dedicated sprout mount is absent"
    );
    let mount_file = File::open(descriptor.mount_point)?;
    let info = mounted_fs_info(&mount_file, descriptor.sprout_fsid)?;
    verify_members(&mount_file, &info, backings, seed, leaf, descriptor)?;
    let pool = verify_pool_subvolume(descriptor, mount_id(descriptor.mount_point)?)?;
    recheck_path(descriptor.seed_image, &backings.seed_meta)?;
    recheck_path(descriptor.leaf_image, &backings.leaf_meta)?;
    Ok((mount_file, pool))
}

fn finish_guard(
    descriptor: &SeededRootDescriptor<'_>,
    backings: Backings,
    seed: LoopMapping,
    leaf: LoopMapping,
    mount_file: File,
    pool_path: PathBuf,
    mounted_now: bool,
) -> SeededRootMount {
    SeededRootMount {
        seed_loop: seed.path,
        leaf_loop: leaf.path,
        mount_point: descriptor.mount_point.to_path_buf(),
        pool_path,
        mounted_now,
        seed_image: descriptor.seed_image.to_path_buf(),
        leaf_image: descriptor.leaf_image.to_path_buf(),
        seed_sha256: descriptor.seed_sha256.to_owned(),
        seed_fsid: descriptor.seed_fsid,
        sprout_fsid: descriptor.sprout_fsid,
        pool_subvolume: descriptor.pool_subvolume.to_owned(),
        _seed_file: backings.seed,
        _leaf_file: backings.leaf,
        _leaf_lock: backings.leaf_lock,
        _mount_file: mount_file,
    }
}

impl SeededRootMount {
    /// One-shot pre-import check. A selected identity root legitimately
    /// populates `pool/containers`, so do not call this on boot or start.
    pub fn verify_clean_core(
        &self,
        descriptor: &SeededRootDescriptor<'_>,
        image_fingerprint: &str,
    ) -> Result<()> {
        self.revalidate(descriptor)?;
        ensure!(
            descriptor.pool_subvolume == "pool",
            "unexpected core pool name"
        );
        verify_clean_core_layout(
            descriptor.mount_point,
            mount_id_fd(&self._mount_file)?,
            image_fingerprint,
            true,
        )
    }

    /// Recheck the prepared root while this guard still owns the leaf lock.
    /// Opening a second guard would contend with our own independent flock.
    pub fn revalidate(&self, descriptor: &SeededRootDescriptor<'_>) -> Result<()> {
        ensure!(
            descriptor.seed_image == self.seed_image
                && descriptor.leaf_image == self.leaf_image
                && descriptor.seed_sha256 == self.seed_sha256
                && descriptor.seed_fsid == self.seed_fsid
                && descriptor.sprout_fsid == self.sprout_fsid
                && descriptor.mount_point == self.mount_point
                && descriptor.pool_subvolume == self.pool_subvolume,
            "prepared seeded root descriptor changed while guarded"
        );
        ensure!(
            exact_mount_present(descriptor.mount_point)?,
            "dedicated sprout mount disappeared"
        );
        let held_id = mount_id_fd(&self._mount_file)?;
        ensure!(
            held_id == mount_id(descriptor.mount_point)?,
            "dedicated sprout mount was replaced"
        );
        let held_meta = self._mount_file.metadata()?;
        let path_meta = fs::metadata(descriptor.mount_point)?;
        ensure!(
            held_meta.dev() == path_meta.dev() && held_meta.ino() == path_meta.ino(),
            "dedicated sprout root changed"
        );
        let seed_meta = self._seed_file.metadata()?;
        let leaf_meta = self._leaf_file.metadata()?;
        let lock_meta = self._leaf_lock.metadata()?;
        ensure!(
            leaf_meta.dev() == lock_meta.dev() && leaf_meta.ino() == lock_meta.ino(),
            "writable leaf lock no longer matches its backing"
        );
        recheck_path(descriptor.seed_image, &seed_meta)?;
        recheck_path(descriptor.leaf_image, &leaf_meta)?;
        superblock_identity(&self._seed_file, descriptor.seed_fsid, true)?;
        superblock_identity(&self._leaf_file, descriptor.sprout_fsid, false)?;
        ensure!(
            sha256_file(&self._seed_file, seed_meta.len())?
                .eq_ignore_ascii_case(descriptor.seed_sha256),
            "sealed seed digest changed while guarded"
        );
        let backings = Backings {
            seed: self._seed_file.try_clone()?,
            leaf: self._leaf_file.try_clone()?,
            leaf_lock: self._leaf_lock.try_clone()?,
            seed_meta,
            leaf_meta,
        };
        let seed = get_loop(&backings.seed, &backings.seed_meta, true, false)?;
        let leaf = get_loop(&backings.leaf, &backings.leaf_meta, false, false)?;
        ensure!(
            seed.path == self.seed_loop && leaf.path == self.leaf_loop,
            "prepared loop device changed while guarded"
        );
        let (mount_file, pool_path) = verify_mount(descriptor, &backings, &seed, &leaf)?;
        ensure!(
            pool_path == self.pool_path && mount_id_fd(&mount_file)? == held_id,
            "prepared pool mount changed while guarded"
        );
        Ok(())
    }

    /// Verify a prepared persistence subvolume beside the pinned Incus pool,
    /// using this guard's existing leaf lock and mount FD.
    pub fn verify_persist_subvolume(
        &self,
        descriptor: &SeededRootDescriptor<'_>,
        child_name: &str,
        require_empty: bool,
    ) -> Result<PathBuf> {
        self.revalidate(descriptor)?;
        ensure!(
            !child_name.is_empty()
                && child_name != "."
                && child_name != ".."
                && !child_name.contains('/')
                && !child_name.contains('\0'),
            "persist subvolume must be one direct path component"
        );
        let path = self.mount_point.join(child_name);
        ensure!(
            fs::canonicalize(&path)? == path,
            "persist subvolume path is not canonical"
        );
        let meta = fs::symlink_metadata(&path)?;
        ensure!(
            meta.is_dir() && meta.ino() == 256 && meta.uid() == 0 && meta.mode() & 0o022 == 0,
            "persist is not a secure Btrfs subvolume root"
        );
        let held_id = mount_id_fd(&self._mount_file)?;
        ensure!(
            mount_id(&path)? == held_id,
            "persist is covered by another mount"
        );
        let child = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(&path)?;
        mounted_fs_info(&child, descriptor.sprout_fsid)?;
        let (child_tree, parent_tree, name) = subvolume_identity(&child)?;
        ensure!(
            child_tree > 5 && parent_tree == 5 && name == child_name.as_bytes(),
            "persist is not a direct child of the prepared Btrfs root"
        );
        if require_empty {
            ensure!(
                fs::read_dir(&path)?.next().is_none(),
                "prepared persist subvolume is not empty"
            );
        }
        ensure!(
            mount_id(&path)? == held_id,
            "persist mount changed during verification"
        );
        Ok(path)
    }
}

/// Verify an existing exact mount and its two kernel members. This never
/// allocates a loop, mounts, or changes filesystem state.
pub fn verify_seeded_root_mounted(
    descriptor: &SeededRootDescriptor<'_>,
) -> Result<SeededRootMount> {
    let backings = prepare_backings(descriptor)?;
    ensure!(
        exact_mount_present(descriptor.mount_point)?,
        "dedicated sprout mount is absent"
    );
    let seed = get_loop(&backings.seed, &backings.seed_meta, true, false)?;
    let leaf = get_loop(&backings.leaf, &backings.leaf_meta, false, false)?;
    let (mount_file, pool_path) = verify_mount(descriptor, &backings, &seed, &leaf)?;
    Ok(finish_guard(
        descriptor, backings, seed, leaf, mount_file, pool_path, false,
    ))
}

/// Reuse an exact mount or lazily map and mount a previously prepared sprout.
/// Runtime creation of a Btrfs filesystem or seed/leaf relationship is refused.
pub fn ensure_seeded_root_mounted(
    descriptor: &SeededRootDescriptor<'_>,
) -> Result<SeededRootMount> {
    let backings = prepare_backings(descriptor)?;
    let present = exact_mount_present(descriptor.mount_point)?;
    if !present {
        private_empty_target(descriptor.mount_point)?;
    }
    let seed = get_loop(&backings.seed, &backings.seed_meta, true, !present)?;
    let leaf = match get_loop(&backings.leaf, &backings.leaf_meta, false, !present) {
        Ok(leaf) => leaf,
        Err(error) => {
            detach_created(&seed)?;
            return Err(error);
        }
    };
    let mut mounted_now = false;
    let mut created_mount: Option<(File, u64)> = None;
    let result = (|| {
        if !present {
            let source = c_path(&leaf.path)?;
            let target = c_path(descriptor.mount_point)?;
            let fs_type = CString::new("btrfs")?;
            let options = CString::new(format!(
                "rw,device={},device={}",
                seed.path.display(),
                leaf.path.display()
            ))?;
            // SAFETY: all pointers are NUL-terminated and remain alive during mount(2).
            if unsafe {
                libc::mount(
                    source.as_ptr(),
                    target.as_ptr(),
                    fs_type.as_ptr(),
                    0,
                    options.as_ptr().cast(),
                )
            } != 0
            {
                return Err(io::Error::last_os_error()).context("mount prepared sprout leaf");
            }
            mounted_now = true;
            let held = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
                .open(descriptor.mount_point)
                .context("pin newly created mount root")?;
            let id = mount_id_fd(&held)?;
            ensure!(
                id == mount_id(descriptor.mount_point)?
                    && id != mount_id(descriptor.mount_point.parent().context("mount parent")?)?,
                "new mount boundary changed while pinning"
            );
            created_mount = Some((held, id));
        }
        verify_mount(descriptor, &backings, &seed, &leaf)
    })();
    match result {
        Ok((mount_file, pool_path)) => Ok(finish_guard(
            descriptor,
            backings,
            seed,
            leaf,
            mount_file,
            pool_path,
            mounted_now,
        )),
        Err(error) => {
            if mounted_now {
                let (held, id) = created_mount.take().context(
                    "newly created mount could not be pinned; retaining mount and loops",
                )?;
                cleanup_created_mount(descriptor.mount_point, held, id).with_context(|| {
                    format!(
                        "newly created mount failed verification ({error}); retaining mounted loops"
                    )
                })?;
            }
            detach_created(&leaf)?;
            detach_created(&seed)?;
            Err(error)
        }
    }
}

fn provision_journal(file: &mut File, stage: &str, data: serde_json::Value) -> Result<()> {
    let record = serde_json::json!({ "stage": stage, "data": data });
    writeln!(file, "{}", record).context("append seed/leaf provision journal")?;
    file.sync_data().context("sync seed/leaf provision journal")
}

fn seed_only_mount(
    root: &File,
    seed: &LoopMapping,
    seed_meta: &Metadata,
    fsid: Uuid,
) -> Result<()> {
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatfs(root.as_raw_fd(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error()).context("stat seed-only mount");
    }
    ensure!(stat.f_type == BTRFS_STATFS_MAGIC, "seed mount is not Btrfs");
    let mut info: FsInfo = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(root.as_raw_fd(), BTRFS_IOC_FS_INFO, &mut info) } != 0 {
        return Err(io::Error::last_os_error()).context("query seed-only filesystem");
    }
    ensure!(
        info.num_devices == 1 && info.max_id > 0 && info.max_id <= 16,
        "seed mount has an unexpected device set"
    );
    let expected_rdev = seed.file.metadata()?.rdev();
    let mut seen = 0;
    for id in 1..=info.max_id {
        let Some(dev) = verify_member(root, id)? else {
            continue;
        };
        seen += 1;
        let end = dev
            .path
            .iter()
            .position(|b| *b == 0)
            .context("unterminated seed device path")?;
        let path = Path::new(std::str::from_utf8(&dev.path[..end])?);
        let member = open_loop(path, false)?;
        ensure!(
            member.metadata()?.rdev() == expected_rdev && dev.fsid == *fsid.as_bytes(),
            "seed mount uses a different kernel device"
        );
        validate_loop(&loop_status(&member)?, seed_meta, true)?;
    }
    ensure!(seen == 1, "seed mount has an unexpected member count");
    Ok(())
}

fn preflight_clean_seed(
    descriptor: &SeededProvisionDescriptor<'_>,
    seed_file: &File,
    seed_meta: &Metadata,
) -> Result<LoopMapping> {
    let seed = allocate_loop(seed_file, seed_meta, true)?;
    let source = c_path(&seed.path)?;
    let target = c_path(descriptor.mount_point)?;
    let fs_type = CString::new("btrfs")?;
    let options = CString::new(format!("ro,device={}", seed.path.display()))?;
    if unsafe {
        libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            fs_type.as_ptr(),
            libc::MS_RDONLY,
            options.as_ptr().cast(),
        )
    } != 0
    {
        let error = io::Error::last_os_error();
        detach_created(&seed)?;
        return Err(error).context("mount core seed for clean preflight");
    }
    let mounted = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(descriptor.mount_point)
        .context("pin clean-core mount; retain unpinned mount on failure")?;
    let pinned_id = mount_id_fd(&mounted)
        .context("pin clean-core mount ID; retain unknown mount on failure")?;
    let result = (|| {
        ensure!(
            pinned_id == mount_id(descriptor.mount_point)?
                && pinned_id
                    != mount_id(
                        descriptor
                            .mount_point
                            .parent()
                            .context("core mount parent")?
                    )?,
            "clean-core preflight mount boundary changed"
        );
        seed_only_mount(&mounted, &seed, seed_meta, descriptor.seed_fsid)?;
        verify_clean_core_layout(
            descriptor.mount_point,
            pinned_id,
            descriptor.image_fingerprint,
            false,
        )
    })();
    cleanup_created_mount(descriptor.mount_point, mounted, pinned_id)?;
    if let Err(error) = result {
        detach_created(&seed).context("detach rejected preflight seed")?;
        return Err(error);
    }
    // Keep the exact validated mapping across the unmount and main provision.
    // LOOP_CLR_FD can return success while Btrfs still holds a reference,
    // leaving an AUTOCLEAR mapping that a subsequent allocation must reject.
    Ok(seed)
}

fn provision_leaf_fsid(leaf: &File) -> Result<Uuid> {
    let mut header = [0u8; 80];
    read_exact_at(leaf, SUPER_OFFSET, &mut header)?;
    ensure!(
        u64::from_le_bytes(header[48..56].try_into()?) == SUPER_OFFSET
            && u64::from_le_bytes(header[64..72].try_into()?) == BTRFS_MAGIC,
        "new leaf has no Btrfs superblock after device add"
    );
    ensure!(
        u64::from_le_bytes(header[56..64].try_into()?) & BTRFS_SEEDING_FLAG == 0,
        "new leaf unexpectedly became a seed"
    );
    let fsid = Uuid::from_bytes(header[32..48].try_into()?);
    ensure!(!fsid.is_nil(), "new leaf has a null filesystem UUID");
    Ok(fsid)
}

impl SeededProvisionGuard {
    /// Recheck the exact seed, writable leaf, loops and mounted filesystem
    /// while retaining the same independent leaf lock.
    pub fn revalidate(&self) -> Result<()> {
        let receipt = &self.receipt;
        ensure!(
            exact_mount_present(&receipt.mount_point)?,
            "provisioned mount disappeared"
        );
        let held_id = mount_id_fd(&self.mount_file)?;
        ensure!(
            held_id == mount_id(&receipt.mount_point)?,
            "provisioned mount changed"
        );
        let held = self.mount_file.metadata()?;
        let current = fs::metadata(&receipt.mount_point)?;
        ensure!(
            held.dev() == current.dev() && held.ino() == current.ino(),
            "provisioned mount root changed"
        );
        let seed_meta = self.seed_file.metadata()?;
        let leaf_meta = self.leaf_file.metadata()?;
        let lock_meta = self.leaf_lock.metadata()?;
        ensure!(
            leaf_meta.dev() == lock_meta.dev() && leaf_meta.ino() == lock_meta.ino(),
            "provisioned leaf lock changed"
        );
        recheck_path(&receipt.seed_image, &seed_meta)?;
        recheck_path(&receipt.leaf_image, &leaf_meta)?;
        superblock_identity(&self.seed_file, receipt.seed_fsid, true)?;
        superblock_identity(&self.leaf_file, receipt.sprout_fsid, false)?;
        ensure!(
            sha256_file(&self.seed_file, seed_meta.len())?
                .eq_ignore_ascii_case(&receipt.seed_sha256),
            "provisioned seed digest changed"
        );
        let seed = get_loop(&self.seed_file, &seed_meta, true, false)?;
        let leaf = get_loop(&self.leaf_file, &leaf_meta, false, false)?;
        ensure!(
            seed.path == receipt.seed_loop && leaf.path == receipt.leaf_loop,
            "provisioned loop mapping changed"
        );
        let info = mounted_fs_info(&self.mount_file, receipt.sprout_fsid)?;
        let backings = Backings {
            seed: self.seed_file.try_clone()?,
            leaf: self.leaf_file.try_clone()?,
            leaf_lock: self.leaf_lock.try_clone()?,
            seed_meta,
            leaf_meta,
        };
        let descriptor = SeededRootDescriptor {
            seed_image: &receipt.seed_image,
            seed_sha256: &receipt.seed_sha256,
            seed_fsid: receipt.seed_fsid,
            leaf_image: &receipt.leaf_image,
            sprout_fsid: receipt.sprout_fsid,
            mount_point: &receipt.mount_point,
            pool_subvolume: "pool",
        };
        verify_members(
            &self.mount_file,
            &info,
            &backings,
            &seed,
            &leaf,
            &descriptor,
        )?;
        Ok(())
    }

    /// Share this held leaf lock with the standard prepared-root verifier.
    /// Duplicated FDs retain the same open file description, so this does not
    /// contend with the provisioning guard or allocate another loop/mount.
    pub fn prepared_root_guard(&self, pool_subvolume: &str) -> Result<SeededRootMount> {
        self.revalidate()?;
        let receipt = &self.receipt;
        let descriptor = SeededRootDescriptor {
            seed_image: &receipt.seed_image,
            seed_sha256: &receipt.seed_sha256,
            seed_fsid: receipt.seed_fsid,
            leaf_image: &receipt.leaf_image,
            sprout_fsid: receipt.sprout_fsid,
            mount_point: &receipt.mount_point,
            pool_subvolume,
        };
        let backings = Backings {
            seed: self.seed_file.try_clone()?,
            leaf: self.leaf_file.try_clone()?,
            leaf_lock: self.leaf_lock.try_clone()?,
            seed_meta: self.seed_file.metadata()?,
            leaf_meta: self.leaf_file.metadata()?,
        };
        let seed = get_loop(&backings.seed, &backings.seed_meta, true, false)?;
        let leaf = get_loop(&backings.leaf, &backings.leaf_meta, false, false)?;
        let (mount_file, pool_path) = verify_mount(&descriptor, &backings, &seed, &leaf)?;
        ensure!(
            mount_id_fd(&mount_file)? == mount_id_fd(&self.mount_file)?,
            "prepared pool is not on the held provision mount"
        );
        Ok(finish_guard(
            &descriptor,
            backings,
            seed,
            leaf,
            mount_file,
            pool_path,
            false,
        ))
    }

    /// Append an owner-visible durable stage after external Incus/Cozo effects.
    pub fn journal_owner_stage(&mut self, stage: &str, data: serde_json::Value) -> Result<()> {
        ensure!(
            !stage.is_empty() && stage.bytes().all(|b| b.is_ascii_uppercase() || b == b'_'),
            "invalid provision stage"
        );
        self.revalidate()?;
        provision_journal(&mut self.journal_file, stage, data)
    }

    /// Confirm the core pool was sealed into the seed, then create only a new
    /// empty sibling `persist` subvolume on this already verified sprout. The
    /// CREATE_STARTED record makes an interrupted ioctl a reconciliation case.
    pub fn prepare_persist_subvolume(&mut self, pool_subvolume: &str) -> Result<PathBuf> {
        self.revalidate()?;
        ensure!(
            pool_subvolume != "persist"
                && !pool_subvolume.is_empty()
                && pool_subvolume != "."
                && pool_subvolume != ".."
                && !pool_subvolume.contains('/')
                && !pool_subvolume.contains('\0'),
            "invalid sealed core pool name"
        );
        let prepared = SeededRootDescriptor {
            seed_image: &self.receipt.seed_image,
            seed_sha256: &self.receipt.seed_sha256,
            seed_fsid: self.receipt.seed_fsid,
            leaf_image: &self.receipt.leaf_image,
            sprout_fsid: self.receipt.sprout_fsid,
            mount_point: &self.receipt.mount_point,
            pool_subvolume,
        };
        verify_pool_subvolume(&prepared, mount_id_fd(&self.mount_file)?)?;
        let persist = self.receipt.mount_point.join("persist");
        match fs::symlink_metadata(&persist) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Ok(_) => bail!("persist already exists; provision cannot reuse it"),
            Err(error) => return Err(error).context("inspect new persist target"),
        }
        provision_journal(
            &mut self.journal_file,
            "CREATE_PERSIST_STARTED",
            serde_json::json!({"pool_subvolume": pool_subvolume}),
        )?;
        let mut args = VolArgs {
            fd: 0,
            name: [0u8; 4088],
        };
        args.name[..7].copy_from_slice(b"persist");
        // SAFETY: fixed kernel ABI args contain a NUL-terminated direct name;
        // the FD pins the verified writable sprout root.
        if unsafe {
            libc::ioctl(
                self.mount_file.as_raw_fd(),
                BTRFS_IOC_SUBVOL_CREATE,
                &mut args,
            )
        } != 0
        {
            return Err(io::Error::last_os_error()).context("create new persist subvolume");
        }
        if unsafe { libc::syncfs(self.mount_file.as_raw_fd()) } != 0 {
            return Err(io::Error::last_os_error()).context("sync new persist subvolume");
        }
        self.revalidate()?;
        ensure!(
            fs::canonicalize(&persist)? == persist,
            "persist path is not canonical"
        );
        let meta = fs::symlink_metadata(&persist)?;
        ensure!(
            meta.is_dir() && meta.ino() == 256 && meta.uid() == 0 && meta.mode() & 0o022 == 0,
            "new persist is not a secure subvolume root"
        );
        ensure!(
            mount_id(&persist)? == mount_id_fd(&self.mount_file)?,
            "new persist is covered by another mount"
        );
        let child = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
            .open(&persist)?;
        mounted_fs_info(&child, self.receipt.sprout_fsid)?;
        let (treeid, parent_id, name) = subvolume_identity(&child)?;
        ensure!(
            treeid > 5 && parent_id == 5 && name == b"persist",
            "new persist is not a direct writable-root child"
        );
        ensure!(
            fs::read_dir(&persist)?.next().is_none(),
            "new persist is not empty"
        );
        provision_journal(
            &mut self.journal_file,
            "PERSIST_READY",
            serde_json::json!({"sprout_fsid": self.receipt.sprout_fsid,
                "persist_treeid": treeid}),
        )?;
        Ok(persist)
    }
}

/// Compose one new writable leaf with a pinned, currently unused RO seed.
///
/// This deliberately refuses an existing leaf, including a standalone
/// `fstorage.img`. The append-only journal and new image remain if any later
/// step fails; once ADD_STARTED is durable, callers must reconcile before
/// considering another attempt. No file is formatted, overwritten or removed.
pub fn provision_seeded_leaf(
    descriptor: &SeededProvisionDescriptor<'_>,
) -> Result<SeededProvisionGuard> {
    ensure!(
        descriptor.leaf_size_bytes >= 1024 * 1024 * 1024
            && descriptor.leaf_size_bytes <= 1024 * 1024 * 1024 * 1024
            && descriptor.leaf_size_bytes.is_multiple_of(4096),
        "new leaf size must be aligned and between 1 GiB and 1 TiB"
    );
    ensure!(
        descriptor.new_leaf_image != descriptor.seed_image,
        "new leaf must differ from the seed"
    );
    ensure!(
        descriptor.seed_sha256.len() == 64
            && descriptor
                .seed_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit()),
        "invalid pinned seed digest"
    );
    ensure!(
        descriptor.image_fingerprint.len() == 64
            && descriptor
                .image_fingerprint
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "invalid pinned core image fingerprint"
    );
    ensure!(
        !exact_mount_present(descriptor.mount_point)?,
        "provision target is already mounted"
    );
    private_empty_target(descriptor.mount_point)?;
    let (seed_file, seed_meta) = root_owned_regular(descriptor.seed_image, false)?;
    superblock_identity(&seed_file, descriptor.seed_fsid, true)?;
    ensure!(
        sha256_file(&seed_file, seed_meta.len())?.eq_ignore_ascii_case(descriptor.seed_sha256),
        "pinned seed digest mismatch"
    );
    // On kernels >=6.17 the seed cannot be independently mounted while any
    // sprout already uses it. Conservatively reject even an idle mapping.
    ensure!(
        existing_loop(&seed_meta, true)?.is_none(),
        "seed is already mapped; existing use must be reconciled first"
    );
    ensure!(
        descriptor.new_leaf_image.is_absolute(),
        "new leaf path must be absolute"
    );
    secure_ancestors(descriptor.new_leaf_image)?;
    let parent = descriptor
        .new_leaf_image
        .parent()
        .context("new leaf has no parent")?;
    ensure!(
        fs::canonicalize(parent)? == parent,
        "new leaf parent is not canonical"
    );
    let parent_meta = fs::symlink_metadata(parent)?;
    ensure!(
        parent_meta.is_dir() && parent_meta.uid() == 0 && parent_meta.mode() & 0o022 == 0,
        "new leaf parent is not secure"
    );
    ensure!(
        fs::symlink_metadata(descriptor.new_leaf_image).is_err(),
        "new leaf path already exists"
    );
    // Reject contaminated or wrong core before even an exclusive journal or
    // blank leaf exists. Repeat against the selected seed before device add.
    let seed = preflight_clean_seed(descriptor, &seed_file, &seed_meta)?;
    let journal_result = (|| -> Result<(PathBuf, File)> {
        private_empty_target(descriptor.mount_point)?;
        let journal_path = descriptor.new_leaf_image.with_extension("provision.jsonl");
        ensure!(
            journal_path != descriptor.seed_image,
            "journal path collides with seed"
        );
        let mut journal = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&journal_path)
            .context("create exclusive provision journal")?;
        provision_journal(
            &mut journal,
            "INTENT",
            serde_json::json!({
                "seed_image": descriptor.seed_image, "seed_sha256": descriptor.seed_sha256,
                "seed_fsid": descriptor.seed_fsid, "image_fingerprint": descriptor.image_fingerprint,
                "leaf_image": descriptor.new_leaf_image,
                "leaf_size_bytes": descriptor.leaf_size_bytes,
                "mount_point": descriptor.mount_point,
            }),
        )?;
        Ok((journal_path, journal))
    })();
    let (journal_path, mut journal) = match journal_result {
        Ok(prepared) => prepared,
        Err(error) => {
            // No durable native INTENT exists yet. Do not leak an untracked
            // mapping on a validation or journal-open/write failure.
            detach_created(&seed).context("detach seed before provision intent")?;
            return Err(error);
        }
    };
    let leaf_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(descriptor.new_leaf_image)
        .context("create new blank leaf exclusively")?;
    leaf_file
        .set_len(descriptor.leaf_size_bytes)
        .context("size new blank leaf")?;
    leaf_file.sync_all().context("sync new blank leaf")?;
    File::open(parent)?
        .sync_all()
        .context("sync new leaf directory")?;
    let leaf_meta = leaf_file.metadata()?;
    ensure!(
        leaf_meta.uid() == 0
            && leaf_meta.mode() & 0o077 == 0
            && leaf_meta.len() == descriptor.leaf_size_bytes,
        "new blank leaf has unexpected metadata"
    );
    provision_journal(
        &mut journal,
        "BLANK_LEAF_CREATED",
        serde_json::json!({
            "leaf_inode": leaf_meta.ino(), "leaf_device": leaf_meta.dev(),
        }),
    )?;
    let leaf_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(descriptor.new_leaf_image)?;
    ensure!(
        leaf_lock.metadata()?.ino() == leaf_meta.ino(),
        "new leaf lock changed"
    );
    if unsafe { libc::flock(leaf_lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::last_os_error()).context("new leaf is busy");
    }
    let leaf = allocate_loop(&leaf_file, &leaf_meta, false)?;
    provision_journal(
        &mut journal,
        "LOOPS_READY",
        serde_json::json!({
            "seed_loop": seed.path, "leaf_loop": leaf.path,
        }),
    )?;
    recheck_path(descriptor.seed_image, &seed_meta)?;
    recheck_path(descriptor.new_leaf_image, &leaf_meta)?;
    ensure!(
        existing_loop(&seed_meta, true)?.is_some(),
        "new seed mapping disappeared"
    );
    let source = c_path(&seed.path)?;
    let target = c_path(descriptor.mount_point)?;
    let fs_type = CString::new("btrfs")?;
    let options = CString::new(format!("ro,device={}", seed.path.display()))?;
    if unsafe {
        libc::mount(
            source.as_ptr(),
            target.as_ptr(),
            fs_type.as_ptr(),
            libc::MS_RDONLY,
            options.as_ptr().cast(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error()).context("mount pinned seed read-only");
    }
    let mounted = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(descriptor.mount_point)
        .context("pin seed mount")?;
    let seed_mount_id = mount_id_fd(&mounted)?;
    ensure!(
        seed_mount_id == mount_id(descriptor.mount_point)?
            && seed_mount_id
                != mount_id(
                    descriptor
                        .mount_point
                        .parent()
                        .context("seed mount parent")?
                )?,
        "seed mount boundary changed"
    );
    seed_only_mount(&mounted, &seed, &seed_meta, descriptor.seed_fsid)?;
    verify_clean_core_layout(
        descriptor.mount_point,
        seed_mount_id,
        descriptor.image_fingerprint,
        false,
    )?;
    provision_journal(
        &mut journal,
        "SEED_MOUNT_VERIFIED",
        serde_json::json!({
            "seed_mount_id": seed_mount_id,
        }),
    )?;
    validate_loop(&loop_status(&seed.file)?, &seed_meta, true)?;
    validate_loop(&loop_status(&leaf.file)?, &leaf_meta, false)?;
    provision_journal(
        &mut journal,
        "ADD_STARTED",
        serde_json::json!({
            "leaf_loop": leaf.path,
        }),
    )?;
    // This ioctl is the only destructive kernel operation, and its target is
    // the blank leaf created O_EXCL above, never a caller-supplied old image.
    crate::btrfs::device_add(&leaf.path, descriptor.mount_point)?;
    provision_journal(&mut journal, "ADD_RETURNED", serde_json::json!({}))?;
    if unsafe { libc::syncfs(mounted.as_raw_fd()) } != 0 {
        return Err(io::Error::last_os_error()).context("sync newly sprouted filesystem");
    }
    let sprout_info: FsInfo = {
        let mut info: FsInfo = unsafe { std::mem::zeroed() };
        if unsafe { libc::ioctl(mounted.as_raw_fd(), BTRFS_IOC_FS_INFO, &mut info) } != 0 {
            return Err(io::Error::last_os_error()).context("query new sprout FSID");
        }
        info
    };
    let sprout_fsid = Uuid::from_bytes(sprout_info.fsid);
    ensure!(
        !sprout_fsid.is_nil() && sprout_fsid != descriptor.seed_fsid,
        "new sprout has invalid filesystem identity"
    );
    provision_journal(
        &mut journal,
        "SPROUT_FSID_OBSERVED",
        serde_json::json!({
            "sprout_fsid": sprout_fsid,
        }),
    )?;
    cleanup_created_mount(descriptor.mount_point, mounted, seed_mount_id)?;
    provision_journal(&mut journal, "SEED_UNMOUNTED", serde_json::json!({}))?;
    private_empty_target(descriptor.mount_point)?;
    let leaf_source = c_path(&leaf.path)?;
    let options = CString::new(format!(
        "rw,device={},device={}",
        seed.path.display(),
        leaf.path.display()
    ))?;
    if unsafe {
        libc::mount(
            leaf_source.as_ptr(),
            target.as_ptr(),
            fs_type.as_ptr(),
            0,
            options.as_ptr().cast(),
        )
    } != 0
    {
        return Err(io::Error::last_os_error()).context("mount new writable sprout");
    }
    let mount_file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(descriptor.mount_point)
        .context("pin new writable sprout mount")?;
    let leaf_fsid = provision_leaf_fsid(&leaf_file)?;
    ensure!(leaf_fsid == sprout_fsid, "new leaf superblock FSID changed");
    let backings = Backings {
        seed: seed_file.try_clone()?,
        leaf: leaf_file.try_clone()?,
        leaf_lock: leaf_lock.try_clone()?,
        seed_meta,
        leaf_meta,
    };
    let prepared = SeededRootDescriptor {
        seed_image: descriptor.seed_image,
        seed_sha256: descriptor.seed_sha256,
        seed_fsid: descriptor.seed_fsid,
        leaf_image: descriptor.new_leaf_image,
        sprout_fsid,
        mount_point: descriptor.mount_point,
        pool_subvolume: "pool",
    };
    let info = mounted_fs_info(&mount_file, sprout_fsid)?;
    verify_members(&mount_file, &info, &backings, &seed, &leaf, &prepared)?;
    ensure!(
        mount_id_fd(&mount_file)? == mount_id(descriptor.mount_point)?,
        "new writable mount changed"
    );
    let receipt = SeededProvisionReceipt {
        seed_image: descriptor.seed_image.to_path_buf(),
        seed_sha256: descriptor.seed_sha256.to_owned(),
        seed_fsid: descriptor.seed_fsid,
        leaf_image: descriptor.new_leaf_image.to_path_buf(),
        sprout_fsid,
        seed_loop: seed.path,
        leaf_loop: leaf.path,
        mount_point: descriptor.mount_point.to_path_buf(),
        journal_path,
    };
    provision_journal(
        &mut journal,
        "READY",
        serde_json::json!({
            "sprout_fsid": sprout_fsid,
        }),
    )?;
    let guard = SeededProvisionGuard {
        receipt,
        seed_file,
        leaf_file,
        leaf_lock,
        mount_file,
        journal_file: journal,
    };
    guard.revalidate()?;
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_status_requires_full_mapping_and_exact_mode() {
        let image = tempfile::NamedTempFile::new().unwrap();
        let meta = image.as_file().metadata().unwrap();
        let mut status: LoopInfo64 = unsafe { std::mem::zeroed() };
        status.lo_device = meta.dev();
        status.lo_inode = meta.ino();
        status.lo_rdevice = meta.rdev();
        assert!(validate_loop(&status, &meta, false).is_ok());
        status.lo_flags = LO_FLAGS_READ_ONLY;
        assert!(validate_loop(&status, &meta, true).is_ok());
        assert!(validate_loop(&status, &meta, false).is_err());
        status.lo_flags = LO_FLAGS_AUTOCLEAR;
        assert!(validate_loop(&status, &meta, false).is_err());
        status.lo_flags = 0;
        status.lo_offset = 4096;
        assert!(validate_loop(&status, &meta, false).is_err());
        status.lo_offset = 0;
        status.lo_sizelimit = 4096;
        assert!(validate_loop(&status, &meta, false).is_err());
        status.lo_sizelimit = 0;
        status.lo_inode += 1;
        assert!(validate_loop(&status, &meta, false).is_err());
    }
}
