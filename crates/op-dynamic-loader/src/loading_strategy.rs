use async_trait::async_trait;
use std::sync::Arc;

use op_execution_tracker::{ExecutionContext, ExecutionTracker};

/// Loading strategy interface
#[async_trait]
pub trait LoadingStrategy: Send + Sync {
    /// Determine if a tool should be loaded
    async fn should_load(&self, tool_name: &str, context: &ExecutionContext) -> bool;

    /// Get load priority (0-100)
    async fn get_priority(&self, tool_name: &str) -> u8;

    /// Get cache TTL in seconds
    fn cache_ttl(&self, tool_name: &str) -> u64;
}

/// Smart loading strategy that considers execution patterns
pub struct SmartLoadingStrategy {
    execution_tracker: Arc<ExecutionTracker>,
    base_cache_ttl: u64,
}

impl SmartLoadingStrategy {
    pub fn new(execution_tracker: Arc<ExecutionTracker>, base_cache_ttl: u64) -> Self {
        Self {
            execution_tracker,
            base_cache_ttl,
        }
    }
}

#[async_trait]
impl LoadingStrategy for SmartLoadingStrategy {
    async fn should_load(&self, tool_name: &str, _context: &ExecutionContext) -> bool {
        // Always load if it's a critical tool
        if self.is_critical_tool(tool_name) {
            return true;
        }

        // Check recent execution history
        let recent_executions = self.execution_tracker.list_recent_completed(10).await;

        let recent_tool_executions = recent_executions
            .iter()
            .filter(|exec| exec.tool_name == tool_name)
            .count();

        // Load if recently used (last 10 executions)
        if recent_tool_executions > 0 {
            return true;
        }

        // Default: load on-demand
        true
    }

    async fn get_priority(&self, tool_name: &str) -> u8 {
        if self.is_critical_tool(tool_name) {
            return 100;
        }

        // Check execution frequency
        let recent_executions = self.execution_tracker.list_recent_completed(50).await;

        let tool_executions = recent_executions
            .iter()
            .filter(|exec| exec.tool_name == tool_name)
            .count();

        // Priority based on usage frequency
        match tool_executions {
            0..=2 => 20, // Low priority
            3..=5 => 50, // Medium priority
            _ => 80,     // High priority
        }
    }

    fn cache_ttl(&self, tool_name: &str) -> u64 {
        if self.is_critical_tool(tool_name) {
            // Critical tools stay loaded longer
            self.base_cache_ttl * 2
        } else {
            self.base_cache_ttl
        }
    }
}

impl SmartLoadingStrategy {
    fn is_critical_tool(&self, tool_name: &str) -> bool {
        // Define critical tools that should always be available
        let critical_tools = [
            "respond_to_user",
            "cannot_perform",
            "systemd_status",
            "file_read",
            "agent_status",
        ];

        critical_tools.contains(&tool_name)
    }
}
