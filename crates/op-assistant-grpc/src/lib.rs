//! op-assistant-grpc — gRPC gateway for Assistant integration.
//!
//! Architecture:
//! ```text
//!  gRPC Client  →  AssistantGrpcServer  →  Transport (D-Bus | HTTP-RPC)  →  Assistant
//! ```
//!
//! - Authentication verifies session genesis against the identity projection
//!   (same gate as op-cognitive-mcp).
//! - Primary transport is D-Bus; falls back to JSON-RPC over HTTP when D-Bus is unavailable.
//! - Each Assistant API surface (agents, sessions, tasks, models, cron, soul, namespace,
//!   memory) is exposed as its own gRPC service.

pub mod agents;
pub mod auth;
pub mod client;
pub mod cognitive_client;
pub mod convert;
pub mod cron;
pub mod dbus_service;
pub mod error;
pub mod incus;
pub mod memory;
pub mod models;
pub mod namespace;
pub mod server;
pub mod sessions;
pub mod soul;
pub mod tasks;
pub mod transport;

pub use auth::{wireguard_auth_interceptor, WireGuardIdentity};
pub use client::AssistantClient;
pub use error::AssistantError;
pub use server::{run_grpc_server, AssistantGrpcServer, ServerConfig};
pub use transport::{Transport, TransportConfig, TransportKind};

/// Generated protobuf types.
pub mod proto {
    tonic::include_proto!("assistant.v1");

    /// Combined FileDescriptorSet for tonic-reflection.
    pub const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("assistant_descriptor");
}
