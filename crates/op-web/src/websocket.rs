//! WebSocket Handler for Real-time Chat
//!
//! Session-isolated WebSocket connections. Each connection only receives
//! events for its own session, preventing cross-session information leakage.

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Extension, WebSocketUpgrade,
    },
    response::Response,
};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::orchestrator::OrchestratorEvent;
use crate::state::AppState;
use op_llm::provider::ChatMessage;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    Chat {
        message: String,
        session_id: Option<String>,
    },
    Response {
        success: bool,
        message: String,
        tools_executed: Vec<String>,
    },
    Event {
        data: OrchestratorEvent,
    },
    System {
        message: String,
    },
    Error {
        message: String,
    },
    Ping,
    Pong,
}

/// WebSocket upgrade handler
pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    Extension(state): Extension<Arc<AppState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    let session_id = uuid::Uuid::new_v4().to_string();
    info!("WebSocket connected: {}", &session_id[..8]);

    // Create session-specific channel for outbound messages
    // This ensures events are only sent to THIS connection, not all connections
    let (session_tx, mut session_rx) = mpsc::channel::<String>(100);

    // Send welcome message
    let welcome = WsMessage::System {
        message: format!(
            "Connected to op-dbus server. Session: {}\nType 'help' for commands.",
            &session_id[..8]
        ),
    };
    if let Err(e) = ws_sender
        .send(Message::Text(
            simd_json::to_string(&welcome)
                .expect("websocket message serialization should not fail"),
        ))
        .await
    {
        error!("Failed to send welcome: {}", e);
        return;
    }

    // Clone for tasks
    let state_clone = state.clone();
    let session_clone = session_id.clone();
    let session_tx_clone = session_tx.clone();

    // Task to send messages from session channel to WebSocket
    let mut send_task = tokio::spawn(async move {
        while let Some(msg) = session_rx.recv().await {
            if ws_sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            match msg {
                Message::Text(text) => {
                    debug!("WS received: {}", text);

                    // Try to parse as WsMessage
                    let ws_msg: Result<WsMessage, _> = serde_json::from_str(&text);

                    let message_text = match ws_msg {
                        Ok(WsMessage::Chat { message, .. }) => message,
                        Ok(WsMessage::Ping) => {
                            let pong = WsMessage::Pong;
                            let _ = session_tx_clone
                                .send(
                                    simd_json::to_string(&pong)
                                        .expect("websocket message serialization should not fail"),
                                )
                                .await;
                            continue;
                        }
                        _ => text.clone(), // Treat as plain text
                    };

                    if message_text.trim().is_empty() {
                        continue;
                    }

                    // Create channel for streaming orchestrator events
                    let (event_tx, mut event_rx) = mpsc::channel::<OrchestratorEvent>(100);
                    let session_tx_for_events = session_tx_clone.clone();

                    // Spawn task to forward orchestrator events to session channel
                    tokio::spawn(async move {
                        while let Some(event) = event_rx.recv().await {
                            let ws_event = WsMessage::Event { data: event };
                            if let Ok(json) = simd_json::to_string(&ws_event) {
                                if session_tx_for_events.send(json).await.is_err() {
                                    break;
                                }
                            }
                        }
                    });

                    // Process through orchestrator with streaming
                    match state_clone
                        .orchestrator
                        .process(&session_clone, &message_text, Some(event_tx))
                        .await
                    {
                        Ok(result) => {
                            // Store in conversation history
                            {
                                let _ = state_clone
                                    .conversations
                                    .create_session(session_clone.clone(), None)
                                    .await;
                                let _ = state_clone
                                    .conversations
                                    .append_message(
                                        &session_clone,
                                        ChatMessage::user(&message_text),
                                    )
                                    .await;
                                let _ = state_clone
                                    .conversations
                                    .append_message(
                                        &session_clone,
                                        ChatMessage::assistant(&result.message),
                                    )
                                    .await;
                            }

                            let response = WsMessage::Response {
                                success: result.success,
                                message: result.message,
                                tools_executed: result.tools_executed,
                            };
                            let _ = session_tx_clone
                                .send(
                                    simd_json::to_string(&response)
                                        .expect("websocket message serialization should not fail"),
                                )
                                .await;
                        }
                        Err(e) => {
                            let error = WsMessage::Error {
                                message: e.to_string(),
                            };
                            let _ = session_tx_clone
                                .send(
                                    simd_json::to_string(&error)
                                        .expect("websocket message serialization should not fail"),
                                )
                                .await;
                        }
                    }
                }
                Message::Close(_) => {
                    info!("WebSocket closed: {}", &session_clone[..8]);
                    break;
                }
                _ => {}
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    }

    info!("WebSocket disconnected: {}", &session_id[..8]);
}
