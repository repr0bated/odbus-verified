//! op-jsonrpc: JSON-RPC server with OVSDB and NonNet support
//!
//! This crate provides:
//! - JSON-RPC 2.0 server over Unix sockets
//! - OVSDB client for Open vSwitch integration
//! - NonNet database for non-network plugin state

pub mod ovsdb;
pub mod protocol;
pub mod server;
pub use ovsdb::OvsdbDbusClient;
pub use server::JsonRpcServer;

/// Prelude for convenient imports
pub mod prelude {
    pub use super::ovsdb::OvsdbDbusClient;
    pub use super::protocol::{JsonRpcRequest, JsonRpcResponse};
    pub use super::server::JsonRpcServer;
}
