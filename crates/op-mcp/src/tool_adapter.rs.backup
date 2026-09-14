
  1 | //! Tool Adapter - Bridges op-tools and external MCPs to MCP protocol
  2 | //!
  3 | //! Aggregates tools from:
  4 | //! - External MCP servers (GitHub, filesystem, etc.)
  5 | //! - Local op-tools (filtered for safety)
  6 | //!
  7 | //! SECURITY: System commands (shell_execute, systemd_*, ovs_*, etc.) are
  8 | //! NOT exposed via MCP. Use the web interface for system operations.
  9 |
 10 | use anyhow::Result;
 11 | use serde_json::{json, Value};
 12 | use std::sync::Arc;
 13 |
 14 | use crate::external_client::{ExternalMcpManager, ExternalTool};
 15 | use op_core::{ToolDefinition, ToolRequest};
 16 | use op_tools::ToolRegistry;
use op_execution_tracker::{ExecutionContext, ExecutionResult, ExecutionStatus, ExecutionTracker};
use op_dynamic_loader::{ExecutionAwareLoader, SmartLoadingStrategy};
 17 |
 18 | /// Patterns that block tools from being exposed via MCP.
 19 | /// Uses substring matching - if the tool name contains any of these patterns, it's blocked.
 20 | const BLOCKED_PATTERNS: &[&str] = &[
 21 |     // Shell/Execution
 22 |     "shell_execute",
 23 |     "write_file",
 24 |     // Systemd mutations
 25 |     "systemd_start",
 26 |     "systemd_stop",
 27 |     "systemd_restart",
 28 |     "systemd_reload",
 29 |     "systemd_enable",
 30 |     "systemd_disable",
 31 |     "systemd_apply",
 32 |     // OVS mutations
 33 |     "ovs_create",
 34 |     "ovs_delete",
 35 |     "ovs_add",
 36 |     "ovs_set",
 37 |     // Plugin mutations (matches any *_apply pattern)
 38 |     "_apply",
 39 |     // BTRFS mutations
 40 |     "btrfs_create",
 41 |     "btrfs_delete",
 42 |     "btrfs_snapshot",
 43 | ];
 44 |
 45 | /// Check if a tool name should be blocked from MCP exposure
 46 | fn is_tool_blocked(name: &str) -> bool {
 47 |     BLOCKED_PATTERNS
 48 |         .iter()
 49 |         .any(|pattern| name.contains(pattern))
 50 | }
 51 |
 52 | fn is_orchestration_tool(name: &str) -> bool {
 53 |     name.starts_with("skill_") || name.starts_with("workstack_") || name.starts_with("workflow_")
 54 | }
 55 |
 56 | /// Check if a tool should be included based on MCP_TOOL_FILTER environment variable
 57 | /// Returns true if tool should be included, false if filtered out
 58 | fn matches_tool_filter(name: &str) -> bool {
 59 |     let filter = std::env::var("MCP_TOOL_FILTER").ok().as_deref();
 60 |
 61 |     match filter {
 62 |         Some("systemd") => name.starts_with("dbus_systemd1_"),
 63 |         Some("login") => name.starts_with("dbus_login1_"),
 64 |         Some("ovs") => name.starts_with("ovs_"),
 65 |         Some("agents") => name.starts_with("agent_") || name.starts_with("list_") || name.starts_with("spawn_") || name.contains("agent"),
 66 |         Some("core") => name.starts_with("dbus_DBus_") || name.starts_with("dbus_login1_") || name.starts_with("ovs_") || name.starts_with("plugin_"),
 67 |         Some("skills") => is_orchestration_tool(name),
        Some(unknown) => {
            tracing::warn!("Unknown MCP_TOOL_FILTER value: '{}'. Including all tools.", unknown);
            true // Default to include all for unknown filters
        }
    }
}

/// Tool Adapter - Unified interface for all tools
pub struct ToolAdapter {
    tool_registry: Arc<ToolRegistry>,
    external_mcp: Arc<ExternalMcpManager>,
    execution_tracker: Option<Arc<ExecutionTracker>>,
    dynamic_loader: Option<Arc<ExecutionAwareLoader>>,
}

