//! Base Execution Agent Implementation

use async_trait::async_trait;
use std::process::Stdio;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

async fn read_bounded<R>(mut reader: R) -> std::io::Result<String>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::with_capacity(8192);
    let mut chunk = [0u8; 8192];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            return Ok(String::from_utf8_lossy(&bytes).into_owned());
        }
        if bytes.len().saturating_add(read) > MAX_OUTPUT_BYTES {
            return Err(std::io::Error::other("executor output exceeded 1MiB bound"));
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
}

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
        let mut security_profile = SecurityProfile::code_execution(id, allowed_commands.clone());
        security_profile
            .config
            .allowed_read_paths
            .push("/srv/git".into());
        security_profile
            .config
            .allowed_write_paths
            .push("/srv/git".into());

        let system_prompt = EXECUTION_AGENT
            .replace("{agent_name}", name)
            .replace("{language}", language)
            .replace("{allowed_commands}", &allowed_commands.join(", "))
            .replace(
                "{file_access}",
                "read: /home, /tmp, /opt, /srv/git; write: /tmp, /srv/git",
            );

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
        // The timeout owns the child future; ensure dropping it cannot leave a
        // detached executor process behind.
        cmd.kill_on_drop(true);
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
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Command execution failed: {}", e))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| "executor stdout pipe unavailable".to_string())?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| "executor stderr pipe unavailable".to_string())?;
        let result = timeout(Duration::from_secs(timeout_secs.min(3600)), async {
            let (stdout, stderr) =
                tokio::try_join!(read_bounded(&mut stdout), read_bounded(&mut stderr))
                    .map_err(|e| e.to_string())?;
            let status = child
                .wait()
                .await
                .map_err(|e| format!("Command wait failed: {}", e))?;
            Ok::<_, String>((stdout, stderr, status.code().unwrap_or(-1)))
        })
        .await;
        match result {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(error)) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                Err(error)
            }
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                Err(format!(
                    "Command timed out after {} seconds",
                    timeout_secs.min(3600)
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell_agent() -> ExecutionAgent {
        ExecutionAgent::new("test-shell", "test shell", "", "shell", vec!["sh"])
    }

    #[tokio::test]
    async fn output_limit_kills_chatty_child() {
        let result = shell_agent()
            .execute_command("sh", &["-c", "while :; do printf x; done"], None, 5)
            .await;
        assert!(result
            .expect_err("chatty child must hit output bound")
            .contains("1MiB bound"));
    }

    #[tokio::test]
    async fn timeout_kills_looping_child() {
        let started = tokio::time::Instant::now();
        let result = shell_agent()
            .execute_command("sh", &["-c", "while :; do :; done"], None, 1)
            .await;
        assert!(result
            .expect_err("looping child must time out")
            .contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(5));
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
