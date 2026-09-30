//! Shell Executor Agent
//!
//! Compact MCP exposes every shell command through this agent.

use async_trait::async_trait;
use simd_json::json;
use simd_json::prelude::*;
use std::path::PathBuf;

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
        let mut base = ExecutionAgent::new(
            "shell-executor",
            "Shell Executor",
            "Executes any shell command for system operations.",
            "shell",
            vec!["any shell command"],
        );
        base.security_profile.config.allow_all_commands = true;
        base.security_profile.config.allowed_commands.clear();
        base.security_profile.config.forbidden_paths.clear();
        base.security_profile.config.allowed_read_paths = vec![PathBuf::from("/")];
        base.security_profile.config.allowed_write_paths = vec![PathBuf::from("/")];
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
        vec![
            AgentCapability::RunCode {
                language: "shell".to_string(),
            },
            AgentCapability::RunCommand {
                commands: vec!["*".to_string()],
            },
            AgentCapability::ReadFiles,
        ]
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
