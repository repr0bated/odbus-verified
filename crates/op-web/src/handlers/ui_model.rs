//! Authenticated HTTP compatibility for the canonical private Gemma workspace.
//!
//! This module owns no gallery files, catalog, identity, or state cache. Every
//! retained CRUD operation enters the schema-declared bridge method with the
//! exact request SID1. Unsupported promoted-catalog operations stay explicit.

use crate::state::AppState;
use axum::{
    extract::{Extension, Path, Query},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use simd_json::prelude::*;
use std::sync::Arc;

const PLUGIN: &str = "gemma_brain";
const READ_CAPABILITY: &str = "gemma.read";
const WRITE_CAPABILITY: &str = "gemma.invoke";

#[derive(Debug, PartialEq)]
struct WorkspaceCall {
    method: &'static str,
    capability: &'static str,
    arguments: Value,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListSpecsQuery {
    pub after: Option<String>,
    #[serde(default)]
    pub limit: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteSpecRequest {
    pub expected_revision: u64,
}

fn list_specs_call(query: ListSpecsQuery) -> WorkspaceCall {
    WorkspaceCall {
        method: "list_ui_specs",
        capability: READ_CAPABILITY,
        arguments: json!({"after":query.after,"limit":query.limit}),
    }
}

fn delete_spec_call(spec_id: String, request: DeleteSpecRequest) -> WorkspaceCall {
    WorkspaceCall {
        method: "delete_ui_spec",
        capability: WRITE_CAPABILITY,
        arguments: json!({
            "spec_id":spec_id,
            "expected_revision":request.expected_revision,
        }),
    }
}

fn json_response(status: StatusCode, value: Value) -> Response {
    (status, Json(value)).into_response()
}

fn unavailable(operation: &str) -> Response {
    json_response(
        StatusCode::NOT_IMPLEMENTED,
        json!({
            "error":"Promoted UI galleries have no sealed owner contract",
            "code":"gallery_contract_unavailable",
            "operation":operation,
        }),
    )
}

fn bridge_error(error: anyhow::Error) -> Response {
    let message = error.to_string();
    let (status, code) = if message.contains("revision") || message.contains("pending") {
        (StatusCode::CONFLICT, "workspace_conflict")
    } else if message.contains("absent") || message.contains("not found") {
        (StatusCode::NOT_FOUND, "workspace_item_absent")
    } else if message.contains("Unauthenticated") {
        (StatusCode::UNAUTHORIZED, "identity_rejected")
    } else if message.contains("PermissionDenied") || message.contains("capability") {
        (StatusCode::FORBIDDEN, "capability_rejected")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "workspace_unavailable")
    };
    json_response(
        status,
        json!({"error":"Canonical UI workspace request failed","code":code}),
    )
}

async fn call_workspace(
    headers: &HeaderMap,
    method: &str,
    capability: &str,
    args: Value,
) -> Result<Value, Response> {
    let identity = super::tched_router::sid1_metadata(headers)?;
    let mut bytes = serde_json::to_vec(&args)
        .map_err(|_| json_response(StatusCode::BAD_REQUEST, json!({"error":"Invalid request"})))?;
    let arguments = simd_json::to_owned_value(&mut bytes)
        .map_err(|_| json_response(StatusCode::BAD_REQUEST, json!({"error":"Invalid request"})))?;
    let envelope = crate::state_manager_client::call_plugin_method(
        PLUGIN, method, arguments, capability, &identity,
    )
    .await
    .map_err(bridge_error)?;
    let payload = envelope.get("result").cloned().ok_or_else(|| {
        json_response(
            StatusCode::BAD_GATEWAY,
            json!({"error":"Canonical UI workspace response is missing its result"}),
        )
    })?;
    let encoded = simd_json::to_string(&payload).map_err(|_| {
        json_response(
            StatusCode::BAD_GATEWAY,
            json!({"error":"Canonical UI workspace response cannot be encoded"}),
        )
    })?;
    serde_json::from_str(&encoded).map_err(|_| {
        json_response(
            StatusCode::BAD_GATEWAY,
            json!({"error":"Canonical UI workspace response is invalid"}),
        )
    })
}

/// GET /api/ui-model/gallery?after=<spec-id>&limit=<n>
///
/// This is a breaking compatibility response: it returns exact
/// `UiListSpecsOutput`, never the former prompt/signature gallery DTO.
pub async fn ui_model_gallery_handler(
    Extension(_state): Extension<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListSpecsQuery>,
) -> Response {
    let call = list_specs_call(query);
    match call_workspace(&headers, call.method, call.capability, call.arguments).await {
        Ok(result) => Json(result).into_response(),
        Err(response) => response,
    }
}

/// DELETE /api/ui-model/gallery/:id with `{ "expected_revision": n }`.
pub async fn ui_model_gallery_delete_handler(
    Extension(_state): Extension<Arc<AppState>>,
    headers: HeaderMap,
    Path(spec_id): Path<String>,
    Json(request): Json<DeleteSpecRequest>,
) -> Response {
    let call = delete_spec_call(spec_id, request);
    match call_workspace(&headers, call.method, call.capability, call.arguments).await {
        Ok(result) => Json(result).into_response(),
        Err(response) => response,
    }
}

pub async fn ui_model_catalog_handler(Extension(_state): Extension<Arc<AppState>>) -> Response {
    unavailable("list")
}

pub async fn ui_model_catalog_promote_handler(
    Extension(_state): Extension<Arc<AppState>>,
    Path(_id): Path<String>,
) -> Response {
    unavailable("promote")
}

pub async fn ui_model_catalog_delete_handler(
    Extension(_state): Extension<Arc<AppState>>,
    Path(_id): Path<String>,
) -> Response {
    unavailable("delete")
}

/// Retained compatibility path delegates to the exact canonical route.
pub async fn ui_model_plugin_schema_handler(
    Extension(_state): Extension<Arc<AppState>>,
    Path(plugin): Path<String>,
) -> Response {
    super::plugin_schema::plugin_schema_handler(Path(plugin))
        .await
        .into_response()
}

/// Retained compatibility path delegates to the exact canonical route.
pub async fn ui_model_list_plugins_handler(
    Extension(_state): Extension<Arc<AppState>>,
) -> Response {
    super::plugin_schema::plugin_list_handler()
        .await
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_catalog_is_explicit_and_not_an_empty_gallery() {
        let response = unavailable("promote");
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    }

    #[test]
    fn delete_requires_an_explicit_revision_and_rejects_extra_fields() {
        assert!(
            serde_json::from_value::<DeleteSpecRequest>(json!({"expected_revision":3})).is_ok()
        );
        assert!(serde_json::from_value::<DeleteSpecRequest>(json!({})).is_err());
        assert!(serde_json::from_value::<DeleteSpecRequest>(
            json!({"expected_revision":3,"latest":true})
        )
        .is_err());

        let call = delete_spec_call(
            "spec-a".into(),
            DeleteSpecRequest {
                expected_revision: 3,
            },
        );
        assert_eq!(call.method, "delete_ui_spec");
        assert_eq!(call.capability, "gemma.invoke");
        assert_eq!(
            call.arguments,
            json!({"spec_id":"spec-a","expected_revision":3})
        );
    }

    #[test]
    fn list_query_is_bounded_by_canonical_method_not_a_hidden_http_default() {
        let query: ListSpecsQuery = serde_json::from_value(json!({})).unwrap();
        assert_eq!(query.limit, 0);
        assert!(query.after.is_none());
        assert!(serde_json::from_value::<ListSpecsQuery>(json!({"owner":"other"})).is_err());

        let call = list_specs_call(query);
        assert_eq!(call.method, "list_ui_specs");
        assert_eq!(call.capability, "gemma.read");
        assert_eq!(call.arguments, json!({"after":null,"limit":0}));
    }
}
