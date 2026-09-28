//! 3tched Router HTTP compatibility handlers and schema-driven UI surface.
//!
//! Port 8080 owns ordinary HTTP. Every model/provider request is adapted into
//! the bridge's schema-declared gRPC method pipeline on port 8090.

use axum::{
    body::Body,
    extract::Extension,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use base64::Engine as _;
use op_grpc_bridge::GhostbridgeCallMetadata;
use serde::Deserialize;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::state::AppState;

#[allow(clippy::result_large_err)]
fn ghostbridge_metadata(headers: &HeaderMap) -> Result<GhostbridgeCallMetadata, Response> {
    let decode = |name| {
        decode_identity_header(headers, name)
            .map_err(|message| json_error_response(StatusCode::BAD_REQUEST, message))
    };
    let sid = decode(op_identity::sealed_id::HTTP_HEADER_NAME)?;
    let oia = decode("x-oracle-identity-assertion-bin")?;
    if sid.is_some() == oia.is_some() {
        return Err(json_error_response(
            StatusCode::UNAUTHORIZED,
            "send exactly one identity credential",
        ));
    }
    Ok(GhostbridgeCallMetadata {
        sealed_id: sid,
        oracle_assertion: oia,
        session_genesis: None,
        trace_id: None,
        wireguard_pubkey: None,
    })
}

#[allow(clippy::result_large_err)]
pub(crate) fn sid1_metadata(headers: &HeaderMap) -> Result<GhostbridgeCallMetadata, Response> {
    let sid = decode_identity_header(headers, op_identity::sealed_id::HTTP_HEADER_NAME)
        .map_err(|message| json_error_response(StatusCode::BAD_REQUEST, message))?
        .ok_or_else(|| {
            json_error_response(StatusCode::UNAUTHORIZED, "SID1 identity is required")
        })?;
    let oracle = decode_identity_header(headers, "x-oracle-identity-assertion-bin")
        .map_err(|message| json_error_response(StatusCode::BAD_REQUEST, message))?;
    if oracle.is_some() || !sid.starts_with(b"SID1") {
        return Err(json_error_response(
            StatusCode::UNAUTHORIZED,
            "private UI workspace requires exactly one SID1 identity",
        ));
    }
    Ok(GhostbridgeCallMetadata {
        sealed_id: Some(sid),
        oracle_assertion: None,
        session_genesis: None,
        trace_id: None,
        wireguard_pubkey: None,
    })
}

fn decode_identity_header(
    headers: &HeaderMap,
    name: &'static str,
) -> Result<Option<Vec<u8>>, &'static str> {
    let values: Vec<_> = headers.get_all(name).iter().collect();
    if values.is_empty() {
        return Ok(None);
    }
    if values.len() != 1 {
        return Err("identity header must occur once");
    }
    let encoded = values[0]
        .to_str()
        .map_err(|_| "identity header is not valid ASCII")?;
    if encoded.len() > (16_usize * 1024 * 4).div_ceil(3) {
        return Err("identity credential is too large");
    }
    let wire = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| "identity header is not canonical base64url")?;
    if wire.is_empty()
        || wire.len() > 16 * 1024
        || base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&wire) != encoded
    {
        return Err("identity header is not canonical base64url");
    }
    Ok(Some(wire))
}

