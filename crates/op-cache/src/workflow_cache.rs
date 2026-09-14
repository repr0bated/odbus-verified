//! Workflow step caching with TTL and input-based keying
//!
//! Caches intermediate results from workflow steps to avoid
//! redundant computation when the same inputs are processed.
//!
//! Features:
//! - Input-hash based caching
//! - Configurable TTL per cache entry
//! - Hot/cold data tracking
//! - BTRFS-backed storage with compression
//! - Cache invalidation strategies

use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::{debug, info};

/// Configuration for workflow caching
#[derive(Debug, Clone)]
pub struct WorkflowCacheConfig {
    /// Default TTL for cached results in seconds (default: 1 hour)
    pub default_ttl_secs: i64,
    /// Maximum cache size in bytes (default: 1GB)
    pub max_size_bytes: u64,
    /// Enable compression for cached data (default: true)
    pub compress: bool,
    /// Hot entry threshold in seconds (default: 10 minutes)
    pub hot_threshold_secs: i64,
}

impl Default for WorkflowCacheConfig {
    fn default() -> Self {
        Self {
            default_ttl_secs: 3600,             // 1 hour
            max_size_bytes: 1024 * 1024 * 1024, // 1GB
            compress: true,
            hot_threshold_secs: 600, // 10 minutes
        }
    }
}

/// Cached step result with metadata
#[derive(Debug, Clone)]
pub struct CachedStepResult {
    pub workflow_id: String,
    pub step_index: usize,
    pub input_hash: String,
    pub output: Vec<u8>,
    pub created_at: i64,
    pub expires_at: i64,
    pub access_count: u32,
    pub last_accessed: i64,
    pub size_bytes: u64,
}

impl CachedStepResult {
    /// Check if the cached result is expired
    pub fn is_expired(&self) -> bool {
        chrono::Utc::now().timestamp() > self.expires_at
    }

    /// Check if this is a "hot" cache entry
    pub fn is_hot(&self, threshold_secs: i64) -> bool {
        let now = chrono::Utc::now().timestamp();
        now - self.last_accessed < threshold_secs
    }
}

pub struct WorkflowCache {
    cache_dir: PathBuf,
    db: Mutex<rusqlite::Connection>,
    config: WorkflowCacheConfig,
}

impl WorkflowCache {
    /// Create new workflow cache
    pub async fn new(cache_dir: PathBuf, config: WorkflowCacheConfig) -> Result<Self> {
        let workflows_dir = cache_dir.join("workflows");
        let data_dir = workflows_dir.join("data");

        tokio::fs::create_dir_all(&data_dir).await?;

        let db_path = workflows_dir.join("cache.db");
        let db = rusqlite::Connection::open(&db_path)
            .context("Failed to open workflow cache database")?;

        // Create tables
        db.execute_batch(
            r#"
            -- Main cache table
            CREATE TABLE IF NOT EXISTS workflow_step_cache (
                cache_key TEXT PRIMARY KEY,
                workflow_id TEXT NOT NULL,
                step_index INTEGER NOT NULL,
                input_hash TEXT NOT NULL,
                output_file TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                access_count INTEGER DEFAULT 1,
                last_accessed INTEGER NOT NULL,
                size_bytes INTEGER NOT NULL,
                compressed INTEGER DEFAULT 0
            );

            -- Workflow-level cache metadata
            CREATE TABLE IF NOT EXISTS workflow_cache_meta (
                workflow_id TEXT PRIMARY KEY,
                total_entries INTEGER DEFAULT 0,
                total_size_bytes INTEGER DEFAULT 0,
                hit_count INTEGER DEFAULT 0,
                miss_count INTEGER DEFAULT 0,
                last_hit INTEGER,
                last_miss INTEGER
            );

            -- Indexes
            CREATE INDEX IF NOT EXISTS idx_cache_workflow ON workflow_step_cache(workflow_id);
            CREATE INDEX IF NOT EXISTS idx_cache_expires ON workflow_step_cache(expires_at);
            CREATE INDEX IF NOT EXISTS idx_cache_accessed ON workflow_step_cache(last_accessed DESC);
            CREATE INDEX IF NOT EXISTS idx_cache_input ON workflow_step_cache(workflow_id, step_index, input_hash);
            "#,
        )?;

        info!("Workflow cache initialized at {:?}", db_path);

        Ok(Self {
            cache_dir: workflows_dir,
            db: Mutex::new(db),
            config,
        })
    }

