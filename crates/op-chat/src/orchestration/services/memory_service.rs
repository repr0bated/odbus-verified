//! MemoryService gRPC service implementation.
//!
//! Backed by an in-memory `HashMap<String, MemoryEntry>` behind `RwLock`.

use std::pin::Pin;

use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tracing::info;

use crate::orchestration::proto::op_chat_orchestration::{
    memory_service_server::MemoryService, BulkForgetRequest, BulkOperationError,
    BulkOperationResponse, BulkRecallRequest, ForgetRequest, ForgetResponse, ListKeysRequest,
    ListKeysResponse, MemorySearchResult, RecallRequest, RecallResponse, RememberRequest,
    RememberResponse, SearchMemoryRequest, SearchMemoryResponse,
};

use super::{MemoryEntry, OrchestrationServer};

/// Simple glob-style pattern matching (supports `*` and `?`).
fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }

    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    glob_match_inner(&p, &t, 0, 0)
}

fn glob_match_inner(pattern: &[char], text: &[char], pi: usize, ti: usize) -> bool {
    if pi == pattern.len() {
        return ti == text.len();
    }

    match pattern[pi] {
        '*' => {
            // Try matching zero or more characters
            for i in ti..=text.len() {
                if glob_match_inner(pattern, text, pi + 1, i) {
                    return true;
                }
            }
            false
        }
        '?' => {
            if ti < text.len() {
                glob_match_inner(pattern, text, pi + 1, ti + 1)
            } else {
                false
            }
        }
        c => {
            if ti < text.len() && text[ti] == c {
                glob_match_inner(pattern, text, pi + 1, ti + 1)
            } else {
                false
            }
        }
    }
}

#[tonic::async_trait]
impl MemoryService for OrchestrationServer {
    /// Store a key-value pair in memory.
    async fn remember(
        &self,
        request: Request<RememberRequest>,
    ) -> Result<Response<RememberResponse>, Status> {
        let req = request.into_inner();
        let now_ms = chrono::Utc::now().timestamp_millis();

        let expires_at = if req.ttl_secs > 0 {
            Some(now_ms + req.ttl_secs * 1000)
        } else {
            None
        };

        let mut mem = self.memory.write().await;

        let was_overwrite = mem.contains_key(&req.key);

        if was_overwrite && !req.overwrite {
            return Ok(Response::new(RememberResponse {
                success: false,
                key: req.key,
                was_overwrite: false,
                expires_at_ms: 0,
            }));
        }

        let entry = MemoryEntry {
            key: req.key.clone(),
            value: req.value,
            tags: req.tags,
            created_at: if was_overwrite {
                mem.get(&req.key).map(|e| e.created_at).unwrap_or(now_ms)
            } else {
                now_ms
            },
            accessed_at: now_ms,
            access_count: if was_overwrite {
                mem.get(&req.key).map(|e| e.access_count).unwrap_or(0)
            } else {
                0
            },
            expires_at,
        };

        mem.insert(req.key.clone(), entry);

        Ok(Response::new(RememberResponse {
            success: true,
            key: req.key,
            was_overwrite,
            expires_at_ms: expires_at.unwrap_or(0),
        }))
    }

    /// Recall a value by key.
    async fn recall(
        &self,
        request: Request<RecallRequest>,
    ) -> Result<Response<RecallResponse>, Status> {
        let req = request.into_inner();
        let now_ms = chrono::Utc::now().timestamp_millis();

        let mut mem = self.memory.write().await;

        if let Some(entry) = mem.get_mut(&req.key) {
            // Check expiry
            if let Some(exp) = entry.expires_at {
                if now_ms > exp {
                    // Expired - remove it
                    mem.remove(&req.key);
                    return Ok(Response::new(RecallResponse {
                        found: false,
                        key: req.key,
                        ..Default::default()
                    }));
                }
            }

            // Update access metadata
            entry.access_count += 1;
            entry.accessed_at = now_ms;

            let resp = RecallResponse {
                found: true,
                key: entry.key.clone(),
                value: entry.value.clone(),
                tags: entry.tags.clone(),
                created_at_ms: entry.created_at,
                accessed_at_ms: entry.accessed_at,
                access_count: entry.access_count,
                expires_at_ms: entry.expires_at.unwrap_or(0),
            };

            Ok(Response::new(resp))
        } else {
            Ok(Response::new(RecallResponse {
                found: false,
                key: req.key,
                ..Default::default()
            }))
        }
    }

