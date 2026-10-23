//! Shared-memory projection layer for the projected plugin tree.
//!
//! The projected plugin tree lives in tmpfs (`/dev/shm/opdbus/state/`).
//! The MutationEngine writes one file per plugin on every mutation (atomic
//! temp+rename). These helpers read the raw JSON files without a held cache or
//! polling, and cooperate through one private service-owned advisory lock
//! around data plus manifest access.
//!
//! This module is the shared contract between the **writer** (op-grpc-bridge
//! MutationEngine, the single write door) and the **readers**. The old
//! op-projection D-Bus server is deleted. Retired projection paths are never a
//! fallback or a source of plugin existence.
//!
//! General raw state files are service-only (0600). Their location does not
//! authorize outward disclosure: wire consumers must apply the exact sealed
//! plugin's state-exposure policy. Identity has a separately redacted public
//! file (0644) paired transactionally with its protected credential file.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "projection_transactions.rs"]
mod transactions;
pub use transactions::{
    CredentialPublication, PendingCredentialPublication, PendingDocumentPublication,
    PrivateDocumentCommit, PrivateDocumentPublication, PrivateProjectionDirectory,
    ProjectionCommit, ProjectionSnapshot, commit_identity_projection_durable_in,
    pending_identity_publication_in, prepare_projection_directory_in, read_projection_snapshot,
    read_projection_snapshot_bounded_in, read_projection_snapshot_in,
    recover_identity_projection_in, restrict_projection_file_in, update_projection,
    update_projection_in, update_projection_with_credentials_in,
};

/// Canonical directory for per-plugin present-state files (one JSON file per
/// plugin). This is the static tree that `write_projection` maintains and all
/// consumers read.
pub const SHM_STATE_DIR: &str = "/dev/shm/opdbus/state";
/// Private tmpfs directory for local possession credentials. Files here are
/// not part of the generic state tree and must never be served by schema
/// routing, state sync, snapshots, or web projection readers.
pub const SHM_CREDENTIAL_DIR: &str = "/dev/shm/opdbus/credentials";
/// Retired path constant retained only for source references. Never read as a
/// fallback by the current projection API.
pub const SHM_PROJECTION_DIR: &str = "/dev/shm/opdbus/projections";

/// Manifest carrying the monotonic `generation` counter. Written LAST as the
/// atomic commit point after each projection write. Readers hold a shared lock
/// across manifest and data reads and reject an in-progress transaction.
pub const SHM_PROJECTION_MANIFEST: &str = "/dev/shm/opdbus/state/.manifest.json";

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// The state tree root: the canonical [`SHM_STATE_DIR`] in production.
/// `OP_SHM_STATE_DIR` overrides it so tests that drive the MutationEngine
/// never write the live projection tree.
pub fn shm_state_dir() -> String {
    std::env::var("OP_SHM_STATE_DIR").unwrap_or_else(|_| SHM_STATE_DIR.to_string())
}

/// Atomically publish `bytes` to a `/dev/shm` path via a sibling temp file +
/// rename, so readers see either the old or new content, never a torn write.
pub fn atomic_write_shm(path: &str, bytes: &[u8]) -> anyhow::Result<()> {
    atomic_write_shm_with_permissions(path, bytes, 0o644, None)
}

