//! Legacy Modernizer Agent

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct LegacyModernizerAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl LegacyModernizerAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("legacy-modernizer"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("assess") || input.contains("audit") {
            recommendations.push("Document current system architecture");
            recommendations.push("Identify technical debt and risks");
            recommendations.push("Map dependencies and integrations");
        }
        if input.contains("migrate") || input.contains("rewrite") {
            recommendations.push("Consider strangler fig pattern");
            recommendations.push("Start with well-bounded components");
            recommendations.push("Maintain backward compatibility during transition");
        }
        if recommendations.is_empty() {
            recommendations.push("Prioritize based on business value and risk");
            recommendations.push("Add tests before refactoring");
            recommendations.push("Modernize incrementally, not big bang");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "strategies": ["Strangler Fig", "Branch by Abstraction", "Event Interception", "Database-First"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for LegacyModernizerAgent {
    fn agent_type(&self) -> &str {
        "legacy-modernizer"
    }
    fn name(&self) -> &str {
        "Legacy Modernizer"
    }
    fn description(&self) -> &str {
        "Modernize legacy systems incrementally and safely."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "assess_system".to_string(),
            "plan_modernization".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Legacy Modernizer agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "legacy-modernizer" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
