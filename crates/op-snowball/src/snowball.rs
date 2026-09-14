//! Streaming snowball with data directories for timing, vectors, and state
//!
//! Architecture:
//! - timing: Immutable audit trail (append-only)
//! - vectors: ML embeddings for semantic search
//! - state: Current system state for DR/reinstall

use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Datelike, Utc};
use nix::fcntl::{Flock, FlockArg};
use std::cmp::Reverse;
use std::collections::{BTreeMap, HashSet};
use std::fs::{File as StdFile, OpenOptions as StdOpenOptions};
use std::io::Write as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::{Mutex, RwLock};
use tokio::time::Instant;
use tracing::{debug, info, warn};

use crate::footprint::{BlockEvent, PluginFootprint};
use crate::retention::RetentionPolicy;
use crate::snapshot::SnapshotInterval;

/// The three data directories that make up one committed logical recovery set.
/// Order is stable for deterministic snapshot construction.
pub const SNAPSHOT_LABELS: [&str; 3] = ["timing", "vectors", "state"];

/// Cap on a single flattened field's length in embedding text, so one large
/// payload cannot dominate a block's vector.
const MAX_EMBEDDING_FIELD_LEN: usize = 512;

/// One snapshot considered by the retention policy.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SnapshotEntry {
    name: String,
    created: DateTime<Utc>,
    /// Aligned counter parsed from the name; the only strictly increasing,
    /// per-snapshot identity available (btrfs snapshots inherit their source's
    /// mtime, so timestamps tie or even move backwards).
    counter: u64,
}

/// Decide which snapshots of one family to keep.
///
/// Sorts newest-first and returns the keep set. Two rules exist independently of
/// the time buckets, because losing either snapshot breaks a chain guarantee:
///
/// - the highest counter is always kept: it is the latest recovery point and the
///   next incremental send's parent;
///
/// This matters when filesystem timestamps tie: the highest counter still
/// identifies the newest committed logical snapshot.
fn retain_snapshots(
    snapshots: &mut [SnapshotEntry],
    policy: &RetentionPolicy,
    now: DateTime<Utc>,
) -> HashSet<String> {
    use chrono::Duration;

    snapshots.sort_by_key(|entry| (Reverse(entry.created), Reverse(entry.counter)));

    let mut hourly: Vec<&str> = Vec::new();
    let mut daily: BTreeMap<String, &str> = BTreeMap::new();
    let mut weekly: BTreeMap<(i32, u32), &str> = BTreeMap::new();
    let mut quarterly: BTreeMap<String, &str> = BTreeMap::new();

    for entry in snapshots.iter() {
        let age = now.signed_duration_since(entry.created);
        if age <= Duration::hours(24) {
            hourly.push(&entry.name);
        } else if age <= Duration::days(30) {
            daily
                .entry(entry.created.format("%Y%m%d").to_string())
                .or_insert(&entry.name);
        } else if age <= Duration::weeks(12) {
            weekly
                .entry((entry.created.year(), entry.created.iso_week().week()))
                .or_insert(&entry.name);
        } else {
            let quarter = (entry.created.month() - 1) / 3 + 1;
            quarterly
                .entry(format!("{}-Q{}", entry.created.year(), quarter))
                .or_insert(&entry.name);
        }
    }

    let mut keep: HashSet<String> = HashSet::new();
    keep.extend(hourly.iter().take(policy.hourly).map(|n| n.to_string()));
    for (bucket, count) in [
        (daily, policy.daily),
        (
            weekly
                .into_iter()
                .map(|((year, week), name)| (format!("{year}-W{week:02}"), name))
                .collect(),
            policy.weekly,
        ),
        (quarterly, policy.quarterly),
    ] {
        // Buckets are keyed chronologically, so reversing takes the newest.
        keep.extend(
            bucket
                .into_values()
                .rev()
                .take(count)
                .map(|name| name.to_string()),
        );
    }

    if let Some(newest) = snapshots.iter().max_by_key(|entry| entry.counter) {
        keep.insert(newest.name.clone());
    }
    keep
}

/// Streaming snowball with data directories for timing, vectors, and state
pub struct StreamingSnowball {
    base_path: PathBuf,
    timing_subvol: PathBuf,
    vector_subvol: PathBuf,
    state_subvol: PathBuf,
    snapshot_interval: SnapshotInterval,
    retention_policy: Arc<RwLock<RetentionPolicy>>,
    snapshot_lock: Arc<Mutex<()>>,
    writer_lock_path: PathBuf,
    last_snapshot_time: Arc<RwLock<Instant>>,
    block_counter: Arc<RwLock<u64>>,
}

impl StreamingSnowball {
    /// Create a new streaming snowball at the given path
    pub async fn new(base_path: impl AsRef<Path>) -> Result<Self> {
        Self::new_with_interval(base_path, SnapshotInterval::from_env()).await
    }

    /// Create with a specific snapshot interval
    pub async fn new_with_interval(
        base_path: impl AsRef<Path>,
        snapshot_interval: SnapshotInterval,
    ) -> Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();
        let timing_subvol = base_path.join("timing");
        let vector_subvol = base_path.join("vectors");
        let state_subvol = base_path.join("state");

        // The configured root and all children must be real directories. A
        // symlinked root could redirect durable state outside the golden tree.
        Self::ensure_data_dir(&base_path).await?;

        // Ensure data directories exist
        Self::ensure_data_dir(&timing_subvol).await?;
        Self::ensure_data_dir(&vector_subvol).await?;
        Self::ensure_data_dir(&state_subvol).await?;

        // Create snapshots directory
        let snapshots_dir = base_path.join("snapshots");
        Self::ensure_data_dir(&snapshots_dir).await?;

        // Resume from the highest block already on disk. Starting at 0 would
        // renumber from block 1 on every restart and overwrite existing timing
        // records — silent destruction of the one artifact that cannot be
        // regenerated.
        let resumed_from = highest_block_number(&timing_subvol).await?;

        info!(
            "Streaming snowball initialized at {:?} with {} interval, resuming after block {}",
            base_path, snapshot_interval, resumed_from
        );

        let retention_policy = Self::load_retention_policy(&base_path)
            .await?
            .unwrap_or_else(RetentionPolicy::from_env);

        let writer_lock_path = base_path.join(".writer.lock");
        open_writer_lock(&writer_lock_path)?;

