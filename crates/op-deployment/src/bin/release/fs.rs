//! Descriptor-bound Linux mount operations for the base-system release CLI.
//! No historical storage coordinator or retirement helpers are used here.
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    ffi::{CStr, CString},
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd},
        unix::{ffi::OsStrExt, fs::{MetadataExt, OpenOptionsExt}},
    },
    path::{Component, Path},
};

const RESOLVE_NO_MAGICLINKS: u64 = 0x02;
const RESOLVE_NO_SYMLINKS: u64 = 0x04;
const RESOLVE_BENEATH: u64 = 0x08;
const FSCONFIG_SET_STRING: u32 = 1;
const FSCONFIG_SET_FD: u32 = 5;
const FSCONFIG_CMD_CREATE: u32 = 6;
const MOVE_MOUNT_F_EMPTY_PATH: u32 = 0x04;
const MOVE_MOUNT_T_EMPTY_PATH: u32 = 0x40;
const MOVE_MOUNT_BENEATH: u32 = 0x200;
const MOUNT_ATTR_RDONLY: u32 = 0x01;
const MOUNT_ATTR_NOSUID: u32 = 0x02;
const MOUNT_ATTR_NODEV: u32 = 0x04;
const MOUNT_ATTR_NOEXEC: u32 = 0x08;

#[repr(C)]
struct OpenHow { flags: u64, mode: u64, resolve: u64 }

fn checked_fd(result: libc::c_long) -> Result<OwnedFd> {
    if result < 0 { return Err(std::io::Error::last_os_error().into()); }
    // SAFETY: successful descriptor-returning syscall transfers a new descriptor.
    Ok(unsafe { OwnedFd::from_raw_fd(result as i32) })
}

pub fn relative(path: &Path) -> Result<()> {
    ensure!(!path.as_os_str().is_empty(), "empty relative path");
    ensure!(path.components().all(|c| matches!(c, Component::Normal(_))),
        "non-normal relative path: {}", path.display());
    ensure!(path.as_os_str().as_bytes().split(|&c| c == b'/')
        .all(|part| !part.is_empty() && part != b"." && part != b".."),
        "non-normal relative path: {}", path.display());
    Ok(())
}

pub fn open_beneath(dir: BorrowedFd<'_>, path: &Path, flags: i32) -> Result<File> {
    relative(path)?;
    let name = CString::new(path.as_os_str().as_bytes())?;
    let how = OpenHow {
        flags: (flags | libc::O_CLOEXEC | libc::O_NOFOLLOW) as u64,
        mode: 0,
        resolve: RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS,
    };
    // SAFETY: name and the correctly sized open_how remain valid for the syscall.
    let fd = unsafe { libc::syscall(libc::SYS_openat2, dir.as_raw_fd(), name.as_ptr(),
        &how, std::mem::size_of::<OpenHow>()) };
    Ok(File::from(checked_fd(fd).with_context(|| format!("open beneath: {}", path.display()))?))
}

pub fn directory(fd: BorrowedFd<'_>) -> Result<File> {
    // Opening '.' converts a mount/O_PATH descriptor into an ioctl/read capable fd.
    // SAFETY: the borrowed directory descriptor remains live throughout openat.
    let result = unsafe { libc::openat(fd.as_raw_fd(), c".".as_ptr(),
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW) };
    Ok(File::from(checked_fd(result as libc::c_long)?))
}

pub fn secure_absolute(path: &Path, flags: i32) -> Result<File> {
    ensure!(path.is_absolute(), "absolute path required: {}", path.display());
    let mut dir = OpenOptions::new().read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC).open("/")?;
    let mut components = path.components().skip(1).enumerate().peekable();
    ensure!(components.peek().is_some(), "filesystem root is not a release operand");
    ensure!(path.as_os_str().as_bytes()[1..].split(|&c| c == b'/')
        .all(|part| !part.is_empty() && part != b"." && part != b".."),
        "non-normal absolute path: {}", path.display());
    while let Some((index, component)) = components.next() {
        let Component::Normal(name) = component else { bail!("non-normal absolute path"); };
        dir = open_beneath(dir.as_fd(), Path::new(name), if components.peek().is_none() { flags } else {
            libc::O_RDONLY | libc::O_DIRECTORY
        })?;
        let metadata = dir.metadata()?;
        // /dev/shm is the single intentional root-owned sticky ancestor. Its
        // descendants still require root ownership and no group/world writes.
        let sticky_shm = index == 1 && path.starts_with("/dev/shm")
            && metadata.is_dir() && metadata.mode() & libc::S_ISVTX != 0;
        ensure!(metadata.uid() == 0 && (metadata.mode() & 0o022 == 0 || sticky_shm),
            "release path must be root-owned and not group/world writable: {}", path.display());
    }
    Ok(dir)
}

