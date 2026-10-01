//! Operator-only FD transfer: clone in host namespace, then attach after idmap.
//! Receipts describe mounts, not keys, grants or identity lifecycle authority.
use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const TARGET: &str = "opt/run-mounts/3tched";
const BTRFS_MAGIC: libc::c_long = 0x9123_683e;
const FUSE_MAGIC: libc::c_long = 0x6573_5546;
const FS_INFO: libc::c_ulong = 0x8400_941f;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LungsReceipt {
    boot_id: String,
    session: String,
    target: PathBuf,
    pid: u32,
    start: String,
    args: Vec<String>,
    mount_id: String,
}

fn trusted(path: &Path) -> Result<()> {
    ensure!(
        path.is_absolute() && fs::canonicalize(path)? == path,
        "noncanonical trusted path"
    );
    for ancestor in path.ancestors() {
        let m = fs::symlink_metadata(ancestor)?;
        ensure!(
            !m.file_type().is_symlink() && m.uid() == 0,
            "trusted path owner changed"
        );
        ensure!(
            m.mode() & 0o022 == 0 || (m.is_dir() && m.mode() & 0o1000 != 0),
            "trusted path is writable"
        );
    }
    Ok(())
}

fn json(path: &Path) -> Result<Value> {
    trusted(path)?;
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let m = f.metadata()?;
    ensure!(
        m.is_file() && m.uid() == 0 && m.mode() & 0o777 == 0o600 && m.len() <= 16384,
        "unsafe mount receipt"
    );
    let mut bytes = Vec::new();
    f.take(16385).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 16384, "oversized mount receipt");
    Ok(serde_json::from_slice(&bytes)?)
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .with_context(|| format!("missing receipt field {field}"))
}

