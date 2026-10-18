//! Unified BTRFS cache and NUMA integration for snowball footprints
//!
//! This module integrates:
//! - StreamingSnowball: Immutable audit trail with vectorization
//! - BtrfsCache: Unlimited disk-based caching with compression
//! - NumaTopology: NUMA-aware CPU/memory optimization
//!
//! Benefits:
//! - Snowball blocks cached in BTRFS cache (faster retrieval)
//! - NUMA-aware writes (optimal CPU/memory placement)
//! - Unified snapshot management
//! - Shared compression and deduplication

use crate::PluginFootprint;
use crate::snapshot::SnapshotInterval;
use crate::snowball::{StreamingSnowball, WriterPlacement};
use anyhow::{Context, Result};
use op_cache::{BtrfsCache, NumaMemoryPolicy, NumaOptimizer, NumaTopology};
use simd_json::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

fn select_writer_cpu(node_cpus: &[u32], allowed: &[u32], requested: Option<u32>) -> Result<u32> {
    let permitted: Vec<_> = node_cpus
        .iter()
        .copied()
        .filter(|cpu| allowed.contains(cpu))
        .collect();
    let fallback = permitted
        .iter()
        .copied()
        .max()
        .context("chosen NUMA node has no CPUs allowed for this identity")?;
    match requested {
        Some(cpu) if permitted.contains(&cpu) => Ok(cpu),
        Some(cpu) => {
            warn!(
                "OPDBUS_SNOWBALL_WRITER_CPU={cpu} is outside the chosen node's allowed CPUs {permitted:?}; using CPU {fallback}"
            );
            Ok(fallback)
        }
        None => Ok(fallback),
    }
}

/// Unified snowball with BTRFS cache and NUMA optimization
pub struct OptimizedSnowball {
    snowball: Arc<StreamingSnowball>,
    cache: Arc<BtrfsCache>,
    numa_topology: Arc<RwLock<Option<NumaTopology>>>,
    cache_enabled: bool,
}

impl OptimizedSnowball {
    /// Create optimized snowball with BTRFS cache and NUMA support.
    ///
    /// NUMA topology is detected first so the snowball's dedicated writer
    /// thread can be pinned (CPU affinity + preferred memory policy) before it
    /// accepts work. Everything durable — timing block, vector file, cache
    /// block — is then written by that one pinned thread. The preferred memory
    /// policy is best effort and does not guarantee where pages are placed.
    /// Tokio workers are never pinned.
    pub async fn new(
        snowball_path: impl AsRef<Path>,
        cache_path: impl AsRef<Path>,
    ) -> Result<Self> {
        // Detect NUMA topology (best-effort)
        let detected = match NumaTopology::detect() {
            Ok(topology) => {
                info!("NUMA topology detected: {} nodes", topology.node_count());
                Some(topology)
            }
            Err(e) => {
                warn!(
                    "NUMA topology detection failed: {} (continuing without NUMA)",
                    e
                );
                None
            }
        };

        // Placement hook executed once on the writer thread: one node, one
        // core. The streaming writer is a single thread, so pinning it to a
        // single CPU of the optimal node keeps its L1/L2 working set warm as
        // well as its memory local. `OPDBUS_SNOWBALL_WRITER_CPU` overrides the
        // core (must belong to the chosen node and the identity's allowed CPU
        // mask); by default the highest permitted CPU of the node is used.
        let placement: Option<WriterPlacement> = detected.as_ref().and_then(|topology| {
            let mut optimizer = NumaOptimizer::from_env();
            // Explicitly disabled placement must remain disabled, rather than
            // falling back to a node and silently pinning anyway.
            let node = optimizer.get_optimal_node()?;
            let node_cpus = optimizer.topology().unwrap_or(topology).cpus_for_node(node);
            let Some(allowed) = NumaOptimizer::current_cpu_affinity() else {
                warn!("Cannot read allowed CPUs; snowball writer runs unpinned");
                return None;
            };
            let requested = std::env::var("OPDBUS_SNOWBALL_WRITER_CPU")
                .ok()
                .and_then(|v| v.trim().parse::<u32>().ok());
            let cpu = match select_writer_cpu(&node_cpus, &allowed, requested) {
                Ok(cpu) => cpu,
                Err(error) => {
                    warn!("NUMA node {node}: {error}; snowball writer runs unpinned");
                    return None;
                }
            };
            Some(Box::new(move || -> Result<()> {
                debug!("snowball writer: pinning to CPU {cpu} on NUMA node {node}");
                NumaOptimizer::apply_cpu_affinity_to(
                    &[cpu],
                    &format!("snowball writer CPU {cpu}"),
                )?;
                if let Err(error) = NumaOptimizer::apply_memory_policy(&NumaMemoryPolicy::Preferred(node)) {
                    warn!("Snowball writer remains pinned to CPU {cpu}; preferred NUMA memory policy for node {node} failed: {error:#}");
                }
                Ok(())
            }) as WriterPlacement)
        });

        // Initialize snowball with the pinned writer
        let snowball = Arc::new(
            StreamingSnowball::new_with_placement(
                snowball_path,
                SnapshotInterval::from_env(),
                placement,
            )
            .await
            .context("Failed to initialize streaming snowball")?,
        );

        // Initialize BTRFS cache
        let cache = Arc::new(
            BtrfsCache::new(cache_path.as_ref().to_path_buf())
                .await
                .context("Failed to initialize BTRFS cache")?,
        );

        let numa_topology = Arc::new(RwLock::new(detected));
        let cache_enabled = true;

        Ok(Self {
            snowball,
            cache,
            numa_topology,
            cache_enabled,
        })
    }

