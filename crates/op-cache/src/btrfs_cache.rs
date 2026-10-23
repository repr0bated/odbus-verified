//! BTRFS-backed cache with CozoDB index, compression, and NUMA optimization
//!
//! Provides unlimited disk-based caching with:
//! - BTRFS transparent compression (zstd)
//! - CozoDB in-memory index for O(1) lookups (via `op-cozo-store`)
//! - Linux page cache for hot data
//! - Automatic snapshot management
//! - NUMA-aware memory allocation and CPU affinity

use anyhow::{Context, Result};
use op_cozo_store::CozoGraphShuttle;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tracing::{debug, info, warn};

use super::managed_process;
use super::numa::{NumaMemoryPolicy, NumaNode, NumaOptimizer, NumaStats, NumaTopology};
use super::snapshot_manager::{
    CACHE_MEMBERS, COMPLETE_MARKER, SnapshotConfig, SnapshotManager, TemporarySet, transfer_permit,
};

fn is_btrfs_filesystem(path: &Path) -> Result<bool> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("opening cache filesystem probe {}", path.display()))?;
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
    if unsafe { libc::fstatfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error()).context("probing cache filesystem type");
    }
    Ok(unsafe { stat.assume_init() }.f_type as u64 == libc::BTRFS_SUPER_MAGIC as u64)
}

/// Clear entries inside a category without trying to remove its subvolume root.
/// Refuse unexpected nested subvolumes before touching their contents.
fn clear_directory_contents(path: &Path) -> Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        let metadata = std::fs::symlink_metadata(&child)?;
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            anyhow::ensure!(
                !(is_btrfs_filesystem(&child)? && metadata.ino() == 256),
                "refusing to clear unexpected nested subvolume {}",
                child.display()
            );
            clear_directory_contents(&child)?;
            std::fs::remove_dir(&child)?;
        } else {
            std::fs::remove_file(&child)?;
        }
    }
    Ok(())
}

/// NUMA-aware cache placement strategy
#[derive(Debug, Clone)]
pub enum CachePlacementStrategy {
    /// Place cache data on the same NUMA node as the requesting CPU
    LocalNode,
    /// Distribute cache data across all NUMA nodes for load balancing
    RoundRobin,
    /// Use the NUMA node with most available memory
    MostMemory,
    /// Disable NUMA optimizations (default)
    Disabled,
}

/// Memory allocation policy for NUMA systems
#[derive(Debug, Clone)]
pub enum MemoryPolicy {
    /// Bind memory to specific NUMA node
    Bind(Vec<u32>),
    /// Prefer memory from specific NUMA node
    Preferred(Option<u32>),
    /// Interleave memory across multiple NUMA nodes
    Interleave(Vec<u32>),
    /// Use default system memory policy
    Default,
}

/// Default maximum number of entries in the in-memory fast path.
const DEFAULT_MEMORY_MAX_ENTRIES: usize = 1000;
/// Default maximum memory usage for the in-memory fast path (100 MB).
const DEFAULT_MEMORY_MAX_BYTES: usize = 100 * 1024 * 1024;

fn validate_ssh_destination(destination: &str) -> Result<()> {
    if destination.is_empty()
        || destination.starts_with('-')
        || !destination.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '.' | '-' | '_' | '@' | ':' | '[' | ']' | '%')
        })
    {
        anyhow::bail!("invalid SSH destination");
    }
    Ok(())
}

fn validate_remote_path(path: &str) -> Result<()> {
    if !path.starts_with('/')
        || !path.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '/' | '.' | '-' | '_')
        })
        || Path::new(path)
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        anyhow::bail!("remote Btrfs path must be an absolute path without shell metacharacters");
    }
    Ok(())
}

fn ssh_command(remote_host: &str, script: &str) -> tokio::process::Command {
    let mut command = tokio::process::Command::new("ssh");
    command
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=15",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=3",
            "--",
        ])
        .arg(remote_host)
        .arg(script);
    command
}

async fn checked_ssh(remote_host: &str, command: &str) -> Result<Vec<u8>> {
    let output = managed_process::output(
        &mut ssh_command(remote_host, command),
        managed_process::COMMAND_TIMEOUT,
    )
    .await?;
    anyhow::ensure!(
        output.status.success(),
        "remote Btrfs operation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(output.stdout)
}

fn transfer_timeout() -> Result<Duration> {
    let seconds = std::env::var("OPDBUS_CACHE_TRANSFER_TIMEOUT_SECS")
        .map(|value| {
            value
                .parse::<u64>()
                .context("invalid OPDBUS_CACHE_TRANSFER_TIMEOUT_SECS")
        })
        .unwrap_or(Ok(300))?;
    anyhow::ensure!(
        seconds > 0 && seconds <= 86400,
        "cache transfer timeout must be 1..=86400 seconds"
    );
    Ok(Duration::from_secs(seconds))
}

/// Identity of a backing file. File mtime is the cache's durable last-access
/// timestamp; inode/size/mtime also prevent stale hits across cache handles.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileStamp {
    device: u64,
    inode: u64,
    bytes: u64,
    modified_seconds: i64,
    modified_nanos: i64,
}

impl FileStamp {
    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            bytes: metadata.len(),
            modified_seconds: metadata.mtime(),
            modified_nanos: metadata.mtime_nsec(),
        }
    }
}

struct CacheEntry {
    data: Vec<u8>,
    accessed_at: chrono::DateTime<chrono::Utc>,
    stamp: Option<FileStamp>,
}

/// BTree nodes can contain eleven entries and twelve child pointers. Charging
/// a whole node per entry conservatively covers partially occupied nodes.
/// This is an allocation budget, not an RSS or kernel-page-cache guarantee.
const MEMORY_ENTRY_OVERHEAD: usize =
    11 * std::mem::size_of::<(String, CacheEntry)>() + 16 * std::mem::size_of::<usize>();
const DEFAULT_INDEX_MAX_ENTRIES: usize = 4096;
const DEFAULT_MAX_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;
const DEFAULT_MAX_EMBEDDING_DIMENSIONS: usize = 4 * 1024 * 1024;
const MAINTENANCE_BATCH: usize = 128;

struct MemoryCache {
    entries: Mutex<std::collections::BTreeMap<String, CacheEntry>>,
    max_entries: usize,
    max_memory_bytes: usize,
    current_memory: AtomicUsize,
}

impl MemoryCache {
    fn new(max_entries: usize, max_memory_bytes: usize) -> Self {
        Self {
            entries: Mutex::new(std::collections::BTreeMap::new()),
            max_entries,
            max_memory_bytes,
            current_memory: AtomicUsize::new(0),
        }
    }

    fn charge(key_capacity: usize, data_capacity: usize) -> usize {
        MEMORY_ENTRY_OVERHEAD
            .saturating_add(key_capacity)
            .saturating_add(data_capacity)
    }

    fn admits(&self, key_bytes: usize, data_bytes: usize) -> bool {
        self.max_entries > 0
            && MEMORY_ENTRY_OVERHEAD
                .checked_add(key_bytes)
                .and_then(|bytes| bytes.checked_add(data_bytes))
                .is_some_and(|bytes| bytes <= self.max_memory_bytes)
    }

    #[cfg(test)]
    fn get(&self, key: &str) -> Option<Vec<u8>> {
        self.get_with_stamp(key, None)
    }

    fn get_with_stamp(&self, key: &str, stamp: Option<FileStamp>) -> Option<Vec<u8>> {
        let mut entries = self.entries.lock().ok()?;
        if entries.get(key).is_some_and(|entry| entry.stamp != stamp) {
            if let Some((old_key, entry)) = entries.remove_entry(key) {
                self.current_memory.fetch_sub(
                    Self::charge(old_key.capacity(), entry.data.capacity()),
                    Ordering::Relaxed,
                );
            }
            return None;
        }
        let entry = entries.get_mut(key)?;
        entry.accessed_at = chrono::Utc::now();
        Some(entry.data.clone())
    }

    fn set_stamp(&self, key: &str, stamp: FileStamp) {
        if let Ok(mut entries) = self.entries.lock()
            && let Some(entry) = entries.get_mut(key)
        {
            entry.stamp = Some(stamp);
        }
    }

    #[cfg(test)]
    fn put(&self, key: String, data: Vec<u8>) {
        self.put_with_stamp(key, data, None);
    }

    fn put_slice(&self, key: &str, data: &[u8], stamp: FileStamp) {
        // Disabled/oversized entries do not create a second payload allocation.
        if self.admits(key.len(), data.len()) {
            self.put_with_stamp(key.to_owned(), data.to_vec(), Some(stamp));
        }
    }

    fn put_with_stamp(&self, key: String, data: Vec<u8>, stamp: Option<FileStamp>) {
        let charge = Self::charge(key.capacity(), data.capacity());
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        if let Some((old_key, old)) = entries.remove_entry(&key) {
            self.current_memory.fetch_sub(
                Self::charge(old_key.capacity(), old.data.capacity()),
                Ordering::Relaxed,
            );
        }
        if self.max_entries == 0 || charge > self.max_memory_bytes {
            return;
        }

        // Evict before insertion under this same lock: concurrent readers
        // cannot observe an over-budget map.
        while entries.len() >= self.max_entries
            || self.current_memory.load(Ordering::Relaxed) > self.max_memory_bytes - charge
        {
            let Some(lru) = entries
                .iter()
                .min_by_key(|(_, entry)| entry.accessed_at)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some((old_key, old)) = entries.remove_entry(&lru) {
                self.current_memory.fetch_sub(
                    Self::charge(old_key.capacity(), old.data.capacity()),
                    Ordering::Relaxed,
                );
            }
        }
        entries.insert(
            key,
            CacheEntry {
                data,
                accessed_at: chrono::Utc::now(),
                stamp,
            },
        );
        self.current_memory.fetch_add(charge, Ordering::Relaxed);
    }

    fn remove(&self, key: &str) -> bool {
        let Ok(mut entries) = self.entries.lock() else {
            return false;
        };
        if let Some((old_key, entry)) = entries.remove_entry(key) {
            self.current_memory.fetch_sub(
                Self::charge(old_key.capacity(), entry.data.capacity()),
                Ordering::Relaxed,
            );
            true
        } else {
            false
        }
    }

    fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            // Drop container allocations as well as values.
            *entries = std::collections::BTreeMap::new();
            self.current_memory.store(0, Ordering::Relaxed);
        }
    }
}

/// A cross-handle/process lock outside every cleared category. All synchronous
/// file/index/memory transactions finish before an async method yields.
struct CacheFileLock(std::fs::File);

impl Drop for CacheFileLock {
    fn drop(&mut self) {
        unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
    }
}

struct IndexEntry {
    embedding: bool,
    hash: String,
    access_count: i64,
    created_at: String,
}

pub struct BtrfsCache {
    cache_dir: PathBuf,
    /// Compact derived metadata only. Rows are bounded independently of the
    /// payload fast path; deterministic disk lookup survives eviction/reopen.
    index: CozoGraphShuttle,
    index_max_entries: usize,
    index_lru: Mutex<std::collections::VecDeque<IndexEntry>>,
    operation: Mutex<()>,
    total_accesses: std::sync::atomic::AtomicU64,
    max_payload_bytes: usize,
    max_embedding_dimensions: usize,
    snapshot_manager: SnapshotManager,
    numa_topology: NumaTopology,
    placement_strategy: CachePlacementStrategy,
    memory_policy: MemoryPolicy,
    cpu_affinity: Vec<u32>,
    current_node_index: AtomicUsize,
    #[allow(dead_code)]
    numa_stats: Mutex<NumaStats>,
    memory_cache: MemoryCache,
}

