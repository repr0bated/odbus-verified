//! Semantic search methods: search_code, search_compliance, index_file, list_collections.
//! Uses the same Voyage embedding and Cozo index as code retrieval.

use crate::rag_pipeline::{
    CodeFilter, DEFAULT_VOYAGE4_COLLECTIONS, RagPipeline, default_collection_from_env,
};
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

    /// Search across the configured coding collections.
    pub async fn search_code(&self, query: &str, limit: usize) -> Result<Value> {
        let filter = CodeFilter::default();
        let collections: Vec<_> = DEFAULT_VOYAGE4_COLLECTIONS
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        let results = self
            .rag
            .query_fused_collections(
                &collections,
                query,
                limit.min(64) as u64,
                64,
                &filter,
                std::time::Duration::from_secs(30),
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
        let stats = self
            .rag
            .ingest_source_text(repo, file_path, content, &default_collection_from_env())
            .await?;
        Ok(json!({
            "repo": repo,
            "file_path": file_path,
            "chunks_indexed": stats.chunks_upserted,
            "estimated_tokens": content.len().div_ceil(4),
            "errors": stats.errors,
            "status": if stats.errors == 0 { "indexed" } else { "incomplete" }
        }))
    }

    /// List active collections.
    pub async fn list_collections(&self) -> Result<Value> {
        let primary = default_collection_from_env();
        let mut collections = vec![primary.clone()];
        for &collection in DEFAULT_VOYAGE4_COLLECTIONS {
            if collection != primary {
                collections.push(collection.to_owned());
            }
        }
        let mut total_points = 0;
        for collection in &collections {
            total_points += self.rag.indexed_count(collection).await?;
        }
        Ok(json!({
            "primary": primary,
            "collections": collections,
            "total_points": total_points,
            "vector_dim": 1024,
            "embedding_model": self.rag.embedding_model()
        }))
    }
}