fn configure(fd: BorrowedFd<'_>, key: &CStr, value: &CStr) -> Result<()> {
    // SAFETY: both nul-terminated strings and the filesystem context remain live.
    let result = unsafe { libc::syscall(libc::SYS_fsconfig, fd.as_raw_fd(),
        FSCONFIG_SET_STRING, key.as_ptr(), value.as_ptr(), 0) };
    ensure_syscall(result).with_context(|| format!("fsconfig {}", key.to_string_lossy()))
}

fn ensure_syscall(result: libc::c_long) -> Result<()> {
    if result < 0 { Err(std::io::Error::last_os_error().into()) } else { Ok(()) }
}

fn context(kind: &CStr) -> Result<OwnedFd> {
    // SAFETY: kind is a valid C string; flag 1 is FSOPEN_CLOEXEC.
    checked_fd(unsafe { libc::syscall(libc::SYS_fsopen, kind.as_ptr(), 1u32) })
}

fn finish_context(context: BorrowedFd<'_>, attrs: u32) -> Result<OwnedFd> {
    // SAFETY: CMD_CREATE takes no key/value, and the borrowed context is valid.
    ensure_syscall(unsafe { libc::syscall(libc::SYS_fsconfig, context.as_raw_fd(),
        FSCONFIG_CMD_CREATE, std::ptr::null::<libc::c_char>(),
        std::ptr::null::<libc::c_char>(), 0) })?;
    // SAFETY: flag 1 is FSMOUNT_CLOEXEC; attrs are kernel mount-attribute bits.
    checked_fd(unsafe { libc::syscall(libc::SYS_fsmount, context.as_raw_fd(), 1u32, attrs) })
}

pub fn detached_btrfs(uuid: &str, subvolume: u64, readonly: bool) -> Result<OwnedFd> {
    let parsed = uuid::Uuid::parse_str(uuid)?;
    let device = File::open(format!("/dev/disk/by-uuid/{parsed}"))?;
    ensure!(device.metadata()?.mode() & libc::S_IFMT == libc::S_IFBLK,
        "Btrfs UUID does not resolve to a block device");
    let context = context(c"btrfs")?;
    let source = CString::new(format!("/proc/self/fd/{}", device.as_raw_fd()))?;
    configure(context.as_fd(), c"source", &source)?;
    configure(context.as_fd(), c"subvolid", &CString::new(subvolume.to_string())?)?;
    let attrs = MOUNT_ATTR_NODEV | MOUNT_ATTR_NOSUID | MOUNT_ATTR_NOEXEC
        | if readonly { MOUNT_ATTR_RDONLY } else { 0 };
    let mount = finish_context(context.as_fd(), attrs)?;
    let root = directory(mount.as_fd())?;
    ensure!(btrfs_uapi::filesystem::filesystem_info(root.as_fd())?.uuid == parsed,
        "mounted filesystem UUID differs from requested release filesystem");
    ensure!(btrfs_uapi::subvolume::subvolume_info(root.as_fd())?.id == subvolume,
        "mounted Btrfs root ID differs from requested root");
    Ok(mount)
}

