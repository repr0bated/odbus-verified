//! Durable private documents share the canonical SHM lock and commit manifest.
//! The marker is a publication gate, never a document or identity authority.
use super::*;

/// A durable receipt can survive a failed post-commit contract check without
/// certifying or exposing its document as committed runtime state.
pub enum PrivateDocumentCommit<T> {
    Publish {
        current: Option<Vec<u8>>,
        durable: T,
    },
    RetainPending {
        durable: T,
        error: anyhow::Error,
    },
    /// The domain owner proved rejection before any durable effect was
    /// submitted. Clear this exact operation's marker without changing state.
    Abort {
        error: anyhow::Error,
    },
}

/// Only Committed permits an owner-scoped wake. Pending retains the durable
/// receipt when available; errors never certify the old SHM document as current.
#[must_use = "only a committed publication permits a private state wake"]
pub enum PrivateDocumentPublication<T> {
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

fn sync_boundary(directory: &File, phase: &str) -> Result<()> {
    #[cfg(test)]
    super::super::tests::write_fault(phase)?;
    let _ = phase;
    directory.sync_all()?;
    Ok(())
}

impl PrivateProjectionDirectory {
    /// Metadata for explicit durable recovery. It does not read state or grant
    /// authority to substitute a different document for the pending operation.
    pub fn pending_document(&self) -> Result<Option<PendingDocumentPublication>> {
        require_private(&self.directory, false)?;
        let Some(locked) = LockedDirectory::from_directory(self.directory.try_clone()?, false)?
        else {
            return Ok(None);
        };
        Ok(locked
            .manifest()?
            .and_then(|(manifest, _)| manifest.document_pending))
    }

    /// Run an owned synchronous durable operation after sealing the pending
    /// gate, then publish exactly its returned complete document. Run this whole
    /// helper in a cancellation-owned blocking worker. The callback may use its
    /// durable domain owner, but must never reenter any SHM projection helper.
    ///
    /// Once the callback starts, every error or panic retains the gate. A
    /// callback may return `Abort` only when its domain owner proves no durable
    /// effect was submitted; that exact marker is then cleared. Admission that
    /// cannot make this guarantee requires a durable recovery.
    /// The callback receives only the previously committed document. It must
    /// enforce domain revisions and validate the exact candidate before return.
    pub fn commit_document_durable<T>(
        &self,
        plugin_id: &str,
        operation_id: &str,
        commit: impl FnOnce(Option<&[u8]>) -> Result<(Option<Vec<u8>>, T)>,
    ) -> Result<PrivateDocumentPublication<T>> {
        self.commit_document_durable_with_decision(plugin_id, operation_id, |previous| {
            commit(previous)
                .map(|(current, durable)| PrivateDocumentCommit::Publish { current, durable })
        })
    }

    /// Variant for post-durable contract checks. RetainPending preserves the
    /// completed durable receipt while leaving the original pending gate intact.
    pub fn commit_document_durable_with_decision<T>(
        &self,
        plugin_id: &str,
        operation_id: &str,
        commit: impl FnOnce(Option<&[u8]>) -> Result<PrivateDocumentCommit<T>>,
    ) -> Result<PrivateDocumentPublication<T>> {
        let name = filename(plugin_id)?;
        ensure!(
            plugin_id != "identity_sled",
            "identity uses its dedicated paired projection"
        );
        identity_transaction::validate_operation_id(operation_id)?;
        require_private(&self.directory, true)?;
        let locked = LockedDirectory::from_directory(self.directory.try_clone()?, true)?
            .context("private namespace lock missing")?;
        let old = locked.manifest()?;
        let had_manifest = old.is_some();
        let mut manifest = old.map_or_else(Manifest::default, |(manifest, _)| manifest);
        ensure!(
            manifest.document_pending.is_none(),
            "private namespace has a pending document operation"
        );
        manifest.require_available(plugin_id)?;
        manifest
            .generation
            .checked_add(1)
            .context("projection generation overflow")?;
        let previous = read_file(&locked.directory, &name, MAX_PROJECTION, false)?;
        ensure!(
            had_manifest || previous.is_none(),
            "uncommitted private document has no manifest"
        );
        manifest.document_pending = Some(PendingDocumentPublication {
            plugin_id: plugin_id.into(),
            operation_id: operation_id.into(),
        });
        write_file(
            &locked.directory,
            MANIFEST_NAME,
            &simd_json::to_vec(&manifest)?,
            0o644,
            None,
        )?;
        if let Err(error) = sync_boundary(&locked.directory, "private_document_marker_sync") {
            return Ok(PrivateDocumentPublication::Pending {
                operation_id: operation_id.into(),
                durable: None,
                error,
            });
        }
        match commit(previous.as_deref()) {
            Ok(PrivateDocumentCommit::Publish { current, durable }) => Ok(finish(
                &locked, manifest, &name, previous, current, durable, false,
            )),
            Ok(PrivateDocumentCommit::RetainPending { durable, error }) => {
                Ok(PrivateDocumentPublication::Pending {
                    operation_id: operation_id.into(),
                    durable: Some(durable),
                    error,
                })
            }
            Ok(PrivateDocumentCommit::Abort { error }) => {
                manifest.document_pending = None;
                match write_file(
                    &locked.directory,
                    MANIFEST_NAME,
                    &simd_json::to_vec(&manifest)?,
                    0o644,
                    None,
                ) {
                    Ok(()) => Err(error),
                    Err(cleanup) => Ok(PrivateDocumentPublication::Pending {
                        operation_id: operation_id.into(),
                        durable: None,
                        error: cleanup.context("failed to clear rejected private operation"),
                    }),
                }
            }
            Err(error) => Ok(PrivateDocumentPublication::Pending {
                operation_id: operation_id.into(),
                durable: None,
                error,
            }),
        }
    }

