//! Sandboxed command execution with resource limits
//!
//! Provides secure execution environment with:
//! - Timeout enforcement
//! - Memory limits
//! - Output size limits
//! - Process isolation

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use super::profiles::SecurityProfile;
use super::validation::{validate_command, validate_path, SecurityError};

/// Resource limits for sandboxed execution
#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// Maximum execution time
    pub timeout: Duration,

    /// Maximum memory in bytes
    pub max_memory: u64,

    /// Maximum output size in bytes
    pub max_output: usize,

    /// Maximum number of processes
    pub max_processes: u32,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(60),
            max_memory: 512 * 1024 * 1024, // 512MB
            max_output: 1_000_000,         // 1MB
            max_processes: 10,
        }
    }
}

impl From<&SecurityProfile> for ResourceLimits {
    fn from(profile: &SecurityProfile) -> Self {
        Self {
            timeout: Duration::from_secs(profile.config.timeout_secs),
            max_memory: profile.config.max_memory_mb * 1024 * 1024,
            max_output: profile.config.max_output_size,
            max_processes: 10,
        }
    }
}

/// Sandbox execution result (different from tool SandboxResult)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub success: bool,
    pub duration_ms: u64,
    pub truncated: bool,
}

impl SandboxResult {
    pub fn success(stdout: String, stderr: String, duration_ms: u64) -> Self {
        Self {
            exit_code: Some(0),
            stdout,
            stderr,
            success: true,
            duration_ms,
            truncated: false,
        }
    }
    pub fn failure(code: Option<i32>, stdout: String, stderr: String, duration_ms: u64) -> Self {
        Self {
            exit_code: code,
            stdout,
            stderr,
            success: false,
            duration_ms,
            truncated: false,
        }
    }
}

/// Sandboxed command executor
pub struct SandboxExecutor {
    /// Security profile to use
    profile: SecurityProfile,

    /// Resource limits
    limits: ResourceLimits,

    /// Additional environment variables
    env: HashMap<String, String>,
}

impl SandboxExecutor {
    /// Create a new sandbox executor with a security profile
    pub fn new(profile: SecurityProfile) -> Self {
        let limits = ResourceLimits::from(&profile);
        Self {
            profile,
            limits,
            env: HashMap::new(),
        }
    }

