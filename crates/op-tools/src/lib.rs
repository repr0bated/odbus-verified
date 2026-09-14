//! op-tools: Tool Registry and Execution
//!
//! Provides the tool registry, built-in tools, and HTTP router.
//!
//! ## Security
//!
//! Security is enforced at the ACCESS level, not command level:
//! - **Unrestricted (Admin)**: Full access - can run any command
//! - **Restricted**: Limited read-only access for untrusted users
//!
//! The chatbot is designed to be a full system administrator.
//! Rate limiting prevents runaway loops.
//!
//! ## Orchestration Plugin
//!
//! The `orchestration_plugin` module provides hooks for tracking all activity:
//! - Tool executions (commands, file ops, etc.)
//! - LLM decisions and tool calls
//! - Session lifecycle events
//!
//! This integrates with snowball for immutable audit logging.

pub mod builtin;
pub mod discovery;
pub mod dynamic_tool;
mod mcptools;
pub mod orchestration_plugin;
pub mod registry;
pub mod router;
pub mod security;
pub mod tool;
pub mod validation;

use tracing::warn;

// Re-export main types
pub use orchestration_plugin::{
    create_tool_event, get_orchestration_registry, LlmDecisionEvent, OrchestrationActivityPlugin,
    OrchestrationPluginRegistry, SessionEvent, ToolExecutedEvent,
};

// Re-export code_search module (feature-gated in consuming crates)
pub use builtin::code_search;

pub use registry::ToolRegistry;
pub use router::{create_router, ToolsServiceRouter, ToolsState};
pub use security::{
    get_security_validator, AccessLevel, SecurityError, SecurityValidator, ToolSecurityProfile,
};
pub use tool::{BoxedTool, Tool};
pub use validation::{InputValidator, ValidatedInput, ValidationConfig};

/// Public error type for tool registry operations.
///
/// Uses `thiserror` per AGENTS.md coding standards.  This keeps `anyhow`
/// out of the public API while preserving ergonomic `?` propagation
/// inside the crate.
#[derive(Debug, thiserror::Error)]
pub enum ToolsError {
    #[error("tool registration failed: {0}")]
    Registration(String),
}

impl From<anyhow::Error> for ToolsError {
    fn from(e: anyhow::Error) -> Self {
        Self::Registration(e.to_string())
    }
}

/// Register all built-in tools.
///
/// Returns a strongly-typed [`ToolsError`] rather than leaking `anyhow`
/// into the public API surface.
pub async fn register_builtin_tools(
    registry: &ToolRegistry,
) -> std::result::Result<(), ToolsError> {
    builtin::register_all_builtin_tools(registry).await?;
    builtin::register_response_tools(registry).await?;
    if let Err(err) = mcptools::register_mcp_tools(registry).await {
        warn!("Failed to register MCP tools: {}", err);
    }
    Ok(())
}
