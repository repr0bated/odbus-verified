//! Terraform Specialist Agent

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct TerraformAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl TerraformAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::code_execution(
                "terraform-specialist",
                vec!["terraform", "tofu"],
            ),
        }
    }

    fn terraform_init(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("terraform");
        cmd.arg("init");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!("Init succeeded\n{}\n{}", stdout, stderr))
        } else {
            Ok(format!("Init failed\n{}\n{}", stdout, stderr))
        }
    }

    fn terraform_plan(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("terraform");
        cmd.arg("plan").arg("-no-color");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!("Plan succeeded\n{}\n{}", stdout, stderr))
        } else {
            Ok(format!("Plan failed\n{}\n{}", stdout, stderr))
        }
    }

    fn terraform_validate(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("terraform");
        cmd.arg("validate");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!("Validation passed\n{}\n{}", stdout, stderr))
        } else {
            Ok(format!("Validation failed\n{}\n{}", stdout, stderr))
        }
    }

    fn terraform_fmt(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("terraform");
        cmd.arg("fmt").arg("-check").arg("-diff");

        if let Some(p) = path {
            let validated_path = validation::validate_path(p, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!("Format check passed\n{}\n{}", stdout, stderr))
        } else {
            Ok(format!("Format check failed\n{}\n{}", stdout, stderr))
        }
    }
}

#[async_trait]
impl AgentTrait for TerraformAgent {
    fn agent_type(&self) -> &str {
        "terraform-specialist"
    }
    fn name(&self) -> &str {
        "Terraform Specialist"
    }
    fn description(&self) -> &str {
        "Infrastructure as Code with Terraform"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "init".to_string(),
            "plan".to_string(),
            "validate".to_string(),
            "fmt".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "init" => self.terraform_init(task.path.as_deref()),
            "plan" => self.terraform_plan(task.path.as_deref()),
            "validate" => self.terraform_validate(task.path.as_deref()),
            "fmt" => self.terraform_fmt(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
