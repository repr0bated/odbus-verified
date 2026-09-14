//! TypeScript Pro Agent - TypeScript development environment

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::{profiles::presets, SecurityProfile};

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct TypeScriptProAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl TypeScriptProAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: presets::typescript_pro(),
        }
    }

    fn tsc_check(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("npx");
        cmd.arg("tsc").arg("--noEmit");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run tsc: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Type check passed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Type check failed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }

    fn tsc_build(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("npx");
        cmd.arg("tsc");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run tsc build: {}", e))?;
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

    fn npm_test(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("npm");
        cmd.arg("test");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run tests: {}", e))?;
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

    fn eslint_check(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("npx");
        cmd.arg("eslint").arg("--ext").arg(".ts,.tsx");

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
                "Lint passed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Lint found issues\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }
}

#[async_trait]
impl AgentTrait for TypeScriptProAgent {
    fn agent_type(&self) -> &str {
        "typescript-pro"
    }
    fn name(&self) -> &str {
        "TypeScript Pro Agent"
    }
    fn description(&self) -> &str {
        "TypeScript development environment"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "check".to_string(),
            "build".to_string(),
            "test".to_string(),
            "lint".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "typescript-pro" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }

        let result = match task.operation.as_str() {
            "check" => self.tsc_check(task.path.as_deref()),
            "build" => self.tsc_build(task.path.as_deref()),
            "test" => self.npm_test(task.path.as_deref()),
            "lint" => self.eslint_check(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
