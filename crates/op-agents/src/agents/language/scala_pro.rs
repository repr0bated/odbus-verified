//! Scala Pro Agent - Scala development environment

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct ScalaProAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl ScalaProAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::code_execution(
                "scala-pro",
                vec!["scala", "scalac", "sbt", "mill"],
            ),
        }
    }

    fn sbt_compile(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("sbt");
        cmd.arg("compile");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Compilation succeeded\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        } else {
            Ok(format!(
                "Compilation failed\nstdout: {}\nstderr: {}",
                stdout, stderr
            ))
        }
    }

    fn sbt_test(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("sbt");
        cmd.arg("test");

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
}

#[async_trait]
impl AgentTrait for ScalaProAgent {
    fn agent_type(&self) -> &str {
        "scala-pro"
    }
    fn name(&self) -> &str {
        "Scala Pro Agent"
    }
    fn description(&self) -> &str {
        "Scala development environment with SBT"
    }

    fn operations(&self) -> Vec<String> {
        vec!["compile".to_string(), "test".to_string()]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "scala-pro" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }

        let result = match task.operation.as_str() {
            "compile" => self.sbt_compile(task.path.as_deref()),
            "test" => self.sbt_test(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
