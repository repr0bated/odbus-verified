//! Code Reviewer Agent

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::{profiles::presets, SecurityProfile};

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct CodeReviewerAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl CodeReviewerAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: presets::code_reviewer(),
        }
    }

    fn search_code(&self, path: Option<&str>, pattern: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("rg");

        if let Some(p) = pattern {
            validation::validate_args(p)?;
            cmd.arg(p);
        } else {
            return Err("Pattern required".to_string());
        }

        if let Some(dir) = path {
            let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        }

        cmd.arg("--no-heading").arg("-n");

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Search results:\n{}\n{}", stdout, stderr))
    }

    fn count_lines(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("tokei");

        if let Some(dir) = path {
            let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Line counts:\n{}\n{}", stdout, stderr))
    }

    fn git_diff(&self, path: Option<&str>, args: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("git");
        cmd.arg("diff");

        if let Some(a) = args {
            validation::validate_args(a)?;
            for arg in a.split_whitespace() {
                cmd.arg(arg);
            }
        }

        if let Some(dir) = path {
            let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Git diff:\n{}\n{}", stdout, stderr))
    }

    fn git_log(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("git");
        cmd.arg("log").arg("--oneline").arg("-20");

        if let Some(dir) = path {
            let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Git log:\n{}\n{}", stdout, stderr))
    }
}

#[async_trait]
impl AgentTrait for CodeReviewerAgent {
    fn agent_type(&self) -> &str {
        "code-reviewer"
    }
    fn name(&self) -> &str {
        "Code Reviewer"
    }
    fn description(&self) -> &str {
        "Code review and analysis"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "search".to_string(),
            "count".to_string(),
            "diff".to_string(),
            "log".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "search" => self.search_code(task.path.as_deref(), task.args.as_deref()),
            "count" => self.count_lines(task.path.as_deref()),
            "diff" => self.git_diff(task.path.as_deref(), task.args.as_deref()),
            "log" => self.git_log(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
