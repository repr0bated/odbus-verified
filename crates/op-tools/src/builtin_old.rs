//! Built-in tools for common system operations

use async_trait::async_trait;
use simd_json::json;
use tracing::debug;

use op_core::{ToolDefinition, ToolRequest, ToolResult};
use crate::Tool;

/// Echo tool for testing
pub struct EchoTool;

#[async_trait]
impl Tool for EchoTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo".to_string(),
            description: "Echo back the input message (for testing)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "Message to echo back"
                    }
                },
                "required": ["message"]
            }),
            category: Some("utility".to_string()),
            tags: vec!["test".to_string(), "utility".to_string()],
        }
    }
    
    async fn execute(&self, request: ToolRequest) -> ToolResult {
        let start = std::time::Instant::now();
        
        let message = request.arguments.get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        debug!("Echo: {}", message);
        
        ToolResult::success(
            &request.id,
            json!({ "echoed": message }),
            start.elapsed().as_millis() as u64,
        )
    }
    
    fn name(&self) -> &str {
        "echo"
    }
}

/// System info tool
pub struct SystemInfoTool;

#[async_trait]
impl Tool for SystemInfoTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "system_info".to_string(),
            description: "Get basic system information".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
            category: Some("system".to_string()),
            tags: vec!["system".to_string(), "info".to_string()],
        }
    }
    
    async fn execute(&self, request: ToolRequest) -> ToolResult {
        let start = std::time::Instant::now();
        
        let hostname = gethostname::gethostname()
            .to_string_lossy()
            .to_string();
        
        let info = json!({
            "hostname": hostname,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "family": std::env::consts::FAMILY,
        });
        
        ToolResult::success(
            &request.id,
            info,
            start.elapsed().as_millis() as u64,
        )
    }
    
    fn name(&self) -> &str {
        "system_info"
    }
}

/// Shell command tool (restricted)
pub struct ShellTool {
    allowed_commands: Vec<String>,
}

impl ShellTool {
    pub fn new(allowed_commands: Vec<String>) -> Self {
        Self { allowed_commands }
    }
    
    pub fn with_defaults() -> Self {
        Self::new(vec![
            "ls".to_string(),
            "cat".to_string(),
            "echo".to_string(),
            "date".to_string(),
            "uptime".to_string(),
            "hostname".to_string(),
            "whoami".to_string(),
            "uname".to_string(),
            "pwd".to_string(),
            "df".to_string(),
            "free".to_string(),
        ])
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "shell".to_string(),
            description: "Execute allowed shell commands".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Command to execute"
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Command arguments"
                    }
                },
                "required": ["command"]
            }),
            category: Some("system".to_string()),
            tags: vec!["shell".to_string(), "command".to_string()],
        }
    }
    
    fn validate(&self, args: &simd_json::OwnedValue) -> Result<(), String> {
        let command = args.get("command")
            .and_then(|v| v.as_str())
            .ok_or("Missing 'command' argument")?;
        
        // Extract base command (before any pipes or other shell features)
        let base_cmd = command.split_whitespace()
            .next()
            .unwrap_or(command);
        
        if !self.allowed_commands.iter().any(|c| c == base_cmd) {
            return Err(format!(
                "Command '{}' is not allowed. Allowed: {:?}",
                base_cmd, self.allowed_commands
            ));
        }
        
        Ok(())
    }
    
    async fn execute(&self, request: ToolRequest) -> ToolResult {
        let start = std::time::Instant::now();
        
        let command = match request.arguments.get("command").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => {
                return ToolResult::error(
                    &request.id,
                    "Missing 'command' argument",
                    start.elapsed().as_millis() as u64,
                );
            }
        };
        
        let args: Vec<&str> = request.arguments.get("args")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        
        debug!("Executing shell command: {} {:?}", command, args);
        
        match tokio::process::Command::new("sh")
            .arg("-c")
            .arg(format!("{} {}", command, args.join(" ")))
            .output()
            .await
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);
                
                if output.status.success() {
                    ToolResult::success(
                        &request.id,
                        json!({
                            "stdout": stdout.trim(),
                            "stderr": stderr.trim(),
                            "exit_code": output.status.code()
                        }),
                        start.elapsed().as_millis() as u64,
                    )
                } else {
                    ToolResult::error(
                        &request.id,
                        format!("Command failed: {}", stderr.trim()),
                        start.elapsed().as_millis() as u64,
                    )
                }
            }
            Err(e) => {
                ToolResult::error(
                    &request.id,
                    format!("Failed to execute command: {}", e),
                    start.elapsed().as_millis() as u64,
                )
            }
        }
    }
    
    fn name(&self) -> &str {
        "shell"
    }
}

