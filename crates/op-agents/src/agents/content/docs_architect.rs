//! Docs Architect Agent

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::{profiles::presets, SecurityProfile};

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct DocsArchitectAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl DocsArchitectAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: presets::docs_architect(),
        }
    }

    fn read_file(&self, path: Option<&str>) -> Result<String, String> {
        if let Some(file) = path {
            let validated_path = validation::validate_path(file, ALLOWED_DIRS)?;
            let content = std::fs::read_to_string(&validated_path)
                .map_err(|e| format!("Failed to read file: {}", e))?;

            // Truncate if too large
            if content.len() > 100000 {
                Ok(format!(
                    "File content (truncated):\n{}...",
                    &content[..100000]
                ))
            } else {
                Ok(format!("File content:\n{}", content))
            }
        } else {
            Err("File path required".to_string())
        }
    }

    fn list_markdown(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("find");

        if let Some(dir) = path {
            let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            cmd.arg(".");
        }

        cmd.arg("-name").arg("*.md").arg("-type").arg("f");

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Markdown files:\n{}\n{}", stdout, stderr))
    }

    fn check_links(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("rg");
        cmd.arg(r"\[.*?\]\(.*?\)");

        if let Some(file) = path {
            let validated_path = validation::validate_path(file, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Links found:\n{}\n{}", stdout, stderr))
    }
}

#[async_trait]
impl AgentTrait for DocsArchitectAgent {
    fn agent_type(&self) -> &str {
        "docs-architect"
    }
    fn name(&self) -> &str {
        "Docs Architect"
    }
    fn description(&self) -> &str {
        "Documentation architecture and organization"
    }

    fn operations(&self) -> Vec<String> {
        vec!["read".to_string(), "list".to_string(), "links".to_string()]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "read" => self.read_file(task.path.as_deref()),
            "list" => self.list_markdown(task.path.as_deref()),
            "links" => self.check_links(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
