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

const MAX_ENTRY_BYTES: usize = 16 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_ENTRIES: usize = 10_000;
const MAX_TTL_SECS: u64 = 30 * 24 * 60 * 60;

/// Cached step entry
struct CachedEntry {
    workstack_id: String,
    step_index: u32,
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
        if let Err(error) = self
            .store_entry(
                workstack_id,
                step_index,
                input_hash,
                output.to_vec(),
                self.default_ttl_secs.max(1) as u64,
            )
            .await
        {
            debug!(%error, "Skipped cache entry that exceeded cache limits");
        }
    }

    async fn store_entry(
        &self,
        workstack_id: &str,
        step_index: u32,
        input_hash: &str,
        output: Vec<u8>,
        ttl: u64,
    ) -> Result<(String, u64), Status> {
        if output.len() > MAX_ENTRY_BYTES {
            return Err(Status::resource_exhausted(format!(
                "cache entry exceeds the {} byte limit",
                MAX_ENTRY_BYTES
            )));
        }
        if ttl > MAX_TTL_SECS {
            return Err(Status::invalid_argument(format!(
                "cache TTL exceeds the {} second limit",
                MAX_TTL_SECS
            )));
        }

        let cache_key = Self::make_cache_key(workstack_id, step_index, input_hash);
        let now = Self::now_timestamp();
        let size_bytes = output.len() as u64;
        let mut entries = self.entries.write().await;
        let replaced_size = entries
            .get(&cache_key)
            .map(|entry| entry.size_bytes)
            .unwrap_or(0);
        let current_size = entries
            .values()
            .map(|entry| entry.size_bytes)
            .sum::<u64>()
            .saturating_sub(replaced_size);
        if current_size.saturating_add(size_bytes) > MAX_TOTAL_BYTES {
            return Err(Status::resource_exhausted(format!(
                "cache exceeds the {} byte cumulative limit",
                MAX_TOTAL_BYTES
            )));
        }
        if !entries.contains_key(&cache_key) && entries.len() >= MAX_ENTRIES {
            return Err(Status::resource_exhausted(format!(
                "cache exceeds the {} entry limit",
                MAX_ENTRIES
            )));
        }

        entries.insert(
            cache_key.clone(),
            CachedEntry {
                workstack_id: workstack_id.to_string(),
                step_index,
                output,
                created_at: now,
                expires_at: now.saturating_add(ttl),
                access_count: 1,
                size_bytes,
                compressed: false,
            },
        );
        Ok((cache_key, size_bytes))
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
        let ttl = if req.ttl_seconds > 0 {
            req.ttl_seconds as u64
        } else {
            self.default_ttl_secs.max(1) as u64
        };
        let (cache_key, size_bytes) = self
            .store_entry(
                &req.workstack_id,
                req.step_index,
                &req.input_hash,
                req.output,
                ttl,
            )
            .await?;

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
        let mut entries = self.entries.write().await;
        let before = entries.len();
        entries.retain(|_, entry| entry.workstack_id != req.workstack_id);

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
        let mut entries = self.entries.write().await;
        let before = entries.len();
        entries.retain(|_, entry| {
            entry.workstack_id != req.workstack_id || entry.step_index != req.step_index
        });
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

        let workstack_entries: Vec<_> = entries
            .iter()
            .filter(|(_, entry)| entry.workstack_id == req.workstack_id)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn oversized_output_is_rejected_without_insertion() {
        let service = CacheServiceImpl::new();
        let result = service
            .put_step(Request::new(PutStepRequest {
                workstack_id: "ws".into(),
                step_index: 1,
                input_hash: "hash".into(),
                output: vec![0; MAX_ENTRY_BYTES + 1],
                ttl_seconds: 1,
            }))
            .await;
        assert_eq!(result.unwrap_err().code(), tonic::Code::ResourceExhausted);
        assert!(service.entries.read().await.is_empty());
    }

    #[tokio::test]
    async fn workstack_and_step_invalidation_match_hashed_entries() {
        let service = CacheServiceImpl::new();
        service.put_step_internal("one", 0, "a", b"first").await;
        service.put_step_internal("one", 1, "b", b"second").await;
        service.put_step_internal("two", 0, "c", b"third").await;

        let response = service
            .invalidate_step(Request::new(InvalidateStepRequest {
                workstack_id: "one".into(),
                step_index: 0,
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(response.entries_removed, 1);

        let response = service
            .invalidate_workstack(Request::new(InvalidateWorkstackRequest {
                workstack_id: "one".into(),
            }))
            .await
            .unwrap()
            .into_inner();
        assert_eq!(response.entries_removed, 1);
        assert_eq!(service.entries.read().await.len(), 1);
    }
}
