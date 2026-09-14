//! Tool executor with timeout and concurrency control

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tracing::{debug, info, warn};

use crate::ToolRegistry;
use op_core::{ToolRequest, ToolResult};

/// Configuration for tool execution
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Maximum concurrent tool executions
    pub max_concurrent: usize,
    /// Default timeout for tool execution (ms)
    pub default_timeout_ms: u64,
    /// Maximum timeout allowed (ms)
    pub max_timeout_ms: u64,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 10,
            default_timeout_ms: 30000,
            max_timeout_ms: 300000, // 5 minutes
        }
    }
}

/// Tool executor with concurrency and timeout control
pub struct ToolExecutor {
    registry: ToolRegistry,
    config: ExecutorConfig,
    semaphore: Arc<Semaphore>,
}

impl ToolExecutor {
    /// Create a new tool executor
    pub fn new(registry: ToolRegistry, config: ExecutorConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent));
        Self {
            registry,
            config,
            semaphore,
        }
    }

    /// Create with default configuration
    pub fn with_defaults(registry: ToolRegistry) -> Self {
        Self::new(registry, ExecutorConfig::default())
    }

    /// Execute a tool with timeout
    pub async fn execute(&self, request: ToolRequest) -> ToolResult {
        let start = std::time::Instant::now();

        // Determine timeout
        let timeout_ms = request
            .timeout_ms
            .unwrap_or(self.config.default_timeout_ms)
            .min(self.config.max_timeout_ms);

        debug!(
            "Executing tool '{}' with timeout {}ms",
            request.tool_name, timeout_ms
        );

        // Acquire semaphore permit
        let _permit = match self.semaphore.acquire().await {
            Ok(permit) => permit,
            Err(_) => {
                return ToolResult::error(
                    &request.id,
                    "Executor shutdown",
                    start.elapsed().as_millis() as u64,
                );
            }
        };

        // Execute with timeout
        let duration = Duration::from_millis(timeout_ms);
        debug!(
            "About to call registry.execute for tool '{}' with timeout {}ms",
            request.tool_name, timeout_ms
        );
        let timeout_result = timeout(duration, self.registry.execute(request.clone())).await;
        debug!(
            "Registry.execute completed for tool '{}' - success: {}",
            request.tool_name,
            timeout_result.is_ok()
        );

        match timeout_result {
            Ok(result) => {
                debug!(
                    "Tool '{}' executed successfully in {}ms",
                    request.tool_name,
                    start.elapsed().as_millis()
                );
                result
            }
            Err(_) => {
                warn!(
                    "Tool '{}' timed out after {}ms",
                    request.tool_name, timeout_ms
                );
                ToolResult::error(
                    &request.id,
                    format!("Tool execution timed out after {}ms", timeout_ms),
                    start.elapsed().as_millis() as u64,
                )
            }
        }
    }

    /// Execute multiple tools concurrently
    pub async fn execute_batch(&self, requests: Vec<ToolRequest>) -> Vec<ToolResult> {
        let futures: Vec<_> = requests.into_iter().map(|req| self.execute(req)).collect();

        futures::future::join_all(futures).await
    }

    /// Get current concurrency usage
    pub fn current_usage(&self) -> usize {
        self.config.max_concurrent - self.semaphore.available_permits()
    }

    /// Get available permits
    pub fn available(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// Get registry reference
    pub fn registry(&self) -> &ToolRegistry {
        &self.registry
    }
}

impl Clone for ToolExecutor {
    fn clone(&self) -> Self {
        Self {
            registry: self.registry.clone(),
            config: self.config.clone(),
            semaphore: Arc::clone(&self.semaphore),
        }
    }
}
