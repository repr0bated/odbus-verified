//! gRPC Transport for op-mcp
//!
//! Provides high-performance gRPC transport for MCP protocol.
//!
//! ## Features
//! - Unary request/response (standard MCP calls)
//! - Server streaming (SSE-like events)
//! - Bidirectional streaming (full duplex)
//! - Run-on-connection agent support
//! - BTRFS cache integration
//! - StateStore execution tracking
//! - Snowball audit trail

#[cfg(feature = "grpc")]
mod client;
#[cfg(feature = "grpc")]
mod server;
#[cfg(feature = "grpc")]
mod service;

#[cfg(feature = "grpc")]
pub use crate::ServerMode as GrpcServerMode; // Direct export from crate root
#[cfg(feature = "grpc")]
pub use client::{GrpcClient, GrpcClientConfig};
#[cfg(feature = "grpc")]
pub use server::{GrpcConfig, GrpcTransport};
#[cfg(feature = "grpc")]
pub use service::{GrpcInfrastructure, McpGrpcService};

// Include generated protobuf code
#[cfg(feature = "grpc")]
pub mod proto {
    tonic::include_proto!("op.mcp.v1");

    /// Combined FileDescriptorSet for reflection.
    pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("mcp_descriptor");
}
