//! gRPC service implementations for op-cache
//!
//! Provides:
//! - AgentService: Register and execute agents
//! - OrchestratorService: Route requests and manage workstacks
//! - CacheService: Workstack step caching
//! - McpService: MCP JSON-RPC bridge to agents
//! - EmbeddingService: Vector embedding cache
//! - SnapshotService: BTRFS snapshot management

pub mod agent_service;
pub mod cache_service;
pub mod mcp_service;
pub mod orchestrator_service;
pub mod server;

pub use agent_service::AgentServiceImpl;
pub use cache_service::CacheServiceImpl;
pub use mcp_service::McpServiceImpl;
pub use orchestrator_service::OrchestratorServiceImpl;
pub use server::{GrpcServer, GrpcServerConfig};

// Re-export generated protobuf types
pub mod proto {
    pub use crate::proto::*;
}
