//! Code-Context MCP Tools
//!
//! Coding-aware retrieval and indexing surfaces backed by the [`RagPipeline`]
//! (Voyage embeddings + Qdrant) and the [`ContextAwarenessEngine`]. These are
//! the tools an external coding client (Droid, Cursor, Codex) calls to pull
//! repository context.
//!
//! Tools:
//! - `code_search`  — filtered semantic search over indexed repos.
//!   subid: `obs.service.code-rag.search@v1`
//! - `code_context` — session-aware fused retrieval + awareness signals.
//!   subid: `exp.service.code-context.render@v1`
//! - `code_index`   — live indexing of a source file / repomix entry.
//!   subid: `src.software.workspace.index@v1`

use crate::context_awareness::{ActivityType, ContextAwarenessEngine};
use crate::rag_pipeline::{CodeFilter, RagPipeline, RagResult, RetrievalMode, RetrievalProfile};
use anyhow::Result;
use async_trait::async_trait;
use op_mcp::tool_registry::{BoxedTool, Tool, ToolRegistry};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::warn;

/// Register all code-context tools against the shared registry.
/// Returns the number of tools registered.
pub async fn register_code_tools(
    registry: &ToolRegistry,
    rag: Arc<RagPipeline>,
    engine: Arc<ContextAwarenessEngine>,
    collection: String,
) -> Result<usize> {
    registry
        .register(Arc::new(CodeSearchTool {
            rag: rag.clone(),
            collection: collection.clone(),
        }) as BoxedTool)
        .await?;
    registry
        .register(Arc::new(CodeContextTool {
            rag: rag.clone(),
            engine,
            collection: collection.clone(),
        }) as BoxedTool)
        .await?;
    registry
        .register(Arc::new(CodeIndexTool { rag, collection }) as BoxedTool)
        .await?;
    Ok(3)
}

// ─── code_search ────────────────────────────────────────────────────────────

pub struct CodeSearchTool {
    rag: Arc<RagPipeline>,
    collection: String,
}

#[async_trait]
impl Tool for CodeSearchTool {
    fn name(&self) -> &str {
        "code_search"
    }

    fn description(&self) -> &str {
        "Semantic code search over indexed repositories. Filter by repo, language, file_type, \
         path substring, or symbol. Returns ranked code chunks with symbols, imports, and line spans."
    }

    fn category(&self) -> &str {
        "code"
    }

    fn tags(&self) -> Vec<String> {
        vec!["code".into(), "search".into(), "rag".into()]
    }

    fn input_schema(&self) -> Value {
        tool_input_schema("code_search")
    }

    fn required_capability(&self) -> Option<&str> {
        Some("cognitive_mcp.read")
    }

    fn subid(&self) -> Option<&str> {
        Some("obs.service.code-rag.search@v1")
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let query = input
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing query"))?;
        let profile = retrieval_profile_from(&input, RetrievalMode::Search);
        let limit = input
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(profile.limit)
            .clamp(1, 50);
        let filter = filter_from(&input, false);
        let collections = collections_from(&input, &self.collection, &profile);

        let results = if collections.len() > 1 {
            self.rag
                .query_fused_collections(&collections, query, limit, profile.fetch_limit, &filter)
                .await?
        } else if input.get("fused").and_then(Value::as_bool).unwrap_or(true) {
            self.rag
                .query_fused(&collections[0], query, limit, &filter)
                .await?
        } else {
            self.rag
                .query_filtered(&collections[0], query, limit, &filter)
                .await?
        };

        Ok(json!({
            "count": results.len(),
            "collections": collections,
            "retrieval_mode": profile.mode.as_str(),
            "rerank_enabled": profile.rerank_enabled,
            "kiro_lsp_state_dir": profile.kiro_lsp_state_dir,
            "results": results_to_simd(&results),
        }))
    }
}

// ─── code_context ───────────────────────────────────────────────────────────

pub struct CodeContextTool {
    rag: Arc<RagPipeline>,
    engine: Arc<ContextAwarenessEngine>,
    collection: String,
}

#[async_trait]
impl Tool for CodeContextTool {
    fn name(&self) -> &str {
        "code_context"
    }

    fn description(&self) -> &str {
        "Session-aware coding context. Records the current activity (query, edit, build_error, \
         test_failure, etc.) into the awareness engine, runs fused retrieval, and returns relevant \
         code plus session signals (stuck, recent errors, topics)."
    }

    fn category(&self) -> &str {
        "code"
    }

    fn tags(&self) -> Vec<String> {
        vec!["code".into(), "context".into(), "session".into()]
    }

    fn input_schema(&self) -> Value {
        tool_input_schema("code_context")
    }

    fn required_capability(&self) -> Option<&str> {
        Some("cognitive_mcp.read")
    }

    fn subid(&self) -> Option<&str> {
        Some("obs.service.code-context.get@v1")
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let query = input
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing query"))?;
        let session_id = input
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or("default");
        let activity_type = input
            .get("activity_type")
            .and_then(Value::as_str)
            .map(ActivityType::parse)
            .unwrap_or(ActivityType::Query);
        let profile = retrieval_profile_from(&input, RetrievalMode::Completion);
        let limit = input
            .get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(profile.limit)
            .clamp(1, 50);

        // Record the activity so stuck/error/topic detection stays current.
        self.engine
            .record_activity(
                session_id,
                activity_type,
                query.to_string(),
                serde_json::json!({ "source": "code_context" }),
            )
            .await;

        let filter = filter_from(&input, false);
        let collections = collections_from(&input, &self.collection, &profile);
        let (results, retrieval_error) = match self
            .rag
            .query_fused_collections(&collections, query, limit, profile.fetch_limit, &filter)
            .await
        {
            Ok(results) => (results, None),
            Err(err) => {
                warn!(
                    error = %err,
                    "Code-context vector retrieval unavailable; returning awareness signals only"
                );
                (Vec::new(), Some(err.to_string()))
            }
        };

        let signals = self
            .engine
            .get_session_signals(session_id)
            .await
            .unwrap_or_else(|| serde_json::json!({ "session_id": session_id }));

        Ok(json!({
            "count": results.len(),
            "collections": collections,
            "retrieval_mode": profile.mode.as_str(),
            "rerank_enabled": profile.rerank_enabled,
            "kiro_lsp_state_dir": profile.kiro_lsp_state_dir,
            "session_id": session_id,
            "signals": serde_to_simd(&signals),
            "results": results_to_simd(&results),
            "retrieval_error": retrieval_error,
        }))
    }
}