async fn call_tched_router_method(
    headers: &HeaderMap,
    method: &str,
    capability: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, Response> {
    let identity = ghostbridge_metadata(headers)?;
    let mut bytes = serde_json::to_vec(&arguments)
        .map_err(|error| json_error_response(StatusCode::BAD_REQUEST, &error.to_string()))?;
    let arguments = simd_json::to_owned_value(&mut bytes)
        .map_err(|error| json_error_response(StatusCode::BAD_REQUEST, &error.to_string()))?;
    let envelope = crate::state_manager_client::call_plugin_method(
        crate::tched_router_routes::ROUTER_PLUGIN_ID,
        method,
        arguments,
        capability,
        &identity,
    )
    .await
    .map_err(|error| {
        let message = error.to_string();
        let status = if message.contains("Unauthenticated") {
            StatusCode::UNAUTHORIZED
        } else if message.contains("Access denied") || message.contains("PermissionDenied") {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::BAD_GATEWAY
        };
        json_error_response(status, &message)
    })?;
    let payload = envelope.get("result").cloned().ok_or_else(|| {
        json_error_response(
            StatusCode::BAD_GATEWAY,
            "tched_router method response is missing its result payload",
        )
    })?;
    let json = simd_json::to_string(&payload)
        .map_err(|error| json_error_response(StatusCode::BAD_GATEWAY, &error.to_string()))?;
    serde_json::from_str(&json)
        .map_err(|error| json_error_response(StatusCode::BAD_GATEWAY, &error.to_string()))
}

#[derive(Debug, Deserialize)]
pub struct OpenAiChatMessage {
    pub role: String,
    #[serde(default)]
    pub content: String,
}

/// Accept both OpenAI `messages` and the legacy single `message` form.
#[derive(Debug, Deserialize)]
pub struct TchedRouterChatRequest {
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub messages: Vec<OpenAiChatMessage>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub stream: bool,
}

/// OpenAI-compatible chat adapter. The bridge remains the sole router.
pub async fn tched_router_chat_handler(
    headers: HeaderMap,
    Extension(_state): Extension<Arc<AppState>>,
    Json(req): Json<TchedRouterChatRequest>,
) -> Response {
    if req.stream {
        return json_error_response(
            StatusCode::BAD_REQUEST,
            "use op_chat.chat.ChatService.Send on the gRPC bridge for streaming",
        );
    }
    let conversation_id = match req.session_id {
        Some(id) if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) => {
            return json_error_response(StatusCode::BAD_REQUEST, "invalid conversation_id");
        }
        other => other,
    };
    let messages = if req.messages.is_empty() {
        if req.message.is_empty() {
            return json_error_response(StatusCode::BAD_REQUEST, "messages must not be empty");
        }
        vec![serde_json::json!({"role": "user", "content": req.message})]
    } else {
        req.messages
            .into_iter()
            .map(|message| {
                serde_json::json!({
                    "role": message.role,
                    "content": message.content,
                })
            })
            .collect()
    };
    let result = match call_tched_router_method(
        &headers,
        "Chat",
        "cap.software.3tched-router.chat@v1",
        serde_json::json!({
            "messages": messages,
            "provider": req.provider.unwrap_or_default(),
            "model": req.model.unwrap_or_default(),
            "conversation_id": conversation_id,
        }),
    )
    .await
    {
        Ok(result) => result,
        Err(response) => return response,
    };

    let content = result
        .get("content")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let model = result
        .get("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let provider = result
        .get("provider")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let finish_reason = result
        .get("finish_reason")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("stop");
    // Salad (and some other providers) emit usage counts as floats (e.g. 30.0).
    // OpenAI-compatible clients like 3tched Router deserialize them as u64 — coerce.
    let usage = normalize_openai_usage(result.get("usage"));

    Json(serde_json::json!({
        "id": format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "provider": provider,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content,
            },
            "finish_reason": finish_reason,
        }],
        "usage": usage,
    }))
    .into_response()
}

fn normalize_openai_usage(usage: Option<&serde_json::Value>) -> serde_json::Value {
    let Some(serde_json::Value::Object(map)) = usage else {
        return serde_json::Value::Null;
    };
    let mut out = serde_json::Map::new();
    for key in ["prompt_tokens", "completion_tokens", "total_tokens"] {
        if let Some(v) = map.get(key) {
            let n = v
                .as_u64()
                .or_else(|| v.as_i64().map(|i| i.max(0) as u64))
                .or_else(|| v.as_f64().map(|f| f.max(0.0).round() as u64))
                .unwrap_or(0);
            out.insert(key.to_string(), serde_json::json!(n));
        }
    }
    // Preserve any other usage fields untouched.
    for (k, v) in map {
        out.entry(k.clone()).or_insert_with(|| v.clone());
    }
    serde_json::Value::Object(out)
}

/// Streaming belongs to the bridge's gRPC ChatService, not a second HTTP
/// provider path.
pub async fn tched_router_chat_stream_handler(
    Extension(_state): Extension<Arc<AppState>>,
    Json(_req): Json<TchedRouterChatRequest>,
) -> Response {
    json_error_response(
        StatusCode::BAD_REQUEST,
        "use op_chat.chat.ChatService.Send on port 8090 for streaming",
    )
}