        Ok(Self {
            base_path,
            timing_subvol,
            vector_subvol,
            state_subvol,
            snapshot_interval,
            retention_policy: Arc::new(RwLock::new(retention_policy)),
            snapshot_lock: Arc::new(Mutex::new(())),
            writer_lock_path,
            last_snapshot_time: Arc::new(RwLock::new(Instant::now())),
            block_counter: Arc::new(RwLock::new(resumed_from)),
        })
    }

    /// Ensure a data directory exists.
    /// Existing directories, including legacy subvolumes, remain usable. New
    /// paths are plain directories inside the enclosing golden filesystem.
    async fn ensure_data_dir(path: &Path) -> Result<()> {
        tokio::fs::create_dir_all(path).await?;
        let metadata = tokio::fs::symlink_metadata(path).await?;
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "snowball data path is not a directory: {}",
            path.display()
        );
        Ok(())
    }

    async fn writer_transaction<T, F>(&self, transaction: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        let local = Arc::clone(&self.snapshot_lock).lock_owned().await;
        let path = self.writer_lock_path.clone();
        tokio::task::spawn_blocking(move || {
            let _local = local;
            let file = open_writer_lock(&path)?;
            let _file = Flock::lock(file, FlockArg::LockExclusive)
                .map_err(|(_, error)| anyhow::anyhow!("lock {}: {error}", path.display()))?;
            transaction()
        })
        .await
        .context("Snowball writer transaction failed")?
    }

    /// Add a plugin footprint to the snowball
    pub async fn add_footprint(&self, footprint: PluginFootprint) -> Result<String> {
        let event = footprint.to_block_event();
        self.add_event(event).await
    }

    /// Add a block event to the snowball
    pub async fn add_event(&self, event: BlockEvent) -> Result<String> {
        let hash = event.hash.clone();
        let timing_data = simd_json::to_vec_pretty(&event)?;
        let vector_data = (!event.vector.is_empty()).then(|| encode_vector(&event.vector));
        let timing_dir = self.timing_subvol.clone();
        let vector_dir = self.vector_subvol.clone();
        let block_num = self
            .writer_transaction(move || {
                let block_num = highest_block_number_sync(&timing_dir)?
                    .checked_add(1)
                    .context("Snowball block counter overflow")?;
                atomic_write_sync(
                    &timing_dir,
                    &timing_dir.join(timing_file_name(block_num)),
                    &format!(".block-{block_num:012}"),
                    &timing_data,
                )?;
                if let Some(bytes) = vector_data {
                    atomic_write_sync(
                        &vector_dir,
                        &vector_dir.join(vector_file_name(block_num)),
                        &format!(".vector-{block_num:012}"),
                        &bytes,
                    )?;
                }
                Ok(block_num)
            })
            .await?;
        let mut cached_counter = self.block_counter.write().await;
        *cached_counter = (*cached_counter).max(block_num);
        drop(cached_counter);

        debug!("Added block {} with hash {}", block_num, hash);

        // Check if we should snapshot
        let should_snapshot = {
            let last = self.last_snapshot_time.read().await;
            self.snapshot_interval.should_snapshot(last.elapsed())
        };

        if should_snapshot {
            self.create_snapshot().await?;
        }

        Ok(hash)
    }

    // ── Vector projections ───────────────────────────────────────────────
    //
    // Vectors live in the chain, not only in the index. Because the vector
    // directory is snapshotted and sent alongside timing, a received replica
    // can rebuild an entire vector index from the stream alone — no
    // re-embedding, no embedding-provider dependency on the restore path.
    //
    // On-disk form is raw little-endian f32 (4096 bytes at 1024 dims), which
    // needs no parse to become an upsert payload.

    /// Path of the vector projection for a block, whether or not it exists.
    pub fn vector_path(&self, block_num: u64) -> PathBuf {
        self.vector_subvol.join(vector_file_name(block_num))
    }

    /// Write (or replace) the vector projection for an existing block.
    ///
    /// Rejects a block that has no timing record, so a vector can never
    /// reference a block that does not exist.
    pub async fn attach_vector(&self, block_num: u64, vector: &[f32]) -> Result<PathBuf> {
        anyhow::ensure!(
            !vector.is_empty(),
            "refusing to attach an empty vector to block {block_num}"
        );
        let timing_file = self.timing_path(block_num);
        let vector_dir = self.vector_subvol.clone();
        let vector_file = self.vector_path(block_num);
        let committed_vector_file = vector_file.clone();
        let bytes = encode_vector(vector);
        self.writer_transaction(move || {
            let timing_metadata = std::fs::symlink_metadata(&timing_file).with_context(|| {
                format!(
                    "no timing record for block {block_num} at {}",
                    timing_file.display()
                )
            })?;
            ensure!(
                timing_metadata.is_file() && !timing_metadata.file_type().is_symlink(),
                "timing record for block {block_num} is not a regular file"
            );
            atomic_write_sync(
                &vector_dir,
                &committed_vector_file,
                &format!(".vector-{block_num:012}"),
                &bytes,
            )
        })
        .await?;

        debug!(
            "Attached {}-dim vector to block {}",
            vector.len(),
            block_num
        );
        Ok(vector_file)
    }

    /// Read a block's vector projection, or `None` when it has not landed yet.
    pub async fn read_vector(&self, block_num: u64) -> Result<Option<Vec<f32>>> {
        let path = self.vector_path(block_num);
        match tokio::fs::read(&path).await {
            Ok(bytes) => {
                Ok(Some(decode_vector(&bytes).with_context(|| {
                    format!("malformed vector file {}", path.display())
                })?))
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => {
                Err(anyhow::Error::from(err).context(format!("failed to read {}", path.display())))
            }
        }
    }

    /// Every block on disk, ascending by block number.
    pub async fn blocks(&self) -> Result<Vec<ChainBlockRef>> {
        let mut blocks = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.timing_subvol)
            .await
            .with_context(|| {
                format!(
                    "failed to read timing directory {}",
                    self.timing_subvol.display()
                )
            })?;

        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(block_num) = parse_block_number(&name) else {
                continue;
            };
            let bytes = tokio::fs::read(entry.path()).await?;
            let has_vector = tokio::fs::try_exists(self.vector_path(block_num))
                .await
                .unwrap_or(false);
            match ChainBlockRef::from_timing_bytes(block_num, bytes, has_vector) {
                Ok(block) => blocks.push(block),
                Err(err) => warn!("skipping unparseable timing record {}: {}", name, err),
            }
        }

        blocks.sort_by_key(|block| block.block_num);
        Ok(blocks)
    }

    /// Blocks whose vector projection has not landed yet — the work queue for
    /// an embedding pass.
    pub async fn blocks_missing_vectors(&self) -> Result<Vec<ChainBlockRef>> {
        Ok(self
            .blocks()
            .await?
            .into_iter()
            .filter(|block| !block.has_vector)
            .collect())
    }

    fn timing_path(&self, block_num: u64) -> PathBuf {
        self.timing_subvol.join(timing_file_name(block_num))
    }

    /// Path of the timing data directory (authoritative audit trail).
    pub fn timing_subvolume_path(&self) -> &Path {
        &self.timing_subvol
    }

    /// Path of the vector data directory (embedding projections).
    pub fn vector_subvolume_path(&self) -> &Path {
        &self.vector_subvol
    }

    /// Path of the state data directory (disaster-recovery state).
    pub fn state_subvolume_path(&self) -> &Path {
        &self.state_subvol
    }

    /// Snapshot all three data directories under one aligned counter.
    ///
    /// These are ordinary directories inside the Snowball data tree. The
    /// enclosing golden filesystem snapshot is the Btrfs send/receive unit;
    /// Snowball snapshots never create additional subvolumes.
    ///
    /// Returns the aligned counter used in the committed snapshot-set name.
    pub async fn create_snapshot(&self) -> Result<String> {
        let counter = self.create_snapshot_aligned().await?;
        Ok(Self::snapshot_set_name(counter))
    }

    /// Same as [`Self::create_snapshot`] but returns the logical counter.
    pub async fn create_snapshot_aligned(&self) -> Result<u64> {
        let operation_guard = Arc::clone(&self.snapshot_lock).lock_owned().await;
        let snapshot_dir = self.base_path.join("snapshots");
        let writer_lock_path = self.writer_lock_path.clone();
        let sources = SNAPSHOT_LABELS.map(|label| {
            (
                label.to_string(),
                self.data_dir_for_label(label).to_path_buf(),
            )
        });
        let task = tokio::task::spawn_blocking(move || {
            let _operation_guard = operation_guard;
            let file = open_writer_lock(&writer_lock_path)?;
            let _writer_lock =
                Flock::lock(file, FlockArg::LockExclusive).map_err(|(_, error)| {
                    anyhow::anyhow!("lock {}: {error}", writer_lock_path.display())
                })?;
            let counter = next_snapshot_counter_sync(&snapshot_dir)?;
            commit_snapshot_set_sync(&snapshot_dir, counter, &sources)?;
            Ok::<u64, anyhow::Error>(counter)
        });
        let counter = task.await.context("Snowball snapshot task failed")??;

        // Update last snapshot time
        *self.last_snapshot_time.write().await = Instant::now();

        // Prune old snapshots according to retention policy
        let snapshot_dir = self.base_path.join("snapshots");
        let base_path = self.base_path.clone();
        let fallback_policy = *self.retention_policy.read().await;
        let prune_result = self
            .writer_transaction(move || {
                let policy = load_retention_policy_sync(&base_path)?.unwrap_or(fallback_policy);
                prune_snapshot_sets_sync(&snapshot_dir, &policy)
            })
            .await;
        if let Err(e) = prune_result {
            warn!("Failed to prune snapshots: {}", e);
        }

        Ok(counter)
    }

    /// Name of one committed logical snapshot set inside the golden tree.
    pub fn snapshot_set_name(counter: u64) -> String {
        format!("SNP-{counter:06}")
    }

    fn data_dir_for_label(&self, label: &str) -> &Path {
        match label {
            "timing" => &self.timing_subvol,
            "vectors" => &self.vector_subvol,
            _ => &self.state_subvol,
        }
    }

    /// Write current state to the state data directory
    pub async fn write_state(&self, key: &str, value: &simd_json::OwnedValue) -> Result<()> {
        let directory = self.state_subvol.clone();
        let target = directory.join(format!("{key}.json"));
        let prefix = format!(".{key}");
        let data = serde_json::to_vec(value)?;
        self.writer_transaction(move || atomic_write_sync(&directory, &target, &prefix, &data))
            .await
    }

    /// Write state data with validation against declared restorable_state_keys.
    ///
    /// Checks whether `key` is declared in any plugin's `restorable_state_keys`.
    /// Supports prefix matching: a declared key ending with `/*` matches any key
    /// sharing that prefix (e.g. `"plugin/foo/*"` matches `"plugin/foo/bar"`).
    ///
    /// In strict mode (`OPDBUS_STRICT_STATE_KEYS=1`), undeclared keys are
    /// rejected with an error. Otherwise a warning is logged and the write
    /// proceeds.
    pub async fn write_state_validated(
        &self,
        key: &str,
        value: &simd_json::OwnedValue,
        declared_keys: &[String],
    ) -> Result<()> {
        let key_declared = declared_keys.iter().any(|dk| {
            if dk.ends_with("/*") {
                key.starts_with(&dk[..dk.len() - 2])
            } else {
                dk == key
            }
        });

        if !key_declared {
            let msg = format!(
                "State key '{}' is not declared in any plugin's restorable_state_keys. \
                This state may not be restored during disaster recovery!",
                key
            );

            let strict = std::env::var("OPDBUS_STRICT_STATE_KEYS")
                .map(|v| v == "1")
                .unwrap_or(false);

            if strict {
                tracing::error!("{}", msg);
                anyhow::bail!("{}", msg);
            } else {
                tracing::warn!("{}", msg);
            }
        }

        self.write_state(key, value).await
    }

    /// Read state from the state data directory
    pub async fn read_state(&self, key: &str) -> Result<simd_json::OwnedValue> {
        let state_file = self.state_subvol.join(format!("{}.json", key));
        let mut data = tokio::fs::read_to_string(&state_file).await?;
        Ok(unsafe { simd_json::from_str(&mut data)? })
    }

    /// Read current DR state without treating an uninitialized chain as `{}`.
    pub async fn current_state(&self) -> Result<Option<simd_json::OwnedValue>> {
        match self.read_state("current").await {
            Ok(state) => Ok(Some(state)),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// List all available snapshots
    pub async fn list_snapshots(&self) -> Result<Vec<(String, String)>> {
        let _snapshot_guard = self.snapshot_lock.lock().await;
        let snapshot_dir = self.base_path.join("snapshots");
        let mut entries = tokio::fs::read_dir(&snapshot_dir).await?;
        let mut snapshots = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if Self::parse_snapshot_set_counter(&name).is_none() {
                continue;
            }

            let file_type = entry.file_type().await?;
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let committed = match tokio::fs::symlink_metadata(entry.path().join(".committed")).await
            {
                Ok(metadata) => metadata.is_file() && !metadata.file_type().is_symlink(),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => return Err(error.into()),
            };
            if !committed {
                continue;
            }

            let metadata = tokio::fs::metadata(entry.path()).await?;
            let ts = metadata.created().or_else(|_| metadata.modified()).ok();
            let human_readable = ts
                .and_then(system_time_to_utc)
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                .unwrap_or_else(|| "unknown".to_string());
            snapshots.push((name, human_readable));
        }

        // Sort by name (newest counter first)
        snapshots.sort_by(|a, b| b.0.cmp(&a.0));

        Ok(snapshots)
    }

    /// Read the DR current-state document stored in a state snapshot.
    pub async fn read_snapshot_state(
        &self,
        snapshot_name: &str,
    ) -> Result<Option<simd_json::OwnedValue>> {
        let _snapshot_guard = self.snapshot_lock.lock().await;
        self.read_snapshot_state_unlocked(snapshot_name).await
    }

    async fn read_snapshot_state_unlocked(
        &self,
        snapshot_name: &str,
    ) -> Result<Option<simd_json::OwnedValue>> {
        self.validate_state_snapshot_name(snapshot_name)?;
        let snapshot_set = self.base_path.join("snapshots").join(snapshot_name);
        let set_metadata = tokio::fs::symlink_metadata(&snapshot_set)
            .await
            .with_context(|| format!("read snapshot metadata {}", snapshot_set.display()))?;
        ensure!(
            set_metadata.is_dir() && !set_metadata.file_type().is_symlink(),
            "Snowball snapshot is not a committed directory: {snapshot_name}"
        );
        let committed = tokio::fs::symlink_metadata(snapshot_set.join(".committed")).await?;
        ensure!(
            committed.is_file() && !committed.file_type().is_symlink(),
            "Snowball snapshot is not committed: {snapshot_name}"
        );
        let state_file = snapshot_set.join("state/current.json");
        let snapshot_path = state_file
            .parent()
            .context("snapshot state path has no parent")?;
        let snapshot_metadata = tokio::fs::symlink_metadata(snapshot_path)
            .await
            .with_context(|| format!("read snapshot metadata {}", snapshot_path.display()))?;
        ensure!(
            snapshot_metadata.is_dir() && !snapshot_metadata.file_type().is_symlink(),
            "Snowball snapshot is not a stored directory: {snapshot_name}"
        );
        let state_metadata = match tokio::fs::symlink_metadata(&state_file).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("read snapshot state {}", state_file.display()))
            }
        };
        ensure!(
            state_metadata.is_file() && !state_metadata.file_type().is_symlink(),
            "Snowball snapshot current state is not a regular file"
        );
        let mut data = tokio::fs::read_to_string(&state_file).await?;
        Ok(Some(
            unsafe { simd_json::from_str(&mut data) }
                .with_context(|| format!("invalid snapshot state {}", state_file.display()))?,
        ))
    }

    /// Restore the authoritative DR current-state document from one snapshot.
    /// The append-only timing trail and derived vectors are never rewound.
    pub async fn rollback(&self, snapshot_name: &str) -> Result<simd_json::OwnedValue> {
        self.validate_state_snapshot_name(snapshot_name)?;
        let snapshot_path = self.base_path.join("snapshots").join(snapshot_name);
        let snapshot_path_for_read = snapshot_path.clone();
        let state_directory = self.state_subvol.clone();
        let name = snapshot_name.to_string();
        let restored = self
            .writer_transaction(move || {
                let state = read_snapshot_state_sync(&snapshot_path_for_read)?
                    .with_context(|| format!("snapshot {name} has no DR current state"))?;
                let data = serde_json::to_vec(&state)?;
                atomic_write_sync(
                    &state_directory,
                    &state_directory.join("current.json"),
                    ".current",
                    &data,
                )?;
                Ok(state)
            })
            .await?;
        info!("Restored DR current state from snapshot: {}", snapshot_name);
        Ok(restored)
    }

    fn validate_state_snapshot_name(&self, snapshot_name: &str) -> Result<u64> {
        let counter = Self::parse_snapshot_set_counter(snapshot_name)
            .with_context(|| format!("invalid Snowball snapshot name: {snapshot_name}"))?;
        Ok(counter)
    }

    fn parse_snapshot_set_counter(name: &str) -> Option<u64> {
        parse_snapshot_set_counter(name)
    }

    /// Validate a state snapshot name and return its aligned counter.
    pub fn state_snapshot_counter(&self, snapshot_name: &str) -> Result<u64> {
        self.validate_state_snapshot_name(snapshot_name)
    }

    /// Get snapshot interval
    pub fn snapshot_interval(&self) -> SnapshotInterval {
        self.snapshot_interval
    }

    /// Set snapshot interval
    pub fn set_snapshot_interval(&mut self, interval: SnapshotInterval) {
        self.snapshot_interval = interval;
        info!("Snapshot interval changed to: {}", interval);
    }

    /// Get retention policy
    pub async fn retention_policy(&self) -> RetentionPolicy {
        *self.retention_policy.read().await
    }

    /// Persist and activate the retention policy for this shared chain.
    pub async fn set_retention_policy(&self, policy: RetentionPolicy) -> Result<()> {
        ensure!(
            policy.max_snapshots() > 0,
            "retention policy must keep at least one snapshot"
        );
        let directory = self.base_path.clone();
        let snapshots = directory.join("snapshots");
        let bytes = serde_json::to_vec_pretty(&policy)?;
        self.writer_transaction(move || {
            atomic_write_sync(
                &directory,
                &directory.join("retention.json"),
                ".retention",
                &bytes,
            )?;
            prune_snapshot_sets_sync(&snapshots, &policy)
        })
        .await?;
        *self.retention_policy.write().await = policy;
        info!(
            "Retention policy updated: {}h/{}d/{}w/{}q",
            policy.hourly, policy.daily, policy.weekly, policy.quarterly
        );
        Ok(())
    }

    async fn load_retention_policy(base_path: &Path) -> Result<Option<RetentionPolicy>> {
        let path = base_path.join("retention.json");
        match tokio::fs::read(&path).await {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).with_context(|| {
                format!("invalid Snowball retention policy {}", path.display())
            })?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
        }
    }

    /// Start a footprint receiver that processes incoming footprints
    pub async fn start_footprint_receiver(
        &self,
        mut receiver: tokio::sync::mpsc::Receiver<PluginFootprint>,
    ) -> Result<()> {
        info!("Starting snowball footprint receiver");

        while let Some(footprint) = receiver.recv().await {
            if let Err(e) = self.add_footprint(footprint).await {
                warn!("Failed to add footprint to snowball: {}", e);
                // Continue processing other footprints
            }
        }

        info!("Snowball footprint receiver stopped");
        Ok(())
    }

    /// Write the current system state (for disaster recovery)
    pub async fn write_current_state(&self, state: &simd_json::OwnedValue) -> Result<()> {
        self.write_state("current", state).await
    }

    /// Get base path
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }
}

