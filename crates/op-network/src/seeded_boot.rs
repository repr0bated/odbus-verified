//! Boot prerequisite for Incus: mount trusted prepared leaves, never provision
//! devices, write identity records, or start containers.
use std::ffi::{CString, OsStr};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

use crate::seeded_root::{ensure_seeded_root_mounted, SeededRootDescriptor, SeededRootMount};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const CONFIG_DIR: &str = "/etc/op-dbus/identity-storage";
const MAX_CONFIG_BYTES: u64 = 16 * 1024;

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct BootLeaf {
    version: u8,
    session_id: String,
    seed_image: String,
    seed_sha256: String,
    seed_fsid: String,
    leaf_image: String,
    sprout_fsid: String,
    mount_point: String,
    pool_subvolume: String,
}

impl BootLeaf {
    fn descriptor(&self, filename: &OsStr) -> Result<SeededRootDescriptor<'_>> {
        let session = Uuid::parse_str(&self.session_id).context("invalid session UUID")?;
        ensure!(self.version == 1, "unsupported boot descriptor version");
        ensure!(
            session.to_string() == self.session_id
                && filename == OsStr::new(&format!("{session}.json")),
            "boot descriptor filename differs from session"
        );
        ensure!(
            self.seed_sha256.len() == 64
                && self
                    .seed_sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid seed digest"
        );
        for path in [&self.seed_image, &self.leaf_image, &self.mount_point] {
            let path = Path::new(path);
            ensure!(
                path.is_absolute()
                    && path
                        .components()
                        .all(|c| matches!(c, Component::RootDir | Component::Normal(_))),
                "boot descriptor paths must be absolute and normalized"
            );
        }
        ensure!(
            self.pool_subvolume != "persist"
                && !self.pool_subvolume.is_empty()
                && matches!(
                    Path::new(&self.pool_subvolume)
                        .components()
                        .collect::<Vec<_>>()
                        .as_slice(),
                    [Component::Normal(_)]
                ),
            "pool must be one distinct direct subvolume"
        );
        Ok(SeededRootDescriptor {
            seed_image: Path::new(&self.seed_image),
            seed_sha256: &self.seed_sha256,
            seed_fsid: Uuid::parse_str(&self.seed_fsid)?,
            leaf_image: Path::new(&self.leaf_image),
            sprout_fsid: Uuid::parse_str(&self.sprout_fsid)?,
            mount_point: Path::new(&self.mount_point),
            pool_subvolume: &self.pool_subvolume,
        })
    }
}

fn open_at(parent: &File, name: &OsStr, flags: i32) -> std::io::Result<File> {
    let name = CString::new(name.as_bytes())?;
    // SAFETY: parent is an open directory and name is a terminated C string.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
        )
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: this call owns the newly opened descriptor.
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn config_directory(create: bool) -> Result<Option<File>> {
    let mut parent = File::open("/")?;
    for component in Path::new(CONFIG_DIR).components().skip(1) {
        let Component::Normal(name) = component else {
            unreachable!()
        };
        let next = match open_at(&parent, name, libc::O_RDONLY | libc::O_DIRECTORY) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !create {
                    return Ok(None);
                }
                let component = CString::new(name.as_bytes())?;
                // SAFETY: parent is a held directory FD; component is a C string.
                let result =
                    unsafe { libc::mkdirat(parent.as_raw_fd(), component.as_ptr(), 0o700) };
                if result != 0
                    && std::io::Error::last_os_error().kind() != std::io::ErrorKind::AlreadyExists
                {
                    return Err(std::io::Error::last_os_error())
                        .context("create boot descriptor directory");
                }
                parent.sync_all()?;
                open_at(&parent, name, libc::O_RDONLY | libc::O_DIRECTORY)?
            }
            Err(error) => return Err(error).context("open trusted boot descriptor directory"),
        };
        let meta = next.metadata()?;
        ensure!(
            meta.uid() == 0 && meta.mode() & 0o022 == 0,
            "boot descriptor directory is not root-controlled"
        );
        parent = next;
    }
    Ok(Some(parent))
}

fn read_config(parent: &File, name: &OsStr, owner_uid: u32) -> Result<BootLeaf> {
    let file = open_at(parent, name, libc::O_RDONLY | libc::O_NONBLOCK)?;
    let before = file.metadata()?;
    ensure!(
        before.is_file() && before.uid() == owner_uid && before.mode() & 0o022 == 0,
        "boot descriptor is not a trusted regular file"
    );
    ensure!(
        before.len() <= MAX_CONFIG_BYTES,
        "boot descriptor is too large"
    );
    let mut bytes = Vec::new();
    (&file).take(MAX_CONFIG_BYTES + 1).read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    ensure!(
        bytes.len() as u64 <= MAX_CONFIG_BYTES
            && before.len() == after.len()
            && before.mtime() == after.mtime()
            && before.mtime_nsec() == after.mtime_nsec()
            && before.ctime() == after.ctime()
            && before.ctime_nsec() == after.ctime_nsec(),
        "boot descriptor changed while being read"
    );
    let config: BootLeaf = serde_json::from_slice(&bytes)?;
    config.descriptor(name)?;
    Ok(config)
}

