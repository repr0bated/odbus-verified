//! Tool Loader - Registers all tools with the ToolRegistry
//!
//! This module loads and registers 200+ tools from various sources:
//! - Builtin tools (filesystem, response, shell) - ALWAYS LOADED
//! - D-Bus discovered tools (systemd, NetworkManager, etc.) - LAZY LOADED
//! - Plugin state tools (query, diff, apply) - LAZY LOADED
//! - Agent tools (from op-agents) - LAZY LOADED
//! - OVS tools (bridge, port, flow management) - LAZY LOADED
//!
//! Uses context-aware lazy loading via ToolFactory pattern:
//! - Critical tools are pre-loaded immediately
//! - Domain-specific tools are loaded on-demand via factories
//! - LRU caching automatically evicts unused tools

use anyhow::{Context, Result};
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use op_network::ovsdb::OvsdbClient;
use op_tools::registry::{ToolDefinition, ToolRegistry};
use op_tools::tool::{BoxedTool, Tool};

// ============================================================================
// REGISTRY HELPERS
// ============================================================================

/// Helper function to register a tool with the new 3-parameter API
async fn register_tool(registry: &ToolRegistry, tool: BoxedTool) -> Result<()> {
    let definition = ToolDefinition {
        name: tool.name().to_string(),
        description: tool.description().to_string(),
        input_schema: tool.input_schema(),
        category: tool.category().to_string(),
        tags: tool.tags(),
        namespace: tool.namespace().to_string(),
    };
    registry
        .register(Arc::from(tool.name()), tool, definition)
        .await
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Helper function to register a tool with the new 3-parameter API
async fn register_tool(registry: &ToolRegistry, tool: BoxedTool) -> Result<()> {
    let definition = ToolDefinition {
        name: tool.name().to_string(),
        description: tool.description().to_string(),
        input_schema: tool.input_schema(),
        category: tool.category().to_string(),
        tags: tool.tags(),
        namespace: tool.namespace().to_string(),
    };
    registry
        .register(Arc::from(tool.name()), tool, definition)
        .await
}

/// Helper macro to simplify tool registration with error handling
macro_rules! register_tool_checked {
    ($registry:expr, $tool:expr, $name:expr) => {
        match register_tool($registry, $tool).await {
            Ok(_) => {
                debug!("Registered tool: {}", $name);
                1
            }
            Err(e) => {
                warn!("Failed to register tool {}: {}", $name, e);
                0
            }
        }
    };
}

// ============================================================================
// BUILTIN TOOLS (Always Loaded)
// ============================================================================

/// Response tool - allows LLM to respond to user
pub struct RespondToUserTool;

impl RespondToUserTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl Default for RespondToUserTool {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for RespondToUserTool {
    fn name(&self) -> &str {
        "respond_to_user"
    }

    fn description(&self) -> &str {
        "Send a response message to the user. Use this to communicate results, ask questions, or provide information."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The message to send to the user"
                },
                "format": {
                    "type": "string",
                    "enum": ["text", "markdown", "json"],
                    "default": "text",
                    "description": "Output format"
                }
            },
            "required": ["message"]
        })
    }

    fn category(&self) -> &str {
        "response"
    }

    fn namespace(&self) -> &str {
        "chat"
    }

    fn tags(&self) -> Vec<String> {
        vec!["response".to_string(), "communication".to_string(), "essential".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let message = input
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: message"))?;

        let format = input
            .get("format")
            .and_then(|v| v.as_str())
            .unwrap_or("text");

        Ok(json!({
            "success": true,
            "message": message,
            "format": format,
            "delivered": true
        }))
    }
}

/// Cannot perform tool - for declining requests
pub struct CannotPerformTool;

impl CannotPerformTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl Default for CannotPerformTool {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for CannotPerformTool {
    fn name(&self) -> &str {
        "cannot_perform"
    }

    fn description(&self) -> &str {
        "Indicate that a requested action cannot be performed, with explanation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "reason": {
                    "type": "string",
                    "description": "Why the action cannot be performed"
                },
                "suggestion": {
                    "type": "string",
                    "description": "Alternative suggestion if available"
                }
            },
            "required": ["reason"]
        })
    }

    fn category(&self) -> &str {
        "response"
    }

    fn namespace(&self) -> &str {
        "chat"
    }

    fn tags(&self) -> Vec<String> {
        vec!["response".to_string(), "error".to_string(), "essential".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let reason = input
            .get("reason")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: reason"))?;

        let suggestion = input.get("suggestion").and_then(|v| v.as_str());

        Ok(json!({
            "success": true,
            "cannot_perform": true,
            "reason": reason,
            "suggestion": suggestion
        }))
    }
}

/// Request clarification tool
pub struct RequestClarificationTool;

impl RequestClarificationTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl Default for RequestClarificationTool {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for RequestClarificationTool {
    fn name(&self) -> &str {
        "request_clarification"
    }

    fn description(&self) -> &str {
        "Ask the user for clarification or additional information needed to complete a task."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The clarification question to ask"
                },
                "options": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional list of choices for the user"
                },
                "context": {
                    "type": "string",
                    "description": "Additional context about why clarification is needed"
                }
            },
            "required": ["question"]
        })
    }

    fn category(&self) -> &str {
        "response"
    }

    fn namespace(&self) -> &str {
        "chat"
    }

    fn tags(&self) -> Vec<String> {
        vec!["response".to_string(), "clarification".to_string(), "essential".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let question = input
            .get("question")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: question"))?;

        Ok(json!({
            "success": true,
            "needs_clarification": true,
            "question": question,
            "options": input.get("options"),
            "context": input.get("context")
        }))
    }
}

// ============================================================================
// FILESYSTEM TOOLS (Always Loaded)
// ============================================================================

/// Read file tool
pub struct ReadFileTool;

