//! Chat API Handlers

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::{
        Json,
        sse::{Event, Sse},
    },
};
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{OwnedValue as Value, json};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{error, info};

use crate::orchestrator::OrchestratorEvent;
use crate::state::AppState;
use op_llm::provider::{ChatMessage, ProviderType};
use std::str::FromStr;

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub message: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>, // For user-specific API credentials
    #[serde(default)]
    pub container_id: Option<String>, // Privacy container for memory scoping
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub success: bool,
    pub message: String,
    // Frontend-compatible aliases (openclaw/ChatPage.tsx expects these)
    pub content: String,
    pub role: String,
    pub id: String,
    pub tool_name: Option<String>,
    pub error: Option<String>,
    pub tools_executed: Vec<String>,
    pub session_id: String,
    pub model: String,
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
}

/// POST /api/chat - Main chat endpoint (Blocking)
pub async fn chat_handler(
    Extension(state): Extension<Arc<AppState>>,
    Json(request): Json<ChatRequest>,
) -> Json<ChatResponse> {
    info!(
        "Chat request: {} chars, user: {:?}",
        request.message.len(),
        request.user_id
    );

    let session_id = request
        .session_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Ensure session exists
    let _ = state
        .conversations
        .create_session(session_id.clone(), None)
        .await;

    // Resolve container_id: session-persisted first, then request, then user store
    let container_id = if let Some(cid) = state.conversations.get_container_id(&session_id).await {
        Some(cid)
    } else if let Some(cid) = request.container_id.clone() {
        // Persist in session for subsequent messages
        let _ = state
            .conversations
            .set_container_id(&session_id, cid.clone())
            .await;
        Some(cid)
    } else if let Some(user_id) = request.user_id.clone() {
        let cid = state
            .user_store
            .get_user(&user_id)
            .await
            .and_then(|u| u.privacy_container_name);
        if let Some(ref c) = cid {
            let _ = state
                .conversations
                .set_container_id(&session_id, c.clone())
                .await;
        }
        cid
    } else {
        None
    };
    let _ = state
        .conversations
        .append_message(&session_id, ChatMessage::user(request.message.clone()))
        .await;

    let fixed_prompt = op_chat::system_prompt::get_fixed_prompt();
    let (custom_prompt, _) = op_chat::system_prompt::load_custom_prompt().await;
    let system_prompt = if custom_prompt.trim().is_empty() {
        fixed_prompt.to_string()
    } else {
        format!("{fixed_prompt}\n\n{custom_prompt}")
    };
    let messages = vec![
        ChatMessage::system(system_prompt),
        ChatMessage::user(request.message.clone()),
    ];

    let requested_provider = request
        .provider
        .as_deref()
        .filter(|value| !value.is_empty());
    let requested_model = request.model.as_deref().filter(|value| !value.is_empty());
    let selected_model = match requested_model {
        Some(model) => model.to_string(),
        None => state.chat_manager.current_model().await,
    };

    if requested_provider.is_none()
        && let Err(e) = crate::chat_router_routes::ensure_model_available(&selected_model).await
    {
        return Json(ChatResponse {
            success: false,
            message: String::new(),
            content: e.to_string(),
            role: "assistant".to_string(),
            id: format!("msg-{}", uuid::Uuid::new_v4()),
            tool_name: None,
            error: Some(e.to_string()),
            tools_executed: vec![],
            session_id,
            model: selected_model,
            provider: "chat_router".to_string(),
            container_id,
        });
    }

    let result = match (requested_provider, requested_model) {
        (Some(provider), Some(model)) => match ProviderType::from_str(provider) {
            Ok(provider_type) if state.chat_manager.has_provider(&provider_type) => {
                state
                    .chat_manager
                    .chat_with(&provider_type, model, messages)
                    .await
            }
            Ok(provider_type) => Err(anyhow::anyhow!(
                "Provider {} is not available",
                provider_type
            )),
            Err(_) => Err(anyhow::anyhow!("Unknown provider: {}", provider)),
        },
        (Some(provider), None) => match ProviderType::from_str(provider) {
            Ok(provider_type) if state.chat_manager.has_provider(&provider_type) => {
                state
                    .chat_manager
                    .chat_with(&provider_type, &selected_model, messages)
                    .await
            }
            Ok(provider_type) => Err(anyhow::anyhow!(
                "Provider {} is not available",
                provider_type
            )),
            Err(_) => Err(anyhow::anyhow!("Unknown provider: {}", provider)),
        },
        (None, Some(model)) => match crate::chat_router_routes::route_for_model(model).await {
            Ok(Some(route)) => match ProviderType::from_str(&route.upstream_provider) {
                Ok(provider_type) if state.chat_manager.has_provider(&provider_type) => {
                    state
                        .chat_manager
                        .chat_with(&provider_type, model, messages)
                        .await
                }
                Ok(provider_type) => Err(anyhow::anyhow!(
                    "Provider {} is not available",
                    provider_type
                )),
                Err(_) => Err(anyhow::anyhow!(
                    "Route upstream provider is unknown: {}",
                    route.upstream_provider
                )),
            },
            Ok(None) => Err(anyhow::anyhow!("Model is not routed by 3tched Router")),
            Err(error) => Err(anyhow::anyhow!(
                "committed chat_router state unavailable: {error}"
            )),
        },
        _ => state.chat_manager.chat(messages).await,
    };

    match result {
        Ok(result) => {
            let provider = result.provider.clone();
            let model = result.model.clone();
            let response_message = result.message.content.clone();

            let _ = state
                .conversations
                .append_message(
                    &session_id,
                    ChatMessage::assistant(response_message.clone()),
                )
                .await;

            Json(ChatResponse {
                success: true,
                message: response_message.clone(),
                content: response_message.clone(),
                role: "assistant".to_string(),
                id: format!("msg-{}", uuid::Uuid::new_v4()),
                tool_name: None,
                error: None,
                tools_executed: result
                    .tool_calls
                    .unwrap_or_default()
                    .into_iter()
                    .map(|tool| tool.name)
                    .collect(),
                session_id,
                model,
                provider,
                container_id: container_id.clone(),
            })
        }
        Err(e) => {
            let provider = state.chat_manager.current_provider().await.to_string();
            let model = state.chat_manager.current_model().await;
            Json(ChatResponse {
                success: false,
                message: String::new(),
                content: e.to_string(),
                role: "assistant".to_string(),
                id: format!("msg-{}", uuid::Uuid::new_v4()),
                tool_name: None,
                error: Some(e.to_string()),
                tools_executed: vec![],
                session_id,
                model,
                provider,
                container_id,
            })
        }
    }
}

