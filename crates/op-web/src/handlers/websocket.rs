//! WebSocket Handler for Real-time Chat
//!
//! Provides bidirectional communication for chat interface.

use axum::{
    extract::{Extension, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::Response,
};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use simd_json::json;
use std::sync::Arc;
use tracing::{info, warn, error};

use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WsMessage {
    /// Chat message from user
    Chat { message: String, session_id: Option<String> },
    /// Response from server
    Response { success: bool, message: String, tools_executed: Vec<String> },
    /// System message
    System { message: String },
    /// Error message
    Error { message: String },
    /// Ping/pong for keepalive
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

/// Handle WebSocket connection
async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    // Generate session ID
    let session_id = uuid::Uuid::new_v4().to_string();
    info!("WebSocket connected: {}", &session_id[..8]);

    // Subscribe to broadcast channel
    let mut broadcast_rx = state.broadcast_tx.subscribe();

    // Send welcome message
    let welcome = WsMessage::System {
        message: format!(
            "Welcome to op-dbus! Session: {}\nType 'help' for available commands.",
            &session_id[..8]
        ),
    };
    if let Err(e) = sender.send(Message::Text(simd_json::to_string(&welcome).expect("websocket message serialization should not fail"))).await {
        error!("Failed to send welcome: {}", e);
        return;
    }

    // Spawn task to handle broadcast messages
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = broadcast_rx.recv().await {
            if sender.send(Message::Text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Clone state for receive task
    let state_clone = state.clone();
    let session_clone = session_id.clone();

    // Handle incoming messages
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    // Try to parse as WsMessage
                    let ws_msg: Result<WsMessage, _> = serde_json::from_str(&text);
                    
                    match ws_msg {
                        Ok(WsMessage::Chat { message, session_id: sid }) => {
                            let sid = sid.unwrap_or_else(|| session_clone.clone());
                            
                            // Process through orchestrator
                            match state_clone.orchestrator.process(&sid, &message).await {
                                Ok(result) => {
                                    let response = WsMessage::Response {
                                        success: result.success,
                                        message: result.message,
                                        tools_executed: result.tools_executed,
                                    };
                                    
                                    // Broadcast to all connected clients
                                    let _ = state_clone.broadcast_tx.send(
                                        simd_json::to_string(&response).expect("websocket message serialization should not fail")
                                    );
                                }
                                Err(e) => {
                                    let error = WsMessage::Error {
                                        message: e.to_string(),
                                    };
                                    let _ = state_clone.broadcast_tx.send(
                                        simd_json::to_string(&error).expect("websocket message serialization should not fail")
                                    );
                                }
                            }
                        }
                        Ok(WsMessage::Ping) => {
                            let pong = WsMessage::Pong;
                            let _ = state_clone.broadcast_tx.send(
                                simd_json::to_string(&pong).expect("websocket message serialization should not fail")
                            );
                        }
                        _ => {
                            // Treat as plain text chat message
                            if !text.trim().is_empty() {
                                match state_clone.orchestrator.process(&session_clone, &text).await {
                                    Ok(result) => {
                                        let response = WsMessage::Response {
                                            success: result.success,
                                            message: result.message,
                                            tools_executed: result.tools_executed,
                                        };
                                        let _ = state_clone.broadcast_tx.send(
                                            simd_json::to_string(&response).expect("websocket message serialization should not fail")
                                        );
                                    }
                                    Err(e) => {
                                        let error = WsMessage::Error {
                                            message: e.to_string(),
                                        };
                                        let _ = state_clone.broadcast_tx.send(
                                            simd_json::to_string(&error).expect("websocket message serialization should not fail")
                                        );
                                    }
                                }
                            }
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
        _ = (&mut send_task) => {
            recv_task.abort();
        }
        _ = (&mut recv_task) => {
            send_task.abort();
        }
    }

    info!("WebSocket disconnected: {}", &session_id[..8]);
}
