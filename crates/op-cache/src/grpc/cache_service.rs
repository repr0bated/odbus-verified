//! Cache service implementation
//!
//! Provides workstack step caching with TTL and compression.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use tracing::debug;

use super::proto::{
    cache_service_server::CacheService, CacheStats, CleanupRequest, CleanupResponse, Empty,
    GetStepRequest, GetStepResponse, GetWorkstackStatsRequest, InvalidateStepRequest,
    InvalidateStepResponse, InvalidateWorkstackRequest, InvalidateWorkstackResponse,
    PutStepRequest, PutStepResponse, WorkstackCacheStats,
};

/// Cached step entry
struct CachedEntry {
    output: Vec<u8>,
    created_at: u64,
    expires_at: u64,
    access_count: u32,
    size_bytes: u64,
    #[allow(dead_code)]
    compressed: bool,
}

/// Per-workstack statistics
#[derive(Default)]
struct WorkstackStats {
    hit_count: AtomicU64,
    miss_count: AtomicU64,
}

pub struct CacheServiceImpl {
    entries: Arc<RwLock<HashMap<String, CachedEntry>>>,
    workstack_stats: Arc<RwLock<HashMap<String, WorkstackStats>>>,
    default_ttl_secs: i64,
    total_hits: AtomicU64,
    total_misses: AtomicU64,
}

