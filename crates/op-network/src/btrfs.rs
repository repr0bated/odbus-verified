//! Native btrfs device operations via ioctl — no CLI subprocesses.
//!
//! An identity device joins and leaves an already-mounted filesystem with
//! `BTRFS_IOC_ADD_DEV` and `BTRFS_IOC_RM_DEV`. That is not a mount or unmount
//! of the identity image.

use std::ffi::CString;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;

use anyhow::{Context, Result, bail};

/// `BTRFS_PATH_NAME_MAX` from `linux/btrfs.h`.
const BTRFS_PATH_NAME_MAX: usize = 4087;

/// `struct btrfs_ioctl_vol_args` from `linux/btrfs.h`.
#[repr(C)]
struct BtrfsIoctlVolArgs {
    fd: i64,
    name: [u8; BTRFS_PATH_NAME_MAX + 1],
}

// _IOW(BTRFS_IOCTL_MAGIC=0x94, 10, struct btrfs_ioctl_vol_args)
// = 0x40000000 | (sizeof(vol_args)=4096 << 16) | (0x94 << 8) | 10
const BTRFS_IOC_ADD_DEV: libc::c_ulong = 0x5000_940a;
// _IOW(BTRFS_IOCTL_MAGIC, 11, struct btrfs_ioctl_vol_args)
const BTRFS_IOC_RM_DEV: libc::c_ulong = 0x5000_940b;
const _: () = assert!(std::mem::size_of::<BtrfsIoctlVolArgs>() == 4096);

/// Attach `device` to the mounted btrfs filesystem at `mount_point`
/// (equivalent to `btrfs device add <device> <mount_point>`, but issued
/// natively). The caller is responsible for the device being a real block
/// device (or loop-backed image) it intends to hand to this filesystem —
/// the kernel wipes and claims it.
pub fn device_add(device: &Path, mount_point: &Path) -> Result<()> {
    vol_args_ioctl(BTRFS_IOC_ADD_DEV, "BTRFS_IOC_ADD_DEV", device, mount_point)
}

/// Remove `device` from the mounted btrfs filesystem at `mount_point`
/// (equivalent to `btrfs device remove <device> <mount_point>`). `device`
/// is the kernel member path, usually the loop device btrfs already has.
pub fn device_remove(device: &Path, mount_point: &Path) -> Result<()> {
    vol_args_ioctl(BTRFS_IOC_RM_DEV, "BTRFS_IOC_RM_DEV", device, mount_point)
}

/// Add the registered backing to `mount_point` when it is not already a member.
pub fn ensure_device_added(device: &Path, mount_point: &Path) -> Result<()> {
    if member_kernel_path(device, mount_point)?.is_some() {
        return Ok(());
    }
    let block = backing_block_path(device)?;
    if let Err(error) = device_add(&block, mount_point) {
        if member_kernel_path(device, mount_point)?.is_some() {
            return Ok(());
        }
        return Err(error);
    }
    Ok(())
}

/// Remove the registered backing from `mount_point` when it is still a member.
/// A device that is already absent is left alone.
pub fn ensure_device_removed(device: &Path, mount_point: &Path) -> Result<()> {
    let Some(kernel_path) = member_kernel_path(device, mount_point)? else {
        return Ok(());
    };
    device_remove(Path::new(&kernel_path), mount_point)
}

