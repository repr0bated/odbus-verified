//! `op-mcp-door`: the external MCP endpoint on the door.
//!
//! TLS-only MCP Streamable HTTP in front; 3tchedFS call files behind. The
//! door opens no gRPC channel, D-Bus connection or bridge socket: whatever
//! serves a FUSE call behind the mount is not its concern.
//! Spec: `.kiro/specs/mcp-door-adapter/`.

pub mod app;
pub mod audit;
pub mod canonical;
pub mod catalog;
pub mod classify;
pub mod config;
pub mod encode;
pub mod fusecall;
pub mod grants;
pub mod identity;
pub mod mcp;
pub mod policy;
pub mod toolsets;