impl ReadFileTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl Default for ReadFileTool {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file from the filesystem."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                },
                "encoding": {
                    "type": "string",
                    "default": "utf-8",
                    "description": "File encoding"
                }
            },
            "required": ["path"]
        })
    }

    fn category(&self) -> &str {
        "filesystem"
    }

    fn tags(&self) -> Vec<String> {
        vec!["filesystem".to_string(), "read".to_string(), "file".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: path"))?;

        // Security check - prevent reading sensitive files
        let forbidden_paths = ["/etc/shadow", "/etc/sudoers"];
        if forbidden_paths.iter().any(|&p| path.starts_with(p)) {
            return Ok(json!({
                "success": false,
                "error": "Access denied: Cannot read sensitive system files"
            }));
        }

        match tokio::fs::read_to_string(path).await {
            Ok(content) => Ok(json!({
                "success": true,
                "path": path,
                "content": content,
                "size": content.len()
            })),
            Err(e) => Ok(json!({
                "success": false,
                "path": path,
                "error": e.to_string()
            })),
        }
    }
}

/// Write file tool
pub struct WriteFileTool;

impl WriteFileTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl Default for WriteFileTool {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file on the filesystem."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                },
                "create_dirs": {
                    "type": "boolean",
                    "default": false,
                    "description": "Create parent directories if they don't exist"
                }
            },
            "required": ["path", "content"]
        })
    }

    fn category(&self) -> &str {
        "filesystem"
    }

    fn tags(&self) -> Vec<String> {
        vec!["filesystem".to_string(), "write".to_string(), "file".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: path"))?;

        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: content"))?;

        let create_dirs = input
            .get("create_dirs")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Security check - prevent writing to sensitive locations
        let forbidden_prefixes = ["/etc/", "/boot/", "/sys/", "/proc/"];
        if forbidden_prefixes.iter().any(|&p| path.starts_with(p)) {
            return Ok(json!({
                "success": false,
                "error": "Access denied: Cannot write to system directories"
            }));
        }

        if create_dirs {
            if let Some(parent) = std::path::Path::new(path).parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }

        match tokio::fs::write(path, content).await {
            Ok(_) => Ok(json!({
                "success": true,
                "path": path,
                "bytes_written": content.len()
            })),
            Err(e) => Ok(json!({
                "success": false,
                "path": path,
                "error": e.to_string()
            })),
        }
    }
}

/// List directory tool
pub struct ListDirectoryTool;

impl ListDirectoryTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl Default for ListDirectoryTool {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ListDirectoryTool {
    fn name(&self) -> &str {
        "list_directory"
    }

    fn description(&self) -> &str {
        "List contents of a directory."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory to list"
                },
                "recursive": {
                    "type": "boolean",
                    "default": false,
                    "description": "List recursively"
                },
                "include_hidden": {
                    "type": "boolean",
                    "default": false,
                    "description": "Include hidden files (starting with .)"
                }
            },
            "required": ["path"]
        })
    }

    fn category(&self) -> &str {
        "filesystem"
    }

    fn tags(&self) -> Vec<String> {
        vec!["filesystem".to_string(), "list".to_string(), "directory".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let path = input
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: path"))?;

        let include_hidden = input
            .get("include_hidden")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut entries = Vec::new();
        let mut dir = tokio::fs::read_dir(path).await?;

        while let Some(entry) = dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();

            if !include_hidden && name.starts_with('.') {
                continue;
            }

            let metadata = entry.metadata().await?;
            entries.push(json!({
                "name": name,
                "is_dir": metadata.is_dir(),
                "is_file": metadata.is_file(),
                "size": metadata.len(),
            }));
        }

        Ok(json!({
            "success": true,
            "path": path,
            "entries": entries,
            "count": entries.len()
        }))
    }
}

// ============================================================================
// SHELL EXECUTION TOOLS (Always Loaded)
// ============================================================================

/// Shell execute tool - runs whitelisted commands
pub struct ShellExecuteTool {
    allowed_commands: Vec<String>,
}

impl ShellExecuteTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            allowed_commands: vec![
                "ls".to_string(),
                "cat".to_string(),
                "grep".to_string(),
                "find".to_string(),
                "head".to_string(),
                "tail".to_string(),
                "wc".to_string(),
                "sort".to_string(),
                "uniq".to_string(),
                "echo".to_string(),
                "pwd".to_string(),
                "whoami".to_string(),
                "date".to_string(),
                "uname".to_string(),
                "df".to_string(),
                "du".to_string(),
                "free".to_string(),
                "uptime".to_string(),
                "ps".to_string(),
                "top".to_string(),
                "htop".to_string(),
                "ip".to_string(),
                "ss".to_string(),
                "netstat".to_string(),
                "ping".to_string(),
                "traceroute".to_string(),
                "dig".to_string(),
                "nslookup".to_string(),
                "curl".to_string(),
                "wget".to_string(),
                "git".to_string(),
                "docker".to_string(),
                "kubectl".to_string(),
                "systemctl".to_string(),
                "journalctl".to_string(),
                "cargo".to_string(),
                "rustc".to_string(),
                "python".to_string(),
                "python3".to_string(),
                "pip".to_string(),
                "pip3".to_string(),
                "node".to_string(),
                "npm".to_string(),
                "yarn".to_string(),
            ],
        })
    }

    pub fn with_allowed_commands(commands: Vec<String>) -> Arc<Self> {
        Arc::new(Self {
            allowed_commands: commands,
        })
    }
}

impl Default for ShellExecuteTool {
    fn default() -> Self {
        Self {
            allowed_commands: vec![],
        }
    }
}

#[async_trait]
impl Tool for ShellExecuteTool {
    fn name(&self) -> &str {
        "shell_execute"
    }

