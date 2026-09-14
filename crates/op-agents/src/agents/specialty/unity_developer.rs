//! Unity Developer Agent

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct UnityDeveloperAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl UnityDeveloperAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("unity-developer"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("performance") {
            recommendations.push("Use object pooling for frequently spawned objects");
            recommendations.push("Optimize draw calls with batching");
            recommendations.push("Profile with Unity Profiler regularly");
        }
        if input.contains("script") || input.contains("code") {
            recommendations.push("Use ScriptableObjects for data");
            recommendations.push("Implement dependency injection");
            recommendations.push("Avoid Update() for infrequent checks");
        }
        if recommendations.is_empty() {
            recommendations.push("Follow Unity's coding conventions");
            recommendations.push("Use prefabs for reusable objects");
            recommendations.push("Implement proper scene management");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "unity_features": ["DOTS", "Addressables", "Input System", "Timeline", "Cinemachine"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for UnityDeveloperAgent {
    fn agent_type(&self) -> &str {
        "unity-developer"
    }
    fn name(&self) -> &str {
        "Unity Developer"
    }
    fn description(&self) -> &str {
        "Build games and interactive experiences with Unity."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "create_script".to_string(),
            "optimize".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Unity Developer agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "unity-developer" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