    /// Remove a key from memory.
    async fn forget(
        &self,
        request: Request<ForgetRequest>,
    ) -> Result<Response<ForgetResponse>, Status> {
        let req = request.into_inner();
        let existed = self.memory.write().await.remove(&req.key).is_some();

        Ok(Response::new(ForgetResponse {
            success: true,
            existed,
        }))
    }

    /// List keys matching a glob pattern and optional tag filter.
    async fn list(
        &self,
        request: Request<ListKeysRequest>,
    ) -> Result<Response<ListKeysResponse>, Status> {
        let req = request.into_inner();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mem = self.memory.read().await;

        let mut matching_keys: Vec<String> = mem
            .values()
            .filter(|entry| {
                // Skip expired entries
                if let Some(exp) = entry.expires_at {
                    if now_ms > exp {
                        return false;
                    }
                }

                // Glob pattern match
                if !req.pattern.is_empty() && !glob_match(&req.pattern, &entry.key) {
                    return false;
                }

                // Tag filter
                if !req.tag_filter.is_empty() && !entry.tags.contains(&req.tag_filter) {
                    return false;
                }

                true
            })
            .map(|e| e.key.clone())
            .collect();

        matching_keys.sort();

        let total_count = matching_keys.len() as i32;

        // Apply offset and limit
        let offset = req.offset as usize;
        let limit = if req.limit > 0 {
            req.limit as usize
        } else {
            usize::MAX
        };

        let paged: Vec<String> = matching_keys.into_iter().skip(offset).take(limit).collect();

        let has_more = (offset + paged.len()) < total_count as usize;

        Ok(Response::new(ListKeysResponse {
            keys: paged,
            total_count,
            has_more,
        }))
    }

    /// Search keys and values by substring, returning results with scores.
    async fn search(
        &self,
        request: Request<SearchMemoryRequest>,
    ) -> Result<Response<SearchMemoryResponse>, Status> {
        let req = request.into_inner();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let query_lower = req.query.to_lowercase();
        let mem = self.memory.read().await;

        let mut results: Vec<MemorySearchResult> = mem
            .values()
            .filter_map(|entry| {
                // Skip expired
                if let Some(exp) = entry.expires_at {
                    if now_ms > exp {
                        return None;
                    }
                }

                let key_lower = entry.key.to_lowercase();
                let val_lower = entry.value.to_lowercase();

                // Score based on matches
                let mut score: f32 = 0.0;

                // Exact key match
                if key_lower == query_lower {
                    score += 1.0;
                } else if key_lower.contains(&query_lower) {
                    // Key contains query - score based on ratio
                    score += 0.7 * (query_lower.len() as f32 / key_lower.len() as f32);
                }

                // Value contains query
                if val_lower.contains(&query_lower) {
                    score += 0.5 * (query_lower.len() as f32 / val_lower.len().max(1) as f32);
                }

                if score < req.min_score || score <= 0.0 {
                    return None;
                }

                Some(MemorySearchResult {
                    key: entry.key.clone(),
                    value: entry.value.clone(),
                    score,
                    tags: entry.tags.clone(),
                })
            })
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Apply limit
        if req.limit > 0 {
            results.truncate(req.limit as usize);
        }

        Ok(Response::new(SearchMemoryResponse { results }))
    }

