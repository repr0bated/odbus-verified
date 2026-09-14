//! Shell Command Execution Tool
//!
//! Allows the LLM to run bash commands when no specific tool exists.
//! This is the "escape hatch" for operations not covered by native tools.

use async_trait::async_trait;
use simd_json::json;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tracing::{error, info, warn};

use crate::security::get_security_validator;
use crate::Tool;
use op_core::{ToolDefinition, ToolRequest, ToolResult};

/// Tool for executing shell commands
pub struct ShellExecuteTool;

#[async_trait]
impl Tool for ShellExecuteTool {
    fn name(&self) -> &str {
        "shell_execute"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "shell_execute".to_string(),
            description: "Execute a shell command and return the output. Use this when no specific tool exists for the task. Commands run as the service user (usually root). Be careful with destructive commands.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute (e.g., 'ls -la /tmp' or 'ip addr show')"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Timeout in seconds (default: 30, max: 300)",
                        "default": 30
                    },
                    "working_dir": {
                        "type": "string",
                        "description": "Working directory for the command (default: /tmp)"
                    }
                },
                "required": ["command"]
            }),
            category: Some("system".to_string()),
            tags: vec!["shell".to_string(), "execute".to_string(), "bash".to_string(), "command".to_string()],
        }
    }

    async fn execute(&self, request: ToolRequest) -> ToolResult {
        let start = std::time::Instant::now();

        let command = match request.arguments.get("command").and_then(|v| v.as_str()) {
            Some(cmd) => cmd,
            None => {
                return ToolResult::error(
                    request.id,
                    "Missing required 'command' argument",
                    start.elapsed().as_millis() as u64,
                );
            }
        };

        let timeout_secs = request
            .arguments
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(30)
            .min(300) as u64; // Max 5 minutes

        let working_dir = request
            .arguments
            .get("working_dir")
            .and_then(|v| v.as_str())
            .unwrap_or("/tmp");

        // Security validation
        let validator = get_security_validator();
        let max_timeout = validator.max_timeout().await;
        let max_output = validator.max_output().await;
        let timeout_secs = timeout_secs.min(max_timeout.as_secs());

        // Validate working directory exists and is not a path traversal
        let working_dir_canonical = std::fs::canonicalize(working_dir).unwrap_or_else(|_| std::path::PathBuf::from(working_dir));
        if !working_dir_canonical.exists() {
            return ToolResult::error(
                request.id,
                format!("Working directory does not exist: {}", working_dir),
                start.elapsed().as_millis() as u64,
            );
        }

        // Check command access (returns warning for native alternatives, error for restricted)
        match validator.check_command(command).await {
            Ok(Some(warning)) => {
                warn!(
                    command = %command,
                    recommendation = %warning,
                    "Consider using native protocol tools"
                );
            }
            Ok(None) => {}
            Err(e) => {
                return ToolResult::error(
                    request.id,
                    format!("Security validation failed: {}", e),
                    start.elapsed().as_millis() as u64,
                );
            }
        }

        info!(
            "Executing shell command: {} (timeout: {}s, cwd: {})",
            command, timeout_secs, working_dir
        );

        // Execute the command
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            execute_command(command, working_dir, max_output),
        )
        .await;

        let exec_time = start.elapsed().as_millis() as u64;

        match result {
            Ok(Ok((stdout, stderr, exit_code))) => {
                info!("Command completed with exit code: {}", exit_code);

                ToolResult::success(
                    request.id,
                    json!({
                        "command": command,
                        "exit_code": exit_code,
                        "stdout": stdout,
                        "stderr": stderr,
                        "success": exit_code == 0,
                        "execution_time_ms": exec_time
                    }),
                    exec_time,
                )
            }
            Ok(Err(e)) => {
                error!("Command execution failed: {}", e);
                ToolResult::error(
                    request.id,
                    format!("Command execution failed: {}", e),
                    exec_time,
                )
            }
            Err(_) => {
                error!("Command timed out after {}s", timeout_secs);
                ToolResult::error(
                    request.id,
                    format!("Command timed out after {} seconds", timeout_secs),
                    exec_time,
                )
            }
        }
    }
}

