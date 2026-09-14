//! Workstack intermediate result caching
//!
//! Caches intermediate results from workstack steps
//! to avoid redundant computation.

use anyhow::{Context, Result};
use rusqlite::OptionalExtension;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::{debug, info};

/// Configuration for workstack caching
#[derive(Debug, Clone)]
pub struct WorkstackCacheConfig {
    /// Default TTL in seconds (default: 1 hour)
    pub default_ttl_secs: i64,
    /// Maximum cache size in bytes (default: 1GB)
    pub max_size_bytes: u64,
    /// Enable compression (default: true)
    pub compress: bool,
    /// Hot entry threshold in seconds (default: 10 minutes)
    pub hot_threshold_secs: i64,
}

impl Default for WorkstackCacheConfig {
    fn default() -> Self {
        Self {
            default_ttl_secs: 3600,
            max_size_bytes: 1024 * 1024 * 1024,
            compress: true,
            hot_threshold_secs: 600,
        }
    }
}

pub struct WorkstackCache {
    cache_dir: PathBuf,
    db: Mutex<rusqlite::Connection>,
    config: WorkstackCacheConfig,
}

impl WorkstackCache {
    /// Create new workstack cache
    pub async fn new(cache_dir: PathBuf, config: WorkstackCacheConfig) -> Result<Self> {
        let workstacks_dir = cache_dir.join("workstacks");
        let data_dir = workstacks_dir.join("data");

        tokio::fs::create_dir_all(&data_dir).await?;

        let db_path = workstacks_dir.join("cache.db");
        let db = rusqlite::Connection::open(&db_path)
            .context("Failed to open workstack cache database")?;

        db.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS step_cache (
                cache_key TEXT PRIMARY KEY,
                workstack_id TEXT NOT NULL,
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

            CREATE TABLE IF NOT EXISTS workstack_meta (
                workstack_id TEXT PRIMARY KEY,
                total_entries INTEGER DEFAULT 0,
                total_size_bytes INTEGER DEFAULT 0,
                hit_count INTEGER DEFAULT 0,
                miss_count INTEGER DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_cache_workstack ON step_cache(workstack_id);
            CREATE INDEX IF NOT EXISTS idx_cache_expires ON step_cache(expires_at);
            CREATE INDEX IF NOT EXISTS idx_cache_accessed ON step_cache(last_accessed DESC);
            "#,
        )?;

        info!("Workstack cache initialized at {:?}", db_path);

        Ok(Self {
            cache_dir: workstacks_dir,
            db: Mutex::new(db),
            config,
        })
    }

    /// Get cached result for a workstack step
    pub fn get(
        &self,
        workstack_id: &str,
        step_index: usize,
        input_hash: &str,
    ) -> Result<Option<Vec<u8>>> {
        let cache_key = self.make_cache_key(workstack_id, step_index, input_hash);
        let now = chrono::Utc::now().timestamp();

        let db = self.db.lock().unwrap();

        let entry: Option<(String, i64, bool)> = db
            .query_row(
                "SELECT output_file, expires_at, compressed
                 FROM step_cache WHERE cache_key = ?1",
                [&cache_key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;

        let (output_file, expires_at, compressed) = match entry {
            Some(e) => e,
            None => {
                self.record_miss(&db, workstack_id)?;
                return Ok(None);
            }
        };

        // Check expiration
        if now > expires_at {
            debug!("Cache entry expired: {}", cache_key);
            self.invalidate_entry(&cache_key)?;
            return Ok(None);
        }

        // Update access stats
        db.execute(
            "UPDATE step_cache SET access_count = access_count + 1, last_accessed = ?1
             WHERE cache_key = ?2",
            rusqlite::params![now, cache_key],
        )?;

        self.record_hit(&db, workstack_id)?;

        // Read data
        let data_path = self.cache_dir.join("data").join(&output_file);
        let data = std::fs::read(&data_path)
            .context(format!("Failed to read cached data: {:?}", data_path))?;

        let output = if compressed {
            self.decompress(&data)?
        } else {
            data
        };

        debug!("Cache hit: {} (key: {})", workstack_id, cache_key);
        Ok(Some(output))
    }

    /// Store result in cache
    pub fn put(
        &self,
        workstack_id: &str,
        step_index: usize,
        input_hash: &str,
        output: &[u8],
        ttl_secs: Option<i64>,
    ) -> Result<()> {
        let cache_key = self.make_cache_key(workstack_id, step_index, input_hash);
        let now = chrono::Utc::now().timestamp();
        let ttl = ttl_secs.unwrap_or(self.config.default_ttl_secs);
        let expires_at = now + ttl;

        // Compress if beneficial
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

        // Write to file
        let output_file = format!("{}.cache", cache_key);
        let data_path = self.cache_dir.join("data").join(&output_file);
        std::fs::write(&data_path, &data)?;

        // Update database
        let db = self.db.lock().unwrap();

        db.execute(
            "INSERT INTO step_cache
             (cache_key, workstack_id, step_index, input_hash, output_file,
              created_at, expires_at, last_accessed, size_bytes, compressed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(cache_key) DO UPDATE SET
                output_file = ?5, expires_at = ?7, last_accessed = ?8,
                size_bytes = ?9, compressed = ?10, access_count = access_count + 1",
            rusqlite::params![
                cache_key,
                workstack_id,
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

        self.update_workstack_meta(&db, workstack_id)?;

        debug!(
            "Cached workstack {} step {} ({} bytes, compressed: {})",
            workstack_id, step_index, size_bytes, compressed
        );

        Ok(())
    }

    /// Invalidate a specific entry
    fn invalidate_entry(&self, cache_key: &str) -> Result<()> {
        let db = self.db.lock().unwrap();

        let output_file: Option<String> = db
            .query_row(
                "SELECT output_file FROM step_cache WHERE cache_key = ?1",
                [cache_key],
                |row| row.get(0),
            )
            .optional()?;

        db.execute("DELETE FROM step_cache WHERE cache_key = ?1", [cache_key])?;

        if let Some(file) = output_file {
            let _ = std::fs::remove_file(self.cache_dir.join("data").join(&file));
        }

        Ok(())
    }

    /// Invalidate all entries for a workstack
    pub fn invalidate_workstack(&self, workstack_id: &str) -> Result<usize> {
        let db = self.db.lock().unwrap();

        let mut stmt = db.prepare("SELECT output_file FROM step_cache WHERE workstack_id = ?1")?;

        let files: Vec<String> = stmt
            .query_map([workstack_id], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let count = files.len();

        db.execute(
            "DELETE FROM step_cache WHERE workstack_id = ?1",
            [workstack_id],
        )?;

        db.execute(
            "DELETE FROM workstack_meta WHERE workstack_id = ?1",
            [workstack_id],
        )?;

        drop(stmt);

        for file in files {
            let _ = std::fs::remove_file(self.cache_dir.join("data").join(&file));
        }

        info!(
            "Invalidated {} cache entries for workstack {}",
            count, workstack_id
        );
        Ok(count)
    }

    /// Cleanup expired entries
    pub fn cleanup_expired(&self) -> Result<CleanupResult> {
        let now = chrono::Utc::now().timestamp();
        let db = self.db.lock().unwrap();

        let mut stmt =
            db.prepare("SELECT output_file, size_bytes FROM step_cache WHERE expires_at < ?1")?;

        let expired: Vec<(String, u64)> = stmt
            .query_map([now], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;

        let count = expired.len();
        let bytes_freed: u64 = expired.iter().map(|(_, size)| size).sum();

        db.execute("DELETE FROM step_cache WHERE expires_at < ?1", [now])?;

        drop(stmt);

        for (file, _) in expired {
            let _ = std::fs::remove_file(self.cache_dir.join("data").join(&file));
        }

        if count > 0 {
            info!(
                "Cleaned up {} expired entries ({} bytes)",
                count, bytes_freed
            );
        }

        Ok(CleanupResult {
            entries_removed: count,
            bytes_freed,
        })
    }

    /// Get cache statistics
    pub fn stats(&self) -> Result<CacheStats> {
        let db = self.db.lock().unwrap();

        let total_entries: u64 =
            db.query_row("SELECT COUNT(*) FROM step_cache", [], |row| row.get(0))?;

        let total_size: u64 = db.query_row(
            "SELECT COALESCE(SUM(size_bytes), 0) FROM step_cache",
            [],
            |row| row.get(0),
        )?;

        let hot_threshold = chrono::Utc::now().timestamp() - self.config.hot_threshold_secs;
        let hot_entries: u64 = db.query_row(
            "SELECT COUNT(*) FROM step_cache WHERE last_accessed > ?1",
            [hot_threshold],
            |row| row.get(0),
        )?;

        let total_hits: u64 = db.query_row(
            "SELECT COALESCE(SUM(hit_count), 0) FROM workstack_meta",
            [],
            |row| row.get(0),
        )?;

        let total_misses: u64 = db.query_row(
            "SELECT COALESCE(SUM(miss_count), 0) FROM workstack_meta",
            [],
            |row| row.get(0),
        )?;

        let workstacks_cached: u64 = db.query_row(
            "SELECT COUNT(DISTINCT workstack_id) FROM step_cache",
            [],
            |row| row.get(0),
        )?;

        Ok(CacheStats {
            total_entries,
            total_size_bytes: total_size,
            hot_entries,
            total_hits,
            total_misses,
            workstacks_cached,
            hit_rate: if total_hits + total_misses > 0 {
                total_hits as f64 / (total_hits + total_misses) as f64
            } else {
                0.0
            },
        })
    }

    fn make_cache_key(&self, workstack_id: &str, step_index: usize, input_hash: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(format!("{}:{}:{}", workstack_id, step_index, input_hash).as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn record_hit(&self, db: &rusqlite::Connection, workstack_id: &str) -> Result<()> {
        db.execute(
            "INSERT INTO workstack_meta (workstack_id, hit_count) VALUES (?1, 1)
             ON CONFLICT(workstack_id) DO UPDATE SET hit_count = hit_count + 1",
            [workstack_id],
        )?;
        Ok(())
    }

    fn record_miss(&self, db: &rusqlite::Connection, workstack_id: &str) -> Result<()> {
        db.execute(
            "INSERT INTO workstack_meta (workstack_id, miss_count) VALUES (?1, 1)
             ON CONFLICT(workstack_id) DO UPDATE SET miss_count = miss_count + 1",
            [workstack_id],
        )?;
        Ok(())
    }

    fn update_workstack_meta(&self, db: &rusqlite::Connection, workstack_id: &str) -> Result<()> {
        let (entries, size): (u64, u64) = db.query_row(
            "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0)
             FROM step_cache WHERE workstack_id = ?1",
            [workstack_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        db.execute(
            "INSERT INTO workstack_meta (workstack_id, total_entries, total_size_bytes)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(workstack_id) DO UPDATE SET
                total_entries = ?2, total_size_bytes = ?3",
            rusqlite::params![workstack_id, entries, size],
        )?;

        Ok(())
    }

    fn compress(&self, data: &[u8]) -> Result<Vec<u8>> {
        zstd::encode_all(std::io::Cursor::new(data), 3).context("Compression failed")
    }

    fn decompress(&self, data: &[u8]) -> Result<Vec<u8>> {
        zstd::decode_all(std::io::Cursor::new(data)).context("Decompression failed")
    }
}

/// Cleanup result
#[derive(Debug, Clone)]
pub struct CleanupResult {
    pub entries_removed: usize,
    pub bytes_freed: u64,
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: u64,
    pub total_size_bytes: u64,
    pub hot_entries: u64,
    pub total_hits: u64,
    pub total_misses: u64,
    pub workstacks_cached: u64,
    pub hit_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_workstack_cache_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkstackCacheConfig::default();
        let cache = WorkstackCache::new(temp_dir.path().to_path_buf(), config).await;
        assert!(cache.is_ok());
    }

    #[tokio::test]
    async fn test_put_and_get() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkstackCacheConfig::default();
        let cache = WorkstackCache::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        let test_data = b"test output";
        cache
            .put("ws-001", 0, "input-hash", test_data, None)
            .unwrap();

        let result = cache.get("ws-001", 0, "input-hash").unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap(), test_data);
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkstackCacheConfig::default();
        let cache = WorkstackCache::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        let result = cache.get("ws-001", 0, "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_invalidation() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkstackCacheConfig::default();
        let cache = WorkstackCache::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        cache.put("ws-001", 0, "hash1", b"data1", None).unwrap();
        cache.put("ws-001", 1, "hash2", b"data2", None).unwrap();

        let count = cache.invalidate_workstack("ws-001").unwrap();
        assert_eq!(count, 2);

        let result = cache.get("ws-001", 0, "hash1").unwrap();
        assert!(result.is_none());
    }
}
