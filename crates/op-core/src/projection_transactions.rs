//! Locked publication/read transactions for the canonical raw SHM state tree.
//!
//! The existing manifest is the commit marker. `publishing=true` means an
//! interrupted transaction is not readable; no reader may invent a committed
//! state from those files. This layer does not decide plugin existence: callers
//! select IDs using the manifest-pinned sealed catalog, never a directory scan.

use anyhow::{Context, Result, bail, ensure};
use nix::{
    errno::Errno,
    fcntl::{Flock, FlockArg, OFlag},
    sys::stat::Mode,
    unistd::{Gid, UnlinkatFlags},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, PermissionsExt},
    },
    path::{Component, Path},
    sync::atomic::Ordering,
};

#[path = "private_projection_directory.rs"]
mod private_directory;
pub use private_directory::{
    PrivateDocumentCommit, PrivateDocumentPublication, PrivateProjectionDirectory,
};

const LOCK_NAME: &str = ".projection.lock";
const MANIFEST_NAME: &str = ".manifest.json";
const MAX_MANIFEST: u64 = 4096;
const MAX_PROJECTION: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ProjectionSnapshot {
    pub generation: u64,
    pub states: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct ProjectionCommit {
    pub generation: u64,
    pub previous: Option<Vec<u8>>,
    pub current: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    generation: u64,
    #[serde(default)]
    publishing: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    credential_pending: Option<PendingCredentialPublication>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    document_pending: Option<PendingDocumentPublication>,
}

/// Private document publication gate in the same manifest as committed state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingDocumentPublication {
    pub plugin_id: String,
    pub operation_id: String,
}

/// A commit gate in the existing manifest, never a credential or identity store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingCredentialPublication {
    pub plugin_id: String,
    pub operation_id: String,
}

impl Manifest {
    fn require_available(&self, plugin_id: &str) -> Result<()> {
        ensure!(
            !self
                .document_pending
                .as_ref()
                .is_some_and(|pending| pending.plugin_id == plugin_id),
            "private document publication is pending; exact-operation durable recovery is required"
        );
        ensure!(
            !self
                .credential_pending
                .as_ref()
                .is_some_and(|pending| pending.plugin_id == plugin_id),
            "identity publication is pending; exact-operation durable recovery is required"
        );
        Ok(())
    }
}

struct LockedDirectory {
    directory: File,
    _lock: Flock<File>,
}

fn open_at(directory: &File, name: &std::ffi::OsStr, flags: OFlag, mode: u32) -> nix::Result<File> {
    let raw = nix::fcntl::openat(
        Some(directory.as_raw_fd()),
        name,
        flags | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
        Mode::from_bits_truncate(mode),
    )?;
    // SAFETY: nix 0.28 returns a newly owned descriptor on success. Ownership
    // transfers exactly once to File, which closes it on every exit path.
    Ok(unsafe { File::from_raw_fd(raw) })
}

fn directory(path: &Path, create: bool, mode: u32) -> Result<Option<File>> {
    ensure!(
        path.is_absolute() && path != Path::new("/"),
        "projection directory must be absolute and not the filesystem root"
    );
    ensure!(
        !path
            .to_string_lossy()
            .split('/')
            .any(|part| part == "." || part == ".."),
        "projection directory must not contain dot components"
    );
    let mut current = File::open("/")?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                let opened = open_at(&current, name, OFlag::O_RDONLY | OFlag::O_DIRECTORY, 0);
                current = match opened {
                    Ok(file) => file,
                    Err(Errno::ENOENT) if create => {
                        match nix::sys::stat::mkdirat(
                            Some(current.as_raw_fd()),
                            name,
                            Mode::from_bits_truncate(mode),
                        ) {
                            Ok(()) | Err(Errno::EEXIST) => {}
                            Err(error) => return Err(error.into()),
                        }
                        open_at(&current, name, OFlag::O_RDONLY | OFlag::O_DIRECTORY, 0)?
                    }
                    Err(Errno::ENOENT) => return Ok(None),
                    Err(error) => {
                        return Err(error)
                            .context("projection directory must have no symlink components");
                    }
                };
            }
            _ => bail!("projection directory must not contain dot components"),
        }
    }
    let metadata = current.metadata()?;
    ensure!(
        trusted_directory_owner(metadata.uid(), nix::unistd::geteuid().as_raw(), create),
        "projection directory must be owned by the writing service user or trusted root reader source"
    );
    ensure!(
        metadata.mode() & 0o022 == 0,
        "projection directory must not be writable by other users or groups"
    );
    Ok(Some(current))
}

