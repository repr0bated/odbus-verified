//! Legal Advisor Agent

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct LegalAdvisorAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl LegalAdvisorAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("legal-advisor"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("privacy") || input.contains("gdpr") {
            recommendations.push("Implement data minimization principles");
            recommendations.push("Provide clear privacy notices");
            recommendations.push("Establish data subject rights processes");
        }
        if input.contains("contract") || input.contains("agreement") {
            recommendations.push("Define clear terms and conditions");
            recommendations.push("Include dispute resolution clauses");
            recommendations.push("Specify governing law and jurisdiction");
        }
        if input.contains("ip") || input.contains("intellectual") {
            recommendations.push("Document ownership of created works");
            recommendations.push("Include proper licensing terms");
            recommendations.push("Protect trade secrets appropriately");
        }
        if recommendations.is_empty() {
            recommendations.push("Always consult qualified legal counsel for specifics");
            recommendations.push("Document compliance efforts");
            recommendations.push("Maintain records of legal decisions");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "disclaimer": "This is general guidance only, not legal advice. Consult qualified legal counsel.",
            "compliance_areas": ["GDPR", "CCPA", "SOC 2", "HIPAA", "PCI DSS"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for LegalAdvisorAgent {
    fn agent_type(&self) -> &str {
        "legal-advisor"
    }
    fn name(&self) -> &str {
        "Legal Advisor"
    }
    fn description(&self) -> &str {
        "General legal guidance for tech and business compliance."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "review_compliance".to_string(),
            "draft_policy".to_string(),
            "advise".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Legal Advisor agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "legal-advisor" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
