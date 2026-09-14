//! Read-only access to one projected identity session.
//!
//! `GET /api/identity/sled?session_id=...` retains the existing route while
//! resolving a session-scoped `identity_sled` record. Omitting the selector is
//! accepted only when the projection contains exactly one current session.

use axum::{
    body::Body,
    extract::{Extension, Query},
    http::{header, StatusCode},
    response::Response,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tracing::{info, warn};

use crate::state::AppState;

#[derive(Debug, Default, Deserialize)]
pub struct IdentityQuery {
    pub session_id: Option<String>,
}

/// Return a single active, unexpired identity record from the shared projection.
pub async fn identity_sled_handler(
    Extension(_state): Extension<Arc<AppState>>,
    Query(query): Query<IdentityQuery>,
) -> Response {
    match op_identity::resolve_identity_session(query.session_id.as_deref()) {
        Ok(identity) => {
            let genesis = identity.genesis.clone().unwrap_or_default();
            let schema_catalog_hash = op_identity::schema_bridge::schema_catalog_hash()
                .map(hex::encode)
                .unwrap_or_else(|| "(missing)".to_string());
            let projection_path = op_core::projection_shm::projection_file_path("identity_sled");

            let body = json!({
                "path": projection_path,
                "is_valid": identity.is_current(),
                "session_id": identity.session_id,
                "wireguard_pubkey": identity.wireguard_pubkey,
                "mutation_index": identity.mutation_index,
                "genesis": genesis,
                "trace_id": identity.trace_id,
                "schema_version": identity.schema_version,
                "schema_catalog_hash": schema_catalog_hash,
                "backend": false,
            });

            info!("Served projected identity session");
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&body).expect("identity body serialization should not fail"),
                ))
                .expect("response with valid body should not fail")
        }
        Err(error) => {
            warn!(%error, "Identity session not available");
            Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({
                        "error": "Identity session not available",
                        "detail": error.to_string(),
                    })
                    .to_string(),
                ))
                .expect("response with valid body should not fail")
        }
    }
}