/// Atomically publish a sensitive projection with an exact mode and optional
/// group owner. Permissions are applied to the sibling temporary file before
/// rename, so there is no world-readable interval at the destination.
pub fn atomic_write_shm_with_permissions(
    path: &str,
    bytes: &[u8],
    mode: u32,
    gid: Option<u32>,
) -> anyhow::Result<()> {
    let destination = Path::new(path);
    let parent = destination
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Projection path has no parent: {path}"))?;
    let filename = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("Projection path has no UTF-8 filename: {path}"))?;

    // A deterministic `<destination>.tmp` races when two mutation tasks
    // publish the same plugin concurrently. Use O_EXCL and a process-unique
    // sibling name so each writer owns its bytes through the final rename.
    let (tmp, mut file) = (0..32)
        .find_map(|_| {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let candidate = parent.join(format!(
                ".{filename}.tmp.{}.{}",
                std::process::id(),
                sequence
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(mode)
                .open(&candidate)
            {
                Ok(file) => Some(Ok((candidate, file))),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(error) => Some(Err(anyhow::anyhow!(
                    "Cannot create {}: {}",
                    candidate.display(),
                    error
                ))),
            }
        })
        .transpose()?
        .ok_or_else(|| anyhow::anyhow!("Could not allocate a unique temp file for {path}"))?;

    let publish = (|| -> anyhow::Result<()> {
        file.set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|e| anyhow::anyhow!("Cannot set permissions on {}: {}", tmp.display(), e))?;
        file.write_all(bytes)
            .map_err(|e| anyhow::anyhow!("Failed to write {}: {}", tmp.display(), e))?;
        file.sync_all()
            .map_err(|e| anyhow::anyhow!("Failed to sync {}: {}", tmp.display(), e))?;
        drop(file);
        if let Some(gid) = gid {
            nix::unistd::chown(&tmp, None, Some(nix::unistd::Gid::from_raw(gid)))
                .map_err(|e| anyhow::anyhow!("Cannot set group on {}: {}", tmp.display(), e))?;
        }
        fs::rename(&tmp, destination).map_err(|e| {
            anyhow::anyhow!(
                "Failed to rename {} -> {}: {}",
                tmp.display(),
                destination.display(),
                e
            )
        })?;
        Ok(())
    })();
    if publish.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    publish
}