fn vol_args_ioctl(
    ioctl: libc::c_ulong,
    name: &str,
    device: &Path,
    mount_point: &Path,
) -> Result<()> {
    let dev_str = device.to_str().context("device path is not valid UTF-8")?;
    if dev_str.len() > BTRFS_PATH_NAME_MAX {
        bail!("device path exceeds BTRFS_PATH_NAME_MAX");
    }
    let dev_c = CString::new(dev_str).context("device path contains NUL")?;
    let dir = File::open(mount_point)
        .with_context(|| format!("open btrfs mount point {}", mount_point.display()))?;
    let mut args = BtrfsIoctlVolArgs {
        fd: 0,
        name: [0u8; BTRFS_PATH_NAME_MAX + 1],
    };
    let bytes = dev_c.as_bytes_with_nul();
    args.name[..bytes.len()].copy_from_slice(bytes);
    // SAFETY: `dir` is a valid open fd and `args` is a NUL-terminated vol_args.
    let rc = unsafe { libc::ioctl(dir.as_raw_fd(), ioctl, &mut args) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        bail!(
            "{name}({} -> {}) failed: {err}",
            device.display(),
            mount_point.display()
        );
    }
    Ok(())
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

/// `struct loop_info64` from `linux/loop.h`. The kernel records the opened
/// backing file's device and inode, which is stronger than its display path.
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

/// `LOOP_GET_STATUS64` and `LO_FLAGS_READ_ONLY` from `linux/loop.h`.
const LOOP_GET_STATUS64: libc::c_ulong = 0x4c05;
const LO_FLAGS_READ_ONLY: u32 = 1;

const _: () = assert!(std::mem::size_of::<FsInfo>() == 1024);
const _: () = assert!(std::mem::size_of::<DevInfo>() == 4096);
const _: () = assert!(std::mem::size_of::<LoopInfo64>() == 232);

fn member_kernel_path(device: &Path, mount_point: &Path) -> Result<Option<String>> {
    let dir = match File::open(mount_point) {
        Ok(dir) => dir,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).context("open btrfs mount while looking up an identity device");
        }
    };
    let backing = File::open(device).context("open registered identity btrfs backing")?;
    let backing_meta = backing.metadata()?;
    // SAFETY: FsInfo is an integer-only kernel ABI struct.
    let mut fs: FsInfo = unsafe { std::mem::zeroed() };
    if unsafe { libc::ioctl(dir.as_raw_fd(), 0x8400_941f as libc::c_ulong, &mut fs) } != 0 {
        return Err(std::io::Error::last_os_error()).context("query btrfs filesystem devices");
    }
    for devid in 1..=fs.max_id {
        // SAFETY: DevInfo is an integer-only kernel ABI struct.
        let mut dev: DevInfo = unsafe { std::mem::zeroed() };
        dev.devid = devid;
        if unsafe { libc::ioctl(dir.as_raw_fd(), 0xd000_941e as libc::c_ulong, &mut dev) } != 0 {
            continue;
        }
        let Some(end) = dev.path.iter().position(|byte| *byte == 0) else {
            continue;
        };
        let Ok(actual) = std::str::from_utf8(&dev.path[..end]) else {
            continue;
        };
        if actual.is_empty() {
            continue;
        }
        if kernel_member_matches(actual, &backing_meta)? {
            return Ok(Some(actual.to_string()));
        }
    }
    Ok(None)
}

fn kernel_member_matches(kernel_path: &str, backing_meta: &std::fs::Metadata) -> Result<bool> {
    let actual_device = match File::open(kernel_path) {
        Ok(file) => file,
        Err(_) => return Ok(false),
    };
    let actual_meta = actual_device.metadata()?;
    if !actual_meta.file_type().is_block_device() {
        return Ok(false);
    }
    if backing_meta.is_file() {
        // SAFETY: loop_info64 is a fixed-size integer kernel ABI struct.
        let mut loop_status: LoopInfo64 = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::ioctl(
                actual_device.as_raw_fd(),
                LOOP_GET_STATUS64,
                &mut loop_status,
            )
        } != 0
        {
            return Ok(false);
        }
        return Ok(loop_status_matches_backing(&loop_status, backing_meta));
    }
    if backing_meta.file_type().is_block_device() {
        return Ok(backing_meta.rdev() == actual_meta.rdev());
    }
    Ok(false)
}

fn backing_block_path(device: &Path) -> Result<std::path::PathBuf> {
    let meta = std::fs::metadata(device)
        .with_context(|| format!("inspect identity btrfs backing {}", device.display()))?;
    if meta.file_type().is_block_device() {
        return Ok(device.to_path_buf());
    }
    anyhow::ensure!(
        meta.is_file(),
        "identity btrfs backing is neither a block device nor an image file"
    );
    for entry in std::fs::read_dir("/dev").context("list loop devices")? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(digits) = name.strip_prefix("loop") else {
            continue;
        };
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let file = match File::options().read(true).open(entry.path()) {
            Ok(file) => file,
            Err(_) => continue,
        };
        if !file.metadata()?.file_type().is_block_device() {
            continue;
        }
        // SAFETY: loop_info64 is a fixed-size integer kernel ABI struct.
        let mut status: LoopInfo64 = unsafe { std::mem::zeroed() };
        if unsafe { libc::ioctl(file.as_raw_fd(), LOOP_GET_STATUS64, &mut status) } != 0 {
            continue;
        }
        if loop_status_matches_backing(&status, &meta) {
            return Ok(entry.path());
        }
    }
    bail!(
        "identity image {} has no loop device to add into the mounted filesystem",
        device.display()
    )
}

fn loop_status_matches_backing(loop_status: &LoopInfo64, backing: &std::fs::Metadata) -> bool {
    backing.is_file()
        && loop_status.lo_device == backing.dev()
        && loop_status.lo_inode == backing.ino()
        && loop_status.lo_rdevice == backing.rdev()
        && loop_status.lo_offset == 0
        && loop_status.lo_sizelimit == 0
        && loop_status.lo_flags & LO_FLAGS_READ_ONLY == 0
}