impl ToolAdapter {
    /// Create new tool adapter
    pub async fn new() -> Result<Self> {
        let tool_registry = Arc::new(ToolRegistry::new());
        let external_mcp = Arc::new(ExternalMcpManager::new());

        tracing::info!("Tool adapter initialized");

        Ok(Self {
            tool_registry,
            external_mcp,
            execution_tracker: None,
            dynamic_loader: None,
        })
    }

    /// Create with a shared tool registry
    pub async fn with_registry(registry: Arc<ToolRegistry>) -> Result<Self> {
        let external_mcp = Arc::new(ExternalMcpManager::new());

        tracing::info!("Tool adapter initialized with shared registry");

        Ok(Self {
            tool_registry: registry,
            external_mcp,
            execution_tracker: None,
            dynamic_loader: None,
        })
    }

    /// Create with execution tracking enabled
    pub async fn with_execution_tracking(
        registry: Arc<ToolRegistry>,
        execution_tracker: Arc<ExecutionTracker>,
    ) -> Result<Self> {
        let external_mcp = Arc::new(ExternalMcpManager::new());

        tracing::info!("Tool adapter initialized with execution tracking");

        Ok(Self {
            tool_registry: registry,
            external_mcp,
    /// Create with dynamic loading enabled
    pub async fn with_dynamic_loading(
        registry: Arc<ToolRegistry>,
        execution_tracker: Arc<ExecutionTracker>,
        dynamic_loader: Arc<ExecutionAwareLoader>,
    ) -> Result<Self> {
        let external_mcp = Arc::new(ExternalMcpManager::new());

        tracing::info!("Tool adapter initialized with dynamic loading and execution tracking");

        Ok(Self {
            tool_registry: registry,
            external_mcp,
            execution_tracker: Some(execution_tracker),
            dynamic_loader: Some(dynamic_loader),
        })
    }
            execution_tracker: Some(execution_tracker),
            dynamic_loader: None,
        })
    }

    /// Create with external MCP configuration
    pub async fn with_external_mcps(mcp_config_path: Option<&str>) -> Result<Self> {
        let adapter = Self::new().await?;

        if let Some(path) = mcp_config_path {
            tracing::info!("Loading external MCP servers from: {}", path);
            adapter.external_mcp.load_from_file(path).await?;
        }

        Ok(adapter)
    }

    /// Create with both shared registry and external MCPs
    pub async fn with_registry_and_external_mcps(
        registry: Arc<ToolRegistry>,
        mcp_config_path: Option<&str>,
    ) -> Result<Self> {
        let external_mcp = Arc::new(ExternalMcpManager::new());

        if let Some(path) = mcp_config_path {
            tracing::info!("Loading external MCP servers from: {}", path);
            external_mcp.load_from_file(path).await?;
        }

        tracing::info!("Tool adapter initialized with shared registry and external MCPs");

        Ok(Self {
            tool_registry: registry,
            external_mcp,
            execution_tracker: None,
            dynamic_loader: None,
        })
    }

    /// Add external MCP server at runtime
    pub async fn add_external_mcp(
        &self,
        config: crate::external_client::ExternalMcpConfig,
    ) -> Result<()> {
        self.external_mcp.add_server(config).await
    }

