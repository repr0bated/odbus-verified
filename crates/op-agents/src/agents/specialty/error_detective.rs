//! Error Detective Agent

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct ErrorDetectiveAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl ErrorDetectiveAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::read_only_analysis("error-detective", vec!["logs", "errors"]),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("stack") || input.contains("trace") {
            recommendations.push("Start from the bottom of the stack trace");
            recommendations.push("Look for your code vs library code");
            recommendations.push("Check for chained exceptions");
        }
        if input.contains("intermittent") || input.contains("random") {
            recommendations.push("Look for race conditions or timing issues");
            recommendations.push("Check for resource exhaustion patterns");
            recommendations.push("Review concurrent access to shared state");
        }
        if recommendations.is_empty() {
            recommendations.push("Reproduce the error consistently first");
            recommendations.push("Check logs around the error timestamp");
            recommendations.push("Identify what changed recently");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "debugging_steps": ["Reproduce", "Isolate", "Identify root cause", "Fix", "Verify", "Prevent regression"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for ErrorDetectiveAgent {
    fn agent_type(&self) -> &str {
        "error-detective"
    }
    fn name(&self) -> &str {
        "Error Detective"
    }
    fn description(&self) -> &str {
        "Analyze errors, exceptions, and debug complex issues."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "analyze_error".to_string(),
            "find_root_cause".to_string(),
            "suggest_fix".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Error Detective agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "error-detective" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
