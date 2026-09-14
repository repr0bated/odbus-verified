//! BackendArchitectService gRPC implementation
//!
//! Delegates to the backend_architect agent in the GrpcAgentPool for
//! architecture analysis, design, review, suggestion, and documentation.

use std::pin::Pin;

use simd_json::prelude::*;
use simd_json::OwnedValue as Value;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use crate::orchestration::proto::op_chat_orchestration::{
    backend_architect_service_server::BackendArchitectService, ArchitectAnalyzeRequest,
    ArchitectAnalyzeResponse, ArchitectDesignRequest, ArchitectDesignResponse,
    ArchitectDocumentRequest, ArchitectReviewRequest, ArchitectReviewResponse,
    ArchitectSuggestRequest, ArchitectSuggestResponse, ArchitectureFinding, DocumentChunk,
    ReviewIssue, Suggestion,
};

use super::OrchestrationServer;

const AGENT_ID: &str = "backend_architect";

/// Parse JSON result from pool execute into an ArchitectAnalyzeResponse.
/// If the agent is unavailable, return a graceful error response (success=false).
fn parse_analyze_response(result: Value) -> ArchitectAnalyzeResponse {
    let analysis_json = simd_json::to_string(&result).unwrap_or_default();

    // Try to extract findings from the result
    let findings = if let Some(arr) = result.get("findings").and_then(|v| v.as_array()) {
        arr.iter()
            .filter_map(|f| {
                Some(ArchitectureFinding {
                    category: f.get("category")?.as_str()?.to_string(),
                    severity: f
                        .get("severity")
                        .and_then(|v| v.as_str())
                        .unwrap_or("info")
                        .to_string(),
                    message: f
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    location: f
                        .get("location")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    suggestion: f
                        .get("suggestion")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
            })
            .collect()
    } else {
        vec![]
    };

    ArchitectAnalyzeResponse {
        success: true,
        analysis_json,
        findings,
    }
}

fn parse_design_response(result: Value) -> ArchitectDesignResponse {
    let design_json = simd_json::to_string(&result).unwrap_or_default();

    let extract_strings = |key: &str| -> Vec<String> {
        result
            .get(key)
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    };

    ArchitectDesignResponse {
        success: true,
        design_json,
        components: extract_strings("components"),
        interfaces: extract_strings("interfaces"),
        trade_offs: extract_strings("trade_offs"),
    }
}

fn parse_review_response(result: Value) -> ArchitectReviewResponse {
    let review_json = simd_json::to_string(&result).unwrap_or_default();

    let score = result.get("score").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

    let issues = result
        .get("issues")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|f| ReviewIssue {
                    category: f
                        .get("category")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    severity: f
                        .get("severity")
                        .and_then(|v| v.as_str())
                        .unwrap_or("info")
                        .to_string(),
                    description: f
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    recommendation: f
                        .get("recommendation")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();

    let strengths = result
        .get("strengths")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    ArchitectReviewResponse {
        success: true,
        review_json,
        score,
        issues,
        strengths,
    }
}

fn parse_suggest_response(result: Value) -> ArchitectSuggestResponse {
    let suggestions = result
        .get("suggestions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|f| Suggestion {
                    title: f
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    description: f
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    rationale: f
                        .get("rationale")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    effort: f.get("effort").and_then(|v| v.as_i64()).unwrap_or(3) as i32,
                    impact: f.get("impact").and_then(|v| v.as_i64()).unwrap_or(3) as i32,
                })
                .collect()
        })
        .unwrap_or_default();

    ArchitectSuggestResponse {
        success: true,
        suggestions,
    }
}

/// Helper to get a session_id for pool calls. Uses "default" as fallback.
fn extract_session_id(session_id: &str) -> &str {
    if session_id.is_empty() {
        "default"
    } else {
        session_id
    }
}

#[tonic::async_trait]
impl BackendArchitectService for OrchestrationServer {
    type DocumentStream =
        Pin<Box<dyn tokio_stream::Stream<Item = Result<DocumentChunk, Status>> + Send + 'static>>;

    #[tracing::instrument(skip(self))]
    async fn analyze(
        &self,
        request: Request<ArchitectAnalyzeRequest>,
    ) -> Result<Response<ArchitectAnalyzeResponse>, Status> {
        let req = request.into_inner();
        let session_id = extract_session_id(&req.session_id);
        info!(path = %req.path, scope = %req.scope, "BackendArchitect: analyze");

        let args = simd_json::json!({
            "path": req.path,
            "scope": req.scope,
            "focus_areas": req.focus_areas,
        });

        match self
            .pool
            .execute(session_id, AGENT_ID, "analyze", args)
            .await
        {
            Ok(result) => Ok(Response::new(parse_analyze_response(result))),
            Err(e) => {
                warn!(error = %e, "Backend architect analyze failed");
                Ok(Response::new(ArchitectAnalyzeResponse {
                    success: false,
                    analysis_json: format!("{{\"error\": \"{}\"}}", e),
                    findings: vec![],
                }))
            }
        }
    }

