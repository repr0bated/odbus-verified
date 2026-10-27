//! Chat API routes

use axum::{Json, extract::Extension, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info};

use crate::state::AppState;
use op_llm::{ChatMessage, ProviderType};

/// Chat message request
#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<String>,
}

/// Chat message response
#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub content: String,
    pub provider: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools_used: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// POST /api/chat - Send a chat message
pub async fn chat_message(
    Extension(state): Extension<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    info!("Chat request: {} chars", req.message.len());

    // Determine provider and model
    let provider_type = if let Some(ref p) = req.provider {
        match p.parse::<ProviderType>() {
            Ok(pt) => Some(pt),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ChatResponse {
                        content: String::new(),
                        provider: p.clone(),
                        model: String::new(),
                        tools_used: None,
                        error: Some(format!("Unknown provider: {}", p)),
                    }),
                );
            }
        }
    } else {
        None
    };

    // Build messages
    let messages = vec![
        ChatMessage::system(
            "You are a helpful AI assistant integrated with op-dbus, a system management tool. You can help with system administration, code, and general questions.",
        ),
        ChatMessage::user(&req.message),
    ];

    let selected_model = match req.model.clone() {
        Some(model) => model,
        None => state.chat_manager.current_model().await,
    };
    if let Err(e) = crate::chat_router_routes::ensure_model_available(&selected_model).await {
        return (
            StatusCode::CONFLICT,
            Json(ChatResponse {
                content: String::new(),
                provider: req.provider.unwrap_or_else(|| "chat_router".to_string()),
                model: selected_model,
                tools_used: None,
                error: Some(e.to_string()),
            }),
        );
    }

    // Send to LLM
    let result = if let Some(pt) = provider_type.as_ref() {
        if !state.chat_manager.has_provider(pt) {
            return (
                StatusCode::CONFLICT,
                Json(ChatResponse {
                    content: String::new(),
                    provider: pt.to_string(),
                    model: selected_model,
                    tools_used: None,
                    error: Some(format!("Provider {} is not available", pt)),
                }),
            );
        }
        state
            .chat_manager
            .chat_with(pt, &selected_model, messages)
            .await
    } else if req.model.is_some() {
        let route = match crate::chat_router_routes::route_for_model(&selected_model).await {
            Ok(Some(route)) => route,
            Ok(None) => {
                return (
                    StatusCode::CONFLICT,
                    Json(ChatResponse {
                        content: String::new(),
                        provider: "chat_router".to_string(),
                        model: selected_model,
                        tools_used: None,
                        error: Some("Model is not routed by 3tched Router".to_string()),
                    }),
                );
            }
            Err(error) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(ChatResponse {
                        content: String::new(),
                        provider: "chat_router".to_string(),
                        model: selected_model,
                        tools_used: None,
                        error: Some(format!("Router state unavailable: {error}")),
                    }),
                );
            }
        };
        let provider_type = match route.upstream_provider.parse::<ProviderType>() {
            Ok(provider_type) if state.chat_manager.has_provider(&provider_type) => provider_type,
            Ok(provider_type) => {
                return (
                    StatusCode::CONFLICT,
                    Json(ChatResponse {
                        content: String::new(),
                        provider: provider_type.to_string(),
                        model: selected_model,
                        tools_used: None,
                        error: Some(format!("Provider {} is not available", provider_type)),
                    }),
                );
            }
            Err(_) => {
                return (
                    StatusCode::CONFLICT,
                    Json(ChatResponse {
                        content: String::new(),
                        provider: route.upstream_provider,
                        model: selected_model,
                        tools_used: None,
                        error: Some("Route upstream provider is unknown".to_string()),
                    }),
                );
            }
        };
        state
            .chat_manager
            .chat_with(&provider_type, &selected_model, messages)
            .await
    } else {
        // Use current provider and model
        state.chat_manager.chat(messages).await
    };

    match result {
        Ok(response) => {
            info!(
                "Chat response from {}/{}: {} chars",
                response.provider,
                response.model,
                response.message.content.len()
            );

            (
                StatusCode::OK,
                Json(ChatResponse {
                    content: response.message.content,
                    provider: response.provider,
                    model: response.model,
                    tools_used: None,
                    error: None,
                }),
            )
        }
        Err(e) => {
            error!("Chat error: {}", e);

            let current_provider = state.chat_manager.current_provider().await;
            let current_model = state.chat_manager.current_model().await;

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ChatResponse {
                    content: String::new(),
                    provider: provider_type
                        .map(|p| p.to_string())
                        .unwrap_or(current_provider.to_string()),
                    model: req.model.unwrap_or(current_model),
                    tools_used: None,
                    error: Some(e.to_string()),
                }),
            )
        }
    }
}
