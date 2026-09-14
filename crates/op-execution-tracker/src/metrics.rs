use prometheus::{Histogram, HistogramOpts, IntCounter, IntGauge, Registry};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Execution metrics collector
#[derive(Clone)]
pub struct ExecutionMetrics {
    /// Total executions started
    executions_started: IntCounter,

    /// Currently active executions
    active_executions: IntGauge,

    /// Executions completed successfully
    executions_succeeded: IntCounter,

    /// Executions failed
    executions_failed: IntCounter,

    /// Execution duration histogram
    execution_duration: Histogram,

    /// Status transitions
    status_transitions: IntCounter,

    /// Registry for custom metrics
    registry: Arc<RwLock<Registry>>,
}

impl ExecutionMetrics {
    /// Create new metrics collector
    pub fn new() -> Result<Self, prometheus::Error> {
        let registry = Registry::new();

        let executions_started = IntCounter::new(
            "mcp_executions_started_total",
            "Total number of executions started",
        )?;
        registry.register(Box::new(executions_started.clone()))?;

        let active_executions = IntGauge::new(
            "mcp_active_executions",
            "Number of currently active executions",
        )?;
        registry.register(Box::new(active_executions.clone()))?;

        let executions_succeeded = IntCounter::new(
            "mcp_executions_succeeded_total",
            "Total number of successfully completed executions",
        )?;
        registry.register(Box::new(executions_succeeded.clone()))?;

        let executions_failed = IntCounter::new(
            "mcp_executions_failed_total",
            "Total number of failed executions",
        )?;
        registry.register(Box::new(executions_failed.clone()))?;

        let execution_duration = Histogram::with_opts(
            HistogramOpts::new(
                "mcp_execution_duration_seconds",
                "Execution duration in seconds",
            )
            .buckets(vec![0.01, 0.05, 0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0]),
        )?;
        registry.register(Box::new(execution_duration.clone()))?;

        let status_transitions = IntCounter::new(
            "mcp_status_transitions_total",
            "Total number of execution status transitions",
        )?;
        registry.register(Box::new(status_transitions.clone()))?;

        Ok(Self {
            executions_started,
            active_executions,
            executions_succeeded,
            executions_failed,
            execution_duration,
            status_transitions,
            registry: Arc::new(RwLock::new(registry)),
        })
    }

    pub fn execution_started(&self, _tool_name: &str) {
        self.executions_started.inc();
        self.active_executions.inc();
    }

    pub fn status_updated(&self, _tool_name: &str, _status: &str) {
        // Keep this lightweight to avoid high-cardinality labels.
        self.status_transitions.inc();
    }

    pub fn execution_succeeded(&self, _tool_name: &str, duration_ms: u64) {
        self.executions_succeeded.inc();
        self.execution_duration.observe(duration_ms as f64 / 1000.0);
        self.active_executions.dec();
    }

    pub fn execution_failed(&self, _tool_name: &str) {
        self.executions_failed.inc();
        self.active_executions.dec();
    }

    /// Get metrics registry for scraping
    pub async fn get_registry(&self) -> Registry {
        self.registry.read().await.clone()
    }

    /// Get metrics as JSON (simplified version)
    pub async fn get_metrics_json(&self) -> Result<simd_json::OwnedValue, simd_json::Error> {
        let registry = self.get_registry().await;
        let metric_families = registry.gather();

        // Simplified JSON format - just return metric names and basic info
        let mut metrics = Vec::new();

        for family in metric_families {
            metrics.push(simd_json::json!({
                "name": family.get_name(),
                "help": family.get_help(),
                "metric_count": family.get_metric().len(),
            }));
        }

        Ok(simd_json::json!({
            "metrics": metrics
        }))
    }
}

impl Default for ExecutionMetrics {
    fn default() -> Self {
        Self::new().expect("Failed to create default metrics")
    }
}
