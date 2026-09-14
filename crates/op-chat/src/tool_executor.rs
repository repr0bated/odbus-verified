//! Tool Executor with Tracking and Rate Limiting
//!
//! Wraps tool execution with:
//! - Accountability tracking (audit log)
//! - Rate limiting per session
//! - Execution metrics and statistics
//! - Integration with ForcedToolPipeline for anti-hallucination
//!
//! ## Security
//!
//! All tool executions are:
//! 1. Tracked for audit purposes
//! 2. Rate limited per session (default: 60/minute)
//! 3. Logged with timing and results
//!
//! ## Usage
//!
//! Use `TrackedToolExecutor` for all LLM-driven tool execution to ensure
//! proper tracking and rate limiting. Do NOT bypass this for LLM operations.

use anyhow::Result;
use chrono::Utc;
use op_execution_tracker::{ExecutionContext, ExecutionResult, ExecutionTracker};
use op_tools::ToolRegistry;
use simd_json::OwnedValue as Value;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Semaphore};
use tracing::{error, info, warn};

// ============================================================================
// RATE LIMITER
// ============================================================================

/// Rate limit configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum executions per minute per session
    pub max_per_minute: u32,
    /// Maximum executions per hour per session
    pub max_per_hour: u32,
    /// Maximum concurrent executions globally
    pub max_concurrent: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_per_minute: 60,
            max_per_hour: 500,
            max_concurrent: 10,
        }
    }
}

/// Rate limiter state for a session
struct SessionRateState {
    minute_count: u32,
    minute_window_start: Instant,
    hour_count: u32,
    hour_window_start: Instant,
}

impl SessionRateState {
    fn new() -> Self {
        Self {
            minute_count: 0,
            minute_window_start: Instant::now(),
            hour_count: 0,
            hour_window_start: Instant::now(),
        }
    }

    fn check_and_increment(&mut self, config: &RateLimitConfig) -> Result<(), String> {
        let now = Instant::now();

        // Reset minute window if needed
        if now.duration_since(self.minute_window_start) > Duration::from_secs(60) {
            self.minute_count = 0;
            self.minute_window_start = now;
        }

        // Reset hour window if needed
        if now.duration_since(self.hour_window_start) > Duration::from_secs(3600) {
            self.hour_count = 0;
            self.hour_window_start = now;
        }

        // Check limits
        if self.minute_count >= config.max_per_minute {
            return Err(format!(
                "Rate limit exceeded: {} executions per minute",
                config.max_per_minute
            ));
        }

        if self.hour_count >= config.max_per_hour {
            return Err(format!(
                "Rate limit exceeded: {} executions per hour",
                config.max_per_hour
            ));
        }

        // Increment counters
        self.minute_count += 1;
        self.hour_count += 1;

        Ok(())
    }
}

// ============================================================================
// TRACKED TOOL EXECUTOR
// ============================================================================

/// Tool executor with built-in tracking, rate limiting, and metrics
pub struct TrackedToolExecutor {
    registry: Arc<ToolRegistry>,
    tracker: Arc<ExecutionTracker>,
    rate_config: RateLimitConfig,
    session_rates: RwLock<HashMap<String, SessionRateState>>,
    concurrent_count: AtomicU64,
    concurrency_semaphore: Arc<Semaphore>,
}

impl TrackedToolExecutor {
    /// Create a new tracked executor
    pub fn new(registry: Arc<ToolRegistry>, tracker: Arc<ExecutionTracker>) -> Self {
        let config = RateLimitConfig::default();
        let semaphore = Arc::new(Semaphore::new(config.max_concurrent as usize));
        Self {
            registry,
            tracker,
            rate_config: config,
            session_rates: RwLock::new(HashMap::new()),
            concurrent_count: AtomicU64::new(0),
            concurrency_semaphore: semaphore,
        }
    }

    /// Create with custom rate limit config
    pub fn with_rate_config(
        registry: Arc<ToolRegistry>,
        tracker: Arc<ExecutionTracker>,
        rate_config: RateLimitConfig,
    ) -> Self {
        let semaphore = Arc::new(Semaphore::new(rate_config.max_concurrent as usize));
        Self {
            registry,
            tracker,
            rate_config,
            session_rates: RwLock::new(HashMap::new()),
            concurrent_count: AtomicU64::new(0),
            concurrency_semaphore: semaphore,
        }
    }