#[allow(dead_code)]
impl BtrfsCache {
    /// Create a Btrfs cache subvolume on Btrfs. Ordinary filesystems use a
    /// regular directory; Btrfs failures are never interpreted as fallback.
    async fn create_btrfs_subvolume(path: &Path) -> Result<()> {
        let existing = match tokio::fs::symlink_metadata(path).await {
            Ok(metadata) => {
                anyhow::ensure!(
                    metadata.is_dir() && !metadata.file_type().is_symlink(),
                    "cache path is not a directory: {}",
                    path.display()
                );
                true
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        let probe = if existing {
            path
        } else {
            path.parent()
                .context("Cache subvolume path has no parent")?
        };
        if !is_btrfs_filesystem(probe)? {
            tokio::fs::create_dir_all(path)
                .await
                .context("Failed to create ordinary cache directory")?;
            return Ok(());
        }
        if existing {
            let output = managed_process::output(
                tokio::process::Command::new("btrfs")
                    .args(["property", "get", "-ts"])
                    .arg(path)
                    .arg("ro"),
                managed_process::COMMAND_TIMEOUT,
            )
            .await
            .context("Failed to execute btrfs command")?;
            if output.status.success()
                && String::from_utf8_lossy(&output.stdout).trim() == "ro=false"
            {
                Self::enable_compression(path).await?;
                return Ok(());
            }

            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "existing cache path is not a usable Btrfs subvolume ({}): {}",
                path.display(),
                stderr.trim()
            );
        }

        let output = managed_process::output(
            tokio::process::Command::new("btrfs")
                .args(["subvolume", "create"])
                .arg(path),
            managed_process::COMMAND_TIMEOUT,
        )
        .await
        .context("Failed to execute btrfs command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("btrfs subvolume create failed: {}", stderr);
        } else {
            Self::enable_compression(path).await?;
        }

        Ok(())
    }

    async fn enable_compression(path: &Path) -> Result<()> {
        let output = managed_process::output(
            tokio::process::Command::new("btrfs")
                .args(["property", "set"])
                .arg(path)
                .args(["compression", "zstd"]),
            managed_process::COMMAND_TIMEOUT,
        )
        .await
        .context("Failed to execute btrfs property command")?;
        if !output.status.success() {
            anyhow::bail!(
                "failed to enable Btrfs zstd compression for {}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    /// Open or create the cache root and its five category subvolumes.
    pub async fn new(cache_dir: PathBuf) -> Result<Self> {
        let snapshot_config = SnapshotConfig {
            snapshot_dir: cache_dir
                .parent()
                .unwrap_or(Path::new("/var/lib/op-dbus"))
                .join("@cache-snapshots"),
            max_snapshots: std::env::var("OPDBUS_MAX_CACHE_SNAPSHOTS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(24),
            prefix: std::env::var("OPDBUS_CACHE_SNAPSHOT_PREFIX")
                .unwrap_or_else(|_| "SNP-cache".to_string()),
        };
        anyhow::ensure!(
            snapshot_config.max_snapshots > 0,
            "Cache rotation requires positive retention"
        );
        let snapshot_manager = SnapshotManager::new(cache_dir.clone(), snapshot_config);

        // Ensure parent directory exists (not as subvolume)
        if let Some(parent) = cache_dir.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Each category remains a Btrfs subvolume on Btrfs. A snapshot of the
        // root alone omits their contents; SnapshotManager snapshots the set.
        Self::create_btrfs_subvolume(&cache_dir).await?;
        for category in ["embeddings", "blocks", "queries", "diffs", "kv"] {
            Self::create_btrfs_subvolume(&cache_dir.join(category)).await?;
        }

        // Create regular directories within subvolumes
        tokio::fs::create_dir_all(cache_dir.join("embeddings/vectors")).await?;
        tokio::fs::create_dir_all(cache_dir.join("blocks/by-number")).await?;
        tokio::fs::create_dir_all(cache_dir.join("blocks/by-hash")).await?;

        // Create CozoDB in-memory index for embeddings (replaces SQLite)
        let index = CozoGraphShuttle::new_in_memory()
            .map_err(|e| anyhow::anyhow!("Failed to create CozoDB index: {e}"))?;

        // Each cache owns a fresh, bounded derived index. Full input texts and
        // keys are never retained; schema positions stay compatible.
        index
            .run_query(
                r#":create embedding_cache {
                text_hash: String
                =>
                text: String default "",
                vector_file: String default "",
                created_at: String default "",
                accessed_at: String default "",
                access_count: Int default 1,
                vector_size: Int default 0
            }"#,
                None,
            )
            .map_err(|error| anyhow::anyhow!("Failed to create embedding cache index: {error}"))?;

        // Create the kv_cache relation for the generic key-value API.
        index
            .run_query(
                r#":create kv_cache {
                key_hash: String
                =>
                key: String default "",
                file_path: String default "",
                created_at: String default "",
                accessed_at: String default "",
                access_count: Int default 1,
                data_size: Int default 0
            }"#,
                None,
            )
            .map_err(|error| anyhow::anyhow!("Failed to create KV cache index: {error}"))?;

        // Detect NUMA topology
        let numa_topology = NumaTopology::detect()?;
        let placement_strategy = Self::determine_placement_strategy(numa_topology.nodes());
        let memory_policy = Self::determine_memory_policy();

        // Record the initial permitted placement; execution applies it to
        // transport children, never to this shared async worker.
        let allowed_cpus = NumaOptimizer::current_cpu_affinity();
        let cpu_affinity = Self::select_placement_node(
            &numa_topology,
            &placement_strategy,
            0,
            allowed_cpus.as_deref(),
            numa_topology.current_node(),
        )
        .map(|node| node.cpu_list)
        .unwrap_or_default();

        // Initialize the in-memory fast path with configurable limits.
        let max_entries = Self::configured_limit(
            "OPDBUS_CACHE_MEMORY_MAX_ENTRIES",
            DEFAULT_MEMORY_MAX_ENTRIES,
        )?;
        let max_bytes =
            Self::configured_limit("OPDBUS_CACHE_MEMORY_MAX_BYTES", DEFAULT_MEMORY_MAX_BYTES)?;
        let index_max_entries =
            Self::configured_limit("OPDBUS_CACHE_INDEX_MAX_ENTRIES", DEFAULT_INDEX_MAX_ENTRIES)?;
        let max_payload_bytes =
            Self::configured_limit("OPDBUS_CACHE_MAX_PAYLOAD_BYTES", DEFAULT_MAX_PAYLOAD_BYTES)?;
        let max_embedding_dimensions = Self::configured_limit(
            "OPDBUS_CACHE_MAX_EMBEDDING_DIMENSIONS",
            DEFAULT_MAX_EMBEDDING_DIMENSIONS,
        )?;
        let memory_cache = MemoryCache::new(max_entries, max_bytes);

        Ok(Self {
            cache_dir,
            index,
            index_max_entries,
            index_lru: Mutex::new(std::collections::VecDeque::new()),
            operation: Mutex::new(()),
            total_accesses: std::sync::atomic::AtomicU64::new(0),
            max_payload_bytes,
            max_embedding_dimensions,
            snapshot_manager,
            numa_topology,
            placement_strategy,
            memory_policy,
            cpu_affinity,
            current_node_index: AtomicUsize::new(0),
            numa_stats: Mutex::new(NumaStats::new()),
            memory_cache,
        })
    }

    fn determine_placement_strategy(numa_nodes: &HashMap<u32, NumaNode>) -> CachePlacementStrategy {
        let default_choice = if numa_nodes.is_empty() {
            "disabled".to_string()
        } else {
            "local".to_string()
        };

        let placement = std::env::var("OPDBUS_CACHE_PLACEMENT")
            .unwrap_or(default_choice)
            .to_lowercase();

        match placement.as_str() {
            "round-robin" | "round_robin" | "roundrobin" => CachePlacementStrategy::RoundRobin,
            "most-memory" | "most_memory" | "mostmemory" => CachePlacementStrategy::MostMemory,
            "disabled" => CachePlacementStrategy::Disabled,
            "local" | "local-node" | "local_node" => {
                if numa_nodes.is_empty() {
                    CachePlacementStrategy::Disabled
                } else {
                    CachePlacementStrategy::LocalNode
                }
            }
            other => {
                warn!(
                    "Unknown OPDBUS_CACHE_PLACEMENT value '{}', defaulting to {}",
                    other,
                    if numa_nodes.is_empty() {
                        "disabled"
                    } else {
                        "local"
                    }
                );
                if numa_nodes.is_empty() {
                    CachePlacementStrategy::Disabled
                } else {
                    CachePlacementStrategy::LocalNode
                }
            }
        }
    }

    fn determine_memory_policy() -> MemoryPolicy {
        match std::env::var("OPDBUS_CACHE_MEMORY_POLICY") {
            Ok(value) => {
                let value_lower = value.to_lowercase();
                if let Some(rest) = value_lower.strip_prefix("bind:") {
                    let nodes = Self::parse_node_list(rest);
                    if nodes.is_empty() {
                        warn!("OPDBUS_CACHE_MEMORY_POLICY=bind but no NUMA nodes listed");
                        MemoryPolicy::Default
                    } else {
                        MemoryPolicy::Bind(nodes)
                    }
                } else if let Some(rest) = value_lower.strip_prefix("preferred:") {
                    if rest.trim().is_empty() {
                        MemoryPolicy::Preferred(None)
                    } else {
                        match rest.trim().parse::<u32>() {
                            Ok(node) => MemoryPolicy::Preferred(Some(node)),
                            Err(e) => {
                                warn!(
                                    "Failed to parse preferred NUMA node '{}': {}",
                                    rest.trim(),
                                    e
                                );
                                MemoryPolicy::Default
                            }
                        }
                    }
                } else if value_lower == "preferred" {
                    MemoryPolicy::Preferred(None)
                } else if let Some(rest) = value_lower.strip_prefix("interleave:") {
                    let nodes = Self::parse_node_list(rest);
                    if nodes.is_empty() {
                        warn!("OPDBUS_CACHE_MEMORY_POLICY=interleave but no NUMA nodes listed");
                        MemoryPolicy::Default
                    } else {
                        MemoryPolicy::Interleave(nodes)
                    }
                } else if value_lower == "default" || value_lower.is_empty() {
                    MemoryPolicy::Default
                } else {
                    warn!(
                        "Unknown OPDBUS_CACHE_MEMORY_POLICY value '{}', using default",
                        value
                    );
                    MemoryPolicy::Default
                }
            }
            Err(_) => MemoryPolicy::Default,
        }
    }

    fn parse_node_list(list: &str) -> Vec<u32> {
        list.split(',')
            .filter_map(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    match trimmed.parse::<u32>() {
                        Ok(value) => Some(value),
                        Err(e) => {
                            warn!("Invalid NUMA node id '{}': {}", trimmed, e);
                            None
                        }
                    }
                }
            })
            .collect()
    }

    fn select_numa_node(&self, operation: &str) -> Option<NumaNode> {
        if matches!(self.placement_strategy, CachePlacementStrategy::Disabled) {
            return None;
        }
        let mut topology = self.numa_topology.clone();
        if let Err(error) = topology.refresh() {
            warn!(
                "NUMA topology refresh failed for {operation}; retaining last snapshot: {error:#}"
            );
        }
        let allowed_cpus = NumaOptimizer::current_cpu_affinity();
        let index = if matches!(self.placement_strategy, CachePlacementStrategy::RoundRobin) {
            self.current_node_index.fetch_add(1, Ordering::Relaxed)
        } else {
            0
        };
        let selection = Self::select_placement_node(
            &topology,
            &self.placement_strategy,
            index,
            allowed_cpus.as_deref(),
            topology.current_node(),
        );
        if let Some(node) = &selection {
            debug!(
                "NUMA node {} selected for {} (memory={} MB, distances={:?})",
                node.node_id,
                operation,
                node.memory_free_kb / 1024,
                node.distance_to_nodes
            );
        } else {
            debug!(
                "No NUMA node selected for {} (strategy={:?})",
                operation, self.placement_strategy
            );
        }

        selection
    }

    fn select_placement_node(
        topology: &NumaTopology,
        strategy: &CachePlacementStrategy,
        index: usize,
        allowed_cpus: Option<&[u32]>,
        current_node: Option<u32>,
    ) -> Option<NumaNode> {
        if matches!(strategy, CachePlacementStrategy::Disabled) {
            return None;
        }
        let nodes: Vec<_> = topology
            .node_ids()
            .into_iter()
            .filter_map(|id| topology.get_node(id))
            .filter(|node| {
                allowed_cpus
                    .is_none_or(|allowed| node.cpu_list.iter().any(|cpu| allowed.contains(cpu)))
            })
            .collect();
        if nodes.is_empty() {
            return None;
        }
        let selected = match strategy {
            CachePlacementStrategy::LocalNode => nodes
                .iter()
                .copied()
                .find(|node| Some(node.node_id) == current_node)
                .unwrap_or(nodes[0]),
            CachePlacementStrategy::RoundRobin => nodes[index % nodes.len()],
            CachePlacementStrategy::MostMemory => nodes
                .iter()
                .copied()
                .min_by_key(|node| (std::cmp::Reverse(node.memory_free_kb), node.node_id))?,
            CachePlacementStrategy::Disabled => return None,
        };
        let mut selected = selected.clone();
        if let Some(allowed) = allowed_cpus {
            selected.cpu_list.retain(|cpu| allowed.contains(cpu));
        }
        Some(selected)
    }

    fn configured_limit(name: &str, default: usize) -> Result<usize> {
        match std::env::var(name) {
            Ok(value) => value
                .parse::<usize>()
                .with_context(|| format!("{name} must be a nonnegative integer")),
            Err(std::env::VarError::NotPresent) => Ok(default),
            Err(error) => Err(error).with_context(|| format!("reading {name}")),
        }
    }

    fn lock_operation(&self) -> Result<(std::sync::MutexGuard<'_, ()>, CacheFileLock)> {
        use std::os::unix::fs::OpenOptionsExt;
        let guard = self
            .operation
            .lock()
            .map_err(|_| anyhow::anyhow!("cache operation lock poisoned"))?;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(self.cache_dir.join(".cache.lock"))?;
        anyhow::ensure!(
            file.metadata()?.is_file(),
            "cache lock is not a regular file"
        );
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error).context("locking cache files");
            }
        }
        Ok((guard, CacheFileLock(file)))
    }

    fn hash_text(&self, text: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn hash_key(&self, key: &str) -> String {
        self.hash_text(key)
    }

    fn embedding_path(&self, hash: &str) -> PathBuf {
        self.cache_dir
            .join("embeddings/vectors")
            .join(format!("{hash}.vec"))
    }

    fn kv_path(&self, hash: &str) -> PathBuf {
        self.cache_dir.join("kv").join(format!("{hash}.dat"))
    }

    fn file_metadata(path: &Path) -> Result<Option<std::fs::Metadata>> {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) => {
                anyhow::ensure!(
                    metadata.is_file() && !metadata.file_type().is_symlink(),
                    "cache payload is not a regular file: {}",
                    path.display()
                );
                Ok(Some(metadata))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
        }
    }

    fn open_payload(path: &Path) -> Result<std::fs::File> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)?;
        anyhow::ensure!(
            file.metadata()?.is_file(),
            "cache payload is not a regular file"
        );
        Ok(file)
    }

    fn validate_payload_size(&self, bytes: usize) -> Result<()> {
        anyhow::ensure!(
            bytes <= self.max_payload_bytes,
            "cache payload exceeds OPDBUS_CACHE_MAX_PAYLOAD_BYTES ({})",
            self.max_payload_bytes
        );
        Ok(())
    }

    fn read_payload(&self, path: &Path) -> Result<Vec<u8>> {
        use std::io::Read;
        let mut file = Self::open_payload(path)?;
        let length =
            usize::try_from(file.metadata()?.len()).context("cache payload size overflow")?;
        self.validate_payload_size(length)?;
        // Allocate from a checked file length, not an untrusted encoded length.
        // Reject growth instead of allocating again if another writer ignores
        // the cache's file lock.
        let mut bytes = vec![0; length];
        file.read_exact(&mut bytes)
            .with_context(|| format!("reading cache payload {}", path.display()))?;
        let mut extra = [0];
        anyhow::ensure!(
            file.read(&mut extra)? == 0,
            "cache payload changed size while reading"
        );
        Ok(bytes)
    }

    fn touch_payload(path: &Path) -> Result<FileStamp> {
        let file = Self::open_payload(path)?;
        file.set_times(std::fs::FileTimes::new().set_modified(std::time::SystemTime::now()))?;
        Ok(FileStamp::from_metadata(&file.metadata()?))
    }

    fn write_payload(path: &Path, bytes: &[u8]) -> Result<FileStamp> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let parent = path.parent().context("cache payload has no parent")?;
        std::fs::create_dir_all(parent)?;
        let temporary = parent.join(format!(".cache-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&temporary)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            let stamp = FileStamp::from_metadata(&file.metadata()?);
            std::fs::rename(&temporary, path)?;
            std::fs::File::open(parent)?.sync_all()?;
            Ok(stamp)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }

    fn decode_embedding(&self, bytes: &[u8]) -> Result<Vec<f32>> {
        use bincode::Options;
        let header: [u8; 8] = bytes
            .get(..8)
            .context("truncated embedding length")?
            .try_into()
            .expect("eight-byte slice");
        let dimensions =
            usize::try_from(u64::from_le_bytes(header)).context("embedding dimension overflow")?;
        anyhow::ensure!(
            dimensions <= self.max_embedding_dimensions,
            "embedding exceeds OPDBUS_CACHE_MAX_EMBEDDING_DIMENSIONS ({})",
            self.max_embedding_dimensions
        );
        let expected = dimensions
            .checked_mul(std::mem::size_of::<f32>())
            .and_then(|size| size.checked_add(8))
            .context("embedding length overflow")?;
        anyhow::ensure!(
            bytes.len() == expected,
            "embedding payload length does not match dimensions"
        );
        self.validate_payload_size(bytes.len())?;
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .reject_trailing_bytes()
            .with_limit(self.max_payload_bytes as u64)
            .deserialize(bytes)
            .context("Failed to deserialize cached embedding")
    }

    fn validate_embedding(&self, vector: &[f32]) -> Result<usize> {
        anyhow::ensure!(
            vector.len() <= self.max_embedding_dimensions,
            "embedding exceeds OPDBUS_CACHE_MAX_EMBEDDING_DIMENSIONS ({})",
            self.max_embedding_dimensions
        );
        let bytes = vector
            .len()
            .checked_mul(std::mem::size_of::<f32>())
            .and_then(|bytes| bytes.checked_add(8))
            .context("embedding size overflow")?;
        self.validate_payload_size(bytes)?;
        Ok(bytes)
    }

    fn forget_index(&self, embedding: bool, hash: &str) -> Result<()> {
        let query = if embedding {
            "?[text_hash] <- [[$hash]] :rm embedding_cache { text_hash }"
        } else {
            "?[key_hash] <- [[$hash]] :rm kv_cache { key_hash }"
        };
        self.index
            .run_query(query, Some(serde_json::json!({"hash": hash})))
            .map_err(|error| anyhow::anyhow!("cache index removal failed: {error}"))?;
        self.index_lru
            .lock()
            .map_err(|_| anyhow::anyhow!("cache index LRU poisoned"))?
            .retain(|entry| entry.embedding != embedding || entry.hash != hash);
        Ok(())
    }

    fn record_index(&self, embedding: bool, hash: &str, size: usize) -> Result<()> {
        if embedding {
            self.total_accesses.fetch_add(1, Ordering::Relaxed);
        }
        if self.index_max_entries == 0 {
            return Ok(());
        }
        let mut order = self
            .index_lru
            .lock()
            .map_err(|_| anyhow::anyhow!("cache index LRU poisoned"))?;
        let existing_position = order
            .iter()
            .position(|entry| entry.embedding == embedding && entry.hash == hash);
        if existing_position.is_none() {
            while order.len() >= self.index_max_entries {
                let old = order.front().context("empty cache index eviction")?;
                let query = if old.embedding {
                    "?[text_hash] <- [[$hash]] :rm embedding_cache { text_hash }"
                } else {
                    "?[key_hash] <- [[$hash]] :rm kv_cache { key_hash }"
                };
                self.index
                    .run_query(query, Some(serde_json::json!({"hash": old.hash})))
                    .map_err(|error| anyhow::anyhow!("cache index eviction failed: {error}"))?;
                order.pop_front();
            }
        }

        let now = chrono::Utc::now().to_rfc3339();
        // The bounded LRU already owns these fixed-size fields. Avoid parsing
        // another Cozo query on every warm read just to increment a counter.
        let existing = existing_position.and_then(|position| order.get(position));
        let count = existing.map_or(1, |entry| entry.access_count.saturating_add(1));
        let created = existing.map_or_else(|| now.clone(), |entry| entry.created_at.clone());
        let file = if embedding {
            format!("{hash}.vec")
        } else {
            format!("kv/{hash}.dat")
        };
        let query = if embedding {
            r#"?[text_hash, text, vector_file, created_at, accessed_at, access_count, vector_size]
                <- [[$hash, "", $file, $created, $now, $count, $size]]
                :put embedding_cache { text_hash => text, vector_file, created_at, accessed_at, access_count, vector_size }"#
        } else {
            r#"?[key_hash, key, file_path, created_at, accessed_at, access_count, data_size]
                <- [[$hash, "", $file, $created, $now, $count, $size]]
                :put kv_cache { key_hash => key, file_path, created_at, accessed_at, access_count, data_size }"#
        };
        self.index.run_query(query, Some(serde_json::json!({
            "hash": hash, "file": file, "created": created, "now": now, "count": count, "size": size,
        }))).map_err(|error| anyhow::anyhow!("cache index write failed: {error}"))?;
        if let Some(position) = existing_position {
            order.remove(position);
        }
        order.push_back(IndexEntry {
            embedding,
            hash: hash.to_owned(),
            access_count: count,
            created_at: created,
        });
        Ok(())
    }

    fn get_embedding_locked(&self, hash: &str) -> Result<Option<Vec<f32>>> {
        let path = self.embedding_path(hash);
        let key = format!("emb:{hash}");
        let Some(metadata) = Self::file_metadata(&path)? else {
            self.memory_cache.remove(&key);
            self.forget_index(true, hash)?;
            return Ok(None);
        };
        self.validate_payload_size(
            usize::try_from(metadata.len()).context("cache payload size overflow")?,
        )?;
        let stamp = FileStamp::from_metadata(&metadata);
        let cached = self.memory_cache.get_with_stamp(&key, Some(stamp));
        let warm = cached.is_some();
        let bytes = match cached {
            Some(bytes) => bytes,
            None => self.read_payload(&path)?,
        };
        let vector = self.decode_embedding(&bytes)?;
        let stamp = Self::touch_payload(&path)?;
        self.record_index(true, hash, vector.len())?;
        // Avoid a second copy on a warm hit.
        if warm {
            self.memory_cache.set_stamp(&key, stamp);
        } else {
            self.memory_cache.put_slice(&key, &bytes, stamp);
        }
        Ok(Some(vector))
    }

    fn save_embedding_locked(&self, hash: &str, vector: &[f32]) -> Result<()> {
        self.validate_embedding(vector)?;
        let bytes = bincode::serialize(vector)?;
        let key = format!("emb:{hash}");
        self.memory_cache.remove(&key);
        let stamp = Self::write_payload(&self.embedding_path(hash), &bytes)?;
        self.record_index(true, hash, vector.len())?;
        if self.memory_cache.admits(key.len(), bytes.len()) {
            self.memory_cache.put_with_stamp(key, bytes, Some(stamp));
        }
        Ok(())
    }

    /// Compute outside the storage lock, then recheck before committing so a
    /// concurrent put/compute wins without being overwritten by stale work.
    pub fn get_or_embed<F>(&self, text: &str, compute_fn: F) -> Result<Vec<f32>>
    where
        F: FnOnce(&str) -> Result<Vec<f32>>,
    {
        let hash = self.hash_text(text);
        {
            let _guard = self.lock_operation()?;
            if let Some(vector) = self.get_embedding_locked(&hash)? {
                return Ok(vector);
            }
        }
        let vector = compute_fn(text)?;
        self.validate_embedding(&vector)?;
        let _guard = self.lock_operation()?;
        if let Some(current) = self.get_embedding_locked(&hash)? {
            return Ok(current);
        }
        self.save_embedding_locked(&hash, &vector)?;
        Ok(vector)
    }

    pub fn get_embedding(&self, text: &str) -> Result<Option<Vec<f32>>> {
        let _guard = self.lock_operation()?;
        self.get_embedding_locked(&self.hash_text(text))
    }

    pub fn put_embedding(&self, text: &str, vector: &[f32]) -> Result<()> {
        self.validate_embedding(vector)?;
        let _guard = self.lock_operation()?;
        self.save_embedding_locked(&self.hash_text(text), vector)
    }

    fn get_raw_locked(&self, hash: &str) -> Result<Option<Vec<u8>>> {
        let path = self.kv_path(hash);
        let key = format!("kv:{hash}");
        let Some(metadata) = Self::file_metadata(&path)? else {
            self.memory_cache.remove(&key);
            self.forget_index(false, hash)?;
            return Ok(None);
        };
        self.validate_payload_size(
            usize::try_from(metadata.len()).context("cache payload size overflow")?,
        )?;
        let stamp = FileStamp::from_metadata(&metadata);
        if let Some(data) = self.memory_cache.get_with_stamp(&key, Some(stamp)) {
            let stamp = Self::touch_payload(&path)?;
            self.record_index(false, hash, data.len())?;
            self.memory_cache.set_stamp(&key, stamp);
            return Ok(Some(data));
        }
        let data = self.read_payload(&path)?;
        let stamp = Self::touch_payload(&path)?;
        self.record_index(false, hash, data.len())?;
        self.memory_cache.put_slice(&key, &data, stamp);
        Ok(Some(data))
    }

    /// Async API compatibility with bounded synchronous file transactions:
    /// no owned payload is queued in an unbounded blocking-task backlog.
    pub async fn get_raw(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let _guard = self.lock_operation()?;
        self.get_raw_locked(&self.hash_key(key))
    }

    pub async fn put_raw(&self, key: &str, data: &[u8]) -> Result<()> {
        self.validate_payload_size(data.len())?;
        let hash = self.hash_key(key);
        let _guard = self.lock_operation()?;
        let memory_key = format!("kv:{hash}");
        self.memory_cache.remove(&memory_key);
        let stamp = Self::write_payload(&self.kv_path(&hash), data)?;
        self.record_index(false, &hash, data.len())?;
        self.memory_cache.put_slice(&memory_key, data, stamp);
        Ok(())
    }

    pub async fn delete(&self, key: &str) -> Result<bool> {
        let hash = self.hash_key(key);
        let _guard = self.lock_operation()?;
        let path = self.kv_path(&hash);
        let indexed = self
            .index_lru
            .lock()
            .map_err(|_| anyhow::anyhow!("cache index LRU poisoned"))?
            .iter()
            .any(|entry| !entry.embedding && entry.hash == hash);
        let in_memory = self
            .memory_cache
            .entries
            .lock()
            .map_err(|_| anyhow::anyhow!("cache memory poisoned"))?
            .contains_key(&format!("kv:{hash}"));
        let existed = match std::fs::remove_file(&path) {
            Ok(()) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("Failed to delete cache file {}", path.display()));
            }
        };
        self.memory_cache.remove(&format!("kv:{hash}"));
        self.forget_index(false, &hash)?;
        Ok(existed || indexed || in_memory)
    }

    pub async fn get_or_compute<F, Fut>(&self, key: &str, compute: F) -> Result<Vec<u8>>
    where
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = Result<Vec<u8>>> + Send,
    {
        if let Some(data) = self.get_raw(key).await? {
            return Ok(data);
        }
        let data = compute().await?;
        self.validate_payload_size(data.len())?;
        let hash = self.hash_key(key);
        let _guard = self.lock_operation()?;
        if let Some(current) = self.get_raw_locked(&hash)? {
            return Ok(current);
        }
        let memory_key = format!("kv:{hash}");
        let stamp = Self::write_payload(&self.kv_path(&hash), &data)?;
        self.record_index(false, &hash, data.len())?;
        self.memory_cache.put_slice(&memory_key, &data, stamp);
        Ok(data)
    }

    /// Statistics scan files with bounded working memory, independent of
    /// derived-index eviction. Access count covers this instance's embedding
    /// operations; disk last-access timestamps survive reopen.
    pub fn stats(&self) -> Result<CacheStats> {
        let _guard = self.lock_operation()?;
        let threshold = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        let mut total_entries = 0;
        let mut hot_entries = 0;
        for entry in std::fs::read_dir(self.cache_dir.join("embeddings/vectors"))? {
            let entry = entry?;
            if Self::vector_hash(&entry.file_name()).is_none() {
                continue;
            }
            let metadata = Self::file_metadata(&entry.path())?
                .context("embedding disappeared during stats")?;
            total_entries += 1;
            if metadata.modified()? > threshold {
                hot_entries += 1;
            }
        }
        let embeddings_size = self.dir_size(&self.cache_dir.join("embeddings"))?;
        let blocks_size = self.dir_size(&self.cache_dir.join("blocks"))?;
        let mut total_size = embeddings_size
            .checked_add(blocks_size)
            .context("cache disk size overflow")?;
        for category in ["queries", "diffs", "kv"] {
            total_size = total_size
                .checked_add(self.dir_size(&self.cache_dir.join(category))?)
                .context("cache disk size overflow")?;
        }
        Ok(CacheStats {
            total_entries,
            hot_entries,
            total_accesses: self.total_accesses.load(Ordering::Relaxed),
            disk_usage_bytes: total_size,
            embeddings_size_bytes: embeddings_size,
            blocks_size_bytes: blocks_size,
        })
    }

    #[allow(clippy::only_used_in_recursion)]
    fn dir_size(&self, path: &Path) -> Result<u64> {
        let mut size = 0u64;
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let metadata = std::fs::symlink_metadata(entry.path())?;
            let bytes = if metadata.is_file() {
                metadata.len()
            } else if metadata.is_dir() {
                self.dir_size(&entry.path())?
            } else {
                0
            };
            size = size
                .checked_add(bytes)
                .context("cache disk size overflow")?;
        }
        Ok(size)
    }

    fn vector_hash(name: &std::ffi::OsStr) -> Option<String> {
        let hash = name.to_str()?.strip_suffix(".vec")?;
        (hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
        .then(|| hash.to_owned())
    }

    /// Expire actual vector files, including entries evicted from metadata or
    /// left by a previous process. File mtime is durable last access. Only a
    /// fixed batch is retained; each completed deletion invalidates all layers.
    pub fn cleanup_old(&self, days: i64) -> Result<usize> {
        let age = chrono::Duration::try_days(days).context("cache expiry age overflow")?;
        let cutoff = chrono::Utc::now()
            .checked_sub_signed(age)
            .context("cache expiry cutoff overflow")?;
        let _guard = self.lock_operation()?;
        let mut removed = 0;
        let mut batch = Vec::with_capacity(MAINTENANCE_BATCH);
        for entry in std::fs::read_dir(self.cache_dir.join("embeddings/vectors"))? {
            let entry = entry?;
            let Some(hash) = Self::vector_hash(&entry.file_name()) else {
                continue;
            };
            let metadata = Self::file_metadata(&entry.path())?
                .context("embedding disappeared during expiry")?;
            let accessed: chrono::DateTime<chrono::Utc> = metadata.modified()?.into();
            if accessed < cutoff {
                batch.push((hash, entry.path()));
            }
            if batch.len() == MAINTENANCE_BATCH {
                removed += self.expire_batch(&mut batch)?;
            }
        }
        removed += self.expire_batch(&mut batch)?;
        Ok(removed)
    }

    fn expire_batch(&self, batch: &mut Vec<(String, PathBuf)>) -> Result<usize> {
        let mut removed = 0;
        for (hash, path) in batch.drain(..) {
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("Failed to expire cached embedding {}", path.display())
                    });
                }
            }
            self.memory_cache.remove(&format!("emb:{hash}"));
            self.forget_index(true, &hash)?;
            removed += 1;
        }
        Ok(removed)
    }

    fn clear_index(&self, embedding_only: bool) -> Result<()> {
        self.index.run_query(
            "?[text_hash] := *embedding_cache[text_hash, _, _, _, _, _, _] :rm embedding_cache { text_hash }",
            None,
        ).map_err(|error| anyhow::anyhow!("Failed to clear embedding index: {error}"))?;
        if !embedding_only {
            self.index.run_query(
                "?[key_hash] := *kv_cache[key_hash, _, _, _, _, _, _] :rm kv_cache { key_hash }",
                None,
            ).map_err(|error| anyhow::anyhow!("Failed to clear KV index: {error}"))?;
        }
        let mut order = self
            .index_lru
            .lock()
            .map_err(|_| anyhow::anyhow!("cache index LRU poisoned"))?;
        if embedding_only {
            order.retain(|entry| !entry.embedding);
            if order.is_empty() {
                *order = std::collections::VecDeque::new();
            }
        } else {
            *order = std::collections::VecDeque::new();
        }
        self.total_accesses.store(0, Ordering::Relaxed);
        Ok(())
    }

    /// Clear current payloads/derived metadata; preserve category subvolumes,
    /// snapshots and the shared file lock. Errors are reported.
    pub fn clear(&self) -> Result<()> {
        let _guard = self.lock_operation()?;
        self.memory_cache.clear();
        self.clear_index(false)?;
        for category in ["embeddings/vectors", "blocks", "queries", "diffs", "kv"] {
            clear_directory_contents(&self.cache_dir.join(category))?;
        }
        std::fs::create_dir_all(self.cache_dir.join("blocks/by-number"))?;
        std::fs::create_dir_all(self.cache_dir.join("blocks/by-hash"))?;
        Ok(())
    }

    pub fn clear_embeddings(&self) -> Result<()> {
        let _guard = self.lock_operation()?;
        self.memory_cache.clear();
        self.clear_index(true)?;
        clear_directory_contents(&self.cache_dir.join("embeddings/vectors"))
    }

    pub fn clear_blocks(&self) -> Result<()> {
        let _guard = self.lock_operation()?;
        clear_directory_contents(&self.cache_dir.join("blocks"))?;
        std::fs::create_dir_all(self.cache_dir.join("blocks/by-number"))?;
        std::fs::create_dir_all(self.cache_dir.join("blocks/by-hash"))?;
        Ok(())
    }

    /// Create BTRFS snapshot of cache
    pub async fn create_snapshot(&self) -> Result<PathBuf> {
        self.snapshot_manager.create_snapshot().await
    }

    /// List all snapshots
    pub async fn list_snapshots(&self) -> Result<Vec<super::snapshot_manager::SnapshotInfo>> {
        self.snapshot_manager.list_snapshots().await
    }

    /// Delete all snapshots
    pub async fn delete_all_snapshots(&self) -> Result<usize> {
        self.snapshot_manager.delete_all_snapshots().await
    }

    /// Stream all six members of a committed cache set with Btrfs send/receive.
    /// This guarantees member coverage, not an atomic multi-member point in
    /// time. The Snowball timing trail is authoritative; cache is rebuildable.
    pub async fn stream_to_remote(
        &self,
        remote_host: &str,
        remote_path: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        validate_ssh_destination(remote_host)?;
        validate_remote_path(remote_path)?;

        let deadline = transfer_timeout()?;
        let temporary = self.snapshot_manager.create_transport_snapshot().await?;
        let set_path = temporary.path();
        let members = self.snapshot_manager.set_members(set_path).await?;
        let set_name = set_path
            .file_name()
            .and_then(|name| name.to_str())
            .context("invalid cache set name")?;
        let remote_set = format!("{remote_path}/{set_name}");
        validate_remote_path(&remote_set)?;
        // An existing destination is never overwritten. Failed transfers leave
        // an uncommitted directory for the operator to inspect.
        checked_ssh(remote_host, &format!("mkdir -- {remote_set}")).await?;
        for member in members {
            let member_name = member
                .file_name()
                .and_then(|name| name.to_str())
                .context("invalid cache member name")?;
            let mut sender = tokio::process::Command::new("btrfs");
            sender.arg("send").arg(&member);
            let mut receiver = ssh_command(remote_host, &format!("btrfs receive {remote_set}"));
            self.configure_transport_command(&mut sender, "cache_streaming")?;
            self.configure_transport_command(&mut receiver, "cache_streaming")?;
            managed_process::pipeline(&mut sender, &mut receiver, deadline).await?;
            checked_ssh(
                remote_host,
                &format!(
                    "btrfs subvolume show {remote_set}/{member_name} >/dev/null && \
                     test \"$(btrfs property get -ts {remote_set}/{member_name} ro)\" = ro=true"
                ),
            )
            .await?;
        }
        checked_ssh(remote_host, &format!("sync -f {remote_set}")).await?;
        checked_ssh(
            remote_host,
            &format!("printf 'cache-snapshot-set-v1\\nroot\\nembeddings\\nblocks\\nqueries\\ndiffs\\nkv\\n' > {remote_set}/.complete"),
        )
        .await?;
        checked_ssh(remote_host, &format!("sync -f {remote_set}")).await?;
        let marker = checked_ssh(remote_host, &format!("cat -- {remote_set}/.complete")).await?;
        if marker != COMPLETE_MARKER.as_bytes() {
            return Err("remote cache set marker mismatch".into());
        }
        temporary.cleanup().await?;
        info!(
            "Successfully streamed complete cache set to {}:{}",
            remote_host, remote_set
        );
        Ok(())
    }

    /// Receive cache data from remote system with NUMA affinity
    pub async fn receive_from_remote(
        &self,
        remote_host: &str,
        remote_snapshot: &str,
        local_path: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        validate_ssh_destination(remote_host)?;
        validate_remote_path(remote_snapshot)?;
        validate_remote_path(local_path)?;

        let deadline = transfer_timeout()?;
        let permit = transfer_permit().await?;

        let marker =
            checked_ssh(remote_host, &format!("cat -- {remote_snapshot}/.complete")).await?;
        if marker != COMPLETE_MARKER.as_bytes() {
            return Err("remote cache set is incomplete".into());
        }
        let set_name = Path::new(remote_snapshot)
            .file_name()
            .and_then(|name| name.to_str())
            .context("invalid remote cache set name")?;
        let local_parent = Path::new(local_path);
        let metadata = tokio::fs::symlink_metadata(local_parent).await?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("receive parent is not a directory".into());
        }
        let local_set = local_parent.join(set_name);
        let temporary = TemporarySet::create(local_set.clone(), permit)?;

        for member in CACHE_MEMBERS {
            let remote_member = format!("{remote_snapshot}/{member}");
            checked_ssh(
                remote_host,
                &format!(
                    "btrfs subvolume show {remote_member} >/dev/null && \
                     test \"$(btrfs property get -ts {remote_member} ro)\" = ro=true"
                ),
            )
            .await?;
            let mut sender = ssh_command(remote_host, &format!("btrfs send {remote_member}"));
            let mut receiver = tokio::process::Command::new("btrfs");
            receiver.arg("receive").arg(&local_set);
            self.configure_transport_command(&mut sender, "cache_receiving")?;
            self.configure_transport_command(&mut receiver, "cache_receiving")?;
            managed_process::pipeline(&mut sender, &mut receiver, deadline).await?;
        }
        SnapshotManager::verify_members(&local_set).await?;
        let marker_path = local_set.join(".complete");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker_path)?;
        use std::io::Write as _;
        file.write_all(COMPLETE_MARKER.as_bytes())?;
        file.sync_all()?;
        std::fs::File::open(&local_set)?.sync_all()?;
        self.snapshot_manager.set_members(&local_set).await?;
        temporary.preserve();
        info!(
            "Successfully received complete cache set at {}",
            local_set.display()
        );
        Ok(())
    }

    /// Get NUMA configuration info
    pub fn numa_info(&self) -> NumaInfo {
        NumaInfo {
            node_count: self.numa_topology.node_count(),
            cpu_affinity: self.cpu_affinity.clone(),
            placement_strategy: self.placement_strategy.clone(),
            memory_policy: self.memory_policy.clone(),
        }
    }

    /// Get cache directory path
    pub fn cache_dir(&self) -> &PathBuf {
        &self.cache_dir
    }

    /// Prepare child placement before spawn; never alter a shared async worker.
    fn configure_transport_command(
        &self,
        command: &mut tokio::process::Command,
        operation: &str,
    ) -> Result<()> {
        if matches!(self.placement_strategy, CachePlacementStrategy::Disabled) {
            return Ok(());
        }
        let selected = self.select_numa_node(operation);
        let cpus = selected
            .as_ref()
            .map(|node| node.cpu_list.clone())
            .unwrap_or_default();
        let policy = match &self.memory_policy {
            MemoryPolicy::Bind(nodes) => NumaMemoryPolicy::Bind(nodes.clone()),
            MemoryPolicy::Preferred(Some(node)) => NumaMemoryPolicy::Preferred(*node),
            MemoryPolicy::Preferred(None) => selected
                .map(|node| NumaMemoryPolicy::Preferred(node.node_id))
                .unwrap_or(NumaMemoryPolicy::Default),
            MemoryPolicy::Default => NumaMemoryPolicy::Default,
            MemoryPolicy::Interleave(nodes) => NumaMemoryPolicy::Interleave(nodes.clone()),
        };
        NumaOptimizer::configure_command(command, &cpus, &policy)
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub hot_entries: usize,
    pub total_accesses: u64,
    pub disk_usage_bytes: u64,
    pub embeddings_size_bytes: u64,
    pub blocks_size_bytes: u64,
}

