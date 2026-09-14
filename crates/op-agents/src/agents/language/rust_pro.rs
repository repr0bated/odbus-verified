//! Rust Pro Agent - Rust development environment
//!
//! Provides secure execution for Rust development tasks including:
//! - Cargo check/build/test
//! - Clippy linting
//! - Format checking

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::{profiles::presets, SecurityProfile};
use simd_json::prelude::*;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct RustProAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl RustProAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: presets::rust_pro(),
        }
    }

    fn validate_features(&self, features: &str) -> Result<(), String> {
        validation::validate_args(features).map(|_| ())
    }

    fn cargo_check(&self, path: Option<&str>, features: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("cargo");
        cmd.arg("check");

        if let Some(feat) = features {
            self.validate_features(feat)?;
            cmd.arg("--features").arg(feat);
        }

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run cargo check: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Check passed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Check failed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }

    fn cargo_build(
        &self,
        path: Option<&str>,
        features: Option<&str>,
        release: bool,
    ) -> Result<String, String> {
        let mut cmd = Command::new("cargo");
        cmd.arg("build");

        if release {
            cmd.arg("--release");
        }

        if let Some(feat) = features {
            self.validate_features(feat)?;
            cmd.arg("--features").arg(feat);
        }

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run cargo build: {}", e))?;

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

    fn cargo_test(&self, path: Option<&str>, features: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("cargo");
        cmd.arg("test");

        if let Some(feat) = features {
            self.validate_features(feat)?;
            cmd.arg("--features").arg(feat);
        }

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run cargo test: {}", e))?;

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

    fn cargo_clippy(&self, path: Option<&str>, features: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("cargo");
        cmd.arg("clippy");

        if let Some(feat) = features {
            self.validate_features(feat)?;
            cmd.arg("--features").arg(feat);
        }

        cmd.arg("--").arg("-D").arg("warnings");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run cargo clippy: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Clippy passed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Clippy failed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }

    fn cargo_fmt(&self, path: Option<&str>, check_only: bool) -> Result<String, String> {
        let mut cmd = Command::new("cargo");
        cmd.arg("fmt");

        if check_only {
            cmd.arg("--check");
        }

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to run cargo fmt: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Format check passed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Format check failed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }
}

#[async_trait]
impl AgentTrait for RustProAgent {
    fn agent_type(&self) -> &str {
        "rust-pro"
    }

    fn name(&self) -> &str {
        "Rust Pro Agent"
    }

    fn description(&self) -> &str {
        "Rust development environment with cargo, clippy, and rustfmt"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "check".to_string(),
            "build".to_string(),
            "test".to_string(),
            "clippy".to_string(),
            "format".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "rust-pro" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }

        let features = task
            .config
            .get("features")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let release = task
            .config
            .get("release")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let result = match task.operation.as_str() {
            "check" => self.cargo_check(task.path.as_deref(), features.as_deref()),
            "build" => self.cargo_build(task.path.as_deref(), features.as_deref(), release),
            "test" => self.cargo_test(task.path.as_deref(), features.as_deref()),
            "clippy" => self.cargo_clippy(task.path.as_deref(), features.as_deref()),
            "format" => self.cargo_fmt(task.path.as_deref(), true),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }

    fn get_status(&self) -> String {
        format!("Rust Pro agent {} is running", self.agent_id)
    }
}
