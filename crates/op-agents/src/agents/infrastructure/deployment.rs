//! Deployment Engineer Agent

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct DeploymentAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl DeploymentAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::code_execution(
                "deployment-engineer",
                vec!["docker", "docker-compose", "ansible"],
            ),
        }
    }

    fn docker_build(&self, path: Option<&str>, args: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("docker");
        cmd.arg("build");

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
            cmd.arg(".");
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!("Docker build succeeded\n{}\n{}", stdout, stderr))
        } else {
            Ok(format!("Docker build failed\n{}\n{}", stdout, stderr))
        }
    }

    fn docker_compose_up(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("docker-compose");
        cmd.arg("up").arg("-d");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!(
                "Docker Compose up succeeded\n{}\n{}",
                stdout, stderr
            ))
        } else {
            Ok(format!("Docker Compose up failed\n{}\n{}", stdout, stderr))
        }
    }

    fn ansible_check(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("ansible-playbook");
        cmd.arg("--check");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            return Err("Playbook path required".to_string());
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Ansible check:\n{}\n{}", stdout, stderr))
    }
}

#[async_trait]
impl AgentTrait for DeploymentAgent {
    fn agent_type(&self) -> &str {
        "deployment-engineer"
    }
    fn name(&self) -> &str {
        "Deployment Engineer"
    }
    fn description(&self) -> &str {
        "Deployment automation with Docker and Ansible"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "docker-build".to_string(),
            "compose-up".to_string(),
            "ansible-check".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "docker-build" => self.docker_build(task.path.as_deref(), task.args.as_deref()),
            "compose-up" => self.docker_compose_up(task.path.as_deref()),
            "ansible-check" => self.ansible_check(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