pub fn release_base() -> Result<File> {
    let base = secure_absolute(Path::new(crate::release::model::RELEASE_BASE),
        libc::O_RDONLY | libc::O_DIRECTORY)?;
    let metadata = base.metadata()?;
    ensure!(metadata.is_dir(), "release base is not a directory");
    ensure!(metadata.uid() == 0 && metadata.gid() == 0, "release base is not root-owned");
    ensure!(metadata.mode() & 0o7022 == 0, "release base has set-id/sticky or group/world write bits");
    // Enumerating through the pinned fd's /proc/self/fd link never re-resolves the path.
    ensure!(std::fs::read_dir(format!("/proc/self/fd/{}", base.as_raw_fd()))?.next().is_none(),
        "release base is not empty");
    // SAFETY: null size query on a live borrowed directory descriptor.
    let size = unsafe { libc::flistxattr(base.as_raw_fd(), std::ptr::null_mut(), 0) };
    ensure_syscall(size as libc::c_long).context("release base xattr query")?;
    ensure!(size == 0, "release base has extended attributes");
    Ok(base)
}

pub fn detached_overlay(lower: BorrowedFd<'_>, base: BorrowedFd<'_>, tag: &str, noexec: bool) -> Result<OwnedFd> {
    ensure!(!tag.is_empty() && tag.bytes().all(|c| c.is_ascii_alphanumeric()
        || b":-_.".contains(&c)), "invalid release mount tag");
    let context = context(c"overlay")?;
    configure(context.as_fd(), c"source", &CString::new(tag)?)?;
    for option in [c"redirect_dir", c"index", c"metacopy", c"xino"] {
        configure(context.as_fd(), option, c"off")?;
    }
    // Linux 6.13+ accepts layer directory descriptors. Never re-resolve a layer path.
    // SAFETY: lower is a live, already verified directory fd, borrowed until mount creation.
    ensure_syscall(unsafe { libc::syscall(libc::SYS_fsconfig, context.as_raw_fd(),
        FSCONFIG_SET_FD, c"lowerdir+".as_ptr(), std::ptr::null::<libc::c_char>(),
        lower.as_raw_fd()) }).context("overlay descriptor layers require Linux 6.13+")?;
    // Without an upper, overlayfs requires a second lower; the verified empty base
    // supplies it without changing the merged view (first lowerdir+ stays topmost).
    // SAFETY: base is a live, verified empty directory fd, borrowed until mount creation.
    ensure_syscall(unsafe { libc::syscall(libc::SYS_fsconfig, context.as_raw_fd(),
        FSCONFIG_SET_FD, c"lowerdir+".as_ptr(), std::ptr::null::<libc::c_char>(),
        base.as_raw_fd()) }).context("overlay descriptor layers require Linux 6.13+")?;
    finish_context(context.as_fd(), MOUNT_ATTR_RDONLY | MOUNT_ATTR_NOSUID
        | MOUNT_ATTR_NODEV | if noexec { MOUNT_ATTR_NOEXEC } else { 0 })
}

pub fn place(mount: BorrowedFd<'_>, target: BorrowedFd<'_>, beneath: bool) -> Result<()> {
    let flags = MOVE_MOUNT_F_EMPTY_PATH | MOVE_MOUNT_T_EMPTY_PATH
        | if beneath { MOVE_MOUNT_BENEATH } else { 0 };
    // SAFETY: both operands are pinned descriptors; empty paths select their objects.
    ensure_syscall(unsafe { libc::syscall(libc::SYS_move_mount, mount.as_raw_fd(),
        c"".as_ptr(), target.as_raw_fd(), c"".as_ptr(), flags) })
}

pub fn detach(mount: BorrowedFd<'_>) -> Result<()> {
    let operand = CString::new(format!("/proc/self/fd/{}", mount.as_raw_fd()))?;
    // /proc/self/fd is a kernel magic link to the pinned old mount, not a reconstructed
    // pathname. Never call this on a candidate placed beneath a still-attached old top.
    // SAFETY: operand is live and nul terminated. MNT_DETACH is only for old overlays.
    ensure_syscall(unsafe { libc::umount2(operand.as_ptr(), libc::MNT_DETACH) } as libc::c_long)
}

pub fn unmount(path: &Path) -> Result<()> {
    let operand = CString::new(path.as_os_str().as_bytes())?;
    // SAFETY: final symlinks are rejected; busy backing mounts MUST remain mounted.
    ensure_syscall(unsafe { libc::umount2(operand.as_ptr(), libc::UMOUNT_NOFOLLOW) } as libc::c_long)
}