    /// Check rate limits for a session
    async fn check_rate_limit(&self, session_id: &str) -> Result<(), String> {
        // Check concurrent limit
        let concurrent = self.concurrent_count.load(Ordering::Relaxed);
        if concurrent >= self.rate_config.max_concurrent as u64 {
            return Err(format!(
                "Too many concurrent executions: {} (max: {})",
                concurrent, self.rate_config.max_concurrent
            ));
        }

        // Check per-session limits
        let mut rates = self.session_rates.write().await;
        let state = rates
            .entry(session_id.to_string())
            .or_insert_with(SessionRateState::new);

        state.check_and_increment(&self.rate_config)
    }

    /// Execute a tool with full tracking and rate limiting
    pub async fn execute(
        &self,
        tool_name: &str,
        arguments: Value,
        initiated_by: Option<String>,
    ) -> Result<TrackedResult> {
        let session_id = initiated_by.as_deref().unwrap_or("anonymous");

        // Check rate limits
        if let Err(e) = self.check_rate_limit(session_id).await {
            error!(session = %session_id, error = %e, "Rate limit exceeded");
            return Err(anyhow::anyhow!("Rate limit exceeded: {}", e));
        }

        // Acquire semaphore permit — actually blocks when at concurrency limit
        let _permit = self
            .concurrency_semaphore
            .acquire()
            .await
            .map_err(|_| anyhow::anyhow!("Concurrency semaphore closed"))?;

        // Increment concurrent counter (for metrics)
        self.concurrent_count.fetch_add(1, Ordering::Relaxed);
        let _guard = ConcurrentGuard {
            counter: &self.concurrent_count,
        };

        // Create execution context
        let mut context = ExecutionContext::new(tool_name);

        // Store input arguments and initiator in metadata
        let mut metadata = simd_json::value::owned::Object::new();
        metadata.insert("arguments".to_string(), arguments.clone());
        if let Some(initiator) = &initiated_by {
            metadata.insert("initiated_by".to_string(), Value::String(initiator.clone()));
        }
        context.set_metadata(Value::Object(Box::new(metadata)));

        // Start tracking
        let execution_record = self
            .tracker
            .start_execution(tool_name, Some(arguments.clone()), initiated_by.clone())
            .await;
        let execution_id = execution_record.id.clone();

        info!(
            execution_id = %execution_id,
            tool = %tool_name,
            session = %session_id,
            "Starting tool execution"
        );

        let start_time = Instant::now();

        // Get tool from registry
        let tool = self
            .registry
            .get(tool_name)
            .await
            .ok_or_else(|| anyhow::anyhow!("Tool '{}' not found in registry", tool_name));

        let execution_result = match tool {
            Ok(t) => {
                // Execute the tool
                match t.execute(arguments.clone()).await {
                    Ok(val) => {
                        let duration = start_time.elapsed().as_millis() as u64;
                        info!(
                            tool = %tool_name,
                            duration_ms = %duration,
                            "Tool execution succeeded"
                        );
                        ExecutionResult {
                            success: true,
                            result: Some(val),
                            error: None,
                            duration_ms: duration,
                            finished_at: Utc::now(),
                        }
                    }
                    Err(e) => {
                        let duration = start_time.elapsed().as_millis() as u64;
                        error!(
                            tool = %tool_name,
                            error = %e,
                            duration_ms = %duration,
                            "Tool execution failed"
                        );
                        ExecutionResult {
                            success: false,
                            result: None,
                            error: Some(e.to_string()),
                            duration_ms: duration,
                            finished_at: Utc::now(),
                        }
                    }
                }
            }
            Err(e) => {
                let duration = start_time.elapsed().as_millis() as u64;
                ExecutionResult {
                    success: false,
                    result: None,
                    error: Some(e.to_string()),
                    duration_ms: duration,
                    finished_at: Utc::now(),
                }
            }
        };

        // Complete tracking
        if execution_result.success {
            self.tracker
                .complete_execution(
                    &execution_id,
                    simd_json::to_string(&execution_result.result).ok(),
                )
                .await;
        } else {
            self.tracker
                .fail_execution(
                    &execution_id,
                    execution_result
                        .error
                        .clone()
                        .unwrap_or("Unknown error".to_string()),
                )
                .await;
        }

        Ok(TrackedResult {
            result: execution_result,
            execution_id,
        })
    }