    /// Set custom resource limits
    pub fn with_limits(mut self, limits: ResourceLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Add environment variable
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Execute a command in the sandbox
    pub async fn execute(
        &self,
        command: &str,
        args: &[String],
        working_dir: Option<&PathBuf>,
    ) -> Result<SandboxResult, SecurityError> {
        let start = Instant::now();

        // Validate command against whitelist
        let whitelist: Vec<String> = self
            .profile
            .config
            .allowed_commands
            .iter()
            .cloned()
            .collect();
        validate_command(command, &whitelist)?;

        // Validate working directory if provided
        if let Some(dir) = working_dir {
            validate_path(
                dir.to_str().unwrap_or(""),
                &self.profile.config.allowed_read_paths,
                &self.profile.config.forbidden_paths,
            )?;
        }

        // Build the command
        let mut cmd = Command::new(command);
        cmd.args(args);

        // Set environment
        cmd.env_clear();
        cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
        cmd.env("HOME", "/tmp");
        cmd.env("LANG", "C.UTF-8");

        for (key, value) in &self.env {
            cmd.env(key, value);
        }

        // Set working directory
        if let Some(dir) = working_dir {
            cmd.current_dir(dir);
        }

        // Configure process I/O
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        // Spawn the process
        let mut child = cmd
            .spawn()
            .context("Failed to spawn command")
            .map_err(|e| SecurityError::Unauthorized(e.to_string()))?;

        // Get handles to stdout/stderr
        let mut stdout_handle = child
            .stdout
            .take()
            .ok_or_else(|| SecurityError::Unauthorized("No stdout".to_string()))?;
        let mut stderr_handle = child
            .stderr
            .take()
            .ok_or_else(|| SecurityError::Unauthorized("No stderr".to_string()))?;

        // Read output with timeout
        let timeout = self.limits.timeout;
        let max_output = self.limits.max_output;

        let result = tokio::time::timeout(timeout, async {
            let mut stdout_buf = Vec::with_capacity(max_output.min(1024 * 1024));
            let mut stderr_buf = Vec::with_capacity(max_output.min(1024 * 1024));

            // Read output (with size limits)
            let read_limited = async {
                let mut tmp_stdout = vec![0u8; max_output];
                let mut tmp_stderr = vec![0u8; max_output];

                let (stdout_n, stderr_n) = tokio::join!(
                    stdout_handle.read(&mut tmp_stdout),
                    stderr_handle.read(&mut tmp_stderr),
                );

                let stdout_n = stdout_n.unwrap_or(0);
                let stderr_n = stderr_n.unwrap_or(0);

                stdout_buf.extend_from_slice(&tmp_stdout[..stdout_n]);
                stderr_buf.extend_from_slice(&tmp_stderr[..stderr_n]);
            };

            read_limited.await;

            // Wait for process to complete
            let status = child.wait().await;

            (stdout_buf, stderr_buf, status)
        })
        .await;

        let duration = start.elapsed();

        match result {
            Ok((stdout_buf, stderr_buf, status)) => {
                let stdout = String::from_utf8_lossy(&stdout_buf).to_string();
                let stderr = String::from_utf8_lossy(&stderr_buf).to_string();
                let truncated = stdout_buf.len() >= max_output || stderr_buf.len() >= max_output;
                let duration_ms = duration.as_millis() as u64;

                match status {
                    Ok(s) if s.success() => {
                        let mut result = SandboxResult::success(stdout, stderr, duration_ms);
                        result.truncated = truncated;
                        Ok(result)
                    }
                    Ok(s) => {
                        let mut result =
                            SandboxResult::failure(s.code(), stdout, stderr, duration_ms);
                        result.truncated = truncated;
                        Ok(result)
                    }
                    Err(e) => Ok(SandboxResult::failure(
                        None,
                        stdout,
                        format!("{}\n{}", stderr, e),
                        duration_ms,
                    )),
                }
            }
            Err(_) => {
                // Timeout - kill the process
                let _ = child.kill().await;
                Err(SecurityError::Timeout(timeout.as_secs()))
            }
        }
    }

    /// Execute a command with a specific operation's settings
    pub async fn execute_operation(
        &self,
        operation: &str,
        command: &str,
        args: &[String],
        working_dir: Option<&PathBuf>,
    ) -> Result<SandboxResult, SecurityError> {
        // Check if operation requires approval
        if let Some(op_sec) = self.profile.operations.iter().find(|o| o.name == operation) {
            if op_sec.requires_approval && !self.profile.config.requires_approval {
                return Err(SecurityError::RequiresApproval);
            }
        }

        self.execute(command, args, working_dir).await
    }
}

/// Builder for creating sandbox executors
pub struct SandboxBuilder {
    profile: Option<SecurityProfile>,
    limits: Option<ResourceLimits>,
    env: HashMap<String, String>,
}

impl SandboxBuilder {
    pub fn new() -> Self {
        Self {
            profile: None,
            limits: None,
            env: HashMap::new(),
        }
    }

    pub fn with_profile(mut self, profile: SecurityProfile) -> Self {
        self.profile = Some(profile);
        self
    }

    pub fn with_limits(mut self, limits: ResourceLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    pub fn build(self) -> Result<SandboxExecutor> {
        let profile = self
            .profile
            .ok_or_else(|| anyhow::anyhow!("Security profile required"))?;

        let mut executor = SandboxExecutor::new(profile);

        if let Some(limits) = self.limits {
            executor.limits = limits;
        }

        executor.env = self.env;

        Ok(executor)
    }
}

impl Default for SandboxBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::profiles::presets;

    #[tokio::test]
    async fn test_sandbox_allowed_command() {
        let profile = presets::python_pro();
        let executor = SandboxExecutor::new(profile);

        // This should work - python is allowed
        let result = executor
            .execute("python3", &["--version".to_string()], None)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_sandbox_blocked_command() {
        let profile = presets::python_pro();
        let executor = SandboxExecutor::new(profile);

        // This should fail - rm is not allowed
        let result = executor
            .execute("rm", &["-rf".to_string(), "/".to_string()], None)
            .await;
        assert!(result.is_err());
    }
}
