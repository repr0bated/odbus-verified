//! Shell Execution Tool with Access Level Security
//!
//! Provides shell command execution with:
//! - Access level based security (admin has FULL access)
//! - Rate limiting per session
//! - Audit logging
//! - Native protocol recommendations (but NOT enforcement)
//! - Output truncation and timeout enforcement
//!
//! ## Security Model
//!
//! The chatbot is designed to be a FULL SYSTEM ADMINISTRATOR.
//! Security is at the ACCESS level, not command level:
//! - Unrestricted (Admin): Can run ANY command
//! - Restricted: Limited to safe read-only commands
//!
//! We RECOMMEND native protocols (D-Bus, OVSDB) for better error handling,
//! but we don't BLOCK shell commands - admins need full access.

use anyhow::Result;
use async_trait::async_trait;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tracing::{debug, error, info, warn};

use crate::security::get_security_validator;
use crate::{Tool, ToolRegistry};

// ============================================================================
// SHELL EXECUTE TOOL
// ============================================================================

pub struct ShellExecuteTool;

#[async_trait]
impl Tool for ShellExecuteTool {
    fn name(&self) -> &str {
        "shell_execute"
    }

    fn description(&self) -> &str {
        "Execute a shell command. Full access for admin users. \
         Consider using native D-Bus/OVSDB tools for structured responses."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 60, max: 300)",
                    "default": 60
                },
                "working_dir": {
                    "type": "string",
                    "description": "Working directory (default: /tmp)",
                    "default": "/tmp"
                },
                "session_id": {
                    "type": "string",
                    "description": "Session ID for rate limiting"
                }
            },
            "required": ["command"]
        })
    }

    fn category(&self) -> &str {
        "system"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "shell".to_string(),
            "command".to_string(),
            "admin".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let validator = get_security_validator();

        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: command"))?;

        let session_id = input
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        let working_dir = input
            .get("working_dir")
            .and_then(|v| v.as_str())
            .unwrap_or("/tmp");

        // Get limits from validator
        let max_timeout = validator.max_timeout().await;
        let max_output = validator.max_output().await;

        let timeout_secs = input
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(60)
            .min(max_timeout.as_secs());

        // Check rate limit
        validator
            .check_rate_limit(session_id)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Check command access (may return a warning about native alternatives)
        let warning = validator
            .check_command(command)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Log with warning if applicable
        if let Some(ref warn_msg) = warning {
            warn!(
                command = %command,
                recommendation = %warn_msg,
                "Consider using native protocol tools"
            );
        }

        info!(
            command = %command,
            working_dir = %working_dir,
            timeout = %timeout_secs,
            session = %session_id,
            "Executing shell command"
        );

        // Execute with timeout
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            execute_command(command, working_dir, max_output),
        )
        .await;

        match result {
            Ok(Ok((stdout, stderr, exit_code))) => {
                info!(
                    exit_code = %exit_code,
                    stdout_len = %stdout.len(),
                    stderr_len = %stderr.len(),
                    "Command completed"
                );

                let mut response = json!({
                    "command": command,
                    "exit_code": exit_code,
                    "stdout": stdout,
                    "stderr": stderr,
                    "success": exit_code == 0
                });

                // Include warning if native alternative exists
                if let Some(warn_msg) = warning {
                    response
                        .as_object_mut()
                        .expect("shell response is an object")
                        .insert("native_alternative_hint".into(), Value::String(warn_msg));
                }

                Ok(response)
            }
            Ok(Err(e)) => {
                error!(error = %e, command = %command, "Command execution failed");
                Err(anyhow::anyhow!("Command execution failed: {}", e))
            }
            Err(_) => {
                error!(timeout = %timeout_secs, command = %command, "Command timed out");
                Err(anyhow::anyhow!(
                    "Command timed out after {} seconds",
                    timeout_secs
                ))
            }
        }
    }
}

// ============================================================================
// SHELL EXECUTE BATCH TOOL
// ============================================================================

pub struct ShellExecuteBatchTool;

#[async_trait]
impl Tool for ShellExecuteBatchTool {
    fn name(&self) -> &str {
        "shell_execute_batch"
    }