/// POST /api/chat/stream - Streaming chat endpoint (SSE)
pub async fn chat_stream_handler(
    Extension(state): Extension<Arc<AppState>>,
    Json(request): Json<ChatRequest>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let session_id = request
        .session_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let (tx, mut rx) = mpsc::channel(100);
    let state_clone = state.clone();
    let message = request.message.clone();
    let session_clone = session_id.clone();

    let _ = state
        .conversations
        .create_session(session_id.clone(), None)
        .await;
    let _ = state
        .conversations
        .append_message(&session_id, ChatMessage::user(request.message.clone()))
        .await;

    tokio::spawn(async move {
        let result = state_clone
            .orchestrator
            .process(&session_clone, &message, Some(tx.clone()))
            .await;

        match result {
            Ok(response) => {
                let _ = tx
                    .send(OrchestratorEvent::Finished {
                        success: response.success,
                        message: response.message.clone(),
                        tools_executed: response.tools_executed.clone(),
                    })
                    .await;
            }
            Err(e) => {
                let _ = tx
                    .send(OrchestratorEvent::Error {
                        message: e.to_string(),
                    })
                    .await;
            }
        }
    });

    // Create stream from receiver
    let stream = async_stream::stream! {
        while let Some(event) = rx.recv().await {
            yield Ok(Event::default().data(simd_json::to_string(&event).unwrap_or_default()));
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("ping"),
    )
}

/// GET /api/chat/history/:session_id - Get conversation history
pub async fn get_history_handler(
    Extension(state): Extension<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Json<Value> {
    if let Some(history) = state.conversations.get_history(&session_id).await {
        Json(json!({
            "session_id": session_id,
            "messages": history.iter().map(|m| json!({
                "role": m.role,
                "content": m.content
            })).collect::<Vec<_>>()
        }))
    } else {
        Json(json!({
            "session_id": session_id,
            "messages": []
        }))
    }
}

/// GET /api/chat/sessions - List all chat sessions
pub async fn list_sessions_handler(Extension(state): Extension<Arc<AppState>>) -> Json<Value> {
    let sessions: Vec<Value> = state
        .conversations
        .list_sessions()
        .await
        .into_iter()
        .map(|(id, msg_count, title)| {
            json!({
                "id": id,
                "title": title,
                "date": chrono::Utc::now().to_rfc3339(),
                "messages": msg_count
            })
        })
        .collect();

    Json(json!({ "sessions": sessions }))
}

/// POST /api/chat/sessions - Create a new chat session
pub async fn create_session_handler(
    Extension(state): Extension<Arc<AppState>>,
    Json(payload): Json<CreateSessionRequest>,
) -> (StatusCode, Json<Value>) {
    let session_id = uuid::Uuid::new_v4().to_string();
    let title = payload.title.unwrap_or_else(|| "New Chat".to_string());

    if let Err(err) = state
        .conversations
        .create_session(session_id.clone(), Some(title.clone()))
        .await
    {
        error!(%err, "Failed to create chat session");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Failed to persist chat session"})),
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "id": session_id,
            "title": title,
            "created_at": chrono::Utc::now().to_rfc3339(),
            "updated_at": chrono::Utc::now().to_rfc3339(),
            "message_count": 0
        })),
    )
}