impl CacheStats {
    pub fn hot_ratio(&self) -> f64 {
        if self.total_entries == 0 {
            0.0
        } else {
            self.hot_entries as f64 / self.total_entries as f64
        }
    }

    pub fn avg_accesses(&self) -> f64 {
        if self.total_entries == 0 {
            0.0
        } else {
            self.total_accesses as f64 / self.total_entries as f64
        }
    }
}
#[derive(Debug, Clone)]
/// NUMA configuration information
pub struct NumaInfo {
    pub node_count: usize,
    pub cpu_affinity: Vec<u32>,
    pub placement_strategy: CachePlacementStrategy,
    pub memory_policy: MemoryPolicy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numa_placement_is_stable_allowed_and_uses_free_memory() {
        let topology = NumaTopology::test_topology(
            vec![
                NumaNode {
                    node_id: 8,
                    cpu_list: vec![7, 9],
                    memory_total_kb: 100,
                    memory_free_kb: 90,
                    distance_to_nodes: HashMap::new(),
                },
                NumaNode {
                    node_id: 3,
                    cpu_list: vec![1, 2],
                    memory_total_kb: 1000,
                    memory_free_kb: 10,
                    distance_to_nodes: HashMap::new(),
                },
            ],
            None,
        );
        let choose = |strategy: CachePlacementStrategy, index, current| {
            BtrfsCache::select_placement_node(&topology, &strategy, index, Some(&[2, 7]), current)
        };
        let local = choose(CachePlacementStrategy::LocalNode, 0, Some(8)).unwrap();
        assert_eq!((local.node_id, local.cpu_list), (8, vec![7]));
        assert_eq!(
            choose(CachePlacementStrategy::MostMemory, 0, None)
                .unwrap()
                .node_id,
            8
        );
        assert_eq!(
            choose(CachePlacementStrategy::RoundRobin, 0, None)
                .unwrap()
                .node_id,
            3
        );
        assert_eq!(
            choose(CachePlacementStrategy::RoundRobin, 1, None)
                .unwrap()
                .node_id,
            8
        );
        assert!(choose(CachePlacementStrategy::Disabled, 0, None).is_none());
        assert!(
            BtrfsCache::select_placement_node(
                &topology,
                &CachePlacementStrategy::LocalNode,
                0,
                Some(&[0]),
                None
            )
            .is_none()
        );
    }
    use std::process::Stdio;