/// Unmount the sprout named by one identity boot descriptor.
///
/// A session with no descriptor has no prepared sprout. A mounted sprout is
/// checked against that descriptor, unmounted with a normal `umount(2)`, and
/// only then has its seed and leaf loops detached.
pub fn unmount_session(session_id: &str) -> Result<()> {
    let session =
        Uuid::parse_str(session_id).context("identity unmount requires a session UUID")?;
    let Some(directory) = config_directory(false)? else {
        return Ok(());
    };
    let filename = format!("{session}.json");
    let name = OsStr::new(&filename);
    match open_at(&directory, name, libc::O_RDONLY) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("open identity boot descriptor"),
        Ok(_) => {}
    }
    let config = read_config(&directory, name, 0)?;
    let descriptor = config.descriptor(name)?;
    crate::seeded_root::unmount_seeded_root(&descriptor)
        .with_context(|| format!("unmount identity btrfs for {session}"))
}

/// Mount every trusted boot descriptor. A descriptor is mount configuration,
/// never authorization to start an identity or replace its Cozo binding.
pub fn mount_all() -> Result<usize> {
    let Some(directory) = config_directory(false)? else {
        return Ok(0);
    };
    // Enumerate the pinned directory, rather than reopening its mutable path.
    let mut names = fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))?
        .map(|entry| entry.map(|e| e.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    // A crash before atomic publication may leave our own staging file. It
    // is not a descriptor and must not prevent previously committed leaves
    // from mounting. Every other entry still undergoes strict validation.
    names.retain(|name| !is_staging_name(name));
    names.sort();
    let configs = names
        .iter()
        .map(|name| read_config(&directory, name, 0))
        .collect::<Result<Vec<_>>>()?;
    // Validate every descriptor before the first mount. Native guards perform
    // the actual backing, seed hash, filesystem and subvolume verification.
    for (name, config) in names.iter().zip(&configs) {
        let descriptor = config.descriptor(name)?;
        let guard = ensure_seeded_root_mounted(&descriptor)
            .with_context(|| format!("mount leaf {}", config.session_id))?;
        guard.verify_persist_subvolume(&descriptor, "persist", false)?;
    }
    Ok(configs.len())
}

fn is_staging_name(name: &OsStr) -> bool {
    name.to_str()
        .and_then(|name| name.strip_prefix(".boot-"))
        .and_then(|name| name.strip_suffix(".tmp"))
        .is_some_and(|id| Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id))
}

