//! PHP Pro Agent - PHP development environment

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct PhpProAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl PhpProAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::code_execution(
                "php-pro",
                vec!["php", "composer", "phpunit", "phpstan"],
            ),
        }
    }

    fn php_run(&self, path: Option<&str>, args: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("php");

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

    fn phpunit_test(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("vendor/bin/phpunit");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
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

    fn phpstan_analyze(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("vendor/bin/phpstan");
        cmd.arg("analyse");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!(
            "PHPStan output\nstdout: {}\nstderr: {}",
            stdout, stderr
        ))
    }

    fn php_lint(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("php");
        cmd.arg("-l");

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
                "Syntax errors found\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }
}

#[async_trait]
impl AgentTrait for PhpProAgent {
    fn agent_type(&self) -> &str {
        "php-pro"
    }
    fn name(&self) -> &str {
        "PHP Pro Agent"
    }
    fn description(&self) -> &str {
        "PHP development environment with PHPUnit and PHPStan"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "run".to_string(),
            "test".to_string(),
            "analyze".to_string(),
            "lint".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "php-pro" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }

        let result = match task.operation.as_str() {
            "run" => self.php_run(task.path.as_deref(), task.args.as_deref()),
            "test" => self.phpunit_test(task.path.as_deref()),
            "analyze" => self.phpstan_analyze(task.path.as_deref()),
            "lint" => self.php_lint(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
