//! Chat Router - HTTP endpoints for chat/LLM
//!
//! This module exports a router that can be mounted by op-http.
//! NO server code here - just route definitions.

use axum::{
    extract::State,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use simd_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::actor::ChatActorHandle;

/// Chat session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSession {
    pub id: String,
    pub messages: Vec<ChatMessage>,
}

/// Chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatSession {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            messages: Vec::new(),
        }
    }

    pub fn add_user_message(&mut self, content: &str) {
        self.messages.push(ChatMessage {
            role: "user".to_string(),
            content: content.to_string(),
        });
    }

    pub fn add_assistant_message(&mut self, content: &str) {
        self.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: content.to_string(),
        });
    }
}

/// Chat service state
#[derive(Clone)]
pub struct ChatState {
    pub sessions: Arc<RwLock<HashMap<String, ChatSession>>>,
    pub handle: ChatActorHandle,
}

impl ChatState {
    pub fn new(handle: ChatActorHandle) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            handle,
        }
    }
}

/// Create the chat router
pub fn create_router(state: ChatState) -> Router {
    Router::new()
        .route("/", post(chat_handler))
        .route("/health", get(health_handler))
        .route("/stream", post(stream_handler))
        .route("/sessions", get(list_sessions_handler))
        .route("/sessions/:id", get(get_session_handler))
        .route("/sessions/:id", delete(delete_session_handler))
        .with_state(state)
}

/// Service info for op-http ServiceRouter trait
pub struct ChatServiceRouter;

impl op_http::router::ServiceRouter for ChatServiceRouter {
    fn prefix() -> &'static str {
        "/api/chat"
    }

    fn name() -> &'static str {
        "chat"
    }

    fn description() -> &'static str {
        "Chat/LLM API endpoints"
    }
}

// === Handlers ===

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    session_id: Option<String>,
}

#[derive(Serialize)]
struct ChatResponse {
    session_id: String,
    message: String,
}

async fn chat_handler(
    State(state): State<ChatState>,
    Json(request): Json<ChatRequest>,
) -> impl IntoResponse {
    let session_id = request
        .session_id
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let mut sessions = state.sessions.write().await;
    let session = sessions
        .entry(session_id.clone())
        .or_insert_with(|| ChatSession::new(&session_id));

    session.add_user_message(&request.message);
    
    // Drop lock before await
    drop(sessions);

    // Call ChatActor
    let actor_response = state.handle.chat(Some(session_id.clone()), &request.message).await;

    let response_text = if actor_response.success {
        if let Some(val) = actor_response.result {
             if let Some(s) = val.as_str() {
                s.to_string()
            } else {
                val.to_string()
            }
        } else {
            "Action completed successfully.".to_string()
        }
    } else {
        format!("Error: {}", actor_response.error.unwrap_or_default())
    };

    // Re-acquire lock to update history
    let mut sessions = state.sessions.write().await;
    if let Some(session) = sessions.get_mut(&session_id) {
        session.add_assistant_message(&response_text);
    }

    Json(ChatResponse {
        session_id,
        message: response_text,
    })
}

async fn health_handler() -> impl IntoResponse {
    Json(json!({
        "status": "healthy",
        "service": "chat"
    }))
}

async fn stream_handler(
    State(state): State<ChatState>,
    Json(request): Json<ChatRequest>,
) -> impl IntoResponse {
    // For now, return regular response
    // TODO: Implement SSE streaming
    chat_handler(State(state), Json(request)).await
}

async fn list_sessions_handler(State(state): State<ChatState>) -> impl IntoResponse {
    let sessions = state.sessions.read().await;
    let ids: Vec<String> = sessions.keys().cloned().collect();
    Json(json!({ "sessions": ids }))
}

async fn get_session_handler(
    State(state): State<ChatState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let sessions = state.sessions.read().await;
    match sessions.get(&id) {
        Some(session) => Json(json!({
            "id": session.id,
            "messages": session.messages
        })),
        None => Json(json!({ "error": "Session not found" })),
    }
}

async fn delete_session_handler(
    State(state): State<ChatState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let mut sessions = state.sessions.write().await;
    if sessions.remove(&id).is_some() {
        Json(json!({ "deleted": true }))
    } else {
        Json(json!({ "error": "Session not found" }))
    }
}