    /// Add footprint; the timing block, vector file and cache block are all
    /// written by the snowball's pinned writer thread.
    pub async fn add_footprint(&self, footprint: PluginFootprint) -> Result<String> {
        // Store in snowball (primary storage)
        let block_hash = self
            .snowball
            .add_footprint(footprint.clone())
            .await
            .context("Failed to add footprint to snowball")?;

        // Cache in BTRFS cache for fast retrieval
        if self.cache_enabled
            && let Err(e) = self.cache_block(block_hash.clone(), &footprint).await
        {
            warn!("Failed to cache snowball block {}: {}", block_hash, e);
            // Don't fail the operation if caching fails
        }

        Ok(block_hash)
    }

    /// Cache snowball block in BTRFS cache
    async fn cache_block(&self, block_hash: String, footprint: &PluginFootprint) -> Result<()> {
        // Serialize footprint for caching
        let block_data = simd_json::json!({
            "plugin_id": footprint.plugin_id,
            "operation": footprint.operation,
            "timestamp": footprint.timestamp,
            "payload": footprint.payload,
            "metadata": footprint.metadata,
            "vector_features": footprint.vector_features,
        });

        // Written by the same CPU-pinned thread that wrote the timing block;
        // page placement depends on whether the NUMA memory policy was applied.
        let blocks_dir = self.cache.cache_dir().join("blocks").join("by-hash");
        let block_json = simd_json::to_string_pretty(&block_data)?;
        let hash_for_write = block_hash.clone();
        self.snowball
            .run_on_writer(move || -> Result<()> {
                use std::io::Write as _;
                std::fs::create_dir_all(&blocks_dir)?;
                let block_file = blocks_dir.join(format!("{}.json", hash_for_write));
                let write_nonce = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .context("system clock is before the Unix epoch")?
                    .as_nanos();
                let block_tmp = blocks_dir.join(format!(
                    ".{}-{}-{}.tmp",
                    hash_for_write,
                    std::process::id(),
                    write_nonce
                ));
                let write_result: Result<()> = (|| {
                    let mut file = std::fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(&block_tmp)?;
                    file.write_all(block_json.as_bytes())?;
                    file.sync_all()?;
                    std::fs::rename(&block_tmp, &block_file)
                        .context("Failed to commit block to cache")?;
                    std::fs::File::open(&blocks_dir)?.sync_all()?;
                    Ok(())
                })();
                if let Err(error) = write_result {
                    let _ = std::fs::remove_file(&block_tmp);
                    return Err(error);
                }
                Ok(())
            })
            .await?;

        debug!("Cached snowball block {} in BTRFS cache", block_hash);
        Ok(())
    }

