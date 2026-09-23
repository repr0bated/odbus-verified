//! Legacy debugger diagnostics require an admitted log/process projection.

use async_trait::async_trait;

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

pub struct DebuggerAgent {
    profile: SecurityProfile,
}

impl DebuggerAgent {
    pub fn new(_agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::read_only_analysis("debugger", vec![]),
        }
    }
}

#[async_trait]
impl AgentTrait for DebuggerAgent {
    fn agent_type(&self) -> &str {
        "debugger"
    }
    fn name(&self) -> &str {
        "Debugger"
    }
    fn description(&self) -> &str {
        "Debugger diagnostics require a canonical projection"
    }
    fn operations(&self) -> Vec<String> {
        Vec::new()
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        Ok(TaskResult::failure(
            &task.operation,
            "Debugger diagnostics require a canonical plugin projection".to_string(),
        ))
    }
}
