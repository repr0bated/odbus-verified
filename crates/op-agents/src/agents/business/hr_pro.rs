//! HR Pro Agent

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct HRProAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl HRProAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("hr-pro"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("policy") {
            recommendations.push("Ensure compliance with labor laws");
            recommendations.push("Document clear procedures and expectations");
            recommendations.push("Include appeal/grievance processes");
        }
        if input.contains("hiring") || input.contains("recruit") {
            recommendations.push("Define clear job requirements and competencies");
            recommendations.push("Use structured interviews for consistency");
            recommendations.push("Ensure fair and unbiased selection process");
        }
        if input.contains("performance") || input.contains("review") {
            recommendations.push("Set clear, measurable objectives");
            recommendations.push("Provide regular feedback throughout the year");
            recommendations.push("Document performance discussions");
        }
        if recommendations.is_empty() {
            recommendations.push("Maintain up-to-date employee handbook");
            recommendations.push("Ensure compliance with employment regulations");
            recommendations.push("Document all HR decisions and rationale");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "compliance_areas": ["Equal Employment", "Workplace Safety", "Privacy", "Benefits Administration"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for HRProAgent {
    fn agent_type(&self) -> &str {
        "hr-pro"
    }
    fn name(&self) -> &str {
        "HR Pro"
    }
    fn description(&self) -> &str {
        "HR policy guidance, compliance, and people management best practices."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "draft_policy".to_string(),
            "review_compliance".to_string(),
            "advise".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("HR Pro agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "hr-pro" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
