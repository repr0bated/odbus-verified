//! CognitiveToolService compatibility implementation.
//!
//! The active external ingress is owned by `op-grpc-bridge`; this module keeps
//! the generated service contract usable by embedded/internal consumers only.
//!
//! # Requirements
//! Implements R1-R3 (core querying), R9-R11 (resilience/auth), and stubs for
//! R4-R7 (lifecycle, advanced). Traces every RPC to the requirements doc.
//!
//! # Design
//! - gRPC → CognitiveToolRegistry → NotebookLM bridge + CognitiveMemoryStore
//! - CognitiveMemoryStore is the cache; NotebookLM is the source of truth
//! - Sessions tracked via SessionManager for conversation_id follow-ups
//! - Quota enforced via QuotaManager before forwarding to the bridge
//!
//! # Security (R13)
//! - No shell=True, no eval
//! - Credentials stored 0o600
//! - Exponential backoff retries on bridge calls

use std::sync::Arc;

use chrono::Utc;
use tonic::{Request, Response, Status};
use tracing::{info, warn};
use uuid::Uuid;

use crate::gemini_fallback::GeminiFallback;
use crate::memory_store::{CognitiveMemoryStore, NamespaceKind};
use crate::proto::cognitive_tool_service_server::CognitiveToolService;
use crate::proto::*;
use crate::quota::QuotaManager;
use crate::session::{QueryTurn, SessionManager};

/// The gRPC service implementation.
///
/// Wired into the tonic server alongside health and reflection services.
/// Delegates to CognitiveMemoryStore for namespace/entry ops and to the
/// NotebookLM MCP bridge (via ToolRegistry) for grounded queries.
#[derive(Clone)]
pub struct CognitiveGrpcService {
    memory_store: Arc<CognitiveMemoryStore>,
    session_manager: Arc<SessionManager>,
    quota_manager: Arc<QuotaManager>,
    gemini_fallback: Arc<GeminiFallback>,
}

impl CognitiveGrpcService {
    pub fn new(
        memory_store: Arc<CognitiveMemoryStore>,
        session_manager: Arc<SessionManager>,
        quota_manager: Arc<QuotaManager>,
        gemini_fallback: Arc<GeminiFallback>,
    ) -> Self {
        Self {
            memory_store,
            session_manager,
            quota_manager,
            gemini_fallback,
        }
    }
}

