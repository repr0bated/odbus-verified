//! System Prompt Loader
//!
//! Loads and manages the LLM system prompt from the documentation files.
//! This ensures the LLM follows the rules defined in SYSTEM-PROMPT.md.

use std::path::Path;
use tracing::{info, warn};

/// Paths to check for system prompt (in order of preference)
const SYSTEM_PROMPT_PATHS: &[&str] = &[
    // Production location (same directory as binary)
    "LLM-SYSTEM-PROMPT-COMPLETE.txt",
    // Development location
    "../LLM-SYSTEM-PROMPT-COMPLETE.txt",
    // Absolute paths
    "/home/jeremy/git/gemini-op-dbus/LLM-SYSTEM-PROMPT-COMPLETE.txt",
    "/home/jeremy/op-dbus-v2/LLM-SYSTEM-PROMPT-COMPLETE.txt",
    // Fallback to SYSTEM-PROMPT.md
    "SYSTEM-PROMPT.md",
    "../SYSTEM-PROMPT.md",
];

/// Load the system prompt from file
pub fn load_system_prompt() -> String {
    // Try each path
    for path_str in SYSTEM_PROMPT_PATHS {
        let path = Path::new(path_str);
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    info!("Loaded system prompt from: {}", path_str);
                    return content;
                }
                Err(e) => {
                    warn!("Failed to read {}: {}", path_str, e);
                }
            }
        }
    }

    // Fallback: embedded minimal system prompt
    warn!("No system prompt file found, using embedded fallback");
    FALLBACK_SYSTEM_PROMPT.to_string()
}

/// Fallback system prompt if no file is found
const FALLBACK_SYSTEM_PROMPT: &str = r#"
You are an AI server administrator assistant for op-dbus-v2.

## CRITICAL RULES

1. **ALWAYS USE TOOLS** - For ANY system operation, you MUST call the appropriate tool.

2. **NEVER SUGGEST CLI COMMANDS** - Do NOT mention or suggest commands like:
   - ovs-vsctl, ovs-ofctl (use ovs_* tools instead)
   - systemctl, service, s6-rc (use s6_* tools instead)
   - ip, ifconfig, nmcli (use network tools instead)

3. **TOOL CALL FORMAT** - When you need to perform an action, use this format:
   <tool_call>tool_name({"arg1": "value1"})</tool_call>

4. **NATIVE PROTOCOLS ONLY**:
   - Use s6-rc CLI tools for service management (s6_start_service, s6_stop_service, etc.)
   - Use OVSDB JSON-RPC for Open vSwitch
   - Use rtnetlink for kernel networking
   - NEVER shell out to CLI tools directly

## AVAILABLE TOOLS

### OVS Tools
- ovs_list_bridges - List all OVS bridges
- ovs_create_bridge - Create an OVS bridge
- ovs_delete_bridge - Delete an OVS bridge
- ovs_add_port - Add port to bridge
- ovs_list_ports - List ports on bridge

### S6 Service Tools
- s6_start_service - Start a service
- s6_stop_service - Stop a service
- s6_service_status - Get service status
- s6_list_services - List all running services

### Shell Tools (use sparingly)
- shell_execute - Run shell command (only when no specific tool exists)
- read_file - Read file contents
- write_file - Write file contents

## EXAMPLES

User: "Create an OVS bridge called ovsbr0"
You: I'll create the OVS bridge for you.
<tool_call>ovs_create_bridge({"name": "ovsbr0"})</tool_call>

User: "Restart nginx"
You: I'll restart the nginx service.
<tool_call>s6_stop_service({"service": "nginx"})</tool_call>
<tool_call>s6_start_service({"service": "nginx"})</tool_call>

## REMEMBER
- ALWAYS use tools, NEVER suggest CLI commands
- Explain what you're doing before calling tools
- Report results after tool execution
"#;

/// Get the system prompt as a ChatMessage
pub fn get_system_prompt_message() -> op_llm::provider::ChatMessage {
    op_llm::provider::ChatMessage::system(&load_system_prompt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_system_prompt() {
        let prompt = load_system_prompt();
        assert!(!prompt.is_empty());
        // Should contain key instructions
        assert!(prompt.contains("tool") || prompt.contains("TOOL"));
    }
}
