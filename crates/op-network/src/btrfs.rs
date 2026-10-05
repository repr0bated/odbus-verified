//! Native btrfs device operations via ioctl — no CLI subprocesses.
//!
//! Explicit storage provisioning may change membership. Identity startup and
//! shutdown only verify mounts: an absent mount must never turn into a device
//! addition to the host filesystem beneath its directory.

use std::ffi::CString;
use std::fs::File;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

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

fn uuid_device_path(expected_uuid: &str) -> Result<PathBuf> {
    let uuid = uuid::Uuid::parse_str(expected_uuid).context("invalid registered btrfs UUID")?;
    anyhow::ensure!(!uuid.is_nil(), "registered btrfs UUID must not be nil");
    Ok(Path::new("/dev/disk/by-uuid").join(uuid.hyphenated().to_string()))
}

fn mount_id(file: &File) -> Result<u64> {
    // SAFETY: statx writes to an initialized integer-only ABI struct; the
    // empty path and AT_EMPTY_PATH query the held directory descriptor.
    let mut info: libc::statx = unsafe { std::mem::zeroed() };
    let rc = unsafe {
        libc::statx(
            file.as_raw_fd(),
            c"".as_ptr(),
            libc::AT_EMPTY_PATH,
            libc::STATX_MNT_ID,
            &mut info,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error()).context("read identity mount boundary");
    }
    anyhow::ensure!(
        info.stx_mask & libc::STATX_MNT_ID != 0,
        "kernel lacks mount ID readback"
    );
    Ok(info.stx_mnt_id)
}

/// Observe an exact mount boundary; a directory on its parent's filesystem is
/// not a mounted identity. Noncanonical and absent paths are errors.
pub fn identity_mount_present(mount_point: &Path) -> Result<bool> {
    anyhow::ensure!(
        mount_point.is_absolute() && mount_point != Path::new("/"),
        "identity mount must be an absolute non-root mount point"
    );
    anyhow::ensure!(
        std::fs::canonicalize(mount_point).context("resolve identity mount")? == mount_point,
        "identity mount path must be canonical and contain no symlinks"
    );
    let dir = File::options()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(mount_point)
        .context("open identity mount directory")?;
    let parent = File::open(
        mount_point
            .parent()
            .context("identity mount has no parent")?,
    )?;
    Ok(mount_id(&dir)? != mount_id(&parent)?)
}

fn verify_exact_mount(mount_point: &Path) -> Result<()> {
    anyhow::ensure!(
        identity_mount_present(mount_point)?,
        "registered identity mount is absent; refusing the containing filesystem"
    );
    Ok(())
}

fn verify_superblock_identity(superblock: &[u8; 4096], expected_uuid: &str) -> Result<()> {
    let expected = uuid::Uuid::parse_str(expected_uuid).context("invalid registered btrfs UUID")?;
    anyhow::ensure!(!expected.is_nil(), "registered btrfs UUID must not be nil");
    anyhow::ensure!(
        &superblock[64..72] == b"_BHRfS_M",
        "UUID source is not Btrfs"
    );
    anyhow::ensure!(
        u64::from_le_bytes(superblock[48..56].try_into()?) == 65536,
        "invalid primary Btrfs superblock offset"
    );
    anyhow::ensure!(
        &superblock[32..48] == expected.as_bytes(),
        "on-disk Btrfs UUID differs from the registered UUID; refusing cached device identity"
    );
    let members = u64::from_le_bytes(superblock[136..144].try_into()?);
    anyhow::ensure!(
        members == 1,
        "UUID source belongs to a multi-device filesystem"
    );
    Ok(())
}

/// Read the primary superblock through a held read-only block descriptor before
/// asking UDisks to mount. Its cached IdUUID alone is not sufficient after a
/// format or device-add operation. The kernel validates the complete filesystem
/// when mounting; this preflight rejects stale identity and shared-pool sources.
pub fn verify_unmounted_source(source: &File, expected_uuid: &str) -> Result<()> {
    anyhow::ensure!(
        source.metadata()?.file_type().is_block_device(),
        "UUID source must be a block device"
    );
    let mut superblock = [0_u8; 4096];
    source
        .read_exact_at(&mut superblock, 65536)
        .context("read Btrfs source superblock")?;
    verify_superblock_identity(&superblock, expected_uuid)
}

