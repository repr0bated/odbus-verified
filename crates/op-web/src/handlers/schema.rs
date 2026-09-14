use crate::state::AppState;
use axum::{extract::Extension, response::Json};
use simd_json::json;
use simd_json::prelude::ValueAsContainer;
use simd_json::OwnedValue as Value;
use std::sync::Arc;

/// Catalog manifest published by the blob sealer — the real present-state of
/// which plugin schemas exist and the canonical catalog_hash. Per CLAUDE.md
/// the monolithic `/dev/shm/live-schema.json` is gone; the blob catalog (one
/// blob per plugin) is the source of truth.
const BLOB_MANIFEST_PATH: &str = "/dev/shm/opdbus/plugin-blobs/.manifest.json";

pub async fn schema_catalog_handler(Extension(_state): Extension<Arc<AppState>>) -> Json<Value> {
    match read_schema_catalog_bytes() {
        Ok(mut bytes) => match simd_json::to_owned_value(&mut bytes) {
            Ok(value) => {
                let count = if let Some(obj) = value.as_object() {
                    obj.get("plugins")
                        .and_then(|p| p.as_object())
                        .map(|p| p.len())
                        .unwrap_or(0)
                } else {
                    0
                };
                Json(json!({
                    "success": true,
                    "catalog": value,
                    "total": count
                }))
            }
            Err(e) => Json(json!({
                "success": false,
                "error": format!("Failed to parse schema: {}", e)
            })),
        },
        Err(e) => Json(json!({
            "success": false,
            "error": format!("Schema catalog not available: {}", e)
        })),
    }
}

pub async fn schema_handler(Extension(_state): Extension<Arc<AppState>>) -> Json<Value> {
    match read_schema_catalog_bytes() {
        Ok(mut bytes) => match simd_json::to_owned_value(&mut bytes) {
            Ok(value) => Json(value),
            Err(_) => Json(json!({})),
        },
        Err(_) => Json(json!({})),
    }
}

fn read_schema_catalog_bytes() -> std::io::Result<Vec<u8>> {
    std::fs::read(BLOB_MANIFEST_PATH)
}
