//! Tool Adapter with Orchestration Integration
//!
//! This module extends the tool adapter to use the orchestrated executor,
//! enabling workstacks, skills, and multi-agent coordination.

use anyhow::Result;
use op_chat::{
    ExecutionMode, OrchestratedExecutor, OrchestratedResult, Workflow, WorkflowStep,
};
use op_core::ExecutionTracker;
use op_tools::ToolRegistry;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Patterns that block tools from being exposed via MCP
const BLOCKED_PATTERNS: &[&str] = &[
    "shell_execute",
    "write_file",
    "systemd_start",
    "systemd_stop",
    "systemd_restart",
    "systemd_reload",
    "systemd_enable",
    "systemd_disable",
    "systemd_apply",
    "ovs_create",
    "ovs_delete",
    "ovs_add",
    "ovs_set",
    "_apply",
    "btrfs_create",
    "btrfs_delete",
    "btrfs_snapshot",
];

/// Check if a tool name should be blocked
fn is_tool_blocked(name: &str) -> bool {
    BLOCKED_PATTERNS
        .iter()
        .any(|pattern| name.contains(pattern))
}

/// Check if tool matches filter
fn matches_tool_filter(name: &str) -> bool {
    match std::env::var("MCP_TOOL_FILTER").ok().as_deref() {
        Some("systemd") => name.starts_with("dbus_systemd1_"),
        Some("login") => name.starts_with("dbus_login1_"),
        Some("ovs") => name.starts_with("ovs_"),
        Some("agents") => {
            name.starts_with("agent_")
                || name.starts_with("list_")
                || name.starts_with("spawn_")
                || name.contains("agent")
        }
        Some("core") => {
            name.starts_with("dbus_DBus_")
                || name.starts_with("dbus_login1_")
                || name.starts_with("ovs_")
                || name.starts_with("plugin_")
        }
        Some("skills") => is_orchestration_tool(name),
        Some("orchestration") => is_orchestration_tool(name),
        Some(_) | None => true,
    }
}

/// Check if this is an orchestration tool
fn is_orchestration_tool(name: &str) -> bool {
    name.starts_with("skill_")
        || name.starts_with("workstack_")
        || name.starts_with("workflow_")
}

/// Orchestrated Tool Adapter - Unified execution with orchestration
pub struct OrchestratedToolAdapter {
    tool_registry: Arc<ToolRegistry>,
    orchestrated_executor: Arc<OrchestratedExecutor>,
    execution_tracker: Arc<ExecutionTracker>,
}

impl OrchestratedToolAdapter {
    /// Create new orchestrated tool adapter
    pub async fn new(tool_registry: Arc<ToolRegistry>) -> Result<Self> {
        let execution_tracker = Arc::new(ExecutionTracker::new(1000));
        let orchestrated_executor = Arc::new(
            OrchestratedExecutor::new(tool_registry.clone(), execution_tracker.clone()).await?,
        );

        info!("Orchestrated tool adapter initialized");

        Ok(Self {
            tool_registry,
            orchestrated_executor,
            execution_tracker,
        })
    }