/// DELETE /api/chat/sessions/:id - Delete a chat session
pub async fn delete_session_handler(
    Extension(state): Extension<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> (StatusCode, Json<Value>) {
    match state.conversations.delete_session(&session_id).await {
        Ok(true) => (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "message": format!("Session {} deleted", session_id)
            })),
        ),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Session not found"})),
        ),
        Err(err) => {
            error!(%err, %session_id, "Failed to delete chat session");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to persist chat session deletion"})),
            )
        }
    }
}

/// POST /api/chat/message - Send a message (creates new session if no session_id provided)
pub async fn send_message_handler(
    Extension(state): Extension<Arc<AppState>>,
    Json(mut request): Json<ChatRequest>,
) -> Json<ChatResponse> {
    // If no session_id, create a new one
    if request.session_id.is_none() {
        request.session_id = Some(uuid::Uuid::new_v4().to_string());
    }

    chat_handler(Extension(state), Json(request)).await
}

/// GET /api/chat/system-prompt - Get system prompt
pub async fn get_system_prompt_handler(Extension(_state): Extension<Arc<AppState>>) -> Json<Value> {
    let fixed = op_chat::system_prompt::get_fixed_prompt();
    let (custom, _source) = op_chat::system_prompt::load_custom_prompt().await;

    json!({
        "immutable": fixed,
        "tunable": custom,
        "custom": custom  // Alias for compatibility
    })
    .into()
}

/// PUT /api/chat/system-prompt - Update custom system prompt
pub async fn update_system_prompt_handler(
    Extension(_state): Extension<Arc<AppState>>,
    Json(payload): Json<Value>,
) -> Json<Value> {
    let content = payload.get("prompt").and_then(|v| v.as_str()).unwrap_or("");

    match op_chat::system_prompt::save_custom_prompt(content).await {
        Ok(path) => json!({
            "success": true,
            "message": "System prompt updated successfully",
            "path": path
        }),
        Err(e) => json!({
            "success": false,
            "error": e.to_string()
        }),
    }
    .into()
}

/// POST /api/chat/transcript - Save conversation transcript to file
/// Accepts either a session_id to save from memory, or direct messages array
pub async fn save_transcript_handler(
    Extension(state): Extension<Arc<AppState>>,
    Json(params): Json<Value>,
) -> (axum::http::StatusCode, Json<Value>) {
    let filename = params
        .get("filename")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| format!("chat-transcript-{}.txt", uuid::Uuid::new_v4()));
    if !valid_transcript_filename(&filename) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false, "error": "Invalid transcript filename"
            })),
        );
    }

    // Check if session_id is provided (for existing conversations)
    if let Some(session_id) = params.get("session_id").and_then(|v| v.as_str()) {
        if let Some(history) = state.conversations.get_history(session_id).await {
            if history.is_empty() {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "error": "No messages in conversation"
                    })),
                );
            }
            return save_transcript_to_file(&history, filename.as_str(), Some(session_id)).await;
        } else {
            return (
                axum::http::StatusCode::NOT_FOUND,
                Json(json!({
                    "success": false,
                    "error": "Conversation not found"
                })),
            );
        }
    }

    // Check if messages array is provided directly
    if let Some(messages) = params.get("messages").and_then(|v| v.as_array()) {
        if messages.is_empty() {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({
                    "success": false,
                    "error": "No messages provided"
                })),
            );
        }

        // Convert Value array to ChatMessage vector
        let mut history = Vec::new();
        for msg in messages {
            if let (Some(role), Some(content)) = (
                msg.get("role").and_then(|v| v.as_str()),
                msg.get("content").and_then(|v| v.as_str()),
            ) {
                history.push(op_llm::ChatMessage {
                    role: role.to_string(),
                    content: content.to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }

        if history.is_empty() {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(json!({
                    "success": false,
                    "error": "Invalid message format"
                })),
            );
        }

        return save_transcript_to_file(&history, filename.as_str(), None).await;
    }

    (
        axum::http::StatusCode::BAD_REQUEST,
        Json(json!({
            "success": false,
            "error": "Either session_id or messages array required",
            "usage": {
                "by_session_id": {"session_id": "session-123", "filename": "optional-filename.txt"},
                "by_messages": {"messages": [{"role": "user", "content": "Hello"}, {"role": "assistant", "content": "Hi"}], "filename": "optional-filename.txt"}
            }
        })),
    )
}

