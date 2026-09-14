//! JavaScript Pro Agent - JavaScript/Node.js development environment
//!
//! Provides secure execution for JavaScript development tasks including:
//! - Node.js script execution
//! - npm/yarn package management
//! - ESLint linting
//! - Jest/Vitest testing
//! - Prettier formatting

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::{profiles::presets, SecurityProfile};

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct JavaScriptProAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl JavaScriptProAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: presets::javascript_pro(),
        }
    }

    fn node_run(&self, path: Option<&str>, args: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("node");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Path required for node run".to_string());
        }

        if let Some(a) = args {
            validation::validate_args(a)?;
            for arg in a.split_whitespace() {
                cmd.arg(arg);
            }
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run node: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Node execution succeeded\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Node execution failed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }

    fn npm_test(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("npm");
        cmd.arg("test");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run npm test: {}", e))?;

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

    fn npm_build(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("npm");
        cmd.arg("run").arg("build");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run npm build: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Build succeeded\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Build failed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }

    fn eslint_check(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("npx");
        cmd.arg("eslint");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            cmd.arg(".");
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run eslint: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "ESLint passed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "ESLint found issues\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }

    fn prettier_check(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("npx");
        cmd.arg("prettier").arg("--check");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            cmd.arg(".");
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run prettier: {}", e))?;

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
impl AgentTrait for JavaScriptProAgent {
    fn agent_type(&self) -> &str {
        "javascript-pro"
    }
    fn name(&self) -> &str {
        "JavaScript Pro Agent"
    }
    fn description(&self) -> &str {
        "JavaScript/Node.js development environment"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "run".to_string(),
            "test".to_string(),
            "build".to_string(),
            "lint".to_string(),
            "format".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "javascript-pro" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }

        let result = match task.operation.as_str() {
            "run" => self.node_run(task.path.as_deref(), task.args.as_deref()),
            "test" => self.npm_test(task.path.as_deref()),
            "build" => self.npm_build(task.path.as_deref()),
            "lint" => self.eslint_check(task.path.as_deref()),
            "format" => self.prettier_check(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
