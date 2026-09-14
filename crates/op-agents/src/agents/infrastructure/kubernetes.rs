//! Kubernetes Architect Agent

use async_trait::async_trait;
use std::process::Command;

use crate::agents::base::{validation, AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

const ALLOWED_DIRS: &[&str] = &["/tmp", "/home", "/opt"];

pub struct KubernetesAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl KubernetesAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::read_only_analysis(
                "kubernetes-architect",
                vec!["kubectl", "helm", "kustomize"],
            ),
        }
    }

    fn kubectl_get(&self, resource: Option<&str>, args: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("kubectl");
        cmd.arg("get");

        if let Some(r) = resource {
            validation::validate_args(r)?;
            cmd.arg(r);
        } else {
            cmd.arg("all");
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

        Ok(format!("kubectl output:\n{}\n{}", stdout, stderr))
    }

    fn kubectl_describe(&self, resource: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("kubectl");
        cmd.arg("describe");

        if let Some(r) = resource {
            validation::validate_args(r)?;
            cmd.arg(r);
        } else {
            return Err("Resource required".to_string());
        }

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("kubectl describe:\n{}\n{}", stdout, stderr))
    }

    fn kubectl_logs(&self, pod: Option<&str>, args: Option<&str>) -> Result<String, String> {
        let mut cmd = Command::new("kubectl");
        cmd.arg("logs");

        if let Some(p) = pod {
            validation::validate_args(p)?;
            cmd.arg(p);
        } else {
            return Err("Pod name required".to_string());
        }

        if let Some(a) = args {
            validation::validate_args(a)?;
            for arg in a.split_whitespace() {
                cmd.arg(arg);
            }
        }

        cmd.arg("--tail=100");

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Pod logs:\n{}\n{}", stdout, stderr))
    }

    fn helm_list(&self) -> Result<String, String> {
        let mut cmd = Command::new("helm");
        cmd.arg("list").arg("-A");

        let output = cmd.output().map_err(|e| format!("Failed: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        Ok(format!("Helm releases:\n{}\n{}", stdout, stderr))
    }
}

#[async_trait]
impl AgentTrait for KubernetesAgent {
    fn agent_type(&self) -> &str {
        "kubernetes-architect"
    }
    fn name(&self) -> &str {
        "Kubernetes Architect"
    }
    fn description(&self) -> &str {
        "Kubernetes cluster management and analysis"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "get".to_string(),
            "describe".to_string(),
            "logs".to_string(),
            "helm-list".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "get" => self.kubectl_get(task.path.as_deref(), task.args.as_deref()),
            "describe" => self.kubectl_describe(task.path.as_deref()),
            "logs" => self.kubectl_logs(task.path.as_deref(), task.args.as_deref()),
            "helm-list" => self.helm_list(),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
