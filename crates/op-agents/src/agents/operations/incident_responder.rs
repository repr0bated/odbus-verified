//! Incident Responder Agent
//!
//! Expert SRE incident responder specializing in rapid problem resolution,
//! modern observability, and comprehensive incident management.

use async_trait::async_trait;
use simd_json::json;

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

/// Incident Responder Agent
pub struct IncidentResponderAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl IncidentResponderAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::read_only_analysis(
                "incident-responder",
                vec!["logs", "metrics", "traces"],
            ),
            agent_id,
        }
    }

    fn handle_incident(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();

        let severity = if input.contains("outage")
            || input.contains("down")
            || input.contains("critical")
        {
            "P0 - Critical"
        } else if input.contains("degraded") || input.contains("slow") || input.contains("error") {
            "P1 - High"
        } else if input.contains("intermittent") || input.contains("minor") {
            "P2 - Medium"
        } else {
            "P3 - Low"
        };

        let mut immediate_actions = Vec::new();
        let mut investigation_steps = Vec::new();

        if input.contains("database") || input.contains("db") {
            immediate_actions.push("Check database connection pools and replication lag");
            immediate_actions.push("Review slow query logs for blocking queries");
            investigation_steps.push("Analyze query execution plans");
        }

        if input.contains("memory") || input.contains("oom") {
            immediate_actions.push("Check for memory leaks and OOM kills");
            immediate_actions.push("Review recent deployment changes");
            investigation_steps.push("Analyze heap dumps if available");
        }

        if input.contains("network") || input.contains("timeout") {
            immediate_actions.push("Check network connectivity and DNS resolution");
            immediate_actions.push("Review load balancer health checks");
            investigation_steps.push("Analyze network traces");
        }

        if immediate_actions.is_empty() {
            immediate_actions.push("Establish incident command structure");
            immediate_actions.push("Check service health dashboards");
            immediate_actions.push("Review recent changes (deployments, configs)");
            investigation_steps.push("Correlate metrics, logs, and traces");
            investigation_steps.push("Check upstream/downstream dependencies");
        }

        let result = json!({
            "incident_assessment": {
                "input": args.unwrap_or(""),
                "severity": severity,
                "immediate_actions": immediate_actions,
                "investigation_steps": investigation_steps
            },
            "incident_command": {
                "structure": {
                    "incident_commander": "Single decision-maker, coordinates response",
                    "communication_lead": "Manages stakeholder updates",
                    "technical_lead": "Coordinates technical investigation"
                }
            },
            "resolution_steps": [
                "1. Stabilize - Apply quick mitigations",
                "2. Investigate - Find root cause",
                "3. Fix - Implement permanent solution",
                "4. Validate - Verify service restoration",
                "5. Document - Prepare post-mortem"
            ]
        });

        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for IncidentResponderAgent {
    fn agent_type(&self) -> &str {
        "incident-responder"
    }
    fn name(&self) -> &str {
        "Incident Responder"
    }
    fn description(&self) -> &str {
        "Expert SRE incident responder specializing in rapid problem resolution and incident management."
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "assess_incident".to_string(),
            "investigate".to_string(),
            "coordinate_response".to_string(),
            "write_postmortem".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    fn get_status(&self) -> String {
        format!("Incident Responder agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "incident-responder" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }

        match self.handle_incident(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
