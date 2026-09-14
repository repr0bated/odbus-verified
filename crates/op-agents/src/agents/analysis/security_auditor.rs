//! Security Auditor Agent

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::{profiles::presets, SecurityProfile};

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct SecurityAuditorAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl SecurityAuditorAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: presets::security_auditor(),
        }
    }

    fn semgrep_scan(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("semgrep");
        cmd.arg("--config=auto");

        if let Some(dir) = path {
            let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            cmd.arg(".");
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Semgrep scan:\n{}\n{}", stdout, stderr))
    }

    fn bandit_scan(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("bandit");
        cmd.arg("-r");

        if let Some(dir) = path {
            let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;
            cmd.arg(validated_path);
        } else {
            cmd.arg(".");
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Bandit scan:\n{}\n{}", stdout, stderr))
    }

    fn cargo_audit(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("cargo");
        cmd.arg("audit");

        if let Some(dir) = path {
            let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Cargo audit:\n{}\n{}", stdout, stderr))
    }

    fn npm_audit(&self, path: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("npm");
        cmd.arg("audit").arg("--json");

        if let Some(dir) = path {
            let validated_path = validation::validate_path(dir, ALLOWED_DIRS)?;
            cmd.current_dir(validated_path);
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("NPM audit:\n{}\n{}", stdout, stderr))
    }
}

#[async_trait]
impl AgentTrait for SecurityAuditorAgent {
    fn agent_type(&self) -> &str {
        "security-auditor"
    }
    fn name(&self) -> &str {
        "Security Auditor"
    }
    fn description(&self) -> &str {
        "Security vulnerability scanning and auditing"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "semgrep".to_string(),
            "bandit".to_string(),
            "cargo-audit".to_string(),
            "npm-audit".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "semgrep" => self.semgrep_scan(task.path.as_deref()),
            "bandit" => self.bandit_scan(task.path.as_deref()),
            "cargo-audit" => self.cargo_audit(task.path.as_deref()),
            "npm-audit" => self.npm_audit(task.path.as_deref()),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
