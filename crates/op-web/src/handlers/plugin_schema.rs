//! Handlers to serve plugin identity from the sealed SHM blob catalog.
//! GET /api/plugins             — every plugin in the catalog
//! GET /api/plugin-schema/:id   — one plugin's schema
//!
//! The sealed catalog under `/dev/shm/opdbus/` is the single source of truth: a
//! plugin exists iff its blob is sealed there. These routes are deliberately
//! model-agnostic — nothing here depends on which inference model is in use.

use axum::{extract::Path, Json};
use op_blob::catalog;
use serde_json::{json, Value};

/// Every plugin actually present in the sealed blob catalog.
///
/// This is NOT the same list as "plugins with generated RPC methods" — state-only
/// plugins have zero methods but are very much present as real blobs.
pub async fn plugin_list_handler() -> Result<Json<Value>, String> {
    match catalog::read_manifest_plugin_ids_shm() {
        Some(ids) => Ok(Json(json!({ "plugins": ids }))),
        None => Err("blob catalog manifest unavailable".to_string()),
    }
}

pub async fn plugin_schema_handler(Path(plugin_id): Path<String>) -> Result<Json<Value>, String> {
    match catalog::read_plugin_schema_shm(&plugin_id) {
        Some(schema) => Ok(Json(
            serde_json::to_value(schema).map_err(|e| e.to_string())?,
        )),
        None => Err(format!("Plugin schema not found: {plugin_id}")),
    }
}
