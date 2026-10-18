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
//! - `code_index`   — live indexing of a source file.
//!   subid: `src.software.workspace.index@v1`

use crate::context_awareness::{ActivityType, ContextAwarenessEngine};
use crate::rag_pipeline::{
    CodeFilter, RagPipeline, RagResult, RetrievalMode, RetrievalProfile, with_retrieval_deadline,
};
use anyhow::Result;
use async_trait::async_trait;
use op_mcp::tool_registry::{BoxedTool, Tool, ToolRegistry};
use simd_json::prelude::*;
use simd_json::{OwnedValue as Value, json};
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
        Some("cognitive.read")
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

        let results = with_retrieval_deadline(profile.deadline, async {
            if collections.len() > 1 {
                self.rag
                    .query_fused_collections_unbounded(
                        &collections,
                        query,
                        limit,
                        profile.fetch_limit,
                        &filter,
                    )
                    .await
            } else if input.get("fused").and_then(Value::as_bool).unwrap_or(true) {
                self.rag
                    .query_fused(&collections[0], query, limit, &filter)
                    .await
            } else {
                self.rag
                    .query_filtered(&collections[0], query, limit, &filter)
                    .await
            }
        })
        .await?;

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
        Some("cognitive.read")
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
                serde_json::json!({
                    "source": "code_context",
                    "memory_namespace": input.get("memory_namespace").and_then(Value::as_str),
                }),
            )
            .await;

        let filter = filter_from(&input, false);
        let collections = collections_from(&input, &self.collection, &profile);
        let (results, retrieval_error) = match with_retrieval_deadline(
            profile.deadline,
            self.rag.query_fused_collections_unbounded(
                &collections,
                query,
                limit,
                profile.fetch_limit,
                &filter,
            ),
        )
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
        "Index code into the semantic store: indexes a single in-memory file \
         (repo, file_path, content) for live workspace updates."
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
        Some("cognitive.code.write")
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
/// (`op_plugins::cognitive_plugin_schema`). The tool never declares its
/// schema inline — the OSCAL subid and field contract live in the PluginSchema.
fn tool_input_schema(field: &str) -> Value {
    op_plugins::cognitive_plugin_schema()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_awareness::{ContextAwarenessConfig, ContextAwarenessEngine};
    use crate::cozo_shuttle::CozoGraphShuttle;
    use crate::memory_store::CognitiveMemoryStore;
    use crate::voyage::VoyageClient;
    use op_plugins::state_plugins::cognitive::CodeContextOutput;
    use qdrant_client::Qdrant;
    struct RestoreEnvironment(Vec<(&'static str, Option<std::ffi::OsString>)>);

    impl Drop for RestoreEnvironment {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                match value {
                    // FIXME: Audit that the environment access only happens in single-threaded code.
                    Some(value) => unsafe { std::env::set_var(key, value) },
                    // FIXME: Audit that the environment access only happens in single-threaded code.
                    None => unsafe { std::env::remove_var(key) },
                }
            }
        }
    }

    #[tokio::test]
    async fn code_search_single_and_fused_branches_obey_deadline() {
        let _restore = RestoreEnvironment(
            [
                "COGNITIVE_SEARCH_DEADLINE_MS",
                "COGNITIVE_COMPLETION_DEADLINE_MS",
                "EMBEDDING_VOYAGE_CREDENTIAL_ORIGIN",
            ]
            .into_iter()
            .map(|key| (key, std::env::var_os(key)))
            .collect(),
        );
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("COGNITIVE_SEARCH_DEADLINE_MS", "100") };
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("COGNITIVE_COMPLETION_DEADLINE_MS", "100") };
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe {
            std::env::set_var(
                "EMBEDDING_VOYAGE_CREDENTIAL_ORIGIN",
                format!("https://127.0.0.1:{port}"),
            )
        };
        let acceptor = tokio::spawn(async move {
            let mut sockets = Vec::new();
            for _ in 0..8 {
                if let Ok((socket, _)) = listener.accept().await {
                    sockets.push(socket);
                }
            }
            sockets
        });
        let qdrant_url = format!("http://127.0.0.1:{port}");
        let qdrant = Qdrant::from_url(&qdrant_url)
            .skip_compatibility_check()
            .build()
            .unwrap();
        let rag = RagPipeline::for_test(
            qdrant,
            VoyageClient::from_params(
                format!("https://127.0.0.1:{port}/v1/embeddings"),
                "fixture-key".into(),
                "fixture-model".into(),
            )
            .unwrap(),
        );
        let rag = Arc::new(rag);
        let tool = CodeSearchTool {
            rag: rag.clone(),
            collection: "test".into(),
        };
        for fused in [true, false] {
            let error = tool
                .execute(json!({
                    "query": "deadline",
                    "collection": "test",
                    "fused": fused
                }))
                .await
                .unwrap_err();
            assert!(error.to_string().contains("deadline exceeded"));
        }
        let error = tool
            .execute(json!({
                "query": "deadline",
                "collections": ["one", "two"]
            }))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("deadline exceeded"), "{error:#}");

        let cozo = Arc::new(CozoGraphShuttle::new_in_memory().unwrap());
        let memory = Arc::new(CognitiveMemoryStore::new(cozo).await.unwrap());
        let engine = Arc::new(ContextAwarenessEngine::new(
            ContextAwarenessConfig::default(),
            memory,
            None,
        ));
        let context = CodeContextTool {
            rag,
            engine,
            collection: "test".into(),
        };
        let output = context
            .execute(json!({"query": "deadline", "session_id": "test"}))
            .await
            .unwrap();
        let output_json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&output).unwrap()).unwrap();
        let typed: CodeContextOutput = serde_json::from_value(output_json).unwrap();
        assert_eq!(typed.count, 0);
        assert!(
            typed
                .retrieval_error
                .as_deref()
                .unwrap()
                .contains("deadline exceeded")
        );
        acceptor.abort();
    }
}