/// OpenAI-compatible model catalog backed by `tched_router.ListModels`.
pub async fn openai_models_handler(
    headers: HeaderMap,
    Extension(_state): Extension<Arc<AppState>>,
) -> Response {
    let result = match call_tched_router_method(
        &headers,
        "ListModels",
        "cap.software.3tched-router.models.read@v1",
        serde_json::json!({}),
    )
    .await
    {
        Ok(result) => result,
        Err(response) => return response,
    };
    let Some(routes) = result
        .get("model_routes")
        .and_then(serde_json::Value::as_array)
    else {
        return json_error_response(
            StatusCode::BAD_GATEWAY,
            "tched_router ListModels returned no model_routes",
        );
    };
    let mut seen = std::collections::HashSet::new();
    let data = routes
        .iter()
        .filter_map(|route| {
            let model = route.get("model")?.as_str()?;
            if !seen.insert(model.to_string()) {
                return None;
            }
            let provider = route
                .get("upstream_provider")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .or_else(|| route.get("provider").and_then(serde_json::Value::as_str))
                .unwrap_or_default();
            Some(serde_json::json!({
                "id": model,
                "object": "model",
                "owned_by": provider,
                "available": route
                    .get("available")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false),
                "route_hint": route
                    .get("hint")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            }))
        })
        .collect::<Vec<_>>();

    Json(serde_json::json!({
        "object": "list",
        "data": data,
    }))
    .into_response()
}

/// Helper to build a JSON error response.
fn json_error_response(status: StatusCode, message: &str) -> Response {
    let body = simd_json::to_string(&json!({"error": message})).unwrap_or_default();
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .body(Body::from(body))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .expect("empty body should always succeed")
        })
}

/// Read the tched_router `PluginSchema` directly from the sealed blob catalog
/// (Absolute Base). Per AGENTS.md the sealed blob IS the plugin; the monolithic
/// `/dev/shm/live-schema.json` is gone. Live projection values take precedence,
/// while schema field defaults provide the boot-safe provider/model catalog.
fn read_tched_router_schema_shm() -> Option<Value> {
    let schema =
        op_blob::catalog::read_plugin_schema_shm(crate::tched_router_routes::ROUTER_PLUGIN_ID)?;
    // `PluginSchema` serializes to { name, version, fields, methods, … }.
    let mut v: Value = simd_json::to_owned_value(&mut serde_json::to_vec(&schema).ok()?).ok()?;
    // Stamp the catalog_hash so consumers can verify lineage.
    if let Ok(bytes) = std::fs::read("/dev/shm/opdbus/plugin-blobs/.manifest.json")
        && let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&bytes)
        && let Some(hash) = manifest.get("catalog_hash").and_then(|h| h.as_str())
        && let Some(obj) = v.as_object_mut()
    {
        obj.insert("catalog_hash".to_string(), simd_json::json!(hash));
    }
    Some(v)
}

fn schema_field_default<'a>(schema: &'a Value, field: &str) -> Option<&'a Value> {
    schema.get("fields")?.get(field)?.get("default")
}

fn method_contract(schema: Option<&Value>, method: &str) -> Value {
    schema
        .and_then(|schema| schema.get("methods"))
        .and_then(|methods| methods.get(method))
        .cloned()
        .unwrap_or_else(|| json!({}))
}

