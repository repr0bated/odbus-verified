//! Cloud Architect Agent

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

pub struct CloudArchitectAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl CloudArchitectAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::read_only_analysis(
                "cloud-architect",
                vec!["aws", "gcloud", "az"],
            ),
        }
    }

    fn aws_describe(&self, resource: Option<&str>, args: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("aws");

        if let Some(r) = resource {
            validation::validate_args(r)?;
            for part in r.split_whitespace() {
                cmd.arg(part);
            }
        } else {
            cmd.arg("sts").arg("get-caller-identity");
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

        Ok(format!("AWS output:\n{}\n{}", stdout, stderr))
    }

    fn gcloud_describe(&self, resource: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("gcloud");

        if let Some(r) = resource {
            validation::validate_args(r)?;
            for part in r.split_whitespace() {
                cmd.arg(part);
            }
        } else {
            cmd.arg("config").arg("list");
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("GCloud output:\n{}\n{}", stdout, stderr))
    }
}

#[async_trait]
impl AgentTrait for CloudArchitectAgent {
    fn agent_type(&self) -> &str {
        "cloud-architect"
    }
    fn name(&self) -> &str {
        "Cloud Architect"
    }
    fn description(&self) -> &str {
        "Multi-cloud architecture analysis"
    }

    fn operations(&self) -> Vec<String> {
        vec!["aws-describe".to_string(), "gcloud-describe".to_string()]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "aws-describe" => self.aws_describe(task.path.as_deref(), task.args.as_deref()),
            "gcloud-describe" => self.gcloud_describe(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