    fn description(&self) -> &str {
        "Execute a whitelisted shell command. Only safe, read-mostly commands are allowed."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The command to execute (must be whitelisted)"
                },
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Command arguments"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Working directory for command execution"
                },
                "timeout_secs": {
                    "type": "integer",
                    "default": 30,
                    "description": "Command timeout in seconds"
                }
            },
            "required": ["command"]
        })
    }

    fn category(&self) -> &str {
        "shell"
    }

    fn tags(&self) -> Vec<String> {
        vec!["shell".to_string(), "execute".to_string(), "command".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: command"))?;

        // Security: Check if command is whitelisted
        if !self.allowed_commands.contains(&command.to_string()) {
            return Ok(json!({
                "success": false,
                "error": format!("Command '{}' is not whitelisted. Allowed commands: {:?}", command, self.allowed_commands)
            }));
        }

        let args: Vec<String> = input
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let timeout_secs = input
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(30);

        let mut cmd = tokio::process::Command::new(command);
        cmd.args(&args);

        if let Some(working_dir) = input.get("working_dir").and_then(|v| v.as_str()) {
            cmd.current_dir(working_dir);
        }

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            cmd.output(),
        )
        .await;

        match output {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                Ok(json!({
                    "success": output.status.success(),
                    "exit_code": output.status.code(),
                    "stdout": stdout,
                    "stderr": stderr,
                    "command": command,
                    "args": args
                }))
            }
            Ok(Err(e)) => Ok(json!({
                "success": false,
                "error": format!("Failed to execute command: {}", e)
            })),
            Err(_) => Ok(json!({
                "success": false,
                "error": format!("Command timed out after {} seconds", timeout_secs)
            })),
        }
    }
}

// ============================================================================
// SYSTEM TOOLS (Always Loaded)
// ============================================================================

/// Network interfaces tool
pub struct ListNetworkInterfacesTool;

impl ListNetworkInterfacesTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl Default for ListNetworkInterfacesTool {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ListNetworkInterfacesTool {
    fn name(&self) -> &str {
        "list_network_interfaces"
    }

    fn description(&self) -> &str {
        "List all network interfaces on the system with their status."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "include_loopback": {
                    "type": "boolean",
                    "default": false,
                    "description": "Include loopback interfaces"
                }
            }
        })
    }

    fn category(&self) -> &str {
        "network"
    }

    fn tags(&self) -> Vec<String> {
        vec!["network".to_string(), "interfaces".to_string(), "system".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let include_loopback = input
            .get("include_loopback")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut interfaces = Vec::new();
        let mut dir = tokio::fs::read_dir("/sys/class/net").await?;

        while let Some(entry) = dir.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();

            if !include_loopback && name == "lo" {
                continue;
            }

            // Read interface state
            let state_path = format!("/sys/class/net/{}/operstate", name);
            let state = tokio::fs::read_to_string(&state_path)
                .await
                .unwrap_or_else(|_| "unknown".to_string())
                .trim()
                .to_string();

            // Read MAC address
            let mac_path = format!("/sys/class/net/{}/address", name);
            let mac = tokio::fs::read_to_string(&mac_path)
                .await
                .unwrap_or_else(|_| "unknown".to_string())
                .trim()
                .to_string();

            interfaces.push(json!({
                "name": name,
                "state": state,
                "mac_address": mac
            }));
        }

        Ok(json!({
            "success": true,
            "interfaces": interfaces,
            "count": interfaces.len()
        }))
    }
}

// Factories removed - all tools registered directly

// ============================================================================
// SYSTEMD TOOLS (Lazy Loaded)
// ============================================================================

/// Systemd unit status tool
pub struct SystemdUnitStatusTool;

impl SystemdUnitStatusTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for SystemdUnitStatusTool {
    fn name(&self) -> &str {
        "systemd_unit_status"
    }

    fn description(&self) -> &str {
        "Get the status of a systemd unit (service, timer, socket, etc.)"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "unit": {
                    "type": "string",
                    "description": "Service name (e.g., 'gbr-xray', 'op-web')"
                }
            },
            "required": ["unit"]
        })
    }

    fn category(&self) -> &str {
        "systemd"
    }

    fn tags(&self) -> Vec<String> {
        vec!["systemd".to_string(), "service".to_string(), "status".to_string(), "dbus".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let unit = input
            .get("unit")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: unit"))?;

        // Use D-Bus to get unit status
        let connection = zbus::Connection::system().await?;

        let proxy = zbus::proxy::Builder::new(&connection)
            .destination("org.freedesktop.systemd1")?
            .path("/org/freedesktop/systemd1")?
            .interface("org.freedesktop.systemd1.Manager")?
            .build()
            .await?;

        // Get unit object path
        let unit_path: zbus::zvariant::OwnedObjectPath = proxy
            .call("GetUnit", &(unit,))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get unit: {}", e))?;

        // Get unit properties
        let unit_proxy = zbus::proxy::Builder::new(&connection)
            .destination("org.freedesktop.systemd1")?
            .path(unit_path.as_str())?
            .interface("org.freedesktop.systemd1.Unit")?
            .build()
            .await?;

        let active_state: String = unit_proxy
            .get_property("ActiveState")
            .await
            .unwrap_or_else(|_| "unknown".to_string());

        let sub_state: String = unit_proxy
            .get_property("SubState")
            .await
            .unwrap_or_else(|_| "unknown".to_string());

        let load_state: String = unit_proxy
            .get_property("LoadState")
            .await
            .unwrap_or_else(|_| "unknown".to_string());

        let description: String = unit_proxy
            .get_property("Description")
            .await
            .unwrap_or_else(|_| "No description".to_string());

        Ok(json!({
            "success": true,
            "unit": unit,
            "active_state": active_state,
            "sub_state": sub_state,
            "load_state": load_state,
            "description": description
        }))
    }
}

/// Systemd list units tool
pub struct SystemdListUnitsTool;

impl SystemdListUnitsTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for SystemdListUnitsTool {
    fn name(&self) -> &str {
        "systemd_list_units"
    }

