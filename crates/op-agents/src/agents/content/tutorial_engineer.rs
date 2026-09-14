//! Tutorial Engineer Agent

use async_trait::async_trait;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct TutorialEngineerAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl TutorialEngineerAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::content_generation("tutorial-engineer"),
        }
    }

    fn analyze_code(&self, path: Option<&str>) -> Result<String, String> {
        if let Some(file) = path {
            let validated_path = validation::validate_path(file, ALLOWED_DIRS)?;
            let content = std::fs::read_to_string(&validated_path)
                .map_err(|e| format!("Failed to read file: {}", e))?;

            // Count lines, functions, etc.
            let lines = content.lines().count();
            let functions = content.matches("fn ").count() + content.matches("def ").count();
            let classes = content.matches("class ").count() + content.matches("struct ").count();

            Ok(format!(
                "Code analysis:\n- Lines: {}\n- Functions: {}\n- Classes/Structs: {}",
                lines, functions, classes
            ))
        } else {
            Err("File path required".to_string())
        }
    }

    fn extract_comments(&self, path: Option<&str>) -> Result<String, String> {
        if let Some(file) = path {
            let validated_path = validation::validate_path(file, ALLOWED_DIRS)?;
            let content = std::fs::read_to_string(&validated_path)
                .map_err(|e| format!("Failed to read file: {}", e))?;

            // Extract comments (simple heuristic)
            let comments: Vec<&str> = content
                .lines()
                .filter(|line| {
                    let trimmed = line.trim();
                    trimmed.starts_with("//")
                        || trimmed.starts_with("#")
                        || trimmed.starts_with("/*")
                        || trimmed.starts_with("*")
                        || trimmed.starts_with("///")
                })
                .take(50)
                .collect();

            Ok(format!("Comments found:\n{}", comments.join("\n")))
        } else {
            Err("File path required".to_string())
        }
    }
}

#[async_trait]
impl AgentTrait for TutorialEngineerAgent {
    fn agent_type(&self) -> &str {
        "tutorial-engineer"
    }
    fn name(&self) -> &str {
        "Tutorial Engineer"
    }
    fn description(&self) -> &str {
        "Tutorial and learning content creation"
    }

    fn operations(&self) -> Vec<String> {
        vec!["analyze".to_string(), "comments".to_string()]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "analyze" => self.analyze_code(task.path.as_deref()),
            "comments" => self.extract_comments(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