impl CacheServiceImpl {
    pub fn new() -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            workstack_stats: Arc::new(RwLock::new(HashMap::new())),
            default_ttl_secs: 3600,
            total_hits: AtomicU64::new(0),
            total_misses: AtomicU64::new(0),
        }
    }

    pub fn with_ttl(default_ttl_secs: i64) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            workstack_stats: Arc::new(RwLock::new(HashMap::new())),
            default_ttl_secs,
            total_hits: AtomicU64::new(0),
            total_misses: AtomicU64::new(0),
        }
    }

    fn make_cache_key(workstack_id: &str, step_index: u32, input_hash: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(format!("{}:{}:{}", workstack_id, step_index, input_hash).as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn now_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    /// Internal method for orchestrator to get cached step
    pub async fn get_step_internal(
        &self,
        workstack_id: &str,
        step_index: u32,
        input_hash: &str,
    ) -> Option<Vec<u8>> {
        let cache_key = Self::make_cache_key(workstack_id, step_index, input_hash);
        let now = Self::now_timestamp();

        let entries = self.entries.read().await;
        if let Some(entry) = entries.get(&cache_key) {
            if now <= entry.expires_at {
                self.total_hits.fetch_add(1, Ordering::Relaxed);
                self.record_hit(workstack_id).await;
                return Some(entry.output.clone());
            }
        }

        self.total_misses.fetch_add(1, Ordering::Relaxed);
        self.record_miss(workstack_id).await;
        None
    }

    /// Internal method for orchestrator to store cached step
    pub async fn put_step_internal(
        &self,
        workstack_id: &str,
        step_index: u32,
        input_hash: &str,
        output: &[u8],
    ) {
        let cache_key = Self::make_cache_key(workstack_id, step_index, input_hash);
        let now = Self::now_timestamp();

        let entry = CachedEntry {
            output: output.to_vec(),
            created_at: now,
            expires_at: now + self.default_ttl_secs as u64,
            access_count: 1,
            size_bytes: output.len() as u64,
            compressed: false,
        };

        let mut entries = self.entries.write().await;
        entries.insert(cache_key, entry);
    }

    /// Get cache statistics (internal)
    pub async fn get_stats_internal(&self) -> CacheStats {
        let entries = self.entries.read().await;
        let now = Self::now_timestamp();

        let total_entries = entries.len() as u64;
        let total_size: u64 = entries.values().map(|e| e.size_bytes).sum();
        let hot_entries = entries
            .values()
            .filter(|e| now.saturating_sub(e.created_at) < 600)
            .count() as u64;
        let expired_entries = entries.values().filter(|e| now > e.expires_at).count() as u64;

        let total_hits = self.total_hits.load(Ordering::Relaxed);
        let total_misses = self.total_misses.load(Ordering::Relaxed);

        let workstack_stats = self.workstack_stats.read().await;
        let workstacks_cached = workstack_stats.len() as u64;

        let hit_rate = if total_hits + total_misses > 0 {
            total_hits as f64 / (total_hits + total_misses) as f64
        } else {
            0.0
        };

        CacheStats {
            total_entries,
            total_size_bytes: total_size,
            hot_entries,
            expired_entries,
            total_hits,
            total_misses,
            workstacks_cached,
            hit_rate,
        }
    }

    async fn record_hit(&self, workstack_id: &str) {
        let mut stats = self.workstack_stats.write().await;
        stats
            .entry(workstack_id.to_string())
            .or_default()
            .hit_count
            .fetch_add(1, Ordering::Relaxed);
    }

    async fn record_miss(&self, workstack_id: &str) {
        let mut stats = self.workstack_stats.write().await;
        stats
            .entry(workstack_id.to_string())
            .or_default()
            .miss_count
            .fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for CacheServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl CacheService for CacheServiceImpl {
    async fn get_step(
        &self,
        request: Request<GetStepRequest>,
    ) -> Result<Response<GetStepResponse>, Status> {
        let req = request.into_inner();
        let cache_key = Self::make_cache_key(&req.workstack_id, req.step_index, &req.input_hash);
        let now = Self::now_timestamp();

        let mut entries = self.entries.write().await;

        if let Some(entry) = entries.get_mut(&cache_key) {
            if now <= entry.expires_at {
                entry.access_count += 1;
                self.total_hits.fetch_add(1, Ordering::Relaxed);
                self.record_hit(&req.workstack_id).await;

                return Ok(Response::new(GetStepResponse {
                    found: true,
                    output: entry.output.clone(),
                    created_at: entry.created_at,
                    expires_at: entry.expires_at,
                    access_count: entry.access_count,
                }));
            }
        }

        self.total_misses.fetch_add(1, Ordering::Relaxed);
        self.record_miss(&req.workstack_id).await;

        Ok(Response::new(GetStepResponse {
            found: false,
            output: Vec::new(),
            created_at: 0,
            expires_at: 0,
            access_count: 0,
        }))
    }

    async fn put_step(
        &self,
        request: Request<PutStepRequest>,
    ) -> Result<Response<PutStepResponse>, Status> {
        let req = request.into_inner();
        let cache_key = Self::make_cache_key(&req.workstack_id, req.step_index, &req.input_hash);
        let now = Self::now_timestamp();

        let ttl = if req.ttl_seconds > 0 {
            req.ttl_seconds as u64
        } else {
            self.default_ttl_secs as u64
        };

        let size_bytes = req.output.len() as u64;

        let entry = CachedEntry {
            output: req.output,
            created_at: now,
            expires_at: now + ttl,
            access_count: 1,
            size_bytes,
            compressed: false, // TODO: add compression
        };

        let mut entries = self.entries.write().await;
        entries.insert(cache_key.clone(), entry);

        debug!(
            "Cached step {} index {} ({} bytes)",
            req.workstack_id, req.step_index, size_bytes
        );

        Ok(Response::new(PutStepResponse {
            success: true,
            cache_key,
            size_bytes,
            compressed: false,
        }))
    }

    async fn invalidate_workstack(
        &self,
        request: Request<InvalidateWorkstackRequest>,
    ) -> Result<Response<InvalidateWorkstackResponse>, Status> {
        let req = request.into_inner();
        let prefix = format!("{}:", req.workstack_id);

        let mut entries = self.entries.write().await;
        let before = entries.len();

        // This is inefficient - in production, maintain a workstack->keys index
        entries.retain(|k, _| !k.starts_with(&prefix));

        let removed = (before - entries.len()) as u32;

        Ok(Response::new(InvalidateWorkstackResponse {
            entries_removed: removed,
        }))
    }

    async fn invalidate_step(
        &self,
        request: Request<InvalidateStepRequest>,
    ) -> Result<Response<InvalidateStepResponse>, Status> {
        let req = request.into_inner();
        let prefix = format!("{}:{}:", req.workstack_id, req.step_index);

        let mut entries = self.entries.write().await;
        let before = entries.len();
        entries.retain(|k, _| !k.starts_with(&prefix));
        let removed = (before - entries.len()) as u32;

        Ok(Response::new(InvalidateStepResponse {
            entries_removed: removed,
        }))
    }

    async fn cleanup(
        &self,
        request: Request<CleanupRequest>,
    ) -> Result<Response<CleanupResponse>, Status> {
        let req = request.into_inner();
        let now = Self::now_timestamp();

        let mut entries = self.entries.write().await;
        let before_len = entries.len();
        let before_size: u64 = entries.values().map(|e| e.size_bytes).sum();

        if req.expired_only {
            entries.retain(|_, e| now <= e.expires_at);
        } else if req.max_age_seconds > 0 {
            let cutoff = now.saturating_sub(req.max_age_seconds);
            entries.retain(|_, e| e.created_at >= cutoff);
        } else {
            entries.retain(|_, e| now <= e.expires_at);
        }

        let after_size: u64 = entries.values().map(|e| e.size_bytes).sum();
        let removed = (before_len - entries.len()) as u32;
        let bytes_freed = before_size.saturating_sub(after_size);

        Ok(Response::new(CleanupResponse {
            entries_removed: removed,
            bytes_freed,
        }))
    }

    async fn get_stats(&self, _request: Request<Empty>) -> Result<Response<CacheStats>, Status> {
        Ok(Response::new(self.get_stats_internal().await))
    }

    async fn get_workstack_stats(
        &self,
        request: Request<GetWorkstackStatsRequest>,
    ) -> Result<Response<WorkstackCacheStats>, Status> {
        let req = request.into_inner();
        let entries = self.entries.read().await;
        let stats = self.workstack_stats.read().await;

        // Count entries for this workstack (inefficient without index)
        let prefix = format!("{}:", req.workstack_id);
        let workstack_entries: Vec<_> = entries
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix))
            .collect();

        let total_entries = workstack_entries.len() as u64;
        let total_size: u64 = workstack_entries.iter().map(|(_, e)| e.size_bytes).sum();

        let (hit_count, miss_count) = if let Some(ws_stats) = stats.get(&req.workstack_id) {
            (
                ws_stats.hit_count.load(Ordering::Relaxed),
                ws_stats.miss_count.load(Ordering::Relaxed),
            )
        } else {
            (0, 0)
        };

        let hit_rate = if hit_count + miss_count > 0 {
            hit_count as f64 / (hit_count + miss_count) as f64
        } else {
            0.0
        };

        Ok(Response::new(WorkstackCacheStats {
            workstack_id: req.workstack_id,
            total_entries,
            total_size_bytes: total_size,
            hit_count,
            miss_count,
            hit_rate,
        }))
    }
}
