//! Django Pro Agent - Django web framework expert

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct DjangoProAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl DjangoProAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("django-pro"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("model") || input.contains("database") {
            recommendations.push("Use migrations for all schema changes");
            recommendations.push("Add db_index=True for frequently queried fields");
            recommendations.push("Use select_related/prefetch_related for N+1 prevention");
        }
        if input.contains("view") || input.contains("api") {
            recommendations.push("Use class-based views for complex logic");
            recommendations.push("Implement proper permission classes");
            recommendations.push("Add pagination for list endpoints");
        }
        if input.contains("auth") || input.contains("security") {
            recommendations.push("Use Django's built-in authentication");
            recommendations.push("Implement CSRF protection");
            recommendations.push("Use django-allauth for social auth");
        }
        if recommendations.is_empty() {
            recommendations.push("Follow Django project structure conventions");
            recommendations.push("Use Django REST Framework for APIs");
            recommendations.push("Implement proper logging and error handling");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "best_practices": ["Use environment variables for secrets", "Implement caching with Redis", "Write tests with pytest-django"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for DjangoProAgent {
    fn agent_type(&self) -> &str {
        "django-pro"
    }
    fn name(&self) -> &str {
        "Django Pro"
    }
    fn description(&self) -> &str {
        "Expert Django developer for web applications and REST APIs."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "create_model".to_string(),
            "create_view".to_string(),
            "configure_auth".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Django Pro agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "django-pro" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
