//! Orchestration Activity Plugin
//!
//! Provides a plugin interface for tracking orchestration activity.
//! Plugins receive notifications about:
//! - Tool executions (commands, file operations, etc.)
//! - LLM decisions and tool calls
//! - Session lifecycle events
//!
//! ## Use Cases
//!
//! - **Snowball Logging**: Immutable audit trail on snowball
//! - **Metrics/Observability**: Prometheus, Grafana integration
//! - **Alerting**: Real-time notifications for critical operations
//! - **Replay/Debugging**: Record and replay orchestration sessions
//!
//! ## Example
//!
//! ```rust,ignore
//! struct SnowballActivityPlugin { /* ... */ }
//!
//! #[async_trait]
//! impl OrchestrationActivityPlugin for SnowballActivityPlugin {
//!     async fn on_tool_executed(&self, event: ToolExecutedEvent) {
//!         // Write to snowball
//!         self.snowball.write_event(event).await;
//!     }
//! }
//! ```

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

// ============================================================================
// EVENT TYPES
// ============================================================================

/// Event emitted when a tool is executed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutedEvent {
    /// Unique event ID
    pub event_id: String,
    /// Session ID (user/chat session)
    pub session_id: String,
    /// Tool name that was executed
    pub tool_name: String,
    /// Tool category (shell, filesystem, dbus, etc.)
    pub tool_category: String,
    /// Input arguments (may be redacted for security)
    pub arguments: Value,
    /// Execution result
    pub result: ToolExecutionResult,
    /// Timestamp when execution started
    pub started_at: DateTime<Utc>,
    /// Timestamp when execution completed
    pub completed_at: DateTime<Utc>,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Additional metadata
    pub metadata: Value,
}

/// Result of tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionResult {
    /// Whether execution succeeded
    pub success: bool,
    /// Exit code (for shell commands)
    pub exit_code: Option<i32>,
    /// Output summary (truncated if large)
    pub output_summary: Option<String>,
    /// Error message if failed
    pub error: Option<String>,
    /// Output size in bytes
    pub output_bytes: usize,
}

/// Event emitted when LLM makes a decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmDecisionEvent {
    /// Unique event ID
    pub event_id: String,
    /// Session ID
    pub session_id: String,
    /// LLM provider used
    pub provider: String,
    /// Model used
    pub model: String,
    /// Tools that were called
    pub tool_calls: Vec<String>,
    /// Was hallucination detected?
    pub hallucination_detected: bool,
    /// Verification status
    pub verified: bool,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Token usage
    pub tokens_used: Option<TokenUsage>,
}

/// Token usage information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Event emitted for session lifecycle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    /// Session ID
    pub session_id: String,
    /// Event type
    pub event_type: SessionEventType,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Additional data
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEventType {
    Started,
    Ended,
    Paused,
    Resumed,
    Error,
}

// ============================================================================
// PLUGIN TRAIT
// ============================================================================

/// Plugin interface for receiving orchestration activity events
#[async_trait]
pub trait OrchestrationActivityPlugin: Send + Sync {
    /// Plugin name for identification
    fn name(&self) -> &str;

    /// Called when a tool is executed
    async fn on_tool_executed(&self, event: ToolExecutedEvent);

    /// Called when LLM makes a decision (optional)
    async fn on_llm_decision(&self, _event: LlmDecisionEvent) {
        // Default: no-op
    }

    /// Called for session lifecycle events (optional)
    async fn on_session_event(&self, _event: SessionEvent) {
        // Default: no-op
    }

    /// Called on plugin initialization
    async fn on_init(&self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Called on plugin shutdown
    async fn on_shutdown(&self) {
        // Default: no-op
    }
}

// ============================================================================
// PLUGIN REGISTRY
// ============================================================================

/// Registry for orchestration activity plugins
pub struct OrchestrationPluginRegistry {
    plugins: RwLock<Vec<Arc<dyn OrchestrationActivityPlugin>>>,
}

impl OrchestrationPluginRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            plugins: RwLock::new(Vec::new()),
        }
    }

    /// Register a plugin
    pub async fn register(
        &self,
        plugin: Arc<dyn OrchestrationActivityPlugin>,
    ) -> anyhow::Result<()> {
        let name = plugin.name().to_string();

        // Initialize the plugin
        plugin.on_init().await?;

        self.plugins.write().await.push(plugin);
        info!(plugin = %name, "Registered orchestration activity plugin");

        Ok(())
    }

    /// Emit a tool executed event to all plugins
    pub async fn emit_tool_executed(&self, event: ToolExecutedEvent) {
        let plugins = self.plugins.read().await;
        for plugin in plugins.iter() {
            plugin.on_tool_executed(event.clone()).await;
        }
    }

    /// Emit an LLM decision event to all plugins
    pub async fn emit_llm_decision(&self, event: LlmDecisionEvent) {
        let plugins = self.plugins.read().await;
        for plugin in plugins.iter() {
            plugin.on_llm_decision(event.clone()).await;
        }
    }

    /// Emit a session event to all plugins
    pub async fn emit_session_event(&self, event: SessionEvent) {
        let plugins = self.plugins.read().await;
        for plugin in plugins.iter() {
            plugin.on_session_event(event.clone()).await;
        }
    }

    /// Shutdown all plugins
    pub async fn shutdown(&self) {
        let plugins = self.plugins.read().await;
        for plugin in plugins.iter() {
            plugin.on_shutdown().await;
        }
    }

    /// Get number of registered plugins
    pub async fn plugin_count(&self) -> usize {
        self.plugins.read().await.len()
    }
}

