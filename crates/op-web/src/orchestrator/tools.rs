use super::UnifiedOrchestrator;
use op_llm::provider::ToolDefinition;
use simd_json::json;

impl UnifiedOrchestrator {
    /// Build compact mode tool definitions (4 meta-tools)
    ///
    /// This restricts the actual tool definitions sent to the LLM API to just these 4,
    /// forcing it to use the "Compact Mode" workflow (execute_tool, etc.) instead of
    /// trying to call one of the 138+ tools directly (which would consume massive context tokens).
    pub(crate) fn build_compact_mode_tools(&self) -> Vec<ToolDefinition> {
        vec![
            ToolDefinition {
                name: "execute_tool".to_string(),
                description: "Execute any tool by name with arguments".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "tool_name": {
                            "type": "string",
                            "description": "Name of the tool to execute"
                        },
                        "arguments": {
                            "type": "object",
                            "description": "Arguments to pass to the tool"
                        }
                    },
                    "required": ["tool_name"]
                }),
                schema_version: String::new(),
                category: String::new(),
                tags: Vec::new(),
                namespace: String::new(),
            },
            ToolDefinition {
                name: "list_tools".to_string(),
                description: "List available tools, optionally filtered by category".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "category": {
                            "type": "string",
                            "description": "Filter by category (e.g., 'ovs', 'systemd', 'network')"
                        }
                    }
                }),
                schema_version: String::new(),
                category: String::new(),
                tags: Vec::new(),
                namespace: String::new(),
            },
            ToolDefinition {
                name: "search_tools".to_string(),
                description: "Search for tools by name or description".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query"
                        }
                    },
                    "required": ["query"]
                }),
                schema_version: String::new(),
                category: String::new(),
                tags: Vec::new(),
                namespace: String::new(),
            },
            ToolDefinition {
                name: "get_tool_schema".to_string(),
                description: "Get the full schema/parameters for a specific tool".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "tool_name": {
                            "type": "string",
                            "description": "Name of the tool"
                        }
                    },
                    "required": ["tool_name"]
                }),
                schema_version: String::new(),
                category: String::new(),
                tags: Vec::new(),
                namespace: String::new(),
            },
            ToolDefinition {
                name: "respond".to_string(),
                description: "Send a final response to the user. Use this when you have completed the task or need to communicate results.".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string",
                            "description": "The response message to send to the user"
                        }
                    },
                    "required": ["message"]
                }),
                schema_version: String::new(),
                category: String::new(),
                tags: Vec::new(),
                namespace: String::new(),
            },
        ]
    }

    /// Build system prompt for compact mode
    ///
    /// This explains the meta-tool architecture to the LLM.
    pub(crate) fn build_compact_mode_system_prompt(&self) -> String {
        r#"You are an AI system administrator with access to 138+ system management tools via a compact interface.

CRITICAL RULES:
1. ALWAYS use tools for system operations - NEVER output text directly, NEVER suggest CLI commands
2. Use the 5 meta-tools to discover, execute, and respond:
   - list_tools() - Browse available tools by category
   - search_tools(query) - Find tools by keyword
   - get_tool_schema(tool_name) - Get input schema before executing
   - execute_tool(tool_name, arguments) - Execute any tool
   - respond(message) - ALWAYS use this to communicate with the user

WORKFLOW:
1. If you don't know which tool to use, call list_tools() or search_tools()
2. Once you find the right tool, call get_tool_schema() to see what arguments it needs
3. Call execute_tool() with the tool name and arguments to perform the action
4. Call respond() with the result to communicate back to the user

IMPORTANT: DO NOT output text directly to the user. ALWAYS use the respond() tool to send messages.

AVAILABLE TOOL CATEGORIES:
- **OVS**: Open vSwitch management (ovs_list_bridges, ovs_add_port, etc.)
- **Service**: Service management via s6 tools (s6_start_service, s6_stop_service, etc.)
- **D-Bus**: Direct D-Bus calls (dbus_call, dbus_introspect, etc.)
- **File**: File operations (file_read, file_write, file_list, etc.)
- **Shell**: Command execution (shell_exec, shell_which, etc.)
- **Network**: Kernel networking via rtnetlink (rtnetlink_list_links, etc.)
- **OpenFlow**: OpenFlow rule management (openflow_add_flow, etc.)
- **Agent**: AI agent operations (agent_spawn, agent_list, etc.)

SPECIAL AGENTS (ALWAYS AVAILABLE):
The following specialized agents are pre-loaded. Use them for complex tasks in their domain. NO need to check availability:
- agent_rust_pro: Rust development (build, check, test, fix)
- agent_backend_architect: System architecture design
- agent_network_engineer: Complex network diagnostics and routing
- agent_context_manager: Session context and memory management

IMPORTANT: Only call these agents if the user request matches their expertise. If the request is unrelated (e.g., "list files" does not require backend-architect), simply use the standard tools or ignore the agents.

EXAMPLES:
User: "List all OVS bridges"
1. search_tools(query="bridge")  → Find ovs_list_bridges
2. execute_tool(tool_name="ovs_list_bridges", arguments={})
3. respond(message="Found bridges: ...")

User: "Restart the xray reverse proxy"
1. search_tools(query="restart service")  → Find sv_restart_service
2. get_tool_schema(tool_name="sv_restart_service")  → See it needs "service" param
3. execute_tool(tool_name="sv_restart_service", arguments={"service": "gbr-xray"})
4. respond(message="gbr-xray has been restarted successfully")

User: "What tools are available for networking?"
1. list_tools(category="network")  → Browse network tools
2. respond(message="Available network tools include: ...")

User: "Create an OVS bridge called ovsbr0"
1. execute_tool(tool_name="ovs_create_bridge", arguments={"name": "ovsbr0"})
2. respond(message="Successfully created OVS bridge ovsbr0")

REMEMBER: You have access to s6 (service management), OVSDB (OVS), D-Bus (NetworkManager), and Netlink (kernel) - all via native protocols, not CLI.

HINT - OVS NETWORKING:
Creating an OVS bridge (`ovs_create_bridge`) does NOT create a Linux network interface automatically.
To assign an IP address to a bridge, you MUST add an internal port with the same name (or different name) to the bridge first.
Example:
1. execute_tool(tool_name="ovs_create_bridge", arguments={"name": "br0"})
2. execute_tool(tool_name="ovs_add_port", arguments={"bridge": "br0", "port": "br0", "type": "internal"})
3. execute_tool(tool_name="rtnetlink_add_address", arguments={"interface": "br0", ...})
"#.to_string()
    }

    /// Build system prompt with tool context
    ///
    /// Combines the base prompt with the injected tool list.
    #[allow(dead_code)]
    pub(crate) fn build_system_prompt(&self, tool_list: &str) -> String {
        let base_prompt = self.config.system_prompt.clone().unwrap_or_else(|| {
            "You are a helpful system administration assistant with access to various tools."
                .to_string()
        });

        format!(
            "{}\n\n## Available Tools\n\nYou have access to the following tools through the `execute_tool` function:\n\n{}\n\n## Instructions\n\n1. Use `list_tools` to see available tools by category\n2. Use `search_tools` to find relevant tools\n3. Use `get_tool_schema` to get detailed parameters for a tool\n4. Use `execute_tool` to run tools with the required arguments\n5. Use `respond` to send your final answer to the user\n\nAlways verify actions completed successfully before reporting completion.",
            base_prompt,
            tool_list
        )
    }
}
