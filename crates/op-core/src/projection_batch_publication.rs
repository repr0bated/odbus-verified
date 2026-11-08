//! Multi-plugin raw state publication under one canonical namespace manifest.
//! The durable callback runs once, after every selected ID has been gated.
use super::*;

pub type ProjectionDocuments = BTreeMap<String, Option<Vec<u8>>>;

pub struct ProjectionBatchCommit {
    pub generation: u64,
    pub current: ProjectionDocuments,
}

#[must_use = "only committed batches permit state notifications"]
pub enum ProjectionBatchPublication<T> {
    Committed { projection: ProjectionBatchCommit, durable: T },
    Pending { operation_id: String, durable: Option<T>, error: anyhow::Error },
}

pub fn commit_projection_batch_durable_in<T>(
    state_dir: &str,
    plugin_ids: &[String],
    operation_id: &str,
    commit: impl FnOnce(&ProjectionDocuments) -> Result<(ProjectionDocuments, T)>,
) -> Result<ProjectionBatchPublication<T>> {
    let locked = LockedDirectory::open(state_dir, true)?.context("projection directory missing")?;
    commit_batch(&locked, plugin_ids, operation_id, commit)
}

pub fn recover_projection_batch_in<T>(
    state_dir: &str,
    plugin_ids: &[String],
    operation_id: &str,
    load: impl FnOnce() -> Result<(ProjectionDocuments, T)>,
) -> Result<ProjectionBatchPublication<T>> {
    let locked = LockedDirectory::open(state_dir, true)?.context("projection directory missing")?;
    recover_batch(&locked, plugin_ids, operation_id, load)
}

impl PrivateProjectionDirectory {
    pub fn commit_documents_durable<T>(
        &self,
        plugin_ids: &[String],
        operation_id: &str,
        commit: impl FnOnce(&ProjectionDocuments) -> Result<(ProjectionDocuments, T)>,
    ) -> Result<ProjectionBatchPublication<T>> {
        require_private(&self.directory, true)?;
        let locked = LockedDirectory::from_directory(self.directory.try_clone()?, true)?
            .context("private namespace lock missing")?;
        commit_batch(&locked, plugin_ids, operation_id, commit)
    }

    pub fn recover_documents<T>(
        &self,
        plugin_ids: &[String],
        operation_id: &str,
        load: impl FnOnce() -> Result<(ProjectionDocuments, T)>,
    ) -> Result<ProjectionBatchPublication<T>> {
        require_private(&self.directory, true)?;
        let locked = LockedDirectory::from_directory(self.directory.try_clone()?, true)?
            .context("private namespace lock missing")?;
        recover_batch(&locked, plugin_ids, operation_id, load)
    }
}

fn identifiers(plugin_ids: &[String]) -> Result<Vec<String>> {
    ensure!(!plugin_ids.is_empty(), "state publication batch is empty");
    let mut ids = plugin_ids.to_vec();
    ids.sort();
    ids.dedup();
    ensure!(ids.len() == plugin_ids.len(), "duplicate plugin in state publication batch");
    for id in &ids {
        filename(id)?;
        ensure!(id != "identity_sled", "identity uses its dedicated paired publication");
    }
    Ok(ids)
}

