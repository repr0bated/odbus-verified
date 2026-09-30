//! Streaming snowball with Btrfs timing, vector, and state subvolumes
//!
//! Architecture:
//! - timing: Immutable audit trail (append-only)
//! - vectors: ML embeddings for semantic search
//! - state: Current system state for DR/reinstall

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Datelike, Utc};
use nix::fcntl::{Flock, FlockArg};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, HashSet};
use std::fs::{File as StdFile, OpenOptions as StdOpenOptions};
use std::io::Write as _;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use std::time::SystemTime;
use tokio::sync::{Mutex, RwLock, oneshot};
use tokio::time::Instant;
use tracing::{debug, info, warn};

use crate::footprint::{BlockEvent, PluginFootprint};
use crate::retention::RetentionPolicy;
use crate::snapshot::SnapshotInterval;

/// The three Btrfs subvolumes that make up one committed recovery set.
/// Order is stable for deterministic snapshot construction.
pub const SNAPSHOT_LABELS: [&str; 3] = ["timing", "vectors", "state"];

/// All three received members are reported only after every native transfer
/// has succeeded. The destination directory is one prepared remote set path.
#[derive(Debug)]
pub struct SnowballTransferReport {
    pub snapshot_name: String,
    pub members: Vec<crate::replication::PreparedSnapshotReport>,
}

/// Cap on a single flattened field's length in embedding text, so one large
/// payload cannot dominate a block's vector.
const MAX_EMBEDDING_FIELD_LEN: usize = 512;
/// Admission bounds queued closures; timing history has its own retention owner.
const MAX_PENDING_WRITER_JOBS: usize = 64;

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

/// Runs once on the dedicated writer thread before it accepts work. This is
/// where `OptimizedSnowball` applies CPU affinity and the NUMA memory policy,
/// so the pin lands on the thread that actually performs timing, vector and
/// cache writes — never on a tokio worker.
pub type WriterPlacement = Box<dyn FnOnce() -> Result<()> + Send + 'static>;

type WriterJob = Box<dyn FnOnce() + Send + 'static>;

/// The single thread that owns every durable write of a snowball.
///
/// Timing blocks, vector files and Btrfs cache blocks for one event are
/// produced by the same dedicated thread. CPU affinity pins its execution,
/// while the requested NUMA memory policy is best effort and does not guarantee
/// page or filesystem placement. This is also the serialization point that
/// the writer flock assumes.
struct SnowballWriter {
    jobs: tokio::sync::mpsc::Sender<WriterJob>,
}

impl SnowballWriter {
    fn spawn(name: String, placement: Option<WriterPlacement>) -> Result<Self> {
        Self::spawn_with_capacity(name, placement, MAX_PENDING_WRITER_JOBS)
    }

    fn spawn_with_capacity(
        name: String,
        placement: Option<WriterPlacement>,
        capacity: usize,
    ) -> Result<Self> {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<WriterJob>(capacity);
        let thread_name = name.clone();
        std::thread::Builder::new()
            .name(name)
            .spawn(move || {
                if let Some(place) = placement
                    && let Err(error) = place()
                {
                    warn!("{thread_name}: writer placement failed, continuing unpinned: {error}");
                }
                while let Some(job) = rx.blocking_recv() {
                    // A panicking job must not take the writer down with it;
                    // the caller sees a dropped oneshot and reports an error.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                }
                debug!("{thread_name}: writer thread exiting");
            })
            .context("spawn snowball writer thread")?;
        Ok(Self { jobs: tx })
    }

    /// Run `f` on the writer thread and await its result.
    async fn run<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        // Waiting for admission yields the async caller; the pinned writer is
        // the only blocking receiver. Cancellation before admission submits
        // nothing. Once submitted, a job still completes if its caller leaves.
        let permit = self
            .jobs
            .reserve()
            .await
            .map_err(|_| anyhow::anyhow!("snowball writer thread is gone"))?;
        let (reply_tx, reply_rx) = oneshot::channel();
        let job: WriterJob = Box::new(move || {
            let _ = reply_tx.send(f());
        });
        permit.send(job);
        reply_rx
            .await
            .context("snowball writer job did not complete")?
    }
}