    /// Execute multiple tools in sequence with rate limiting
    pub async fn execute_sequence(
        &self,
        tools: Vec<(String, Value)>,
        initiated_by: Option<String>,
    ) -> Vec<TrackedResult> {
        let mut results = Vec::new();

        for item in tools {
            let tool_name = item.0;
            let arguments = item.1;
            let result = self
                .execute(&tool_name, arguments, initiated_by.clone())
                .await;

            match result {
                Ok(tracked) => {
                    let should_continue = tracked.success();
                    results.push(tracked);
                    if !should_continue {
                        warn!(
                            tool = %tool_name,
                            "Stopping sequence due to failed execution"
                        );
                        break;
                    }
                }
                Err(e) => {
                    error!(error = %e, tool = %tool_name, "Execution error in sequence");
                    break;
                }
            }
        }

        results
    }

    /// Get execution history (recent completed executions)
    pub async fn get_history(&self, limit: usize) -> Vec<Value> {
        let records = self.tracker.get_recent(limit).await;
        records
            .into_iter()
            .map(|r| {
                simd_json::json!({
                    "id": r.id,
                    "tool_name": r.tool_name,
                    "status": format!("{:?}", r.status),
                    "initiated_by": r.initiated_by,
                })
            })
            .collect()
    }

    /// Get execution statistics
    pub async fn get_stats(&self) -> Value {
        let stats = self.tracker.get_stats().await;
        simd_json::json!({
            "total_executions": stats.total_executions,
            "successful_executions": stats.successful_executions,
            "failed_executions": stats.failed_executions,
            "average_duration_ms": stats.average_duration_ms(),
            "success_rate": stats.success_rate(),
            "concurrent": self.concurrent_count.load(Ordering::Relaxed),
            "max_concurrent": self.rate_config.max_concurrent,
        })
    }

    /// Get rate limit status for a session
    pub async fn get_rate_status(&self, session_id: &str) -> Value {
        let rates = self.session_rates.read().await;
        if let Some(state) = rates.get(session_id) {
            simd_json::json!({
                "session_id": session_id,
                "minute_count": state.minute_count,
                "max_per_minute": self.rate_config.max_per_minute,
                "hour_count": state.hour_count,
                "max_per_hour": self.rate_config.max_per_hour,
                "concurrent": self.concurrent_count.load(Ordering::Relaxed),
                "max_concurrent": self.rate_config.max_concurrent
            })
        } else {
            simd_json::json!({
                "session_id": session_id,
                "minute_count": 0,
                "max_per_minute": self.rate_config.max_per_minute,
                "hour_count": 0,
                "max_per_hour": self.rate_config.max_per_hour,
                "concurrent": self.concurrent_count.load(Ordering::Relaxed),
                "max_concurrent": self.rate_config.max_concurrent
            })
        }
    }

    /// Clear rate limit state for a session (admin only)
    pub async fn clear_rate_limit(&self, session_id: &str) {
        let mut rates = self.session_rates.write().await;
        rates.remove(session_id);
    }

    /// Get tracker reference
    pub fn tracker(&self) -> &Arc<ExecutionTracker> {
        &self.tracker
    }

    /// Get registry reference
    pub fn registry(&self) -> &Arc<ToolRegistry> {
        &self.registry
    }
}

// ============================================================================
// HELPER STRUCTS
// ============================================================================

/// Guard to decrement concurrent counter on drop
struct ConcurrentGuard<'a> {
    counter: &'a AtomicU64,
}

impl Drop for ConcurrentGuard<'_> {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Result with tracking information
#[derive(Debug)]
pub struct TrackedResult {
    /// The actual tool result
    pub result: ExecutionResult,
    /// Execution ID for audit trail
    pub execution_id: String,
}

impl TrackedResult {
    pub fn success(&self) -> bool {
        self.result.success
    }

    pub fn content(&self) -> &Option<Value> {
        &self.result.result
    }

    pub fn error(&self) -> Option<&String> {
        self.result.error.as_ref()
    }

    pub fn duration_ms(&self) -> u64 {
        self.result.duration_ms
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.max_per_minute, 60);
        assert_eq!(config.max_per_hour, 500);
        assert_eq!(config.max_concurrent, 10);
    }

    #[test]
    fn test_session_rate_state() {
        let config = RateLimitConfig {
            max_per_minute: 3,
            max_per_hour: 10,
            max_concurrent: 5,
        };

        let mut state = SessionRateState::new();

        // First 3 should pass
        assert!(state.check_and_increment(&config).is_ok());
        assert!(state.check_and_increment(&config).is_ok());
        assert!(state.check_and_increment(&config).is_ok());

        // 4th should fail
        assert!(state.check_and_increment(&config).is_err());
    }
}
