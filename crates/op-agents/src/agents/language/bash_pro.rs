//! Bash Pro Agent - Shell scripting environment

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct BashProAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl BashProAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::code_execution("bash-pro", vec!["bash", "sh", "shellcheck"]),
        }
    }

    fn bash_run(&self, path: Option<&str>, args: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("bash");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Path required".to_string());
        }

        if let Some(a) = args {
            validation::validate_args(a)?;
            for arg in a.split_whitespace() {
                cmd.arg(arg);
            }
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Script succeeded\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Script failed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }

    fn shellcheck_lint(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("shellcheck");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Path required".to_string());
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "ShellCheck passed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "ShellCheck found issues\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }

    fn bash_syntax_check(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("bash");
        cmd.arg("-n");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Path required".to_string());
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!("Syntax OK\nstdout: {}\nstderr: {}", stdout, stderr))
        } else {
            Ok(format!(
                "Syntax errors\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }
}

#[async_trait]
impl AgentTrait for BashProAgent {
    fn agent_type(&self) -> &str {
        "bash-pro"
    }
    fn name(&self) -> &str {
        "Bash Pro Agent"
    }
    fn description(&self) -> &str {
        "Shell scripting environment with ShellCheck"
    }

    fn operations(&self) -> Vec<String> {
        vec!["run".to_string(), "lint".to_string(), "check".to_string()]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "bash-pro" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }

        let result = match task.operation.as_str() {
            "run" => self.bash_run(task.path.as_deref(), task.args.as_deref()),
            "lint" => self.shellcheck_lint(task.path.as_deref()),
            "check" => self.bash_syntax_check(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