pub fn mount_id(fd: BorrowedFd<'_>) -> Result<u64> {
    let mut info = std::mem::MaybeUninit::<libc::statx>::zeroed();
    // SAFETY: info points to a correctly sized writable statx and fd is borrowed live.
    ensure_syscall(unsafe { libc::statx(fd.as_raw_fd(), c"".as_ptr(),
        libc::AT_EMPTY_PATH | libc::AT_SYMLINK_NOFOLLOW, libc::STATX_MNT_ID,
        info.as_mut_ptr()) } as libc::c_long)?;
    // SAFETY: statx completed successfully and initialized the struct.
    let info = unsafe { info.assume_init() };
    ensure!(info.stx_mask & libc::STATX_MNT_ID != 0, "kernel did not supply mount ID");
    Ok(info.stx_mnt_id)
}

pub fn hash_file(file: &mut File) -> Result<String> {
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 { break; }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn read_bounded(file: &mut File, limit: usize) -> Result<Vec<u8>> {
    let size = usize::try_from(file.metadata()?.len())?;
    ensure!(size <= limit, "release metadata exceeds {limit} bytes");
    let mut bytes = Vec::with_capacity(size);
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "release metadata grew past its bound");
    Ok(bytes)
}

fn inspect_xattr_names(mut list: impl FnMut(*mut libc::c_char, usize) -> isize) -> Result<()> {
    let size = list(std::ptr::null_mut(), 0);
    ensure_syscall(size as libc::c_long)?;
    ensure!(size <= 64 * 1024, "extended attribute list exceeds bound");
    if size == 0 { return Ok(()); }
    let mut names = vec![0u8; size as usize];
    let read = list(names.as_mut_ptr().cast(), names.len());
    ensure_syscall(read as libc::c_long)?;
    ensure!(read as usize <= names.len(), "extended attribute list changed");
    for name in names[..read as usize].split(|&b| b == 0) {
        ensure!(!name.starts_with(b"trusted.overlay.") && !name.starts_with(b"user.overlay."),
            "release lower tree contains overlay control metadata");
    }
    Ok(())
}

pub fn reject_overlay_metadata(fd: BorrowedFd<'_>) -> Result<()> {
    // SAFETY: the helper supplies either a null size query or a valid writable buffer.
    inspect_xattr_names(|buffer, length| unsafe { libc::flistxattr(fd.as_raw_fd(), buffer, length) })
}

fn basename(path: &Path) -> Result<CString> {
    relative(path)?;
    ensure!(!path.as_os_str().as_bytes().contains(&b'/'), "single path component required");
    Ok(CString::new(path.as_os_str().as_bytes())?)
}

pub fn reject_symlink_overlay_metadata(parent: BorrowedFd<'_>, name: &Path) -> Result<()> {
    let name = basename(name)?;
    let mut bytes = format!("/proc/self/fd/{}/", parent.as_raw_fd()).into_bytes();
    bytes.extend_from_slice(name.to_bytes());
    let path = CString::new(bytes)?;
    // SAFETY: pinned parent remains live; llistxattr never follows the final symlink.
    inspect_xattr_names(|buffer, length| unsafe { libc::llistxattr(path.as_ptr(), buffer, length) })
}

fn sync_directory(fd: BorrowedFd<'_>) -> Result<()> {
    // SAFETY: fsync only acts on the borrowed descriptor.
    let result = unsafe { libc::fsync(fd.as_raw_fd()) };
    if result == 0 { return Ok(()); }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::EBADF) {
        directory(fd)?.sync_all()?;
        Ok(())
    } else { Err(error.into()) }
}

pub fn mkdir_at(parent: BorrowedFd<'_>, name: &Path, mode: u32) -> Result<()> {
    let component = basename(name)?;
    // SAFETY: name is a single normal component beneath the pinned parent.
    ensure_syscall(unsafe { libc::mkdirat(parent.as_raw_fd(), component.as_ptr(), mode) } as libc::c_long)?;
    // The launcher's private umask must not remove traversal permissions from
    // service/logger directories. Apply the caller's exact mode only to this
    // newly created, descriptor-bound directory; 0700 administration stays private.
    let created = open_beneath(parent, name, libc::O_RDONLY | libc::O_DIRECTORY)?;
    // SAFETY: created owns the new directory descriptor for the entire call.
    ensure_syscall(unsafe { libc::fchmod(created.as_raw_fd(), mode) } as libc::c_long)?;
    created.sync_all()?;
    sync_directory(parent)
}

