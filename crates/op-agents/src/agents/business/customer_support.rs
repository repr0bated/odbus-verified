//! Customer Support Agent

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct CustomerSupportAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl CustomerSupportAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("customer-support"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("ticket") || input.contains("issue") {
            recommendations.push("Acknowledge the issue promptly");
            recommendations.push("Gather necessary information to diagnose");
            recommendations.push("Set clear expectations for resolution timeline");
        }
        if input.contains("escalat") {
            recommendations.push("Define clear escalation criteria");
            recommendations.push("Document issue history before escalating");
            recommendations.push("Ensure warm handoff with context");
        }
        if input.contains("refund") || input.contains("complaint") {
            recommendations.push("Listen and acknowledge customer frustration");
            recommendations.push("Follow established policies consistently");
            recommendations.push("Document resolution and follow up");
        }
        if recommendations.is_empty() {
            recommendations.push("Practice empathy and active listening");
            recommendations.push("Use positive language and solution-focus");
            recommendations.push("Document interactions for continuity");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "best_practices": ["First contact resolution", "CSAT tracking", "Knowledge base maintenance", "Proactive communication"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for CustomerSupportAgent {
    fn agent_type(&self) -> &str {
        "customer-support"
    }
    fn name(&self) -> &str {
        "Customer Support"
    }
    fn description(&self) -> &str {
        "Handle customer inquiries and resolve issues effectively."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "handle_ticket".to_string(),
            "escalate".to_string(),
            "draft_response".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Customer Support agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "customer-support" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