/// Verify an identity's existing mount using its registered filesystem UUID.
/// Block device names are discovery hints only: `/dev/sdX` may change on boot.
/// Image-backed sprouts additionally retain their inode/loop provenance check.
/// The storage owner must mount the declared subvolume first; this binding has
/// no subvolume selector, so guessing the filesystem default would be unsafe.
/// This function never mounts, formats, adds or removes a device.
pub fn verify_identity_mount(device: &Path, mount_point: &Path, expected_uuid: &str) -> Result<()> {
    let uuid_path = uuid_device_path(expected_uuid)?;
    verify_exact_mount(mount_point)?;
    // A stale /dev name must not be opened or used as fallback if UUID lookup
    // fails. Regular image paths remain pinned to the actual backing inode.
    let backing = if device.starts_with("/dev") {
        uuid_path
    } else {
        let metadata = std::fs::metadata(device).context("inspect identity image backing")?;
        anyhow::ensure!(
            metadata.is_file(),
            "identity image backing must be a regular file"
        );
        device.to_path_buf()
    };
    verify_dedicated_mount(&backing, mount_point, expected_uuid)
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
    fn uuid_device_selection_is_canonical_and_rejects_paths() {
        assert_eq!(
            uuid_device_path("806E2801-8BE0-4B0D-919C-3E319452667B").unwrap(),
            Path::new("/dev/disk/by-uuid/806e2801-8be0-4b0d-919c-3e319452667b")
        );
        for invalid in [
            "",
            "../../sdb2",
            "/dev/sdb2",
            "00000000-0000-0000-0000-000000000000",
        ] {
            assert!(uuid_device_path(invalid).is_err());
        }
    }

    #[test]
    fn source_preflight_rejects_stale_uuid_and_pool_membership() {
        let uuid = "806e2801-8be0-4b0d-919c-3e319452667b";
        let mut superblock = [0_u8; 4096];
        superblock[32..48].copy_from_slice(uuid::Uuid::parse_str(uuid).unwrap().as_bytes());
        superblock[64..72].copy_from_slice(b"_BHRfS_M");
        superblock[48..56].copy_from_slice(&65536_u64.to_le_bytes());
        superblock[136..144].copy_from_slice(&1_u64.to_le_bytes());
        verify_superblock_identity(&superblock, uuid).unwrap();
        superblock[32] ^= 1;
        assert!(
            verify_superblock_identity(&superblock, uuid)
                .unwrap_err()
                .to_string()
                .contains("on-disk")
        );
        superblock[32] ^= 1;
        superblock[136..144].copy_from_slice(&3_u64.to_le_bytes());
        assert!(
            verify_superblock_identity(&superblock, uuid)
                .unwrap_err()
                .to_string()
                .contains("multi-device")
        );
        superblock[64] = 0;
        assert!(
            verify_superblock_identity(&superblock, uuid)
                .unwrap_err()
                .to_string()
                .contains("not Btrfs")
        );
    }

    #[test]
    fn identity_directory_cannot_be_treated_as_its_parent_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let error = verify_identity_mount(
            Path::new("/dev/nonexistent-stale-device"),
            dir.path(),
            "806e2801-8be0-4b0d-919c-3e319452667b",
        )
        .unwrap_err();
        assert!(error.to_string().contains("mount is absent"));
        assert!(verify_exact_mount(Path::new("/")).is_err());
        let link = dir.path().join("alias");
        std::os::unix::fs::symlink(dir.path(), &link).unwrap();
        assert!(
            verify_exact_mount(&link)
                .unwrap_err()
                .to_string()
                .contains("canonical")
        );
    }

    /// Run through deploy/scripts/tests/test-identity-uuid-mount.sh, which owns
    /// disposable loop devices and a private mount namespace/UUID alias tree.
    #[test]
    #[ignore = "requires isolated root-owned Btrfs loop fixture"]
    fn native_uuid_mount_fixture() {
        let fixture = PathBuf::from(std::env::var_os("IDENTITY_UUID_FIXTURE").unwrap());
        let uuid = std::fs::read_to_string(fixture.join("uuid")).unwrap();
        let uuid = uuid.trim();
        let mount = fixture.join("mount");
        let source = File::open(uuid_device_path(uuid).unwrap()).unwrap();
        verify_unmounted_source(&source, uuid).unwrap();
        // Neither an absent historical name nor another current block device
        // is authoritative once the registered UUID has been resolved.
        verify_identity_mount(Path::new("/dev/stale-sdb2"), &mount, uuid).unwrap();
        verify_identity_mount(Path::new("/dev/sda4"), &mount, uuid).unwrap();
        verify_identity_mount(&fixture.join("image"), &mount, uuid).unwrap();
        assert!(verify_identity_mount(&fixture.join("other-image"), &mount, uuid).is_err());
        assert!(
            verify_identity_mount(
                Path::new("/dev/stale-sdb2"),
                &fixture.join("unmounted"),
                uuid
            )
            .is_err()
        );
        let wrong_uuid = "806e2801-8be0-4b0d-919c-3e319452667b";
        assert!(verify_unmounted_source(&source, wrong_uuid).is_err());
        assert!(verify_identity_mount(Path::new("/dev/stale-sdb2"), &mount, wrong_uuid).is_err());
        // A stale by-uuid alias cannot override the FSID reported by the kernel.
        std::os::unix::fs::symlink(
            uuid_device_path(uuid).unwrap(),
            uuid_device_path(wrong_uuid).unwrap(),
        )
        .unwrap();
        assert!(verify_identity_mount(Path::new("/dev/stale-sdb2"), &mount, wrong_uuid).is_err());
        std::fs::remove_file(uuid_device_path(uuid).unwrap()).unwrap();
        assert!(verify_identity_mount(Path::new("/dev/stale-sdb2"), &mount, uuid).is_err());
        // Failure does not damage or eject the actual image-backed filesystem.
        verify_identity_mount(&fixture.join("image"), &mount, uuid).unwrap();
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