    /// Get cached block from BTRFS cache (fast path)
    pub async fn get_cached_block(&self, block_hash: &str) -> Result<Option<PluginFootprint>> {
        if !self.cache_enabled {
            return Ok(None);
        }

        let cache_dir = self.cache.cache_dir();
        let block_file = cache_dir
            .join("blocks")
            .join("by-hash")
            .join(format!("{}.json", block_hash));

        if !block_file.exists() {
            return Ok(None);
        }

        // Read from BTRFS cache (page cache will keep hot blocks in RAM)
        let mut data = tokio::fs::read_to_string(&block_file).await?;
        let block_data: simd_json::OwnedValue = unsafe { simd_json::from_str(&mut data)? };

        // Reconstruct footprint
        let footprint = PluginFootprint {
            plugin_id: block_data["plugin_id"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing plugin_id"))?
                .to_string(),
            operation: block_data["operation"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing operation"))?
                .to_string(),
            timestamp: block_data["timestamp"]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("Missing timestamp"))?,
            payload: block_data
                .get("payload")
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Missing payload"))?,
            metadata: simd_json::serde::from_owned_value(block_data["metadata"].clone())?,
            vector_features: simd_json::serde::from_owned_value(
                block_data["vector_features"].clone(),
            )?,
        };

        Ok(Some(footprint))
    }

    /// Get snowball instance (for direct access if needed)
    pub fn snowball(&self) -> &Arc<StreamingSnowball> {
        &self.snowball
    }

    /// Get cache instance
    pub fn cache(&self) -> &Arc<BtrfsCache> {
        &self.cache
    }

    /// Get NUMA topology info
    pub async fn numa_info(&self) -> Option<NumaTopology> {
        self.numa_topology.read().await.clone()
    }

    /// Start footprint receiver with caching
    pub async fn start_footprint_receiver(
        &self,
        mut receiver: tokio::sync::mpsc::UnboundedReceiver<PluginFootprint>,
    ) -> Result<()> {
        info!("Starting optimized footprint receiver (with BTRFS cache and NUMA)");

        while let Some(footprint) = receiver.recv().await {
            if let Err(e) = self.add_footprint(footprint).await {
                tracing::error!("Failed to add footprint: {}", e);
                // Continue processing other footprints
            }
        }

        info!("Optimized footprint receiver shutting down");
        Ok(())
    }

    /// Create unified snapshot (snowball + cache)
    pub async fn create_unified_snapshot(&self) -> Result<Vec<PathBuf>> {
        let mut snapshots = Vec::new();

        // One committed set contains timing, vectors and state at the same
        // writer-locked point; a state-only snapshot is not a recovery point.
        let snowball_name = self.snowball.create_snapshot().await?;
        let snowball_snapshot = self
            .snowball
            .base_path()
            .join("snapshots")
            .join(snowball_name);
        snapshots.push(snowball_snapshot);
        info!(
            "Created snowball snapshot: {}",
            snapshots.last().unwrap().display()
        );

        // Snapshot cache
        let cache_snapshot = self.cache.create_snapshot().await?;
        snapshots.push(cache_snapshot);
        info!(
            "Created cache snapshot: {}",
            snapshots.last().unwrap().display()
        );

        Ok(snapshots)
    }

    /// Get cache statistics
    pub async fn cache_stats(&self) -> Result<op_cache::btrfs_cache::CacheStats> {
        self.cache.stats()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn writer_cpu_respects_allowed_sparse_mask() {
        assert_eq!(
            super::select_writer_cpu(&[0, 1, 7, 11], &[1, 7], None).unwrap(),
            7
        );
        assert_eq!(
            super::select_writer_cpu(&[0, 1, 7, 11], &[1, 7], Some(1)).unwrap(),
            1
        );
        assert_eq!(
            super::select_writer_cpu(&[0, 1, 7, 11], &[1, 7], Some(11)).unwrap(),
            7
        );
        assert!(super::select_writer_cpu(&[0, 1], &[7], None).is_err());
    }
    /// The placement hook must run on the writer thread and its pin must
    /// still be in effect when writes execute there — while the caller's
    /// (tokio) thread is left untouched.
    #[tokio::test]
    async fn writer_thread_carries_the_pin_and_tokio_workers_do_not() {
        use crate::snowball::{StreamingSnowball, WriterPlacement};
        use op_cache::NumaOptimizer;

        let Some(before) = NumaOptimizer::current_cpu_affinity() else {
            return; // non-Linux
        };
        let target = *before.first().expect("at least one runnable CPU");
        let dir = crate::snowball::tests::tempdir().unwrap();
        let placement: WriterPlacement =
            Box::new(move || NumaOptimizer::apply_cpu_affinity_to(&[target], "test writer"));
        let snowball = StreamingSnowball::new_with_placement(
            dir.path(),
            crate::snapshot::SnapshotInterval::from_env(),
            Some(placement),
        )
        .await
        .unwrap();

        let (on_writer, name) = snowball
            .run_on_writer(|| {
                Ok((
                    NumaOptimizer::current_cpu_affinity(),
                    std::thread::current().name().map(str::to_owned),
                ))
            })
            .await
            .unwrap();
        assert_eq!(
            on_writer,
            Some(vec![target]),
            "writer thread is pinned to one CPU"
        );
        assert_eq!(name.as_deref(), Some("snowball-writer"));

        // A real write goes through the same thread and stays pinned.
        let hash = snowball
            .add_footprint(crate::PluginFootprint::new(
                "test",
                "write",
                &simd_json::json!({"k": "v"}),
            ))
            .await
            .unwrap();
        assert!(!hash.is_empty());
        let after_write = snowball
            .run_on_writer(|| Ok(NumaOptimizer::current_cpu_affinity()))
            .await
            .unwrap();
        assert_eq!(after_write, Some(vec![target]));

        // The calling thread's mask is unchanged.
        assert_eq!(NumaOptimizer::current_cpu_affinity(), Some(before));
    }

    use super::*;
    #[tokio::test]
    #[ignore = "requires an isolated Btrfs fixture at OPDBUS_TEST_BTRFS_ROOT"]
    async fn test_optimized_snowball_creation_and_caching() {
        struct Fixture(tempfile::TempDir);
        impl Drop for Fixture {
            fn drop(&mut self) {
                use std::os::unix::fs::MetadataExt;
                fn collect(path: &std::path::Path, volumes: &mut Vec<std::path::PathBuf>) {
                    let Ok(entries) = std::fs::read_dir(path) else {
                        return;
                    };
                    for entry in entries.flatten() {
                        let child = entry.path();
                        let Ok(metadata) = std::fs::symlink_metadata(&child) else {
                            continue;
                        };
                        if metadata.is_dir() && !metadata.file_type().is_symlink() {
                            collect(&child, volumes);
                            if metadata.ino() == 256 {
                                volumes.push(child);
                            }
                        }
                    }
                }
                let mut volumes = Vec::new();
                collect(self.0.path(), &mut volumes);
                for volume in volumes {
                    let _ = std::process::Command::new("btrfs")
                        .args(["property", "set", "-ts"])
                        .arg(&volume)
                        .args(["ro", "false"])
                        .output();
                    let _ = std::process::Command::new("btrfs")
                        .args(["subvolume", "delete"])
                        .arg(&volume)
                        .output();
                }
            }
        }
        let btrfs_root = std::path::PathBuf::from(
            std::env::var_os("OPDBUS_TEST_BTRFS_ROOT")
                .expect("set OPDBUS_TEST_BTRFS_ROOT to an isolated Btrfs directory"),
        );
        let fixture = Fixture(
            tempfile::Builder::new()
                .prefix("opdbus-optimized-snowball-")
                .tempdir_in(&btrfs_root)
                .expect("create isolated Btrfs fixture"),
        );
        let snowball_path = fixture.0.path().join("snowball");
        let cache_path = fixture.0.path().join("cache");

        let opt_bc = OptimizedSnowball::new(snowball_path, cache_path)
            .await
            .expect("create OptimizedSnowball on Btrfs fixture");

        let footprint = PluginFootprint::new(
            "systemd",
            "unit_started",
            &simd_json::json!({"unit": "nginx.service"}),
        );
        let hash = opt_bc
            .add_footprint(footprint)
            .await
            .expect("add footprint with NUMA");

        let cached = opt_bc
            .get_cached_block(&hash)
            .await
            .expect("read cached block");
        assert!(cached.is_some(), "Block should be found in cache");
        let cached_footprint = cached.unwrap();
        assert_eq!(cached_footprint.plugin_id, "systemd");
        assert_eq!(cached_footprint.operation, "unit_started");
    }
}