/// Verify the registered image or block device is the kernel's sole writable
/// btrfs device, either standalone or as the leaf of a seed/sprout filesystem.
/// This does not authenticate the seed: a seeded identity also needs explicit
/// core-seed provenance before it can be selected as an Incus root. A direct
/// block path likewise needs owner-side proof that it is not a shared pool.
/// Read-only ioctls; no format, pool membership change, or mount occurs.
pub fn verify_dedicated_mount(
    device: &Path,
    mount_point: &Path,
    expected_uuid: &str,
) -> Result<()> {
    let expected = uuid::Uuid::parse_str(expected_uuid).context("invalid registered btrfs UUID")?;
    let dir = File::open(mount_point).context("open registered btrfs mount")?;
    // SAFETY: these kernel ABI structs contain only integer fields/arrays.
    let mut fs: FsInfo = unsafe { std::mem::zeroed() };
    // _IOR(0x94, 31, struct btrfs_ioctl_fs_info_args), linux/btrfs.h.
    if unsafe { libc::ioctl(dir.as_raw_fd(), 0x8400_941f as libc::c_ulong, &mut fs) } != 0 {
        return Err(std::io::Error::last_os_error()).context("query btrfs filesystem identity");
    }
    if fs.fsid != *expected.as_bytes() || fs.num_devices != 1 {
        bail!("mount is not the registered single-writable-device btrfs filesystem");
    }
    // SAFETY: same integer-only ABI guarantee; max_id is kernel-provided.
    let mut dev: DevInfo = unsafe { std::mem::zeroed() };
    dev.devid = fs.max_id;
    // _IOWR(0x94, 30, struct btrfs_ioctl_dev_info_args), linux/btrfs.h.
    if unsafe { libc::ioctl(dir.as_raw_fd(), 0xd000_941e as libc::c_ulong, &mut dev) } != 0 {
        return Err(std::io::Error::last_os_error()).context("query btrfs backing device");
    }
    let end = dev
        .path
        .iter()
        .position(|byte| *byte == 0)
        .context("unterminated kernel device path")?;
    let actual = std::str::from_utf8(&dev.path[..end]).context("invalid kernel device path")?;
    let backing = File::open(device).context("open registered btrfs backing")?;
    let backing_meta = backing.metadata()?;
    let actual_device = File::open(actual).context("open mounted btrfs block device")?;
    let actual_meta = actual_device.metadata()?;
    anyhow::ensure!(
        actual_meta.file_type().is_block_device(),
        "mounted btrfs member is not a block device"
    );
    if backing_meta.is_file() {
        // SAFETY: loop_info64 is a fixed-size kernel ABI struct of integer fields.
        let mut loop_status: LoopInfo64 = unsafe { std::mem::zeroed() };
        if unsafe {
            libc::ioctl(
                actual_device.as_raw_fd(),
                LOOP_GET_STATUS64,
                &mut loop_status,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error())
                .context("mounted image member is not a loop device");
        }
        if !loop_status_matches_backing(&loop_status, &backing_meta) {
            bail!("mounted btrfs loop device does not match the registered writable image");
        }
    } else if backing_meta.file_type().is_block_device() {
        anyhow::ensure!(
            backing_meta.rdev() == actual_meta.rdev(),
            "mounted btrfs block device differs from the registered block device"
        );
    } else {
        bail!("registered btrfs backing is neither a regular image nor a block device");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;
    #[test]
    fn dedicated_mount_verification_rejects_invalid_uuid_before_io() {
        assert!(
            verify_dedicated_mount(Path::new("/missing"), Path::new("/missing"), "invalid")
                .unwrap_err()
                .to_string()
                .contains("UUID")
        );
    }

    #[test]
    fn loop_backing_identity_rejects_wrong_inode_readonly_and_partial_mapping() {
        let image = tempfile::NamedTempFile::new().unwrap();
        let meta = image.as_file().metadata().unwrap();
        let mut status: LoopInfo64 = unsafe { std::mem::zeroed() };
        status.lo_device = meta.dev();
        status.lo_inode = meta.ino();
        status.lo_rdevice = meta.rdev();
        assert!(loop_status_matches_backing(&status, &meta));
        status.lo_device += 1;
        assert!(!loop_status_matches_backing(&status, &meta));
        status.lo_device = meta.dev();
        status.lo_inode += 1;
        assert!(!loop_status_matches_backing(&status, &meta));
        status.lo_inode = meta.ino();
        status.lo_rdevice += 1;
        assert!(!loop_status_matches_backing(&status, &meta));
        status.lo_rdevice = meta.rdev();
        status.lo_flags = LO_FLAGS_READ_ONLY;
        assert!(!loop_status_matches_backing(&status, &meta));
        status.lo_flags = 0;
        status.lo_offset = 4096;
        assert!(!loop_status_matches_backing(&status, &meta));
        status.lo_offset = 0;
        status.lo_sizelimit = 4096;
        assert!(!loop_status_matches_backing(&status, &meta));
    }
}
