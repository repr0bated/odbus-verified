//! SEO Content Writer Agent

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct SEOContentWriterAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl SEOContentWriterAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("seo-content-writer"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("blog") || input.contains("article") {
            recommendations.push("Include target keyword in title, H1, and first paragraph");
            recommendations.push("Use semantic keywords throughout content");
            recommendations.push("Structure with H2/H3 headers for readability");
        }
        if input.contains("product") || input.contains("landing") {
            recommendations.push("Focus on user intent and benefits");
            recommendations.push("Include clear calls-to-action");
            recommendations.push("Add schema markup for rich snippets");
        }
        if recommendations.is_empty() {
            recommendations.push("Write for users first, search engines second");
            recommendations.push("Create comprehensive, valuable content");
            recommendations.push("Include internal and external links");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "content_checklist": ["Title tag (50-60 chars)", "Meta description (150-160 chars)", "Header hierarchy", "Image alt text", "Internal links"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for SEOContentWriterAgent {
    fn agent_type(&self) -> &str {
        "seo-content-writer"
    }
    fn name(&self) -> &str {
        "SEO Content Writer"
    }
    fn description(&self) -> &str {
        "Create SEO-optimized content that ranks and converts."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "write_article".to_string(),
            "optimize_content".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("SEO Content Writer agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "seo-content-writer" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
