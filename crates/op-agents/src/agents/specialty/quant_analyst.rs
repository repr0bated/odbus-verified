//! Quant Analyst Agent

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct QuantAnalystAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl QuantAnalystAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("quant-analyst"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("backtest") || input.contains("strategy") {
            recommendations.push("Use walk-forward optimization to avoid overfitting");
            recommendations.push("Account for transaction costs and slippage");
            recommendations.push("Test across multiple market regimes");
        }
        if input.contains("risk") {
            recommendations.push("Calculate VaR and Expected Shortfall");
            recommendations.push("Monitor portfolio correlations");
            recommendations.push("Implement position sizing rules");
        }
        if recommendations.is_empty() {
            recommendations.push("Start with robust data cleaning and validation");
            recommendations.push("Use proper statistical significance tests");
            recommendations.push("Document all assumptions and limitations");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "tools": ["Python (pandas, numpy)", "QuantLib", "zipline", "backtrader"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for QuantAnalystAgent {
    fn agent_type(&self) -> &str {
        "quant-analyst"
    }
    fn name(&self) -> &str {
        "Quant Analyst"
    }
    fn description(&self) -> &str {
        "Quantitative analysis and algorithmic trading strategies."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "backtest".to_string(),
            "analyze_risk".to_string(),
            "develop_strategy".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Quant Analyst agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "quant-analyst" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
