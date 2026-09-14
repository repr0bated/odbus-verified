//! Observability Engineer Agent

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct ObservabilityEngineerAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl ObservabilityEngineerAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("observability-engineer"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("metrics") || input.contains("prometheus") {
            recommendations.push("Use RED method (Rate, Errors, Duration) for services");
            recommendations.push("Use USE method (Utilization, Saturation, Errors) for resources");
            recommendations.push("Set up proper alerting thresholds");
        }
        if input.contains("logging") || input.contains("log") {
            recommendations.push("Use structured logging (JSON format)");
            recommendations.push("Include correlation IDs across services");
            recommendations.push("Set appropriate log levels");
        }
        if input.contains("tracing") || input.contains("trace") {
            recommendations.push("Implement distributed tracing with OpenTelemetry");
            recommendations.push("Add span attributes for debugging context");
            recommendations.push("Sample appropriately for high-volume services");
        }
        if recommendations.is_empty() {
            recommendations.push("Implement all three pillars: metrics, logs, traces");
            recommendations.push("Create service-level dashboards");
            recommendations.push("Define SLIs and SLOs");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "tools": { "metrics": ["Prometheus", "Grafana"], "logs": ["Loki", "ELK"], "traces": ["Jaeger", "Tempo"] }
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for ObservabilityEngineerAgent {
    fn agent_type(&self) -> &str {
        "observability-engineer"
    }
    fn name(&self) -> &str {
        "Observability Engineer"
    }
    fn description(&self) -> &str {
        "Set up monitoring, logging, and tracing infrastructure."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "setup_metrics".to_string(),
            "configure_logging".to_string(),
            "implement_tracing".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("Observability Engineer agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "observability-engineer" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
