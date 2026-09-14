//! DevOps Troubleshooter Agent
//!
//! Expert troubleshooter for investigating production issues,
//! analyzing logs and metrics, and diagnosing system problems.

use async_trait::async_trait;
use simd_json::json;

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

/// DevOps Troubleshooter Agent
pub struct DevOpsTroubleshooterAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl DevOpsTroubleshooterAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::read_only_analysis(
                "devops-troubleshooter",
                vec!["kubectl", "docker", "systemctl"],
            ),
            agent_id,
        }
    }

    fn troubleshoot(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();

        let mut investigation_areas = Vec::new();
        let mut commands_to_run = Vec::new();
        let mut log_patterns = Vec::new();

        if input.contains("kubernetes") || input.contains("k8s") || input.contains("pod") {
            investigation_areas.push("Kubernetes cluster health");
            commands_to_run.push("kubectl get pods -A | grep -v Running");
            commands_to_run.push("kubectl describe pod <pod-name>");
            log_patterns.push("OOMKilled, CrashLoopBackOff, ImagePullBackOff");
        }

        if input.contains("network") || input.contains("dns") || input.contains("connectivity") {
            investigation_areas.push("Network connectivity");
            commands_to_run.push("nslookup <hostname>");
            commands_to_run.push("curl -v <endpoint>");
            log_patterns.push("Connection refused, timeout, DNS resolution failed");
        }

        if input.contains("disk") || input.contains("storage") || input.contains("space") {
            investigation_areas.push("Disk usage and I/O");
            commands_to_run.push("df -h");
            commands_to_run.push("du -sh /*");
            log_patterns.push("No space left on device, I/O errors");
        }

        if input.contains("memory") || input.contains("oom") {
            investigation_areas.push("Memory utilization");
            commands_to_run.push("free -h");
            commands_to_run.push("dmesg | grep -i oom");
            log_patterns.push("OutOfMemoryError, OOM killer invoked");
        }

        if investigation_areas.is_empty() {
            investigation_areas.push("System overview");
            commands_to_run.push("uptime");
            commands_to_run.push("free -h");
            commands_to_run.push("df -h");
            log_patterns.push("Error, Warning, Critical");
        }

        let result = json!({
            "troubleshooting": {
                "input": args.unwrap_or(""),
                "investigation_areas": investigation_areas,
                "diagnostic_commands": commands_to_run,
                "log_patterns_to_search": log_patterns
            },
            "systematic_approach": [
                "1. Gather symptoms and timeline",
                "2. Check recent changes",
                "3. Review monitoring dashboards",
                "4. Analyze logs for error patterns",
                "5. Check resource utilization",
                "6. Investigate dependencies",
                "7. Test hypotheses systematically"
            ],
            "common_root_causes": [
                "Recent deployment introduced regression",
                "Resource exhaustion (CPU, memory, disk)",
                "External dependency failure",
                "Configuration drift or misconfiguration",
                "Network connectivity issues"
            ]
        });

        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for DevOpsTroubleshooterAgent {
    fn agent_type(&self) -> &str {
        "devops-troubleshooter"
    }
    fn name(&self) -> &str {
        "DevOps Troubleshooter"
    }
    fn description(&self) -> &str {
        "Expert troubleshooter for investigating production issues and diagnosing system problems."
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "investigate".to_string(),
            "analyze_logs".to_string(),
            "check_metrics".to_string(),
            "diagnose_issue".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    fn get_status(&self) -> String {
        format!("DevOps Troubleshooter agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "devops-troubleshooter" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }

        match self.troubleshoot(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