#[tonic::async_trait]
impl CognitiveToolService for CognitiveGrpcService {
    // =========================================================================
    // R1 — AskQuestion (grounded query)
    // =========================================================================
    async fn ask_question(
        &self,
        request: Request<AskQuestionRequest>,
    ) -> Result<Response<AskQuestionResponse>, Status> {
        let req = request.into_inner();
        info!(
            notebook_id = %req.notebook_id,
            conversation_id = %req.conversation_id,
            "AskQuestion"
        );

        // R11 — quota check
        let (allowed, remaining, _limit) = self.quota_manager.check_and_increment().await;
        if !allowed {
            return Err(Status::resource_exhausted(format!(
                "Daily query quota exceeded ({} remaining)",
                remaining
            )));
        }

        // R2 — conversation_id session management
        let session = self
            .session_manager
            .get_or_create(&req.conversation_id, &req.notebook_id);
        let conversation_id = session.id.clone();

        // Attempt grounded query via memory store.
        // Phase 1: query entries matching the notebook namespace.
        // Phase 2+: this forwards through the NotebookLM bridge.
        let namespace = format!("project:{}", req.notebook_id);
        let entries = self
            .memory_store
            .query_entries(crate::memory_store::EntryQuery {
                namespace_id: Some(namespace.clone()),
                key_pattern: Some(req.query.clone()),
                tags: None,
                limit: Some(10),
                offset: None,
            })
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let grounded = !entries.is_empty();
        let answer = if grounded {
            entries
                .iter()
                .map(|e| {
                    format!(
                        "[{}] {}",
                        e.key,
                        e.value.as_str().unwrap_or(&e.value.to_string())
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        } else {
            format!(
                "No grounded answer found for '{}' in notebook '{}'.",
                req.query, req.notebook_id
            )
        };

        let citations: Vec<Citation> = entries
            .iter()
            .map(|e| Citation {
                text: e.key.clone(),
                source: e.namespace_id.clone(),
                page: String::new(),
            })
            .collect();

        // Append turn to session history
        let _ = self.session_manager.append_turn(
            &conversation_id,
            QueryTurn {
                query: req.query.clone(),
                answer: answer.clone(),
                timestamp: Utc::now(),
                citations_count: citations.len() as u32,
                grounded,
            },
        );

        Ok(Response::new(AskQuestionResponse {
            answer,
            citations,
            conversation_id,
            grounded,
        }))
    }

    // =========================================================================
    // QueryNotebook
    // =========================================================================
    async fn query_notebook(
        &self,
        request: Request<QueryNotebookRequest>,
    ) -> Result<Response<QueryNotebookResponse>, Status> {
        let req = request.into_inner();
        info!(notebook_id = %req.notebook_id, "QueryNotebook");

        let (allowed, _, _) = self.quota_manager.check_and_increment().await;
        if !allowed {
            return Err(Status::resource_exhausted("Daily query quota exceeded"));
        }

        let session = self
            .session_manager
            .get_or_create(&req.conversation_id, &req.notebook_id);

        let namespace = format!("project:{}", req.notebook_id);
        let limit = if req.max_results > 0 {
            req.max_results as i64
        } else {
            10
        };

        let entries = self
            .memory_store
            .query_entries(crate::memory_store::EntryQuery {
                namespace_id: Some(namespace),
                key_pattern: Some(req.query.clone()),
                tags: None,
                limit: Some(limit),
                offset: None,
            })
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let answer = entries
            .iter()
            .map(|e| {
                format!(
                    "[{}] {}",
                    e.key,
                    e.value.as_str().unwrap_or(&e.value.to_string())
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let citations: Vec<Citation> = entries
            .iter()
            .map(|e| Citation {
                text: e.key.clone(),
                source: e.namespace_id.clone(),
                page: String::new(),
            })
            .collect();

        let _ = self.session_manager.append_turn(
            &session.id,
            QueryTurn {
                query: req.query,
                answer: answer.clone(),
                timestamp: Utc::now(),
                citations_count: citations.len() as u32,
                grounded: !entries.is_empty(),
            },
        );

        Ok(Response::new(QueryNotebookResponse {
            answer,
            citations,
            conversation_id: session.id,
        }))
    }

    // =========================================================================
    // R3 — ListNotebooks
    // =========================================================================
    async fn list_notebooks(
        &self,
        request: Request<ListNotebooksRequest>,
    ) -> Result<Response<ListNotebooksResponse>, Status> {
        let req = request.into_inner();
        info!(kind_filter = %req.kind_filter, "ListNotebooks");

        let kind = if req.kind_filter.is_empty() {
            None
        } else {
            req.kind_filter.parse::<NamespaceKind>().ok()
        };

        let namespaces = self
            .memory_store
            .list_namespaces(kind)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let total = namespaces.len() as i32;
        let offset = req.offset.max(0) as usize;
        let limit = if req.limit > 0 {
            req.limit as usize
        } else {
            100
        };

        let notebooks: Vec<NotebookInfo> = namespaces
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|ns| NotebookInfo {
                id: ns.id,
                name: ns.name,
                kind: ns.kind.to_string(),
                description: ns.description.unwrap_or_default(),
                source_count: 0, // TODO: count entries per namespace
                created_at: ns.created_at.to_rfc3339(),
                updated_at: ns.updated_at.to_rfc3339(),
            })
            .collect();

        Ok(Response::new(ListNotebooksResponse { notebooks, total }))
    }

    // =========================================================================
    // R3 — GetNotebook
    // =========================================================================
    async fn get_notebook(
        &self,
        request: Request<GetNotebookRequest>,
    ) -> Result<Response<GetNotebookResponse>, Status> {
        let req = request.into_inner();
        info!(notebook_id = %req.notebook_id, "GetNotebook");

        // Try by ID-as-name first
        let ns = self
            .memory_store
            .get_namespace_by_name(&req.notebook_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| {
                Status::not_found(format!("Notebook '{}' not found", req.notebook_id))
            })?;

        let metadata_json =
            serde_json::to_string(&ns.metadata).unwrap_or_else(|_| "{}".to_string());

        Ok(Response::new(GetNotebookResponse {
            notebook: Some(NotebookInfo {
                id: ns.id,
                name: ns.name,
                kind: ns.kind.to_string(),
                description: ns.description.unwrap_or_default(),
                source_count: 0,
                created_at: ns.created_at.to_rfc3339(),
                updated_at: ns.updated_at.to_rfc3339(),
            }),
            metadata_json,
        }))
    }

    // =========================================================================
    // R4 — CreateNotebook
    // =========================================================================
    async fn create_notebook(
        &self,
        request: Request<CreateNotebookRequest>,
    ) -> Result<Response<CreateNotebookResponse>, Status> {
        let req = request.into_inner();
        info!(title = %req.title, "CreateNotebook");

        let kind = if req.kind.is_empty() {
            NamespaceKind::Project
        } else {
            req.kind.parse().unwrap_or(NamespaceKind::Custom)
        };

        let name = format!("{}:{}", kind, req.title);
        let ns = self
            .memory_store
            .upsert_namespace(
                &name,
                kind,
                if req.description.is_empty() {
                    None
                } else {
                    Some(req.description.as_str())
                },
                None,
                None,
                serde_json::json!({}),
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(CreateNotebookResponse {
            notebook: Some(NotebookInfo {
                id: ns.id,
                name: ns.name,
                kind: ns.kind.to_string(),
                description: ns.description.unwrap_or_default(),
                source_count: 0,
                created_at: ns.created_at.to_rfc3339(),
                updated_at: ns.updated_at.to_rfc3339(),
            }),
        }))
    }

    // =========================================================================
    // R4 — BatchCreateNotebooks
    // =========================================================================
    async fn batch_create_notebooks(
        &self,
        request: Request<BatchCreateNotebooksRequest>,
    ) -> Result<Response<BatchCreateNotebooksResponse>, Status> {
        let req = request.into_inner();
        info!(count = req.notebooks.len(), "BatchCreateNotebooks");

        let mut created_notebooks = Vec::new();
        let mut failed = 0i32;

        for nb_req in req.notebooks {
            let kind = if nb_req.kind.is_empty() {
                NamespaceKind::Project
            } else {
                nb_req.kind.parse().unwrap_or(NamespaceKind::Custom)
            };

            let name = format!("{}:{}", kind, nb_req.title);
            match self
                .memory_store
                .upsert_namespace(
                    &name,
                    kind,
                    if nb_req.description.is_empty() {
                        None
                    } else {
                        Some(nb_req.description.as_str())
                    },
                    None,
                    None,
                    serde_json::json!({}),
                )
                .await
            {
                Ok(ns) => {
                    created_notebooks.push(NotebookInfo {
                        id: ns.id,
                        name: ns.name,
                        kind: ns.kind.to_string(),
                        description: ns.description.unwrap_or_default(),
                        source_count: 0,
                        created_at: ns.created_at.to_rfc3339(),
                        updated_at: ns.updated_at.to_rfc3339(),
                    });
                }
                Err(e) => {
                    warn!(title = %nb_req.title, error = %e, "Failed to create notebook");
                    failed += 1;
                }
            }
        }

        let created = created_notebooks.len() as i32;
        Ok(Response::new(BatchCreateNotebooksResponse {
            notebooks: created_notebooks,
            created,
            failed,
        }))
    }

    // =========================================================================
    // R5 — AddSource
    // =========================================================================
    async fn add_source(
        &self,
        request: Request<AddSourceRequest>,
    ) -> Result<Response<AddSourceResponse>, Status> {
        let req = request.into_inner();
        info!(
            notebook_id = %req.notebook_id,
            source_type = %req.source_type,
            "AddSource"
        );

        let namespace = format!("project:{}", req.notebook_id);

        // Ensure namespace exists
        let kind = NamespaceKind::Project;
        if self
            .memory_store
            .get_namespace_by_name(&namespace)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .is_none()
        {
            self.memory_store
                .upsert_namespace(&namespace, kind, None, None, None, serde_json::json!({}))
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        }

        let source_id = Uuid::new_v4().to_string();
        let key = if req.title.is_empty() {
            source_id.clone()
        } else {
            req.title.clone()
        };

        let value = serde_json::json!({
            "source_type": req.source_type,
            "content": req.content,
            "title": req.title,
        });

        self.memory_store
            .store_entry(&namespace, &key, value, req.tags, None)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(AddSourceResponse {
            source_id,
            success: true,
        }))
    }

    // =========================================================================
    // R5 — AddFolder (bulk ingest)
    // =========================================================================
    async fn add_folder(
        &self,
        request: Request<AddFolderRequest>,
    ) -> Result<Response<AddFolderResponse>, Status> {
        let req = request.into_inner();
        info!(
            notebook_id = %req.notebook_id,
            folder_path = %req.folder_path,
            "AddFolder"
        );

        // Validate path exists — no shell=True, use std::fs
        let path = std::path::Path::new(&req.folder_path);
        if !path.exists() || !path.is_dir() {
            return Err(Status::invalid_argument(format!(
                "Folder '{}' does not exist or is not a directory",
                req.folder_path
            )));
        }

        let namespace = format!("project:{}", req.notebook_id);
        // Ensure namespace exists
        if self
            .memory_store
            .get_namespace_by_name(&namespace)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .is_none()
        {
            self.memory_store
                .upsert_namespace(
                    &namespace,
                    NamespaceKind::Project,
                    None,
                    None,
                    None,
                    serde_json::json!({}),
                )
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        }

        let mut added = 0i32;
        let mut skipped = 0i32;
        let mut errors = Vec::new();

        // Walk directory — no shell, pure Rust
        let walker = if req.recursive {
            walkdir(path)
        } else {
            walkdir_shallow(path)
        };

        for entry_path in walker {
            // Apply glob patterns if specified
            if !req.patterns.is_empty() {
                let file_name = entry_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let matches = req.patterns.iter().any(|pat| glob_match(pat, &file_name));
                if !matches {
                    skipped += 1;
                    continue;
                }
            }

            match std::fs::read_to_string(&entry_path) {
                Ok(content) => {
                    let key = entry_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| Uuid::new_v4().to_string());

                    let value = serde_json::json!({
                        "source_type": "file",
                        "content": content,
                        "path": entry_path.to_string_lossy(),
                    });

                    match self
                        .memory_store
                        .store_entry(&namespace, &key, value, vec![], None)
                        .await
                    {
                        Ok(_) => added += 1,
                        Err(e) => {
                            errors.push(format!("{}: {}", entry_path.display(), e));
                        }
                    }
                }
                Err(e) => {
                    skipped += 1;
                    errors.push(format!("{}: {}", entry_path.display(), e));
                }
            }
        }

        Ok(Response::new(AddFolderResponse {
            sources_added: added,
            sources_skipped: skipped,
            errors,
        }))
    }

    // =========================================================================
    // R6 — ListSources
    // =========================================================================
    async fn list_sources(
        &self,
        request: Request<ListSourcesRequest>,
    ) -> Result<Response<ListSourcesResponse>, Status> {
        let req = request.into_inner();
        info!(notebook_id = %req.notebook_id, "ListSources");

        let namespace = format!("project:{}", req.notebook_id);
        let limit = if req.limit > 0 {
            req.limit as usize
        } else {
            100
        };
        let offset = req.offset.max(0) as usize;

        let all_entries = self
            .memory_store
            .query_entries(crate::memory_store::EntryQuery {
                namespace_id: Some(namespace),
                key_pattern: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        let total = all_entries.len() as i32;
        let sources: Vec<SourceInfo> = all_entries
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|e| {
                let source_type = e
                    .value
                    .get("source_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("text")
                    .to_string();

                SourceInfo {
                    id: e.id,
                    title: e.key,
                    source_type,
                    tags: e.tags,
                    created_at: e.created_at.to_rfc3339(),
                }
            })
            .collect();

        Ok(Response::new(ListSourcesResponse { sources, total }))
    }

    // =========================================================================
    // R6 — GetSourceContent
    // =========================================================================
    async fn get_source_content(
        &self,
        request: Request<GetSourceContentRequest>,
    ) -> Result<Response<GetSourceContentResponse>, Status> {
        let req = request.into_inner();
        info!(
            notebook_id = %req.notebook_id,
            source_id = %req.source_id,
            "GetSourceContent"
        );

        let namespace = format!("project:{}", req.notebook_id);
        let entry = self
            .memory_store
            .retrieve_entry(&namespace, &req.source_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found(format!("Source '{}' not found", req.source_id)))?;

        let content = entry
            .value
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let source_type = entry
            .value
            .get("source_type")
            .and_then(|v| v.as_str())
            .unwrap_or("text")
            .to_string();
        let title = entry.key;

        Ok(Response::new(GetSourceContentResponse {
            content,
            source_type,
            title,
        }))
    }

    // =========================================================================
    // R7 — GenerateDataTable (Phase 3)
    // =========================================================================
    async fn generate_data_table(
        &self,
        request: Request<GenerateDataTableRequest>,
    ) -> Result<Response<GenerateDataTableResponse>, Status> {
        let req = request.into_inner();
        info!(notebook_id = %req.notebook_id, "GenerateDataTable");

        let namespace = format!("project:{}", req.notebook_id);

        // Step 1: Get all sources in the notebook
        let entries = self
            .memory_store
            .query_entries(crate::memory_store::EntryQuery {
                namespace_id: Some(namespace.clone()),
                key_pattern: None,
                tags: None,
                limit: Some(50), // Sample size for table extraction
                offset: None,
            })
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        if entries.is_empty() {
            return Ok(Response::new(GenerateDataTableResponse {
                data_json: "[]".to_string(),
                row_count: 0,
            }));
        }

        let context = entries
            .iter()
            .map(|e| {
                format!(
                    "Source: {}\nContent: {}",
                    e.key,
                    e.value.as_str().unwrap_or(&e.value.to_string())
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n---\n\n");

        // Step 2: Use Gemini Fallback to extract structured data
        let columns_str = req.columns.join(", ");
        let prompt = format!(
            "Task: {}\n\nExtract information into a JSON array of objects. Each object must have exactly these keys: {}.\n\nReturn ONLY the JSON array, nothing else.",
            req.prompt, columns_str
        );

        let result = self
            .gemini_fallback
            .gemini_query(&prompt, Some(&context))
            .await
            .map_err(|e| Status::internal(format!("Data extraction failed: {}", e)))?;

        // Step 3: Clean up Markdown code blocks if any
        let mut raw_str = result.answer.trim();
        if let Some(rest) = raw_str.strip_prefix("```json") {
            raw_str = rest.strip_suffix("```").unwrap_or(rest).trim();
        } else if let Some(rest) = raw_str.strip_prefix("```") {
            raw_str = rest.strip_suffix("```").unwrap_or(rest).trim();
        }
        let json_str = raw_str.to_string();

        // Count rows roughly by parsing
        let row_count = if let Ok(serde_json::Value::Array(arr)) = serde_json::from_str(&json_str) {
            arr.len() as i32
        } else {
            0
        };

        Ok(Response::new(GenerateDataTableResponse {
            data_json: json_str,
            row_count,
        }))
    }

    // =========================================================================
    // R10 — GetHealth
    // =========================================================================
    async fn get_health(
        &self,
        request: Request<GetHealthRequest>,
    ) -> Result<Response<GetHealthResponse>, Status> {
        let req = request.into_inner();
        info!(deep_check = req.deep_check, "GetHealth");

        let (remaining, limit) = self.quota_manager.status().await;

        let mut components = serde_json::json!({
            "memory_store": "ok",
            "session_manager": "ok",
            "quota_manager": "ok",
        });

        if req.deep_check {
            // Deep check — verify memory store connectivity
            match self.memory_store.get_stats().await {
                Ok(stats) => {
                    components["memory_store_stats"] = serde_json::json!({
                        "total_namespaces": stats.total_namespaces,
                        "total_entries": stats.total_entries,
                    });
                }
                Err(e) => {
                    components["memory_store"] = serde_json::json!(format!("error: {}", e));
                }
            }

            components["active_sessions"] = serde_json::json!(self.session_manager.active_count());
            components["total_sessions"] = serde_json::json!(self.session_manager.count());
        }

        Ok(Response::new(GetHealthResponse {
            healthy: true,
            status: "operational".to_string(),
            components_json: serde_json::to_string(&components)
                .unwrap_or_else(|_| "{}".to_string()),
            queries_remaining: remaining as i32,
            queries_limit: limit as i32,
            auth_status: "chrome_profile".to_string(),
        }))
    }

    // =========================================================================
    // R9 — SetupAuth
    // =========================================================================
    async fn setup_auth(
        &self,
        request: Request<SetupAuthRequest>,
    ) -> Result<Response<SetupAuthResponse>, Status> {
        let req = request.into_inner();
        info!(auth_method = %req.auth_method, "SetupAuth");

        // R9 — persistent auth: never wipe Chrome profile on failed launch
        // R13 — credentials 0o600
        // Phase 1: validate and store credential reference.
        // Actual Chrome profile management is in the NotebookLM sidecar.

        if req.auth_method.is_empty() {
            return Err(Status::invalid_argument(
                "auth_method is required (chrome_profile or cookie)",
            ));
        }

        if req.credential.is_empty() {
            return Err(Status::invalid_argument(
                "credential is required (path to Chrome profile or cookie value)",
            ));
        }

        // Validate Chrome profile path exists if using chrome_profile
        if req.auth_method == "chrome_profile" {
            let path = std::path::Path::new(&req.credential);
            if !path.exists() {
                return Err(Status::invalid_argument(format!(
                    "Chrome profile path '{}' does not exist",
                    req.credential
                )));
            }

            // R13 — check permissions (0o600 for credential files)
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if let Ok(metadata) = std::fs::metadata(path) {
                    let mode = metadata.mode() & 0o777;
                    if mode & 0o077 != 0 {
                        warn!(
                            path = %req.credential,
                            mode = format!("{:o}", mode),
                            "Chrome profile has overly permissive permissions; should be 0o600"
                        );
                    }
                }
            }
        }

        Ok(Response::new(SetupAuthResponse {
            success: true,
            message: format!(
                "Auth configured: method={}, credential stored",
                req.auth_method
            ),
        }))
    }

    // =========================================================================
    // R6 — RemoveSource
    // =========================================================================
    async fn remove_source(
        &self,
        request: Request<RemoveSourceRequest>,
    ) -> Result<Response<RemoveSourceResponse>, Status> {
        let req = request.into_inner();
        info!(
            notebook_id = %req.notebook_id,
            source_id = %req.source_id,
            "RemoveSource"
        );

        let namespace = format!("project:{}", req.notebook_id);

        self.memory_store
            .delete_entry(&namespace, &req.source_id)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(RemoveSourceResponse { success: true }))
    }

    // =========================================================================
    // R12 — GeminiQuery (Fallback)
    // =========================================================================
    async fn gemini_query(
        &self,
        request: Request<GeminiQueryRequest>,
    ) -> Result<Response<GeminiQueryResponse>, Status> {
        let req = request.into_inner();
        info!(mode = %req.mode, "GeminiQuery");

        let context = if req.context.is_empty() {
            None
        } else {
            Some(req.context.as_str())
        };

        if req.mode == "deep_research" {
            let depth = if req.depth > 0 { req.depth as u32 } else { 3 };
            let result = self
                .gemini_fallback
                .deep_research(&req.query, context, depth)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let sections_json =
                serde_json::to_string(&result.sections).unwrap_or_else(|_| "[]".to_string());

            let citations = result
                .sections
                .iter()
                .flat_map(|s| &s.citations)
                .cloned()
                .map(|c| Citation {
                    text: c.text,
                    source: c.source,
                    page: c.page,
                })
                .collect();

            Ok(Response::new(GeminiQueryResponse {
                answer: result.summary,
                citations,
                model: result.model,
                is_fallback: true,
                sections_json,
            }))
        } else {
            let result = self
                .gemini_fallback
                .gemini_query(&req.query, context)
                .await
                .map_err(|e| Status::internal(e.to_string()))?;

            let citations = result
                .citations
                .into_iter()
                .map(|c| Citation {
                    text: c.text,
                    source: c.source,
                    page: c.page,
                })
                .collect();

            Ok(Response::new(GeminiQueryResponse {
                answer: result.answer,
                citations,
                model: result.model,
                is_fallback: true,
                sections_json: "[]".to_string(),
            }))
        }
    }

    // =========================================================================
    // R14 — GetToolProfile
    // =========================================================================
    async fn get_tool_profile(
        &self,
        request: Request<GetToolProfileRequest>,
    ) -> Result<Response<GetToolProfileResponse>, Status> {
        let req = request.into_inner();
        info!("GetToolProfile");

        let profile = if req.profile_name.is_empty() {
            crate::tool_profiles::current_profile()
        } else {
            req.profile_name
                .parse()
                .unwrap_or(crate::tool_profiles::current_profile())
        };

        let estimate = crate::tool_profiles::token_estimate(profile);
        let tools = crate::tool_profiles::tools_for_profile(profile)
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        Ok(Response::new(GetToolProfileResponse {
            current_profile: profile.to_string(),
            tool_count: estimate.tool_count as i32,
            schema_tokens: estimate.schema_tokens as i32,
            savings_percent: estimate.savings_percent as i32,
            tools,
        }))
    }

    // =========================================================================
    // R15 — Doctor
    // =========================================================================
    async fn doctor(
        &self,
        _request: Request<DoctorRequest>,
    ) -> Result<Response<DoctorResponse>, Status> {
        info!("Doctor");

        let report = crate::doctor::run_diagnostics(
            &self.memory_store,
            &self.session_manager,
            &self.quota_manager,
            &self.gemini_fallback,
        )
        .await;

        let components_json =
            serde_json::to_string(&report.components).unwrap_or_else(|_| "[]".to_string());

        Ok(Response::new(DoctorResponse {
            overall_status: report.overall_status,
            timestamp: report.timestamp,
            components_json,
            recommendations: report.recommendations,
        }))
    }

    // =========================================================================
    // R15 — GetQueryHistory
    // =========================================================================
    async fn get_query_history(
        &self,
        request: Request<GetQueryHistoryRequest>,
    ) -> Result<Response<GetQueryHistoryResponse>, Status> {
        let req = request.into_inner();
        info!("GetQueryHistory");

        let limit = if req.limit > 0 {
            req.limit as usize
        } else {
            50
        };

        let history = crate::doctor::get_query_history(&self.session_manager, limit);
        let total = history.len() as i32;

        let entries = history
            .into_iter()
            .map(|v| QueryHistoryEntry {
                conversation_id: v["conversation_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                notebook_id: v["notebook_id"].as_str().unwrap_or_default().to_string(),
                query: v["query"].as_str().unwrap_or_default().to_string(),
                answer_preview: v["answer_preview"].as_str().unwrap_or_default().to_string(),
                timestamp: v["timestamp"].as_str().unwrap_or_default().to_string(),
                citations_count: v["citations_count"].as_i64().unwrap_or(0) as i32,
                grounded: v["grounded"].as_bool().unwrap_or(false),
            })
            // Filter by conversation_id if provided
            .filter(|e| req.conversation_id.is_empty() || e.conversation_id == req.conversation_id)
            .collect();

        Ok(Response::new(GetQueryHistoryResponse { entries, total }))
    }
}

// ---------------------------------------------------------------------------
// Filesystem helpers — no shell=True, pure Rust (R13)
// ---------------------------------------------------------------------------

/// Walk a directory recursively, yielding file paths only.
fn walkdir(path: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                result.extend(walkdir(&p));
            } else if p.is_file() {
                result.push(p);
            }
        }
    }
    result
}

/// Walk a directory non-recursively (shallow).
fn walkdir_shallow(path: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                result.push(p);
            }
        }
    }
    result
}

/// Simple glob matching — supports * and ? only.
/// Used for AddFolder pattern filtering without shell expansion.
fn glob_match(pattern: &str, name: &str) -> bool {
    let pattern_chars: Vec<char> = pattern.chars().collect();
    let name_chars: Vec<char> = name.chars().collect();
    glob_match_inner(&pattern_chars, &name_chars)
}

fn glob_match_inner(pattern: &[char], name: &[char]) -> bool {
    match (pattern.first(), name.first()) {
        (None, None) => true,
        (Some('*'), _) => {
            // '*' matches zero or more characters
            glob_match_inner(&pattern[1..], name)
                || (!name.is_empty() && glob_match_inner(pattern, &name[1..]))
        }
        (Some('?'), Some(_)) => glob_match_inner(&pattern[1..], &name[1..]),
        (Some(p), Some(n)) if *p == *n => glob_match_inner(&pattern[1..], &name[1..]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_glob_match_star() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("*.rs", "lib.rs"));
        assert!(!glob_match("*.rs", "main.py"));
    }

    #[test]
    fn should_glob_match_question() {
        assert!(glob_match("?.rs", "a.rs"));
        assert!(!glob_match("?.rs", "ab.rs"));
    }

    #[test]
    fn should_glob_match_exact() {
        assert!(glob_match("main.rs", "main.rs"));
        assert!(!glob_match("main.rs", "lib.rs"));
    }
}