    /// TempDir cannot remove Btrfs subvolume roots. Clean all test-owned
    /// sources and snapshot members even when a test assertion unwinds.
    struct CacheTestDir(tempfile::TempDir);

    impl CacheTestDir {
        fn new() -> Self {
            Self(tempfile::tempdir().unwrap())
        }

        fn path(&self) -> &Path {
            self.0.path()
        }
    }

    impl Drop for CacheTestDir {
        fn drop(&mut self) {
            if !is_btrfs_filesystem(self.path()).unwrap_or(false) {
                return;
            }
            fn delete(path: &Path) {
                if !path.exists() {
                    return;
                }
                // A test-owned read-only snapshot must be made writable for
                // unprivileged fixture cleanup, even after an assertion fails.
                let _ = std::process::Command::new("btrfs")
                    .args(["property", "set", "-ts"])
                    .arg(path)
                    .args(["ro", "false"])
                    .output();
                let output = std::process::Command::new("btrfs")
                    .args(["subvolume", "delete"])
                    .arg(path)
                    .output();
                if let Ok(output) = output
                    && !output.status.success()
                {
                    eprintln!(
                        "test subvolume cleanup failed for {}: {}",
                        path.display(),
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
            let snapshots = self.path().join("@cache-snapshots");
            if let Ok(entries) = std::fs::read_dir(snapshots) {
                for entry in entries.flatten() {
                    let set = entry.path();
                    if set.is_dir() && !set.is_symlink() {
                        for member in CACHE_MEMBERS.iter().rev() {
                            delete(&set.join(member));
                        }
                    }
                }
            }
            let received = self.path().join("received");
            for member in CACHE_MEMBERS.iter().rev() {
                delete(&received.join(member));
            }
            let cache = self.path().join("cache");
            for category in ["kv", "diffs", "queries", "blocks", "embeddings"] {
                delete(&cache.join(category));
            }
            delete(&cache);
        }
    }

    #[tokio::test]
    async fn ordinary_filesystem_creates_cache_directory_without_btrfs_command() {
        let parent = tempfile::tempdir().unwrap();
        if is_btrfs_filesystem(parent.path()).unwrap() {
            return; // This fixture only exercises the ordinary-filesystem branch.
        }
        let cache = parent.path().join("cache");
        BtrfsCache::create_btrfs_subvolume(&cache).await.unwrap();
        assert!(cache.is_dir());
        assert!(!is_btrfs_filesystem(&cache).unwrap());
    }

    #[tokio::test]
    async fn owning_user_can_delete_complete_readonly_cache_set() {
        let parent = CacheTestDir::new();
        if !is_btrfs_filesystem(parent.path()).unwrap() {
            return;
        }
        let cache = BtrfsCache::new(parent.path().join("cache")).await.unwrap();
        let set = cache.create_snapshot().await.unwrap();
        assert!(set.join(".complete").is_file());
        SnapshotManager::verify_members(&set).await.unwrap();
        assert_eq!(cache.delete_all_snapshots().await.unwrap(), 1);
        assert!(!set.exists());
        assert!(cache.list_snapshots().await.unwrap().is_empty());
    }

    #[tokio::test]
    #[ignore = "native Btrfs send/receive requires root; isolated fixture deletes only its own subvolumes"]
    async fn btrfs_cache_reopens_children_and_snapshot_contains_child_data() {
        let parent = CacheTestDir::new();
        if !is_btrfs_filesystem(parent.path()).unwrap() {
            return; // Requires a Btrfs fixture filesystem.
        }
        let cache_path = parent.path().join("cache");
        let cache = BtrfsCache::new(cache_path.clone()).await.unwrap();
        for category in ["embeddings", "blocks", "queries", "diffs", "kv"] {
            let path = cache_path.join(category);
            assert_eq!(std::fs::metadata(&path).unwrap().ino(), 256);
            let probe = tokio::process::Command::new("btrfs")
                .args(["property", "get", "-ts"])
                .arg(&path)
                .arg("ro")
                .output()
                .await
                .unwrap();
            assert!(
                probe.status.success(),
                "{}",
                String::from_utf8_lossy(&probe.stderr)
            );
            assert_eq!(String::from_utf8_lossy(&probe.stdout).trim(), "ro=false");
        }
        let block = cache_path.join("blocks/by-hash/probe.json");
        tokio::fs::write(&block, b"before").await.unwrap();
        let set = cache.create_snapshot().await.unwrap();
        assert_eq!(
            tokio::fs::read(set.join("blocks/by-hash/probe.json"))
                .await
                .unwrap(),
            b"before"
        );
        let received = parent.path().join("received");
        tokio::fs::create_dir(&received).await.unwrap();
        for member in CACHE_MEMBERS {
            let send = std::process::Command::new("btrfs")
                .arg("send")
                .arg(set.join(member))
                .output()
                .unwrap();
            assert!(
                send.status.success(),
                "{}",
                String::from_utf8_lossy(&send.stderr)
            );
            let mut receive = std::process::Command::new("btrfs")
                .arg("receive")
                .arg(&received)
                .stdin(Stdio::piped())
                .spawn()
                .unwrap();
            use std::io::Write as _;
            receive
                .stdin
                .take()
                .unwrap()
                .write_all(&send.stdout)
                .unwrap();
            assert!(receive.wait().unwrap().success());
            let path = received.join(member);
            let ro = std::process::Command::new("btrfs")
                .args(["property", "get", "-ts"])
                .arg(&path)
                .arg("ro")
                .output()
                .unwrap();
            assert!(ro.status.success());
            assert_eq!(String::from_utf8_lossy(&ro.stdout).trim(), "ro=true");
        }
        assert_eq!(
            tokio::fs::read(received.join("blocks/by-hash/probe.json"))
                .await
                .unwrap(),
            b"before"
        );
        tokio::fs::write(&block, b"after").await.unwrap();
        assert_eq!(
            tokio::fs::read(set.join("blocks/by-hash/probe.json"))
                .await
                .unwrap(),
            b"before"
        );
        cache.clear_blocks().unwrap();
        assert_eq!(
            std::fs::metadata(cache_path.join("blocks")).unwrap().ino(),
            256
        );
        assert_eq!(
            tokio::fs::read(set.join("blocks/by-hash/probe.json"))
                .await
                .unwrap(),
            b"before"
        );
        assert_eq!(cache.list_snapshots().await.unwrap().len(), 1);
        drop(cache);

        let reopened = BtrfsCache::new(cache_path.clone()).await.unwrap();
        assert_eq!(reopened.list_snapshots().await.unwrap().len(), 1);
        reopened.delete_all_snapshots().await.unwrap();
        drop(reopened);
        for member in CACHE_MEMBERS.iter().rev() {
            assert!(
                tokio::process::Command::new("btrfs")
                    .args(["subvolume", "delete"])
                    .arg(received.join(member))
                    .output()
                    .await
                    .unwrap()
                    .status
                    .success()
            );
        }
        for category in ["kv", "diffs", "queries", "blocks", "embeddings"] {
            assert!(
                tokio::process::Command::new("btrfs")
                    .args(["subvolume", "delete"])
                    .arg(cache_path.join(category))
                    .output()
                    .await
                    .unwrap()
                    .status
                    .success()
            );
        }
        assert!(
            tokio::process::Command::new("btrfs")
                .args(["subvolume", "delete"])
                .arg(cache_path)
                .output()
                .await
                .unwrap()
                .status
                .success()
        );
    }

    #[test]
    fn remote_stream_arguments_reject_shell_syntax_and_traversal() {
        assert!(validate_ssh_destination("cache@example.internal").is_ok());
        assert!(validate_ssh_destination("cache;touch /tmp/pwned").is_err());
        assert!(validate_ssh_destination("-oProxyCommand=bad").is_err());

        assert!(validate_remote_path("/var/lib/op-dbus/cache").is_ok());
        assert!(validate_remote_path("/var/lib/op-dbus/../secret").is_err());
        assert!(validate_remote_path("/tmp/cache;touch_pwned").is_err());
        assert!(validate_remote_path("relative/cache").is_err());
    }

    #[tokio::test]
    async fn test_text_hashing() {
        let parent = CacheTestDir::new();
        let cache_path = parent.path().join("cache");
        let cache = BtrfsCache::new(cache_path).await.unwrap();
        let hash1 = cache.hash_text("test");
        let hash2 = cache.hash_text("test");
        let hash3 = cache.hash_text("different");

        assert_eq!(hash1, hash2);
        assert_ne!(hash1, hash3);
        assert_eq!(hash1.len(), 64); // SHA256 hex length
    }

    #[test]
    fn memory_cache_put_and_get() {
        let mc = MemoryCache::new(10, 10 * MEMORY_ENTRY_OVERHEAD + 1024);
        mc.put("a".into(), vec![1, 2, 3]);
        assert_eq!(mc.get("a"), Some(vec![1, 2, 3]));
        assert_eq!(mc.get("b"), None);
    }

    #[test]
    fn memory_cache_remove() {
        let mc = MemoryCache::new(10, 10 * MEMORY_ENTRY_OVERHEAD + 1024);
        mc.put("x".into(), vec![9]);
        assert!(mc.remove("x"));
        assert!(!mc.remove("x"));
        assert_eq!(mc.get("x"), None);
    }

    #[test]
    fn memory_cache_evicts_lru_on_entry_limit() {
        let mc = MemoryCache::new(2, 1024 * 1024);
        mc.put("a".into(), vec![1]);
        mc.put("b".into(), vec![2]);
        // Access "a" so it becomes most-recently used
        let _ = mc.get("a");
        // Insert "c" — should evict "b" (oldest access)
        mc.put("c".into(), vec![3]);
        assert!(mc.get("a").is_some());
        assert_eq!(mc.get("b"), None);
        assert!(mc.get("c").is_some());
    }

    #[test]
    fn memory_cache_evicts_on_memory_limit() {
        // A whole-entry budget forces eviction after a couple of entries.
        let mc = MemoryCache::new(1000, MEMORY_ENTRY_OVERHEAD + 10);
        mc.put("a".into(), vec![0u8; 6]);
        mc.put("b".into(), vec![0u8; 6]);
        // "a" should have been evicted to stay under 10 bytes
        assert_eq!(mc.get("a"), None);
        assert!(mc.get("b").is_some());
    }

    #[test]
    fn memory_cache_clear() {
        let mc = MemoryCache::new(10, 10 * MEMORY_ENTRY_OVERHEAD + 1024);
        mc.put("a".into(), vec![1]);
        mc.put("b".into(), vec![2]);
        mc.clear();
        assert_eq!(mc.get("a"), None);
        assert_eq!(mc.get("b"), None);
        assert_eq!(mc.current_memory.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn embedding_put_get() {
        let parent = CacheTestDir::new();
        let cache = BtrfsCache::new(parent.path().join("cache")).await.unwrap();

        cache.put_embedding("test text", &[1.0, 2.0, 3.0]).unwrap();
        let result = cache.get_embedding("test text").unwrap();
        assert_eq!(result, Some(vec![1.0, 2.0, 3.0]));
    }

    #[tokio::test]
    async fn kv_put_get_delete_round_trip() {
        let parent = CacheTestDir::new();
        let cache = BtrfsCache::new(parent.path().join("cache")).await.unwrap();

        // put + get
        cache.put_raw("hello", b"world").await.unwrap();
        let val = cache.get_raw("hello").await.unwrap();
        assert_eq!(val, Some(b"world".to_vec()));

        // delete
        let existed = cache.delete("hello").await.unwrap();
        assert!(existed);
        let val = cache.get_raw("hello").await.unwrap();
        assert_eq!(val, None);

        // delete again — not found
        let existed = cache.delete("hello").await.unwrap();
        assert!(!existed);
    }

    #[tokio::test]
    async fn failed_backing_file_delete_is_reported_and_retains_index() {
        let parent = CacheTestDir::new();
        let cache = BtrfsCache::new(parent.path().join("cache")).await.unwrap();
        cache.put_raw("key", b"value").await.unwrap();
        let backing = cache
            .cache_dir
            .join("kv")
            .join(format!("{}.dat", cache.hash_key("key")));
        tokio::fs::remove_file(&backing).await.unwrap();
        tokio::fs::create_dir(&backing).await.unwrap();
        assert!(
            cache
                .delete("key")
                .await
                .unwrap_err()
                .to_string()
                .contains("Failed to delete cache file")
        );
        tokio::fs::remove_dir(&backing).await.unwrap();
        // The failed removal did not pretend that its index row was deleted.
        assert!(cache.delete("key").await.unwrap());
    }

    #[tokio::test]
    async fn kv_get_or_compute() {
        let parent = CacheTestDir::new();
        let cache = BtrfsCache::new(parent.path().join("cache")).await.unwrap();

        let computed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let computed_clone = computed.clone();

        // First call should compute
        let val = cache
            .get_or_compute("key1", move || {
                computed_clone.fetch_add(1, Ordering::Relaxed);
                async { Ok(b"computed-value".to_vec()) }
            })
            .await
            .unwrap();
        assert_eq!(val, b"computed-value".to_vec());
        assert_eq!(computed.load(Ordering::Relaxed), 1);

        // Second call should use the cache (compute not called again)
        let val = cache
            .get_or_compute("key1", || async { Ok(b"should-not-be-used".to_vec()) })
            .await
            .unwrap();
        assert_eq!(val, b"computed-value".to_vec());
        assert_eq!(computed.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn kv_overwrite_replaces_value() {
        let parent = CacheTestDir::new();
        let cache = BtrfsCache::new(parent.path().join("cache")).await.unwrap();

        cache.put_raw("k", b"v1").await.unwrap();
        cache.put_raw("k", b"v2").await.unwrap();

        let val = cache.get_raw("k").await.unwrap();
        assert_eq!(val, Some(b"v2".to_vec()));
    }

    #[tokio::test]
    async fn kv_memory_cache_serves_fast_path() {
        let parent = CacheTestDir::new();
        let cache = BtrfsCache::new(parent.path().join("cache")).await.unwrap();

        cache.put_raw("fast", b"data").await.unwrap();

        // The in-memory fast path should be populated
        let mem_key = format!("kv:{}", cache.hash_key("fast"));
        assert!(
            cache
                .memory_cache
                .entries
                .lock()
                .unwrap()
                .contains_key(&mem_key)
        );

        // get_raw should return from memory
        let val = cache.get_raw("fast").await.unwrap();
        assert_eq!(val, Some(b"data".to_vec()));
    }

    async fn tmpfs_cache() -> (tempfile::TempDir, BtrfsCache) {
        let directory = tempfile::tempdir_in("/dev/shm").unwrap();
        let cache = BtrfsCache::new(directory.path().join("cache"))
            .await
            .unwrap();
        (directory, cache)
    }

    #[test]
    fn memory_budget_counts_keys_capacity_and_container_nodes() {
        let budget = MEMORY_ENTRY_OVERHEAD + 100;
        let mc = MemoryCache::new(10, budget);
        let mut oversized_capacity = Vec::with_capacity(101);
        oversized_capacity.push(1);
        mc.put("a".into(), oversized_capacity);
        assert!(mc.get("a").is_none());
        mc.put("k".repeat(101), Vec::new());
        assert!(mc.entries.lock().unwrap().is_empty());
        mc.put("a".into(), vec![1; 90]);
        assert!(mc.get("a").is_some());
        assert!(mc.current_memory.load(Ordering::Relaxed) <= budget);
        mc.clear();
        assert_eq!(mc.current_memory.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn concurrent_memory_inserts_never_publish_over_budget() {
        let budget = 4 * (MEMORY_ENTRY_OVERHEAD + 100);
        let mc = std::sync::Arc::new(MemoryCache::new(4, budget));
        std::thread::scope(|scope| {
            for writer in 0..8 {
                let mc = &mc;
                scope.spawn(move || {
                    for iteration in 0..50 {
                        mc.put(format!("{writer}:{iteration}"), vec![1; 90]);
                        let entries = mc.entries.lock().unwrap();
                        let retained: usize = entries
                            .iter()
                            .map(|(key, entry)| {
                                MemoryCache::charge(key.capacity(), entry.data.capacity())
                            })
                            .sum();
                        assert!(entries.len() <= 4);
                        assert!(retained <= budget);
                        assert_eq!(retained, mc.current_memory.load(Ordering::Relaxed));
                    }
                });
            }
        });
    }

    #[tokio::test]
    async fn index_is_compact_bounded_and_disk_fallback_survives_reopen() {
        let (directory, mut cache) = tmpfs_cache().await;
        cache.index_max_entries = 3;
        cache.memory_cache = MemoryCache::new(0, 0);
        for id in 0..20 {
            cache
                .put_embedding(&format!("{id}:{}", "x".repeat(65536)), &[id as f32])
                .unwrap();
            cache
                .put_raw(&format!("{id}:{}", "k".repeat(65536)), &[id])
                .await
                .unwrap();
            assert!(cache.index_lru.lock().unwrap().len() <= 3);
        }
        let embedding_rows = cache
            .index
            .run_query("?[text] := *embedding_cache[_, text, _, _, _, _, _]", None)
            .unwrap();
        let kv_rows = cache
            .index
            .run_query("?[key] := *kv_cache[_, key, _, _, _, _, _]", None)
            .unwrap();
        assert!(embedding_rows.as_array().unwrap().len() + kv_rows.as_array().unwrap().len() <= 3);
        assert!(
            embedding_rows
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["text"] == "")
        );
        assert!(
            kv_rows
                .as_array()
                .unwrap()
                .iter()
                .all(|row| row["key"] == "")
        );
        assert_eq!(cache.stats().unwrap().total_entries, 20);
        let first_text = format!("0:{}", "x".repeat(65536));
        let first_key = format!("0:{}", "k".repeat(65536));
        assert_eq!(cache.get_embedding(&first_text).unwrap(), Some(vec![0.0]));
        assert_eq!(cache.get_raw(&first_key).await.unwrap(), Some(vec![0]));
        drop(cache);
        let reopened = BtrfsCache::new(directory.path().join("cache"))
            .await
            .unwrap();
        assert_eq!(
            reopened.get_embedding(&first_text).unwrap(),
            Some(vec![0.0])
        );
        assert_eq!(reopened.get_raw(&first_key).await.unwrap(), Some(vec![0]));
        assert_eq!(reopened.stats().unwrap().total_entries, 20);
    }

    #[tokio::test]
    async fn expiry_invalidates_memory_evicted_metadata_and_recomputed_values() {
        let (_directory, mut cache) = tmpfs_cache().await;
        cache.index_max_entries = 2;
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3 * 86400);
        for id in 0..6 {
            let text = format!("embedding-{id}");
            cache.put_embedding(&text, &[id as f32]).unwrap();
            BtrfsCache::open_payload(&cache.embedding_path(&cache.hash_text(&text)))
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(old))
                .unwrap();
        }
        // A fresh hit must durably protect this entry even if its index row was evicted.
        assert_eq!(cache.get_embedding("embedding-0").unwrap(), Some(vec![0.0]));
        assert_eq!(cache.cleanup_old(1).unwrap(), 5);
        assert!(cache.get_embedding("embedding-1").unwrap().is_none());
        assert_eq!(
            cache
                .get_or_embed("embedding-1", |_| Ok(vec![99.0]))
                .unwrap(),
            vec![99.0]
        );
        assert_eq!(
            cache.get_embedding("embedding-1").unwrap(),
            Some(vec![99.0])
        );
        assert_eq!(cache.cleanup_old(-1).unwrap(), 2);
        assert!(cache.get_embedding("embedding-0").unwrap().is_none());
        assert!(cache.memory_cache.entries.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn expiry_reports_backing_failure_without_pretending_success() {
        let (_directory, cache) = tmpfs_cache().await;
        cache.put_embedding("key", &[1.0]).unwrap();
        let path = cache.embedding_path(&cache.hash_text("key"));
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(cache.cleanup_old(-1).is_err());
        assert_eq!(cache.index_lru.lock().unwrap().len(), 1);
        assert!(cache.get_embedding("key").is_err());
    }

    #[tokio::test]
    async fn malformed_embedding_and_oversized_payloads_are_rejected() {
        let (_directory, mut cache) = tmpfs_cache().await;
        cache.max_payload_bytes = 32;
        cache.max_embedding_dimensions = 4;
        assert!(cache.put_raw("too-big", &[0; 33]).await.is_err());
        assert!(!cache.kv_path(&cache.hash_key("too-big")).exists());
        assert!(cache.put_embedding("too-big", &[0.0; 5]).is_err());
        let hash = cache.hash_text("invalid");
        let path = cache.embedding_path(&hash);
        for bytes in [
            u64::MAX.to_le_bytes().to_vec(),
            4u64.to_le_bytes().to_vec(),
            [1u64.to_le_bytes().as_slice(), &[0; 5]].concat(),
            vec![0; 33],
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(cache.get_embedding("invalid").is_err());
            assert!(cache.memory_cache.entries.lock().unwrap().is_empty());
        }
        assert!(cache.cleanup_old(i64::MAX).is_err());
    }

    #[tokio::test]
    async fn separate_handles_detect_replacement_and_deletion() {
        let (directory, first) = tmpfs_cache().await;
        let second = BtrfsCache::new(directory.path().join("cache"))
            .await
            .unwrap();
        first.put_embedding("same", &[1.0]).unwrap();
        first.put_raw("same", b"old").await.unwrap();
        second.put_embedding("same", &[2.0]).unwrap();
        second.put_raw("same", b"new").await.unwrap();
        assert_eq!(first.get_embedding("same").unwrap(), Some(vec![2.0]));
        assert_eq!(first.get_raw("same").await.unwrap(), Some(b"new".to_vec()));
        second.delete("same").await.unwrap();
        second.cleanup_old(-1).unwrap();
        assert!(first.get_raw("same").await.unwrap().is_none());
        assert!(first.get_embedding("same").unwrap().is_none());
    }

    #[tokio::test]
    async fn compute_does_not_hold_lock_or_overwrite_a_concurrent_put() {
        let (_directory, cache) = tmpfs_cache().await;
        let cache = std::sync::Arc::new(cache);
        let entered = std::sync::Arc::new(std::sync::Barrier::new(2));
        let (release, resume) = std::sync::mpsc::channel();
        let worker_cache = cache.clone();
        let worker_entered = entered.clone();
        let worker = std::thread::spawn(move || {
            worker_cache
                .get_or_embed("key", |_| {
                    worker_entered.wait();
                    resume.recv().unwrap();
                    Ok(vec![1.0])
                })
                .unwrap()
        });
        entered.wait();
        cache.put_embedding("key", &[2.0]).unwrap();
        release.send(()).unwrap();
        assert_eq!(worker.join().unwrap(), vec![2.0]);
        assert_eq!(cache.get_embedding("key").unwrap(), Some(vec![2.0]));
        let result = cache
            .get_or_compute("raw", || async {
                cache.put_raw("raw", b"newer").await.unwrap();
                Ok(b"stale".to_vec())
            })
            .await
            .unwrap();
        assert_eq!(result, b"newer");
    }

    #[tokio::test]
    async fn clear_preserves_category_roots_and_releases_all_derived_owners() {
        let (_directory, cache) = tmpfs_cache().await;
        cache.put_embedding("key", &[1.0]).unwrap();
        cache.put_raw("key", b"value").await.unwrap();
        for category in ["queries", "diffs"] {
            std::fs::write(cache.cache_dir.join(category).join("payload"), b"data").unwrap();
        }
        assert_eq!(cache.stats().unwrap().disk_usage_bytes, 12 + 5 + 8);
        cache.clear().unwrap();
        assert!(cache.index_lru.lock().unwrap().is_empty());
        assert_eq!(cache.index_lru.lock().unwrap().capacity(), 0);
        assert!(cache.memory_cache.entries.lock().unwrap().is_empty());
        assert_eq!(cache.stats().unwrap().disk_usage_bytes, 0);
        assert!(cache.cache_dir.join(".cache.lock").is_file());
        for category in ["embeddings", "blocks", "queries", "diffs", "kv"] {
            assert!(cache.cache_dir.join(category).is_dir());
        }
    }
}
