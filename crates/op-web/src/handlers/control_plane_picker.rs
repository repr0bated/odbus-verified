//! Control-plane model picker.
//!
//! The typed password is handed to `unix_chkpwd` and discarded. The account
//! hash stays in the system password database; this handler never copies it.

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

const SUDO_USER: &str = "jeremy";
const CATALOG_URL: &str = "https://opencode.ai/zen/v1/models";
const ROUTER_ENDPOINT: &str = "http://127.0.0.1:8084";
const TOKEN_FILE: &str = "/etc/op-dbus/secrets/tched-router-runtime-token";

#[derive(Deserialize)]
pub(crate) struct PasswordBody {
    password: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    wire: String,
}

#[derive(Serialize)]
struct ModelRow {
    id: String,
}

pub async fn list_models(Json(body): Json<PasswordBody>) -> impl IntoResponse {
    if !sudo_password_ok(&body.password) {
        return reject();
    }
    match fetch_models().await {
        Ok(models) => (StatusCode::OK, Json(json!({ "models": models }))).into_response(),
        Err(error) => (StatusCode::BAD_GATEWAY, Json(json!({ "error": error }))).into_response(),
    }
}

pub async fn set_model(Json(body): Json<PasswordBody>) -> impl IntoResponse {
    if !sudo_password_ok(&body.password) {
        return reject();
    }
    let wire = match body.wire.as_str() {
        "chat_completions" | "responses" | "messages" => body.wire.as_str(),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "wire must be chat_completions, responses, or messages" })),
            )
                .into_response();
        }
    };
    if !valid_model_id(&body.model) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "model id is not in the live catalog shape" })),
        )
            .into_response();
    }
    let models = match fetch_models().await {
        Ok(models) => models,
        Err(error) => {
            return (StatusCode::BAD_GATEWAY, Json(json!({ "error": error }))).into_response();
        }
    };
    if !models.iter().any(|model| model.id == body.model) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "model is not in the live catalog" })),
        )
            .into_response();
    }
    match apply_selection(&body.model, wire).await {
        Ok(()) => (
            StatusCode::OK,
            Json(json!({ "model": body.model, "wire": wire })),
        )
            .into_response(),
        Err(error) => (StatusCode::BAD_GATEWAY, Json(json!({ "error": error }))).into_response(),
    }
}

fn reject() -> axum::response::Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "sudo password was not accepted" })),
    )
        .into_response()
}

fn sudo_password_ok(password: &str) -> bool {
    if password.is_empty()
        || password.len() > 1024
        || password
            .bytes()
            .any(|byte| byte == 0 || byte == b'\n' || byte == b'\r')
    {
        return false;
    }
    let mut child = match Command::new("/usr/bin/unix_chkpwd")
        .arg(SUDO_USER)
        .arg("nonull")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };
    let accepted = (|| {
        let mut stdin = child.stdin.take()?;
        stdin.write_all(password.as_bytes()).ok()?;
        stdin.write_all(b"\n").ok()?;
        drop(stdin);
        Some(child.wait().ok()?.code() == Some(0))
    })();
    accepted.unwrap_or(false)
}

fn valid_model_id(model: &str) -> bool {
    !model.is_empty()
        && model.len() <= 128
        && model
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

async fn fetch_models() -> Result<Vec<ModelRow>, String> {
    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?
        .get(CATALOG_URL)
        .header("user-agent", "op-web")
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("model catalog returned {}", response.status()));
    }
    let body: Value = response.json().await.map_err(|error| error.to_string())?;
    let rows = body
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| "model catalog has no data array".to_string())?;
    let mut models = Vec::new();
    for row in rows {
        let Some(id) = row.get("id").and_then(Value::as_str) else {
            continue;
        };
        if valid_model_id(id) {
            models.push(ModelRow { id: id.to_string() });
        }
    }
    models.sort_by(|left, right| left.id.cmp(&right.id));
    if models.is_empty() {
        return Err("model catalog was empty".into());
    }
    Ok(models)
}

async fn apply_selection(model: &str, wire: &str) -> Result<(), String> {
    let token = std::fs::read_to_string(TOKEN_FILE)
        .map_err(|_| "router token is unavailable".to_string())?;
    let token = token.trim();
    if token.is_empty() {
        return Err("router token is empty".into());
    }
    // Messages models reuse the Anthropic profile that already holds the OpenCode
    // API key. Chat and responses stay on the custom OpenCode profile.
    let patch = if wire == "messages" {
        json!([
            {"op":"replace","path":"/providers/models/anthropic/zen_claude_fable_51/model","value": model},
            {"op":"replace","path":"/providers/models/anthropic/zen_claude_fable_51/uri","value": "https://opencode.ai/zen"},
            {"op":"replace","path":"/agents/control_plane/model_provider","value": "anthropic.zen_claude_fable_51"}
        ])
    } else {
        json!([
            {"op":"replace","path":"/providers/models/custom/opencode_control_plane/model","value": model},
            {"op":"replace","path":"/providers/models/custom/opencode_control_plane/uri","value": "https://opencode.ai/zen/v1"},
            {"op":"replace","path":"/providers/models/custom/opencode_control_plane/wire_api","value": wire},
            {"op":"replace","path":"/agents/control_plane/model_provider","value": "custom.opencode_control_plane"}
        ])
    };
    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|error| error.to_string())?
        .patch(format!("{ROUTER_ENDPOINT}/api/config"))
        .bearer_auth(token)
        .json(&patch)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "router rejected the model change ({})",
            response.status()
        ));
    }
    Ok(())
}