/// Streaming snowball with Btrfs subvolumes for timing, vectors, and state
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
    writer: SnowballWriter,
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
        Self::new_with_placement(base_path, snapshot_interval, None).await
    }

    /// Create with a specific snapshot interval and a writer-thread placement
    /// hook (see [`WriterPlacement`]). `None` runs the writer unpinned.
    pub async fn new_with_placement(
        base_path: impl AsRef<Path>,
        snapshot_interval: SnapshotInterval,
        placement: Option<WriterPlacement>,
    ) -> Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();
        let timing_subvol = base_path.join("timing");
        let vector_subvol = base_path.join("vectors");
        let state_subvol = base_path.join("state");

        // The configured root and all children must be real directories. A
        // symlinked root could redirect durable state outside the golden tree.
        Self::ensure_data_dir(&base_path).await?;

        // Existing data is never flattened or migrated implicitly. A plain
        // directory at any of these paths needs an explicit offline migration.
        Self::ensure_source_subvolume(&timing_subvol).await?;
        Self::ensure_source_subvolume(&vector_subvol).await?;
        Self::ensure_source_subvolume(&state_subvol).await?;

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

        let writer = SnowballWriter::spawn("snowball-writer".to_string(), placement)?;

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
            writer,
        })
    }

    /// Run a blocking closure on the snowball's pinned writer thread without
    /// taking the timing writer lock. Used for side files that must stay on the
    /// same NUMA node as the stream (the Btrfs cache block for an event).
    pub async fn run_on_writer<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        self.writer.run(f).await
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

    async fn ensure_source_subvolume(path: &Path) -> Result<()> {
        match tokio::fs::symlink_metadata(path).await {
            Ok(_) => verify_subvolume_root(path).with_context(|| {
                format!(
                    "existing Snowball data path requires a Btrfs subvolume: {}",
                    path.display()
                )
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let output = tokio::process::Command::new("btrfs")
                    .args(["subvolume", "create"])
                    .arg(path)
                    .output()
                    .await
                    .with_context(|| format!("create Snowball subvolume {}", path.display()))?;
                ensure!(
                    output.status.success(),
                    "btrfs subvolume create {} failed: {}",
                    path.display(),
                    String::from_utf8_lossy(&output.stderr).trim()
                );
                verify_subvolume_root(path)
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn writer_transaction<T, F>(&self, transaction: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T> + Send + 'static,
    {
        let local = Arc::clone(&self.snapshot_lock).lock_owned().await;
        let path = self.writer_lock_path.clone();
        self.writer
            .run(move || {
                let _local = local;
                let file = open_writer_lock(&path)?;
                let _file = Flock::lock(file, FlockArg::LockExclusive)
                    .map_err(|(_, error)| anyhow::anyhow!("lock {}: {error}", path.display()))?;
                transaction()
            })
            .await
            .context("Snowball writer transaction failed")
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

    /// Sidecar next to a vector: what the projector observed when it embedded
    /// (host load, latencies). Travels with the vector through send/receive;
    /// never part of the hashed timing record.
    pub fn vector_meta_path(&self, block_num: u64) -> PathBuf {
        self.vector_subvol
            .join(format!("{}.meta.json", vector_file_name(block_num)))
    }

    /// Write the vector's sidecar metadata (see [`Self::vector_meta_path`]).
    pub async fn attach_vector_meta(
        &self,
        block_num: u64,
        meta: &simd_json::OwnedValue,
    ) -> Result<PathBuf> {
        let vector_file = self.vector_path(block_num);
        let vector_dir = self.vector_subvol.clone();
        let meta_file = self.vector_meta_path(block_num);
        let committed = meta_file.clone();
        let bytes = simd_json::to_vec(meta)?;
        self.writer_transaction(move || {
            ensure!(
                vector_file.is_file(),
                "no vector for block {block_num}; attach the vector before its metadata"
            );
            atomic_write_sync(
                &vector_dir,
                &committed,
                &format!(".vector-meta-{block_num:012}"),
                &bytes,
            )
        })
        .await?;
        Ok(meta_file)
    }

    /// Read a vector's sidecar metadata, if any.
    pub async fn read_vector_meta(&self, block_num: u64) -> Result<Option<simd_json::OwnedValue>> {
        match tokio::fs::read(self.vector_meta_path(block_num)).await {
            Ok(mut bytes) => Ok(Some(simd_json::to_owned_value(&mut bytes)?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
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
        Ok(self.pending_blocks(usize::MAX).await?.0)
    }

    /// Select pending timing records before parsing their payloads.
    ///
    /// Full filename/presence scans retain at most `limit` candidate paths,
    /// independent of retained history. Numeric ordering includes gaps below
    /// any index watermark. Malformed JSON is warned/skipped as in `blocks`;
    /// another bounded scan fills the batch after those records, so a corrupt
    /// old record cannot hide later valid work. I/O failures are returned.
    /// The second return value counts timing filenames with a vector present,
    /// without parsing already-projected timing documents.
    pub async fn pending_blocks(&self, limit: usize) -> Result<(Vec<ChainBlockRef>, usize)> {
        pending_blocks_in(&self.timing_subvol, &self.vector_subvol, limit).await
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
    /// Each published set is an ordinary directory containing three aligned,
    /// read-only Btrfs subvolume snapshots. The enclosing directory and its
    /// commit marker give readers one complete recovery point.
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

    /// Send a complete read-only Btrfs triple to an operator-prepared remote
    /// directory dedicated to this counter. No remote set is advertised by
    /// this method after a partial receive; the caller must publish it only
    /// after the returned three-member report. The parent, when supplied,
    /// must be a complete local native triple already present remotely. Both
    /// local sets are pinned against retention for the transfer's duration;
    /// the caller remains responsible for the remote parent's existence.
    pub async fn transfer_snapshot_set(
        &self,
        snapshot_name: &str,
        parent_name: Option<&str>,
        host: &str,
        remote_set_directory: &str,
        timeout: Duration,
    ) -> Result<SnowballTransferReport> {
        Self::parse_snapshot_set_counter(snapshot_name)
            .with_context(|| format!("invalid Btrfs Snowball set name: {snapshot_name}"))?;
        let directory = self.base_path.join("snapshots");
        let set = directory.join(snapshot_name);
        let parent = parent_name.map(|name| directory.join(name));
        if let Some(name) = parent_name {
            Self::parse_snapshot_set_counter(name)
                .with_context(|| format!("invalid Btrfs parent set name: {name}"))?;
        }
        let lock_set = set.clone();
        let lock_parent = parent.clone();
        let _pins = self
            .writer_transaction(move || {
                complete_btrfs_snapshot_set(&lock_set)?;
                if let Some(parent) = &lock_parent {
                    complete_btrfs_snapshot_set(parent)?;
                }
                SnapshotTransferPins::create(&lock_set, lock_parent.as_deref())
            })
            .await?;

        let mut members = Vec::with_capacity(SNAPSHOT_LABELS.len());
        for label in SNAPSHOT_LABELS {
            let request = crate::replication::PreparedSnapshotTransfer {
                snapshot: set.join(label),
                parent: parent.as_ref().map(|path| path.join(label)),
                host: host.to_owned(),
                remote_directory: remote_set_directory.to_owned(),
                on_receive: None,
                timeout,
            };
            match crate::replication::transfer_prepared_snapshot(request).await {
                Ok(report) => members.push(report),
                Err(error) => {
                    return Err(error).with_context(|| format!(
                        "Snowball set {snapshot_name} is incomplete at remote destination; {} member(s) transferred before {label} failed",
                        members.len()
                    ));
                }
            }
        }
        Ok(SnowballTransferReport {
            snapshot_name: snapshot_name.to_owned(),
            members,
        })
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
            let file_type = entry.file_type().await?;
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let valid = if Self::parse_snapshot_set_counter(&name).is_some() {
                // A marker alone is insufficient: all three aligned members
                // must exist and have one consistent storage form.
                complete_snapshot_set(&entry.path()).is_ok()
            } else if let Some(counter) = parse_legacy_state_counter(&name) {
                complete_legacy_snapshot_triple(&snapshot_dir, counter).is_ok()
            } else {
                false
            };
            if !valid {
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
        let snapshot_dir = self.base_path.join("snapshots");
        if let Some(counter) = parse_legacy_state_counter(snapshot_name) {
            complete_legacy_snapshot_triple(&snapshot_dir, counter)?;
            let state_file = snapshot_dir.join(snapshot_name).join("current.json");
            return read_state_file_async(&state_file).await;
        }
        let snapshot_set = snapshot_dir.join(snapshot_name);
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
        complete_snapshot_set(&snapshot_set)?;
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
                    .with_context(|| format!("read snapshot state {}", state_file.display()));
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
            .or_else(|| parse_legacy_state_counter(snapshot_name))
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

type TimingCandidate = (u64, PathBuf);

fn select_pending_timing(
    timing: &Path,
    vectors: &Path,
    after: Option<&TimingCandidate>,
    limit: usize,
) -> Result<(Vec<TimingCandidate>, usize)> {
    let mut candidates = BinaryHeap::new();
    let mut already_present = 0;
    for entry in std::fs::read_dir(timing)
        .with_context(|| format!("failed to read timing directory {}", timing.display()))?
    {
        let entry = entry?;
        let Some(number) = parse_block_number(&entry.file_name().to_string_lossy()) else {
            continue;
        };
        let has_vector = vectors
            .join(vector_file_name(number))
            .try_exists()
            .with_context(|| format!("failed to inspect vector for block {number}"))?;
        if has_vector {
            already_present += 1;
            continue;
        }
        if limit == 0 {
            continue;
        }
        let candidate = (number, entry.path());
        if after.is_some_and(|after| &candidate <= after) {
            continue;
        }
        if candidates.len() < limit {
            candidates.push(candidate);
        } else if candidates.peek().is_some_and(|last| &candidate < last) {
            candidates.pop();
            candidates.push(candidate);
        }
    }
    Ok((candidates.into_sorted_vec(), already_present))
}

async fn pending_blocks_in(
    timing: &Path,
    vectors: &Path,
    limit: usize,
) -> Result<(Vec<ChainBlockRef>, usize)> {
    let mut blocks = Vec::new();
    let mut after = None;
    loop {
        let timing = timing.to_path_buf();
        let vectors = vectors.to_path_buf();
        let cursor = after.clone();
        let remaining = limit - blocks.len();
        let (selected, already_present) = tokio::task::spawn_blocking(move || {
            select_pending_timing(&timing, &vectors, cursor.as_ref(), remaining)
        })
        .await
        .context("Snowball pending selection task failed")??;
        if selected.is_empty() {
            return Ok((blocks, already_present));
        }
        for (number, path) in selected {
            let bytes = tokio::fs::read(&path)
                .await
                .with_context(|| format!("failed to read timing record {}", path.display()))?;
            match ChainBlockRef::from_timing_bytes(number, bytes, false) {
                Ok(block) => blocks.push(block),
                Err(error) => warn!(
                    "skipping unparseable timing record {}: {}",
                    path.display(),
                    error
                ),
            }
            after = Some((number, path));
        }
        if blocks.len() == limit {
            return Ok((blocks, already_present));
        }
    }
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
#[derive(Debug, Clone, PartialEq)]
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
    /// The record's `data` object as written (footprint payload + metadata),
    /// for schema-described projections that need the structured value.
    pub data: simd_json::OwnedValue,
}

impl ChainBlockRef {
    fn from_timing_bytes(block_num: u64, mut bytes: Vec<u8>, has_vector: bool) -> Result<Self> {
        let mut value = simd_json::to_owned_value(&mut bytes)
            .map_err(|e| anyhow::anyhow!("invalid JSON in timing record: {e}"))?;

        let mut data_fields = Vec::new();
        // Move the structured payload out of the parsed record rather than
        // cloning every nested value before flattening it.
        let data = match &mut value {
            simd_json::OwnedValue::Object(map) => map.remove("data"),
            _ => None,
        }
        .unwrap_or(simd_json::OwnedValue::Static(simd_json::StaticNode::Null));
        flatten_sorted(&data, "data", &mut data_fields);

        Ok(Self {
            block_num,
            hash: owned_str(&value, "hash").unwrap_or_default(),
            category: owned_str(&value, "category").unwrap_or_default(),
            action: owned_str(&value, "action").unwrap_or_default(),
            timestamp: owned_u64(&value, "timestamp").unwrap_or_default(),
            data_fields,
            has_vector,
            data,
        })
    }

    /// Render retrieval text from caller-selected `(label, value)` lines under
    /// the same deterministic header as [`Self::embedding_text`]. Used by the
    /// schema-described projection, which decides what is signal and what is
    /// noise; the header keeps block identity stable across policies.
    pub fn embedding_text_from(&self, lines: &[(String, String)]) -> String {
        use std::fmt::Write;
        let mut out = format!(
            "block: {}\nplugin: {}\naction: {}",
            self.block_num, self.category, self.action
        );
        for (label, value) in lines {
            let _ = write!(out, "\n{label}: {value}");
        }
        out
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
        use std::fmt::Write;
        let mut text = format!(
            "block: {}\nplugin: {}\naction: {}\nhash: {}\ntimestamp: {}",
            self.block_num, self.category, self.action, self.hash, self.timestamp
        );
        for (key, value) in &self.data_fields {
            let _ = write!(text, "\n{key}: {value}");
        }
        text
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

fn bounded_scalar_to_string(value: &simd_json::OwnedValue) -> Option<String> {
    match value {
        simd_json::OwnedValue::String(text) if text.len() > MAX_EMBEDDING_FIELD_LEN => {
            let mut end = MAX_EMBEDDING_FIELD_LEN;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let mut clipped = String::with_capacity(end + '…'.len_utf8());
            clipped.push_str(&text[..end]);
            clipped.push('…');
            Some(clipped)
        }
        other => scalar_to_string(other),
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
            if let Some(text) = bounded_scalar_to_string(scalar) {
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
                .context(format!("failed to read {}", timing_dir.display())));
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
    let result: Result<()> = (|| {
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

fn verify_subvolume_root(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink() && metadata.ino() == 256,
        "Snowball path is not a Btrfs subvolume root: {}",
        path.display()
    );
    let file = StdFile::open(path)?;
    let mut stat = std::mem::MaybeUninit::<nix::libc::statfs>::uninit();
    if unsafe { nix::libc::fstatfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error()).context("probe Snowball filesystem");
    }
    ensure!(
        unsafe { stat.assume_init() }.f_type as u64 == nix::libc::BTRFS_SUPER_MAGIC as u64,
        "Snowball path is not on Btrfs: {}",
        path.display()
    );
    Ok(())
}

fn verify_readonly_snapshot(path: &Path) -> Result<()> {
    verify_subvolume_root(path)?;
    let output = Command::new("btrfs")
        .args(["property", "get", "-ts"])
        .arg(path)
        .arg("ro")
        .output()
        .with_context(|| format!("inspect Btrfs snapshot {}", path.display()))?;
    ensure!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "ro=true",
        "Snowball snapshot is not read-only: {}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

fn btrfs_snapshot_readonly(source: &Path, target: &Path) -> Result<()> {
    verify_subvolume_root(source)?;
    // State may later be restored by key. Reject indirections there before a
    // recovery point is published; Btrfs itself never follows source links.
    if source.file_name().is_some_and(|name| name == "state") {
        reject_state_symlinks(source)?;
    }
    let output = Command::new("btrfs")
        .args(["subvolume", "snapshot", "-r"])
        .arg(source)
        .arg(target)
        .output()
        .with_context(|| format!("snapshot Snowball subvolume {}", source.display()))?;
    ensure!(
        output.status.success(),
        "Btrfs snapshot {} failed: {}",
        target.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    verify_readonly_snapshot(target)
}

fn reject_state_symlinks(directory: &Path) -> Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            "Snowball state snapshot refuses symlink {}",
            entry.path().display()
        );
        if kind.is_dir() {
            reject_state_symlinks(&entry.path())?;
        } else {
            ensure!(
                kind.is_file(),
                "Snowball state snapshot refuses special file {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn delete_snapshot_member(path: &Path) -> Result<()> {
    verify_readonly_snapshot(path)?;
    // Btrfs rejects deletion of a read-only subvolume for its unprivileged
    // owner. The set has already been uncommitted before this transition.
    let writable = Command::new("btrfs")
        .args(["property", "set", "-ts"])
        .arg(path)
        .args(["ro", "false"])
        .output()
        .with_context(|| {
            format!(
                "make Snowball snapshot writable for deletion {}",
                path.display()
            )
        })?;
    ensure!(
        writable.status.success(),
        "Btrfs snapshot property update {} failed: {}",
        path.display(),
        String::from_utf8_lossy(&writable.stderr).trim()
    );
    let output = Command::new("btrfs")
        .args(["subvolume", "delete"])
        .arg(path)
        .output()
        .with_context(|| format!("delete Snowball snapshot {}", path.display()))?;
    ensure!(
        output.status.success(),
        "Btrfs snapshot delete {} failed: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(())
}

fn remove_btrfs_snapshot_set(path: &Path) -> Result<()> {
    remove_btrfs_snapshot_set_with(path, delete_snapshot_member)
}

fn remove_btrfs_snapshot_set_with(
    path: &Path,
    delete_member: impl Fn(&Path) -> Result<()>,
) -> Result<()> {
    let marker = path.join(".committed");
    match std::fs::symlink_metadata(&marker) {
        Ok(metadata) => {
            ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "invalid Snowball snapshot marker"
            );
            // A committed set must have all three owned, read-only members
            // before any destructive step. Withdraw it durably first so a
            // failed member deletion cannot leave an advertised partial set.
            complete_btrfs_snapshot_set(path)?;
            std::fs::remove_file(&marker)?;
            StdFile::open(path)?.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // Constructor cleanup may contain only a partial, uncommitted
            // staging triple; validate each existing member below.
        }
        Err(error) => return Err(error.into()),
    }
    for label in SNAPSHOT_LABELS {
        let member = path.join(label);
        match std::fs::symlink_metadata(&member) {
            Ok(_) => delete_member(&member)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    std::fs::remove_dir(path)?;
    Ok(())
}

fn complete_btrfs_snapshot_set(path: &Path) -> Result<()> {
    ensure!(
        complete_snapshot_set(path)?,
        "Snowball set is a legacy copied snapshot, not Btrfs"
    );
    Ok(())
}

/// True for a complete native Btrfs triple; false for a complete historical
/// copied set. A missing or mixed member is never a valid recovery point.
fn complete_snapshot_set(path: &Path) -> Result<bool> {
    let metadata = std::fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "invalid Snowball snapshot set {}",
        path.display()
    );
    let marker = std::fs::symlink_metadata(path.join(".committed"))?;
    ensure!(
        marker.is_file() && !marker.file_type().is_symlink(),
        "Snowball snapshot set is incomplete: {}",
        path.display()
    );
    let mut native = 0;
    for label in SNAPSHOT_LABELS {
        let member = path.join(label);
        let metadata = std::fs::symlink_metadata(&member)?;
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "Snowball snapshot member is missing or unsafe: {}",
            member.display()
        );
        if metadata.ino() == 256 {
            verify_readonly_snapshot(&member)?;
            native += 1;
        }
    }
    ensure!(
        native == 0 || native == 3,
        "mixed copied/Btrfs Snowball snapshot set"
    );
    Ok(native == 3)
}

struct SnapshotTransferPins(Vec<PathBuf>);

impl SnapshotTransferPins {
    fn create(set: &Path, parent: Option<&Path>) -> Result<Self> {
        ensure!(
            parent != Some(set),
            "Snowball transfer parent must differ from source"
        );
        let mut pins = Self(Vec::new());
        for directory in std::iter::once(set).chain(parent) {
            let nonce = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)?
                .as_nanos();
            let path = directory.join(format!(".in-transfer-{}-{nonce}", std::process::id()));
            let file = StdOpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(nix::libc::O_NOFOLLOW)
                .open(&path)?;
            pins.0.push(path);
            file.sync_all()?;
            StdFile::open(directory)?.sync_all()?;
        }
        Ok(pins)
    }
}

impl Drop for SnapshotTransferPins {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
            if let Some(parent) = path.parent() {
                let _ = StdFile::open(parent).and_then(|file| file.sync_all());
            }
        }
    }
}

fn snapshot_transfer_in_progress(path: &Path) -> Result<bool> {
    for entry in std::fs::read_dir(path)? {
        let name = entry?.file_name();
        if name.to_string_lossy().starts_with(".in-transfer-") {
            return Ok(true);
        }
    }
    Ok(false)
}

fn parse_snapshot_set_counter(name: &str) -> Option<u64> {
    let counter = name.strip_prefix("SNP-")?.parse::<u64>().ok()?;
    (name == StreamingSnowball::snapshot_set_name(counter)).then_some(counter)
}

fn parse_legacy_state_counter(name: &str) -> Option<u64> {
    parse_legacy_member_counter(name, "state")
}

fn parse_legacy_member_counter(name: &str, label: &str) -> Option<u64> {
    let counter = name
        .strip_prefix(&format!("SNP-{label}-"))?
        .parse::<u64>()
        .ok()?;
    (name == format!("SNP-{label}-{counter:06}")).then_some(counter)
}

fn complete_legacy_snapshot_triple(snapshot_dir: &Path, counter: u64) -> Result<()> {
    for label in SNAPSHOT_LABELS {
        verify_readonly_snapshot(&snapshot_dir.join(format!("SNP-{label}-{counter:06}")))?;
    }
    Ok(())
}

async fn read_state_file_async(path: &Path) -> Result<Option<simd_json::OwnedValue>> {
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "Snowball snapshot current state is not a regular file"
    );
    let mut data = tokio::fs::read_to_string(path).await?;
    Ok(Some(
        unsafe { simd_json::from_str(&mut data) }
            .with_context(|| format!("invalid snapshot state {}", path.display()))?,
    ))
}

fn next_snapshot_counter_sync(snapshot_dir: &Path) -> Result<u64> {
    let mut max_counter = 0_u64;
    for entry in std::fs::read_dir(snapshot_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(counter) = parse_snapshot_set_counter(&name).or_else(|| {
            SNAPSHOT_LABELS
                .iter()
                .find_map(|label| parse_legacy_member_counter(&name, label))
        }) {
            max_counter = max_counter.max(counter);
        }
    }
    max_counter
        .checked_add(1)
        .context("Snowball snapshot counter overflow")
}

fn read_snapshot_state_sync(snapshot_set: &Path) -> Result<Option<simd_json::OwnedValue>> {
    if let Some(counter) = snapshot_set
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(parse_legacy_state_counter)
    {
        let snapshot_dir = snapshot_set
            .parent()
            .context("legacy snapshot has no parent")?;
        complete_legacy_snapshot_triple(snapshot_dir, counter)?;
        let state_file = snapshot_set.join("current.json");
        return read_snapshot_state_file_sync(&state_file);
    }
    complete_snapshot_set(snapshot_set)?;
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
    read_snapshot_state_file_sync(&state_file)
}

fn read_snapshot_state_file_sync(state_file: &Path) -> Result<Option<simd_json::OwnedValue>> {
    let state_metadata = match std::fs::symlink_metadata(state_file) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        state_metadata.is_file() && !state_metadata.file_type().is_symlink(),
        "Snowball snapshot current state is not a regular file"
    );
    let data = std::fs::read(state_file)
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
        // Existing copied logical sets remain readable but are never silently
        // deleted by Btrfs retention. Only complete native triples are owned
        // by the current snapshot lifecycle.
        if complete_btrfs_snapshot_set(&entry.path()).is_err() {
            continue;
        }
        if snapshot_transfer_in_progress(&entry.path())? {
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
        remove_btrfs_snapshot_set(&snapshot_dir.join(&snapshot.name))?;
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
    // Validate state before creating any read-only member. A recoverable
    // validation failure must not leave an undeletable partial set when the
    // writer lacks privileges to delete read-only Btrfs snapshots.
    for (label, source) in sources {
        if label == "state" {
            reject_state_symlinks(source)?;
        }
    }
    std::fs::create_dir(&staging)?;
    let result: Result<()> = (|| {
        for (label, source) in sources {
            btrfs_snapshot_readonly(source, &staging.join(label))?;
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
        info!("Committed Snowball Btrfs snapshot triple: {name}");
        Ok(())
    })();
    if let Err(error) = result {
        if let Err(cleanup_error) = remove_btrfs_snapshot_set(&staging) {
            return Err(error.context(format!(
                "incomplete Btrfs snapshot staging remains at {}: {cleanup_error:#}",
                staging.display()
            )));
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct BtrfsFixture(TempDir);

    impl BtrfsFixture {
        fn path(&self) -> &Path {
            self.0.path()
        }
    }

    impl Drop for BtrfsFixture {
        fn drop(&mut self) {
            fn collect(path: &Path, subvolumes: &mut Vec<PathBuf>) {
                let Ok(entries) = std::fs::read_dir(path) else {
                    return;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                        continue;
                    };
                    if metadata.is_dir() && !metadata.file_type().is_symlink() {
                        collect(&path, subvolumes);
                        if metadata.ino() == 256 {
                            subvolumes.push(path);
                        }
                    }
                }
            }
            let mut subvolumes = Vec::new();
            collect(self.path(), &mut subvolumes);
            // Children precede parents. A test-created read-only snapshot is
            // made writable solely to let an unprivileged test delete itself.
            for path in subvolumes {
                let _ = Command::new("btrfs")
                    .args(["property", "set", "-ts"])
                    .arg(&path)
                    .args(["ro", "false"])
                    .output();
                let _ = Command::new("btrfs")
                    .args(["subvolume", "delete"])
                    .arg(&path)
                    .output();
            }
        }
    }

    fn tempdir() -> Result<BtrfsFixture> {
        Ok(BtrfsFixture(tempfile::tempdir()?))
    }

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
                // FIXME: Audit that the environment access only happens in single-threaded code.
                unsafe { std::env::set_var("OPDBUS_STRICT_STATE_KEYS", "1") };
            } else {
                // FIXME: Audit that the environment access only happens in single-threaded code.
                unsafe { std::env::remove_var("OPDBUS_STRICT_STATE_KEYS") };
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
                // FIXME: Audit that the environment access only happens in single-threaded code.
                unsafe { std::env::set_var("OPDBUS_STRICT_STATE_KEYS", previous) };
            } else {
                // FIXME: Audit that the environment access only happens in single-threaded code.
                unsafe { std::env::remove_var("OPDBUS_STRICT_STATE_KEYS") };
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

    fn pending_fixture() -> (TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let timing = root.path().join("timing");
        let vectors = root.path().join("vectors");
        std::fs::create_dir(&timing).unwrap();
        std::fs::create_dir(&vectors).unwrap();
        (root, timing, vectors)
    }

    #[tokio::test]
    async fn pending_selection_does_not_parse_large_vectorized_history() {
        let (_root, timing, vectors) = pending_fixture();
        // Already projected records need no payload parsing, even when their
        // historical payload is malformed. Only filenames count as coverage.
        for number in 1..=4096 {
            std::fs::write(timing.join(timing_file_name(number)), b"not JSON").unwrap();
            std::fs::write(
                vectors.join(vector_file_name(number)),
                encode_vector(&[0.25]),
            )
            .unwrap();
        }
        let (pending, already_present) = pending_blocks_in(&timing, &vectors, 256).await.unwrap();
        assert!(pending.is_empty());
        assert_eq!(already_present, 4096);
        assert_eq!(std::fs::read_dir(&timing).unwrap().count(), 4096);
    }

    #[tokio::test]
    async fn pending_selection_recovers_gaps_and_orders_before_limiting() {
        let (_root, timing, vectors) = pending_fixture();
        for number in [900, 7, 2, 100] {
            std::fs::write(timing.join(timing_file_name(number)), TIMING_RECORD).unwrap();
        }
        std::fs::write(vectors.join(vector_file_name(100)), encode_vector(&[0.25])).unwrap();
        // The high-watermark vector does not hide the missing older vectors.
        let (pending, count) = pending_blocks_in(&timing, &vectors, 2).await.unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            pending.iter().map(|b| b.block_num).collect::<Vec<_>>(),
            [2, 7]
        );
        assert!(pending.iter().all(|block| !block.has_vector));
    }

    #[tokio::test]
    async fn pending_selection_refills_after_malformed_oldest_record() {
        let (_root, timing, vectors) = pending_fixture();
        for number in 1..=4 {
            std::fs::write(
                timing.join(timing_file_name(number)),
                if number <= 2 {
                    b"not JSON"
                } else {
                    TIMING_RECORD
                },
            )
            .unwrap();
        }
        let (pending, count) = pending_blocks_in(&timing, &vectors, 1).await.unwrap();
        assert_eq!(count, 0);
        assert_eq!(pending[0].block_num, 3);
        assert_eq!(pending.len(), 1);
        // Repeated passes still reach valid work instead of starving on 1/2.
        std::fs::write(vectors.join(vector_file_name(3)), encode_vector(&[0.25])).unwrap();
        let (next, _) = pending_blocks_in(&timing, &vectors, 1).await.unwrap();
        assert_eq!(next[0].block_num, 4);
        assert_eq!(
            std::fs::read(timing.join(timing_file_name(1))).unwrap(),
            b"not JSON"
        );
    }

    #[tokio::test]
    async fn pending_zero_limit_counts_coverage_without_reading_payloads() {
        let (_root, timing, vectors) = pending_fixture();
        // A selected directory would fail its payload read. With a zero limit
        // it must never be opened as a record.
        std::fs::create_dir(timing.join(timing_file_name(1))).unwrap();
        std::fs::write(timing.join(timing_file_name(2)), b"not JSON").unwrap();
        std::fs::write(vectors.join(vector_file_name(2)), encode_vector(&[0.25])).unwrap();
        let (pending, count) = pending_blocks_in(&timing, &vectors, 0).await.unwrap();
        assert!(pending.is_empty());
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn pending_selection_propagates_selected_io_failure() {
        let (_root, timing, vectors) = pending_fixture();
        std::fs::create_dir(timing.join(timing_file_name(1))).unwrap();
        let error = pending_blocks_in(&timing, &vectors, 1).await.unwrap_err();
        assert!(error.to_string().contains("failed to read timing record"));
        assert!(
            pending_blocks_in(&timing.join("missing"), &vectors, 0)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn pending_retry_keeps_work_after_partial_projection_failure() {
        let (_root, timing, vectors) = pending_fixture();
        for number in [1, 2, 3] {
            std::fs::write(timing.join(timing_file_name(number)), TIMING_RECORD).unwrap();
        }
        let (first, _) = pending_blocks_in(&timing, &vectors, 3).await.unwrap();
        assert_eq!(first.len(), 3);
        // Simulate a pass that committed 1 then failed embedding 2. A later
        // pass sees 2 and 3 regardless of unrelated index watermarks.
        std::fs::write(vectors.join(vector_file_name(1)), encode_vector(&[0.25])).unwrap();
        let (retry, count) = pending_blocks_in(&timing, &vectors, 3).await.unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            retry.iter().map(|b| b.block_num).collect::<Vec<_>>(),
            [2, 3]
        );
    }

    #[tokio::test]
    async fn bounded_writer_admission_yields_and_preserves_fifo() {
        use std::future::Future;
        use std::task::Poll;
        let writer =
            SnowballWriter::spawn_with_capacity("bounded-test-writer".into(), None, 1).unwrap();
        let (release, wait) = std::sync::mpsc::channel();
        let (started_tx, started_rx) = oneshot::channel();
        let mut first = Box::pin(writer.run(move || {
            let _ = started_tx.send(());
            wait.recv().unwrap();
            Ok(1)
        }));
        std::future::poll_fn(|cx| {
            assert!(first.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        started_rx.await.unwrap();
        let mut second = Box::pin(writer.run(|| Ok(2)));
        let mut third = Box::pin(writer.run(|| Ok(3)));
        std::future::poll_fn(|cx| {
            assert!(second.as_mut().poll(cx).is_pending());
            assert_eq!(writer.jobs.capacity(), 0);
            assert!(third.as_mut().poll(cx).is_pending());
            assert_eq!(writer.jobs.capacity(), 0);
            Poll::Ready(())
        })
        .await;
        // This task is still running on a current-thread runtime even though
        // the writer and its queue are both occupied.
        tokio::task::yield_now().await;
        release.send(()).unwrap();
        let (first, second, third) = tokio::join!(first, second, third);
        assert_eq!((first.unwrap(), second.unwrap(), third.unwrap()), (1, 2, 3));
    }

    #[tokio::test]
    async fn bounded_writer_drains_admitted_jobs_after_sender_drop() {
        let writer =
            SnowballWriter::spawn_with_capacity("draining-test-writer".into(), None, 1).unwrap();
        let (reply, result) = oneshot::channel();
        writer
            .jobs
            .send(Box::new(move || {
                let _ = reply.send(42);
            }))
            .await
            .unwrap();
        drop(writer);
        assert_eq!(result.await.unwrap(), 42);
    }

    #[tokio::test]
    async fn bounded_writer_recovers_after_panicking_job() {
        let writer =
            SnowballWriter::spawn_with_capacity("panic-test-writer".into(), None, 1).unwrap();
        let error = writer
            .run::<(), _>(|| panic!("injected job panic"))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("did not complete"));
        assert_eq!(writer.run(|| Ok(42)).await.unwrap(), 42);
    }

    #[test]
    fn bounded_fields_preserve_utf8_and_release_oversized_capacity() {
        for text in ["x".repeat(65536), "a".repeat(511) + "💾💾", "é".repeat(300)] {
            let value = simd_json::OwnedValue::String(text.clone());
            let clipped = bounded_scalar_to_string(&value).unwrap();
            assert!(clipped.len() <= MAX_EMBEDDING_FIELD_LEN + '…'.len_utf8());
            assert!(clipped.capacity() <= MAX_EMBEDDING_FIELD_LEN + '…'.len_utf8());
            assert!(clipped.ends_with('…'));
            assert!(text.starts_with(clipped.trim_end_matches('…')));
        }
        let short = simd_json::OwnedValue::String("é💾".into());
        assert_eq!(bounded_scalar_to_string(&short).as_deref(), Some("é💾"));
    }

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
        for name in names {
            let set = path.join("snapshots").join(name);
            complete_btrfs_snapshot_set(&set).unwrap();
        }
    }

    #[tokio::test]
    async fn failed_native_set_deletion_withdraws_commit_before_touching_members() {
        let fixture = tempdir().unwrap();
        let chain = StreamingSnowball::new(fixture.path().join("snowball"))
            .await
            .unwrap();
        chain
            .write_current_state(&simd_json::json!({"generation": 1}))
            .await
            .unwrap();
        let name = chain.create_snapshot().await.unwrap();
        let set = fixture.path().join("snowball/snapshots").join(&name);
        let error = remove_btrfs_snapshot_set_with(&set, |_| {
            anyhow::bail!("injected member deletion failure")
        })
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("injected member deletion failure")
        );
        assert!(!set.join(".committed").exists());
        for label in SNAPSHOT_LABELS {
            verify_readonly_snapshot(&set.join(label)).unwrap();
        }
        assert!(chain.list_snapshots().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn existing_plain_data_directory_requires_explicit_migration() {
        let fixture = tempdir().unwrap();
        let path = fixture.path().join("snowball");
        std::fs::create_dir_all(path.join("timing")).unwrap();
        std::fs::write(path.join("timing/block-000000000001.json"), b"important").unwrap();
        let error = StreamingSnowball::new(&path).await.err().unwrap();
        assert!(error.to_string().contains("requires a Btrfs subvolume"));
        assert_eq!(
            std::fs::read(path.join("timing/block-000000000001.json")).unwrap(),
            b"important"
        );
    }

    #[tokio::test]
    async fn incomplete_and_legacy_snapshot_sets_are_handled_without_pruning_history() {
        let fixture = tempdir().unwrap();
        let chain = StreamingSnowball::new(fixture.path().join("snowball"))
            .await
            .unwrap();
        let snapshots = chain.base_path().join("snapshots");
        let incomplete = snapshots.join("SNP-000003");
        std::fs::create_dir(&incomplete).unwrap();
        std::fs::create_dir(incomplete.join("timing")).unwrap();
        std::fs::write(incomplete.join(".committed"), b"").unwrap();
        assert!(chain.read_snapshot_state("SNP-000003").await.is_err());
        assert!(
            !chain
                .list_snapshots()
                .await
                .unwrap()
                .iter()
                .any(|(name, _)| name == "SNP-000003")
        );

        let copied = snapshots.join("SNP-000004");
        std::fs::create_dir(&copied).unwrap();
        for label in SNAPSHOT_LABELS {
            std::fs::create_dir(copied.join(label)).unwrap();
        }
        std::fs::write(copied.join("state/current.json"), br#"{"generation":4}"#).unwrap();
        std::fs::write(copied.join(".committed"), b"").unwrap();
        assert_eq!(
            chain
                .read_snapshot_state("SNP-000004")
                .await
                .unwrap()
                .unwrap()["generation"],
            4
        );
        chain
            .set_retention_policy(RetentionPolicy::minimal())
            .await
            .unwrap();
        assert!(copied.exists(), "historical copied set must not be pruned");

        let partial_legacy = snapshots.join("SNP-timing-000042");
        btrfs_snapshot_readonly(chain.timing_subvolume_path(), &partial_legacy).unwrap();
        assert!(
            !chain
                .list_snapshots()
                .await
                .unwrap()
                .iter()
                .any(|(name, _)| name == "SNP-state-000042")
        );
        assert!(chain.read_snapshot_state("SNP-state-000042").await.is_err());
        assert_eq!(chain.create_snapshot().await.unwrap(), "SNP-000043");
        assert!(
            partial_legacy.exists(),
            "incomplete historical triple stays untouched"
        );
    }

    #[tokio::test]
    async fn complete_historical_flat_triple_remains_readable() {
        let fixture = tempdir().unwrap();
        let chain = StreamingSnowball::new(fixture.path().join("snowball"))
            .await
            .unwrap();
        chain
            .write_current_state(&simd_json::json!({"generation": 9}))
            .await
            .unwrap();
        let snapshots = chain.base_path().join("snapshots");
        for label in SNAPSHOT_LABELS {
            let source = chain.data_dir_for_label(label);
            btrfs_snapshot_readonly(source, &snapshots.join(format!("SNP-{label}-000009")))
                .unwrap();
        }
        assert!(
            chain
                .list_snapshots()
                .await
                .unwrap()
                .iter()
                .any(|(name, _)| name == "SNP-state-000009")
        );
        assert_eq!(
            chain
                .read_snapshot_state("SNP-state-000009")
                .await
                .unwrap()
                .unwrap()["generation"],
            9
        );
        assert_eq!(chain.create_snapshot().await.unwrap(), "SNP-000010");
    }

    #[tokio::test]
    async fn active_transfer_pin_prevents_native_snapshot_pruning() {
        let fixture = tempdir().unwrap();
        let chain = StreamingSnowball::new(fixture.path().join("snowball"))
            .await
            .unwrap();
        let first = chain.create_snapshot().await.unwrap();
        let first_path = chain.base_path().join("snapshots").join(&first);
        let _pins = SnapshotTransferPins::create(&first_path, None).unwrap();
        let second = chain.create_snapshot().await.unwrap();
        chain
            .set_retention_policy(RetentionPolicy::minimal())
            .await
            .unwrap();
        assert!(first_path.exists(), "in-flight source was pruned");
        assert!(chain.base_path().join("snapshots").join(second).exists());
    }

    #[tokio::test]
    #[ignore = "native Btrfs send/receive requires root; isolated fixture deletes only its own subvolumes"]
    async fn native_snapshot_triple_send_receive_round_trip() {
        let fixture = tempdir().unwrap();
        let chain = StreamingSnowball::new(fixture.path().join("snowball"))
            .await
            .unwrap();
        chain
            .write_current_state(&simd_json::json!({"generation": 1}))
            .await
            .unwrap();
        chain
            .add_event(
                BlockEvent::new("fixture", "write", simd_json::json!({"id": 1}))
                    .with_vector(vec![0.25]),
            )
            .await
            .unwrap();
        let name = chain.create_snapshot().await.unwrap();
        let set = chain.base_path().join("snapshots").join(name);
        complete_btrfs_snapshot_set(&set).unwrap();
        let received = fixture.path().join("received");
        std::fs::create_dir(&received).unwrap();
        for label in SNAPSHOT_LABELS {
            let mut send = Command::new("btrfs")
                .arg("send")
                .arg(set.join(label))
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let stream = send.stdout.take().unwrap();
            let receive = Command::new("btrfs")
                .arg("receive")
                .arg(&received)
                .stdin(std::process::Stdio::from(stream))
                .output()
                .unwrap();
            assert!(send.wait().unwrap().success(), "Btrfs send failed");
            assert!(
                receive.status.success(),
                "Btrfs receive: {}",
                String::from_utf8_lossy(&receive.stderr)
            );
            verify_readonly_snapshot(&received.join(label)).unwrap();
        }
        assert_eq!(
            std::fs::read(received.join("state/current.json")).unwrap(),
            br#"{"generation":1}"#
        );
        assert!(received.join("timing/block-000000000001.json").exists());
        assert!(received.join("vectors/vec-000000000001.bin").exists());
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
        assert!(
            first
                .embedding_text()
                .starts_with("block: 42\nplugin: zeroclaw")
        );
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
        let fixture = tempdir().unwrap();
        let snowball = StreamingSnowball::new(fixture.path().join("snowball"))
            .await
            .unwrap();

        let declared = vec!["plugin_foo".to_string(), "plugin_bar/*".to_string()];
        let value = simd_json::OwnedValue::Static(simd_json::StaticNode::Bool(true));

        // Exact match
        assert!(
            snowball
                .write_state_validated("plugin_foo", &value, &declared)
                .await
                .is_ok()
        );

        // Prefix match: "plugin_bar_baz" starts with "plugin_bar" (from "plugin_bar/*")
        assert!(
            snowball
                .write_state_validated("plugin_bar_baz", &value, &declared)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn write_state_validated_warns_but_allows_undeclared_in_non_strict() {
        let _environment = StrictStateKeysEnvironment::set(false).await;
        let fixture = tempdir().unwrap();
        let snowball = StreamingSnowball::new(fixture.path().join("snowball"))
            .await
            .unwrap();

        let declared = vec!["plugin_foo".to_string()];
        let value = simd_json::OwnedValue::Static(simd_json::StaticNode::Bool(true));

        // Undeclared key should succeed (warn, not block)
        assert!(
            snowball
                .write_state_validated("undeclared_key", &value, &declared)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn write_state_validated_rejects_undeclared_in_strict_mode() {
        let _environment = StrictStateKeysEnvironment::set(true).await;
        let fixture = tempdir().unwrap();
        let snowball = StreamingSnowball::new(fixture.path().join("snowball"))
            .await
            .unwrap();

        let declared = vec!["plugin_foo".to_string()];
        let value = simd_json::OwnedValue::Static(simd_json::StaticNode::Bool(true));

        // Undeclared key should fail in strict mode
        let result = snowball
            .write_state_validated("undeclared_key", &value, &declared)
            .await;
        assert!(result.is_err());

        // Declared key should still succeed
        assert!(
            snowball
                .write_state_validated("plugin_foo", &value, &declared)
                .await
                .is_ok()
        );
    }
}
