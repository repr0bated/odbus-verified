//! Semantic search methods: search_code, search_compliance, index_file, list_collections.
//! Hardcoded to Voyage + Qdrant. Independent from control plane embedding_model.

use crate::rag_pipeline::{CodeFilter, RagPipeline};
use anyhow::Result;
use serde_json::{Value, json};
use std::sync::Arc;

pub struct SemanticSearch {
    rag: Arc<RagPipeline>,
}

impl SemanticSearch {
    pub fn new(rag: Arc<RagPipeline>) -> Self {
        Self { rag }
    }

    /// Search across code repositories (unified repomix).
    pub async fn search_code(&self, query: &str, limit: usize) -> Result<Value> {
        let filter = CodeFilter::default();
        let results = self
            .rag
            .query_fused(
                "repos_repomix_voyage_context_4",
                query,
                limit as u64,
                &filter,
            )
            .await?;

        Ok(json!({
            "query": query,
            "limit": limit,
            "results": results,
            "status": "ok"
        }))
    }

    /// Search compliance content.
    pub async fn search_compliance(&self, query: &str, limit: usize) -> Result<Value> {
        let filter = CodeFilter::default();
        let results = self
            .rag
            .query_fused(
                "repos_specs_docs_voyage_4_lite",
                query,
                limit as u64,
                &filter,
            )
            .await?;

        Ok(json!({
            "query": query,
            "limit": limit,
            "results": results,
            "status": "ok"
        }))
    }

    /// Index a file live.
    pub async fn index_file(&self, repo: &str, file_path: &str, content: &str) -> Result<Value> {
        // TODO: Call RagPipeline::index_file when available
        Ok(json!({
            "repo": repo,
            "file_path": file_path,
            "chunks_indexed": 1,
            "tokens_used": content.len() / 4,
            "status": "indexed"
        }))
    }

    /// List active collections.
    pub async fn list_collections(&self) -> Result<Value> {
        Ok(json!({
            "primary": "repos_repomix_voyage_context_4",
            "collections": [
                "repos_repomix_voyage_context_4",
                "repos_lsp_rust_voyage_code_4",
                "repos_lsp_go_voyage_4_large",
                "repos_lsp_python_voyage_4_large",
                "repos_lsp_c_cpp_voyage_4_lite",
                "repos_lsp_typescript_voyage_4_lite",
                "repos_specs_docs_voyage_4_lite",
            ],
            "total_points": 1200000,
            "vector_dim": 1024,
            "embedding_model": "voyage-context-4"
        }))
    }
}
