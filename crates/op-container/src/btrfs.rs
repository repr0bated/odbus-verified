//! Minimal fd-pinned Btrfs and directory primitives (kernel ABI from
//! linux/btrfs.h). Every path step is opened relative to a held directory fd
//! with O_NOFOLLOW, so a symlink or rename cannot redirect an operation.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

use anyhow::{Context, Result, bail, ensure};

const BTRFS_STATFS_MAGIC: libc::c_long = 0x9123_683e;
const BTRFS_IOC_FS_INFO: libc::c_ulong = 0x8400_941f;
const BTRFS_IOC_GET_SUBVOL_INFO: libc::c_ulong = 0x81f8_943c;
/// _IOW(0x94, 15, struct btrfs_ioctl_vol_args)
const BTRFS_IOC_SNAP_DESTROY: libc::c_ulong = 0x5000_940f;
/// _IOW(0x94, 23, struct btrfs_ioctl_vol_args_v2)
const BTRFS_IOC_SNAP_CREATE_V2: libc::c_ulong = 0x5000_9417;
pub const SUBVOL_RDONLY: u64 = 1;
/// A subvolume root directory always has this inode number.
pub const SUBVOL_ROOT_INO: u64 = 256;

pub struct SubvolumeInfo {
    pub treeid: u64,
    pub parent_id: u64,
    pub name: Vec<u8>,
    pub flags: u64,
    pub uuid: [u8; 16],
    pub parent_uuid: [u8; 16],
}

pub fn open_absolute_dir(path: &Path) -> Result<File> {
    ensure!(path.is_absolute(), "expected absolute directory path");
    let mut current = open_at(None, b"/", false)?.context("filesystem root is absent")?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                current = open_dir(&current, name.as_encoded_bytes())?
                    .with_context(|| format!("{} is absent", path.display()))?;
            }
            _ => bail!("trusted path has traversal or prefix"),
        }
    }
    Ok(current)
}

/// Open a child directory owned by root and not group/world writable.
pub fn open_dir(parent: &File, name: &[u8]) -> Result<Option<File>> {
    let Some(dir) = open_at(Some(parent), name, true)? else {
        return Ok(None);
    };
    let meta = dir.metadata()?;
    ensure!(
        meta.uid() == 0 && meta.mode() & 0o022 == 0,
        "{} has unexpected ownership or write mode",
        String::from_utf8_lossy(name)
    );
    Ok(Some(dir))
}

/// Incus protects a started container wrapper with its isolated host-root UID.
/// Only that exact daemon-authored UID is allowed; pool/seed ancestors still
/// use the root-only opener above.
pub fn open_volume(parent: &File, name: &[u8], isolated_uid: Option<u32>) -> Result<Option<File>> {
    let Some(dir) = open_at(Some(parent), name, true)? else { return Ok(None) };
    let meta = dir.metadata()?;
    ensure!((meta.uid() == 0 && meta.mode() & 0o022 == 0)
        || (isolated_uid.is_some_and(|uid| uid != 0 && uid == meta.uid())
            && meta.gid() == 0 && meta.mode() & 0o777 == 0o100),
        "container wrapper no longer matches the exact isolated native root");
    Ok(Some(dir))
}

fn open_at(parent: Option<&File>, name: &[u8], nofollow: bool) -> Result<Option<File>> {
    let c_name = CString::new(name)?;
    let dirfd = parent.map_or(libc::AT_FDCWD, |p| p.as_raw_fd());
    let flags = libc::O_RDONLY
        | libc::O_DIRECTORY
        | libc::O_CLOEXEC
        | if nofollow { libc::O_NOFOLLOW } else { 0 };
    // SAFETY: dirfd is a held directory (or AT_FDCWD for "/"); name is NUL-terminated.
    let fd = unsafe { libc::openat(dirfd, c_name.as_ptr(), flags) };
    if fd < 0 {
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(error).with_context(|| format!("open {}", String::from_utf8_lossy(name)));
    }
    // SAFETY: successful openat returns an owned descriptor.
    Ok(Some(unsafe { File::from_raw_fd(fd) }))
}

pub fn mount_id(file: &File) -> Result<u64> {
    // SAFETY: statx writes into an initialized struct for a pinned fd.
    let mut info: libc::statx = unsafe { std::mem::zeroed() };
    let empty = CString::new("")?;
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
        return Err(io::Error::last_os_error()).context("read mount identity");
    }
    ensure!(info.stx_mask & libc::STATX_MNT_ID != 0, "kernel lacks mount identity");
    Ok(info.stx_mnt_id)
}

pub fn fsid(file: &File) -> Result<[u8; 16]> {
    // SAFETY: fstatfs writes into an initialized struct for a pinned fd.
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstatfs(file.as_raw_fd(), &mut stat) } != 0 {
        return Err(io::Error::last_os_error()).context("read filesystem type");
    }
    ensure!(stat.f_type == BTRFS_STATFS_MAGIC, "not a Btrfs filesystem");
    let mut raw = [0u8; 1024];
    // SAFETY: buffer matches btrfs_ioctl_fs_info_args (1024 bytes).
    if unsafe { libc::ioctl(file.as_raw_fd(), BTRFS_IOC_FS_INFO, raw.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error()).context("read Btrfs FSID");
    }
    Ok(raw[16..32].try_into()?)
}