pub fn remove_at(parent: BorrowedFd<'_>, name: &Path, directory: bool) -> Result<()> {
    let name = basename(name)?;
    // SAFETY: unlinkat does not follow the final component; no recursive removal.
    ensure_syscall(unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(),
        if directory { libc::AT_REMOVEDIR } else { 0 }) } as libc::c_long)?;
    sync_directory(parent)
}

pub fn rename_at(from: BorrowedFd<'_>, name: &Path, to: BorrowedFd<'_>, destination: &Path) -> Result<()> {
    let name = basename(name)?;
    let destination = basename(destination)?;
    // SAFETY: both names are single components beneath pinned directories; an
    // unrelated destination is never overwritten, including after a recovery race.
    ensure_syscall(unsafe { libc::syscall(libc::SYS_renameat2, from.as_raw_fd(), name.as_ptr(),
        to.as_raw_fd(), destination.as_ptr(), libc::RENAME_NOREPLACE) })?;
    sync_directory(from)?;
    if from.as_raw_fd() != to.as_raw_fd() { sync_directory(to)?; }
    Ok(())
}

fn atomic_write(path: &Path, mode: u32, write: impl FnOnce(&mut File) -> Result<()>) -> Result<()> {
    let parent = secure_absolute(path.parent().context("atomic path lacks parent")?,
        libc::O_RDONLY | libc::O_DIRECTORY)?;
    let destination = basename(Path::new(path.file_name().context("atomic path lacks filename")?))?;
    let temporary = CString::new(format!(".release-{}", uuid::Uuid::new_v4()))?;
    // SAFETY: exclusive single-component creation under the pinned parent.
    let descriptor = unsafe { libc::openat(parent.as_raw_fd(), temporary.as_ptr(),
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC, 0o600) };
    let mut file = File::from(checked_fd(descriptor as libc::c_long)?);
    let result = (|| -> Result<()> {
        write(&mut file)?;
        // SAFETY: set exact visibility independent of the launcher's restrictive umask.
        ensure_syscall(unsafe { libc::fchmod(file.as_raw_fd(), mode) } as libc::c_long)?;
        file.sync_all()?;
        // SAFETY: rename replaces only this owned journal/readiness filename.
        ensure_syscall(unsafe { libc::renameat(parent.as_raw_fd(), temporary.as_ptr(),
            parent.as_raw_fd(), destination.as_ptr()) } as libc::c_long)?;
        parent.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        // SAFETY: remove only the exclusively created temporary name, if it remains.
        unsafe { libc::unlinkat(parent.as_raw_fd(), temporary.as_ptr(), 0); }
        let _ = parent.sync_all();
    }
    result
}

pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    atomic_write(path, 0o600, |file| {
        serde_json::to_writer(&mut *file, value)?;
        file.write_all(b"\n")?;
        Ok(())
    })
}

pub fn atomic_text(path: &Path, value: &str) -> Result<()> {
    atomic_write(path, 0o644, |file| { file.write_all(value.as_bytes())?; Ok(()) })
}

pub fn verify_launcher_lock(path: &Path) -> Result<()> {
    let declared = secure_absolute(path, libc::O_RDONLY)?;
    // SAFETY: fstat only inspects inherited fd9; it does not take ownership or close it.
    let mut stat = std::mem::MaybeUninit::<libc::stat>::zeroed();
    ensure_syscall(unsafe { libc::fstat(9, stat.as_mut_ptr()) } as libc::c_long)
        .context("mutating release verbs require launcher lock on fd9")?;
    // SAFETY: successful fstat initialized stat.
    let stat = unsafe { stat.assume_init() };
    let expected = declared.metadata()?;
    ensure!(stat.st_dev == expected.dev() && stat.st_ino == expected.ino(),
        "fd9 is not the release transaction lock");
    // Reassert the lock on the SAME open-file description, not a separately opened fd.
    // SAFETY: flock accepts the validated inherited descriptor and integer flags.
    ensure_syscall(unsafe { libc::flock(9, libc::LOCK_EX | libc::LOCK_NB) } as libc::c_long)
}