fn valid_transcript_filename(filename: &str) -> bool {
    !filename.is_empty()
        && filename.len() <= 128
        && filename != "."
        && filename != ".."
        && filename
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

async fn save_transcript_to_file(
    history: &[op_llm::ChatMessage],
    filename: &str,
    session_id: Option<&str>,
) -> (axum::http::StatusCode, Json<Value>) {
    save_transcript_to_file_in(history, filename, session_id, std::path::Path::new("/tmp")).await
}

async fn save_transcript_to_file_in(
    history: &[op_llm::ChatMessage],
    filename: &str,
    session_id: Option<&str>,
    root: &std::path::Path,
) -> (axum::http::StatusCode, Json<Value>) {
    if !valid_transcript_filename(filename) {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false, "error": "Invalid transcript filename"
            })),
        );
    }
    // Format transcript
    let mut transcript = String::new();

    if let Some(session) = session_id {
        transcript.push_str(&format!(
            "Chat Transcript - Session: {}
",
            session
        ));
    } else {
        transcript.push_str("Chat Transcript\n");
    }

    transcript.push_str(&format!(
        "Generated: {}
",
        chrono::Utc::now().to_rfc3339()
    ));
    transcript.push_str(&"=".repeat(50));
    transcript.push_str("\n\n");

    for (i, message) in history.iter().enumerate() {
        let role = match message.role.as_str() {
            "user" => "👤 User",
            "assistant" => "🤖 Assistant",
            "system" => "⚙️ System",
            _ => "Unknown",
        };
        transcript.push_str(&format!(
            "[{}] {}
\n",
            role, message.content
        ));
        if i < history.len() - 1 {
            let separator = "─".repeat(30);
            transcript.push_str(&separator);
            transcript.push_str("\n\n");
        }
    }

    // Save to file
    let filepath = root.join(filename);
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    let result = options.open(&filepath).await;
    match result {
        Ok(mut file) => {
            use tokio::io::AsyncWriteExt;
            if let Err(e) = file.write_all(transcript.as_bytes()).await {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "success": false, "error": format!("Failed to save transcript: {}", e)
                    })),
                );
            }
            (
                axum::http::StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": "Transcript saved successfully",
                    "filepath": filepath.to_string_lossy(),
                    "filename": filename,
                    "message_count": history.len(),
                    "transcript_preview": transcript.chars().take(200).collect::<String>() + "..."
                })),
            )
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "error": format!("Failed to save transcript: {}", e)
            })),
        ),
    }
}

#[cfg(test)]
mod transcript_security_tests {
    use super::*;

    fn message() -> op_llm::ChatMessage {
        op_llm::ChatMessage {
            role: "user".to_string(),
            content: "hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[tokio::test]
    async fn transcript_rejects_escape_and_existing_symlink() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let sentinel = outside.path().join("sentinel.txt");
        tokio::fs::write(&sentinel, "keep").await.unwrap();
        let (status, _) =
            save_transcript_to_file_in(&[message()], "../sentinel.txt", None, root.path()).await;
        assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(tokio::fs::read_to_string(&sentinel).await.unwrap(), "keep");

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&sentinel, root.path().join("linked.txt")).unwrap();
            let (status, _) =
                save_transcript_to_file_in(&[message()], "linked.txt", None, root.path()).await;
            assert!(!status.is_success());
            assert_eq!(tokio::fs::read_to_string(&sentinel).await.unwrap(), "keep");
        }
        let (status, _) =
            save_transcript_to_file_in(&[message()], "valid.txt", None, root.path()).await;
        assert_eq!(status, axum::http::StatusCode::OK);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(root.path().join("valid.txt"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
