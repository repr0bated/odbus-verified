//! Agent API handlers — sealed-door adapters (op-web agent-orchestration port).
//!
//! The spawn/task path uses canonical admission: each admitted request is
//! exactly one `PluginV1.Call` over the
//! bridge door carrying the caller's identity metadata unchanged, so the
//! sealed contract, the caller's grants, and the MutationEngine decide
//! everything. This module executes no tool loop and schedules nothing.
//! Workflow start receipts are consumed by the bridge's event-driven
//! workstack runner (no timers, no polls).
//!
//! GET /api/agents returns {"agents": [AgentInstance, ...], "workflows":
//! [WorkflowSummary, ...]}. AgentInstance fields are id, agent_type, pid,
//! status, started_at, last_health_check, restart_count. Workflow entries are
//! copied unchanged from committed public workflows.workflows state (normally
//! id, name, steps, triggers, status); they declare pipelines, not running agents.
//! A missing or withheld public projection yields an empty workflows array;
//! the source workflows schema currently declares no public-state paths.
//! A failed state read
//! returns an error with HTTP 502, never a fabricated empty inventory.
//! Successful spawn/task responses mark admission with accepted: true and
//! retain the door result in execution/data; that flag does not claim that
//! execution succeeded (e.g. a result can contain a nonzero exit_code).

use std::sync::Arc;

use axum::extract::{Extension, Path};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Json as ResponseJson, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::info;

use crate::state::{AppState, call_sealed_method};

/// GET /api/agents — declared workflow pipelines plus (compatibility only)
/// in-process registry instances; new workloads are never spawned locally.
pub async fn list_agents_handler(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<ResponseJson<Value>, (StatusCode, ResponseJson<Value>)> {
    let registry = state.agent_registry.read().await;
    let agents = registry.list_instances().await;
    drop(registry);
    let workflows = crate::chat_router_routes::read_workflows_summaries()
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, json_error(&error.to_string())))?;
    Ok(ResponseJson(
        json!({ "agents": agents, "workflows": workflows }),
    ))
}

/// GET /api/agents/types — the unified execution catalog the sealed
/// `cognitive` methods correspond to.
pub async fn list_agent_types_handler(
    Extension(state): Extension<Arc<AppState>>,
) -> ResponseJson<Value> {
    {
        let registry = state.agent_registry.read().await;
        let types = registry.list_agent_types().await;
        if !types.is_empty() {
            return ResponseJson(json!({ "types": types }));
        }
    }
    let types: Vec<String> = op_agents::list_agent_types()
        .into_iter()
        .map(str::to_string)
        .collect();
    ResponseJson(json!({ "types": types }))
}