fn system_time_to_utc(ts: SystemTime) -> Option<DateTime<Utc>> {
    Some(DateTime::<Utc>::from(ts))
}

// ----------------------------------------------------------------------------
// Block references and vector encoding
// ----------------------------------------------------------------------------

/// A block as it exists on disk: its authoritative timing record, plus whether
/// its vector projection has landed.
///
/// `data_fields` is the block's payload flattened to sorted dotted paths, which
/// is what makes [`Self::embedding_text`] deterministic — the on-disk JSON
/// object order is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainBlockRef {
    pub block_num: u64,
    pub hash: String,
    /// Plugin id (the `category` field of the block event).
    pub category: String,
    /// Method or operation name (the `action` field).
    pub action: String,
    pub timestamp: u64,
    pub data_fields: Vec<(String, String)>,
    pub has_vector: bool,
}

impl ChainBlockRef {
    fn from_timing_bytes(block_num: u64, mut bytes: Vec<u8>, has_vector: bool) -> Result<Self> {
        let value = simd_json::to_owned_value(&mut bytes)
            .map_err(|e| anyhow::anyhow!("invalid JSON in timing record: {e}"))?;

        let mut data_fields = Vec::new();
        if let Some(data) = owned_get(&value, "data") {
            flatten_sorted(data, "data", &mut data_fields);
        }

        Ok(Self {
            block_num,
            hash: owned_str(&value, "hash").unwrap_or_default(),
            category: owned_str(&value, "category").unwrap_or_default(),
            action: owned_str(&value, "action").unwrap_or_default(),
            timestamp: owned_u64(&value, "timestamp").unwrap_or_default(),
            data_fields,
            has_vector,
        })
    }

