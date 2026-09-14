//! Prompt Engineer Agent - Prompt design, optimization, evaluation

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct PromptEngineerAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl PromptEngineerAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("prompt-engineer"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("system") || input.contains("persona") {
            recommendations.push("Define clear role and expertise boundaries");
            recommendations.push("Include behavioral constraints and guardrails");
            recommendations.push("Specify output format expectations");
        }
        if input.contains("few-shot") || input.contains("example") {
            recommendations.push("Use 3-5 diverse, representative examples");
            recommendations.push("Include both positive and edge cases");
            recommendations.push("Format examples consistently");
        }
        if input.contains("chain") || input.contains("reasoning") {
            recommendations.push("Use step-by-step reasoning prompts");
            recommendations.push("Add 'Let's think step by step' for complex tasks");
            recommendations.push("Break complex tasks into subtasks");
        }
        if recommendations.is_empty() {
            recommendations.push("Be specific and unambiguous in instructions");
            recommendations.push("Test prompts with diverse inputs");
            recommendations.push("Iterate based on failure cases");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "techniques": ["Few-shot learning", "Chain-of-thought", "Self-consistency", "Tree-of-thoughts", "ReAct"],
            "evaluation": ["Human evaluation", "LLM-as-judge", "Task-specific metrics"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for PromptEngineerAgent {
    fn agent_type(&self) -> &str {
        "prompt-engineer"
    }
    fn name(&self) -> &str {
        "Prompt Engineer"
    }
    fn description(&self) -> &str {
        "Design, optimize, and evaluate prompts for LLM applications."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "design_prompt".to_string(),
            "optimize".to_string(),
            "evaluate".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Prompt Engineer agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "prompt-engineer" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
