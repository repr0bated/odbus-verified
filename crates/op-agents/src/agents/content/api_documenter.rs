//! API Documenter Agent

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct ApiDocumenterAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl ApiDocumenterAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::content_generation("api-documenter"),
        }
    }

    fn find_routes(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("rg");
        cmd.arg("-n")
            .arg(r#"@(app\.|router\.|get|post|put|delete|patch)"#);

        if let Some(dir) = path {
            let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("API routes found:\n{}\n{}", stdout, stderr))
    }

    fn find_schemas(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("rg");
        cmd.arg("-n").arg(r#"(class|interface|type|struct).*\{"#);

        if let Some(dir) = path {
            let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Schemas found:\n{}\n{}", stdout, stderr))
    }

    fn generate_cargo_doc(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("cargo");
        cmd.arg("doc").arg("--no-deps");

        if let Some(dir) = path {
            let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            Ok(format!("Documentation generated\n{}\n{}", stdout, stderr))
        } else {
            Ok(format!("Documentation failed\n{}\n{}", stdout, stderr))
        }
    }
}

#[async_trait]
impl AgentTrait for ApiDocumenterAgent {
    fn agent_type(&self) -> &str {
        "api-documenter"
    }
    fn name(&self) -> &str {
        "API Documenter"
    }
    fn description(&self) -> &str {
        "API documentation generation"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "routes".to_string(),
            "schemas".to_string(),
            "cargo-doc".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "routes" => self.find_routes(task.path.as_deref()),
            "schemas" => self.find_schemas(task.path.as_deref()),
            "cargo-doc" => self.generate_cargo_doc(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
