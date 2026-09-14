//! op-gateway: MCP Gateway with WireGuard authentication and smart routing

pub mod encrypted_storage;
mod error;
pub mod mcp_gateway;
pub mod wireguard_auth;

pub use encrypted_storage::{
    EncryptedKeyEntry, EncryptedKeyStorage, EncryptedStorageConfig, KdfParams, KeyType,
    StorageStats,
};
pub use error::GatewayError;
pub use mcp_gateway::{AccessLevel, McpClientInfo, McpGatewayManager, McpSession, RoutingDecision};
pub use wireguard_auth::{
    ClientInfo, SessionFilter, SimdCryptoEngine, WireGuardAuthManager, WireGuardDatabase,
    WireGuardSession, WireGuardStats,
};