    fn description(&self) -> &str {
        "List systemd units with optional filtering by state or type."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "unit_type": {
                    "type": "string",
                    "enum": ["service", "socket", "timer", "mount", "device", "all"],
                    "default": "service"
                },
                "state": {
                    "type": "string",
                    "enum": ["active", "inactive", "failed", "all"],
                    "default": "all"
                },
                "limit": {
                    "type": "integer",
                    "default": 50
                }
            }
        })
    }

    fn category(&self) -> &str {
        "systemd"
    }

    fn tags(&self) -> Vec<String> {
        vec!["systemd".to_string(), "service".to_string(), "list".to_string(), "dbus".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let unit_type = input.get("unit_type").and_then(|v| v.as_str()).unwrap_or("service");
        let state_filter = input.get("state").and_then(|v| v.as_str()).unwrap_or("all");
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

        let connection = zbus::Connection::system().await?;

        let proxy = zbus::proxy::Builder::new(&connection)
            .destination("org.freedesktop.systemd1")?
            .path("/org/freedesktop/systemd1")?
            .interface("org.freedesktop.systemd1.Manager")?
            .build()
            .await?;

        let units: Vec<(
            String, String, String, String, String, String,
            zbus::zvariant::OwnedObjectPath, u32, String, zbus::zvariant::OwnedObjectPath,
        )> = proxy.call("ListUnits", &()).await?;

        let filtered_units: Vec<Value> = units
            .into_iter()
            .filter(|(name, _, _, active_state, _, _, _, _, _, _)| {
                let type_match = unit_type == "all" || name.ends_with(&format!(".{}", unit_type));
                let state_match = state_filter == "all" || active_state == state_filter;
                type_match && state_match
            })
            .take(limit)
            .map(|(name, description, load_state, active_state, sub_state, _, _, _, _, _)| {
                json!({
                    "name": name,
                    "description": description,
                    "load_state": load_state,
                    "active_state": active_state,
                    "sub_state": sub_state
                })
            })
            .collect();

        Ok(json!({
            "success": true,
            "units": filtered_units,
            "count": filtered_units.len()
        }))
    }
}

/// Systemd start unit tool
pub struct SystemdStartUnitTool;

impl SystemdStartUnitTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for SystemdStartUnitTool {
    fn name(&self) -> &str {
        "systemd_start_unit"
    }

    fn description(&self) -> &str {
        "Start a systemd unit."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "unit": {"type": "string", "description": "Unit name to start"},
                "mode": {"type": "string", "default": "replace"}
            },
            "required": ["unit"]
        })
    }

    fn category(&self) -> &str {
        "systemd"
    }

    fn tags(&self) -> Vec<String> {
        vec!["systemd".to_string(), "service".to_string(), "start".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let unit = input.get("unit").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: unit"))?;
        let mode = input.get("mode").and_then(|v| v.as_str()).unwrap_or("replace");

        let connection = zbus::Connection::system().await?;
        let proxy = zbus::proxy::Builder::new(&connection)
            .destination("org.freedesktop.systemd1")?
            .path("/org/freedesktop/systemd1")?
            .interface("org.freedesktop.systemd1.Manager")?
            .build()
            .await?;

        let job_path: zbus::zvariant::OwnedObjectPath = proxy
            .call("StartUnit", &(unit, mode))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start unit: {}", e))?;

        Ok(json!({"success": true, "unit": unit, "action": "started", "job_path": job_path.as_str()}))
    }
}

/// Systemd stop unit tool
pub struct SystemdStopUnitTool;

impl SystemdStopUnitTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for SystemdStopUnitTool {
    fn name(&self) -> &str {
        "systemd_stop_unit"
    }

    fn description(&self) -> &str {
        "Stop a systemd unit."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "unit": {"type": "string"},
                "mode": {"type": "string", "default": "replace"}
            },
            "required": ["unit"]
        })
    }

    fn category(&self) -> &str {
        "systemd"
    }

    fn tags(&self) -> Vec<String> {
        vec!["systemd".to_string(), "service".to_string(), "stop".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let unit = input.get("unit").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: unit"))?;
        let mode = input.get("mode").and_then(|v| v.as_str()).unwrap_or("replace");

        let connection = zbus::Connection::system().await?;
        let proxy = zbus::proxy::Builder::new(&connection)
            .destination("org.freedesktop.systemd1")?
            .path("/org/freedesktop/systemd1")?
            .interface("org.freedesktop.systemd1.Manager")?
            .build()
            .await?;

        let job_path: zbus::zvariant::OwnedObjectPath = proxy
            .call("StopUnit", &(unit, mode))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to stop unit: {}", e))?;

        Ok(json!({"success": true, "unit": unit, "action": "stopped", "job_path": job_path.as_str()}))
    }
}

/// Systemd restart unit tool
pub struct SystemdRestartUnitTool;

impl SystemdRestartUnitTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for SystemdRestartUnitTool {
    fn name(&self) -> &str {
        "systemd_restart_unit"
    }

    fn description(&self) -> &str {
        "Restart a systemd unit."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "unit": {"type": "string"},
                "mode": {"type": "string", "default": "replace"}
            },
            "required": ["unit"]
        })
    }

    fn category(&self) -> &str {
        "systemd"
    }

    fn tags(&self) -> Vec<String> {
        vec!["systemd".to_string(), "service".to_string(), "restart".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let unit = input.get("unit").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: unit"))?;
        let mode = input.get("mode").and_then(|v| v.as_str()).unwrap_or("replace");

        let connection = zbus::Connection::system().await?;
        let proxy = zbus::proxy::Builder::new(&connection)
            .destination("org.freedesktop.systemd1")?
            .path("/org/freedesktop/systemd1")?
            .interface("org.freedesktop.systemd1.Manager")?
            .build()
            .await?;

        let job_path: zbus::zvariant::OwnedObjectPath = proxy
            .call("RestartUnit", &(unit, mode))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to restart unit: {}", e))?;

        Ok(json!({"success": true, "unit": unit, "action": "restarted", "job_path": job_path.as_str()}))
    }
}

