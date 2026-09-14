//! ARM Cortex Expert Agent

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;
use async_trait::async_trait;
use simd_json::json;

pub struct ARMCortexExpertAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl ARMCortexExpertAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            profile: SecurityProfile::content_generation("arm-cortex-expert"),
            agent_id,
        }
    }

    fn analyze(&self, args: Option<&str>) -> Result<String, String> {
        let input = args.unwrap_or("").to_lowercase();
        let mut recommendations = Vec::new();

        if input.contains("interrupt") || input.contains("isr") {
            recommendations.push("Keep ISRs short - defer work to main loop");
            recommendations.push("Use proper priority configuration (NVIC)");
            recommendations.push("Disable interrupts carefully with critical sections");
        }
        if input.contains("power") || input.contains("sleep") {
            recommendations.push("Use appropriate sleep modes for power savings");
            recommendations.push("Configure wake-up sources correctly");
            recommendations.push("Disable unused peripherals");
        }
        if input.contains("memory") || input.contains("dma") {
            recommendations.push("Use DMA for bulk data transfers");
            recommendations.push("Align data structures for efficient access");
            recommendations.push("Consider cache coherency for Cortex-M7");
        }
        if recommendations.is_empty() {
            recommendations.push("Use CMSIS for portable code");
            recommendations.push("Configure clocks appropriately for application");
            recommendations.push("Implement watchdog for reliability");
        }

        let result = json!({
            "analysis": { "input": args.unwrap_or(""), "recommendations": recommendations },
            "cortex_variants": ["Cortex-M0/M0+", "Cortex-M3", "Cortex-M4", "Cortex-M7", "Cortex-M33"]
        });
        Ok(simd_json::to_string_pretty(&result).unwrap_or_default())
    }
}

#[async_trait]
impl AgentTrait for ARMCortexExpertAgent {
    fn agent_type(&self) -> &str {
        "arm-cortex-expert"
    }
    fn name(&self) -> &str {
        "ARM Cortex Expert"
    }
    fn description(&self) -> &str {
        "Embedded systems development for ARM Cortex-M microcontrollers."
    }
    fn operations(&self) -> Vec<String> {
        vec![
            "configure_peripheral".to_string(),
            "optimize_power".to_string(),
            "debug".to_string(),
            "analyze".to_string(),
        ]
    }
    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }
    fn get_status(&self) -> String {
        format!("ARM Cortex Expert agent {} is running", self.agent_id)
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        if task.task_type != "arm-cortex-expert" {
            return Err(format!("Invalid task type: {}", task.task_type));
        }
        match self.analyze(task.args.as_deref()) {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