// ─── code_index ─────────────────────────────────────────────────────────────

pub struct CodeIndexTool {
    rag: Arc<RagPipeline>,
    collection: String,
}

#[async_trait]
impl Tool for CodeIndexTool {
    fn name(&self) -> &str {
        "code_index"
    }

    fn description(&self) -> &str {
        "Index code into the semantic store. mode='source' indexes a single in-memory file \
         (repo, file_path, content) for live workspace updates; mode='repomix_zip' ingests an \
         entry from a repomix zip (zip_path, entry)."
    }

    fn category(&self) -> &str {
        "code"
    }

    fn tags(&self) -> Vec<String> {
        vec!["code".into(), "index".into(), "ingest".into()]
    }

    fn input_schema(&self) -> Value {
        tool_input_schema("code_index")
    }

    fn required_capability(&self) -> Option<&str> {
        Some("cognitive_mcp.invoke")
    }

    fn subid(&self) -> Option<&str> {
        Some("mut.service.code-rag.index@v1")
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let collection = input
            .get("collection")
            .and_then(Value::as_str)
            .unwrap_or(&self.collection);
        let mode = input
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("source");

        let stats = match mode {
            "source" => {
                let repo = input
                    .get("repo")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("source mode requires 'repo'"))?;
                let file_path = input
                    .get("file_path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("source mode requires 'file_path'"))?;
                let content = input
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("source mode requires 'content'"))?;
                self.rag
                    .ingest_source_text(repo, file_path, content, collection)
                    .await?
            }
            "repomix_zip" => {
                let zip_path = input
                    .get("zip_path")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("repomix_zip mode requires 'zip_path'"))?;
                let entry = input
                    .get("entry")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("repomix_zip mode requires 'entry'"))?;
                self.rag
                    .ingest_repomix_entry(&PathBuf::from(zip_path), entry, collection)
                    .await?
            }
            other => return Err(anyhow::anyhow!("unknown mode: {other}")),
        };

        Ok(json!({
            "ok": true,
            "mode": mode,
            "collection": collection,
            "files_parsed": stats.files_parsed as u64,
            "chunks_created": stats.chunks_created as u64,
            "chunks_upserted": stats.chunks_upserted as u64,
            "chunks_skipped": stats.chunks_skipped as u64,
            "errors": stats.errors as u64,
        }))
    }
}

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Derive a tool's JSON input schema from the single canonical authority
/// (`op_plugins::cognitive_mcp_plugin_schema`). The tool never declares its
/// schema inline — the OSCAL subid and field contract live in the PluginSchema.
fn tool_input_schema(field: &str) -> Value {
    op_plugins::cognitive_mcp_plugin_schema()
        .field_input_schema(field)
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }))
}

fn filter_from(input: &Value, default_exclude_tests: bool) -> CodeFilter {
    CodeFilter {
        repo: input.get("repo").and_then(Value::as_str).map(String::from),
        language: input
            .get("language")
            .and_then(Value::as_str)
            .map(String::from),
        file_type: input
            .get("file_type")
            .and_then(Value::as_str)
            .map(String::from),
        path_contains: input
            .get("path_contains")
            .and_then(Value::as_str)
            .map(String::from),
        symbol_contains: input
            .get("symbol_contains")
            .and_then(Value::as_str)
            .map(String::from),
        exclude_tests: input
            .get("exclude_tests")
            .and_then(Value::as_bool)
            .unwrap_or(default_exclude_tests),
    }
}

fn retrieval_profile_from(input: &Value, default_mode: RetrievalMode) -> RetrievalProfile {
    let mode = input
        .get("retrieval_mode")
        .or_else(|| input.get("mode"))
        .and_then(Value::as_str)
        .map(RetrievalMode::parse)
        .unwrap_or(default_mode);
    RetrievalProfile::from_env(mode)
}

fn collections_from(input: &Value, fallback: &str, profile: &RetrievalProfile) -> Vec<String> {
    if let Some(collection) = input.get("collection").and_then(Value::as_str) {
        return vec![collection.to_string()];
    }

    if let Some(collections) = input.get("collections").and_then(Value::as_array) {
        let parsed: Vec<String> = collections
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect();
        if !parsed.is_empty() {
            return parsed;
        }
    }

    if !profile.collections.is_empty() {
        return profile.collections.clone();
    }

    vec![fallback.to_string()]
}

fn results_to_simd(results: &[RagResult]) -> Value {
    let serde_val = serde_json::to_value(results).unwrap_or(serde_json::Value::Null);
    serde_to_simd(&serde_val)
}

fn serde_to_simd(v: &serde_json::Value) -> Value {
    let s = serde_json::to_string(v).unwrap_or_default();
    let mut buf = s.into_bytes();
    simd_json::from_slice(&mut buf).unwrap_or(Value::Static(simd_json::StaticNode::Null))
}