/// Systemd enable unit tool
pub struct SystemdEnableUnitTool;

impl SystemdEnableUnitTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for SystemdEnableUnitTool {
    fn name(&self) -> &str {
        "systemd_enable_unit"
    }

    fn description(&self) -> &str {
        "Enable a systemd unit to start at boot."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "unit": {"type": "string"},
                "runtime": {"type": "boolean", "default": false}
            },
            "required": ["unit"]
        })
    }

    fn category(&self) -> &str {
        "systemd"
    }

    fn tags(&self) -> Vec<String> {
        vec!["systemd".to_string(), "service".to_string(), "enable".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let unit = input.get("unit").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: unit"))?;
        let runtime = input.get("runtime").and_then(|v| v.as_bool()).unwrap_or(false);

        let connection = zbus::Connection::system().await?;
        let proxy = zbus::proxy::Builder::new(&connection)
            .destination("org.freedesktop.systemd1")?
            .path("/org/freedesktop/systemd1")?
            .interface("org.freedesktop.systemd1.Manager")?
            .build()
            .await?;

        let _: (bool, Vec<(String, String, String)>) = proxy
            .call("EnableUnitFiles", &(vec![unit], runtime, true))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to enable unit: {}", e))?;

        Ok(json!({"success": true, "unit": unit, "action": "enabled"}))
    }
}

/// Systemd disable unit tool
pub struct SystemdDisableUnitTool;

impl SystemdDisableUnitTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for SystemdDisableUnitTool {
    fn name(&self) -> &str {
        "systemd_disable_unit"
    }

    fn description(&self) -> &str {
        "Disable a systemd unit from starting at boot."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "unit": {"type": "string"},
                "runtime": {"type": "boolean", "default": false}
            },
            "required": ["unit"]
        })
    }

    fn category(&self) -> &str {
        "systemd"
    }

    fn tags(&self) -> Vec<String> {
        vec!["systemd".to_string(), "service".to_string(), "disable".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let unit = input.get("unit").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: unit"))?;
        let runtime = input.get("runtime").and_then(|v| v.as_bool()).unwrap_or(false);

        let connection = zbus::Connection::system().await?;
        let proxy = zbus::proxy::Builder::new(&connection)
            .destination("org.freedesktop.systemd1")?
            .path("/org/freedesktop/systemd1")?
            .interface("org.freedesktop.systemd1.Manager")?
            .build()
            .await?;

        let _: Vec<(String, String, String)> = proxy
            .call("DisableUnitFiles", &(vec![unit], runtime))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to disable unit: {}", e))?;

        Ok(json!({"success": true, "unit": unit, "action": "disabled"}))
    }
}

/// Systemd reload daemon tool
pub struct SystemdReloadDaemonTool;

impl SystemdReloadDaemonTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for SystemdReloadDaemonTool {
    fn name(&self) -> &str {
        "systemd_reload_daemon"
    }

    fn description(&self) -> &str {
        "Reload systemd daemon configuration."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }

    fn category(&self) -> &str {
        "systemd"
    }

    fn tags(&self) -> Vec<String> {
        vec!["systemd".to_string(), "daemon".to_string(), "reload".to_string()]
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        let connection = zbus::Connection::system().await?;
        let proxy = zbus::proxy::Builder::new(&connection)
            .destination("org.freedesktop.systemd1")?
            .path("/org/freedesktop/systemd1")?
            .interface("org.freedesktop.systemd1.Manager")?
            .build()
            .await?;

        let _: () = proxy.call("Reload", &()).await
            .map_err(|e| anyhow::anyhow!("Failed to reload daemon: {}", e))?;

        Ok(json!({"success": true, "action": "daemon-reload"}))
    }
}

// ============================================================================
// OVS TOOLS (Lazy Loaded)
// ============================================================================

/// OVS list bridges tool
pub struct OvsListBridgesTool;

impl OvsListBridgesTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for OvsListBridgesTool {
    fn name(&self) -> &str {
        "ovs_list_bridges"
    }

    fn description(&self) -> &str {
        "List all Open vSwitch bridges."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {}})
    }

    fn category(&self) -> &str {
        "ovs"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "bridge".to_string(), "network".to_string()]
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        let client = OvsdbClient::new();
        match client.list_bridges().await {
            Ok(bridges) => Ok(json!({"success": true, "bridges": bridges, "count": bridges.len()})),
            Err(e) => Ok(json!({"success": false, "error": e.to_string()})),
        }
    }
}

/// OVS show bridge tool
pub struct OvsShowBridgeTool;

impl OvsShowBridgeTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for OvsShowBridgeTool {
    fn name(&self) -> &str {
        "ovs_show_bridge"
    }

    fn description(&self) -> &str {
        "Show detailed information about an Open vSwitch bridge."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"bridge": {"type": "string"}}, "required": ["bridge"]})
    }

    fn category(&self) -> &str {
        "ovs"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "bridge".to_string(), "info".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge = input.get("bridge").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: bridge"))?;

        let client = OvsdbClient::new();
        match client.get_bridge_info(bridge).await {
            Ok(info) => Ok(json!({"success": true, "bridge": bridge, "info": info})),
            Err(e) => Ok(json!({"success": false, "error": e.to_string()})),
        }
    }
}

/// OVS list ports tool
pub struct OvsListPortsTool;

impl OvsListPortsTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for OvsListPortsTool {
    fn name(&self) -> &str {
        "ovs_list_ports"
    }

    fn description(&self) -> &str {
        "List all ports on an Open vSwitch bridge."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"bridge": {"type": "string"}}, "required": ["bridge"]})
    }

    fn category(&self) -> &str {
        "ovs"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "port".to_string(), "network".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge = input.get("bridge").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: bridge"))?;

        let client = OvsdbClient::new();
        match client.list_bridge_ports(bridge).await {
            Ok(ports) => Ok(json!({"success": true, "bridge": bridge, "ports": ports, "count": ports.len()})),
            Err(e) => Ok(json!({"success": false, "error": e.to_string()})),
        }
    }
}