    #[tracing::instrument(skip(self))]
    async fn design(
        &self,
        request: Request<ArchitectDesignRequest>,
    ) -> Result<Response<ArchitectDesignResponse>, Status> {
        let req = request.into_inner();
        let session_id = extract_session_id(&req.session_id);
        info!("BackendArchitect: design");

        let args = simd_json::json!({
            "context": req.context,
            "requirements": req.requirements,
            "constraints": req.constraints,
            "style": req.style,
        });

        match self
            .pool
            .execute(session_id, AGENT_ID, "design", args)
            .await
        {
            Ok(result) => Ok(Response::new(parse_design_response(result))),
            Err(e) => {
                warn!(error = %e, "Backend architect design failed");
                Ok(Response::new(ArchitectDesignResponse {
                    success: false,
                    design_json: format!("{{\"error\": \"{}\"}}", e),
                    components: vec![],
                    interfaces: vec![],
                    trade_offs: vec![],
                }))
            }
        }
    }

    #[tracing::instrument(skip(self))]
    async fn review(
        &self,
        request: Request<ArchitectReviewRequest>,
    ) -> Result<Response<ArchitectReviewResponse>, Status> {
        let req = request.into_inner();
        let session_id = extract_session_id(&req.session_id);
        info!("BackendArchitect: review");

        let args = simd_json::json!({
            "design": req.design,
            "review_criteria": req.review_criteria,
        });

        match self
            .pool
            .execute(session_id, AGENT_ID, "review", args)
            .await
        {
            Ok(result) => Ok(Response::new(parse_review_response(result))),
            Err(e) => {
                warn!(error = %e, "Backend architect review failed");
                Ok(Response::new(ArchitectReviewResponse {
                    success: false,
                    review_json: format!("{{\"error\": \"{}\"}}", e),
                    score: 0,
                    issues: vec![],
                    strengths: vec![],
                }))
            }
        }
    }

    #[tracing::instrument(skip(self))]
    async fn suggest(
        &self,
        request: Request<ArchitectSuggestRequest>,
    ) -> Result<Response<ArchitectSuggestResponse>, Status> {
        let req = request.into_inner();
        let session_id = extract_session_id(&req.session_id);
        info!("BackendArchitect: suggest");

        let args = simd_json::json!({
            "context": req.context,
            "constraints": req.constraints,
            "max_suggestions": req.max_suggestions,
        });

        match self
            .pool
            .execute(session_id, AGENT_ID, "suggest", args)
            .await
        {
            Ok(result) => Ok(Response::new(parse_suggest_response(result))),
            Err(e) => {
                warn!(error = %e, "Backend architect suggest failed");
                Ok(Response::new(ArchitectSuggestResponse {
                    success: false,
                    suggestions: vec![],
                }))
            }
        }
    }

    #[tracing::instrument(skip(self))]
    async fn document(
        &self,
        request: Request<ArchitectDocumentRequest>,
    ) -> Result<Response<Self::DocumentStream>, Status> {
        let req = request.into_inner();
        let session_id = extract_session_id(&req.session_id).to_string();
        info!(path = %req.path, "BackendArchitect: document (streaming)");

        let args = simd_json::json!({
            "path": req.path,
            "format": req.format,
            "sections": req.sections,
        });

        let pool = self.pool.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(64);

        tokio::spawn(async move {
            let mut sequence = 0i32;

            let callback_tx = tx.clone();
            let result = pool
                .execute_streaming(&session_id, AGENT_ID, "document", args, move |chunk| {
                    let msg = DocumentChunk {
                        content: chunk.content,
                        section: String::new(),
                        is_final: chunk.is_final,
                        sequence,
                    };
                    // Use try_send since callback is not async
                    let _ = callback_tx.try_send(Ok(msg));
                    sequence += 1;
                })
                .await;

            // If the streaming call itself failed, send error as final chunk
            if let Err(e) = result {
                let _ = tx
                    .send(Ok(DocumentChunk {
                        content: format!("Error: {}", e),
                        section: String::new(),
                        is_final: true,
                        sequence,
                    }))
                    .await;
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}
