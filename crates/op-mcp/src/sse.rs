//! SSE (Server-Sent Events) Transport for MCP
//!
//! Allows MCP server to run as a long-lived HTTP daemon.
//! Clients connect via SSE for responses and POST for requests.

use crate::{McpRequest, McpServer};
use axum::{
    extract::State,
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use futures::stream::Stream;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

/// SSE Transport state
pub struct SseTransport {
    mcp_server: Arc<McpServer>,
    /// Broadcast channel for SSE events
    event_tx: broadcast::Sender<String>,
}

impl SseTransport {
    pub fn new(mcp_server: Arc<McpServer>) -> Self {
        let (event_tx, _) = broadcast::channel(100);
        Self {
            mcp_server,
            event_tx,
        }
    }

    /// Create the Axum router for SSE transport
    pub fn router(self) -> Router {
        let state = Arc::new(self);

        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        Router::new()
            .route("/sse", get(sse_handler))
            .route("/message", post(message_handler))
            .route("/health", get(health_handler))
            .with_state(state)
            .layer(cors)
    }
}

/// SSE endpoint - clients connect here to receive responses
async fn sse_handler(
    State(state): State<Arc<SseTransport>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    info!("SSE client connected");

    let rx = state.event_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        match result {
            Ok(data) => Some(Ok(Event::default().data(data))),
            Err(_) => None, // Skip lagged messages
        }
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("ping"),
    )
}

/// Message endpoint - clients POST MCP requests here
async fn message_handler(
    State(state): State<Arc<SseTransport>>,
    Json(request): Json<McpRequest>,
) -> Json<simd_json::OwnedValue> {
    info!("Received MCP request via HTTP: {}", request.method);

    // Handle the request
    let response = state.mcp_server.handle_request(request).await;

    // Do NOT broadcast command responses to all SSE clients.
    // Responses should only go back to the caller.
    // The shared event_tx channel should be reserved for actual server events/notifications.

    // Return response directly (for non-SSE clients)
    Json(simd_json::serde::to_owned_value(&response).unwrap_or_default())
}

/// Health check endpoint
async fn health_handler() -> &'static str {
    "ok"
}

/// Run the SSE server
pub async fn run_sse_server(mcp_server: Arc<McpServer>, bind_addr: &str) -> anyhow::Result<()> {
    let transport = SseTransport::new(mcp_server);
    let app = transport.router();

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    info!("MCP SSE server listening on {}", bind_addr);

    axum::serve(listener, app).await?;
    Ok(())
}