fn publish_config(directory: &File, name: &OsStr, config: &BootLeaf, owner_uid: u32) -> Result<()> {
    let bytes = serde_json::to_vec(config)?;
    ensure!(
        bytes.len() as u64 <= MAX_CONFIG_BYTES,
        "boot descriptor is too large"
    );
    let temporary = CString::new(format!(".boot-{}.tmp", Uuid::new_v4()))?;
    let destination = CString::new(name.as_bytes())?;
    // SAFETY: names are terminated and directory is held open. O_EXCL ensures
    // this operation never opens an existing file for writing.
    let fd = unsafe {
        libc::openat(
            directory.as_raw_fd(),
            temporary.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    ensure!(
        fd >= 0,
        "create boot descriptor: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: fd was newly opened above and ownership is transferred once.
    let mut file = unsafe { File::from_raw_fd(fd) };
    let result = (|| -> Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        // SAFETY: both names belong to the held directory. NOREPLACE protects
        // a concurrently installed or historical descriptor from replacement.
        let rc = unsafe {
            libc::renameat2(
                directory.as_raw_fd(),
                temporary.as_ptr(),
                directory.as_raw_fd(),
                destination.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if rc != 0 {
            let error = std::io::Error::last_os_error();
            ensure!(
                error.kind() == std::io::ErrorKind::AlreadyExists,
                "publish boot descriptor: {error}"
            );
            ensure!(
                read_config(directory, name, owner_uid)? == *config,
                "existing boot descriptor differs; explicit reconciliation required"
            );
        }
        directory.sync_all()?;
        Ok(())
    })();
    // The random temporary name was created exclusively by this invocation.
    // After a successful rename it is absent; never unlink the destination.
    let cleanup = unsafe { libc::unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0) };
    if cleanup != 0 {
        let error = std::io::Error::last_os_error();
        ensure!(
            error.kind() == std::io::ErrorKind::NotFound,
            "clean boot descriptor temporary file: {error}"
        );
    }
    directory.sync_all()?;
    result
}

/// Install only from a verified, held native mount. The identity owner calls
/// this before its final Cozo commit; journal fencing still controls activation
/// if that later commit fails. Existing differing descriptors are retained.
pub fn install_verified(
    session_id: &str,
    descriptor: &SeededRootDescriptor<'_>,
    mount: &SeededRootMount,
) -> Result<()> {
    // SAFETY: geteuid has no pointer arguments or side effects.
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "boot descriptor installation requires root"
    );
    let config = BootLeaf {
        version: 1,
        session_id: session_id.to_owned(),
        seed_image: descriptor
            .seed_image
            .to_str()
            .context("seed path is not UTF-8")?
            .to_owned(),
        seed_sha256: descriptor.seed_sha256.to_owned(),
        seed_fsid: descriptor.seed_fsid.to_string(),
        leaf_image: descriptor
            .leaf_image
            .to_str()
            .context("leaf path is not UTF-8")?
            .to_owned(),
        sprout_fsid: descriptor.sprout_fsid.to_string(),
        mount_point: descriptor
            .mount_point
            .to_str()
            .context("mount path is not UTF-8")?
            .to_owned(),
        pool_subvolume: descriptor.pool_subvolume.to_owned(),
    };
    let name = format!("{session_id}.json");
    config.descriptor(OsStr::new(&name))?;
    mount.verify_persist_subvolume(descriptor, "persist", false)?;
    let directory = config_directory(true)?.context("boot configuration directory absent")?;
    publish_config(&directory, OsStr::new(&name), &config, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn fixture() -> (String, String) {
        let session = "f036f8d8-aabb-c5f2-49c9-18dac19f41ea";
        let value = serde_json::json!({"version":1,"session_id":session,
            "seed_image":"/var/lib/seed.img","seed_sha256":"a".repeat(64),
            "seed_fsid":session,"leaf_image":"/var/lib/leaf.img","sprout_fsid":session,
            "mount_point":"/mnt/identity","pool_subvolume":"pool"});
        (format!("{session}.json"), value.to_string())
    }

    #[test]
    fn refuses_symlink_and_writable_descriptor_before_parsing() {
        let temp = tempfile::tempdir().unwrap();
        let parent = File::open(temp.path()).unwrap();
        let (name, body) = fixture();
        fs::write(temp.path().join("source"), body).unwrap();
        symlink("source", temp.path().join(&name)).unwrap();
        let owner = parent.metadata().unwrap().uid();
        assert!(read_config(&parent, OsStr::new(&name), owner).is_err());
        fs::remove_file(temp.path().join(&name)).unwrap();
        fs::rename(temp.path().join("source"), temp.path().join(&name)).unwrap();
        fs::set_permissions(temp.path().join(&name), fs::Permissions::from_mode(0o666)).unwrap();
        assert!(read_config(&parent, OsStr::new(&name), owner).is_err());
        fs::set_permissions(temp.path().join(&name), fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_config(&parent, OsStr::new(&name), owner).is_ok());
    }

    #[test]
    fn refuses_foreign_session_and_parent_traversal() {
        let (name, body) = fixture();
        let mut config: BootLeaf = serde_json::from_str(&body).unwrap();
        assert!(config.descriptor(OsStr::new("foreign.json")).is_err());
        config.mount_point = "/var/lib/../host".into();
        assert!(config.descriptor(OsStr::new(&name)).is_err());
        config.mount_point = "/mnt/identity".into();
        config.pool_subvolume = "../pool".into();
        assert!(config.descriptor(OsStr::new(&name)).is_err());
    }

    #[test]
    fn atomic_publication_never_replaces_a_different_descriptor() {
        let temp = tempfile::tempdir().unwrap();
        let directory = File::open(temp.path()).unwrap();
        let owner = directory.metadata().unwrap().uid();
        let (name, body) = fixture();
        let mut config: BootLeaf = serde_json::from_str(&body).unwrap();
        publish_config(&directory, OsStr::new(&name), &config, owner).unwrap();
        let original = fs::read(temp.path().join(&name)).unwrap();
        publish_config(&directory, OsStr::new(&name), &config, owner).unwrap();
        config.leaf_image = "/var/lib/other-leaf.img".into();
        assert!(publish_config(&directory, OsStr::new(&name), &config, owner).is_err());
        assert_eq!(fs::read(temp.path().join(&name)).unwrap(), original);
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[test]
    fn crash_staging_names_are_not_committed_descriptors() {
        assert!(is_staging_name(OsStr::new(
            ".boot-f036f8d8-aabb-c5f2-49c9-18dac19f41ea.tmp"
        )));
        assert!(!is_staging_name(OsStr::new(".boot-invalid.tmp")));
        assert!(!is_staging_name(OsStr::new(
            "f036f8d8-aabb-c5f2-49c9-18dac19f41ea.json"
        )));
    }
}