    /// Get cached result for a workflow step
    pub fn get(
        &self,
        workflow_id: &str,
        step_index: usize,
        input_hash: &str,
    ) -> Result<Option<Vec<u8>>> {
        let cache_key = self.make_cache_key(workflow_id, step_index, input_hash);
        let now = chrono::Utc::now().timestamp();

        let db = self.db.lock().unwrap();

        // Look up cache entry
        let entry: Option<(String, i64, bool)> = db
            .query_row(
                "SELECT output_file, expires_at, compressed
                 FROM workflow_step_cache
                 WHERE cache_key = ?1",
                [&cache_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        let (output_file, expires_at, compressed) = match entry {
            Some(e) => e,
            None => {
                // Record miss
                self.record_miss(&db, workflow_id)?;
                return Ok(None);
            }
        };

        // Check expiration
        if now > expires_at {
            debug!("Cache entry expired for {}", cache_key);
            drop(db);
            self.invalidate(workflow_id, step_index, input_hash)?;
            return Ok(None);
        }

        // Update access stats
        db.execute(
            "UPDATE workflow_step_cache
             SET access_count = access_count + 1, last_accessed = ?1
             WHERE cache_key = ?2",
            rusqlite::params![now, cache_key],
        )?;

        // Record hit
        self.record_hit(&db, workflow_id)?;

        drop(db);

        // Read data from file
        let data_path = self.cache_dir.join("data").join(&output_file);
        let data = std::fs::read(&data_path)
            .context(format!("Failed to read cached data: {:?}", data_path))?;

        // Decompress if needed
        let output = if compressed {
            self.decompress(&data)?
        } else {
            data
        };

        debug!(
            "Cache hit for workflow {} step {} (key: {})",
            workflow_id, step_index, cache_key
        );

        Ok(Some(output))
    }

    /// Store result in cache
    pub fn put(
        &self,
        workflow_id: &str,
        step_index: usize,
        input_hash: &str,
        output: &[u8],
        ttl_secs: Option<i64>,
    ) -> Result<()> {
        let cache_key = self.make_cache_key(workflow_id, step_index, input_hash);
        let now = chrono::Utc::now().timestamp();
        let ttl = ttl_secs.unwrap_or(self.config.default_ttl_secs);
        let expires_at = now + ttl;

        // Compress if enabled and beneficial
        let (data, compressed) = if self.config.compress && output.len() > 1024 {
            match self.compress(output) {
                Ok(compressed_data) if compressed_data.len() < output.len() => {
                    (compressed_data, true)
                }
                _ => (output.to_vec(), false),
            }
        } else {
            (output.to_vec(), false)
        };

        let size_bytes = data.len() as u64;

        // Write data to file
        let output_file = format!("{}.cache", cache_key);
        let data_path = self.cache_dir.join("data").join(&output_file);
        std::fs::write(&data_path, &data)?;

        // Update database
        let db = self.db.lock().unwrap();

        db.execute(
            "INSERT INTO workflow_step_cache
             (cache_key, workflow_id, step_index, input_hash, output_file,
              created_at, expires_at, last_accessed, size_bytes, compressed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(cache_key) DO UPDATE SET
                output_file = ?5,
                expires_at = ?7,
                last_accessed = ?8,
                size_bytes = ?9,
                compressed = ?10,
                access_count = access_count + 1",
            rusqlite::params![
                cache_key,
                workflow_id,
                step_index,
                input_hash,
                output_file,
                now,
                expires_at,
                now,
                size_bytes,
                compressed
            ],
        )?;

        // Update workflow metadata
        self.update_workflow_meta(&db, workflow_id)?;

        debug!(
            "Cached workflow {} step {} output ({} bytes, compressed: {})",
            workflow_id, step_index, size_bytes, compressed
        );

        Ok(())
    }

    /// Invalidate a specific cache entry
    pub fn invalidate(&self, workflow_id: &str, step_index: usize, input_hash: &str) -> Result<()> {
        let cache_key = self.make_cache_key(workflow_id, step_index, input_hash);

        let db = self.db.lock().unwrap();

        // Get file path before deleting
        let output_file: Option<String> = db
            .query_row(
                "SELECT output_file FROM workflow_step_cache WHERE cache_key = ?1",
                [&cache_key],
                |row| row.get(0),
            )
            .optional()?;

        // Delete from database
        db.execute(
            "DELETE FROM workflow_step_cache WHERE cache_key = ?1",
            [&cache_key],
        )?;

        drop(db);

        // Delete file
        if let Some(file) = output_file {
            let data_path = self.cache_dir.join("data").join(&file);
            let _ = std::fs::remove_file(data_path);
        }

        debug!("Invalidated cache entry: {}", cache_key);

        Ok(())
    }

    /// Invalidate all cache entries for a workflow
    pub fn invalidate_workflow(&self, workflow_id: &str) -> Result<usize> {
        let db = self.db.lock().unwrap();

        // Get all file paths
        let mut stmt =
            db.prepare("SELECT output_file FROM workflow_step_cache WHERE workflow_id = ?1")?;

        let files: Vec<String> = stmt
            .query_map([workflow_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let count = files.len();

        // Delete from database
        db.execute(
            "DELETE FROM workflow_step_cache WHERE workflow_id = ?1",
            [workflow_id],
        )?;

        // Delete workflow meta
        db.execute(
            "DELETE FROM workflow_cache_meta WHERE workflow_id = ?1",
            [workflow_id],
        )?;

        // Delete files
        for file in files {
            let data_path = self.cache_dir.join("data").join(&file);
            let _ = std::fs::remove_file(data_path);
        }

        info!(
            "Invalidated {} cache entries for workflow {}",
            count, workflow_id
        );

        Ok(count)
    }

    /// Invalidate all cache entries for a specific step (all inputs)
    pub fn invalidate_step(&self, workflow_id: &str, step_index: usize) -> Result<usize> {
        let db = self.db.lock().unwrap();

        let mut stmt = db.prepare(
            "SELECT output_file FROM workflow_step_cache
             WHERE workflow_id = ?1 AND step_index = ?2",
        )?;

        let files: Vec<String> = stmt
            .query_map(rusqlite::params![workflow_id, step_index], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let count = files.len();

        db.execute(
            "DELETE FROM workflow_step_cache
             WHERE workflow_id = ?1 AND step_index = ?2",
            rusqlite::params![workflow_id, step_index],
        )?;

        for file in files {
            let data_path = self.cache_dir.join("data").join(&file);
            let _ = std::fs::remove_file(data_path);
        }

        info!(
            "Invalidated {} cache entries for workflow {} step {}",
            count, workflow_id, step_index
        );

        Ok(count)
    }

    /// Cleanup expired entries
    pub fn cleanup_expired(&self) -> Result<CleanupResult> {
        let now = chrono::Utc::now().timestamp();
        let db = self.db.lock().unwrap();

        // Find expired entries
        let mut stmt = db.prepare(
            "SELECT output_file, size_bytes FROM workflow_step_cache
             WHERE expires_at < ?1",
        )?;

        let expired: Vec<(String, u64)> = stmt
            .query_map([now], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;

        let count = expired.len();
        let bytes_freed: u64 = expired.iter().map(|(_, size)| size).sum();

        // Delete from database
        db.execute(
            "DELETE FROM workflow_step_cache WHERE expires_at < ?1",
            [now],
        )?;

        // Delete files
        for (file, _) in expired {
            let data_path = self.cache_dir.join("data").join(&file);
            let _ = std::fs::remove_file(data_path);
        }

        if count > 0 {
            info!(
                "Cleaned up {} expired cache entries ({} bytes freed)",
                count, bytes_freed
            );
        }

        Ok(CleanupResult {
            entries_removed: count,
            bytes_freed,
        })
    }

    /// Evict oldest entries to stay under size limit
    pub fn evict_to_size(&self, max_bytes: u64) -> Result<CleanupResult> {
        let db = self.db.lock().unwrap();

        // Get current total size
        let total_size: u64 = db.query_row(
            "SELECT COALESCE(SUM(size_bytes), 0) FROM workflow_step_cache",
            [],
            |row| row.get(0),
        )?;

        if total_size <= max_bytes {
            return Ok(CleanupResult {
                entries_removed: 0,
                bytes_freed: 0,
            });
        }

        let target_reduction = total_size - max_bytes;
        let mut bytes_freed = 0u64;
        let mut count = 0usize;

        // Get oldest entries first
        let mut stmt = db.prepare(
            "SELECT cache_key, output_file, size_bytes FROM workflow_step_cache
             ORDER BY last_accessed ASC",
        )?;

        let entries: Vec<(String, String, u64)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;

        drop(stmt);

        // Evict until we've freed enough space
        for (cache_key, file, size) in entries {
            if bytes_freed >= target_reduction {
                break;
            }

            db.execute(
                "DELETE FROM workflow_step_cache WHERE cache_key = ?1",
                [&cache_key],
            )?;

            let data_path = self.cache_dir.join("data").join(&file);
            let _ = std::fs::remove_file(data_path);

            bytes_freed += size;
            count += 1;
        }

        info!(
            "Evicted {} cache entries ({} bytes freed) to stay under limit",
            count, bytes_freed
        );

        Ok(CleanupResult {
            entries_removed: count,
            bytes_freed,
        })
    }

    /// Get cache statistics
    pub fn stats(&self) -> Result<CacheStats> {
        let db = self.db.lock().unwrap();

        let total_entries: u64 =
            db.query_row("SELECT COUNT(*) FROM workflow_step_cache", [], |row| {
                row.get(0)
            })?;

        let total_size: u64 = db.query_row(
            "SELECT COALESCE(SUM(size_bytes), 0) FROM workflow_step_cache",
            [],
            |row| row.get(0),
        )?;

        let hot_threshold = chrono::Utc::now().timestamp() - self.config.hot_threshold_secs;
        let hot_entries: u64 = db.query_row(
            "SELECT COUNT(*) FROM workflow_step_cache WHERE last_accessed > ?1",
            [hot_threshold],
            |row| row.get(0),
        )?;

        let expired_entries: u64 = db.query_row(
            "SELECT COUNT(*) FROM workflow_step_cache WHERE expires_at < ?1",
            [chrono::Utc::now().timestamp()],
            |row| row.get(0),
        )?;

        let total_hits: u64 = db.query_row(
            "SELECT COALESCE(SUM(hit_count), 0) FROM workflow_cache_meta",
            [],
            |row| row.get(0),
        )?;

        let total_misses: u64 = db.query_row(
            "SELECT COALESCE(SUM(miss_count), 0) FROM workflow_cache_meta",
            [],
            |row| row.get(0),
        )?;

        let workflows_cached: u64 = db.query_row(
            "SELECT COUNT(DISTINCT workflow_id) FROM workflow_step_cache",
            [],
            |row| row.get(0),
        )?;

        Ok(CacheStats {
            total_entries,
            total_size_bytes: total_size,
            hot_entries,
            expired_entries,
            total_hits,
            total_misses,
            workflows_cached,
            hit_rate: if total_hits + total_misses > 0 {
                total_hits as f64 / (total_hits + total_misses) as f64
            } else {
                0.0
            },
        })
    }

    /// Get stats for a specific workflow
    pub fn workflow_stats(&self, workflow_id: &str) -> Result<Option<WorkflowCacheStats>> {
        let db = self.db.lock().unwrap();

        let meta: Option<(u64, u64, u64, u64)> = db
            .query_row(
                "SELECT total_entries, total_size_bytes, hit_count, miss_count
                 FROM workflow_cache_meta WHERE workflow_id = ?1",
                [workflow_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;

        match meta {
            Some((entries, size, hits, misses)) => Ok(Some(WorkflowCacheStats {
                workflow_id: workflow_id.to_string(),
                total_entries: entries,
                total_size_bytes: size,
                hit_count: hits,
                miss_count: misses,
                hit_rate: if hits + misses > 0 {
                    hits as f64 / (hits + misses) as f64
                } else {
                    0.0
                },
            })),
            None => Ok(None),
        }
    }

    /// Generate cache key from workflow+step+input
    fn make_cache_key(&self, workflow_id: &str, step_index: usize, input_hash: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(format!("{}:{}:{}", workflow_id, step_index, input_hash).as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Record cache hit
    fn record_hit(&self, db: &rusqlite::Connection, workflow_id: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        db.execute(
            "INSERT INTO workflow_cache_meta (workflow_id, hit_count, last_hit)
             VALUES (?1, 1, ?2)
             ON CONFLICT(workflow_id) DO UPDATE SET
                hit_count = hit_count + 1,
                last_hit = ?2",
            rusqlite::params![workflow_id, now],
        )?;
        Ok(())
    }

    /// Record cache miss
    fn record_miss(&self, db: &rusqlite::Connection, workflow_id: &str) -> Result<()> {
        let now = chrono::Utc::now().timestamp();
        db.execute(
            "INSERT INTO workflow_cache_meta (workflow_id, miss_count, last_miss)
             VALUES (?1, 1, ?2)
             ON CONFLICT(workflow_id) DO UPDATE SET
                miss_count = miss_count + 1,
                last_miss = ?2",
            rusqlite::params![workflow_id, now],
        )?;
        Ok(())
    }

    /// Update workflow metadata after put
    fn update_workflow_meta(&self, db: &rusqlite::Connection, workflow_id: &str) -> Result<()> {
        // Recalculate totals
        let (entries, size): (u64, u64) = db.query_row(
            "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0)
             FROM workflow_step_cache WHERE workflow_id = ?1",
            [workflow_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        db.execute(
            "INSERT INTO workflow_cache_meta (workflow_id, total_entries, total_size_bytes)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(workflow_id) DO UPDATE SET
                total_entries = ?2,
                total_size_bytes = ?3",
            rusqlite::params![workflow_id, entries, size],
        )?;

        Ok(())
    }

    /// Compress data using zstd
    fn compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        zstd::encode_all(std::io::Cursor::new(data), 3).context("Failed to compress data")
    }

    /// Decompress data using zstd
    fn decompress(&self, data: &[u8]) -> Result<Vec<u8>> {
        zstd::decode_all(std::io::Cursor::new(data)).context("Failed to decompress data")
    }
}

/// Cleanup result
#[derive(Debug, Clone)]
pub struct CleanupResult {
    pub entries_removed: usize,
    pub bytes_freed: u64,
}

/// Overall cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: u64,
    pub total_size_bytes: u64,
    pub hot_entries: u64,
    pub expired_entries: u64,
    pub total_hits: u64,
    pub total_misses: u64,
    pub workflows_cached: u64,
    pub hit_rate: f64,
}

/// Per-workflow cache statistics
#[derive(Debug, Clone)]
pub struct WorkflowCacheStats {
    pub workflow_id: String,
    pub total_entries: u64,
    pub total_size_bytes: u64,
    pub hit_count: u64,
    pub miss_count: u64,
    pub hit_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_workflow_cache_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkflowCacheConfig::default();
        let cache = WorkflowCache::new(temp_dir.path().to_path_buf(), config).await;
        assert!(cache.is_ok());
    }

    #[tokio::test]
    async fn test_put_and_get() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkflowCacheConfig::default();
        let cache = WorkflowCache::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        let test_data = b"test output data";
        cache
            .put("wf-001", 0, "input-hash-1", test_data, None)
            .unwrap();

        let result = cache.get("wf-001", 0, "input-hash-1").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), test_data);
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkflowCacheConfig::default();
        let cache = WorkflowCache::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        let result = cache.get("wf-001", 0, "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_invalidation() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkflowCacheConfig::default();
        let cache = WorkflowCache::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        cache.put("wf-001", 0, "input-1", b"data1", None).unwrap();
        cache.put("wf-001", 1, "input-2", b"data2", None).unwrap();

        let count = cache.invalidate_workflow("wf-001").unwrap();
        assert_eq!(count, 2);

        let result = cache.get("wf-001", 0, "input-1").unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_expiration() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkflowCacheConfig::default();
        let cache = WorkflowCache::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        // Insert with very short TTL
        cache
            .put("wf-001", 0, "input-1", b"data", Some(-1))
            .unwrap();

        // Should be expired immediately
        let result = cache.get("wf-001", 0, "input-1").unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_stats() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkflowCacheConfig::default();
        let cache = WorkflowCache::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        cache.put("wf-001", 0, "input-1", b"data", None).unwrap();

        let stats = cache.stats().unwrap();
        assert_eq!(stats.total_entries, 1);
        assert_eq!(stats.workflows_cached, 1);
    }
}
