//! OpenClaw Gateway Handlers
//!
//! Provides direct access to the OpenClaw gateway (fabric address by default)
//! for health checks, configuration, and proxied requests.

use axum::{extract::Extension, response::Json};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, error};

use crate::state::AppState;

const DEFAULT_OPENCLAW_BASE_URL: &str = "http://10.0.0.1:8090";
const DEFAULT_OPENCLAW_MODEL: &str = "openclaw:main";
const DEFAULT_OPENCLAW_HOST: &str = "10.0.0.1";

fn openclaw_base_url() -> String {
    std::env::var("OPENCLAW_BASE_URL")
        .unwrap_or_else(|_| DEFAULT_OPENCLAW_BASE_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

fn openclaw_default_model() -> String {
    std::env::var("OPENCLAW_DEFAULT_MODEL").unwrap_or_else(|_| DEFAULT_OPENCLAW_MODEL.to_string())
}

fn configured_openclaw_models() -> Value {
    let default_model = openclaw_default_model();
    json!({
        "models": [
            {
                "id": default_model,
                "name": default_model,
                "description": "Configured OpenClaw route key. OpenClaw selects the target agent and that agent's configured model stack.",
                "routing": "agent",
                "available": true
            }
        ],
        "source": "configured-default",
        "note": "OpenClaw's OpenAI-compatible endpoint routes requests by agent id (for example model=openclaw:<agentId> or x-openclaw-agent-id). /v1/models may not be implemented on the gateway."
    })
}

fn openclaw_host_label(base_url: &str) -> String {
    base_url
        .strip_prefix("http://")
        .or_else(|| base_url.strip_prefix("https://"))
        .unwrap_or(base_url)
        .split(':')
        .next()
        .unwrap_or(DEFAULT_OPENCLAW_HOST)
        .to_string()
}

#[derive(Serialize)]
pub struct OpenClawStatusResponse {
    pub available: bool,
    pub endpoint: String,
    pub model: String,
    pub container_ip: String,
    pub internal_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize)]
pub struct OpenClawConfigResponse {
    pub endpoint: String,
    pub model: String,
    pub internal_only: bool,
    pub container_ip: String,
    pub container_port: u16,
}

/// GET /api/openclaw/status - Check OpenClaw gateway health
pub async fn openclaw_status_handler(
    Extension(_state): Extension<Arc<AppState>>,
) -> Json<OpenClawStatusResponse> {
    let base_url = openclaw_base_url();
    let default_model = openclaw_default_model();

    let container_ip = openclaw_host_label(&base_url);

    // Try to ping OpenClaw
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("HTTP client builder with timeout should succeed");

    let ping_url = format!("{}/v1/chat/completions", base_url);
    let mut response = OpenClawStatusResponse {
        available: false,
        endpoint: base_url.clone(),
        model: default_model,
        container_ip,
        internal_only: true,
        error: None,
    };

    match client.get(&ping_url).send().await {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success()
                || status == reqwest::StatusCode::METHOD_NOT_ALLOWED
                || status == reqwest::StatusCode::BAD_REQUEST
            {
                debug!("OpenClaw gateway is available at {}", base_url);
                response.available = true;
            } else {
                response.error = Some(format!("HTTP {}", status));
                debug!("OpenClaw returned status {}", status);
            }
        }
        Err(e) => {
            error!("Failed to reach OpenClaw: {}", e);
            response.error = Some(e.to_string());
        }
    }

    Json(response)
}

/// GET /api/openclaw/config - Get OpenClaw configuration
pub async fn openclaw_config_handler(
    Extension(_state): Extension<Arc<AppState>>,
) -> Json<OpenClawConfigResponse> {
    let endpoint = openclaw_base_url();

    Json(OpenClawConfigResponse {
        endpoint: endpoint.clone(),
        model: openclaw_default_model(),
        internal_only: true,
        container_ip: openclaw_host_label(&endpoint),
        container_port: 8090,
    })
}

#[derive(Debug, Deserialize)]
pub struct OpenClawChatRequest {
    pub message: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub temperature: Option<f32>,
}

/// POST /api/openclaw/chat - Direct chat via OpenClaw (bypasses op-llm)
pub async fn openclaw_chat_handler(
    Extension(_state): Extension<Arc<AppState>>,
    Json(request): Json<OpenClawChatRequest>,
) -> Json<Value> {
    let base_url = openclaw_base_url();

    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("HTTP client builder with timeout should succeed");

    let chat_url = format!("{}/v1/chat/completions", base_url);

    let model = request.model.unwrap_or_else(openclaw_default_model);

    let payload = json!({
        "model": model,
        "messages": [
            {
                "role": "user",
                "content": request.message
            }
        ],
        "max_tokens": request.max_tokens.unwrap_or(2048),
        "temperature": request.temperature.unwrap_or(0.7)
    });

    match client
        .post(&chat_url)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                match resp.json::<Value>().await {
                    Ok(data) => {
                        // Extract message from OpenAI format response
                        let message = data
                            .get("choices")
                            .and_then(|c| c.as_array())
                            .and_then(|arr| arr.get(0))
                            .and_then(|choice| choice.get("message"))
                            .and_then(|msg| msg.get("content"))
                            .and_then(|c| c.as_str())
                            .unwrap_or("No response");

                        Json(json!({
                            "success": true,
                            "message": message,
                            "model": model,
                            "provider": "openclaw",
                            "raw_response": data
                        }))
                    }
                    Err(e) => Json(json!({
                        "success": false,
                        "error": format!("Failed to parse response: {}", e)
                    })),
                }
            } else {
                let status = resp.status();
                let error_text = resp.text().await.unwrap_or_default();
                Json(json!({
                    "success": false,
                    "error": format!("OpenClaw API error {}: {}", status, error_text)
                }))
            }
        }
        Err(e) => Json(json!({
            "success": false,
            "error": format!("Request failed: {}", e)
        })),
    }
}

/// GET /api/openclaw/models - List configured OpenClaw route keys
pub async fn openclaw_models_handler(Extension(_state): Extension<Arc<AppState>>) -> Json<Value> {
    let base_url = openclaw_base_url();

    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("HTTP client builder with timeout should succeed");

    let models_url = format!("{}/v1/models", base_url);

    match client.get(&models_url).send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                match resp.json::<Value>().await {
                    Ok(data) => Json(data),
                    Err(e) => {
                        debug!(
                            "OpenClaw /v1/models returned a non-JSON response, using configured default route: {}",
                            e
                        );
                        Json(configured_openclaw_models())
                    }
                }
            } else {
                debug!(
                    "OpenClaw /v1/models returned {}, using configured default route",
                    resp.status()
                );
                Json(configured_openclaw_models())
            }
        }
        Err(e) => {
            debug!(
                "OpenClaw /v1/models request failed, using configured default route: {}",
                e
            );
            Json(configured_openclaw_models())
        }
    }
}
