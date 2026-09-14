//! Shell Execution Tools

use crate::tool_registry::{Tool, ToolRegistry};
use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use std::time::Duration;

pub async fn register_all(registry: &ToolRegistry) -> Result<usize> {
    registry.register(Arc::new(ShellExecuteTool::new())).await?;
    Ok(1)
}

pub struct ShellExecuteTool {
    allowed_commands: Vec<String>,
}

impl ShellExecuteTool {
    pub fn new() -> Self {
        Self {
            allowed_commands: vec![
                "ls", "cat", "grep", "find", "head", "tail", "wc", "sort", "uniq",
                "echo", "pwd", "whoami", "date", "uname", "df", "du", "free", "uptime",
                "ps", "top", "ip", "ss", "netstat", "ping", "dig", "curl", "wget",
                "git", "docker", "kubectl", "systemctl", "journalctl",
                "cargo", "rustc", "python", "python3", "pip", "pip3",
                "node", "npm", "yarn",
            ].into_iter().map(String::from).collect()
        }
    }
}

#[async_trait]
impl Tool for ShellExecuteTool {
    fn name(&self) -> &str { "shell_execute" }
    fn description(&self) -> &str { "Execute a whitelisted shell command." }
    fn category(&self) -> &str { "shell" }
    fn tags(&self) -> Vec<String> { vec!["shell".into(), "execute".into()] }
    
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {"type": "string", "description": "Command to execute (must be whitelisted)"},
                "args": {"type": "array", "items": {"type": "string"}},
                "timeout_secs": {"type": "integer", "default": 30}
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let command = input.get("command").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing command"))?;
        
        if !self.allowed_commands.contains(&command.to_string()) {
            return Ok(json!({
                "success": false,
                "error": format!("Command '{}' not whitelisted", command)
            }));
        }
        
        let args: Vec<String> = input.get("args")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        
        let timeout = input.get("timeout_secs").and_then(|v| v.as_u64()).unwrap_or(30);
        
        let result = tokio::time::timeout(
            Duration::from_secs(timeout),
            tokio::process::Command::new(command).args(&args).output()
        ).await;
        
        match result {
            Ok(Ok(output)) => Ok(json!({
                "success": output.status.success(),
                "exit_code": output.status.code(),
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr)
            })),
            Ok(Err(e)) => Ok(json!({"success": false, "error": e.to_string()})),
            Err(_) => Ok(json!({"success": false, "error": "Command timed out"}))
        }
    }
}
