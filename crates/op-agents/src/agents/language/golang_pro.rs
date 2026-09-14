//! Go Pro Agent - Go development environment
//!
//! Provides secure execution for Go development tasks including:
//! - go build/test/run
//! - gofmt formatting
//! - go vet static analysis
//! - staticcheck linting

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::{profiles::presets, SecurityProfile};

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct GolangProAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl GolangProAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: presets::golang_pro(),
        }
    }

    fn go_build(&self, path: Option<&str>, args: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("go");
        cmd.arg("build");

        if let Some(a) = args {
            validation::validate_args(a)?;
            for arg in a.split_whitespace() {
                cmd.arg(arg);
            }
        }

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run go build: {}", e))?;

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

    fn go_test(&self, path: Option<&str>, args: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("go");
        cmd.arg("test");
        cmd.arg("./...");
        cmd.arg("-v");

        if let Some(a) = args {
            validation::validate_args(a)?;
            for arg in a.split_whitespace() {
                cmd.arg(arg);
            }
        }

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run go test: {}", e))?;

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

    fn go_fmt(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("gofmt");
        cmd.arg("-l");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            cmd.arg(".");
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run gofmt: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if stdout.is_empty() {
            Ok(format!("Code is properly formatted\nstderr: {}", stderr))
        } else {
            Ok(format!(
                "Files need formatting:\n{}\nstderr: {}",
                stdout, stderr
            ))
        }
    }

    fn go_vet(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("go");
        cmd.arg("vet");
        cmd.arg("./...");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run go vet: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "go vet passed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "go vet found issues\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }

    fn go_run(&self, path: Option<&str>, args: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("go");
        cmd.arg("run");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            cmd.arg(".");
        }

        if let Some(a) = args {
            validation::validate_args(a)?;
            for arg in a.split_whitespace() {
                cmd.arg(arg);
            }
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run go run: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Execution succeeded\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Execution failed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }
}

#[async_trait]
impl AgentTrait for GolangProAgent {
    fn agent_type(&self) -> &str {
        "golang-pro"
    }

    fn name(&self) -> &str {
        "Go Pro Agent"
    }

    fn description(&self) -> &str {
        "Go development environment with build, test, and analysis tools"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "build".to_string(),
            "test".to_string(),
            "fmt".to_string(),
            "vet".to_string(),
            "run".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "golang-pro" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }

        let result = match task.operation.as_str() {
            "build" => self.go_build(task.path.as_deref(), task.args.as_deref()),
            "test" => self.go_test(task.path.as_deref(), task.args.as_deref()),
            "fmt" => self.go_fmt(task.path.as_deref()),
            "vet" => self.go_vet(task.path.as_deref()),
            "run" => self.go_run(task.path.as_deref(), task.args.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }

    fn get_status(&self) -> String {
        format!("Go Pro agent {} is running", self.agent_id)
    }
}