/// File read tool
pub struct FileReadTool;

#[async_trait]
impl Tool for FileReadTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_read".to_string(),
            description: "Read contents of a file".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to read"
                    },
                    "max_bytes": {
                        "type": "integer",
                        "description": "Maximum bytes to read (default: 1MB)"
                    }
                },
                "required": ["path"]
            }),
            category: Some("filesystem".to_string()),
            tags: vec!["file".to_string(), "read".to_string()],
        }
    }
    
    async fn execute(&self, request: ToolRequest) -> ToolResult {
        let start = std::time::Instant::now();
        
        let path = match request.arguments.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return ToolResult::error(
                    &request.id,
                    "Missing 'path' argument",
                    start.elapsed().as_millis() as u64,
                );
            }
        };
        
        let max_bytes = request.arguments.get("max_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(1_048_576) as usize; // 1MB default
        
        debug!("Reading file: {} (max {} bytes)", path, max_bytes);
        
        match tokio::fs::read(path).await {
            Ok(contents) => {
                let truncated = contents.len() > max_bytes;
                let contents = if truncated {
                    &contents[..max_bytes]
                } else {
                    &contents
                };
                
                match String::from_utf8(contents.to_vec()) {
                    Ok(text) => {
                        ToolResult::success(
                            &request.id,
                            json!({
                                "content": text,
                                "size": contents.len(),
                                "truncated": truncated
                            }),
                            start.elapsed().as_millis() as u64,
                        )
                    }
                    Err(_) => {
                        // Binary file - return base64
                        ToolResult::success(
                            &request.id,
                            json!({
                                "content_base64": base64::encode(contents),
                                "size": contents.len(),
                                "truncated": truncated,
                                "binary": true
                            }),
                            start.elapsed().as_millis() as u64,
                        )
                    }
                }
            }
            Err(e) => {
                ToolResult::error(
                    &request.id,
                    format!("Failed to read file: {}", e),
                    start.elapsed().as_millis() as u64,
                )
            }
        }
    }
    
    fn name(&self) -> &str {
        "file_read"
    }
}

// Simple base64 encoding (to avoid additional dependency)
mod base64 {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    
    pub fn encode(data: &[u8]) -> String {
        let mut result = String::new();
        
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as usize;
            let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
            let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
            
            result.push(ALPHABET[b0 >> 2] as char);
            result.push(ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)] as char);
            
            if chunk.len() > 1 {
                result.push(ALPHABET[((b1 & 0x0f) << 2) | (b2 >> 6)] as char);
            } else {
                result.push('=');
            }
            
            if chunk.len() > 2 {
                result.push(ALPHABET[b2 & 0x3f] as char);
            } else {
                result.push('=');
            }
        }
        
        result
    }
}

/// Register all built-in tools with a registry
pub async fn register_builtins(registry: &crate::ToolRegistry) -> Result<(), String> {
    registry.register(EchoTool).await?;
    registry.register(SystemInfoTool).await?;
    registry.register(ShellTool::with_defaults()).await?;
    registry.register(FileReadTool).await?;
    Ok(())
}