    /// Value of one flattened field, e.g. `data.metadata.event_hash`.
    pub fn field(&self, path: &str) -> Option<&str> {
        self.data_fields
            .iter()
            .find(|(key, _)| key == path)
            .map(|(_, value)| value.as_str())
    }

    /// Deterministic retrieval text for this block.
    ///
    /// Both the origin (which embeds) and any replica (which may re-embed
    /// after a policy change) must derive identical text from identical
    /// timing records, so this is the single renderer for block embeddings.
    pub fn embedding_text(&self) -> String {
        let mut lines = vec![
            format!("block: {}", self.block_num),
            format!("plugin: {}", self.category),
            format!("action: {}", self.action),
            format!("hash: {}", self.hash),
            format!("timestamp: {}", self.timestamp),
        ];
        for (key, value) in &self.data_fields {
            lines.push(format!("{key}: {value}"));
        }
        lines.join("\n")
    }
}

fn owned_get<'a>(value: &'a simd_json::OwnedValue, key: &str) -> Option<&'a simd_json::OwnedValue> {
    match value {
        simd_json::OwnedValue::Object(map) => map.get(key),
        _ => None,
    }
}

fn owned_str(value: &simd_json::OwnedValue, key: &str) -> Option<String> {
    match owned_get(value, key)? {
        simd_json::OwnedValue::String(s) => Some(s.clone()),
        other => Some(scalar_to_string(other)?),
    }
}

