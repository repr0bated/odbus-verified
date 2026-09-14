//! Test Automator Agent
//!
//! Expert test automation engineer specializing in creating comprehensive
//! test suites with high coverage and maintainability.

use async_trait::async_trait;
use simd_json::json;

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

/// Test Automator Agent
pub struct TestAutomatorAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl TestAutomatorAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("test-automator"),
            agent_id,
        }
    }

    fn generate_tests(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();

        let mut test_types = Vec::new();
        let mut frameworks = Vec::new();
        let mut strategies = Vec::new();

        let (lang, test_framework) = if input.contains("python") {
            ("Python", "pytest")
        } else if input.contains("javascript")
            || input.contains("typescript")
            || input.contains("react")
        {
            ("JavaScript/TypeScript", "Jest + React Testing Library")
        } else if input.contains("rust") {
            ("Rust", "built-in test framework")
        } else if input.contains("go") || input.contains("golang") {
            ("Go", "testing package + testify")
        } else {
            ("General", "language-appropriate framework")
        };

        frameworks.push(test_framework);

        if input.contains("unit") || input.contains("function") {
            test_types.push("Unit Tests");
            strategies.push("Test individual functions/methods in isolation");
            strategies.push("Mock external dependencies");
        }

        if input.contains("integration") || input.contains("api") {
            test_types.push("Integration Tests");
            strategies.push("Test component interactions");
            strategies.push("Use test databases/containers");
        }

        if input.contains("e2e") || input.contains("end-to-end") || input.contains("ui") {
            test_types.push("E2E Tests");
            frameworks.push("Playwright or Cypress");
            strategies.push("Test critical user journeys");
        }

        if test_types.is_empty() {
            test_types.push("Unit Tests");
            test_types.push("Integration Tests");
            strategies.push("Follow testing pyramid (many unit, fewer integration, few E2E)");
            strategies.push("Aim for 80%+ code coverage");
        }

        let result = json!({
            "test_plan": {
                "input": args.unwrap_or(""),
                "language": lang,
                "test_types": test_types,
                "recommended_frameworks": frameworks,
                "strategies": strategies
            },
            "test_structure": {
                "naming": "test_<what>_<scenario>_<expected_result>",
                "organization": "Mirror source code structure",
                "fixtures": "Shared setup in fixtures/conftest"
            },
            "best_practices": [
                "Arrange-Act-Assert (AAA) pattern",
                "One assertion per test (ideally)",
                "Independent and isolated tests",
                "Fast execution (unit tests < 1s each)",
                "Descriptive test names"
            ],
            "coverage_targets": {
                "line_coverage": "80%+",
                "branch_coverage": "75%+",
                "critical_paths": "100%"
            }
        });

        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for TestAutomatorAgent {
    fn agent_type(&self) -> &str {
        "test-automator"
    }
    fn name(&self) -> &str {
        "Test Automator"
    }
    fn description(&self) -> &str {
        "Expert test automation engineer specializing in creating comprehensive test suites."
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "generate_unit_tests".to_string(),
            "generate_integration_tests".to_string(),
            "generate_e2e_tests".to_string(),
            "analyze_coverage".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    fn get_status(&self) -> String {
        format!("Test Automator agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "test-automator" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }

        match self.generate_tests(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