fn trusted_directory_owner(owner: u32, caller: u32, writer: bool) -> bool {
    owner == caller || (!writer && owner == 0)
}

fn check_file(file: &File, private: bool, owner: u32) -> Result<()> {
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.nlink() == 1,
        "projection file must be a regular file with one link"
    );
    ensure!(
        metadata.uid() == owner,
        "projection file must be owned by its trusted service directory owner"
    );
    ensure!(
        metadata.mode() & 0o022 == 0,
        "projection file must not be group/world writable"
    );
    if private {
        ensure!(
            metadata.mode() & 0o007 == 0,
            "credential projection must not be world accessible"
        );
    }
    Ok(())
}

fn read_file(directory: &File, name: &str, maximum: u64, private: bool) -> Result<Option<Vec<u8>>> {
    let file = match open_at(
        directory,
        name.as_ref(),
        OFlag::O_RDONLY | OFlag::O_NONBLOCK,
        0,
    ) {
        Ok(file) => file,
        Err(Errno::ENOENT) => return Ok(None),
        Err(error) => return Err(error).context("open canonical projection file without symlinks"),
    };
    check_file(&file, private, directory.metadata()?.uid())?;
    ensure!(
        file.metadata()?.len() <= maximum,
        "projection file exceeds its size limit"
    );
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= maximum,
        "projection file grew beyond its size limit"
    );
    Ok(Some(bytes))
}

impl LockedDirectory {
    fn open(state_dir: &str, exclusive: bool) -> Result<Option<Self>> {
        let Some(directory) = directory(Path::new(state_dir), exclusive, 0o755)? else {
            return Ok(None);
        };
        Self::from_directory(directory, exclusive)
    }

    fn from_directory(directory: File, exclusive: bool) -> Result<Option<Self>> {
        let flags = if exclusive {
            OFlag::O_RDWR | OFlag::O_CREAT | OFlag::O_NONBLOCK
        } else {
            OFlag::O_RDONLY | OFlag::O_NONBLOCK
        };
        let lock = match open_at(&directory, LOCK_NAME.as_ref(), flags, 0o600) {
            Ok(file) => file,
            Err(Errno::ENOENT) if !exclusive => {
                ensure!(
                    read_file(&directory, MANIFEST_NAME, MAX_MANIFEST, false)?.is_none(),
                    "projection manifest exists without its lock; writer must prepare the projection directory before readers start"
                );
                return Ok(None);
            }
            Err(error) => return Err(error).context("open private projection lock"),
        };
        check_file(&lock, true, directory.metadata()?.uid())?;
        ensure!(
            matches!(lock.metadata()?.mode() & 0o777, 0o600 | 0o640),
            "projection lock must have service-private 0600 or protected-group 0640 permissions"
        );
        let inode = lock.metadata()?;
        let lock = Flock::lock(
            lock,
            if exclusive {
                FlockArg::LockExclusive
            } else {
                FlockArg::LockShared
            },
        )
        .map_err(|(_, error)| anyhow::anyhow!("lock projection transaction: {error}"))?;
        let current = open_at(
            &directory,
            LOCK_NAME.as_ref(),
            OFlag::O_RDONLY | OFlag::O_NONBLOCK,
            0,
        )?;
        let current = current.metadata()?;
        ensure!(
            inode.dev() == current.dev() && inode.ino() == current.ino(),
            "projection lock was replaced while waiting"
        );
        Ok(Some(Self {
            directory,
            _lock: lock,
        }))
    }

