//! Business Analyst Agent

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct BusinessAnalystAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl BusinessAnalystAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("business-analyst"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("requirement") {
            recommendations.push("Define SMART requirements (Specific, Measurable, Achievable, Relevant, Time-bound)");
            recommendations.push("Identify stakeholders and their needs");
            recommendations.push("Document acceptance criteria");
        }
        if input.contains("process") || input.contains("workflow") {
            recommendations.push("Map current state (as-is) process");
            recommendations.push("Identify bottlenecks and inefficiencies");
            recommendations.push("Design future state (to-be) process");
        }
        if input.contains("metric") || input.contains("kpi") {
            recommendations.push("Define leading and lagging indicators");
            recommendations.push("Establish baselines and targets");
            recommendations.push("Create measurement framework");
        }
        if recommendations.is_empty() {
            recommendations.push("Start with problem statement and business context");
            recommendations.push("Gather requirements from all stakeholder groups");
            recommendations.push("Document assumptions and constraints");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "artifacts": ["BRD (Business Requirements Document)", "User Stories", "Process Maps", "Data Flow Diagrams"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for BusinessAnalystAgent {
    fn agent_type(&self) -> &str {
        "business-analyst"
    }
    fn name(&self) -> &str {
        "Business Analyst"
    }
    fn description(&self) -> &str {
        "Gather requirements, analyze processes, and bridge business-IT communication."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "gather_requirements".to_string(),
            "analyze_process".to_string(),
            "define_kpis".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Business Analyst agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "business-analyst" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
