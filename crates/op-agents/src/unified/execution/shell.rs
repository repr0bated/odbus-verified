//! Shell Executor Agent
//!
//! Executes whitelisted shell commands.

use async_trait::async_trait;
use simd_json::json;
use simd_json::prelude::*;

use super::super::agent_trait::{
    AgentCapability, AgentCategory, AgentRequest, AgentResponse, UnifiedAgent,
};
use super::base::ExecutionAgent;
use crate::security::SecurityProfile;

pub struct ShellExecutor {
    base: ExecutionAgent,
}

impl ShellExecutor {
    pub fn new() -> Self {
        let base = ExecutionAgent::new(
            "shell-executor",
            "Shell Executor",
            "Executes whitelisted shell commands for system operations.",
            "shell",
            vec![
                // File operations (read-only)
                "ls", "cat", "head", "tail", "find", "grep", "wc", "file", "stat",
                // System info
                "uname", "hostname", "uptime", "df", "free", "ps", "top",
                // Network info (read-only)
                "ip", "ss", "netstat", "ping", "dig", "nslookup",
                // Git (read operations)
                "git", // Text processing
                "sort", "uniq", "cut", "awk", "sed", "jq",
            ],
        );
        Self { base }
    }
}

impl Default for ShellExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UnifiedAgent for ShellExecutor {
    fn id(&self) -> &str {
        self.base.id()
    }
    fn name(&self) -> &str {
        self.base.name()
    }
    fn description(&self) -> &str {
        self.base.description()
    }
    fn category(&self) -> AgentCategory {
        AgentCategory::Execution
    }
    fn capabilities(&self) -> Vec<AgentCapability> {
        self.base.capabilities()
    }
    fn system_prompt(&self) -> &str {
        self.base.system_prompt()
    }
    fn knowledge_base(&self) -> Option<&str> {
        self.base.knowledge_base()
    }
    fn security_profile(&self) -> Option<&SecurityProfile> {
        self.base.security_profile()
    }
    fn operations(&self) -> Vec<&str> {
        vec!["exec"]
    }

    async fn execute(&self, request: AgentRequest) -> AgentResponse {
        if request.operation != "exec" {
            return AgentResponse::failure(format!("Unknown operation: {}", request.operation));
        }

        let command = match request.args.get("command").and_then(|v| v.as_str()) {
            Some(cmd) => cmd,
            None => return AgentResponse::failure("No command specified"),
        };

        // Parse command into program and args
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return AgentResponse::failure("Empty command");
        }

        let program = parts[0];
        let args: Vec<&str> = parts[1..].to_vec();

        let working_dir = request.args.get("cwd").and_then(|v| v.as_str());

        let timeout = request
            .args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(30);

        match self
            .base
            .execute_command(program, &args, working_dir, timeout)
            .await
        {
            Ok((stdout, stderr, code)) => AgentResponse::success(
                json!({
                    "stdout": stdout,
                    "stderr": stderr,
                    "exit_code": code,
                    "command": command
                }),
                if code == 0 {
                    "Command completed"
                } else {
                    "Command failed"
                },
            ),
            Err(e) => AgentResponse::failure(e),
        }
    }
}
