//! Data Scientist Agent - Data analysis, visualization, experimentation

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct DataScientistAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl DataScientistAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("data-scientist"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("analysis") || input.contains("explore") {
            recommendations.push("Start with univariate analysis before multivariate");
            recommendations.push("Check for missing values and outliers");
            recommendations.push("Visualize distributions and correlations");
        }
        if input.contains("model") || input.contains("predict") {
            recommendations.push("Establish baseline with simple models first");
            recommendations.push("Use cross-validation for robust evaluation");
            recommendations.push("Feature importance analysis for interpretability");
        }
        if input.contains("experiment") || input.contains("ab") {
            recommendations.push("Define success metrics before experiment");
            recommendations.push("Calculate required sample size");
            recommendations.push("Account for multiple comparison correction");
        }
        if recommendations.is_empty() {
            recommendations.push("Define clear hypothesis or question");
            recommendations.push("Understand data provenance and quality");
            recommendations.push("Choose appropriate statistical methods");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "tools": ["pandas", "numpy", "scikit-learn", "matplotlib", "seaborn", "jupyter"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for DataScientistAgent {
    fn agent_type(&self) -> &str {
        "data-scientist"
    }
    fn name(&self) -> &str {
        "Data Scientist"
    }
    fn description(&self) -> &str {
        "Analyze data, build models, run experiments, and derive insights."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "explore_data".to_string(),
            "build_model".to_string(),
            "run_experiment".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Data Scientist agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "data-scientist" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
