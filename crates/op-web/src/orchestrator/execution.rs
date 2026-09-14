use super::types::{OrchestratorResponse, ToolResult};
use super::UnifiedOrchestrator;
use anyhow::Result;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use tracing::error;

impl UnifiedOrchestrator {
    /// Execute a single tool
    pub(crate) async fn execute_tool(&self, name: &str, args: Value) -> ToolResult {
        // Handle compact mode meta-tools
        match name {
            "list_tools" => return self.handle_list_tools(args).await,
            "search_tools" => return self.handle_search_tools(args).await,
            "get_tool_schema" => return self.handle_get_tool_schema(args).await,
            "execute_tool" => {
                // Extract the actual tool name and arguments
                let tool_name = args.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
                let tool_args = args.get("arguments").cloned().unwrap_or(json!({}));
                // Recursively execute the actual tool (boxed to avoid infinite future)
                return Box::pin(self.execute_tool(tool_name, tool_args)).await;
            }
            _ => {}
        }

        // Execute actual tool from registry
        match self.tool_registry.get(name).await {
            Some(tool) => match tool.execute(args).await {
                Ok(result) => ToolResult {
                    name: name.to_string(),
                    success: true,
                    result: Some(result),
                    error: None,
                },
                Err(e) => {
                    error!("Tool {} failed: {}", name, e);
                    ToolResult {
                        name: name.to_string(),
                        success: false,
                        result: None,
                        error: Some(e.to_string()),
                    }
                }
            },
            None => {
                error!("Tool not found: {}", name);
                ToolResult {
                    name: name.to_string(),
                    success: false,
                    result: None,
                    error: Some(format!("Tool not found: {}. Use list_tools or search_tools to find available tools.", name)),
                }
            }
        }
    }

    /// Execute direct tool command: "tool_name {json_args}"
    pub(crate) async fn execute_direct_tool(&self, input: &str) -> Result<OrchestratorResponse> {
        let parts: Vec<&str> = input.splitn(2, ' ').collect();
        let tool_name = parts[0].trim();
        let args: Value = if parts.len() > 1 {
            serde_json::from_str(parts[1].trim()).unwrap_or(json!({}))
        } else {
            json!({})
        };

        let result = self.execute_tool(tool_name, args).await;

        let message = if result.success {
            format!(
                "✅ **{}**\n```json\n{}\n```",
                tool_name,
                simd_json::to_string_pretty(&result.result).unwrap_or_default()
            )
        } else {
            format!(
                "❌ **{}** failed: {}",
                tool_name,
                result.error.as_ref().unwrap_or(&"Unknown".to_string())
            )
        };

        Ok(OrchestratorResponse {
            success: result.success,
            message,
            tools_executed: vec![tool_name.to_string()],
            tool_results: vec![result],
            turns: 0,
        })
    }

