//! ContextManagerService gRPC implementation
//!
//! Manages persistent context across sessions with versioning,
//! tagging, export/import, and merge support.

use std::pin::Pin;

use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{debug, info, warn};

use crate::orchestration::proto::op_chat_orchestration::{
    context_manager_service_server::ContextManagerService, ContextInfo, ContextType,
    DeleteContextRequest, DeleteContextResponse, ExportChunk, ExportContextRequest, ImportChunk,
    ImportContextResponse, ListContextsRequest, ListContextsResponse, LoadContextRequest,
    LoadContextResponse, MergeContextsRequest, MergeContextsResponse, MergeStrategy,
    SaveContextRequest, SaveContextResponse,
};

use super::{ContextEntry, OrchestrationServer};

/// Helper to parse version string "v3" -> 3, or return 1 if empty/unparseable
fn parse_version(v: &str) -> u64 {
    if v.is_empty() {
        return 0;
    }
    v.strip_prefix('v').unwrap_or(v).parse().unwrap_or(0)
}

/// Helper to increment a version string
fn increment_version(v: &str) -> String {
    let n = parse_version(v);
    format!("v{}", n + 1)
}

#[tonic::async_trait]
impl ContextManagerService for OrchestrationServer {
    type ExportStream =
        Pin<Box<dyn tokio_stream::Stream<Item = Result<ExportChunk, Status>> + Send + 'static>>;

    async fn save(
        &self,
        request: Request<SaveContextRequest>,
    ) -> Result<Response<SaveContextResponse>, Status> {
        let req = request.into_inner();
        debug!(name = %req.name, "Saving context");

        let mut contexts = self.contexts.write().await;
        let now = chrono::Utc::now().timestamp_millis();

        if let Some(existing) = contexts.get_mut(&req.name) {
            if !req.overwrite {
                return Ok(Response::new(SaveContextResponse {
                    success: false,
                    name: req.name,
                    size_bytes: 0,
                    version: String::new(),
                }));
            }

            // Update in place, increment version
            let new_version = increment_version(&existing.version);
            existing.content = req.content.clone();
            existing.tags = req.tags;
            existing.context_type = req.context_type;
            existing.updated_at = now;
            existing.version = new_version.clone();

            let size = existing.content.len() as i64;
            info!(name = %req.name, version = %new_version, "Context updated");
            Ok(Response::new(SaveContextResponse {
                success: true,
                name: req.name,
                size_bytes: size,
                version: new_version,
            }))
        } else {
            let size = req.content.len() as i64;
            let entry = ContextEntry {
                name: req.name.clone(),
                content: req.content,
                tags: req.tags,
                context_type: req.context_type,
                created_at: now,
                updated_at: now,
                version: "v1".to_string(),
            };
            contexts.insert(req.name.clone(), entry);

            info!(name = %req.name, "Context created");
            Ok(Response::new(SaveContextResponse {
                success: true,
                name: req.name,
                size_bytes: size,
                version: "v1".to_string(),
            }))
        }
    }

    async fn load(
        &self,
        request: Request<LoadContextRequest>,
    ) -> Result<Response<LoadContextResponse>, Status> {
        let req = request.into_inner();
        debug!(name = %req.name, version = %req.version, "Loading context");

        let contexts = self.contexts.read().await;

        match contexts.get(&req.name) {
            Some(entry) => {
                // If a specific version was requested and it does not match, report not found.
                // (Full version history would require storing snapshots; here we only have
                //  the current version.)
                if !req.version.is_empty() && req.version != entry.version {
                    return Ok(Response::new(LoadContextResponse {
                        found: false,
                        name: req.name,
                        ..Default::default()
                    }));
                }

                Ok(Response::new(LoadContextResponse {
                    found: true,
                    name: entry.name.clone(),
                    content: entry.content.clone(),
                    tags: entry.tags.clone(),
                    context_type: entry.context_type,
                    created_at_ms: entry.created_at,
                    updated_at_ms: entry.updated_at,
                    version: entry.version.clone(),
                }))
            }
            None => Ok(Response::new(LoadContextResponse {
                found: false,
                name: req.name,
                ..Default::default()
            })),
        }
    }

