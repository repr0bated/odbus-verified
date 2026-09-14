//! Native btrfs device operations via ioctl — no CLI subprocesses.
//!
//! Identity persistence uses a dedicated, already-mounted filesystem. Binding
//! it to a sled must never add/wipe a device into a shared storage pool.

use std::ffi::CString;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::Path;

use anyhow::{bail, Context, Result};

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
const _: () = assert!(std::mem::size_of::<BtrfsIoctlVolArgs>() == 4096);

/// Attach `device` to the mounted btrfs filesystem at `mount_point`
/// (equivalent to `btrfs device add <device> <mount_point>`, but issued
/// natively). The caller is responsible for the device being a real block
/// device (or loop-backed image) it intends to hand to this filesystem —
/// the kernel wipes and claims it.
pub fn device_add(device: &Path, mount_point: &Path) -> Result<()> {
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

    // SAFETY: `dir` is a valid open fd and `args` is a properly initialized,
    // NUL-terminated btrfs_ioctl_vol_args matching the kernel ABI.
    let rc = unsafe { libc::ioctl(dir.as_raw_fd(), BTRFS_IOC_ADD_DEV, &mut args) };
    if rc != 0 {
        let err = std::io::Error::last_os_error();
        bail!(
            "BTRFS_IOC_ADD_DEV({} -> {}) failed: {err}",
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

const _: () = assert!(std::mem::size_of::<FsInfo>() == 1024);
const _: () = assert!(std::mem::size_of::<DevInfo>() == 4096);

/// Verify a dedicated mount by kernel FSID and backing device. Read-only ioctls;
/// no format, pool membership change, mount, or filesystem mutation occurs.
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
        bail!("mount is not the registered dedicated single-device btrfs filesystem");
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
    if std::fs::canonicalize(device)? != std::fs::canonicalize(actual)? {
        bail!("mounted btrfs backing device differs from the registered device");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dedicated_mount_verification_rejects_invalid_uuid_before_io() {
        assert!(
            verify_dedicated_mount(Path::new("/missing"), Path::new("/missing"), "invalid")
                .unwrap_err()
                .to_string()
                .contains("UUID")
        );
    }
}