/// Sanitize a plugin id into a safe flat filename (no path separators or NUL).
fn safe_filename(plugin_id: &str) -> String {
    plugin_id
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c == '\0' {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// File path for a specific plugin's projection (`<state_dir>/<plugin>.json`).
pub fn projection_file_path(plugin_id: &str) -> String {
    projection_file_path_in(&shm_state_dir(), plugin_id)
}

/// File path for a projection under one already-resolved state directory.
/// Writers use this form so a process-global test override cannot change
/// between resolving the directory and publishing the file.
pub fn projection_file_path_in(state_dir: &str, plugin_id: &str) -> String {
    format!("{state_dir}/{}.json", safe_filename(plugin_id))
}

/// Credential directory paired with one already-resolved state tree. Test
/// roots keep credentials beneath the same temporary root; production uses a
/// separate non-enumerated tmpfs directory.
pub fn credential_projection_dir_for_state_dir(state_dir: &str) -> String {
    if Path::new(state_dir) == Path::new(SHM_STATE_DIR) {
        SHM_CREDENTIAL_DIR.to_string()
    } else {
        format!("{state_dir}/.credentials")
    }
}

pub fn credential_projection_dir() -> String {
    credential_projection_dir_for_state_dir(&shm_state_dir())
}

/// Protected credential file for one plugin. This path is intentionally not
/// considered by generic projection readers or plugin enumeration.
pub fn credential_projection_file_path(plugin_id: &str) -> String {
    let directory = credential_projection_dir();
    format!("{directory}/{}.json", safe_filename(plugin_id))
}

pub fn credential_projection_file_path_in(state_dir: &str, plugin_id: &str) -> String {
    let directory = credential_projection_dir_for_state_dir(state_dir);
    format!("{directory}/{}.json", safe_filename(plugin_id))
}

/// Write a plugin's full projected state to shm and bump the manifest
/// generation.
///
/// Called by the MutationEngine for an already-completed state value. Opaque
/// configuration values remain service-only raw state until a schema-owned
/// projector explicitly selects the public wire view.
pub fn write_projection(plugin_id: &str, json_bytes: &[u8]) -> anyhow::Result<()> {
    let commit = update_projection(plugin_id, |_| Ok(Some(json_bytes.to_vec())))?;
    tracing::debug!(
        plugin_id,
        generation = commit.generation,
        "Projection committed to shm"
    );
    Ok(())
}

/// This former API cannot safely commit independently installed files. Callers
/// must use update_projection or update_projection_with_credentials_in.
pub fn bump_manifest_generation() -> anyhow::Result<u64> {
    bump_manifest_generation_in(&shm_state_dir())
}

/// Fail closed rather than certify state written outside a transaction.
pub fn bump_manifest_generation_in(_state_dir: &str) -> anyhow::Result<u64> {
    anyhow::bail!(
        "standalone projection generation bumps are unsafe; use the transactional projection API"
    )
}

/// Read the raw bytes of a plugin's projection from shm.
pub fn read_projection_bytes(plugin_id: &str) -> Option<Vec<u8>> {
    read_projection_snapshot(&[plugin_id.to_string()])
        .ok()?
        .states
        .remove(plugin_id)
}

/// Read a protected local credential projection. There is deliberately no
/// legacy or public-state fallback: callers requesting possession material
/// must fail closed when the private projection is absent.
pub fn read_credential_projection_bytes(plugin_id: &str) -> Option<Vec<u8>> {
    read_credential_projection_bytes_in(&shm_state_dir(), plugin_id)
        .ok()
        .flatten()
}

/// Strict privileged credential read under the shared transaction lock. The
/// caller resolves the state directory once; absence remains distinct from
/// unsafe or incomplete publication. Existing protected-group OS permissions
/// apply. Generic state readers never call this helper.
pub fn read_credential_projection_bytes_in(
    state_dir: &str,
    plugin_id: &str,
) -> anyhow::Result<Option<Vec<u8>>> {
    transactions::read_credential_in(state_dir, plugin_id)
}

/// Legacy best-effort generation read (0 on absence or error). Authoritative
/// consumers use read_projection_snapshot so malformed/incomplete publication
/// remains an error rather than looking like generation zero.
pub fn read_manifest_generation() -> u64 {
    read_projection_snapshot(&[])
        .map(|snapshot| snapshot.generation)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn safe_filename_replaces_separators() {
        assert_eq!(safe_filename("wireguard"), "wireguard");
        assert_eq!(safe_filename("a/b"), "a_b");
        assert_eq!(safe_filename("a\\b"), "a_b");
    }

    #[test]
    fn projection_file_path_under_canonical_dir() {
        assert_eq!(
            projection_file_path("cognitive"),
            "/dev/shm/opdbus/state/cognitive.json"
        );
    }

    #[test]
    fn restricted_atomic_publish_never_uses_world_readable_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("identity_sled.json");
        atomic_write_shm_with_permissions(
            path.to_str().unwrap(),
            br#"{"sleds":[]}"#,
            0o640,
            Some(nix::unistd::Gid::current().as_raw()),
        )
        .unwrap();
        let metadata = std::fs::metadata(path).unwrap();
        assert_eq!(metadata.mode() & 0o777, 0o640);
        assert_eq!(metadata.gid(), nix::unistd::Gid::current().as_raw());
    }

    #[test]
    fn concurrent_atomic_publish_never_shares_a_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let first_path = path.clone();
        let second_path = path.clone();
        let first = std::thread::spawn(move || {
            for _ in 0..64 {
                atomic_write_shm(first_path.to_str().unwrap(), br#"{"writer":"first"}"#).unwrap();
            }
        });
        let second = std::thread::spawn(move || {
            for _ in 0..64 {
                atomic_write_shm(second_path.to_str().unwrap(), br#"{"writer":"second"}"#).unwrap();
            }
        });
        first.join().unwrap();
        second.join().unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes == br#"{"writer":"first"}"# || bytes == br#"{"writer":"second"}"#);
        assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp.")
        }));
    }

    #[test]
    fn test_state_roots_keep_credentials_out_of_the_public_tree() {
        assert_eq!(
            credential_projection_dir_for_state_dir("/tmp/example-state"),
            "/tmp/example-state/.credentials"
        );
        assert_eq!(
            credential_projection_file_path_in("/tmp/example-state", "identity_sled"),
            "/tmp/example-state/.credentials/identity_sled.json"
        );
    }
}
