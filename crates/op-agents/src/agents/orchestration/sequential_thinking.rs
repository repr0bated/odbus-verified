//! Sequential Thinking Agent
//!
//! Helper agent for breaking down complex tasks into sequential steps.

use async_trait::async_trait;
use simd_json::json;

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

pub struct SequentialThinkingAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl SequentialThinkingAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::orchestration("sequential-thinking", vec!["*"]),
        }
    }

    fn analyze(&self, input: &str) -> Result<String, String> {
        // In a real implementation, this might use an LLM or stricter logic.
        // For now, it scaffolds a thinking process.
        let steps = json!({
            "thought_process": {
                "input": input,
                "analysis": "Decomposing task into sequential steps...",
                "steps": [
                    "1. Identify core intent",
                    "2. Check constraints",
                    "3. Formulate plan",
                    "4. Execute step-by-step"
                ],
                "recommendation": "Proceed with step 1"
            }
        });
        Ok(simd_json::to_string_pretty(&steps).unwrap())
    }
}

#[async_trait]
impl AgentTrait for SequentialThinkingAgent {
    fn agent_type(&self) -> &str {
        "sequential-thinking"
    }
    fn name(&self) -> &str {
        "Sequential Thinking"
    }
    fn description(&self) -> &str {
        "Helps break down complex problems into linear, sequential steps"
    }

    fn operations(&self) -> Vec<String> {
        vec!["analyze".to_string()]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let input = task.args.as_deref().unwrap_or("");

        let result = match task.operation.as_str() {
            "analyze" => self.analyze(input),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
