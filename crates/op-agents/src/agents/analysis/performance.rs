//! Performance Engineer Agent

use async_trait::async_trait;
use std::fs;

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

pub struct PerformanceEngineerAgent {
    agent_id: String,
    profile: SecurityProfile,
}

impl PerformanceEngineerAgent {
    pub fn new(agent_id: String) -> Self {
        Self {
            agent_id,
            profile: SecurityProfile::read_only_analysis(
                "performance-engineer",
                vec!["top", "htop", "vmstat", "iostat"],
            ),
        }
    }

    fn system_stats(&self) -> Result<String, String> {
        let content = fs::read_to_string("/proc/vmstat")
            .map_err(|e| format!("Failed to read /proc/vmstat: {}", e))?;

        Ok(format!("System stats (/proc/vmstat):\n{}", content))
    }

    fn io_stats(&self) -> Result<String, String> {
        let content = fs::read_to_string("/proc/diskstats")
            .map_err(|e| format!("Failed to read /proc/diskstats: {}", e))?;

        Ok(format!("I/O stats (/proc/diskstats):\n{}", content))
    }

    fn memory_info(&self) -> Result<String, String> {
        let content = fs::read_to_string("/proc/meminfo")
            .map_err(|e| format!("Failed to read /proc/meminfo: {}", e))?;

        Ok(format!("Memory info (/proc/meminfo):\n{}", content))
    }

    fn cpu_info(&self) -> Result<String, String> {
        let content = fs::read_to_string("/proc/cpuinfo")
            .map_err(|e| format!("Failed to read /proc/cpuinfo: {}", e))?;

        Ok(format!("CPU info (/proc/cpuinfo):\n{}", content))
    }
}

#[async_trait]
impl AgentTrait for PerformanceEngineerAgent {
    fn agent_type(&self) -> &str {
        "performance-engineer"
    }
    fn name(&self) -> &str {
        "Performance Engineer"
    }
    fn description(&self) -> &str {
        "System performance analysis"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "vmstat".to_string(),
            "iostat".to_string(),
            "memory".to_string(),
            "cpu".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "vmstat" => self.system_stats(),
            "iostat" => self.io_stats(),
            "memory" => self.memory_info(),
            "cpu" => self.cpu_info(),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
