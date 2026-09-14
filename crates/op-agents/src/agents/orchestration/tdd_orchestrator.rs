//! TDD Orchestrator Agent

use async_trait::async_trait;

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::{profiles::presets, SecurityProfile};

pub struct TddOrchestratorAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl TddOrchestratorAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: presets::tdd_orchestrator(),
        }
    }

    fn plan_red_phase(&self) -> Result<String, String> {
        Ok("TDD Red Phase Plan:\n\
            1. Write a failing test for the new feature\n\
            2. Run tests to verify it fails\n\
            3. Ensure the test failure message is clear\n\
            \n\
            Subagents to invoke: test-automator, debugger"
            .to_string())
    }

    fn plan_green_phase(&self) -> Result<String, String> {
        Ok("TDD Green Phase Plan:\n\
            1. Write minimal code to pass the test\n\
            2. Run tests to verify they pass\n\
            3. Ensure no other tests broke\n\
            \n\
            Subagents to invoke: code-reviewer, test-automator"
            .to_string())
    }

    fn plan_refactor_phase(&self) -> Result<String, String> {
        Ok("TDD Refactor Phase Plan:\n\
            1. Identify code smells and duplication\n\
            2. Apply refactoring patterns\n\
            3. Run tests after each change\n\
            4. Ensure code quality improved\n\
            \n\
            Subagents to invoke: code-reviewer, test-automator, debugger"
            .to_string())
    }

    fn full_cycle(&self) -> Result<String, String> {
        Ok("TDD Full Cycle Plan:\n\
            \n\
            Phase 1 - RED:\n\
            - Write failing test\n\
            - Verify test fails correctly\n\
            \n\
            Phase 2 - GREEN:\n\
            - Write minimal implementation\n\
            - Verify all tests pass\n\
            \n\
            Phase 3 - REFACTOR:\n\
            - Improve code quality\n\
            - Maintain test coverage\n\
            \n\
            Coordination: Sequential execution with validation gates"
            .to_string())
    }
}

#[async_trait]
impl AgentTrait for TddOrchestratorAgent {
    fn agent_type(&self) -> &str {
        "tdd-orchestrator"
    }
    fn name(&self) -> &str {
        "TDD Orchestrator"
    }
    fn description(&self) -> &str {
        "Test-Driven Development workflow orchestration"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "red".to_string(),
            "green".to_string(),
            "refactor".to_string(),
            "cycle".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "red" => self.plan_red_phase(),
            "green" => self.plan_green_phase(),
            "refactor" => self.plan_refactor_phase(),
            "cycle" => self.full_cycle(),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