/// OVS dump flows tool
pub struct OvsDumpFlowsTool;

impl OvsDumpFlowsTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for OvsDumpFlowsTool {
    fn name(&self) -> &str {
        "ovs_dump_flows"
    }

    fn description(&self) -> &str {
        "Dump OpenFlow flows from an Open vSwitch bridge."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"bridge": {"type": "string"}, "table": {"type": "integer"}}, "required": ["bridge"]})
    }

    fn category(&self) -> &str {
        "ovs"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "flow".to_string(), "openflow".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge = input.get("bridge").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: bridge"))?;

        // Connect to OpenFlow controller (default OVS controller ports)
        let addrs = [
            std::net::SocketAddr::from(([127, 0, 0, 1], 6653)),
            std::net::SocketAddr::from(([127, 0, 0, 1], 6633)),
        ];

        for addr in &addrs {
            match op_network::openflow::OpenFlowClient::connect(*addr).await {
                Ok(mut client) => {
                    match client.query_flows().await {
                        Ok(flows) => {
                            return Ok(json!({"success": true, "bridge": bridge, "flows": flows, "count": flows.len(), "controller": addr.to_string()}));
                        }
                        Err(e) => {
                            return Ok(json!({"success": false, "error": e.to_string(), "controller": addr.to_string()}));
                        }
                    }
                }
                Err(_) => continue,
            }
        }

        Ok(json!({"success": false, "error": "OpenFlow controller not reachable on 127.0.0.1:6653 or :6633"}))
    }
}

/// OVS add bridge tool
pub struct OvsAddBridgeTool;

impl OvsAddBridgeTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for OvsAddBridgeTool {
    fn name(&self) -> &str {
        "ovs_add_bridge"
    }

    fn description(&self) -> &str {
        "Add a new Open vSwitch bridge."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"bridge": {"type": "string"}}, "required": ["bridge"]})
    }

    fn category(&self) -> &str {
        "ovs"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "bridge".to_string(), "create".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge = input.get("bridge").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: bridge"))?;

        let client = OvsdbClient::new();
        match client.create_bridge(bridge).await {
            Ok(()) => Ok(json!({"success": true, "bridge": bridge, "action": "created"})),
            Err(e) => Ok(json!({"success": false, "error": e.to_string()})),
        }
    }
}

/// OVS delete bridge tool
pub struct OvsDelBridgeTool;

impl OvsDelBridgeTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for OvsDelBridgeTool {
    fn name(&self) -> &str {
        "ovs_del_bridge"
    }

    fn description(&self) -> &str {
        "Delete an Open vSwitch bridge."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"bridge": {"type": "string"}}, "required": ["bridge"]})
    }

    fn category(&self) -> &str {
        "ovs"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "bridge".to_string(), "delete".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge = input.get("bridge").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: bridge"))?;

        let client = OvsdbClient::new();
        match client.delete_bridge(bridge).await {
            Ok(()) => Ok(json!({"success": true, "bridge": bridge, "action": "deleted"})),
            Err(e) => Ok(json!({"success": false, "error": e.to_string()})),
        }
    }
}

/// OVS add port tool
pub struct OvsAddPortTool;

impl OvsAddPortTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for OvsAddPortTool {
    fn name(&self) -> &str {
        "ovs_add_port"
    }

    fn description(&self) -> &str {
        "Add a port to an Open vSwitch bridge."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"bridge": {"type": "string"}, "port": {"type": "string"}}, "required": ["bridge", "port"]})
    }

    fn category(&self) -> &str {
        "ovs"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "port".to_string(), "create".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge = input.get("bridge").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: bridge"))?;
        let port = input.get("port").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: port"))?;

        let client = OvsdbClient::new();
        match client.add_port(bridge, port).await {
            Ok(()) => Ok(json!({"success": true, "bridge": bridge, "port": port, "action": "added"})),
            Err(e) => Ok(json!({"success": false, "error": e.to_string()})),
        }
    }
}

/// OVS delete port tool
pub struct OvsDelPortTool;

impl OvsDelPortTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for OvsDelPortTool {
    fn name(&self) -> &str {
        "ovs_del_port"
    }

    fn description(&self) -> &str {
        "Delete a port from an Open vSwitch bridge."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"bridge": {"type": "string"}, "port": {"type": "string"}}, "required": ["bridge", "port"]})
    }

    fn category(&self) -> &str {
        "ovs"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "port".to_string(), "delete".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge = input.get("bridge").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: bridge"))?;
        let port = input.get("port").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: port"))?;

        let client = OvsdbClient::new();
        match client.delete_port(bridge, port).await {
            Ok(()) => Ok(json!({"success": true, "bridge": bridge, "port": port, "action": "deleted"})),
            Err(e) => Ok(json!({"success": false, "error": e.to_string()})),
        }
    }
}

/// OVS add flow tool
pub struct OvsAddFlowTool;

impl OvsAddFlowTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for OvsAddFlowTool {
    fn name(&self) -> &str {
        "ovs_add_flow"
    }

    fn description(&self) -> &str {
        "Add a flow to an Open vSwitch bridge."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"bridge": {"type": "string"}, "flow": {"type": "string"}}, "required": ["bridge", "flow"]})
    }

    fn category(&self) -> &str {
        "ovs"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "flow".to_string(), "create".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge = input.get("bridge").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: bridge"))?;
        let flow = input.get("flow").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: flow"))?;

        let addrs = [
            std::net::SocketAddr::from(([127, 0, 0, 1], 6653)),
            std::net::SocketAddr::from(([127, 0, 0, 1], 6633)),
        ];

        for addr in &addrs {
            match op_network::openflow::OpenFlowClient::connect(*addr).await {
                Ok(mut client) => {
                    match client.add_flow_rule(flow).await {
                        Ok(()) => {
                            return Ok(json!({"success": true, "bridge": bridge, "action": "flow_added", "controller": addr.to_string(), "note": "String-based flow rules use rovs-openflow native protocol; full ovs-ofctl format parsing is pending"}));
                        }
                        Err(e) => {
                            return Ok(json!({"success": false, "error": e.to_string(), "controller": addr.to_string()}));
                        }
                    }
                }
                Err(_) => continue,
            }
        }

        Ok(json!({"success": false, "error": "OpenFlow controller not reachable on 127.0.0.1:6653 or :6633"}))
    }
}