    /// Client-streaming: collect all RememberRequests, insert each.
    async fn bulk_remember(
        &self,
        request: Request<tonic::Streaming<RememberRequest>>,
    ) -> Result<Response<BulkOperationResponse>, Status> {
        use tokio_stream::StreamExt;

        let mut stream = request.into_inner();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut success_count: i32 = 0;
        let mut failure_count: i32 = 0;
        let mut errors: Vec<BulkOperationError> = Vec::new();

        while let Some(result) = stream.next().await {
            match result {
                Ok(req) => {
                    let expires_at = if req.ttl_secs > 0 {
                        Some(now_ms + req.ttl_secs * 1000)
                    } else {
                        None
                    };

                    let mut mem = self.memory.write().await;
                    let was_existing = mem.contains_key(&req.key);

                    if was_existing && !req.overwrite {
                        failure_count += 1;
                        errors.push(BulkOperationError {
                            key: req.key,
                            error_code: "KEY_EXISTS".to_string(),
                            error_message: "Key already exists and overwrite is false".to_string(),
                        });
                        continue;
                    }

                    let entry = MemoryEntry {
                        key: req.key.clone(),
                        value: req.value,
                        tags: req.tags,
                        created_at: if was_existing {
                            mem.get(&req.key).map(|e| e.created_at).unwrap_or(now_ms)
                        } else {
                            now_ms
                        },
                        accessed_at: now_ms,
                        access_count: 0,
                        expires_at,
                    };

                    mem.insert(req.key, entry);
                    success_count += 1;
                }
                Err(e) => {
                    failure_count += 1;
                    errors.push(BulkOperationError {
                        key: String::new(),
                        error_code: "STREAM_ERROR".to_string(),
                        error_message: e.to_string(),
                    });
                }
            }
        }

        info!(
            success = success_count,
            failed = failure_count,
            "BulkRemember complete"
        );

        Ok(Response::new(BulkOperationResponse {
            success_count,
            failure_count,
            errors,
        }))
    }

    // -- server streaming: BulkRecall ----------------------------------------

    type BulkRecallStream =
        Pin<Box<dyn Stream<Item = Result<RecallResponse, Status>> + Send + 'static>>;

    /// Server-streaming: for each key in the request, yield a RecallResponse.
    async fn bulk_recall(
        &self,
        request: Request<BulkRecallRequest>,
    ) -> Result<Response<Self::BulkRecallStream>, Status> {
        let req = request.into_inner();
        let now_ms = chrono::Utc::now().timestamp_millis();

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<RecallResponse, Status>>(64);
        let memory = self.memory.clone();

        tokio::spawn(async move {
            for key in req.keys {
                let mut mem = memory.write().await;
                let resp = if let Some(entry) = mem.get_mut(&key) {
                    // Check expiry
                    if let Some(exp) = entry.expires_at {
                        if now_ms > exp {
                            mem.remove(&key);
                            RecallResponse {
                                found: false,
                                key,
                                ..Default::default()
                            }
                        } else {
                            entry.access_count += 1;
                            entry.accessed_at = now_ms;
                            RecallResponse {
                                found: true,
                                key: entry.key.clone(),
                                value: entry.value.clone(),
                                tags: entry.tags.clone(),
                                created_at_ms: entry.created_at,
                                accessed_at_ms: entry.accessed_at,
                                access_count: entry.access_count,
                                expires_at_ms: entry.expires_at.unwrap_or(0),
                            }
                        }
                    } else {
                        entry.access_count += 1;
                        entry.accessed_at = now_ms;
                        RecallResponse {
                            found: true,
                            key: entry.key.clone(),
                            value: entry.value.clone(),
                            tags: entry.tags.clone(),
                            created_at_ms: entry.created_at,
                            accessed_at_ms: entry.accessed_at,
                            access_count: entry.access_count,
                            expires_at_ms: 0,
                        }
                    }
                } else {
                    RecallResponse {
                        found: false,
                        key,
                        ..Default::default()
                    }
                };

                if tx.send(Ok(resp)).await.is_err() {
                    break;
                }
            }
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(stream) as Self::BulkRecallStream))
    }

    /// Batch delete: remove all specified keys, return counts.
    async fn bulk_forget(
        &self,
        request: Request<BulkForgetRequest>,
    ) -> Result<Response<BulkOperationResponse>, Status> {
        let req = request.into_inner();
        let mut mem = self.memory.write().await;

        let mut success_count: i32 = 0;
        let mut failure_count: i32 = 0;
        let errors: Vec<BulkOperationError> = Vec::new();

        for key in &req.keys {
            if mem.remove(key).is_some() {
                success_count += 1;
            } else {
                failure_count += 1;
            }
        }

        info!(
            success = success_count,
            not_found = failure_count,
            "BulkForget complete"
        );

        Ok(Response::new(BulkOperationResponse {
            success_count,
            failure_count,
            errors,
        }))
    }
}