fn commit_batch<T>(
    locked: &LockedDirectory,
    plugin_ids: &[String],
    operation_id: &str,
    commit: impl FnOnce(&ProjectionDocuments) -> Result<(ProjectionDocuments, T)>,
) -> Result<ProjectionBatchPublication<T>> {
    let ids = identifiers(plugin_ids)?;
    identity_transaction::validate_operation_id(operation_id)?;
    let old = locked.manifest()?;
    let had_manifest = old.is_some();
    let mut manifest = old.map_or_else(Manifest::default, |(manifest, _)| manifest);
    ensure!(manifest.document_pending.is_none(), "state namespace has a pending document operation");
    manifest.generation.checked_add(1).context("projection generation exhausted")?;
    let mut previous = ProjectionDocuments::new();
    for id in &ids {
        manifest.require_available(id)?;
        let value = read_file(&locked.directory, &filename(id)?, MAX_PROJECTION, false)?;
        ensure!(had_manifest || value.is_none(), "uncommitted state exists without a manifest");
        previous.insert(id.clone(), value);
    }
    manifest.document_pending = Some(PendingDocumentPublication {
        plugin_id: ids[0].clone(), operation_id: operation_id.into(), plugin_ids: ids.clone(),
    });
    let marker = simd_json::to_vec(&manifest)?;
    ensure!(marker.len() as u64 <= MAX_MANIFEST, "state publication batch exceeds manifest byte bound");
    write_file(&locked.directory, MANIFEST_NAME, &marker, 0o644, None)?;
    let committed = (|| {
        locked.directory.sync_all()?;
        commit(&previous)
    })();
    match committed {
        Ok((current, durable)) => Ok(finish_batch(locked, manifest, &ids, Some(previous), current, durable)),
        Err(error) => Ok(ProjectionBatchPublication::Pending { operation_id: operation_id.into(), durable: None, error }),
    }
}

fn recover_batch<T>(
    locked: &LockedDirectory,
    plugin_ids: &[String],
    operation_id: &str,
    load: impl FnOnce() -> Result<(ProjectionDocuments, T)>,
) -> Result<ProjectionBatchPublication<T>> {
    let ids = identifiers(plugin_ids)?;
    identity_transaction::validate_operation_id(operation_id)?;
    let (manifest, _) = locked.manifest()?.context("batch recovery requires a manifest")?;
    let pending = manifest.document_pending.as_ref().context("batch recovery has no pending operation")?;
    let expected = if pending.plugin_ids.is_empty() { vec![pending.plugin_id.clone()] } else { pending.plugin_ids.clone() };
    ensure!(pending.operation_id == operation_id && identifiers(&expected)? == ids,
        "batch recovery requires the exact pending plugin set and operation");
    for id in &ids {
        match open_at(&locked.directory, filename(id)?.as_ref(), OFlag::O_RDONLY | OFlag::O_NONBLOCK, 0) {
            Ok(file) => check_file(&file, false, locked.directory.metadata()?.uid())?,
            Err(Errno::ENOENT) => {}
            Err(error) => return Err(error.into()),
        }
    }
    match load() {
        Ok((current, durable)) => Ok(finish_batch(locked, manifest, &ids, None, current, durable)),
        Err(error) => Ok(ProjectionBatchPublication::Pending { operation_id: operation_id.into(), durable: None, error }),
    }
}

fn finish_batch<T>(
    locked: &LockedDirectory,
    mut manifest: Manifest,
    ids: &[String],
    previous: Option<ProjectionDocuments>,
    current: ProjectionDocuments,
    durable: T,
) -> ProjectionBatchPublication<T> {
    let operation_id = manifest.document_pending.as_ref().expect("pending batch").operation_id.clone();
    let result = (|| -> Result<()> {
        ensure!(current.keys().eq(ids.iter()), "durable state batch returned a different plugin set");
        for bytes in current.values().flatten() {
            ensure!(bytes.len() as u64 <= MAX_PROJECTION, "state batch document exceeds projection limit");
        }
        if previous.as_ref() != Some(&current) {
            manifest.generation = manifest.generation.checked_add(1).context("projection generation exhausted")?;
        }
        for (id, bytes) in &current {
            restore_file(&locked.directory, &filename(id)?, bytes.as_deref(), 0o600, None)?;
        }
        locked.directory.sync_all()?;
        manifest.document_pending = None;
        // One final manifest rename commits every file in this namespace.
        write_file(&locked.directory, MANIFEST_NAME, &simd_json::to_vec(&manifest)?, 0o644, None)
    })();
    match result {
        Ok(()) => ProjectionBatchPublication::Committed {
            projection: ProjectionBatchCommit { generation: manifest.generation, current }, durable,
        },
        Err(error) => ProjectionBatchPublication::Pending { operation_id, durable: Some(durable), error },
    }
}