    async fn list(
        &self,
        request: Request<ListContextsRequest>,
    ) -> Result<Response<ListContextsResponse>, Status> {
        let req = request.into_inner();
        debug!(tag = %req.tag_filter, "Listing contexts");

        let contexts = self.contexts.read().await;

        let mut filtered: Vec<ContextInfo> = contexts
            .values()
            .filter(|entry| {
                // Filter by tag
                if !req.tag_filter.is_empty() && !entry.tags.iter().any(|t| t == &req.tag_filter) {
                    return false;
                }

                // Filter by type (0 = GENERIC means no filter)
                if req.type_filter != 0 && entry.context_type != req.type_filter {
                    return false;
                }

                true
            })
            .map(|entry| ContextInfo {
                name: entry.name.clone(),
                size_bytes: entry.content.len() as i64,
                tags: entry.tags.clone(),
                context_type: entry.context_type,
                updated_at_ms: entry.updated_at,
                version: entry.version.clone(),
            })
            .collect();

        // Sort by updated_at descending
        filtered.sort_by_key(|b| std::cmp::Reverse(b.updated_at_ms));

        let total_count = filtered.len() as i32;

        // Apply offset and limit
        let offset = req.offset.max(0) as usize;
        if offset > 0 && offset < filtered.len() {
            filtered = filtered[offset..].to_vec();
        } else if offset >= filtered.len() && !filtered.is_empty() {
            filtered.clear();
        }

        if req.limit > 0 {
            filtered.truncate(req.limit as usize);
        }

        Ok(Response::new(ListContextsResponse {
            contexts: filtered,
            total_count,
        }))
    }

    async fn delete(
        &self,
        request: Request<DeleteContextRequest>,
    ) -> Result<Response<DeleteContextResponse>, Status> {
        let req = request.into_inner();
        debug!(name = %req.name, all_versions = req.all_versions, "Deleting context");

        let mut contexts = self.contexts.write().await;

        if contexts.remove(&req.name).is_some() {
            info!(name = %req.name, "Context deleted");
            Ok(Response::new(DeleteContextResponse {
                success: true,
                versions_deleted: 1,
            }))
        } else {
            Ok(Response::new(DeleteContextResponse {
                success: false,
                versions_deleted: 0,
            }))
        }
    }