/// Build the HTTP compatibility document from the same sealed Schemars method
/// contracts used by D-Bus and gRPC. No external documentation server exists.
fn compatibility_openapi(schema: Option<&Value>) -> Value {
    let chat = method_contract(schema, "Chat");
    let list_models = method_contract(schema, "ListModels");
    let chat_args = chat.get("args").cloned().unwrap_or_else(|| json!({}));

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "3tched Router HTTP compatibility",
            "version": "1.0.0"
        },
        "paths": {
            "/v1/models": {
                "get": {
                    "operationId": "tched_router.ListModels",
                    "x-opdbus-method-contract": list_models,
                    "responses": {
                        "200": {
                            "description": "OpenAI-compatible model catalog"
                        }
                    }
                }
            },
            "/v1/chat/completions": {
                "post": {
                    "operationId": "tched_router.Chat",
                    "x-opdbus-method-contract": chat,
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": chat_args
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "OpenAI-compatible chat completion"
                        }
                    }
                }
            },
            "/api/tched-router/chat": {
                "post": {
                    "operationId": "tched_router.Chat",
                    "responses": {
                        "200": {
                            "description": "3tched Router chat completion"
                        }
                    }
                }
            },
            "/api/llm/chat": {
                "post": {
                    "operationId": "tched_router.Chat",
                    "responses": {
                        "200": {
                            "description": "3tched Router chat completion"
                        }
                    }
                }
            }
        }
    })
}

