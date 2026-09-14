//! op-mcp: MCP protocol and tool-runtime library.
//!
//! The crate owns no production listener binary. `op-grpc-bridge` owns the
//! authenticated external MCP frontend and uses these protocol/tool primitives
//! for embedded execution. Library consumers can compose the following modes:
//! - **Compact**: 5 meta-tools with per-request lazy tool loading (recommended for LLMs)
//! - **Agents**: Always-on cognitive agents (memory, sequential_thinking, etc.)
//! - **Full**: All tools directly exposed (may hit client limits)
//! - **BlobSchema**: sealed plugin-blob catalog (schema/manifest/methods)
//!
//! Supports multiple transports:
//! - Stdio (standard MCP transport)
//! - HTTP (REST endpoints)
//! - SSE (Server-Sent Events)
//! - HTTP+SSE (bidirectional)
//! - WebSocket (full duplex)
//! - gRPC (high-performance RPC)

pub mod agents_server;
pub mod blob_schema;
pub mod cognitive_bridge;
pub mod compact;
pub mod external_client;
pub mod protocol;
pub mod resources;
pub mod server;
pub mod transport;

pub mod tool_registry;

#[cfg(feature = "grpc")]
pub mod grpc;

// Re-exports
pub use agents_server::AgentsServer;
pub use blob_schema::BlobSchemaExecutor;
pub use compact::{
    run_compact_stdio_server, run_compact_unix_server, CompactServer, PrewarmedOpToolsExecutor,
    SessionContext,
};
pub use external_client::{
    AuthMethod, ExternalMcpClient, ExternalMcpConfig, ExternalMcpManager, ExternalTool,
};
pub use op_core::SecurityLevel;
pub use protocol::{JsonRpcError, McpError, McpRequest, McpResponse};
pub use resources::ResourceRegistry;
pub use server::{DefaultToolExecutor, McpServer, McpServerConfig, ToolExecutor, ToolInfo};
pub use tool_registry::{Tool, ToolRegistry};
pub use transport::{
    HttpSseTransport, HttpTransport, SseTransport, StdioTransport, Transport, WebSocketTransport,
};

#[cfg(feature = "grpc")]
pub use grpc::GrpcTransport;

/// Protocol version
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// Server info
pub const SERVER_NAME: &str = "op-mcp";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Server mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerMode {
    /// 5 meta-tools for tool discovery and response
    Compact,
    /// Always-on cognitive agents
    Agents,
    /// All tools directly exposed
    Full,
    /// Cognitive tools sourced from op-grpc-bridge (fan-in proxy).
    ///
    /// One authenticated caller fronting the bridge, so MCP clients need no
    /// credential and the cognitive store keeps a single writer.
    Cognitive,
    /// Read-only sealed blob catalog: PluginSchema, manifest, methods.
    BlobSchema,
}

impl std::fmt::Display for ServerMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerMode::Compact => write!(f, "compact"),
            ServerMode::Agents => write!(f, "agents"),
            ServerMode::Full => write!(f, "full"),
            ServerMode::Cognitive => write!(f, "cognitive"),
            ServerMode::BlobSchema => write!(f, "blob-schema"),
        }
    }
}

impl std::str::FromStr for ServerMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "compact" => Ok(ServerMode::Compact),
            "agents" => Ok(ServerMode::Agents),
            "full" | "standard" => Ok(ServerMode::Full),
            "cognitive" | "bridge" => Ok(ServerMode::Cognitive),
            "blob" | "blob-schema" | "schema" => Ok(ServerMode::BlobSchema),
            _ => Err(format!("Unknown server mode: {}", s)),
        }
    }
}