/// GET /api/agents/:id — in-process registry status (legacy instances only).
pub async fn get_agent_handler(
    Extension(state): Extension<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<ResponseJson<Value>, (StatusCode, ResponseJson<Value>)> {
    let registry = state.agent_registry.read().await;
    match registry.get_instance_status(&id).await {
        Ok(status) => Ok(ResponseJson(json!({ "agent": status }))),
        Err(e) => Err((StatusCode::NOT_FOUND, json_error(&e.to_string()))),
    }
}

#[derive(Debug, Deserialize)]
pub struct SpawnAgentRequest {
    #[serde(rename = "type")]
    pub agent_type: String,
    #[serde(default)]
    pub config: Option<Value>,
}

fn agent_capability(agent_type: &str) -> String {
    format!("cognitive.agent.{}", sanitize(agent_type))
}

fn agent_method_name(agent_type: &str, operation: &str) -> String {
    format!("agent_{}_{}", sanitize(agent_type), sanitize(operation))
}

/// POST /api/agents — start an admitted workload through the sealed door.
///
/// Three admissible forms, each exactly one identity-carried admission:
/// - `config.steps` → an orchestration catalog `run_workflow` operation;
///   structured steps are forwarded intact for sealed contract validation.
/// - `config.arguments` → one catalog executor operation
///   (`agent_<agent>_<operation>`; the sealed per-operation contract owns
///   the shape).
/// - otherwise → a declared pipeline start (`workflows.start_workflow`),
///   whose sealed receipt the runner consumes reactively.
#[allow(clippy::type_complexity)]
pub async fn spawn_agent_handler(
    headers: HeaderMap,
    ResponseJson(request): ResponseJson<SpawnAgentRequest>,
) -> Result<ResponseJson<Value>, (StatusCode, ResponseJson<Value>)> {
    info!(agent_type = %request.agent_type, "sealed agent workload requested");
    let identity = crate::handlers::chat_router::ghostbridge_metadata(&headers)
        .map_err(|refusal| (refusal_status(&refusal), refusal_body(&refusal)))?;
    let config = request.config.clone().unwrap_or_else(|| json!({}));
    let steps = config.get("steps");
    let operation = config
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or(if steps.is_some() {
            "run_workflow"
        } else {
            "run"
        });

    let outcome = if let Some(steps) = steps {
        call_sealed_method(
            "cognitive",
            &agent_method_name(&request.agent_type, operation),
            json!({ "steps": steps, "context": config.get("context").cloned() }),
            &agent_capability(&request.agent_type),
            &identity,
        )
        .await
    } else if config.get("arguments").is_some() {
        call_sealed_method(
            "cognitive",
            &agent_method_name(&request.agent_type, operation),
            config
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({})),
            &agent_capability(&request.agent_type),
            &identity,
        )
        .await
    } else {
        call_sealed_method(
            "workflows",
            "start_workflow",
            json!({
                "workflow_id": request.agent_type,
                "params": config.get("params").cloned().unwrap_or_else(|| json!({})),
            }),
            "workflows.write",
            &identity,
        )
        .await
    };

    match outcome {
        Ok(execution) => Ok(ResponseJson(json!({
            "accepted": true,
            "agent_type": request.agent_type,
            "execution": execution,
        }))),
        Err(error) => Err((
            door_error_status(&error.to_string()),
            json_error(&error.to_string()),
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct AgentTaskRequest {
    pub operation: String,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub args: Option<String>,
}

/// POST /api/agents/:id/task — one identity-carried catalog operation for
/// `id` (operation + optional `path`/`args` ride the sealed argument
/// contract; the door is the validator, never this module).
#[allow(clippy::type_complexity)]
pub async fn agent_task_handler(
    Extension(state): Extension<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ResponseJson(request): ResponseJson<AgentTaskRequest>,
) -> Result<ResponseJson<Value>, (StatusCode, ResponseJson<Value>)> {
    let agent_type = {
        let registry = state.agent_registry.read().await;
        match registry.get_instance_status(&id).await {
            Ok(inst) => inst.agent_type.clone(),
            Err(e) => {
                return Err((StatusCode::NOT_FOUND, json_error(&e.to_string())));
            }
        }
    };
    let identity = crate::handlers::chat_router::ghostbridge_metadata(&headers)
        .map_err(|refusal| (refusal_status(&refusal), refusal_body(&refusal)))?;
    let mut arguments = json!({});
    if let Some(path) = request.path {
        arguments["path"] = json!(path);
    }
    if let Some(args) = request.args {
        arguments["args"] = json!(args);
    }

    match call_sealed_method(
        "cognitive",
        &agent_method_name(&agent_type, &request.operation),
        arguments,
        &agent_capability(&agent_type),
        &identity,
    )
    .await
    {
        Ok(data) => Ok(ResponseJson(json!({
            "accepted": true,
            "operation": request.operation,
            "data": data,
        }))),
        Err(error) => Err((
            door_error_status(&error.to_string()),
            json_error(&error.to_string()),
        )),
    }
}

/// DELETE /api/agents/:id — kills a legacy in-process instance; admitted
/// workloads are owned by the bridge runner, not this module.
pub async fn kill_agent_handler(
    Extension(state): Extension<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<ResponseJson<Value>, (StatusCode, ResponseJson<Value>)> {
    info!("Kill request for agent: {}", id);
    let registry = state.agent_registry.write().await;
    match registry.kill_agent(&id).await {
        Ok(_) => Ok(ResponseJson(json!({ "killed": true }))),
        Err(e) => {
            let message = e.to_string();
            let status = if message.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            Err((status, json_error(&message)))
        }
    }
}

fn sanitize(agent_type: &str) -> String {
    // Match the identifier normalization in the plugin-owned producer,
    // op-plugins/src/state_plugins/cognitive_agents.rs. This only names a
    // request; existence, shape and grants are still checked by the door.
    agent_type
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn refusal_status(refusal: &Response) -> StatusCode {
    refusal.status()
}

fn refusal_body(_refusal: &Response) -> ResponseJson<Value> {
    ResponseJson(json!({
        "error": "identity is required to start acknowledged agent workloads",
        "success": false,
    }))
}

fn door_error_status(message: &str) -> StatusCode {
    let message = message.to_ascii_lowercase();
    // The client currently wraps both tonic statuses and numeric CallMethod
    // error codes as text. Prefer the explicit remote code over its message.
    if let Some(code) = message
        .split_once("remote error [")
        .and_then(|(_, tail)| tail.split_once(']'))
        .and_then(|(code, _)| code.parse::<i32>().ok())
    {
        return match op_grpc_bridge::proto::ErrorCode::try_from(code) {
            Ok(op_grpc_bridge::proto::ErrorCode::NotFound) => StatusCode::NOT_FOUND,
            Ok(op_grpc_bridge::proto::ErrorCode::PermissionDenied) => StatusCode::FORBIDDEN,
            Ok(op_grpc_bridge::proto::ErrorCode::ValidationFailed) => StatusCode::BAD_REQUEST,
            _ => StatusCode::BAD_GATEWAY,
        };
    }
    if message.contains("connection failed") || message.contains("parse error") {
        return StatusCode::BAD_GATEWAY;
    }
    if let Some(code) = message
        .split_once("status: ")
        .and_then(|(_, tail)| tail.split(',').next())
    {
        return match code.trim() {
            "unauthenticated" => StatusCode::UNAUTHORIZED,
            "permissiondenied" => StatusCode::FORBIDDEN,
            "notfound" => StatusCode::NOT_FOUND,
            "invalidargument" => StatusCode::BAD_REQUEST,
            _ => StatusCode::BAD_GATEWAY,
        };
    }
    if message.contains("access denied")
        || message.contains("accessdenied")
        || message.contains("permissiondenied")
        || message.contains("permission denied")
    {
        StatusCode::FORBIDDEN
    } else if message.contains("unauthenticated") || message.contains("credential") {
        StatusCode::UNAUTHORIZED
    } else if message.contains("notfound") || message.contains("not found") {
        StatusCode::NOT_FOUND
    } else if message.contains("invalidargument") {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::BAD_GATEWAY
    }
}

fn json_error(message: &str) -> ResponseJson<Value> {
    ResponseJson(json!({ "error": message, "success": false }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::Request,
        response::IntoResponse,
        routing::post,
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn spawn_rejects_missing_invalid_and_ambiguous_identity_without_a_bridge() {
        for (credentials, expected) in [
            (vec![], StatusCode::UNAUTHORIZED),
            (
                vec![(op_identity::sealed_id::HTTP_HEADER_NAME, "not base64")],
                StatusCode::BAD_REQUEST,
            ),
            (
                vec![
                    (op_identity::sealed_id::HTTP_HEADER_NAME, "U0lEMXg"),
                    ("x-oracle-identity-assertion-bin", "T0lBMXg"),
                ],
                StatusCode::UNAUTHORIZED,
            ),
        ] {
            let mut request =
                Request::post("/api/agents").header("content-type", "application/json");
            for (name, value) in credentials {
                request = request.header(name, value);
            }
            let response = Router::new()
                .route("/api/agents", post(spawn_agent_handler))
                .oneshot(request.body(Body::from(r#"{"type":"deploy"}"#)).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
            let body: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(body["success"], false);
            assert!(body["error"].is_string());
            assert!(body.get("accepted").is_none());
        }
    }

    #[test]
    fn capability_and_method_names_follow_catalog_normalization() {
        assert_eq!(
            agent_capability("rust-executor"),
            "cognitive.agent.rust_executor"
        );
        assert_eq!(
            agent_capability("Sequential-Thinking"),
            "cognitive.agent.sequential_thinking"
        );
        assert_eq!(
            agent_method_name("Sequential-Thinking", "run-workflow"),
            "agent_sequential_thinking_run_workflow"
        );
        assert_eq!(sanitize("Rust-Executor"), "rust_executor");
        assert_eq!(sanitize("a.b/é"), "a_b__");
    }

    #[test]
    fn door_errors_preserve_authorization_and_not_found_statuses() {
        for (message, status) in [
            (
                "Request failed: status: Unauthenticated, message: missing identity",
                StatusCode::UNAUTHORIZED,
            ),
            (
                "PermissionDenied: identity lacks credential grant",
                StatusCode::FORBIDDEN,
            ),
            ("AccessDenied: capability missing", StatusCode::FORBIDDEN),
            ("Remote error [2]: identity missing", StatusCode::FORBIDDEN),
            (
                "Remote error [1]: credential object missing",
                StatusCode::NOT_FOUND,
            ),
            ("Remote error [3]: invalid steps", StatusCode::BAD_REQUEST),
            (
                "Remote error [6]: identity backend unavailable",
                StatusCode::BAD_GATEWAY,
            ),
            (
                "Request failed: status: NotFound, message: method missing",
                StatusCode::NOT_FOUND,
            ),
            (
                "Request failed: status: InvalidArgument, message: invalid steps",
                StatusCode::BAD_REQUEST,
            ),
            (
                "Connection failed: credential file not found",
                StatusCode::BAD_GATEWAY,
            ),
            (
                "Request failed: status: Internal, message: credential store not found",
                StatusCode::BAD_GATEWAY,
            ),
            (
                "Request failed: status: NotFound, message: credential object missing",
                StatusCode::NOT_FOUND,
            ),
            ("identity backend unavailable", StatusCode::BAD_GATEWAY),
        ] {
            assert_eq!(door_error_status(message), status, "{message}");
        }
        let response = (StatusCode::FORBIDDEN, json_error("denied")).into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