impl Default for OrchestrationPluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL REGISTRY
// ============================================================================

// Global orchestration plugin registry (initialized eagerly)
static ORCHESTRATION_REGISTRY: std::sync::OnceLock<Arc<OrchestrationPluginRegistry>> =
    std::sync::OnceLock::new();

/// Initialize the global orchestration plugin registry (call once at startup)
pub fn init_orchestration_registry() {
    ORCHESTRATION_REGISTRY
        .set(Arc::new(OrchestrationPluginRegistry::new()))
        .unwrap_or_else(|_| panic!("Orchestration registry already initialized"));
}

/// Get the global orchestration plugin registry
pub fn get_orchestration_registry() -> Arc<OrchestrationPluginRegistry> {
    ORCHESTRATION_REGISTRY
        .get()
        .expect("Orchestration registry not initialized")
        .clone()
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Create a ToolExecutedEvent from execution data
#[allow(clippy::too_many_arguments)]
pub fn create_tool_event(
    session_id: &str,
    tool_name: &str,
    tool_category: &str,
    arguments: Value,
    success: bool,
    exit_code: Option<i32>,
    output: Option<&str>,
    error: Option<&str>,
    started_at: DateTime<Utc>,
    duration_ms: u64,
) -> ToolExecutedEvent {
    let output_bytes = output.map(|s| s.len()).unwrap_or(0);
    let output_summary = output.map(|s| {
        if s.len() > 500 {
            format!("{}... ({} bytes total)", &s[..500], s.len())
        } else {
            s.to_string()
        }
    });

    ToolExecutedEvent {
        event_id: uuid::Uuid::new_v4().to_string(),
        session_id: session_id.to_string(),
        tool_name: tool_name.to_string(),
        tool_category: tool_category.to_string(),
        arguments,
        result: ToolExecutionResult {
            success,
            exit_code,
            output_summary,
            error: error.map(|s| s.to_string()),
            output_bytes,
        },
        started_at,
        completed_at: Utc::now(),
        duration_ms,
        metadata: Value::null(),
    }
}

// ============================================================================
// EXAMPLE PLUGINS
// ============================================================================

/// Simple logging plugin (for development/debugging)
pub struct LoggingActivityPlugin;

#[async_trait]
impl OrchestrationActivityPlugin for LoggingActivityPlugin {
    fn name(&self) -> &str {
        "logging"
    }

    async fn on_tool_executed(&self, event: ToolExecutedEvent) {
        info!(
            event_id = %event.event_id,
            session_id = %event.session_id,
            tool = %event.tool_name,
            category = %event.tool_category,
            success = %event.result.success,
            duration_ms = %event.duration_ms,
            "Tool executed"
        );
    }

    async fn on_llm_decision(&self, event: LlmDecisionEvent) {
        info!(
            event_id = %event.event_id,
            session_id = %event.session_id,
            provider = %event.provider,
            model = %event.model,
            tools_called = ?event.tool_calls,
            verified = %event.verified,
            "LLM decision"
        );
    }

    async fn on_session_event(&self, event: SessionEvent) {
        info!(
            session_id = %event.session_id,
            event_type = ?event.event_type,
            "Session event"
        );
    }
}

/// Metrics plugin (placeholder for Prometheus/etc integration)
pub struct MetricsActivityPlugin {
    // Counter metrics would go here
}

impl MetricsActivityPlugin {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for MetricsActivityPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OrchestrationActivityPlugin for MetricsActivityPlugin {
    fn name(&self) -> &str {
        "metrics"
    }

    async fn on_tool_executed(&self, event: ToolExecutedEvent) {
        // Increment counters, record histograms, etc.
        debug!(
            tool = %event.tool_name,
            duration = %event.duration_ms,
            "Recording tool execution metrics"
        );
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct CountingPlugin {
        count: AtomicU32,
    }

    impl CountingPlugin {
        fn new() -> Self {
            Self {
                count: AtomicU32::new(0),
            }
        }

        fn get_count(&self) -> u32 {
            self.count.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl OrchestrationActivityPlugin for CountingPlugin {
        fn name(&self) -> &str {
            "counting"
        }

        async fn on_tool_executed(&self, _event: ToolExecutedEvent) {
            self.count.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[tokio::test]
    async fn test_plugin_registration() {
        let registry = OrchestrationPluginRegistry::new();
        let plugin = Arc::new(CountingPlugin::new());

        registry.register(plugin.clone()).await.unwrap();
        assert_eq!(registry.plugin_count().await, 1);
    }

    #[tokio::test]
    async fn test_event_emission() {
        let registry = OrchestrationPluginRegistry::new();
        let plugin = Arc::new(CountingPlugin::new());
        registry.register(plugin.clone()).await.unwrap();

        let event = create_tool_event(
            "session1",
            "test_tool",
            "test",
            simd_json::json!({}),
            true,
            Some(0),
            Some("output"),
            None,
            Utc::now(),
            100,
        );

        registry.emit_tool_executed(event).await;
        assert_eq!(plugin.get_count(), 1);

        // Emit more events
        for _ in 0..5 {
            let event = create_tool_event(
                "session1",
                "test_tool",
                "test",
                simd_json::json!({}),
                true,
                None,
                None,
                None,
                Utc::now(),
                50,
            );
            registry.emit_tool_executed(event).await;
        }

        assert_eq!(plugin.get_count(), 6);
    }
}