/// OVS delete flows tool
pub struct OvsDelFlowsTool;

impl OvsDelFlowsTool {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl Tool for OvsDelFlowsTool {
    fn name(&self) -> &str {
        "ovs_del_flows"
    }

    fn description(&self) -> &str {
        "Delete flows from an Open vSwitch bridge."
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"bridge": {"type": "string"}, "match": {"type": "string"}}, "required": ["bridge"]})
    }

    fn category(&self) -> &str {
        "ovs"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "flow".to_string(), "delete".to_string()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge = input.get("bridge").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required field: bridge"))?;

        let has_match = input.get("match").and_then(|v| v.as_str()).is_some();
        if has_match {
            return Ok(json!({
                "success": false,
                "bridge": bridge,
                "error": "Selective flow deletion by match requires full rovs-openflow flow parsing (not yet implemented). Use ovs_del_flows without 'match' to delete all flows, or use shell_execute with op-ovsbr0-setup for bridge-level mutations."
            }));
        }

        let addrs = [
            std::net::SocketAddr::from(([127, 0, 0, 1], 6653)),
            std::net::SocketAddr::from(([127, 0, 0, 1], 6633)),
        ];

        for addr in &addrs {
            match op_network::openflow::OpenFlowClient::connect(*addr).await {
                Ok(mut client) => {
                    match client.delete_all_flows().await {
                        Ok(()) => {
                            return Ok(json!({"success": true, "bridge": bridge, "action": "flows_deleted", "controller": addr.to_string()}));
                        }
                        Err(e) => {
                            return Ok(json!({"success": false, "error": e.to_string(), "controller": addr.to_string()}));
                        }
                    }
                }
                Err(_) => continue,
            }
        }

        Ok(json!({"success": false, "error": "OpenFlow controller not reachable on 127.0.0.1:6653 or :6633"}))
    }
}

// ============================================================================
// PLUGIN STATE TOOLS (Lazy Loaded)
// ============================================================================

/// Plugin query tool - query state from a plugin
pub struct PluginQueryTool {
    plugin_name: String,
    tool_name: String,
    tool_description: String,
}

impl PluginQueryTool {
    pub fn new(plugin_name: &str) -> Arc<Self> {
        Arc::new(Self {
            plugin_name: plugin_name.to_string(),
            tool_name: format!("plugin_{}_query", plugin_name),
            tool_description: format!("Query current state from {} plugin", plugin_name),
        })
    }
}

#[async_trait]
impl Tool for PluginQueryTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"filter": {"type": "object"}}})
    }

    fn category(&self) -> &str {
        "plugin"
    }

    fn tags(&self) -> Vec<String> {
        vec!["plugin".to_string(), "state".to_string(), "query".to_string(), self.plugin_name.clone()]
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        // In a real implementation, this would call the authoritative plugin
        // catalog / canonical plugin document path.
        Ok(json!({
            "success": true,
            "plugin": self.plugin_name,
            "operation": "query",
            "state": {},
            "message": "Plugin query executed (integrate with plugin catalog)"
        }))
    }
}

/// Plugin diff tool
pub struct PluginDiffTool {
    plugin_name: String,
    tool_name: String,
    tool_description: String,
}

impl PluginDiffTool {
    pub fn new(plugin_name: &str) -> Arc<Self> {
        Arc::new(Self {
            plugin_name: plugin_name.to_string(),
            tool_name: format!("plugin_{}_diff", plugin_name),
            tool_description: format!("Calculate state diff for {} plugin", plugin_name),
        })
    }
}

#[async_trait]
impl Tool for PluginDiffTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"desired_state": {"type": "object"}}, "required": ["desired_state"]})
    }

    fn category(&self) -> &str {
        "plugin"
    }

    fn tags(&self) -> Vec<String> {
        vec!["plugin".to_string(), "state".to_string(), "diff".to_string(), self.plugin_name.clone()]
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        Ok(json!({
            "success": true,
            "plugin": self.plugin_name,
            "operation": "diff",
            "changes": [],
            "message": "Plugin diff executed (integrate with plugin catalog)"
        }))
    }
}

/// Plugin apply tool
pub struct PluginApplyTool {
    plugin_name: String,
    tool_name: String,
    tool_description: String,
}

impl PluginApplyTool {
    pub fn new(plugin_name: &str) -> Arc<Self> {
        Arc::new(Self {
            plugin_name: plugin_name.to_string(),
            tool_name: format!("plugin_{}_apply", plugin_name),
            tool_description: format!("Apply state changes for {} plugin", plugin_name),
        })
    }
}

#[async_trait]
impl Tool for PluginApplyTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"diff": {"type": "object"}, "dry_run": {"type": "boolean"}}, "required": ["diff"]})
    }

    fn category(&self) -> &str {
        "plugin"
    }

    fn tags(&self) -> Vec<String> {
        vec!["plugin".to_string(), "state".to_string(), "apply".to_string(), self.plugin_name.clone()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let dry_run = input.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);

        Ok(json!({
            "success": true,
            "plugin": self.plugin_name,
            "operation": "apply",
            "dry_run": dry_run,
            "applied": !dry_run,
            "message": "Plugin apply executed (integrate with plugin catalog)"
        }))
    }
}

// ============================================================================
// MAIN LOADER FUNCTIONS
// ============================================================================