    async fn export(
        &self,
        request: Request<ExportContextRequest>,
    ) -> Result<Response<Self::ExportStream>, Status> {
        let req = request.into_inner();
        debug!(names = ?req.names, "Exporting contexts");

        let contexts = self.contexts.read().await;

        // Collect matching contexts
        let matching: Vec<&ContextEntry> = if req.names.is_empty() {
            contexts.values().collect()
        } else {
            req.names
                .iter()
                .filter_map(|name| contexts.get(name))
                .collect()
        };

        // Serialize to JSON via an intermediate serde-compatible struct
        #[derive(Serialize)]
        struct ExportedContext {
            name: String,
            content: String,
            tags: Vec<String>,
            context_type: i32,
            created_at: i64,
            updated_at: i64,
            version: String,
        }

        let exported: Vec<ExportedContext> = matching
            .iter()
            .map(|e| ExportedContext {
                name: e.name.clone(),
                content: e.content.clone(),
                tags: e.tags.clone(),
                context_type: e.context_type,
                created_at: e.created_at,
                updated_at: e.updated_at,
                version: e.version.clone(),
            })
            .collect();

        let json_string = simd_json::to_string(&exported)
            .map_err(|e| Status::internal(format!("Serialization error: {}", e)))?;
        let json_bytes = json_string.into_bytes();
        let total_size = json_bytes.len() as i64;

        // Chunk into 64KB pieces
        let (tx, rx) = tokio::sync::mpsc::channel(16);

        tokio::spawn(async move {
            let chunk_size = 65536usize;
            let mut offset = 0i64;

            if json_bytes.is_empty() {
                // Empty export: send a single final chunk
                let _ = tx
                    .send(Ok(ExportChunk {
                        data: vec![],
                        offset: 0,
                        total_size: 0,
                        is_final: true,
                    }))
                    .await;
                return;
            }

            for chunk_data in json_bytes.chunks(chunk_size) {
                let is_final = (offset as usize + chunk_data.len()) >= total_size as usize;
                let msg = ExportChunk {
                    data: chunk_data.to_vec(),
                    offset,
                    total_size,
                    is_final,
                };
                if tx.send(Ok(msg)).await.is_err() {
                    break;
                }
                offset += chunk_data.len() as i64;
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn import(
        &self,
        request: Request<tonic::Streaming<ImportChunk>>,
    ) -> Result<Response<ImportContextResponse>, Status> {
        let mut stream = request.into_inner();

        let mut data = Vec::new();
        let mut overwrite = false;

        // Collect all chunks
        while let Some(chunk) = stream.message().await? {
            overwrite = chunk.overwrite_existing;
            data.extend_from_slice(&chunk.data);
            if chunk.is_final {
                break;
            }
        }

        if data.is_empty() {
            return Ok(Response::new(ImportContextResponse {
                success: true,
                imported_count: 0,
                skipped_count: 0,
                errors: vec![],
            }));
        }

        // Deserialize from JSON
        #[derive(Deserialize)]
        struct ImportedContext {
            name: String,
            content: String,
            #[serde(default)]
            tags: Vec<String>,
            #[serde(default)]
            context_type: i32,
            #[serde(default)]
            created_at: i64,
            #[serde(default)]
            updated_at: i64,
            #[serde(default)]
            version: String,
        }

        let imported: Vec<ImportedContext> =
            simd_json::from_slice::<Vec<ImportedContext>>(&mut data)
                .map_err(|e| Status::invalid_argument(format!("Invalid JSON: {}", e)))?;

        let mut contexts = self.contexts.write().await;
        let mut imported_count = 0i32;
        let mut skipped_count = 0i32;
        let mut errors = Vec::new();
        let now = chrono::Utc::now().timestamp_millis();

        for item in imported {
            if item.name.is_empty() {
                errors.push("Skipped entry with empty name".to_string());
                skipped_count += 1;
                continue;
            }

            if contexts.contains_key(&item.name) && !overwrite {
                skipped_count += 1;
                continue;
            }

            let entry = ContextEntry {
                name: item.name.clone(),
                content: item.content,
                tags: item.tags,
                context_type: item.context_type,
                created_at: if item.created_at > 0 {
                    item.created_at
                } else {
                    now
                },
                updated_at: if item.updated_at > 0 {
                    item.updated_at
                } else {
                    now
                },
                version: if item.version.is_empty() {
                    "v1".to_string()
                } else {
                    item.version
                },
            };

            contexts.insert(item.name, entry);
            imported_count += 1;
        }

        info!(
            imported = imported_count,
            skipped = skipped_count,
            "Import complete"
        );

        Ok(Response::new(ImportContextResponse {
            success: errors.is_empty(),
            imported_count,
            skipped_count,
            errors,
        }))
    }

    async fn merge(
        &self,
        request: Request<MergeContextsRequest>,
    ) -> Result<Response<MergeContextsResponse>, Status> {
        let req = request.into_inner();
        debug!(
            sources = ?req.source_names,
            target = %req.target_name,
            "Merging contexts"
        );

        if req.source_names.is_empty() {
            return Err(Status::invalid_argument("No source contexts specified"));
        }

        if req.target_name.is_empty() {
            return Err(Status::invalid_argument("No target name specified"));
        }

        let mut contexts = self.contexts.write().await;

        // Collect source contents
        let mut source_contents: Vec<(String, String, i64)> = Vec::new();
        for name in &req.source_names {
            match contexts.get(name) {
                Some(entry) => {
                    source_contents.push((
                        entry.name.clone(),
                        entry.content.clone(),
                        entry.updated_at,
                    ));
                }
                None => {
                    warn!(name = %name, "Source context not found for merge");
                }
            }
        }

        if source_contents.is_empty() {
            return Ok(Response::new(MergeContextsResponse {
                success: false,
                target_name: req.target_name,
                merged_size_bytes: 0,
            }));
        }

        let strategy = MergeStrategy::try_from(req.strategy).unwrap_or(MergeStrategy::Concat);

        let merged_content = match strategy {
            MergeStrategy::Concat => {
                // Concatenate all content with headers
                source_contents
                    .iter()
                    .map(|(name, content, _)| format!("--- {} ---\n{}", name, content))
                    .collect::<Vec<_>>()
                    .join("\n\n")
            }
            MergeStrategy::Dedupe => {
                // Concatenate but deduplicate identical lines
                let mut seen = std::collections::HashSet::new();
                let mut lines = Vec::new();
                for (_, content, _) in &source_contents {
                    for line in content.lines() {
                        if seen.insert(line.to_string()) {
                            lines.push(line.to_string());
                        }
                    }
                }
                lines.join("\n")
            }
            MergeStrategy::Latest => {
                // Take only the most recently updated source
                let mut sorted = source_contents.clone();
                sorted.sort_by_key(|b| std::cmp::Reverse(b.2));
                sorted
                    .first()
                    .map(|(_, content, _)| content.clone())
                    .unwrap_or_default()
            }
        };

        let merged_size = merged_content.len() as i64;
        let now = chrono::Utc::now().timestamp_millis();

        // Collect all tags from sources
        let mut all_tags: Vec<String> = Vec::new();
        for name in &req.source_names {
            if let Some(entry) = contexts.get(name) {
                for tag in &entry.tags {
                    if !all_tags.contains(tag) {
                        all_tags.push(tag.clone());
                    }
                }
            }
        }

        // Create or update target
        if let Some(target) = contexts.get_mut(&req.target_name) {
            let new_version = increment_version(&target.version);
            target.content = merged_content;
            target.tags = all_tags;
            target.updated_at = now;
            target.version = new_version;
        } else {
            let entry = ContextEntry {
                name: req.target_name.clone(),
                content: merged_content,
                tags: all_tags,
                context_type: ContextType::Generic as i32,
                created_at: now,
                updated_at: now,
                version: "v1".to_string(),
            };
            contexts.insert(req.target_name.clone(), entry);
        }

        info!(target = %req.target_name, size = merged_size, "Merge complete");

        Ok(Response::new(MergeContextsResponse {
            success: true,
            target_name: req.target_name,
            merged_size_bytes: merged_size,
        }))
    }
}
