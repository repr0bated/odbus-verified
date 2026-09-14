//! Base Execution Agent Implementation

use async_trait::async_trait;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use super::super::agent_trait::{
    AgentCapability, AgentCategory, AgentRequest, AgentResponse, UnifiedAgent,
};
use super::super::prompts::templates::EXECUTION_AGENT;
use crate::security::SecurityProfile;

/// Base implementation for execution agents
pub struct ExecutionAgent {
    pub id: String,
    pub name: String,
    pub description: String,
    pub language: String,
    pub system_prompt: String,
    pub knowledge: String,
    pub security_profile: SecurityProfile,
    pub operations: Vec<String>,
}

impl ExecutionAgent {
    /// Create a new execution agent
    pub fn new(
        id: &str,
        name: &str,
        description: &str,
        language: &str,
        allowed_commands: Vec<&str>,
    ) -> Self {
        let security_profile = SecurityProfile::code_execution(id, allowed_commands.clone());

        let system_prompt = EXECUTION_AGENT
            .replace("{agent_name}", name)
            .replace("{language}", language)
            .replace("{allowed_commands}", &allowed_commands.join(", "))
            .replace("{file_access}", "read: /home, /tmp; write: /tmp");

        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            language: language.to_string(),
            system_prompt,
            knowledge: String::new(),
            security_profile,
            operations: vec![
                "run".to_string(),
                "check".to_string(),
                "format".to_string(),
                "lint".to_string(),
                "test".to_string(),
            ],
        }
    }

    /// Execute a command with sandboxing
    pub async fn execute_command(
        &self,
        command: &str,
        args: &[&str],
        working_dir: Option<&str>,
        timeout_secs: u64,
    ) -> Result<(String, String, i32), String> {
        // Validate command is allowed
        if !self.security_profile.is_command_allowed(command) {
            return Err(format!(
                "Command '{}' not allowed by security profile",
                command
            ));
        }

        let mut cmd = Command::new(command);
        cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());

        if let Some(dir) = working_dir {
            // Validate path
            let path = std::path::Path::new(dir);
            if !self.security_profile.can_read_path(path) {
                return Err(format!("Path '{}' not allowed by security profile", dir));
            }
            cmd.current_dir(dir);
        }

        // Execute with timeout
        let result = timeout(Duration::from_secs(timeout_secs), cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let code = output.status.code().unwrap_or(-1);
                Ok((stdout, stderr, code))
            }
            Ok(Err(e)) => Err(format!("Command execution failed: {}", e)),
            Err(_) => Err(format!("Command timed out after {} seconds", timeout_secs)),
        }
    }
}

#[async_trait]
impl UnifiedAgent for ExecutionAgent {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn category(&self) -> AgentCategory {
        AgentCategory::Execution
    }

    fn capabilities(&self) -> Vec<AgentCapability> {
        vec![
            AgentCapability::RunCode {
                language: self.language.clone(),
            },
            AgentCapability::RunCommand {
                commands: self
                    .security_profile
                    .config
                    .allowed_commands
                    .iter()
                    .cloned()
                    .collect(),
            },
            AgentCapability::ReadFiles,
        ]
    }

    fn system_prompt(&self) -> &str {
        &self.system_prompt
    }

    fn knowledge_base(&self) -> Option<&str> {
        if self.knowledge.is_empty() {
            None
        } else {
            Some(&self.knowledge)
        }
    }

    fn security_profile(&self) -> Option<&SecurityProfile> {
        Some(&self.security_profile)
    }

    fn operations(&self) -> Vec<&str> {
        self.operations.iter().map(|s| s.as_str()).collect()
    }

    async fn execute(&self, request: AgentRequest) -> AgentResponse {
        // Base has no default ops — subclasses that implement operations override this.
        // Never return a soft success / stub payload for unimplemented work.
        let _ = request;
        AgentResponse::failure("unsupported operation")
    }
}
