//! op-mcp: Model Context Protocol implementations
//!
//! This crate provides MCP servers and tools for AI agent integration.

pub mod agents_server;
pub mod builtin_trait_agents;
pub mod compact_server;
pub mod critical;
pub mod stdio_server;
pub mod tool_adapter;

pub use agents_server::{AgentsServer, AgentsServerConfig, AgentDefinition, ExecutorType};
pub use builtin_trait_agents::register_builtin_agents;
