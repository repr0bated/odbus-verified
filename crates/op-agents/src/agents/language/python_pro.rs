//! Python Pro Agent - Python 3.12+ development environment
//!
//! Provides secure execution for Python development tasks including:
//! - Script execution
//! - Testing with pytest
//! - Linting with ruff/pylint
//! - Type checking with mypy
//! - Formatting with black

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::{profiles::presets, SecurityProfile};

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct PythonProAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl PythonProAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: presets::python_pro(),
        }
    }

    fn python_run(&self, path: Option<&str>, args: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("python3");

        if let Some(a) = args {
            validation::validate_args(a)?;
            for arg in a.split_whitespace() {
                cmd.arg(arg);
            }
        }

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Path required for python run".to_string());
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run python: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Python execution succeeded\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Python execution failed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }

    fn pytest_run(&self, path: Option<&str>, args: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("pytest");
        cmd.arg("-v");

        if let Some(a) = args {
            validation::validate_args(a)?;
            for arg in a.split_whitespace() {
                cmd.arg(arg);
            }
        }

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run pytest: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Tests passed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Tests failed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }

    fn ruff_lint(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("ruff");
        cmd.arg("check");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Path required for ruff".to_string());
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run ruff: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!(
            "Ruff output\nstdout: {}\nstderr: {}",
            stdout, stderr
        ))
    }

    fn mypy_check(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("mypy");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Path required for mypy".to_string());
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run mypy: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Mypy passed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Mypy found issues\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }

    fn black_format(&self, path: Option<&str>, check_only: bool) -> Result<String, String> {
        let mut cmd = Command::new("black");

        if check_only {
            cmd.arg("--check").arg("--diff");
        }

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Path required for black".to_string());
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run black: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Code is properly formatted\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Code needs formatting\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }
}

#[async_trait]
impl AgentTrait for PythonProAgent {
    fn agent_type(&self) -> &str {
        "python-pro"
    }

    fn name(&self) -> &str {
        "Python Pro Agent"
    }

    fn description(&self) -> &str {
        "Python 3.12+ development environment with modern tooling"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "run".to_string(),
            "test".to_string(),
            "lint".to_string(),
            "typecheck".to_string(),
            "format".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "python-pro" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }

        let result = match task.operation.as_str() {
            "run" => self.python_run(task.path.as_deref(), task.args.as_deref()),
            "test" => self.pytest_run(task.path.as_deref(), task.args.as_deref()),
            "lint" => self.ruff_lint(task.path.as_deref()),
            "typecheck" => self.mypy_check(task.path.as_deref()),
            "format" => self.black_format(task.path.as_deref(), true),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }

    fn get_status(&self) -> String {
        format!("Python Pro agent {} is running", self.agent_id)
    }
}
