//! op-mcp-aggregator: MCP Server Aggregator
//!
//! This crate provides an aggregator that proxies multiple upstream MCP servers,
//! presenting a unified tool interface while staying under Cursor's 40-tool limit.
//!
//! ## Modes
//!
//! ### Full Mode (Traditional)
//! Exposes all tools directly. Good for small tool sets (<40 tools).
//!
//! ### Compact Mode (Recommended)
//! Reduces 750+ tools to 4-5 meta-tools:
//! - `list_tools` - Browse available tools
//! - `search_tools` - Find tools by keyword  
//! - `get_tool_schema` - Get input schema for a tool
//! - `execute_tool` - Execute any tool by name
//!
//! Benefits:
//! - ~95% context token savings
//! - Bypasses 40-tool limit entirely
//! - Works with Cursor, Gemini CLI, and any MCP client
//! - All tools remain accessible via execute_tool
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    op-mcp-aggregator                        │
//! │  ┌───────────────────────────────────────────────────────┐  │
//! │  │              Mode: Compact / Full                     │  │
//! │  │  Compact: 4 meta-tools (list, search, schema, exec)  │  │
//! │  │  Full: All tools from all servers                     │  │
//! │  └───────────────────────────────────────────────────────┘  │
//! │                           │                                  │
//! │  ┌───────────────────────────────────────────────────────┐  │
//! │  │              Profile Manager                          │  │
//! │  │  /profile/sysadmin → [systemd, network, dbus]        │  │
//! │  │  /profile/dev      → [github, filesystem, shell]     │  │
//! │  └───────────────────────────────────────────────────────┘  │
//! │                           │                                  │
//! │  ┌───────────────────────────────────────────────────────┐  │
//! │  │              Upstream Registry                        │  │
//! │  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐     │  │
//! │  │  │ GitHub  │ │ Postgres│ │ Custom  │ │ Local   │     │  │
//! │  │  │ MCP     │ │ MCP     │ │ Server  │ │ Tools   │     │  │
//! │  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘     │  │
//! │  └───────────────────────────────────────────────────────┘  │
//! │                           │                                  │
//! │  ┌───────────────────────────────────────────────────────┐  │
//! │  │              Tool Cache (LRU + TTL)                   │  │
//! │  └───────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use op_mcp_aggregator::{Aggregator, AggregatorConfig, ToolMode};
//!
//! let config = AggregatorConfig::load("/etc/op-dbus/aggregator.json")?;
//! let aggregator = Aggregator::new(config).await?;
//! aggregator.initialize().await?;
//!
//! // Get MCP tools (compact mode returns 4 meta-tools)
//! let mcp_tools = aggregator.get_mcp_tools(ToolMode::Compact).await?;
//!
//! // Or use full mode for direct tool access
//! let all_tools = aggregator.get_mcp_tools(ToolMode::Full).await?;
//! ```

pub mod aggregator;
pub mod cache;
pub mod client;
pub mod compact;
pub mod config;
pub mod groups;
pub mod profile; // Used by op-web for IP-based security

// Re-exports
pub use aggregator::{Aggregator, AggregatorStats, HealthStatus, ToolMode};
pub use cache::ToolCache;
pub use client::McpClient;
pub use compact::{compact_mode_summary, create_compact_tools, CompactModeConfig};
pub use config::{AggregatorConfig, ProfileConfig, UpstreamServer};
pub use groups::{builtin_groups, builtin_presets};
pub use op_core::security::{AccessZone, NetworkConfig, SecurityLevel};
pub use profile::ProfileManager;

/// Prelude for convenient imports
pub mod prelude {
    pub use super::{
        create_compact_tools, AccessZone, Aggregator, AggregatorConfig, CompactModeConfig,
        McpClient, NetworkConfig, ProfileConfig, ProfileManager, SecurityLevel, ToolCache,
        ToolMode, UpstreamServer,
    };
}