/// Execute a command and capture output
async fn execute_command(
    command: &str,
    working_dir: &str,
    max_output: usize,
) -> Result<(String, String, i32), String> {
    let mut child = Command::new("bash")
        .arg("-c")
        .arg(command)
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to spawn command: {}", e))?;

    let mut stdout = String::new();
    let mut stderr = String::new();

    if let Some(mut stdout_pipe) = child.stdout.take() {
        stdout_pipe
            .read_to_string(&mut stdout)
            .await
            .map_err(|e| format!("Failed to read stdout: {}", e))?;
    }

    if let Some(mut stderr_pipe) = child.stderr.take() {
        stderr_pipe
            .read_to_string(&mut stderr)
            .await
            .map_err(|e| format!("Failed to read stderr: {}", e))?;
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed to wait for command: {}", e))?;

    let exit_code = status.code().unwrap_or(-1);

    // Truncate very long output
    if stdout.len() > max_output {
        stdout.truncate(max_output);
        stdout.push_str("\n... (output truncated)");
    }
    if stderr.len() > max_output {
        stderr.truncate(max_output);
        stderr.push_str("\n... (output truncated)");
    }

    Ok((stdout, stderr, exit_code))
}

/// Tool for reading file contents
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description:
                "Read the contents of a file. Useful for checking configuration files, logs, etc."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path to the file to read"
                    },
                    "max_lines": {
                        "type": "integer",
                        "description": "Maximum number of lines to read (default: 1000)"
                    },
                    "tail": {
                        "type": "boolean",
                        "description": "If true, read from the end of the file (like tail)"
                    }
                },
                "required": ["path"]
            }),
            category: Some("filesystem".to_string()),
            tags: vec!["file".to_string(), "read".to_string(), "cat".to_string()],
        }
    }

    async fn execute(&self, request: ToolRequest) -> ToolResult {
        let start = std::time::Instant::now();

        let path = match request.arguments.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return ToolResult::error(
                    request.id,
                    "Missing required 'path' argument",
                    start.elapsed().as_millis() as u64,
                );
            }
        };

        let max_lines = request
            .arguments
            .get("max_lines")
            .and_then(|v| v.as_u64())
            .unwrap_or(1000) as usize;

        let tail = request
            .arguments
            .get("tail")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        match tokio::fs::read_to_string(path).await {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let total_lines = lines.len();

                let selected_lines: Vec<&str> = if tail {
                    lines
                        .into_iter()
                        .rev()
                        .take(max_lines)
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect()
                } else {
                    lines.into_iter().take(max_lines).collect()
                };

                let truncated = total_lines > max_lines;
                let output = selected_lines.join("\n");

                ToolResult::success(
                    request.id,
                    json!({
                        "path": path,
                        "content": output,
                        "lines_returned": selected_lines.len(),
                        "total_lines": total_lines,
                        "truncated": truncated
                    }),
                    start.elapsed().as_millis() as u64,
                )
            }
            Err(e) => ToolResult::error(
                request.id,
                format!("Failed to read file '{}': {}", path, e),
                start.elapsed().as_millis() as u64,
            ),
        }
    }
}

/// Tool for writing file contents
pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write_file".to_string(),
            description: "Write content to a file. Creates the file if it doesn't exist, overwrites if it does. Use with caution!".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Absolute path to the file to write"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    },
                    "append": {
                        "type": "boolean",
                        "description": "If true, append to file instead of overwriting (default: false)"
                    }
                },
                "required": ["path", "content"]
            }),
            category: Some("filesystem".to_string()),
            tags: vec!["file".to_string(), "write".to_string()],
        }
    }

    async fn execute(&self, request: ToolRequest) -> ToolResult {
        let start = std::time::Instant::now();

        let path = match request.arguments.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => {
                return ToolResult::error(
                    request.id,
                    "Missing required 'path' argument",
                    start.elapsed().as_millis() as u64,
                );
            }
        };

        let content = match request.arguments.get("content").and_then(|v| v.as_str()) {
            Some(c) => c,
            None => {
                return ToolResult::error(
                    request.id,
                    "Missing required 'content' argument",
                    start.elapsed().as_millis() as u64,
                );
            }
        };

        let append = request
            .arguments
            .get("append")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let result = if append {
            use tokio::io::AsyncWriteExt;
            let mut file = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .await;

            match file {
                Ok(mut f) => f.write_all(content.as_bytes()).await,
                Err(e) => Err(e),
            }
        } else {
            tokio::fs::write(path, content).await
        };

        match result {
            Ok(()) => ToolResult::success(
                request.id,
                json!({
                    "path": path,
                    "bytes_written": content.len(),
                    "append": append,
                    "success": true
                }),
                start.elapsed().as_millis() as u64,
            ),
            Err(e) => ToolResult::error(
                request.id,
                format!("Failed to write file '{}': {}", path, e),
                start.elapsed().as_millis() as u64,
            ),
        }
    }
}

/// Create shell and file tools
pub fn create_shell_tools() -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(ShellExecuteTool),
        Box::new(ReadFileTool),
        Box::new(WriteFileTool),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shell_execute() {
        let tool = ShellExecuteTool;
        let request = ToolRequest {
            id: "test-1".to_string(),
            tool_name: "shell_execute".to_string(),
            arguments: json!({
                "command": "echo hello world"
            }),
            timeout_ms: None,
        };

        let result = tool.execute(request).await;
        assert!(result.success);
        assert!(result
            .content
            .get("stdout")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("hello world"));
    }

    #[tokio::test]
    async fn test_shell_with_exit_code() {
        let tool = ShellExecuteTool;
        let request = ToolRequest {
            id: "test-2".to_string(),
            tool_name: "shell_execute".to_string(),
            arguments: json!({
                "command": "exit 42"
            }),
            timeout_ms: None,
        };

        let result = tool.execute(request).await;
        assert!(result.success); // Tool succeeded even though command had non-zero exit
        assert_eq!(
            result.content.get("exit_code").unwrap().as_i64().unwrap(),
            42
        );
    }
}
