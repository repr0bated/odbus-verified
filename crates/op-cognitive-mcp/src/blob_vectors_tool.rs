//! MCP tools for the blob-vector Qdrant collection.
//!
//! `refresh_blob_vectors` — user-triggered wholesale rebuild of the
//!   `blob_vectors` collection (one embedding per active plugin schema).
//!
//! `search_blob_vectors` — semantic search over the collection using a
//!   free-text query; backing call for the "Vectors" context modifier
//!   described in `.kiro/specs/unified-blob-catalog-mcp/`.

use crate::cognitive_tools::field;
use anyhow::{Context, Result};
use async_trait::async_trait;
use op_mcp::tool_registry::{BoxedTool, Tool, ToolRegistry};
use simd_json::prelude::ValueAsScalar;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;

use crate::qdrant_shuttle::QdrantSemanticShuttle;

pub async fn register_blob_vectors_tools(
    registry: &ToolRegistry,
    qdrant: Option<Arc<QdrantSemanticShuttle>>,
) -> Result<()> {
    registry
        .register(Arc::new(RefreshBlobVectorsTool {
            shuttle: qdrant.clone(),
        }) as BoxedTool)
        .await?;
    registry
        .register(Arc::new(SearchBlobVectorsTool { shuttle: qdrant }) as BoxedTool)
        .await?;
    Ok(())
}

// ── refresh_blob_vectors ─────────────────────────────────────────────────────

struct RefreshBlobVectorsTool {
    shuttle: Option<Arc<QdrantSemanticShuttle>>,
}

#[async_trait]
impl Tool for RefreshBlobVectorsTool {
    fn name(&self) -> &str {
        "refresh_blob_vectors"
    }

    fn description(&self) -> &str {
        "Rebuild the blob_vectors Qdrant collection from scratch: embeds every \
         active plugin's schema text via Voyage and upserts all points. \
         User-triggered only — never runs automatically."
    }

    fn category(&self) -> &str {
        "schema"
    }

    fn namespace(&self) -> &str {
        "plugins"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "schema".to_string(),
            "blob".to_string(),
            "vectors".to_string(),
            "refresh".to_string(),
        ]
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "additionalProperties": false})
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        let shuttle = self.shuttle.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "refresh_blob_vectors unavailable: Qdrant Semantic Shuttle is not configured"
            )
        })?;
        let summary = shuttle
            .refresh_blob_vectors()
            .await
            .context("refresh_blob_vectors failed")?;
        Ok(json!({
            "ok": true,
            "embedded": summary.embedded,
            "collection": summary.collection,
        }))
    }
}

// ── search_blob_vectors ──────────────────────────────────────────────────────

struct SearchBlobVectorsTool {
    shuttle: Option<Arc<QdrantSemanticShuttle>>,
}

#[async_trait]
impl Tool for SearchBlobVectorsTool {
    fn name(&self) -> &str {
        "search_blob_vectors"
    }

    fn description(&self) -> &str {
        "Semantic search over the blob_vectors collection. Returns the top-k \
         plugin schemas most relevant to the query. Backing call for the \
         Vectors context modifier; also usable standalone."
    }

    fn category(&self) -> &str {
        "schema"
    }

    fn namespace(&self) -> &str {
        "plugins"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "schema".to_string(),
            "blob".to_string(),
            "vectors".to_string(),
            "search".to_string(),
        ]
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Free-text query — embedded with voyage-4 for similarity search"
                },
                "limit": {
                    "type": "integer",
                    "default": 10,
                    "minimum": 1,
                    "description": "Maximum number of results to return"
                }
            },
            "additionalProperties": false
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let shuttle = self.shuttle.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "search_blob_vectors unavailable: Qdrant Semantic Shuttle is not configured"
            )
        })?;
        let query = field(&input, "query")
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing required field: query"))?;
        let limit = field(&input, "limit").as_i64().unwrap_or(10).max(1) as u64;

        let results = shuttle
            .search_blob_vectors(query, limit)
            .await
            .context("search_blob_vectors failed")?;

        let items: Vec<Value> = results
            .into_iter()
            .map(|pt| {
                let plugin_id = pt
                    .payload
                    .get("plugin_id")
                    .and_then(|v| v.as_str())
                    .cloned()
                    .unwrap_or_default();
                let text = pt
                    .payload
                    .get("text")
                    .and_then(|v| v.as_str())
                    .cloned()
                    .unwrap_or_default();
                json!({
                    "plugin_id": plugin_id,
                    "score": pt.score,
                    "text": text,
                })
            })
            .collect();

        Ok(json!({ "count": items.len(), "results": items }))
    }
}
