//! Frontend Security Coder Agent

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct FrontendSecurityCoderAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl FrontendSecurityCoderAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("frontend-security-coder"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("xss") || input.contains("injection") {
            recommendations.push("Use framework's built-in XSS protection");
            recommendations.push("Sanitize user input before rendering");
            recommendations.push("Implement Content Security Policy (CSP)");
        }
        if input.contains("csrf") {
            recommendations.push("Use CSRF tokens for state-changing requests");
            recommendations.push("Implement SameSite cookie attribute");
            recommendations.push("Validate Origin/Referer headers");
        }
        if input.contains("storage") || input.contains("token") {
            recommendations.push("Don't store sensitive data in localStorage");
            recommendations.push("Use HttpOnly cookies for auth tokens");
            recommendations.push("Clear sensitive data on logout");
        }
        if recommendations.is_empty() {
            recommendations.push("Validate and sanitize all user inputs");
            recommendations.push("Use secure communication (HTTPS)");
            recommendations.push("Implement proper error handling (don't leak info)");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "vulnerabilities_to_check": ["XSS", "CSRF", "Clickjacking", "Open Redirects", "Sensitive Data Exposure"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for FrontendSecurityCoderAgent {
    fn agent_type(&self) -> &str {
        "frontend-security-coder"
    }
    fn name(&self) -> &str {
        "Frontend Security Coder"
    }
    fn description(&self) -> &str {
        "Write secure frontend code and prevent client-side vulnerabilities."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "secure_component".to_string(),
            "audit_code".to_string(),
            "fix_vulnerability".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Frontend Security Coder agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "frontend-security-coder" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
