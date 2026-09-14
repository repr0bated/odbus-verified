//! Identity's durable-store boundary within the existing SHM manifest/lock.
//! These are trusted writer/recovery APIs, never caller-request entry points.
use super::*;

const IDENTITY: &str = "identity_sled";
const IDENTITY_FILE: &str = "identity_sled.json";
type Pair = Option<(Vec<u8>, Vec<u8>)>;

/// Only Committed may update the engine mirror or emit a state notification.
/// Pending retains the exact durable result when the callback completed, so
/// an owned lifecycle worker can distinguish that from an uncertain failure.
/// Neither path restores an old readable credential after the callback starts.
pub enum CredentialPublication<T> {
    Committed {
        projection: ProjectionCommit,
        durable: T,
    },
    Pending {
        operation_id: String,
        durable: Option<T>,
        error: anyhow::Error,
    },
}

pub(super) fn validate_operation_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "identity publication operation id is invalid"
    );
    Ok(())
}

fn credentials(locked: &LockedDirectory, state_dir: &str, gid: u32) -> Result<File> {
    locked.enable_credential_readers(gid)?;
    let path = super::super::credential_projection_dir_for_state_dir(state_dir);
    let private =
        directory(Path::new(&path), true, 0o750)?.context("credential directory missing")?;
    private.set_permissions(std::fs::Permissions::from_mode(0o750))?;
    nix::unistd::fchown(private.as_raw_fd(), None, Some(Gid::from_raw(gid)))?;
    Ok(private)
}

fn sync_boundary(directory: &File, phase: &str) -> Result<()> {
    #[cfg(test)]
    super::tests::write_fault(phase)?;
    let _ = phase;
    directory.sync_all()?;
    Ok(())
}

fn check_existing_pair_file(directory: &File, private: bool) -> Result<()> {
    match open_at(
        directory,
        IDENTITY_FILE.as_ref(),
        OFlag::O_RDONLY | OFlag::O_NONBLOCK,
        0,
    ) {
        Ok(file) => check_file(&file, private, directory.metadata()?.uid()),
        Err(Errno::ENOENT) => Ok(()),
        Err(error) => Err(error).context("identity recovery refuses unsafe existing file"),
    }
}

/// Read the existing pending marker without reading any credential bytes.
/// Operators use its exact token for a durable reread; it is not authority to
/// choose replacement state or recover a different operation.
pub fn pending_identity_publication_in(
    state_dir: &str,
) -> Result<Option<PendingCredentialPublication>> {
    let Some(locked) = LockedDirectory::open(state_dir, false)? else {
        return Ok(None);
    };
    Ok(locked
        .manifest()?
        .and_then(|(manifest, _)| manifest.credential_pending))
}

/// Synchronous durable callback under the same lock as every strict SHM reader.
/// The caller must run the whole helper in a cancellation-owned worker. The
/// callback must not reenter SHM helpers; it commits/syncs the authoritative
/// identity store and returns the exact full public/private pair to publish.
/// The callback receives the committed PRIVATE predecessor. Validate typed
/// edits/expected revision inside it; an error may mean a DB commit was
/// uncertain, so even a callback error leaves identity unavailable.
pub fn commit_identity_projection_durable_in<T>(
    state_dir: &str,
    credential_gid: u32,
    operation_id: &str,
    commit: impl FnOnce(Option<&[u8]>) -> Result<(Pair, T)>,
) -> Result<CredentialPublication<T>> {
    validate_operation_id(operation_id)?;
    let locked = LockedDirectory::open(state_dir, true)?.context("projection directory missing")?;
    let manifest = locked.manifest()?;
    let mut manifest = manifest.map_or_else(Manifest::default, |(manifest, _)| manifest);
    manifest.require_available(IDENTITY)?;
    // Reserve room before any durable effect, even for a potential no-op.
    manifest
        .generation
        .checked_add(1)
        .context("projection generation overflow")?;
    let private = credentials(&locked, state_dir, credential_gid)?;
    let previous = read_file(&locked.directory, IDENTITY_FILE, MAX_PROJECTION, false)?;
    let old_private = read_file(&private, IDENTITY_FILE, MAX_PROJECTION, true)?;
    // Existing pairs always require a commit manifest; a raw orphan is not input.
    ensure!(
        locked.manifest()?.is_some() || (previous.is_none() && old_private.is_none()),
        "uncommitted identity files exist without a manifest"
    );
    manifest.credential_pending = Some(PendingCredentialPublication {
        plugin_id: IDENTITY.into(),
        operation_id: operation_id.into(),
    });
    write_file(
        &locked.directory,
        MANIFEST_NAME,
        &simd_json::to_vec(&manifest)?,
        0o644,
        None,
    )?;
    // Marker file data was fsynced by write_file; sync the containing directory
    // BEFORE calling Cozo so a crash cannot recover the old readable marker.
    // Failure here never executes commit; a visible pending marker is retained.
    if let Err(error) = sync_boundary(&locked.directory, "identity_marker_sync") {
        return Ok(CredentialPublication::Pending {
            operation_id: operation_id.into(),
            durable: None,
            error: error.context("sync identity pending marker before durable callback"),
        });
    }
    match commit(old_private.as_deref()) {
        Ok((pair, durable)) => Ok(finish(
            &locked,
            &private,
            manifest,
            previous,
            old_private,
            pair,
            credential_gid,
            durable,
            false,
        )),
        Err(error) => Ok(CredentialPublication::Pending {
            operation_id: operation_id.into(),
            durable: None,
            error,
        }),
    }
}

