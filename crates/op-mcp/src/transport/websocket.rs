//! WebSocket Transport
//!
//! Full-duplex WebSocket transport for MCP.

use super::{McpHandler, Transport};
use crate::{JsonRpcError, McpRequest, McpResponse};
use anyhow::Result;
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::{SinkExt, StreamExt};
use simd_json::json;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::{debug, error, info, warn};

/// WebSocket transport
pub struct WebSocketTransport {
    bind_addr: String,
}

impl WebSocketTransport {
    pub fn new(bind_addr: impl Into<String>) -> Self {
        Self {
            bind_addr: bind_addr.into(),
        }
    }
}

struct WsState<H> {
    handler: Arc<H>,
}

#[async_trait::async_trait]
impl Transport for WebSocketTransport {
    async fn serve<H: McpHandler + 'static>(self, handler: Arc<H>) -> Result<()> {
        info!(addr = %self.bind_addr, "Starting WebSocket transport");

        let state = Arc::new(WsState { handler });

        let app = Router::new()
            .route("/", get(ws_handler::<H>))
            .route("/ws", get(ws_handler::<H>))
            .route("/health", get(health_handler))
            .layer(
                CorsLayer::new()
                    .allow_origin(Any)
                    .allow_methods(Any)
                    .allow_headers(Any),
            )
            .with_state(state);

        let listener = tokio::net::TcpListener::bind(&self.bind_addr).await?;
        info!(addr = %self.bind_addr, "WebSocket transport listening");

        axum::serve(listener, app).await?;
        Ok(())
    }
}

async fn health_handler() -> impl IntoResponse {
    axum::Json(json!({
        "status": "ok",
        "transport": "websocket"
    }))
}

async fn ws_handler<H: McpHandler + 'static>(
    ws: WebSocketUpgrade,
    State(state): State<Arc<WsState<H>>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, state))
}

async fn handle_ws_connection<H: McpHandler>(socket: WebSocket, state: Arc<WsState<H>>) {
    info!("WebSocket client connected");

    let (mut sender, mut receiver) = socket.split();

    // Send welcome message
    let welcome = json!({
        "type": "welcome",
        "server": "op-mcp",
        "version": crate::SERVER_VERSION,
        "protocol": crate::PROTOCOL_VERSION
    });

    if let Err(e) = sender.send(Message::Text(welcome.to_string())).await {
        error!(error = %e, "Failed to send welcome");
        return;
    }

    // Handle incoming messages
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                debug!(request = %text, "WebSocket request");

                // axum 0.8 hands `Message::Text` back as `Utf8Bytes`, which is
                // immutable behind its Deref. simd_json parses in place, so it
                // needs an owned buffer of its own.
                let mut text_mut = text.as_str().to_owned();
                let response = match unsafe { simd_json::from_str::<McpRequest>(&mut text_mut) } {
                    Ok(request) => state.handler.handle_request(request).await,
                    Err(e) => {
                        warn!(error = %e, "Invalid request");
                        McpResponse::error(None, JsonRpcError::parse_error(e.to_string()))
                    }
                };

                let response_json = simd_json::to_string(&response).unwrap_or_default();

                if let Err(e) = sender.send(Message::Text(response_json)).await {
                    error!(error = %e, "Failed to send response");
                    break;
                }
            }
            Ok(Message::Ping(data)) => {
                if let Err(e) = sender.send(Message::Pong(data)).await {
                    error!(error = %e, "Failed to send pong");
                    break;
                }
            }
            Ok(Message::Close(_)) => {
                info!("WebSocket client disconnected");
                break;
            }
            Ok(_) => {} // Ignore binary, pong, etc.
            Err(e) => {
                error!(error = %e, "WebSocket error");
                break;
            }
        }
    }

    info!("WebSocket connection closed");
}