    fn manifest(&self) -> Result<Option<(Manifest, Vec<u8>)>> {
        let Some(bytes) = read_file(&self.directory, MANIFEST_NAME, MAX_MANIFEST, false)? else {
            return Ok(None);
        };
        let mut parsed = bytes.clone();
        let manifest: Manifest =
            simd_json::from_slice(&mut parsed).context("invalid projection commit manifest")?;
        if let Some(pending) = &manifest.credential_pending {
            ensure!(
                pending.plugin_id == "identity_sled",
                "invalid pending credential plugin"
            );
            identity_transaction::validate_operation_id(&pending.operation_id)?;
        }
        if let Some(pending) = &manifest.document_pending {
            filename(&pending.plugin_id)?;
            ensure!(
                pending.plugin_id != "identity_sled",
                "identity cannot use the private document gate"
            );
            identity_transaction::validate_operation_id(&pending.operation_id)?;
        }
        ensure!(
            !manifest.publishing,
            "projection transaction is incomplete; operator recovery is required before reading or updating state"
        );
        Ok(Some((manifest, bytes)))
    }

    fn enable_credential_readers(&self, credential_gid: u32) -> Result<()> {
        // Identity writers grant the existing protected group shared-lock
        // access; no second lock or credential authority is introduced.
        // Revoke the old group's access before changing group ownership.
        self._lock
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
        nix::unistd::fchown(
            self._lock.as_raw_fd(),
            None,
            Some(Gid::from_raw(credential_gid)),
        )?;
        self._lock
            .set_permissions(std::fs::Permissions::from_mode(0o640))?;
        Ok(())
    }
}

fn filename(plugin_id: &str) -> Result<String> {
    ensure!(
        !plugin_id.is_empty()
            && plugin_id.len() <= 200
            && plugin_id.as_bytes()[0].is_ascii_alphanumeric()
            && plugin_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')),
        "invalid flat plugin identifier"
    );
    Ok(format!("{plugin_id}.json"))
}