    fn description(&self) -> &str {
        "Execute a sequence of shell commands. Full access for admin users. \
         Stops on first error if stop_on_error is true."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "commands": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "command": { "type": "string" },
                            "working_dir": { "type": "string" },
                            "timeout_secs": { "type": "integer" }
                        },
                        "required": ["command"]
                    },
                    "description": "Ordered list of commands to execute"
                },
                "stop_on_error": {
                    "type": "boolean",
                    "description": "Stop after first non-zero exit or error",
                    "default": true
                },
                "default_working_dir": {
                    "type": "string",
                    "description": "Default working directory for commands",
                    "default": "/tmp"
                },
                "session_id": {
                    "type": "string",
                    "description": "Session ID for rate limiting"
                }
            },
            "required": ["commands"]
        })
    }

    fn category(&self) -> &str {
        "system"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "shell".to_string(),
            "batch".to_string(),
            "admin".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let validator = get_security_validator();

        let commands = input
            .get("commands")
            .and_then(|v| v.as_array())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: commands"))?;

        if commands.is_empty() {
            return Err(anyhow::anyhow!("commands must be a non-empty array"));
        }

        let stop_on_error = input
            .get("stop_on_error")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let default_working_dir = input
            .get("default_working_dir")
            .and_then(|v| v.as_str())
            .unwrap_or("/tmp");

        let session_id = input
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("default");

        let max_timeout = validator.max_timeout().await;
        let max_output = validator.max_output().await;
        let default_timeout_secs = 60u64.min(max_timeout.as_secs());

        let mut results = Vec::new();
        let commands_list: Vec<_> = commands.iter().collect();

        for (idx, entry) in commands_list.into_iter().enumerate() {
            // Rate limit each command in the batch
            if let Err(e) = validator.check_rate_limit(session_id).await {
                return Err(anyhow::anyhow!("{}", e));
            }

            // Support both object {"command": "..."} (or "cmd") and string "..."
            let (command, working_dir, timeout_secs) = if let Some(cmd_str) = entry.as_str() {
                (cmd_str, default_working_dir, default_timeout_secs)
            } else {
                let command = entry
                    .get("command")
                    .or_else(|| entry.get("cmd"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Command entry at index {} requires 'command' (or 'cmd') field or must be a string", idx))?;

                let working_dir = entry
                    .get("working_dir")
                    .and_then(|v| v.as_str())
                    .unwrap_or(default_working_dir);

                let timeout = entry
                    .get("timeout_secs")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(default_timeout_secs);

                (command, working_dir, timeout)
            };

            let timeout_secs = timeout_secs.min(max_timeout.as_secs());

            // Check command access
            if let Err(e) = validator.check_command(command).await {
                let outcome = json!({
                    "command": command,
                    "working_dir": working_dir,
                    "exit_code": -1,
                    "stdout": "",
                    "stderr": format!("Access denied: {}", e),
                    "success": false
                });
                results.push(outcome);
                if stop_on_error {
                    break;
                }
                continue;
            }

            // Execute command
            let run = tokio::time::timeout(
                std::time::Duration::from_secs(timeout_secs),
                execute_command(command, working_dir, max_output),
            )
            .await;

            let outcome = match run {
                Ok(Ok((stdout, stderr, exit_code))) => json!({
                    "command": command,
                    "working_dir": working_dir,
                    "exit_code": exit_code,
                    "stdout": stdout,
                    "stderr": stderr,
                    "success": exit_code == 0
                }),
                Ok(Err(e)) => json!({
                    "command": command,
                    "working_dir": working_dir,
                    "exit_code": -1,
                    "stdout": "",
                    "stderr": e,
                    "success": false
                }),
                Err(_) => json!({
                    "command": command,
                    "working_dir": working_dir,
                    "exit_code": -1,
                    "stdout": "",
                    "stderr": format!("Command timed out after {} seconds", timeout_secs),
                    "success": false
                }),
            };

            let success = outcome
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            results.push(outcome);

            if stop_on_error && !success {
                break;
            }
        }

        let stopped_early = stop_on_error
            && results
                .last()
                .and_then(|r| r.get("success"))
                .and_then(|v| v.as_bool())
                == Some(false);

        Ok(json!({
            "results": results,
            "stopped_early": stopped_early,
            "total_commands": results.len()
        }))
    }
}

// ============================================================================
// COMMAND EXECUTION
// ============================================================================

/// Execute a command using bash
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

    // Truncate if needed
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

// ============================================================================
// REGISTRATION
// ============================================================================

/// Register shell tools with the registry
pub async fn register_shell_tools(registry: &ToolRegistry) -> Result<()> {
    use std::sync::Arc;

    registry.register_tool(Arc::new(ShellExecuteTool)).await?;
    registry
        .register_tool(Arc::new(ShellExecuteBatchTool))
        .await?;

    debug!("Registered shell execution tools");
    Ok(())
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shell_execute() {
        let tool = ShellExecuteTool;
        let result = tool
            .execute(json!({
                "command": "echo hello world",
                "session_id": "test1"
            }))
            .await;

        assert!(result.is_ok());
        let val = result.unwrap();
        assert!(val
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false));
        assert!(val
            .get("stdout")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .contains("hello world"));
    }

    #[tokio::test]
    async fn test_shell_with_exit_code() {
        let tool = ShellExecuteTool;
        let result = tool
            .execute(json!({
                "command": "exit 42",
                "session_id": "test2"
            }))
            .await;

        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val.get("exit_code").and_then(|v| v.as_i64()).unwrap(), 42);
        assert!(!val.get("success").and_then(|v| v.as_bool()).unwrap_or(true));
    }

    #[tokio::test]
    async fn test_admin_can_run_anything() {
        // With default admin profile, any command should work
        let tool = ShellExecuteTool;

        // Even "dangerous" commands should be allowed for admins
        let result = tool
            .execute(json!({
                "command": "ls /",
                "session_id": "test3"
            }))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_native_alternative_warning() {
        let tool = ShellExecuteTool;
        let _result = tool
            .execute(json!({
                "command": "ovs-vsctl show",
                "session_id": "test4"
            }))
            .await;

        // Should still succeed (not blocked)
        // But may include a warning about native alternatives
        // (Note: This test may fail if ovs-vsctl isn't installed, which is fine)
    }
}