fn fd_open(path: &Path, flags: i32) -> Result<OwnedFd> {
    let path = CString::new(path.as_os_str().as_bytes())?;
    // SAFETY: nul-terminated path; a successful descriptor is uniquely owned.
    let fd = unsafe { libc::open(path.as_ptr(), flags | libc::O_CLOEXEC) };
    ensure!(fd >= 0, "open failed: {}", std::io::Error::last_os_error());
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn proc_open(proc: &OwnedFd, name: &str, flags: i32) -> Result<OwnedFd> {
    let name = CString::new(name)?;
    // SAFETY: proc is a held directory FD and name is fixed by this module.
    let fd = unsafe { libc::openat(proc.as_raw_fd(), name.as_ptr(), flags | libc::O_CLOEXEC) };
    ensure!(
        fd >= 0,
        "process entry unavailable: {}",
        std::io::Error::last_os_error()
    );
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn start_time(proc: &OwnedFd) -> Result<u64> {
    let fd = proc_open(proc, "stat", libc::O_RDONLY | libc::O_NOFOLLOW)?;
    let mut text = String::new();
    File::from(fd).take(4096).read_to_string(&mut text)?;
    let fields = text.rsplit_once(") ").context("invalid process stat")?.1;
    fields
        .split_whitespace()
        .nth(19)
        .context("missing start time")?
        .parse()
        .context("invalid start time")
}

fn alive(pidfd: &OwnedFd) -> Result<()> {
    let mut p = libc::pollfd {
        fd: pidfd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: p is a valid one-element poll buffer; timeout is nonblocking.
    let result = unsafe { libc::poll(&mut p, 1, 0) };
    ensure!(result == 0, "target process exited or pidfd check failed");
    Ok(())
}

fn metadata(fd: &OwnedFd) -> Result<std::fs::Metadata> {
    Ok(File::from(fd.try_clone()?).metadata()?)
}

fn fs_type(fd: &OwnedFd) -> Result<libc::c_long> {
    // SAFETY: statfs is an output-only POD buffer.
    let mut s: libc::statfs = unsafe { std::mem::zeroed() };
    ensure!(
        unsafe { libc::fstatfs(fd.as_raw_fd(), &mut s) } == 0,
        "fstatfs failed"
    );
    Ok(s.f_type)
}

fn mount_id(fd: &OwnedFd) -> Result<u64> {
    // SAFETY: statx is an output-only POD buffer; empty path addresses fd.
    let mut s: libc::statx = unsafe { std::mem::zeroed() };
    ensure!(
        unsafe {
            libc::statx(
                fd.as_raw_fd(),
                c"".as_ptr(),
                libc::AT_EMPTY_PATH,
                libc::STATX_MNT_ID,
                &mut s,
            )
        } == 0
            && s.stx_mask & libc::STATX_MNT_ID != 0,
        "statx mount ID unavailable"
    );
    Ok(s.stx_mnt_id)
}

#[repr(C)]
struct OpenHow {
    flags: u64,
    mode: u64,
    resolve: u64,
}

fn target(root: &OwnedFd) -> Result<OwnedFd> {
    let how = OpenHow {
        flags: (libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC) as u64,
        mode: 0,
        resolve: 0x08 | 0x04,
    };
    let path = CString::new(TARGET)?;
    // SAFETY: ABI-shaped open_how; fixed relative path below pinned root FD.
    let fd = unsafe {
        libc::syscall(
            libc::SYS_openat2,
            root.as_raw_fd(),
            path.as_ptr(),
            &how,
            std::mem::size_of::<OpenHow>(),
        )
    };
    ensure!(
        fd >= 0,
        "unsafe or unavailable lungs target: {}",
        std::io::Error::last_os_error()
    );
    Ok(unsafe { OwnedFd::from_raw_fd(fd as i32) })
}

pub(super) fn attach(name: &str, pid: u32, expected_start: u64, source: &Path) -> Result<()> {
    ensure!(unsafe { libc::geteuid() } == 0, "requires host root");
    let map: Vec<u64> = fs::read_to_string("/proc/self/uid_map")?
        .split_whitespace()
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    ensure!(
        map == [0, 0, 4294967295],
        "requires initial host user namespace"
    );
    ensure!(
        !name.is_empty()
            && name.len() <= 63
            && name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            && name.as_bytes()[0].is_ascii_alphanumeric(),
        "unsafe instance name"
    );
    ensure!(pid > 1 && expected_start > 0, "invalid process pin");
    for (key, expected) in [("LXC_NAME", name.to_owned()), ("LXC_PID", pid.to_string())] {
        if let Ok(actual) = std::env::var(key) {
            ensure!(actual == expected, "hook caller binding changed");
        }
    }
    let state = PathBuf::from(
        std::env::var_os("OP_IDENTITY_SEED_STATE")
            .unwrap_or_else(|| "/var/lib/opdbus-runtime/seed-identities".into()),
    );
    let storage = json(&state.join(name).join("state.json"))?;
    ensure!(
        storage["version"] == 1 && storage["phase"] == "ready" && storage["name"] == name,
        "storage receipt is not ready for instance"
    );
    let session = string(&storage, "session")?;
    ensure!(
        Uuid::parse_str(session)?.to_string() == session,
        "invalid session pin"
    );
    let native_uuid = string(&storage, "instance_uuid")?;
    ensure!(
        Uuid::parse_str(native_uuid)?.to_string() == native_uuid,
        "invalid native UUID pin"
    );
    let native_root = Path::new(string(&storage, "root")?);
    let incus = PathBuf::from(
        std::env::var_os("OP_IDENTITY_INCUS_STATE_DIR").unwrap_or_else(|| "/var/lib/incus".into()),
    );
    ensure!(
        native_root.starts_with(incus.join("storage-pools"))
            && native_root.ends_with(Path::new("containers").join(name).join("rootfs"))
            && fs::canonicalize(native_root)? == native_root,
        "native root pin changed"
    );
    let private = PathBuf::from(
        std::env::var_os("OP_IDENTITY_SEED_LUNGS_DIR")
            .unwrap_or_else(|| "/dev/shm/opdbus-physical-lungs".into()),
    )
    .join(native_uuid);
    ensure!(
        source == private.join("mount"),
        "source is not this native instance's private lungs mount"
    );
    trusted(&private)?;
    let receipt: LungsReceipt = serde_json::from_value(json(&private.join("mount.json"))?)?;
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    ensure!(
        storage["boot_id"] == boot.trim()
            && receipt.boot_id == boot.trim()
            && receipt.session == session
            && receipt.target == source,
        "lungs receipt binding changed"
    );
    let source_fd = fd_open(source, libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW)?;
    ensure!(
        fs_type(&source_fd)? == FUSE_MAGIC
            && metadata(&source_fd)?.ino() == 1
            && mount_id(&source_fd)? == receipt.mount_id.parse::<u64>()?,
        "source mount pin changed"
    );
    // SAFETY: statvfs is an output-only POD buffer.
    let mut flags: libc::statvfs = unsafe { std::mem::zeroed() };
    let required = libc::ST_NOSUID | libc::ST_NODEV | libc::ST_NOEXEC;
    ensure!(
        unsafe { libc::fstatvfs(source_fd.as_raw_fd(), &mut flags) } == 0
            && flags.f_flag & required == required,
        "source lacks required safe mount flags"
    );
    let binary = receipt.args.first().context("missing lungs binary")?;
    trusted(Path::new(binary))?;
    let expected_args = vec![
        binary.clone(),
        "--store".into(),
        private.join("store").to_string_lossy().into_owned(),
        "mount".into(),
        "--allow-other".into(),
        "--auto-unmount".into(),
        "--state".into(),
        private.join("empty-state").to_string_lossy().into_owned(),
        "--lungs-socket".into(),
        "/run/opdbus/lungs.sock".into(),
        "--lungs-session".into(),
        session.into(),
        source.to_string_lossy().into_owned(),
    ];
    ensure!(
        receipt.args == expected_args,
        "lungs arguments do not match session/store/socket"
    );
    let daemon = fd_open(
        Path::new(&format!("/proc/{}", receipt.pid)),
        libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW,
    )?;
    ensure!(
        metadata(&daemon)?.uid() == 0 && start_time(&daemon)? == receipt.start.parse::<u64>()?,
        "lungs daemon process pin changed"
    );
    let cmd = proc_open(&daemon, "cmdline", libc::O_RDONLY | libc::O_NOFOLLOW)?;
    let mut bytes = Vec::new();
    File::from(cmd).take(16385).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 16384, "oversized lungs command line");
    let actual: Vec<&str> = std::str::from_utf8(&bytes)?
        .trim_end_matches('\0')
        .split('\0')
        .collect();
    ensure!(
        actual
            .iter()
            .copied()
            .eq(expected_args.iter().map(String::as_str)),
        "lungs daemon arguments changed"
    );
    ensure!(
        fs::read_link(format!("/proc/{}/exe", receipt.pid))? == Path::new(binary),
        "lungs daemon executable changed"
    );
    let proc = fd_open(
        Path::new(&format!("/proc/{pid}")),
        libc::O_PATH | libc::O_DIRECTORY | libc::O_NOFOLLOW,
    )?;
    // SAFETY: pidfd_open has scalar arguments; successful FD is uniquely owned.
    let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    ensure!(
        pidfd >= 0,
        "pidfd unavailable: {}",
        std::io::Error::last_os_error()
    );
    let pidfd = unsafe { OwnedFd::from_raw_fd(pidfd as i32) };
    alive(&pidfd)?;
    ensure!(
        start_time(&proc)? == expected_start,
        "target PID was reused"
    );
    let namespace = proc_open(&proc, "ns/mnt", libc::O_RDONLY)?;
    let host_namespace = fd_open(Path::new("/proc/self/ns/mnt"), libc::O_RDONLY)?;
    let guest_ns = metadata(&namespace)?;
    let host_ns = metadata(&host_namespace)?;
    ensure!(
        (guest_ns.dev(), guest_ns.ino()) != (host_ns.dev(), host_ns.ino()),
        "target is the host mount namespace"
    );
    let root = proc_open(&proc, "root", libc::O_RDONLY | libc::O_DIRECTORY)?;
    let host_root = fd_open(
        native_root,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW,
    )?;
    let a = metadata(&root)?;
    let b = metadata(&host_root)?;
    ensure!(
        (a.dev(), a.ino()) == (b.dev(), b.ino()) && fs_type(&root)? == BTRFS_MAGIC,
        "target root does not match instance storage"
    );
    let mut info = [0u8; 1024];
    // SAFETY: exact 1024-byte kernel FS_INFO output ABI, aligned to u64 below.
    let mut aligned = [0u64; 128];
    ensure!(
        unsafe { libc::ioctl(root.as_raw_fd(), FS_INFO, aligned.as_mut_ptr()) } == 0,
        "Btrfs root identity unavailable"
    );
    for (i, value) in aligned.iter().enumerate() {
        info[i * 8..i * 8 + 8].copy_from_slice(&value.to_ne_bytes());
    }
    ensure!(
        info[16..32] == *Uuid::parse_str(string(&storage, "sprout_fsid")?)?.as_bytes(),
        "target sprout FSID changed"
    );
    let destination = target(&root)?;
    ensure!(
        mount_id(&destination)? == mount_id(&root)? && fs_type(&destination)? == BTRFS_MAGIC,
        "target already has a child mount"
    );
    ensure!(
        fs::read_dir(format!("/proc/self/fd/{}", destination.as_raw_fd()))?
            .next()
            .is_none(),
        "target directory is not empty"
    );
    // Clone while still in the host mount namespace. A source path from the
    // old namespace cannot be cloned after setns (EINVAL on this host).
    let detached = unsafe {
        libc::syscall(
            libc::SYS_open_tree,
            source_fd.as_raw_fd(),
            c"".as_ptr(),
            1u32 | libc::O_CLOEXEC as u32 | libc::AT_EMPTY_PATH as u32,
        )
    };
    ensure!(
        detached >= 0,
        "host FUSE clone failed: {}",
        std::io::Error::last_os_error()
    );
    let detached = unsafe { OwnedFd::from_raw_fd(detached as i32) };
    alive(&pidfd)?;
    ensure!(
        start_time(&proc)? == expected_start,
        "target process changed before attach"
    );
    // SAFETY: held namespace FD; enter mount namespace only, preserving host
    // user/PID namespace and credentials. Both mount endpoints are pinned FDs.
    ensure!(
        unsafe { libc::setns(namespace.as_raw_fd(), libc::CLONE_NEWNS) } == 0,
        "enter target mount namespace failed: {}",
        std::io::Error::last_os_error()
    );
    let moved = unsafe {
        libc::syscall(
            libc::SYS_move_mount,
            detached.as_raw_fd(),
            c"".as_ptr(),
            destination.as_raw_fd(),
            c"".as_ptr(),
            0x04u32 | 0x40u32,
        )
    };
    if moved < 0 {
        bail!(
            "attach scoped FUSE mount failed: {}",
            std::io::Error::last_os_error()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn target_fd_rejects_symlink_escape_and_pins_real_directory() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("opt/run-mounts")).unwrap();
        let path = root.path().join(TARGET);
        symlink(outside.path(), &path).unwrap();
        let root_fd = fd_open(root.path(), libc::O_RDONLY | libc::O_DIRECTORY).unwrap();
        assert!(
            target(&root_fd).is_err(),
            "guest symlink must not redirect host attach"
        );
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let pinned = target(&root_fd).unwrap();
        let before = metadata(&pinned).unwrap();
        fs::rename(&path, root.path().join("original-target")).unwrap();
        symlink(outside.path(), &path).unwrap();
        let after = metadata(&pinned).unwrap();
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
        assert!(
            target(&root_fd).is_err(),
            "replaced pathname must not redirect pinned FD"
        );
    }
}