fn owned_u64(value: &simd_json::OwnedValue, key: &str) -> Option<u64> {
    match owned_get(value, key)? {
        simd_json::OwnedValue::Static(simd_json::StaticNode::U64(n)) => Some(*n),
        simd_json::OwnedValue::Static(simd_json::StaticNode::I64(n)) => u64::try_from(*n).ok(),
        _ => None,
    }
}

fn scalar_to_string(value: &simd_json::OwnedValue) -> Option<String> {
    use simd_json::StaticNode;
    match value {
        simd_json::OwnedValue::String(s) => Some(s.clone()),
        simd_json::OwnedValue::Static(StaticNode::U64(n)) => Some(n.to_string()),
        simd_json::OwnedValue::Static(StaticNode::I64(n)) => Some(n.to_string()),
        simd_json::OwnedValue::Static(StaticNode::F64(n)) => Some(n.to_string()),
        simd_json::OwnedValue::Static(StaticNode::Bool(b)) => Some(b.to_string()),
        simd_json::OwnedValue::Static(StaticNode::Null) => Some("null".to_string()),
        _ => None,
    }
}

/// Flatten a JSON value into sorted `dotted.path` / value pairs.
fn flatten_sorted(value: &simd_json::OwnedValue, prefix: &str, out: &mut Vec<(String, String)>) {
    match value {
        simd_json::OwnedValue::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for key in keys {
                if let Some(child) = map.get(key.as_str()) {
                    flatten_sorted(child, &format!("{prefix}.{key}"), out);
                }
            }
        }
        simd_json::OwnedValue::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                flatten_sorted(item, &format!("{prefix}[{index}]"), out);
            }
        }
        scalar => {
            if let Some(mut text) = scalar_to_string(scalar) {
                if text.len() > MAX_EMBEDDING_FIELD_LEN {
                    text.truncate(MAX_EMBEDDING_FIELD_LEN);
                    text.push('…');
                }
                out.push((prefix.to_string(), text));
            }
        }
    }
}

fn timing_file_name(block_num: u64) -> String {
    format!("block-{:012}.json", block_num)
}

fn vector_file_name(block_num: u64) -> String {
    format!("vec-{:012}.bin", block_num)
}

/// Block number from a timing file name, or `None` for anything else in the
/// directory.
pub fn parse_block_number(file_name: &str) -> Option<u64> {
    file_name
        .strip_prefix("block-")?
        .strip_suffix(".json")?
        .parse()
        .ok()
}

/// Block number from a vector file name — the receive-side counterpart of
/// [`parse_block_number`].
pub fn parse_vector_block_number(file_name: &str) -> Option<u64> {
    file_name
        .strip_prefix("vec-")?
        .strip_suffix(".bin")?
        .parse()
        .ok()
}

/// Decode a raw little-endian f32 vector file.
pub fn decode_vector(bytes: &[u8]) -> Result<Vec<f32>> {
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len().is_multiple_of(4),
        "vector file length {} is not a non-zero multiple of 4",
        bytes.len()
    );
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

