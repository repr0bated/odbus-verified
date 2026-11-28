//! Exchange point (owner, 2026-11-22; Requirement 34, task 27.5): one place on
//! the door, beside the gRPC services, where a UI sets variables and receives
//! the schema. It holds zero logic of its own:
//!
//! * identity: the same SID1 verification as every door call
//!   (`BridgeService::authenticate`);
//! * set variables: forwarded unchanged to the existing sealed methods
//!   (`project.select_context`, `chat_router.SetSelection`) through
//!   `Runtime::call`, so each set is admitted, executed and recorded by the
//!   execution tracker like any tool call (and reaches the sled only from the
//!   tracker commit, personal filtered);
//! * receive schema: the bridge selects it from the sealed catalog by the
//!   caller's exact grants; versioning is the catalog's own (blob
//!   `FORMAT_VERSION`, manifest `catalog_hash`/`previous_catalog_hash`/
//!   `generation`, per-plugin `schema_hash` and `version`); allowed chat modes
//!   come from the same `chat_mode_policy` function as admission.
//!
//! No new subid, method or catalog entry is introduced.

use axum::Json;
use axum::http::{HeaderMap, StatusCode};
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::sync::Arc;

use crate::runtime::{Runtime, VerifiedCaller};
use crate::service::BridgeService;

pub(crate) const PATH: &str = "/exchange/v1";

type Reply = (StatusCode, Json<Value>);

fn refuse(status: StatusCode, message: &str) -> Reply {
    (status, Json(json!({"error": message})))
}

async fn verified(
    service: &BridgeService,
    headers: HeaderMap,
) -> Result<(Arc<Runtime>, VerifiedCaller), Reply> {
    let runtime = service
        .runtime_handle()
        .ok_or_else(|| refuse(StatusCode::SERVICE_UNAVAILABLE, "runtime unavailable"))?;
    let mut request = tonic::Request::new(());
    *request.metadata_mut() = tonic::metadata::MetadataMap::from_headers(headers);
    let caller = service
        .authenticate(&request)
        .await
        .map_err(|status| refuse(StatusCode::UNAUTHORIZED, status.message()))?;
    Ok((runtime, caller))
}

fn manifest(runtime: &Runtime) -> Value {
    let manifest = std::fs::read_to_string(runtime.catalog_dir().join(".manifest.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_default();
    json!({
        "blob_format_version": op_blob::blob::FORMAT_VERSION,
        "catalog_hash": runtime.catalog().hash(),
        "manifest_catalog_hash": manifest["catalog_hash"],
        "previous_catalog_hash": manifest["previous_catalog_hash"],
        "generation": manifest["generation"],
    })
}

/// The schema the bridge delivers: every sealed method the caller's exact
/// grants admit, grouped by plugin with its versioning.
async fn schema(runtime: &Runtime, caller: &VerifiedCaller) -> Result<Value, Reply> {
    let grants: BTreeSet<String> = runtime
        .current_grants(caller)
        .await
        .map_err(|_| refuse(StatusCode::FORBIDDEN, "grants unavailable"))?
        .into_iter()
        .collect();
    let mut plugins = Vec::new();
    for plugin in runtime.catalog().plugins() {
        let mut methods = Vec::new();
        for method in &plugin.methods {
            let Ok((_, declaration)) = runtime.pinned_method(&plugin.id, &method.name) else {
                continue;
            };
            let Some(capability) = declaration.required_capability.as_deref() else {
                continue;
            };
            if grants.contains(capability) {
                methods.push(
                    json!({"name": method.name, "required_capability": capability,
                    "subid": declaration.subid}),
                );
            }
        }
        if !methods.is_empty() {
            plugins.push(json!({"plugin_id": plugin.id, "version": plugin.version,
                "schema_hash": plugin.schema_hash, "methods": methods}));
        }
    }
    let store = runtime.identity_store().clone();
    let session = caller.session_id.clone();
    let (kind, _) = tokio::task::spawn_blocking(move || {
        crate::chat_mode_policy::session_identity(&store, &session)
    })
    .await
    .map_err(|_| {
        refuse(
            StatusCode::INTERNAL_SERVER_ERROR,
            "identity owner unavailable",
        )
    })?
    .map_err(|_| {
        refuse(
            StatusCode::INTERNAL_SERVER_ERROR,
            "identity owner unavailable",
        )
    })?;
    let current = runtime
        .call(
            caller.clone(),
            "project",
            "get_context",
            json!({}),
            uuid::Uuid::new_v4(),
        )
        .await
        .map(|completed| completed.result)
        .unwrap_or(Value::Null);
    Ok(json!({
        "exchange_point": "v1",
        "catalog": manifest(runtime),
        "session": {"principal_id": caller.principal_id, "session_id": caller.session_id},
        "modes": {
            "allowed": crate::chat_mode_policy::allowed_modes(&grants, kind),
            "default": crate::chat_mode_policy::default_mode(&grants, kind),
            "current": current,
        },
        "plugins": plugins,
    }))
}

/// GET: receive the schema.
pub(crate) async fn read(service: BridgeService, headers: HeaderMap) -> Reply {
    let (runtime, caller) = match verified(&service, headers).await {
        Ok(verified) => verified,
        Err(reply) => return reply,
    };
    match schema(&runtime, &caller).await {
        Ok(body) => (StatusCode::OK, Json(body)),
        Err(reply) => reply,
    }
}

/// Map selection variables to the same sealed calls on both doors.
pub(crate) fn selection_calls(
    fields: &serde_json::Map<String, Value>,
) -> Vec<(&'static str, &'static str, Value)> {
    let mut calls = Vec::new();
    if fields.contains_key("mode") {
        let mut args = serde_json::Map::new();
        for name in ["mode", "project_id", "personal_namespace"] {
            if let Some(value) = fields.get(name) {
                args.insert(name.into(), value.clone());
            }
        }
        calls.push(("project", "select_context", Value::Object(args)));
    }
    if fields.contains_key("provider_id") || fields.contains_key("model_id") {
        let mut args = serde_json::Map::new();
        for name in ["provider_id", "model_id"] {
            if let Some(value) = fields.get(name) {
                args.insert(name.into(), value.clone());
            }
        }
        calls.push(("chat_router", "SetSelection", Value::Object(args)));
    }
    calls
}

/// POST: set variables, then receive the schema. Each variable group is one
/// existing sealed call; the bridge, not this endpoint, validates it.
pub(crate) async fn set(
    service: BridgeService,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Reply {
    let (runtime, caller) = match verified(&service, headers).await {
        Ok(verified) => verified,
        Err(reply) => return reply,
    };
    let Some(fields) = body.as_object() else {
        return refuse(StatusCode::BAD_REQUEST, "variables must be one object");
    };
    let calls = selection_calls(fields);
    for (plugin, method, args) in calls {
        if let Err(error) = runtime
            .call(caller.clone(), plugin, method, args, uuid::Uuid::new_v4())
            .await
        {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({"error": error.to_string(), "plugin_id": plugin, "method": method})),
            );
        }
    }
    match schema(&runtime, &caller).await {
        Ok(body) => (StatusCode::OK, Json(body)),
        Err(reply) => reply,
    }
}
