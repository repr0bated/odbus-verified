//! LLM API Handlers

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use simd_json::{OwnedValue as Value, json};
use std::str::FromStr;
use std::sync::Arc;

use crate::state::AppState;
use op_llm::provider::ProviderType;

/// Matches `crates/src/api/types.ts` `LlmProvider`.
#[derive(Serialize)]
pub struct LlmProviderCard {
    pub name: String,
    pub enabled: bool,
    pub models: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// Matches `crates/src/api/types.ts` `LlmStatus` (`active_*`) plus the
/// older `provider`/`model` keys so both UIs can read the same payload.
#[derive(Serialize)]
pub struct LlmStatusResponse {
    pub provider: String,
    pub model: String,
    pub active_provider: String,
    pub active_model: String,
    pub model_non_sandboxed: bool,
    pub available: bool,
    pub providers: Vec<LlmProviderCard>,
}

#[derive(Serialize)]
pub struct LlmProvidersResponse {
    pub providers: Vec<String>,
    pub current: String,
}

/// GET /api/llm/status - Get LLM status
pub async fn llm_status_handler(
    Extension(state): Extension<Arc<AppState>>,
) -> Result<Json<LlmStatusResponse>, (StatusCode, String)> {
    let mut provider = state.chat_manager.current_provider().await.to_string();
    let mut model = state.chat_manager.current_model().await;
    if let Some((selected_provider, selected_model)) =
        crate::tched_router_routes::selected_provider_model()
            .await
            .map_err(|error| {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("committed tched_router state unavailable: {error}"),
                )
            })?
    {
        if !selected_provider.is_empty() {
            provider = selected_provider;
        }
        if !selected_model.is_empty() {
            model = selected_model;
        }
    }
    let available = !state.chat_manager.available_providers().is_empty();
    let model_non_sandboxed = state.chat_manager.current_model_non_sandboxed().await;
    let providers: Vec<LlmProviderCard> = state
        .chat_manager
        .available_providers()
        .into_iter()
        .map(|p| LlmProviderCard {
            name: p.to_string(),
            enabled: true,
            models: Vec::new(),
            status: Some("available".to_string()),
        })
        .collect();
    Ok(Json(LlmStatusResponse {
        active_provider: provider.clone(),
        active_model: model.clone(),
        provider,
        model,
        model_non_sandboxed,
        available,
        providers,
    }))
}

#[derive(Debug, Deserialize)]
pub struct SwitchModelRequest {
    pub model: String,
}

/// POST /api/llm/model — LlmPage "Switch" button.
pub async fn switch_model_handler(
    Extension(state): Extension<Arc<AppState>>,
    axum::Json(req): axum::Json<SwitchModelRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .chat_manager
        .switch_model(&req.model)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    Ok(Json(json!({
        "ok": true,
        "active_model": req.model,
        "model": req.model,
    })))
}

/// GET /api/llm/providers - List available providers
pub async fn list_providers_handler(
    Extension(state): Extension<Arc<AppState>>,
) -> Json<LlmProvidersResponse> {
    let providers: Vec<String> = state
        .chat_manager
        .available_providers()
        .into_iter()
        .map(|provider| provider.to_string())
        .collect();
    let current = state.chat_manager.current_provider().await.to_string();
    Json(LlmProvidersResponse { providers, current })
}

/// GET /api/llm/models - List available models
pub async fn list_models_handler(Extension(state): Extension<Arc<AppState>>) -> Json<Value> {
    match state.chat_manager.list_models().await {
        Ok(models) => {
            let current = state.chat_manager.current_model().await;
            Json(json!({
                "models": models,
                "current": current
            }))
        }
        Err(e) => Json(json!({
            "models": [],
            "current": null,
            "error": e.to_string()
        })),
    }
}

/// GET /api/llm/models/:provider - List models for a provider
pub async fn list_models_for_provider_handler(
    Extension(state): Extension<Arc<AppState>>,
    Path(provider): Path<String>,
) -> Json<Value> {
    match ProviderType::from_str(&provider) {
        Ok(provider_type) => match state
            .chat_manager
            .list_models_for_provider(&provider_type)
            .await
        {
            Ok(models) => Json(json!({
                "provider": provider,
                "models": models,
                "current": state.chat_manager.current_model().await
            })),
            Err(e) => Json(json!({
                "provider": provider,
                "models": [],
                "current": null,
                "error": e.to_string()
            })),
        },
        Err(e) => Json(json!({
            "provider": provider,
            "models": [],
            "current": null,
            "error": e
        })),
    }
}