/// Recover only the exact pending operation by rereading the authoritative
/// durable store. The callback never sees possibly mixed SHM files. There is
/// no "clear marker" or caller-supplied-old-state rollback API. Failure/panic
/// leaves the same token pending. Recovery is explicit, never a read fallback.
pub fn recover_identity_projection_in<T>(
    state_dir: &str,
    credential_gid: u32,
    operation_id: &str,
    load_durable: impl FnOnce() -> Result<(Pair, T)>,
) -> Result<CredentialPublication<T>> {
    validate_operation_id(operation_id)?;
    let locked = LockedDirectory::open(state_dir, true)?.context("projection directory missing")?;
    let (manifest, _) = locked
        .manifest()?
        .context("identity recovery requires a commit manifest")?;
    ensure!(
        manifest.credential_pending.as_ref().is_some_and(
            |pending| pending.plugin_id == IDENTITY && pending.operation_id == operation_id
        ),
        "identity recovery requires the exact pending operation id"
    );
    manifest
        .generation
        .checked_add(1)
        .context("projection generation overflow")?;
    let private = credentials(&locked, state_dir, credential_gid)?;
    check_existing_pair_file(&locked.directory, false)?;
    check_existing_pair_file(&private, true)?;
    match load_durable() {
        Ok((pair, durable)) => Ok(finish(
            &locked,
            &private,
            manifest,
            None,
            None,
            pair,
            credential_gid,
            durable,
            true,
        )),
        Err(error) => Ok(CredentialPublication::Pending {
            operation_id: operation_id.into(),
            durable: None,
            error,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn finish<T>(
    locked: &LockedDirectory,
    private: &File,
    mut manifest: Manifest,
    previous: Option<Vec<u8>>,
    old_private: Option<Vec<u8>>,
    pair: Pair,
    gid: u32,
    durable: T,
    recovery: bool,
) -> CredentialPublication<T> {
    let operation_id = manifest
        .credential_pending
        .as_ref()
        .expect("pending intent")
        .operation_id
        .clone();
    let (current, credential) = pair.map_or((None, None), |(public, private)| {
        (Some(public), Some(private))
    });
    let result = (|| -> Result<ProjectionCommit> {
        ensure!(
            current
                .as_ref()
                .is_none_or(|bytes| bytes.len() as u64 <= MAX_PROJECTION),
            "identity public projection exceeds size limit"
        );
        ensure!(
            credential
                .as_ref()
                .is_none_or(|bytes| bytes.len() as u64 <= MAX_PROJECTION),
            "identity credential projection exceeds size limit"
        );
        if recovery || previous != current || old_private != credential {
            manifest.generation = manifest
                .generation
                .checked_add(1)
                .context("projection generation overflow")?;
        }
        restore_file(
            private,
            IDENTITY_FILE,
            credential.as_deref(),
            0o640,
            Some(gid),
        )?;
        restore_file(
            &locked.directory,
            IDENTITY_FILE,
            current.as_deref(),
            0o644,
            None,
        )?;
        // Persist both pair renames/deletions BEFORE the final clear-marker
        // rename. After power loss the manifest can be pending or committed;
        // either case can never certify the prior credential over new Cozo.
        sync_boundary(private, "identity_private_sync")?;
        sync_boundary(&locked.directory, "identity_public_sync")?;
        manifest.credential_pending = None;
        write_file(
            &locked.directory,
            MANIFEST_NAME,
            &simd_json::to_vec(&manifest)?,
            0o644,
            None,
        )?;
        // Final manifest rename is the commit point, with no fallible work
        // afterwards. A crash may retain the synced pending marker and require
        // conservative recovery; it cannot resurrect the pre-intent manifest.
        Ok(ProjectionCommit {
            generation: manifest.generation,
            previous,
            current,
        })
    })();
    match result {
        Ok(projection) => CredentialPublication::Committed {
            projection,
            durable,
        },
        Err(error) => CredentialPublication::Pending {
            operation_id,
            durable: Some(durable),
            error,
        },
    }
}

#[cfg(test)]
#[path = "identity_projection_transaction_tests.rs"]
mod tests;