    /// Handle list_tools meta-tool
    async fn handle_list_tools(&self, args: Value) -> ToolResult {
        let category = args
            .get("category")
            .and_then(|v| v.as_str())
            .unwrap_or("all");
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

        let all_tools = self.tool_registry.list().await;

        let filtered: Vec<_> = if category == "all" {
            all_tools
        } else {
            all_tools
                .into_iter()
                .filter(|t| match category {
                    "ovs" => t.name.starts_with("ovs_"),
                    "systemd" => {
                        t.name.starts_with("dbus_systemd_")
                            || t.name.starts_with("dbus_dinit_")
                            || t.name.starts_with("sv_")
                    }
                    "dbus" => t.name.starts_with("dbus_"),
                    "file" => t.name.starts_with("file_"),
                    "shell" => t.name.starts_with("shell_"),
                    "network" => t.name.starts_with("rtnetlink_"),
                    "openflow" => t.name.starts_with("openflow_"),
                    "agent" => t.name.starts_with("agent_"),
                    _ => false,
                })
                .collect()
        };

        let tools_json: Vec<Value> = filtered
            .iter()
            .take(limit)
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                })
            })
            .collect();

        ToolResult {
            name: "list_tools".to_string(),
            success: true,
            result: Some(json!({
                "tools": tools_json,
                "total": filtered.len(),
                "showing": tools_json.len(),
                "category": category,
            })),
            error: None,
        }
    }

    /// Handle search_tools meta-tool
    async fn handle_search_tools(&self, args: Value) -> ToolResult {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();

        if query.is_empty() {
            return ToolResult {
                name: "search_tools".to_string(),
                success: false,
                result: None,
                error: Some("Query parameter is required".to_string()),
            };
        }

        let all_tools = self.tool_registry.list().await;
        let matches: Vec<Value> = all_tools
            .iter()
            .filter(|t| {
                t.name.to_lowercase().contains(&query)
                    || t.description.to_lowercase().contains(&query)
            })
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                })
            })
            .collect();

        ToolResult {
            name: "search_tools".to_string(),
            success: true,
            result: Some(json!({
                "query": query,
                "matches": matches,
                "count": matches.len(),
            })),
            error: None,
        }
    }

    /// Handle get_tool_schema meta-tool
    async fn handle_get_tool_schema(&self, args: Value) -> ToolResult {
        let tool_name = args.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");

        if tool_name.is_empty() {
            return ToolResult {
                name: "get_tool_schema".to_string(),
                success: false,
                result: None,
                error: Some("tool_name parameter is required".to_string()),
            };
        }

        match self.tool_registry.get_definition(tool_name).await {
            Some(def) => ToolResult {
                name: "get_tool_schema".to_string(),
                success: true,
                result: Some(json!({
                    "name": def.name,
                    "description": def.description,
                    "input_schema": def.input_schema,
                })),
                error: None,
            },
            None => ToolResult {
                name: "get_tool_schema".to_string(),
                success: false,
                result: None,
                error: Some(format!(
                    "Tool not found: {}. Use list_tools or search_tools to find available tools.",
                    tool_name
                )),
            },
        }
    }

    // === Command handlers ===

    pub(crate) fn help_response(&self) -> OrchestratorResponse {
        OrchestratorResponse::success(
            r#"📚 **op-dbus Help**

**Commands:**
- `help` - Show this help
- `tools` - List all available tools
- `status` - System status
- `run <tool> {args}` - Execute tool directly

**Natural Language:**
Just describe what you want:
- "Create an OVS bridge called ovsbr0"
- "Restart gbr-xray"
- "List all network interfaces"
- "Show systemd unit status for sshd"

The AI uses native protocols (D-Bus, OVSDB, Netlink) - never CLI commands."#,
        )
    }

    pub(crate) async fn list_tools_response(&self) -> OrchestratorResponse {
        let tools = self.tool_registry.list().await;
        let mut output = format!("🔧 **{} Tools Available**\n\n", tools.len());

        // Group by prefix
        let prefixes = [
            "ovs_",
            "dbus_systemd_",
            "dbus_",
            "file_",
            "shell_",
            "rtnetlink_",
            "openflow_",
            "agent_",
        ];
        let names = [
            "OVS", "Systemd", "D-Bus", "File", "Shell", "Network", "OpenFlow", "Agents",
        ];

        for (prefix, name) in prefixes.iter().zip(names.iter()) {
            let group: Vec<_> = tools
                .iter()
                .filter(|t| t.name.starts_with(prefix))
                .collect();
            if !group.is_empty() {
                output.push_str(&format!("**{}** ({})\n", name, group.len()));
                for t in group.iter().take(5) {
                    output.push_str(&format!("  • `{}`\n", t.name));
                }
                if group.len() > 5 {
                    output.push_str(&format!("  ... +{} more\n", group.len() - 5));
                }
                output.push('\n');
            }
        }

        // Other
        let other: Vec<_> = tools
            .iter()
            .filter(|t| !prefixes.iter().any(|p| t.name.starts_with(p)))
            .collect();
        if !other.is_empty() {
            output.push_str(&format!("**Other** ({})\n", other.len()));
            for t in other.iter().take(5) {
                output.push_str(&format!("  • `{}`\n", t.name));
            }
            if other.len() > 5 {
                output.push_str(&format!("  ... +{} more\n", other.len() - 5));
            }
        }

        OrchestratorResponse::success(output)
    }

    pub(crate) async fn status_response(&self) -> OrchestratorResponse {
        let tools = self.tool_registry.list().await;
        let model = self.chat_manager.current_model().await;
        let provider = format!("{:?}", self.chat_manager.current_provider().await);

        OrchestratorResponse::success(format!(
            r#"📊 **System Status**

🔧 Tools: {} registered
🤖 LLM: {} ({})\n✅ Ready for commands"#,
            tools.len(),
            model,
            provider
        ))
    }
}
