//! Built-in tools for op-dbus
//!
//! All tools registered eagerly at startup.
//! Agents started as D-Bus services when registered.

pub mod agent_tool;
pub mod response_tools;

// Include other modules if they exist in your codebase
// pub mod dbus;
pub mod anydesk;
pub mod code_search;
pub mod dbus_introspection;
pub mod file;
pub mod gcloud_tools;
pub mod ovs_tools;
pub mod rtnetlink_tools;
pub mod shell;
pub mod sv;
// pub mod self_tools;
// pub mod self_tools;
// pub mod shell;

use crate::registry::ToolDefinition;
use crate::ToolRegistry;
use anyhow::Result;

/// Register all built-in tools
pub async fn register_all_builtin_tools(registry: &ToolRegistry) -> Result<()> {
    tracing::info!("Registering built-in tools...");

    // Register agent tools (starts D-Bus services)
    tracing::info!("Starting agent D-Bus services...");
    agent_tool::register_all_agents(registry).await?;

    // Register AnyDesk tools
    tracing::info!("Registering AnyDesk tools...");
    anydesk::register_anydesk_tools(registry).await?;

    // Register OVS tools
    tracing::info!("Registering OVS tools...");
    ovs_tools::register_ovs_tools(registry).await?;

    // Register rtnetlink tools
    tracing::info!("Registering rtnetlink tools...");
    rtnetlink_tools::register_rtnetlink_tools(registry).await?;

    // Register file tools
    tracing::info!("Registering file tools...");
    file::register_file_tools(registry).await?;

    // Register shell tools
    tracing::info!("Registering shell tools...");
    shell::register_shell_tools(registry).await?;

    // Register runit service tools
    tracing::info!("Registering runit (sv) tools...");
    sv::register_sv_tools(registry).await?;

    // Register D-Bus introspection tools
    tracing::info!("Registering D-Bus introspection tools...");
    dbus_introspection::register_dbus_introspection_tools(registry).await?;

    // Register gcloud introspection tools
    tracing::info!("Registering gcloud introspection tools...");
    gcloud_tools::register_gcloud_tools(registry).await?;

    // Code search context injection is handled automatically via MCP server
    // No separate tools needed - context is injected into all tool calls

    let count = registry.list().await.len();
    tracing::info!("Registered {} tools", count);

    Ok(())
}

/// Register response tools (respond_to_user, cannot_perform, request_clarification)
pub async fn register_response_tools(registry: &ToolRegistry) -> Result<()> {
    tracing::info!("Registering response tools...");

    // Initialize response accumulator
    response_tools::init_response_accumulator();

    // Create and register response tools
    let tools = response_tools::create_response_tools();
    let tool_count = tools.len();
    for tool in tools {
        let name = tool.name().to_string();
        let definition = ToolDefinition {
            name: name.clone(),
            description: tool.description().to_string(),
            input_schema: tool.input_schema(),
            schema_version: "https://json-schema.org/draft/next/schema".to_string(),
            category: tool.category().to_string(),
            namespace: tool.namespace().to_string(),
            tags: tool.tags(),
        };
        registry.register(name.into(), tool, definition).await?;
    }

    tracing::info!("Registered {} response tools", tool_count);
    Ok(())
}

// Re-exports
pub use agent_tool::{
    create_agent_tool, create_agent_tool_with_executor, AgentConnectionRegistry, AgentDef,
    AgentExecutor, AgentTool, BusType, DbusAgentExecutor, AGENT_DEFINITIONS,
};
