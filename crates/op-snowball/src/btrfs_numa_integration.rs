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

use crate::snowball::StreamingSnowball;
use crate::PluginFootprint;
use anyhow::{Context, Result};
use op_cache::{BtrfsCache, NumaMemoryPolicy, NumaOptimizer, NumaTopology};
use simd_json::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Unified snowball with BTRFS cache and NUMA optimization
pub struct OptimizedSnowball {
    snowball: Arc<StreamingSnowball>,
    cache: Arc<BtrfsCache>,
    numa_topology: Arc<RwLock<Option<NumaTopology>>>,
    cache_enabled: bool,
}

impl OptimizedSnowball {
    /// Create optimized snowball with BTRFS cache and NUMA support
    pub async fn new(
        snowball_path: impl AsRef<Path>,
        cache_path: impl AsRef<Path>,
    ) -> Result<Self> {
        // Initialize snowball
        let snowball = Arc::new(
            StreamingSnowball::new(snowball_path)
                .await
                .context("Failed to initialize streaming snowball")?,
        );

        // Initialize BTRFS cache
        let cache = Arc::new(
            BtrfsCache::new(cache_path.as_ref().to_path_buf())
                .await
                .context("Failed to initialize BTRFS cache")?,
        );

        // Detect NUMA topology (best-effort, non-blocking)
        let numa_topology = Arc::new(RwLock::new(None));
        {
            match NumaTopology::detect() {
                Ok(topology) => {
                    info!("NUMA topology detected: {} nodes", topology.node_count());
                    *numa_topology.write().await = Some(topology);
                }
                Err(e) => {
                    warn!(
                        "NUMA topology detection failed: {} (continuing without NUMA)",
                        e
                    );
                }
            }
        }

        let cache_enabled = true;

        Ok(Self {
            snowball,
            cache,
            numa_topology,
            cache_enabled,
        })
    }

    /// Add footprint with NUMA-aware caching
    pub async fn add_footprint(&self, footprint: PluginFootprint) -> Result<String> {
        // Apply NUMA affinity for write operations
        self.apply_numa_affinity("snowball_write").await?;

        // Store in snowball (primary storage)
        let block_hash = self
            .snowball
            .add_footprint(footprint.clone())
            .await
            .context("Failed to add footprint to snowball")?;

        // Cache in BTRFS cache for fast retrieval
        if self.cache_enabled {
            if let Err(e) = self.cache_block(block_hash.clone(), &footprint).await {
                warn!("Failed to cache snowball block {}: {}", block_hash, e);
                // Don't fail the operation if caching fails
            }
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

        // Use cache's embedding storage for block data
        // (blocks are stored as JSON, not vectors, but we use the same infrastructure)
        // Store as JSON in cache (BTRFS will compress it)
        let cache_dir = self.cache.cache_dir();
        let blocks_dir = cache_dir.join("blocks").join("by-hash");
        tokio::fs::create_dir_all(&blocks_dir).await?;

        let block_file = blocks_dir.join(format!("{}.json", block_hash));
        let write_nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .context("system clock is before the Unix epoch")?
            .as_nanos();
        let block_tmp = blocks_dir.join(format!(
            ".{}-{}-{}.tmp",
            block_hash,
            std::process::id(),
            write_nonce
        ));
        let block_json = simd_json::to_string_pretty(&block_data)?;
        let write_result: Result<()> = async {
            let mut file = tokio::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&block_tmp)
                .await?;
            file.write_all(block_json.as_bytes()).await?;
            file.sync_all().await?;
            tokio::fs::rename(&block_tmp, &block_file)
                .await
                .context("Failed to commit block to cache")?;
            tokio::fs::File::open(&blocks_dir).await?.sync_all().await?;
            Ok(())
        }
        .await;
        if let Err(error) = write_result {
            let _ = tokio::fs::remove_file(&block_tmp).await;
            return Err(error);
        }

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

    /// Apply NUMA affinity for snowball operations
    async fn apply_numa_affinity(&self, operation: &str) -> Result<()> {
        let numa = self.numa_topology.read().await;
        if let Some(ref topology) = *numa {
            // Get optimal NUMA node
            let optimal_node = topology.optimal_node();

            if let Some(node) = topology.get_node(optimal_node) {
                debug!(
                    "Applying NUMA affinity: node {} ({} CPUs, {} MB free) for {}",
                    node.node_id,
                    node.cpu_list.len(),
                    node.memory_free_kb / 1024,
                    operation
                );
            }

            // Apply real CPU affinity via sched_setaffinity
            let optimizer = NumaOptimizer::from_env();
            if let Err(e) = optimizer.apply_cpu_affinity(optimal_node) {
                warn!(
                    "Failed to set CPU affinity for node {} ({}): {}",
                    optimal_node, operation, e
                );
            }

            // Apply default memory policy (preferred on the optimal node)
            let mem_policy = NumaMemoryPolicy::Preferred(optimal_node);
            if let Err(e) = NumaOptimizer::apply_memory_policy(&mem_policy) {
                warn!(
                    "Failed to set memory policy {:?} for {}: {}",
                    mem_policy, operation, e
                );
            }
        }
        Ok(())
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

        // Snapshot snowball
        let snowball_snapshot = self
            .snowball
            .as_ref()
            .state_subvolume_path()
            .parent()
            .ok_or_else(|| anyhow::anyhow!("No parent path for snowball"))?
            .join("snapshots")
            .join(format!(
                "snowball-{}",
                chrono::Utc::now().format("%Y%m%d-%H%M%S")
            ));

        // Use btrfs snapshot command
        let output = tokio::process::Command::new("btrfs")
            .args(["subvolume", "snapshot", "-r"])
            .arg(self.snowball.as_ref().state_subvolume_path())
            .arg(&snowball_snapshot)
            .output()
            .await
            .context("Failed to create snowball snapshot")?;

        if output.status.success() {
            snapshots.push(snowball_snapshot);
            info!(
                "Created snowball snapshot: {}",
                snapshots.last().unwrap().display()
            );
        }

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
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_optimized_snowball_creation_and_caching() {
        let snowball_parent = tempdir().unwrap();
        let cache_parent = tempdir().unwrap();
        let snowball_path = snowball_parent.path().join("snowball");
        let cache_path = cache_parent.path().join("cache");

        let opt_bc = OptimizedSnowball::new(snowball_path, cache_path).await;
        assert!(opt_bc.is_ok(), "Failed to create OptimizedSnowball");
        let opt_bc = opt_bc.unwrap();

        let footprint = PluginFootprint::new(
            "systemd",
            "unit_started",
            &simd_json::json!({"unit": "nginx.service"}),
        );
        let hash_result = opt_bc.add_footprint(footprint).await;
        assert!(hash_result.is_ok(), "Failed to add footprint with NUMA");
        let hash = hash_result.unwrap();

        let cached = opt_bc.get_cached_block(&hash).await;
        assert!(cached.is_ok(), "Failed to get cached block");
        let cached = cached.unwrap();
        assert!(cached.is_some(), "Block should be found in cache");
        let cached_footprint = cached.unwrap();
        assert_eq!(cached_footprint.plugin_id, "systemd");
        assert_eq!(cached_footprint.operation, "unit_started");
    }
}
