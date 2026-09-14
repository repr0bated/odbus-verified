//! Backend Security Coder Agent

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct BackendSecurityCoderAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl BackendSecurityCoderAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("backend-security-coder"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("injection") || input.contains("sql") {
            recommendations.push("Use parameterized queries/prepared statements");
            recommendations.push("Implement input validation and sanitization");
            recommendations.push("Apply principle of least privilege for DB access");
        }
        if input.contains("auth") || input.contains("session") {
            recommendations.push("Use secure session management (HttpOnly, Secure flags)");
            recommendations.push("Implement proper password hashing (bcrypt/argon2)");
            recommendations.push("Add rate limiting for authentication endpoints");
        }
        if input.contains("api") || input.contains("endpoint") {
            recommendations.push("Implement proper authorization checks");
            recommendations.push("Add request validation middleware");
            recommendations.push("Use HTTPS everywhere with proper TLS config");
        }
        if recommendations.is_empty() {
            recommendations.push("Follow OWASP Top 10 guidelines");
            recommendations.push("Implement defense in depth");
            recommendations.push("Add security headers (CSP, HSTS, X-Frame-Options)");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "vulnerabilities_to_check": ["SQL Injection", "Authentication Bypass", "IDOR", "SSRF", "XXE"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for BackendSecurityCoderAgent {
    fn agent_type(&self) -> &str {
        "backend-security-coder"
    }
    fn name(&self) -> &str {
        "Backend Security Coder"
    }
    fn description(&self) -> &str {
        "Write secure backend code and identify vulnerabilities."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "secure_endpoint".to_string(),
            "audit_code".to_string(),
            "fix_vulnerability".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Backend Security Coder agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "backend-security-coder" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