/// Encode a vector to its on-disk little-endian f32 form.
pub fn encode_vector(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

/// Highest block number already written, so a restart appends instead of
/// renumbering over existing records.
async fn highest_block_number(timing_dir: &Path) -> Result<u64> {
    let mut entries = match tokio::fs::read_dir(timing_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => {
            return Err(anyhow::Error::from(err)
                .context(format!("failed to read {}", timing_dir.display())))
        }
    };

    let mut highest = 0u64;
    while let Some(entry) = entries.next_entry().await? {
        if let Some(block_num) = parse_block_number(&entry.file_name().to_string_lossy()) {
            highest = highest.max(block_num);
        }
    }
    Ok(highest)
}

fn highest_block_number_sync(timing_dir: &Path) -> Result<u64> {
    let mut highest = 0_u64;
    for entry in std::fs::read_dir(timing_dir)
        .with_context(|| format!("failed to read {}", timing_dir.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_file() || file_type.is_symlink() {
            continue;
        }
        if let Some(block_num) = parse_block_number(&entry.file_name().to_string_lossy()) {
            highest = highest.max(block_num);
        }
    }
    Ok(highest)
}

fn atomic_write_sync(directory: &Path, target: &Path, prefix: &str, data: &[u8]) -> Result<()> {
    let directory_metadata = std::fs::symlink_metadata(directory)
        .with_context(|| format!("inspect Snowball data directory {}", directory.display()))?;
    ensure!(
        directory_metadata.is_dir() && !directory_metadata.file_type().is_symlink(),
        "Snowball data directory is not a real directory: {}",
        directory.display()
    );
    ensure!(
        target.parent() == Some(directory),
        "Snowball atomic-write target must be directly inside its data directory"
    );
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_nanos();
    let staging = directory.join(format!("{prefix}-{}-{nonce}.tmp", std::process::id()));
    let result = (|| {
        let mut file = StdOpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW)
            .open(&staging)
            .with_context(|| format!("create Snowball staging file {}", staging.display()))?;
        file.write_all(data)?;
        file.sync_all()?;
        std::fs::rename(&staging, target)
            .with_context(|| format!("commit Snowball data file {}", target.display()))?;
        StdFile::open(directory)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    result
}

fn load_retention_policy_sync(base_path: &Path) -> Result<Option<RetentionPolicy>> {
    let path = base_path.join("retention.json");
    let metadata = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Snowball retention policy is not a regular file: {}",
        path.display()
    );
    let bytes = std::fs::read(&path)?;
    Ok(Some(serde_json::from_slice(&bytes).with_context(|| {
        format!("invalid Snowball retention policy {}", path.display())
    })?))
}

fn open_writer_lock(path: &Path) -> Result<StdFile> {
    let file = StdOpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)
        .with_context(|| format!("open Snowball writer lock {}", path.display()))?;
    ensure!(
        file.metadata()?.is_file(),
        "Snowball writer lock is not a regular file"
    );
    Ok(file)
}

fn parse_snapshot_set_counter(name: &str) -> Option<u64> {
    let counter = name.strip_prefix("SNP-")?.parse::<u64>().ok()?;
    (name == StreamingSnowball::snapshot_set_name(counter)).then_some(counter)
}

fn next_snapshot_counter_sync(snapshot_dir: &Path) -> Result<u64> {
    let mut max_counter = 0_u64;
    for entry in std::fs::read_dir(snapshot_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(counter) = parse_snapshot_set_counter(&name) {
            max_counter = max_counter.max(counter);
        }
    }
    max_counter
        .checked_add(1)
        .context("Snowball snapshot counter overflow")
}

fn read_snapshot_state_sync(snapshot_set: &Path) -> Result<Option<simd_json::OwnedValue>> {
    let set_metadata = std::fs::symlink_metadata(snapshot_set)
        .with_context(|| format!("inspect snapshot {}", snapshot_set.display()))?;
    ensure!(
        set_metadata.is_dir() && !set_metadata.file_type().is_symlink(),
        "Snowball snapshot is not a committed directory: {}",
        snapshot_set.display()
    );
    let marker = snapshot_set.join(".committed");
    let marker_metadata = std::fs::symlink_metadata(&marker)
        .with_context(|| format!("inspect snapshot marker {}", marker.display()))?;
    ensure!(
        marker_metadata.is_file() && !marker_metadata.file_type().is_symlink(),
        "Snowball snapshot has no regular commit marker: {}",
        snapshot_set.display()
    );
    let state_dir = snapshot_set.join("state");
    let state_dir_metadata = std::fs::symlink_metadata(&state_dir)
        .with_context(|| format!("inspect snapshot state directory {}", state_dir.display()))?;
    ensure!(
        state_dir_metadata.is_dir() && !state_dir_metadata.file_type().is_symlink(),
        "Snowball snapshot state path is not a real directory: {}",
        state_dir.display()
    );
    let state_file = state_dir.join("current.json");
    let state_metadata = match std::fs::symlink_metadata(&state_file) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        state_metadata.is_file() && !state_metadata.file_type().is_symlink(),
        "Snowball snapshot current state is not a regular file"
    );
    let data = std::fs::read(&state_file)
        .with_context(|| format!("read snapshot state {}", state_file.display()))?;
    Ok(Some(serde_json::from_slice(&data).with_context(|| {
        format!("invalid snapshot state {}", state_file.display())
    })?))
}

