//! Mermaid Expert Agent

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct MermaidExpertAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl MermaidExpertAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::content_generation("mermaid-expert"),
        }
    }

    fn validate_mermaid(&self, path: Option<&str>) -> Result<String, String> {
        // Read mermaid content from file
        if let Some(file) = path {
            let validated_path = validation::validate_path(file, ALLOWED_DIRS)?;
            let content = std::fs::read_to_string(&validated_path)
                .map_err(|e| format!("Failed to read file: {}", e))?;

            // Basic validation
            let valid_starts = [
                "graph",
                "sequenceDiagram",
                "classDiagram",
                "stateDiagram",
                "erDiagram",
                "flowchart",
                "gantt",
                "pie",
            ];
            let is_valid = valid_starts.iter().any(|s| content.trim().starts_with(s));

            if is_valid {
                Ok(format!("Mermaid syntax appears valid:\n{}", content))
            } else {
                Ok(format!(
                    "Warning: Mermaid diagram should start with a valid diagram type. Content:\n{}",
                    content
                ))
            }
        } else {
            Err("File path required".to_string())
        }
    }

    fn find_diagrams(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("rg");
        cmd.arg("-n").arg("```mermaid");

        if let Some(dir) = path {
            let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Mermaid diagrams found:\n{}\n{}", stdout, stderr))
    }
}

#[async_trait]
impl AgentTrait for MermaidExpertAgent {
    fn agent_type(&self) -> &str {
        "mermaid-expert"
    }
    fn name(&self) -> &str {
        "Mermaid Expert"
    }
    fn description(&self) -> &str {
        "Mermaid diagram creation and validation"
    }

    fn operations(&self) -> Vec<String> {
        vec!["validate".to_string(), "find".to_string()]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "validate" => self.validate_mermaid(task.path.as_deref()),
            "find" => self.find_diagrams(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