    /// List all available tools including orchestration tools
    pub async fn list_tools(
        &self,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<Vec<Value>> {
        let mut all_tools = Vec::new();

        // Get regular tools from registry
        let local_tools = self.tool_registry.list().await;

        for tool in local_tools {
            if is_tool_blocked(&tool.name) {
                continue;
            }
            if !matches_tool_filter(&tool.name) {
                continue;
            }
            all_tools.push(json!({
                "name": tool.name,
                "description": tool.description,
                "inputSchema": tool.input_schema
            }));
        }

        // Add orchestration tools
        all_tools.extend(self.get_orchestration_tools().await);

        // Sort for consistent ordering
        all_tools.sort_by(|a, b| {
            let name_a = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let name_b = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
            name_a.cmp(name_b)
        });

        // Apply pagination
        let offset = offset.unwrap_or(0);
        if offset > 0 || limit.is_some() {
            let end = limit.map(|l| offset + l).unwrap_or(all_tools.len());
            all_tools = all_tools.into_iter().skip(offset).take(end - offset).collect();
        }

        Ok(all_tools)
    }

    /// Get orchestration tools (workstacks, skills, workflows)
    async fn get_orchestration_tools(&self) -> Vec<Value> {
        let mut tools = Vec::new();

        // Add workstack tools
        let workstack_registry = self.orchestrated_executor.workstack_registry().read().await;
        for workstack in workstack_registry.list() {
            tools.push(json!({
                "name": format!("workstack_{}", workstack.id),
                "description": format!("[Workstack] {}: {}", workstack.name, workstack.description),
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "arguments": {
                            "type": "string",
                            "description": "Arguments/context for the workstack"
                        },
                        "skip_phases": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Phases to skip"
                        }
                    },
                    "required": ["arguments"]
                }
            }));
        }

        // Add skill tools
        let skill_registry = self.orchestrated_executor.skill_registry().read().await;
        for skill in skill_registry.list() {
            tools.push(json!({
                "name": format!("skill_{}", skill.name),
                "description": format!("[Skill] {}: {}", skill.name, skill.metadata.description),
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "tool": {
                            "type": "string",
                            "description": "Tool to execute with this skill activated"
                        },
                        "arguments": {
                            "type": "object",
                            "description": "Arguments for the tool"
                        }
                    },
                    "required": ["tool", "arguments"]
                }
            }));
        }

        tools
    }

    /// Execute tool with orchestration support
    pub async fn execute_tool(
        &self,
        name: &str,
        arguments: Value,
        session_id: Option<String>,
    ) -> Result<Value> {
        // Check blocklist
        if is_tool_blocked(name) {
            warn!("Blocked attempt to execute restricted tool: {}", name);
            return Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("Tool '{}' is not available via MCP for security reasons.", name)
                }],
                "isError": true
            }));
        }

        // Check filter
        if !matches_tool_filter(name) {
            return Ok(json!({
                "content": [{
                    "type": "text",
                    "text": format!("Tool '{}' is not available in this MCP instance (filtered).", name)
                }],
                "isError": true
            }));
        }

        info!(tool = %name, "Executing tool via orchestrated executor");

        // Execute via orchestrated executor (handles workstacks, skills, workflows, direct)
        let result = self
            .orchestrated_executor
            .execute(name, arguments, session_id)
            .await?;

        // Convert to MCP format
        self.orchestrated_result_to_mcp(result)
    }

    /// Convert orchestrated result to MCP format
    fn orchestrated_result_to_mcp(&self, result: OrchestratedResult) -> Result<Value> {
        let mode_str = match &result.mode {
            ExecutionMode::Direct { tool_name } => format!("direct:{}", tool_name),
            ExecutionMode::Workstack { workstack_id } => format!("workstack:{}", workstack_id),
            ExecutionMode::Skill { skill_name } => format!("skill:{}", skill_name),
            ExecutionMode::MultiAgent { agents } => format!("multi_agent:{}", agents.join(",")),
            ExecutionMode::Workflow { workflow_id } => format!("workflow:{}", workflow_id),
        };

        if result.success {
            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": result.content.to_string()
                }],
                "execution_id": result.execution_id,
                "duration_ms": result.duration_ms,
                "mode": mode_str,
                "skills_activated": result.skills_activated,
                "agents_involved": result.agents_involved,
                "trace": result.trace,
            }))
        } else {
            Ok(json!({
                "content": [{
                    "type": "text",
                    "text": result.content.to_string()
                }],
                "isError": true,
                "execution_id": result.execution_id,
                "duration_ms": result.duration_ms,
                "mode": mode_str,
            }))
        }
    }

    /// Register a workflow
    pub async fn register_workflow(&self, workflow: Workflow) {
        self.orchestrated_executor.register_workflow(workflow).await;
    }

    /// Get execution tracker
    pub fn execution_tracker(&self) -> &Arc<ExecutionTracker> {
        &self.execution_tracker
    }

    /// Get tool registry
    pub fn tool_registry(&self) -> &Arc<ToolRegistry> {
        &self.tool_registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orchestration_tool_detection() {
        assert!(is_orchestration_tool("skill_python_debugging"));
        assert!(is_orchestration_tool("workstack_full_stack_feature"));
        assert!(is_orchestration_tool("workflow_deploy_production"));
        assert!(!is_orchestration_tool("ovs_list_bridges"));
        assert!(!is_orchestration_tool("agent_python_pro"));
    }

    #[test]
    fn test_blocked_patterns() {
        assert!(is_tool_blocked("shell_execute"));
        assert!(is_tool_blocked("systemd_start"));
        assert!(!is_tool_blocked("systemd_status"));
        assert!(!is_tool_blocked("ovs_list_bridges"));
    }
}
