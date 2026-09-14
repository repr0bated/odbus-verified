//! MCP tool exposing the full sealed-blob plugin catalog as one payload.
//!
//! Per-plugin tools (`read_plugin_schema_shm(id)`) only ever show a model one
//! blob at a time, so it can never reason about relationships *across*
//! plugins. This tool dumps every active plugin's schema in a single
//! response so the calling model can find cross-blob connections itself —
//! e.g. for UI generation that combines fields/state from unrelated plugins
//! in ways a fixed per-plugin template never would.

use anyhow::{Context, Result};
use async_trait::async_trait;
use op_mcp::tool_registry::{BoxedTool, Tool, ToolRegistry};
use serde_json::{json, Value};
use std::sync::Arc;

pub async fn register_blob_catalog_tool(registry: &ToolRegistry) -> Result<()> {
    registry
        .register(Arc::new(BlobCatalogTool) as BoxedTool)
        .await
}

struct BlobCatalogTool;

#[async_trait]
impl Tool for BlobCatalogTool {
    fn name(&self) -> &str {
        "blob_catalog"
    }

    fn description(&self) -> &str {
        "Return every active plugin's sealed schema (from the SHM blob catalog) in one \
         payload, keyed by plugin id. Unlike per-plugin schema lookups, this gives a model \
         the full catalog at once so it can reason about connections across plugins.\n\
         Use mode='list' to get plugin IDs only (~2KB), then pass specific IDs to \
         mode='full' for complete schemas of just those plugins (prevents truncation)."
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
            "catalog".to_string(),
        ]
    }

    fn input_schema(&self) -> simd_json::OwnedValue {
        let schema = json!({
            "type": "object",
            "properties": {
                "mode": {
                    "type": "string",
                    "enum": ["list", "summary", "full"],
                    "default": "summary",
                    "description": "Output mode: 'list' returns plugin IDs only (~2KB), 'summary' returns lightweight metadata (~50KB), 'full' returns complete schemas"
                },
                "plugin_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional: only return schemas for these specific plugin IDs (empty = all plugins)"
                },
                "category": {
                    "type": "string",
                    "description": "Optional filter: only return plugins matching this category"
                }
            },
            "additionalProperties": false
        });
        simd_json::serde::to_owned_value(&schema).expect("static schema serializes")
    }

    async fn execute(&self, input: simd_json::OwnedValue) -> Result<simd_json::OwnedValue> {
        let input: Value = serde_json::to_value(&input).context("simd_json input -> serde_json")?;
        let mode = input
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("summary");

        let plugin_ids_filter: Option<Vec<String>> = input
            .get("plugin_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            });

        let category_filter = input.get("category").and_then(|v| v.as_str());

        let ids = op_blob::catalog::read_manifest_plugin_ids_shm()
            .context("failed to read plugin blob manifest from SHM catalog")?;

        // If mode is "list", return just the plugin IDs
        if mode == "list" {
            let result = json!({
                "plugin_count": ids.len(),
                "plugin_ids": ids,
            });
            return simd_json::serde::to_owned_value(&result)
                .context("serde_json result -> simd_json");
        }

        let mut plugins = serde_json::Map::new();
        let mut missing = Vec::new();

        // Determine which IDs to process
        let ids_to_process: Vec<String> = match &plugin_ids_filter {
            Some(filter_ids) => {
                // Validate that requested IDs exist
                for id in filter_ids {
                    if !ids.contains(id) {
                        missing.push(id.clone());
                    }
                }
                filter_ids.clone()
            }
            None => ids.clone(),
        };

        for id in &ids_to_process {
            if let Some(schema) = op_blob::catalog::read_plugin_schema_shm(id) {
                // Apply category filter if specified
                if let Some(cat) = category_filter {
                    let schema_value = serde_json::to_value(schema.clone())
                        .with_context(|| format!("failed to serialize schema for '{id}'"))?;
                    if let Some(plugin_category) = schema_value.get("category") {
                        if plugin_category.as_str().unwrap_or("") != cat {
                            continue; // Skip plugins not matching category
                        }
                    }
                }

                if mode == "summary" {
                    // Summary mode: return lightweight metadata only
                    let schema_value = serde_json::to_value(schema.clone())
                        .with_context(|| format!("failed to serialize schema for '{id}'"))?;

                    let mut summary = serde_json::Map::new();
                    summary.insert(
                        "name".to_string(),
                        schema_value.get("name").cloned().unwrap_or(Value::Null),
                    );
                    summary.insert(
                        "description".to_string(),
                        schema_value
                            .get("description")
                            .cloned()
                            .unwrap_or(Value::Null),
                    );
                    summary.insert(
                        "category".to_string(),
                        schema_value.get("category").cloned().unwrap_or(Value::Null),
                    );
                    summary.insert(
                        "version".to_string(),
                        schema_value.get("version").cloned().unwrap_or(Value::Null),
                    );

                    // Count methods and signals
                    let method_count = schema_value
                        .get("methods")
                        .and_then(|m| m.as_object())
                        .map(|m| m.len())
                        .unwrap_or(0);
                    let signal_count = schema_value
                        .get("signals")
                        .and_then(|s| s.as_array())
                        .map(|s| s.len())
                        .unwrap_or(0);

                    summary.insert("method_count".to_string(), json!(method_count));
                    summary.insert("signal_count".to_string(), json!(signal_count));

                    plugins.insert(id.clone(), Value::Object(summary));
                } else {
                    // Full mode: return complete schema
                    let value = serde_json::to_value(schema)
                        .with_context(|| format!("failed to serialize schema for '{id}'"))?;
                    plugins.insert(id.clone(), value);
                }
            } else if plugin_ids_filter.is_some() {
                // Only mark as missing if explicitly requested
                missing.push(id.clone());
            }
        }

        let result = json!({
            "plugin_count": plugins.len(),
            "mode": mode,
            "plugins": Value::Object(plugins),
            "missing_schemas": missing,
        });
        simd_json::serde::to_owned_value(&result).context("serde_json result -> simd_json")
    }
}