/// Serve the combined schema for schema-driven UI rendering.
///
/// Combines the locally-derived HTTP adapter document with the 3tched Router plugin
/// projection (providers, model_routes, tools, structured_output) so the
/// frontend can render the entire chat interface from a single schema.
/// GET /api/tched-router/schema
pub async fn tched_router_schema_handler(Extension(_state): Extension<Arc<AppState>>) -> Response {
    // Read 3tched Router state directly from the SHM state tree. Fall back to the
    // sealed PluginSchema blob so the schema surface still renders when no
    // mutation has populated SHM yet.
    let (router, router_schema) = match crate::tched_router_routes::read_router_plugin().await {
        Ok(Some(v)) => {
            info!("Using tched_router from SHM state tree (live provider/model catalog)");
            (v, read_tched_router_schema_shm())
        }
        Ok(None) => match read_tched_router_schema_shm() {
            Some(schema) => {
                warn!("3tched Router SHM state unavailable; using PluginSchema only");
                (schema.clone(), Some(schema))
            }
            None => {
                error!("3tched Router not available from SHM state tree or PluginSchema");
                return json_error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "3tched Router projection not available",
                );
            }
        },
        Err(error) => {
            error!(%error, "Committed tched_router state read failed");
            return json_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "3tched Router projection is unreadable",
            );
        }
    };

    // Providers + model_routes live under `projection` in the live state.
    let projection = router.get("projection").and_then(|v| v.as_object());

    // Build a JSON Schema for the chat form from 3tched Router providers + model_routes
    let providers = projection
        .and_then(|p| p.get("providers"))
        .and_then(|v| v.as_array())
        .or_else(|| router.get("providers").and_then(|v| v.as_array()))
        .or_else(|| schema_field_default(&router, "providers").and_then(|v| v.as_array()))
        .or_else(|| {
            router_schema
                .as_ref()
                .and_then(|schema| schema_field_default(schema, "providers"))
                .and_then(|v| v.as_array())
        })
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p.get("id").and_then(|v| v.as_str()).map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let model_routes = projection
        .and_then(|p| p.get("model_routes"))
        .and_then(|v| v.as_array())
        .or_else(|| router.get("model_routes").and_then(|v| v.as_array()))
        .or_else(|| schema_field_default(&router, "model_routes").and_then(|v| v.as_array()))
        .or_else(|| {
            router_schema
                .as_ref()
                .and_then(|schema| schema_field_default(schema, "model_routes"))
                .and_then(|v| v.as_array())
        })
        .map(|arr| {
            arr.iter()
                .filter_map(|r| {
                    let model = r.get("model").and_then(|v| v.as_str())?;
                    let hint = r.get("hint").and_then(|v| v.as_str());
                    let provider = r
                        .get("provider")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let available = r
                        .get("available")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    // Build label: "provider/model (hint) [status]"
                    let mut label = format!("{}/{}", provider, model);
                    if let Some(h) = hint {
                        label.push_str(&format!(" ({})", h));
                    }
                    if !available {
                        label.push_str(" [declared]");
                    }

                    Some(json!({"const": model, "title": label}))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let chat_schema = json!({
        "$schema": "http://json-schema.org/draft-07/schema#",
        "type": "object",
        "title": "3tched Router Chat",
        "properties": {
            "provider": {
                "type": "string",
                "enum": providers,
                "description": "LLM provider route"
            },
            "model": {
                "type": "string",
                "oneOf": model_routes,
                "description": "Model to use for this turn"
            },
            "message": {
                "type": "string",
                "description": "Chat message",
                "minLength": 1
            }
        },
        "required": ["message"]
    });

    let structured_output = projection
        .and_then(|p| p.get("structured_output"))
        .cloned()
        .or_else(|| router.get("structured_output").cloned())
        .or_else(|| schema_field_default(&router, "structured_output").cloned())
        .or_else(|| {
            router_schema
                .as_ref()
                .and_then(|schema| schema_field_default(schema, "structured_output"))
                .cloned()
        })
        .unwrap_or(json!({}));
    let tools = projection
        .and_then(|p| p.get("tools"))
        .cloned()
        .or_else(|| router.get("tools").cloned())
        .or_else(|| schema_field_default(&router, "tools").cloned())
        .or_else(|| {
            router_schema
                .as_ref()
                .and_then(|schema| schema_field_default(schema, "tools"))
                .cloned()
        })
        .unwrap_or(json!([]));

    let openapi = compatibility_openapi(router_schema.as_ref());
    let schema = json!({
        "openapi": openapi,
        "tched_router": {
            "plugin_state": router,
            "schema": router_schema,
            "chat_form_schema": chat_schema,
            "structured_output": structured_output,
            "tools": tools,
        }
    });

    let body = simd_json::to_string(&schema).unwrap_or_default();
    Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(body))
        .unwrap_or_else(|_| {
            json_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build schema response",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn schema_field_defaults_supply_boot_catalogs() {
        let schema = json!({
            "fields": {
                "providers": {
                    "default": [{"id": "factory"}]
                },
                "model_routes": {
                    "default": [{"model": "auto"}]
                }
            }
        });

        assert_eq!(
            schema_field_default(&schema, "providers")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            schema_field_default(&schema, "model_routes")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn identity_headers_require_one_canonical_bounded_credential() {
        assert!(ghostbridge_metadata(&HeaderMap::new()).is_err());

        let mut headers = HeaderMap::new();
        headers.insert(
            op_identity::sealed_id::HTTP_HEADER_NAME,
            HeaderValue::from_static("U0lEMXg"),
        );
        let metadata = ghostbridge_metadata(&headers).unwrap();
        assert_eq!(metadata.sealed_id.as_deref(), Some(b"SID1x".as_slice()));
        assert!(metadata.oracle_assertion.is_none());

        headers.insert(
            "x-oracle-identity-assertion-bin",
            HeaderValue::from_static("T0lBMXg"),
        );
        assert!(ghostbridge_metadata(&headers).is_err());
        headers.remove(op_identity::sealed_id::HTTP_HEADER_NAME);
        let metadata = ghostbridge_metadata(&headers).unwrap();
        assert_eq!(
            metadata.oracle_assertion.as_deref(),
            Some(b"OIA1x".as_slice())
        );
        headers.insert(
            "x-oracle-identity-assertion-bin",
            HeaderValue::from_static("T0lBMXg="),
        );
        assert!(ghostbridge_metadata(&headers).is_err());
    }

    #[test]
    fn private_workspace_metadata_requires_the_exact_request_sid1() {
        let headers = HeaderMap::new();
        let response = match sid1_metadata(&headers) {
            Err(response) => response,
            Ok(_) => panic!("missing SID1 must be rejected"),
        };
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-oracle-identity-assertion-bin",
            HeaderValue::from_static("T0lBMXg"),
        );
        let response = match sid1_metadata(&headers) {
            Err(response) => response,
            Ok(_) => panic!("missing SID1 must be rejected"),
        };
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        headers.remove("x-oracle-identity-assertion-bin");
        headers.insert(
            op_identity::sealed_id::HTTP_HEADER_NAME,
            HeaderValue::from_static("U0lEMXByaXZhdGU"),
        );
        let metadata = sid1_metadata(&headers).unwrap();
        assert_eq!(
            metadata.sealed_id.as_deref(),
            Some(b"SID1private".as_slice())
        );
        assert!(metadata.oracle_assertion.is_none());
        assert!(metadata.session_genesis.is_none());
    }
}
