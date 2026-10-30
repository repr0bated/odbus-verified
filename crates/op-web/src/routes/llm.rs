//! LLM API routes for provider and model management

use axum::{
    Json,
    extract::{Extension, Query},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use std::sync::Arc;
use tracing::error;

use crate::state::AppState;
use op_llm::ProviderType;

/// LLM status response
#[derive(Debug, Serialize)]
pub struct LlmStatusResponse {
    pub provider: String,
    pub model: String,
    pub model_non_sandboxed: bool,
    pub available_providers: Vec<String>,
}

/// Model list response
#[derive(Debug, Serialize)]
pub struct ModelsResponse {
    pub provider: String,
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub available: bool,
    pub provider: Option<String>,
    pub upstream_provider: Option<String>,
    pub transport: Option<String>,
    pub status: Option<String>,
    pub route_hint: Option<String>,
    pub route_kind: Option<String>,
    pub status_reason: Option<String>,
    pub source: Option<String>,
}

/// Query params for models endpoint
#[derive(Debug, Deserialize)]
pub struct ModelsQuery {
    pub provider: Option<String>,
}

/// GET /api/llm/status - Get current LLM status
pub async fn get_llm_status(Extension(state): Extension<Arc<AppState>>) -> impl IntoResponse {
    let status = state.chat_manager.get_status().await;

    Json(LlmStatusResponse {
        provider: status["provider"].as_str().unwrap_or("unknown").to_string(),
        model: status["model"].as_str().unwrap_or("").to_string(),
        model_non_sandboxed: state.chat_manager.current_model_non_sandboxed().await,
        available_providers: status["available_providers"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

/// Build the model list from the tched_router plugin projection (`model_routes`).
///
/// The tched_router plugin is the single source of truth: its live state
/// is projected verbatim at `/org/opdbus/v1/plugins/tched_router`. We read it from
/// the SHM state tree and surface each route as a selectable model.
async fn models_from_tched_router(_state: &AppState) -> anyhow::Result<Option<Vec<ModelInfo>>> {
    let Some(routes) = crate::chat_router_routes::routes().await? else {
        return Ok(None);
    };

    let models: Vec<ModelInfo> = routes
        .iter()
        .map(|route| {
            let description = match route.hint.as_deref() {
                Some(h) => format!("{} · {} · {}", route.provider, h, route.status),
                None => format!("{} · {}", route.provider, route.status),
            };
            let name = match route.hint.as_deref() {
                Some(h) => format!("{} ({})", route.model, h),
                None => route.model.clone(),
            };
            ModelInfo {
                id: route.model.clone(),
                name,
                description: Some(description),
                available: route.available,
                provider: Some(route.upstream_provider.clone()),
                upstream_provider: Some(route.upstream_provider.clone()),
                transport: route.transport.clone(),
                status: Some(route.status.clone()),
                route_hint: route.hint.clone(),
                route_kind: route.kind.clone(),
                status_reason: route.status_reason.clone(),
                source: route.source.clone(),
            }
        })
        .collect();

    if models.is_empty() {
        Ok(None)
    } else {
        Ok(Some(models))
    }
}

/// GET /api/llm/models - Get models for current or specified provider
pub async fn get_models(
    Extension(state): Extension<Arc<AppState>>,
    Query(query): Query<ModelsQuery>,
) -> impl IntoResponse {
    let Some(ref provider_str) = query.provider else {
        // The tched_router plugin projection is the default source of truth for the combined model list.
        if let Ok(Some(models)) = models_from_tched_router(&state).await {
            return Json(ModelsResponse {
                provider: crate::chat_router_routes::ROUTER_PLUGIN_ID.to_string(),
                models,
            })
            .into_response();
        }

        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(simd_json::json!({
                "error": "tched_router model route catalog is unavailable",
                "provider": crate::chat_router_routes::ROUTER_PLUGIN_ID,
                "models": []
            })),
        )
            .into_response();
    };

    if provider_str == crate::chat_router_routes::ROUTER_PLUGIN_ID {
        if let Ok(Some(models)) = models_from_tched_router(&state).await {
            return Json(ModelsResponse {
                provider: crate::chat_router_routes::ROUTER_PLUGIN_ID.to_string(),
                models,
            })
            .into_response();
        }
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(simd_json::json!({
                "error": "tched_router model route catalog is unavailable",
                "provider": crate::chat_router_routes::ROUTER_PLUGIN_ID,
                "models": []
            })),
        )
            .into_response();
    }

    let provider_type = match provider_str.parse::<ProviderType>() {
        Ok(pt) => pt,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(simd_json::json!({ "error": format!("Unknown provider: {}", provider_str) })),
            )
                .into_response();
        }
    };

    // Check if provider is available
    if !state.chat_manager.has_provider(&provider_type) {
        return (
            StatusCode::BAD_REQUEST,
            Json(simd_json::json!({
                "error": format!("Provider {} not available. Check API key.", provider_type),
                "provider": provider_type.to_string(),
                "models": []
            })),
        )
            .into_response();
    }

    match state
        .chat_manager
        .list_models_for_provider(&provider_type)
        .await
    {
        Ok(models) => {
            let model_infos: Vec<ModelInfo> = models
                .into_iter()
                .map(|m| ModelInfo {
                    id: m.id,
                    name: m.name,
                    description: m.description,
                    available: m.available,
                    provider: Some(provider_type.to_string()),
                    upstream_provider: Some(provider_type.to_string()),
                    transport: None,
                    status: Some(
                        if m.available {
                            "available"
                        } else {
                            "unavailable"
                        }
                        .to_string(),
                    ),
                    route_hint: None,
                    route_kind: None,
                    status_reason: None,
                    source: None,
                })
                .collect();

            Json(ModelsResponse {
                provider: provider_type.to_string(),
                models: model_infos,
            })
            .into_response()
        }
        Err(e) => {
            error!("Failed to list models for {}: {}", provider_type, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(simd_json::json!({
                    "error": format!("Failed to list models: {}", e),
                    "provider": provider_type.to_string(),
                    "models": []
                })),
            )
                .into_response()
        }
    }
}