    /// List all available tools in MCP format (filtered local + external)
    pub async fn list_tools(
        &self,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<Vec<Value>> {
        let mut all_tools = Vec::new();
        let mut blocked_count = 0;
        let mut filtered_count = 0;

        // Get local tools from op-tools registry (with dynamic loading if available)
        let local_tools = if let Some(loader) = &self.dynamic_loader {
            tracing::debug!("Using dynamic loader for tool listing");
            loader.list_tools_with_dynamic_loading().await
        } else {
            tracing::debug!("Using direct registry for tool listing");
            self.tool_registry.list().await
        };
        let local_total = local_tools.len();

        // Log filter status
        if let Ok(filter) = std::env::var("MCP_TOOL_FILTER") {
            tracing::info!("MCP_TOOL_FILTER active: {}", filter);
        }

        // Collect all allowed tools first
        let mut allowed_tools = Vec::new();
        for tool in local_tools {
            if is_tool_blocked(&tool.name) {
                blocked_count += 1;
                tracing::trace!("Blocking tool from MCP: {}", tool.name);
            } else if !matches_tool_filter(&tool.name) {
                filtered_count += 1;
                tracing::trace!("Filtering tool from MCP: {}", tool.name);
            } else {
                allowed_tools.push(self.tool_definition_to_mcp(&tool));
            }
        }

        let local_allowed = local_total - blocked_count - filtered_count;

        // Get tools from external MCP servers
        let external_tools = self.external_mcp.get_all_tools().await;
        let mut external_allowed = 0;

        for tool in external_tools {
            if matches_tool_filter(&tool.name) {
                allowed_tools.push(self.external_tool_to_mcp(tool));
                external_allowed += 1;
            } else {
                filtered_count += 1;
                tracing::trace!("Filtering external tool from MCP: {}", tool.name);
            }
        }

        // Sort tools by name for consistent chunking across instances
        allowed_tools.sort_by(|a, b| {
            let name_a = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let name_b = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
            name_a.cmp(name_b)
        });

        // Apply chunking if offset and/or limit are set
        let offset = offset.unwrap_or(0);

        tracing::debug!(
            "Chunking check: offset={:?}, limit={:?}, total_tools={}",
            offset,
            limit,
            allowed_tools.len()
        );

        if offset > 0 || limit.is_some() {
            let end = limit.map(|l| offset + l).unwrap_or(allowed_tools.len());
            let before_count = allowed_tools.len();
            all_tools = allowed_tools
                .into_iter()
                .skip(offset)
                .take(end - offset)
                .collect();
            tracing::info!(
                "MCP tool chunking: offset={}, limit={:?}, showing tools {}-{} of {} (reduced from {} to {})",
                offset,
                limit,
                offset,
                all_tools.len() + offset,
                local_allowed + external_allowed,
                before_count,
                all_tools.len()
            );
        } else {
            all_tools = allowed_tools;
        }

        tracing::info!(
            "MCP tools: {} total ({} local allowed, {} blocked, {} filtered, {} external)",
            all_tools.len(),
            local_allowed,
            blocked_count,
            filtered_count,
            external_allowed
        );

        Ok(all_tools)
    }

    /// Execute tool and return MCP-formatted result
    pub async fn execute_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        // Check if this is an external MCP tool (contains ':')
        if name.contains(':') {
            tracing::debug!("Executing external MCP tool: {}", name);
            return self.external_mcp.call_tool(name, arguments).await;
        }

        // Check blocklist before executing local tools
        if is_tool_blocked(name) {
            tracing::warn!(
                "Blocked attempt to execute restricted tool via MCP: {}",
                name
            );
            return Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("Tool '{}' is not available via MCP for security reasons. Use the web interface for system operations.", name)
                }],
                "isError": true
            }));
        }

        // Check filter before executing local tools
        if !matches_tool_filter(name) {
            tracing::warn!("Filtered attempt to execute tool via MCP: {}", name);
            return Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("Tool '{}' is not available in this MCP instance (filtered). Try a different MCP endpoint.", name)
                }],
                "isError": true
            }));
        }

        // Create execution context if tracking is enabled
        let execution_id = if let Some(tracker) = &self.execution_tracker {
            let context = ExecutionContext::new(name);
            let exec_id = tracker.track_execution(context).await?;
            tracker.update_status(&exec_id, ExecutionStatus::Dispatched).await?;
            Some(exec_id)
        } else {
            None
        };

        // Execute via dynamic loader if available, otherwise use direct registry
        tracing::debug!("Executing local tool: {}", name);

        let start_time = Utc::now();
        let request = ToolRequest::new(name, arguments);

        let result = if let Some(loader) = &self.dynamic_loader {
            tracing::debug!("Using dynamic loader for tool execution: {}", name);
            loader.execute_with_dynamic_loading(request, self.tool_registry.clone()).await
        } else {
            tracing::debug!("Using direct registry execution for tool: {}", name);
            self.tool_registry.execute(request).await
        };

        let end_time = Utc::now();
        let duration_ms = (end_time - start_time).num_milliseconds() as u64;

        // Update execution tracking if enabled
        if let Some(exec_id) = execution_id {
            if let Some(tracker) = &self.execution_tracker {
                let execution_result = ExecutionResult {
                    success: result.success,
                    result: Some(serde_json::json!({
                        "content": result.content.to_string(),
                        "duration_ms": duration_ms,
                    })),
                    error: result.error,
                    duration_ms,
                    finished_at: end_time,
                };

                tracker.complete_execution(&exec_id, execution_result).await?;
            }
        }

        // Convert ToolResult to MCP format
        if result.success {
            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": result.content.to_string()
                }],
                "execution_id": execution_id,
                "duration_ms": duration_ms,
            }))
        } else {
            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": result.error.unwrap_or_else(|| "Unknown error".to_string())
                }],
                "isError": true,
                "execution_id": execution_id,
                "duration_ms": duration_ms,
            }))
        }
    }

    /// Convert ToolDefinition to MCP format
    fn tool_definition_to_mcp(&self, tool: &ToolDefinition) -> Value {
        json!({
            "name": tool.name,
            "description": tool.description,
            "inputSchema": tool.input_schema
        })
    }

    /// Convert external tool to MCP format
    fn external_tool_to_mcp(&self, tool: ExternalTool) -> Value {
        json!({
            "name": tool.name,
            "description": tool.description,
            "inputSchema": tool.input_schema
        })
    }

    /// Get external MCP manager (for advanced operations)
    pub fn external_mcp_manager(&self) -> Arc<ExternalMcpManager> {
        self.external_mcp.clone()
    }

    /// Get tool registry (for advanced operations)
    /// Get dynamic loader (if enabled)
    pub fn dynamic_loader(&self) -> Option<Arc<ExecutionAwareLoader>> {
        self.dynamic_loader.clone()
    }
    pub fn tool_registry(&self) -> Arc<ToolRegistry> {
        self.tool_registry.clone()
    }

    /// Get execution tracker (if enabled)
    pub fn execution_tracker(&self) -> Option<Arc<ExecutionTracker>> {
        self.execution_tracker.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blocked_patterns() {
        // Shell/execution
        assert!(is_tool_blocked("shell_execute"));
        assert!(is_tool_blocked("write_file"));

        // Systemd mutations
        assert!(is_tool_blocked("systemd_start"));
        assert!(is_tool_blocked("systemd_stop"));
        assert!(is_tool_blocked("systemd_restart"));
        assert!(is_tool_blocked("systemd_reload"));
        assert!(is_tool_blocked("systemd_enable"));
        assert!(is_tool_blocked("systemd_disable"));
        assert!(is_tool_blocked("systemd_apply"));

        // OVS mutations
        assert!(is_tool_blocked("ovs_create_bridge"));
        assert!(is_tool_blocked("ovs_delete_port"));
        assert!(is_tool_blocked("ovs_add_port"));
        assert!(is_tool_blocked("ovs_set_controller"));

        // Plugin apply patterns
        assert!(is_tool_blocked("network_apply"));
        assert!(is_tool_blocked("plugin_apply"));

        // BTRFS mutations
        assert!(is_tool_blocked("btrfs_create_subvolume"));
        assert!(is_tool_blocked("btrfs_delete_snapshot"));
        assert!(is_tool_blocked("btrfs_snapshot"));
    }

    #[test]
    fn test_allowed_tools() {
        // Read operations should be allowed
        assert!(!is_tool_blocked("systemd_status"));
        assert!(!is_tool_blocked("systemd_list"));
        assert!(!is_tool_blocked("ovs_list_bridges"));
        assert!(!is_tool_blocked("ovs_list_ports"));
        assert!(!is_tool_blocked("read_file"));
        assert!(!is_tool_blocked("btrfs_list"));
        assert!(!is_tool_blocked("btrfs_info"));

        // Agent tools should be allowed
        assert!(!is_tool_blocked("agent_.list"));
        assert!(!is_tool_blocked("agent_status"));

        // Response tools should be allowed
        assert!(!is_tool_blocked("respond_to_user"));
        assert!(!is_tool_blocked("cannot_perform"));
    }
}