pub fn subvolume_info(file: &File) -> Result<SubvolumeInfo> {
    let mut raw = [0u8; 504];
    // SAFETY: buffer matches btrfs_ioctl_get_subvol_info_args (504 bytes).
    // The kernel can return 1 (not -1) when the subvolume's back-reference
    // lies on a seed device: identity, flags and UUIDs are filled, but
    // parent_id/name are not. Only a negative return is an error.
    if unsafe { libc::ioctl(file.as_raw_fd(), BTRFS_IOC_GET_SUBVOL_INFO, raw.as_mut_ptr()) } < 0 {
        return Err(io::Error::last_os_error()).context("read Btrfs subvolume identity");
    }
    let end = raw[8..264]
        .iter()
        .position(|byte| *byte == 0)
        .context("unterminated Btrfs subvolume name")?;
    // Layout: treeid@0 name[256]@8 parent_id@264 dirid@272 generation@280
    // flags@288 uuid@296 parent_uuid@312.
    Ok(SubvolumeInfo {
        treeid: u64::from_ne_bytes(raw[0..8].try_into()?),
        name: raw[8..8 + end].to_vec(),
        parent_id: u64::from_ne_bytes(raw[264..272].try_into()?),
        flags: u64::from_ne_bytes(raw[288..296].try_into()?),
        uuid: raw[296..312].try_into()?,
        parent_uuid: raw[312..328].try_into()?,
    })
}

#[repr(C)]
struct VolArgs {
    fd: i64,
    name: [u8; 4088],
}

#[repr(C)]
struct VolArgsV2 {
    fd: i64,
    transid: u64,
    flags: u64,
    unused: [u64; 4],
    name: [u8; 4040],
}
const _: () = assert!(std::mem::size_of::<VolArgs>() == 4096);
const _: () = assert!(std::mem::size_of::<VolArgsV2>() == 4096);

/// Create `parent/name` as a writable snapshot of the subvolume `source`.
pub fn snapshot(source: &File, parent: &File, name: &[u8]) -> Result<()> {
    ensure!(!name.is_empty() && name.len() < 4040 && !name.contains(&0), "invalid snapshot name");
    let mut args = VolArgsV2 {
        fd: source.as_raw_fd() as i64,
        transid: 0,
        flags: 0,
        unused: [0; 4],
        name: [0; 4040],
    };
    args.name[..name.len()].copy_from_slice(name);
    // SAFETY: fixed-size kernel struct with a NUL-terminated direct name.
    if unsafe { libc::ioctl(parent.as_raw_fd(), BTRFS_IOC_SNAP_CREATE_V2, &mut args) } != 0 {
        return Err(io::Error::last_os_error()).context("create Btrfs snapshot");
    }
    Ok(())
}

/// Delete the direct child subvolume `parent/name`.
pub fn destroy_subvolume(parent: &File, name: &[u8]) -> Result<()> {
    ensure!(!name.is_empty() && name.len() < 4088 && !name.contains(&0), "invalid subvolume name");
    let mut args = VolArgs { fd: 0, name: [0; 4088] };
    args.name[..name.len()].copy_from_slice(name);
    // SAFETY: fixed-size kernel struct with a NUL-terminated direct name.
    if unsafe { libc::ioctl(parent.as_raw_fd(), BTRFS_IOC_SNAP_DESTROY, &mut args) } != 0 {
        return Err(io::Error::last_os_error()).context("destroy Btrfs subvolume");
    }
    Ok(())
}

/// Atomically swap two entries in the same directory.
pub fn exchange(parent: &File, a: &[u8], b: &[u8]) -> Result<()> {
    let (a, b) = (CString::new(a)?, CString::new(b)?);
    let fd = parent.as_raw_fd();
    // SAFETY: both names are NUL-terminated and resolved in one held directory.
    if unsafe { libc::renameat2(fd, a.as_ptr(), fd, b.as_ptr(), libc::RENAME_EXCHANGE) } != 0 {
        return Err(io::Error::last_os_error()).context("swap container volume into place");
    }
    Ok(())
}

pub fn syncfs(file: &File) -> Result<()> {
    // SAFETY: pinned fd.
    if unsafe { libc::syncfs(file.as_raw_fd()) } != 0 {
        return Err(io::Error::last_os_error()).context("sync filesystem");
    }
    Ok(())
}

pub fn list(dir: &File) -> Result<Vec<Vec<u8>>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir(format!("/proc/self/fd/{}", dir.as_raw_fd()))? {
        names.push(entry?.file_name().into_encoded_bytes());
    }
    names.sort();
    Ok(names)
}