fn prune_snapshot_sets_sync(snapshot_dir: &Path, policy: &RetentionPolicy) -> Result<()> {
    let mut snapshots = Vec::new();
    for entry in std::fs::read_dir(snapshot_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(counter) = parse_snapshot_set_counter(&name) else {
            continue;
        };
        let file_type = entry.file_type()?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }
        let marker = entry.path().join(".committed");
        let committed = match std::fs::symlink_metadata(marker) {
            Ok(metadata) => metadata.is_file() && !metadata.file_type().is_symlink(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if !committed {
            continue;
        }
        let metadata = entry.metadata()?;
        let created = metadata
            .created()
            .or_else(|_| metadata.modified())
            .ok()
            .and_then(system_time_to_utc);
        if let Some(created) = created {
            snapshots.push(SnapshotEntry {
                name,
                created,
                counter,
            });
        }
    }

    let keep = retain_snapshots(&mut snapshots, policy, Utc::now());
    let mut deleted = 0_usize;
    for snapshot in snapshots {
        if keep.contains(&snapshot.name) {
            continue;
        }
        std::fs::remove_dir_all(snapshot_dir.join(&snapshot.name))?;
        deleted += 1;
        debug!("Pruned snapshot: {}", snapshot.name);
    }
    if deleted > 0 {
        StdFile::open(snapshot_dir)?.sync_all()?;
        info!(
            "Pruned {} snapshots (retention: {}h/{}d/{}w/{}q)",
            deleted, policy.hourly, policy.daily, policy.weekly, policy.quarterly
        );
    }
    Ok(())
}

fn commit_snapshot_set_sync(
    snapshot_dir: &Path,
    counter: u64,
    sources: &[(String, PathBuf); 3],
) -> Result<()> {
    let snapshot_metadata = std::fs::symlink_metadata(snapshot_dir)
        .with_context(|| format!("inspect snapshots directory {}", snapshot_dir.display()))?;
    ensure!(
        snapshot_metadata.is_dir() && !snapshot_metadata.file_type().is_symlink(),
        "Snowball snapshots path is not a real directory: {}",
        snapshot_dir.display()
    );
    let name = StreamingSnowball::snapshot_set_name(counter);
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .context("system clock is before the Unix epoch")?
        .as_nanos();
    let staging = snapshot_dir.join(format!(".{name}-{}-{nonce}.tmp", std::process::id()));
    std::fs::create_dir(&staging)?;
    let result = (|| {
        for (label, source) in sources {
            copy_dir_recursive_sync(source, &staging.join(label))?;
        }
        let marker = staging.join(".committed");
        let marker_file = StdOpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&marker)?;
        marker_file.sync_all()?;
        StdFile::open(&staging)?.sync_all()?;
        std::fs::rename(&staging, snapshot_dir.join(&name))?;
        StdFile::open(snapshot_dir)?.sync_all()?;
        info!("Committed Snowball logical snapshot: {name}");
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

fn copy_dir_recursive_sync(src: &Path, dst: &Path) -> Result<()> {
    let source_metadata = std::fs::symlink_metadata(src)
        .with_context(|| format!("inspect Snowball snapshot source {}", src.display()))?;
    ensure!(
        source_metadata.is_dir() && !source_metadata.file_type().is_symlink(),
        "Snowball snapshot source is not a real directory: {}",
        src.display()
    );
    std::fs::create_dir(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let displayed_name = file_name.to_string_lossy();
        if displayed_name.starts_with('.') && displayed_name.ends_with(".tmp") {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(file_name);
        let file_type = entry.file_type()?;
        ensure!(
            !file_type.is_symlink(),
            "Snowball snapshots do not follow symlinks: {}",
            src_path.display()
        );
        if file_type.is_dir() {
            copy_dir_recursive_sync(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&src_path, &dst_path)?;
            StdFile::open(&dst_path)?.sync_all()?;
        } else {
            anyhow::bail!(
                "Snowball snapshot source is not a regular file: {}",
                src_path.display()
            );
        }
    }
    StdFile::open(dst)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    struct StrictStateKeysEnvironment {
        previous: Option<std::ffi::OsString>,
        _guard: tokio::sync::MutexGuard<'static, ()>,
    }

    impl StrictStateKeysEnvironment {
        async fn set(strict: bool) -> Self {
            static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
            let guard = LOCK.lock().await;
            let previous = std::env::var_os("OPDBUS_STRICT_STATE_KEYS");
            if strict {
                std::env::set_var("OPDBUS_STRICT_STATE_KEYS", "1");
            } else {
                std::env::remove_var("OPDBUS_STRICT_STATE_KEYS");
            }
            Self {
                previous,
                _guard: guard,
            }
        }
    }

    impl Drop for StrictStateKeysEnvironment {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                std::env::set_var("OPDBUS_STRICT_STATE_KEYS", previous);
            } else {
                std::env::remove_var("OPDBUS_STRICT_STATE_KEYS");
            }
        }
    }

    const TIMING_RECORD: &[u8] = br#"{
      "timestamp": 1786156636821,
      "category": "zeroclaw",
      "action": "Chat",
      "data": {
        "plugin_id": "zeroclaw",
        "operation": "Chat",
        "payload": { "message": "hello" },
        "metadata": { "event_hash": "97aadd58", "decision": "Allow", "event_id": 89 }
      },
      "hash": "027ef6db",
      "vector": []
    }"#;

    #[tokio::test]
    async fn separate_handles_allocate_distinct_committed_snapshot_counters() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("snowball");
        let first = Arc::new(StreamingSnowball::new(&path).await.unwrap());
        let second = Arc::new(StreamingSnowball::new(&path).await.unwrap());
        first
            .write_current_state(&simd_json::json!({"generation": 1}))
            .await
            .unwrap();

        let first_task = tokio::spawn({
            let first = Arc::clone(&first);
            async move { first.create_snapshot().await.unwrap() }
        });
        let second_task = tokio::spawn({
            let second = Arc::clone(&second);
            async move { second.create_snapshot().await.unwrap() }
        });
        let mut names = vec![first_task.await.unwrap(), second_task.await.unwrap()];
        names.sort();
        assert_eq!(names, ["SNP-000001", "SNP-000002"]);
        assert_eq!(first.list_snapshots().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn separate_handles_allocate_distinct_block_numbers_from_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("snowball");
        let first = Arc::new(StreamingSnowball::new(&path).await.unwrap());
        let second = Arc::new(StreamingSnowball::new(&path).await.unwrap());
        let first_write = tokio::spawn({
            let first = Arc::clone(&first);
            async move {
                first
                    .add_event(BlockEvent::new("fixture", "one", simd_json::json!({})))
                    .await
                    .unwrap()
            }
        });
        let second_write = tokio::spawn({
            let second = Arc::clone(&second);
            async move {
                second
                    .add_event(BlockEvent::new("fixture", "two", simd_json::json!({})))
                    .await
                    .unwrap()
            }
        });
        first_write.await.unwrap();
        second_write.await.unwrap();
        let numbers: Vec<u64> = first
            .blocks()
            .await
            .unwrap()
            .into_iter()
            .map(|block| block.block_num)
            .collect();
        assert_eq!(numbers, [1, 2]);
    }

    #[tokio::test]
    async fn concurrent_event_and_snapshot_never_split_timing_from_vector() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("snowball");
        let writer = Arc::new(StreamingSnowball::new(&path).await.unwrap());
        let snapshooter = Arc::new(StreamingSnowball::new(&path).await.unwrap());
        let event = BlockEvent::new("fixture", "write", simd_json::json!({"id": 1}))
            .with_vector(vec![0.25, 0.5]);
        let write = tokio::spawn({
            let writer = Arc::clone(&writer);
            async move { writer.add_event(event).await.unwrap() }
        });
        let snapshot = tokio::spawn({
            let snapshooter = Arc::clone(&snapshooter);
            async move { snapshooter.create_snapshot().await.unwrap() }
        });
        write.await.unwrap();
        let name = snapshot.await.unwrap();
        let root = path.join("snapshots").join(name);
        let timing = std::fs::read_dir(root.join("timing")).unwrap().count();
        let vectors = std::fs::read_dir(root.join("vectors")).unwrap().count();
        assert_eq!(
            timing, vectors,
            "a committed snapshot must contain both projections or neither"
        );
    }

    #[tokio::test]
    async fn retention_ignores_staging_and_preserves_unknown_snapshot_names() {
        let dir = tempdir().unwrap();
        let chain = StreamingSnowball::new(dir.path().join("snowball"))
            .await
            .unwrap();
        let snapshots = chain.base_path().join("snapshots");
        tokio::fs::create_dir(snapshots.join(".SNP-000007-fixture.tmp"))
            .await
            .unwrap();
        let sentinel = snapshots.join("SNP-state-manual");
        tokio::fs::create_dir(&sentinel).await.unwrap();
        chain
            .set_retention_policy(RetentionPolicy::minimal())
            .await
            .unwrap();
        assert!(chain.list_snapshots().await.unwrap().is_empty());
        assert!(tokio::fs::try_exists(sentinel).await.unwrap());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn refuses_symlinked_chain_root() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let target = dir.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let linked = dir.path().join("linked");
        symlink(&target, &linked).unwrap();
        assert!(StreamingSnowball::new(linked).await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_snapshot_removes_hidden_staging_directory() {
        use std::os::unix::fs::symlink;

        let dir = tempdir().unwrap();
        let chain = StreamingSnowball::new(dir.path().join("snowball"))
            .await
            .unwrap();
        let outside = dir.path().join("outside");
        std::fs::write(&outside, b"private").unwrap();
        symlink(&outside, chain.state_subvolume_path().join("redirect")).unwrap();

        assert!(chain.create_snapshot().await.is_err());
        assert!(chain.list_snapshots().await.unwrap().is_empty());
        let leftovers: Vec<_> = std::fs::read_dir(chain.base_path().join("snapshots"))
            .unwrap()
            .collect::<std::io::Result<_>>()
            .unwrap();
        assert!(leftovers.is_empty(), "failed snapshot left staging data");
        assert_eq!(std::fs::read(outside).unwrap(), b"private");
    }

    #[test]
    fn vector_round_trips_through_on_disk_encoding() {
        let original = vec![0.25f32, -0.5, 1.5, 0.0];
        let decoded = decode_vector(&encode_vector(&original)).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn rejects_truncated_or_empty_vector_files() {
        assert!(decode_vector(&[]).is_err(), "empty file is not a vector");
        assert!(
            decode_vector(&[1, 2, 3]).is_err(),
            "length must be a multiple of 4"
        );
    }

    #[test]
    fn parses_block_and_vector_file_names() {
        assert_eq!(parse_block_number("block-000000000042.json"), Some(42));
        assert_eq!(parse_vector_block_number("vec-000000000042.bin"), Some(42));
        assert_eq!(parse_block_number("vec-000000000042.bin"), None);
        assert_eq!(parse_block_number("notes.txt"), None);
    }

    #[test]
    fn block_ref_reads_timing_record_and_flattens_data() {
        let block = ChainBlockRef::from_timing_bytes(42, TIMING_RECORD.to_vec(), false).unwrap();

        assert_eq!(block.block_num, 42);
        assert_eq!(block.category, "zeroclaw");
        assert_eq!(block.action, "Chat");
        assert_eq!(block.timestamp, 1786156636821);
        assert_eq!(block.field("data.metadata.event_hash"), Some("97aadd58"));
        assert!(!block.has_vector);
    }

    #[test]
    fn embedding_text_is_deterministic_and_sorted() {
        let first = ChainBlockRef::from_timing_bytes(42, TIMING_RECORD.to_vec(), false).unwrap();
        let second = ChainBlockRef::from_timing_bytes(42, TIMING_RECORD.to_vec(), false).unwrap();
        assert_eq!(
            first.embedding_text(),
            second.embedding_text(),
            "identical timing records must render identical text so origin and \
             replica derive the same vector"
        );

        let keys: Vec<&String> = first.data_fields.iter().map(|(key, _)| key).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "flattened fields must be key-sorted");
        assert!(first
            .embedding_text()
            .starts_with("block: 42\nplugin: zeroclaw"));
    }

    #[test]
    fn long_fields_are_capped_in_embedding_text() {
        let long = "x".repeat(MAX_EMBEDDING_FIELD_LEN * 2);
        let record = format!(
            r#"{{"timestamp":1,"category":"c","action":"a","hash":"h","vector":[],"data":{{"big":"{long}"}}}}"#
        );
        let block = ChainBlockRef::from_timing_bytes(1, record.into_bytes(), true).unwrap();
        let value = block.field("data.big").unwrap();
        assert!(
            value.chars().count() <= MAX_EMBEDDING_FIELD_LEN + 1,
            "one oversized payload field must not dominate the vector"
        );
    }

    fn entry(name: &str, created: DateTime<Utc>, counter: u64) -> SnapshotEntry {
        SnapshotEntry {
            name: name.to_string(),
            created,
            counter,
        }
    }

    #[test]
    fn retention_never_prunes_the_newest_snapshot_on_a_timestamp_tie() {
        // Filesystems can report tied timestamps; the logical counter remains
        // the authoritative ordering key.
        let stale = Utc::now() - chrono::Duration::days(10);
        let mut snapshots = vec![entry("SNP-000001", stale, 1), entry("SNP-000002", stale, 2)];
        let policy = RetentionPolicy {
            hourly: 0,
            daily: 1,
            weekly: 0,
            quarterly: 0,
        };

        let keep = retain_snapshots(&mut snapshots, &policy, Utc::now());
        assert!(
            keep.contains("SNP-000002"),
            "the newest counter must always survive: {keep:?}"
        );
    }

    #[test]
    fn retention_still_prunes_beyond_the_policy() {
        let now = Utc::now();
        let mut snapshots = vec![
            entry("SNP-000001", now - chrono::Duration::hours(1), 1),
            entry("SNP-000002", now - chrono::Duration::hours(2), 2),
            entry("SNP-000003", now - chrono::Duration::hours(3), 3),
        ];
        let policy = RetentionPolicy {
            hourly: 1,
            daily: 0,
            weekly: 0,
            quarterly: 0,
        };

        let keep = retain_snapshots(&mut snapshots, &policy, now);
        assert_eq!(
            keep.len(),
            2,
            "one hourly slot plus the mandatory newest counter: {keep:?}"
        );
        assert!(keep.contains("SNP-000001"), "newest by time");
        assert!(keep.contains("SNP-000003"), "highest counter");
    }

    #[test]
    fn snapshot_set_name_is_canonical() {
        assert_eq!(StreamingSnowball::snapshot_set_name(7), "SNP-000007");
    }

    #[tokio::test]
    async fn resumes_block_numbering_from_disk() {
        let dir = std::env::temp_dir().join(format!("op-chain-test-{}", uuid::Uuid::new_v4()));
        let timing = dir.join("timing");
        tokio::fs::create_dir_all(&timing).await.unwrap();
        assert_eq!(highest_block_number(&timing).await.unwrap(), 0);

        tokio::fs::write(timing.join(timing_file_name(7)), b"{}")
            .await
            .unwrap();
        tokio::fs::write(timing.join(timing_file_name(3)), b"{}")
            .await
            .unwrap();
        tokio::fs::write(timing.join("unrelated.json"), b"{}")
            .await
            .unwrap();

        assert_eq!(
            highest_block_number(&timing).await.unwrap(),
            7,
            "a restart must append after the highest existing block, not overwrite from 1"
        );
        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn write_state_validated_allows_declared_keys() {
        let dir =
            std::env::temp_dir().join(format!("op-snowball-wsv-test-{}", uuid::Uuid::new_v4()));
        let snowball = StreamingSnowball::new(&dir).await.unwrap();

        let declared = vec!["plugin_foo".to_string(), "plugin_bar/*".to_string()];
        let value = simd_json::OwnedValue::Static(simd_json::StaticNode::Bool(true));

        // Exact match
        assert!(snowball
            .write_state_validated("plugin_foo", &value, &declared)
            .await
            .is_ok());

        // Prefix match: "plugin_bar_baz" starts with "plugin_bar" (from "plugin_bar/*")
        assert!(snowball
            .write_state_validated("plugin_bar_baz", &value, &declared)
            .await
            .is_ok());

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn write_state_validated_warns_but_allows_undeclared_in_non_strict() {
        let _environment = StrictStateKeysEnvironment::set(false).await;
        let dir = std::env::temp_dir().join(format!(
            "op-snowball-wsv-nonstrict-{}",
            uuid::Uuid::new_v4()
        ));
        let snowball = StreamingSnowball::new(&dir).await.unwrap();

        let declared = vec!["plugin_foo".to_string()];
        let value = simd_json::OwnedValue::Static(simd_json::StaticNode::Bool(true));

        // Undeclared key should succeed (warn, not block)
        assert!(snowball
            .write_state_validated("undeclared_key", &value, &declared)
            .await
            .is_ok());

        tokio::fs::remove_dir_all(&dir).await.ok();
    }

    #[tokio::test]
    async fn write_state_validated_rejects_undeclared_in_strict_mode() {
        let _environment = StrictStateKeysEnvironment::set(true).await;
        let dir =
            std::env::temp_dir().join(format!("op-snowball-wsv-strict-{}", uuid::Uuid::new_v4()));
        let snowball = StreamingSnowball::new(&dir).await.unwrap();

        let declared = vec!["plugin_foo".to_string()];
        let value = simd_json::OwnedValue::Static(simd_json::StaticNode::Bool(true));

        // Undeclared key should fail in strict mode
        let result = snowball
            .write_state_validated("undeclared_key", &value, &declared)
            .await;
        assert!(result.is_err());

        // Declared key should still succeed
        assert!(snowball
            .write_state_validated("plugin_foo", &value, &declared)
            .await
            .is_ok());

        tokio::fs::remove_dir_all(&dir).await.ok();
    }
}
