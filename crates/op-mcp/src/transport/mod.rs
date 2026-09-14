//! Transport Layer
//!
//! Provides multiple transport implementations:
//! - Stdio (standard input/output)
//! - HTTP (REST endpoints)
//! - SSE (Server-Sent Events)
//! - HTTP+SSE (bidirectional)
//! - WebSocket (full duplex)
//! - gRPC (high-performance RPC) [optional feature]

mod http;
mod stdio;
mod websocket;

pub use http::{HttpSseTransport, HttpTransport, SseTransport};
pub use stdio::StdioTransport;
pub use websocket::WebSocketTransport;

use anyhow::Result;
use std::sync::Arc;

/// Generic MCP server trait for transport layer
#[async_trait::async_trait]
pub trait McpHandler: Send + Sync {
    async fn handle_request(&self, request: crate::McpRequest) -> crate::McpResponse;
}

/// Transport trait - implement for new transport types
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    /// Serve requests using this transport
    async fn serve<H: McpHandler + 'static>(self, handler: Arc<H>) -> Result<()>;
}

// Implement McpHandler for all server types
#[async_trait::async_trait]
impl McpHandler for crate::McpServer {
    async fn handle_request(&self, request: crate::McpRequest) -> crate::McpResponse {
        self.handle_request(request).await
    }
}

#[async_trait::async_trait]
impl McpHandler for crate::AgentsServer {
    async fn handle_request(&self, request: crate::McpRequest) -> crate::McpResponse {
        self.handle_request(request).await
    }
}