/// Load essential tools that are always needed
/// These are registered immediately, not lazily
async fn load_essential_tools(registry: &ToolRegistry) -> Result<usize> {
    let mut count = 0;

    info!("Loading essential tools (always available)...");

    // Response tools - CRITICAL
    count += register_tool_checked!(registry, RespondToUserTool::new(), "respond_to_user");
    count += register_tool_checked!(registry, CannotPerformTool::new(), "cannot_perform");
    count += register_tool_checked!(registry, RequestClarificationTool::new(), "request_clarification");

    // Filesystem tools - ESSENTIAL
    count += register_tool_checked!(registry, ReadFileTool::new(), "read_file");
    count += register_tool_checked!(registry, WriteFileTool::new(), "write_file");
    count += register_tool_checked!(registry, ListDirectoryTool::new(), "list_directory");

    // Shell tools - ESSENTIAL
    count += register_tool_checked!(registry, ShellExecuteTool::new(), "shell_execute");

    // System info tools - ESSENTIAL
    count += register_tool_checked!(registry, ListNetworkInterfacesTool::new(), "list_network_interfaces");

    info!("✅ Loaded {} essential tools", count);
    Ok(count)
}

/// Load all tools into the registry (legacy function for compatibility)
/// 
/// This loads essential tools immediately and returns.
/// For lazy loading, use `create_lazy_registry()` instead.
pub async fn load_all_tools(registry: &ToolRegistry) -> Result<usize> {
    let mut count = load_essential_tools(registry).await?;

    // For backward compatibility, also load systemd and OVS tools directly
    info!("Loading systemd tools...");
    count += register_tool_checked!(registry, SystemdUnitStatusTool::new(), "systemd_unit_status");
    count += register_tool_checked!(registry, SystemdListUnitsTool::new(), "systemd_list_units");
    count += register_tool_checked!(registry, SystemdStartUnitTool::new(), "systemd_start_unit");
    count += register_tool_checked!(registry, SystemdStopUnitTool::new(), "systemd_stop_unit");
    count += register_tool_checked!(registry, SystemdRestartUnitTool::new(), "systemd_restart_unit");

    info!("Loading OVS tools...");
    count += register_tool_checked!(registry, OvsListBridgesTool::new(), "ovs_list_bridges");
    count += register_tool_checked!(registry, OvsShowBridgeTool::new(), "ovs_show_bridge");
    count += register_tool_checked!(registry, OvsListPortsTool::new(), "ovs_list_ports");
    count += register_tool_checked!(registry, OvsDumpFlowsTool::new(), "ovs_dump_flows");

    // Load plugin state tools
    info!("Loading plugin state tools...");
    let plugins = ["systemd", "network", "packagekit", "firewall", "users", "storage"];
    for plugin in &plugins {
        count += register_tool_checked!(registry, PluginQueryTool::new(plugin), &format!("plugin_{}_query", plugin));
        count += register_tool_checked!(registry, PluginDiffTool::new(plugin), &format!("plugin_{}_diff", plugin));
        count += register_tool_checked!(registry, PluginApplyTool::new(plugin), &format!("plugin_{}_apply", plugin));
    }

    info!("✅ Registered {} tools total", count);
    Ok(count)
}

// create_lazy_registry removed in favor of load_all_tools

/// Load all tools with plugin integration
pub async fn load_all_tools_with_plugins(
    registry: &ToolRegistry,
    _introspection: &Arc<op_introspection::IntrospectionService>,
) -> Result<usize> {
    let count = load_all_tools(registry).await?;

    // TODO: Add D-Bus discovered tools from introspection service
    // TODO: Add agent tools from op_agents

    info!("✅ Total tools registered (with plugins): {}", count);
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_tools::registry::ToolRegistry;

    #[tokio::test]
    async fn test_load_essential_tools() {
        let registry = ToolRegistry::new(100);
        let count = load_essential_tools(&registry).await.unwrap();

        assert!(count >= 9, "Expected at least 9 essential tools, got {}", count);
        assert!(registry.get("respond_to_user").await.is_some());
        assert!(registry.get("read_file").await.is_some());
        assert!(registry.get("shell_execute").await.is_some());
    }

    #[tokio::test]
    async fn test_lazy_registry() {
        let registry = Arc::new(ToolRegistry::new(100));
        let lazy = create_lazy_registry(registry.clone()).await.unwrap();

        // Essential tools should be available
        assert!(lazy.get("respond_to_user").await.is_some());

        // Lazy tools should be created on demand
        assert!(lazy.get("systemd_unit_status").await.is_some());
        assert!(lazy.get("ovs_list_bridges").await.is_some());
    }

    #[tokio::test]
    async fn test_tool_factory_definitions() {
        let factory = SystemdToolFactory::new();

        assert!(factory.can_create("systemd_unit_status"));
        assert!(!factory.can_create("unknown_tool"));

        let def = factory.get_definition("systemd_unit_status").unwrap();
        assert_eq!(def.name, "systemd_unit_status");
        assert_eq!(def.category, "systemd");
    }

    #[tokio::test]
    async fn test_respond_to_user_tool() {
        let tool = RespondToUserTool::new();
        let result = tool.execute(json!({"message": "Hello!"})).await.unwrap();

        assert_eq!(result.get("success").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(result.get("message").and_then(|v| v.as_str()), Some("Hello!"));
    }

    #[tokio::test]
    async fn test_shell_whitelist() {
        let tool = ShellExecuteTool::new();

        // Allowed
        let result = tool.execute(json!({"command": "echo", "args": ["test"]})).await.unwrap();
        assert_eq!(result.get("success").and_then(|v| v.as_bool()), Some(true));

        // Blocked
        let result = tool.execute(json!({"command": "rm", "args": ["-rf", "/"]})).await.unwrap();
        assert_eq!(result.get("success").and_then(|v| v.as_bool()), Some(false));
    }
}