    /// Exact-operation recovery invokes the authoritative durable reader. It
    /// never passes potentially mixed SHM bytes to the callback and has no
    /// clear-marker/old-state rollback alternative. On process activation an
    /// owner must recover/reconcile durability before enabling private reads;
    /// volatile SHM cannot retain markers across machine reboot.
    pub fn recover_document<T>(
        &self,
        plugin_id: &str,
        operation_id: &str,
        load_durable: impl FnOnce() -> Result<(Option<Vec<u8>>, T)>,
    ) -> Result<PrivateDocumentPublication<T>> {
        self.recover_document_with_decision(plugin_id, operation_id, || {
            load_durable()
                .map(|(current, durable)| PrivateDocumentCommit::Publish { current, durable })
        })
    }

    /// Keep a recovered durable receipt even when current admission prevents
    /// publication. Only Publish may clear the exact pending operation.
    pub fn recover_document_with_decision<T>(
        &self,
        plugin_id: &str,
        operation_id: &str,
        load_durable: impl FnOnce() -> Result<PrivateDocumentCommit<T>>,
    ) -> Result<PrivateDocumentPublication<T>> {
        let name = filename(plugin_id)?;
        ensure!(
            plugin_id != "identity_sled",
            "identity uses its dedicated paired projection"
        );
        identity_transaction::validate_operation_id(operation_id)?;
        require_private(&self.directory, true)?;
        let locked = LockedDirectory::from_directory(self.directory.try_clone()?, true)?
            .context("private namespace lock missing")?;
        let (manifest, _) = locked
            .manifest()?
            .context("private recovery requires a commit manifest")?;
        ensure!(
            manifest
                .document_pending
                .as_ref()
                .is_some_and(|pending| pending.plugin_id == plugin_id
                    && pending.operation_id == operation_id),
            "private recovery requires the exact pending plugin and operation"
        );
        manifest
            .generation
            .checked_add(1)
            .context("projection generation overflow")?;
        // Check file safety without treating its potentially mixed bytes as a
        // predecessor or recovery authority.
        match open_at(
            &locked.directory,
            name.as_ref(),
            OFlag::O_RDONLY | OFlag::O_NONBLOCK,
            0,
        ) {
            Ok(file) => check_file(&file, false, locked.directory.metadata()?.uid())?,
            Err(Errno::ENOENT) => {}
            Err(error) => return Err(error.into()),
        }
        match load_durable() {
            Ok(PrivateDocumentCommit::Publish { current, durable }) => Ok(finish(
                &locked, manifest, &name, None, current, durable, true,
            )),
            Ok(PrivateDocumentCommit::RetainPending { durable, error }) => {
                Ok(PrivateDocumentPublication::Pending {
                    operation_id: operation_id.into(),
                    durable: Some(durable),
                    error,
                })
            }
            Ok(PrivateDocumentCommit::Abort { .. }) => Ok(PrivateDocumentPublication::Pending {
                operation_id: operation_id.into(),
                durable: None,
                error: anyhow::anyhow!(
                    "private recovery owner did not provide an authoritative durable row"
                ),
            }),
            Err(error) => Ok(PrivateDocumentPublication::Pending {
                operation_id: operation_id.into(),
                durable: None,
                error,
            }),
        }
    }
}

#[allow(clippy::too_many_arguments)] // Explicit old/new document and durable receipt remain paired.
fn finish<T>(
    locked: &LockedDirectory,
    mut manifest: Manifest,
    name: &str,
    previous: Option<Vec<u8>>,
    current: Option<Vec<u8>>,
    durable: T,
    recovery: bool,
) -> PrivateDocumentPublication<T> {
    let operation_id = manifest
        .document_pending
        .as_ref()
        .expect("pending operation")
        .operation_id
        .clone();
    let result = (|| -> Result<ProjectionCommit> {
        ensure!(
            current
                .as_ref()
                .is_none_or(|bytes| bytes.len() as u64 <= MAX_PROJECTION),
            "private document exceeds projection size limit"
        );
        if recovery || previous != current {
            manifest.generation = manifest
                .generation
                .checked_add(1)
                .context("projection generation overflow")?;
        }
        restore_file(&locked.directory, name, current.as_deref(), 0o600, None)?;
        sync_boundary(&locked.directory, "private_document_data_sync")?;
        manifest.document_pending = None;
        // This final rename is the commit point. All data and the preceding
        // pending marker were synced; no fallible step follows the commit.
        write_file(
            &locked.directory,
            MANIFEST_NAME,
            &simd_json::to_vec(&manifest)?,
            0o644,
            None,
        )?;
        Ok(ProjectionCommit {
            generation: manifest.generation,
            previous,
            current,
        })
    })();
    match result {
        Ok(projection) => PrivateDocumentPublication::Committed {
            projection,
            durable,
        },
        Err(error) => PrivateDocumentPublication::Pending {
            operation_id,
            durable: Some(durable),
            error,
        },
    }
}

#[cfg(test)]
#[path = "private_document_publication_tests.rs"]
mod tests;