fn remove_file(directory: &File, name: &str) -> Result<()> {
    match nix::unistd::unlinkat(
        Some(directory.as_raw_fd()),
        name,
        UnlinkatFlags::NoRemoveDir,
    ) {
        Ok(()) | Err(Errno::ENOENT) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn write_file(
    directory: &File,
    name: &str,
    bytes: &[u8],
    mode: u32,
    gid: Option<u32>,
) -> Result<()> {
    #[cfg(test)]
    tests::write_fault(name)?;
    let mut allocated = None;
    for _ in 0..32 {
        let sequence = super::TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = format!(".{name}.tmp.{}.{}", std::process::id(), sequence);
        match open_at(
            directory,
            candidate.as_ref(),
            OFlag::O_WRONLY | OFlag::O_CREAT | OFlag::O_EXCL,
            mode,
        ) {
            Ok(file) => {
                allocated = Some((candidate, file));
                break;
            }
            Err(Errno::EEXIST) => {}
            Err(error) => return Err(error.into()),
        }
    }
    let (temporary, mut file) =
        allocated.context("cannot allocate private projection temporary file")?;
    let result = (|| -> Result<()> {
        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        if let Some(gid) = gid {
            nix::unistd::fchown(file.as_raw_fd(), None, Some(Gid::from_raw(gid)))?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // Rename is the last fallible step. In particular, no error after a
        // successful commit-manifest rename may incorrectly trigger rollback.
        nix::fcntl::renameat(
            Some(directory.as_raw_fd()),
            temporary.as_str(),
            Some(directory.as_raw_fd()),
            name,
        )?;
        Ok(())
    })();
    if result.is_err() {
        let _ = remove_file(directory, &temporary);
    }
    result
}

fn restore_file(
    directory: &File,
    name: &str,
    previous: Option<&[u8]>,
    mode: u32,
    gid: Option<u32>,
) -> Result<()> {
    match previous {
        Some(bytes) => write_file(directory, name, bytes, mode, gid),
        None => remove_file(directory, name),
    }
}

/// Writer-startup preparation only. Generic readers never create the lock or
/// a directory. This also enables locked reads of existing canonical raw state
/// and generation manifests after upgrading from the previous implementation.
pub fn prepare_projection_directory_in(state_dir: &str) -> Result<()> {
    let _lock = LockedDirectory::open(state_dir, true)?.context("projection directory missing")?;
    Ok(())
}

/// Read a consistent raw state snapshot for IDs already selected by the
/// caller's sealed catalog. No enumeration, credential read or retired
/// fallback. Service readers must apply the sealed exposure policy before
/// sending these bytes over a public transport.
pub fn read_projection_snapshot_in(
    state_dir: &str,
    plugin_ids: &[String],
) -> Result<ProjectionSnapshot> {
    read_projection_snapshot_bounded_in(state_dir, plugin_ids, u64::MAX)
}

/// Consistent snapshot with an aggregate byte budget, checked before each read.
/// An over-budget snapshot fails as a whole; it never looks like missing state.
pub fn read_projection_snapshot_bounded_in(
    state_dir: &str,
    plugin_ids: &[String],
    maximum_bytes: u64,
) -> Result<ProjectionSnapshot> {
    for plugin in plugin_ids {
        filename(plugin)?;
    }
    let Some(locked) = LockedDirectory::open(state_dir, false)? else {
        return Ok(ProjectionSnapshot {
            generation: 0,
            states: BTreeMap::new(),
        });
    };
    read_locked_snapshot_bounded(&locked, plugin_ids, maximum_bytes)
}

fn read_locked_snapshot(
    locked: &LockedDirectory,
    plugin_ids: &[String],
) -> Result<ProjectionSnapshot> {
    read_locked_snapshot_bounded(locked, plugin_ids, u64::MAX)
}

fn read_locked_snapshot_bounded(
    locked: &LockedDirectory,
    plugin_ids: &[String],
    maximum_bytes: u64,
) -> Result<ProjectionSnapshot> {
    let Some((manifest, _)) = locked.manifest()? else {
        // Uncommitted files beside a missing manifest are not state authority.
        return Ok(ProjectionSnapshot {
            generation: 0,
            states: BTreeMap::new(),
        });
    };
    let mut states = BTreeMap::new();
    let mut remaining = maximum_bytes;
    for plugin in plugin_ids {
        manifest.require_available(plugin)?;
        if let Some(bytes) = read_file(
            &locked.directory,
            &filename(plugin)?,
            MAX_PROJECTION.min(remaining),
            false,
        )? {
            remaining = remaining
                .checked_sub(bytes.len() as u64)
                .context("projection snapshot exceeds aggregate byte budget")?;
            states.insert(plugin.clone(), bytes);
        }
    }
    Ok(ProjectionSnapshot {
        generation: manifest.generation,
        states,
    })
}

pub fn read_projection_snapshot(plugin_ids: &[String]) -> Result<ProjectionSnapshot> {
    read_projection_snapshot_in(&super::shm_state_dir(), plugin_ids)
}

type CredentialPairPublication<'a> = (&'a File, Option<Vec<u8>>, Option<Vec<u8>>, u32);

fn publish(
    locked: &LockedDirectory,
    old_manifest: Option<(Manifest, Vec<u8>)>,
    name: &str,
    previous: Option<Vec<u8>>,
    current: Option<Vec<u8>>,
    credential: Option<CredentialPairPublication<'_>>,
) -> Result<ProjectionCommit> {
    let projection_mode = if credential.is_some() { 0o644 } else { 0o600 };
    if let Some(bytes) = &current {
        ensure!(
            bytes.len() as u64 <= MAX_PROJECTION,
            "projection exceeds size limit"
        );
    }
    if let Some((_, _, Some(bytes), _)) = &credential {
        ensure!(
            bytes.len() as u64 <= MAX_PROJECTION,
            "credential projection exceeds size limit"
        );
    }
    let old_generation = old_manifest
        .as_ref()
        .map_or(0, |(manifest, _)| manifest.generation);
    let private_changed = credential
        .as_ref()
        .is_some_and(|(_, old, new, _)| old != new);
    if previous == current && !private_changed {
        return Ok(ProjectionCommit {
            generation: old_generation,
            previous,
            current,
        });
    }
    let generation = old_generation
        .checked_add(1)
        .context("projection generation overflow")?;
    let intent = simd_json::to_vec(&Manifest {
        generation: old_generation,
        publishing: true,
        credential_pending: old_manifest
            .as_ref()
            .and_then(|(manifest, _)| manifest.credential_pending.clone()),
        document_pending: old_manifest
            .as_ref()
            .and_then(|(manifest, _)| manifest.document_pending.clone()),
    })?;
    write_file(&locked.directory, MANIFEST_NAME, &intent, 0o644, None)?;
    let result = (|| -> Result<()> {
        if let Some((directory, _, bytes, gid)) = &credential {
            restore_file(directory, name, bytes.as_deref(), 0o640, Some(*gid))?;
        }
        restore_file(
            &locked.directory,
            name,
            current.as_deref(),
            projection_mode,
            None,
        )?;
        let committed = simd_json::to_vec(&Manifest {
            generation,
            publishing: false,
            credential_pending: old_manifest
                .as_ref()
                .and_then(|(manifest, _)| manifest.credential_pending.clone()),
            document_pending: old_manifest
                .as_ref()
                .and_then(|(manifest, _)| manifest.document_pending.clone()),
        })?;
        write_file(&locked.directory, MANIFEST_NAME, &committed, 0o644, None)?;
        Ok(())
    })();
    if let Err(error) = result {
        let restore_public = restore_file(
            &locked.directory,
            name,
            previous.as_deref(),
            projection_mode,
            None,
        );
        let restore_private = match &credential {
            Some((directory, previous, _, gid)) => {
                restore_file(directory, name, previous.as_deref(), 0o640, Some(*gid))
            }
            None => Ok(()),
        };
        if restore_public.is_ok() && restore_private.is_ok() {
            restore_file(&locked.directory, MANIFEST_NAME, old_manifest.as_ref().map(|(_, bytes)| bytes.as_slice()), 0o644, None)
                .context("projection rollback restored files but could not restore the manifest; state remains unreadable")?;
            return Err(error)
                .context("projection publication failed; previous committed state restored");
        }
        bail!(
            "projection publication failed ({error}); rollback incomplete (public={restore_public:?}, private={restore_private:?}); in-progress manifest retained and state is unreadable"
        )
    }
    Ok(ProjectionCommit {
        generation,
        previous,
        current,
    })
}

/// Fold opaque JSON bytes under the same exclusive filesystem lock used for
/// publication. The closure must be synchronous, side-effect-free, and must not
/// call another projection helper (locks are intentionally not reentrant).
/// Returning None deletes only this public projection; credential bytes are
/// never read or deleted by the generic API.
pub fn update_projection_in(
    state_dir: &str,
    plugin_id: &str,
    fold: impl FnOnce(Option<&[u8]>) -> Result<Option<Vec<u8>>>,
) -> Result<ProjectionCommit> {
    let name = filename(plugin_id)?;
    let locked = LockedDirectory::open(state_dir, true)?.context("projection directory missing")?;
    update_locked_projection(&locked, plugin_id, &name, fold)
}

fn update_locked_projection(
    locked: &LockedDirectory,
    plugin_id: &str,
    name: &str,
    fold: impl FnOnce(Option<&[u8]>) -> Result<Option<Vec<u8>>>,
) -> Result<ProjectionCommit> {
    let manifest = locked.manifest()?;
    if let Some((manifest, _)) = &manifest {
        manifest.require_available(plugin_id)?;
    }
    restrict_file_mode(&locked.directory, name)?;
    let previous = read_file(&locked.directory, name, MAX_PROJECTION, false)?;
    ensure!(
        manifest.is_some() || previous.is_none(),
        "uncommitted projection file exists without a manifest; refusing to overwrite it"
    );
    let current = fold(previous.as_deref())?;
    publish(locked, manifest, name, previous, current, None)
}

fn restrict_file_mode(directory: &File, name: &str) -> Result<()> {
    let file = match open_at(
        directory,
        name.as_ref(),
        OFlag::O_RDONLY | OFlag::O_NONBLOCK,
        0,
    ) {
        Ok(file) => file,
        Err(Errno::ENOENT) => return Ok(()),
        Err(error) => return Err(error).context("open raw state for service-only mode repair"),
    };
    check_file(&file, false, directory.metadata()?.uid())?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

/// Upgrade one caller-selected, manifest-pinned raw state file to service-only
/// access without changing bytes or generation. No scans or group changes.
/// The identity pair has a separate redacted public file and must use its
/// dedicated publication path instead.
pub fn restrict_projection_file_in(state_dir: &str, plugin_id: &str) -> Result<()> {
    ensure!(
        plugin_id != "identity_sled",
        "identity public metadata uses paired publication permissions"
    );
    let name = filename(plugin_id)?;
    let locked = LockedDirectory::open(state_dir, true)?.context("projection directory missing")?;
    locked.manifest()?;
    restrict_file_mode(&locked.directory, &name)
}

pub fn update_projection(
    plugin_id: &str,
    fold: impl FnOnce(Option<&[u8]>) -> Result<Option<Vec<u8>>>,
) -> Result<ProjectionCommit> {
    update_projection_in(&super::shm_state_dir(), plugin_id, fold)
}

/// Publish public redacted and protected credential projections as one commit.
/// The closure sees PRIVATE prior bytes and returns Some((public, private)),
/// or None to delete both files. The returned receipt and all generic readers
/// contain only public bytes. Identical pairs do not advance the generation.
pub fn update_projection_with_credentials_in(
    state_dir: &str,
    plugin_id: &str,
    credential_gid: u32,
    fold: impl FnOnce(Option<&[u8]>) -> Result<Option<(Vec<u8>, Vec<u8>)>>,
) -> Result<ProjectionCommit> {
    let name = filename(plugin_id)?;
    let locked = LockedDirectory::open(state_dir, true)?.context("projection directory missing")?;
    let manifest = locked.manifest()?;
    if let Some((manifest, _)) = &manifest {
        manifest.require_available(plugin_id)?;
    }
    let private_path = super::credential_projection_dir_for_state_dir(state_dir);
    locked.enable_credential_readers(credential_gid)?;
    let private = directory(Path::new(&private_path), true, 0o750)?
        .context("credential directory missing")?;
    private.set_permissions(std::fs::Permissions::from_mode(0o750))?;
    nix::unistd::fchown(
        private.as_raw_fd(),
        None,
        Some(Gid::from_raw(credential_gid)),
    )?;
    let previous = read_file(&locked.directory, &name, MAX_PROJECTION, false)?;
    let old_private = read_file(&private, &name, MAX_PROJECTION, true)?;
    ensure!(
        manifest.is_some() || (previous.is_none() && old_private.is_none()),
        "uncommitted identity projection exists without a manifest; refusing to overwrite it"
    );
    let (public, credential) = match fold(old_private.as_deref())? {
        Some((public, credential)) => (Some(public), Some(credential)),
        None => (None, None),
    };
    publish(
        &locked,
        manifest,
        &name,
        previous,
        public,
        Some((&private, old_private, credential, credential_gid)),
    )
}

pub(super) fn read_credential_in(state_dir: &str, plugin_id: &str) -> Result<Option<Vec<u8>>> {
    let name = filename(plugin_id)?;
    let Some(locked) = LockedDirectory::open(state_dir, false)? else {
        return Ok(None);
    };
    let Some((manifest, _)) = locked.manifest()? else {
        return Ok(None);
    };
    manifest.require_available(plugin_id)?;
    let private_path = super::credential_projection_dir_for_state_dir(state_dir);
    let Some(private) = directory(Path::new(&private_path), false, 0)? else {
        return Ok(None);
    };
    ensure!(
        private.metadata()?.mode() & 0o007 == 0,
        "credential directory must not be world accessible"
    );
    read_file(&private, &name, MAX_PROJECTION, true)
}

#[path = "identity_projection_transaction.rs"]
mod identity_transaction;
pub use identity_transaction::{
    CredentialPublication, commit_identity_projection_durable_in, pending_identity_publication_in,
    recover_identity_projection_in,
};

#[cfg(test)]
#[path = "projection_transactions_tests.rs"]
mod tests;
