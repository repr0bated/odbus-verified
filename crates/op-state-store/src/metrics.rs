//! Prometheus metrics for state store operations
//!
//! Provides observability into store operations including:
//! - Job counts by status
//! - Operation latencies
//! - Error rates

use lazy_static::lazy_static;
use prometheus::{Counter, CounterVec, GaugeVec, HistogramOpts, HistogramVec, Opts, Registry};
use std::sync::Once;
use tracing::info;

lazy_static! {
    /// Global metrics registry
    pub static ref REGISTRY: Registry = Registry::new();

    // Job metrics
    /// Total jobs created
    pub static ref JOBS_CREATED_TOTAL: Counter = Counter::new(
        "op_state_jobs_created_total",
        "Total number of jobs created"
    ).expect("hardcoded metric definition is valid");

    /// Jobs by status
    pub static ref JOBS_BY_STATUS: GaugeVec = GaugeVec::new(
        Opts::new("op_state_jobs_by_status", "Number of jobs by status"),
        &["status"]
    ).expect("hardcoded metric definition is valid");

    /// Job status transitions
    pub static ref JOB_STATUS_TRANSITIONS: CounterVec = CounterVec::new(
        Opts::new("op_state_job_transitions_total", "Job status transitions"),
        &["from_status", "to_status"]
    ).expect("hardcoded metric definition is valid");

    /// Job execution duration
    pub static ref JOB_DURATION_SECONDS: HistogramVec = HistogramVec::new(
        HistogramOpts::new("op_state_job_duration_seconds", "Job execution duration")
            .buckets(vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0]),
        &["tool_name"]
    ).expect("hardcoded metric definition is valid");

    // Store operation metrics
    /// Store operation latency
    pub static ref STORE_OP_DURATION: HistogramVec = HistogramVec::new(
        HistogramOpts::new("op_state_store_operation_seconds", "Store operation duration")
            .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0]),
        &["operation", "store_type"]
    ).expect("hardcoded metric definition is valid");

    /// Store operation errors
    pub static ref STORE_OP_ERRORS: CounterVec = CounterVec::new(
        Opts::new("op_state_store_errors_total", "Store operation errors"),
        &["operation", "store_type", "error_type"]
    ).expect("hardcoded metric definition is valid");

    // Plugin metrics
    /// Plugin state queries
    pub static ref PLUGIN_QUERIES_TOTAL: CounterVec = CounterVec::new(
        Opts::new("op_state_plugin_queries_total", "Plugin state queries"),
        &["plugin_name"]
    ).expect("hardcoded metric definition is valid");

    /// Plugin state applies
    pub static ref PLUGIN_APPLIES_TOTAL: CounterVec = CounterVec::new(
        Opts::new("op_state_plugin_applies_total", "Plugin state applies"),
        &["plugin_name", "success"]
    ).expect("hardcoded metric definition is valid");

    /// Plugin checkpoints created
    pub static ref CHECKPOINTS_CREATED: CounterVec = CounterVec::new(
        Opts::new("op_state_checkpoints_created_total", "Checkpoints created"),
        &["plugin_name"]
    ).expect("hardcoded metric definition is valid");

    // Audit metrics
    /// Audit log entries
    pub static ref AUDIT_ENTRIES_TOTAL: Counter = Counter::new(
        "op_state_audit_entries_total",
        "Total audit log entries"
    ).expect("hardcoded metric definition is valid");

}

static INIT: Once = Once::new();

/// Register all metrics with the global registry
pub fn register_metrics() {
    INIT.call_once(|| {
        info!("Registering state store metrics");

        // Job metrics
        REGISTRY.register(Box::new(JOBS_CREATED_TOTAL.clone())).ok();
        REGISTRY.register(Box::new(JOBS_BY_STATUS.clone())).ok();
        REGISTRY
            .register(Box::new(JOB_STATUS_TRANSITIONS.clone()))
            .ok();
        REGISTRY
            .register(Box::new(JOB_DURATION_SECONDS.clone()))
            .ok();

        // Store operation metrics
        REGISTRY.register(Box::new(STORE_OP_DURATION.clone())).ok();
        REGISTRY.register(Box::new(STORE_OP_ERRORS.clone())).ok();

        // Plugin metrics
        REGISTRY
            .register(Box::new(PLUGIN_QUERIES_TOTAL.clone()))
            .ok();
        REGISTRY
            .register(Box::new(PLUGIN_APPLIES_TOTAL.clone()))
            .ok();
        REGISTRY
            .register(Box::new(CHECKPOINTS_CREATED.clone()))
            .ok();

        // Audit metrics
        REGISTRY
            .register(Box::new(AUDIT_ENTRIES_TOTAL.clone()))
            .ok();

        info!("State store metrics registered");
    });
}

/// Helper to time a store operation
pub struct OperationTimer {
    operation: String,
    store_type: String,
    start: std::time::Instant,
}

impl OperationTimer {
    pub fn new(operation: &str, store_type: &str) -> Self {
        Self {
            operation: operation.to_string(),
            store_type: store_type.to_string(),
            start: std::time::Instant::now(),
        }
    }
}

impl Drop for OperationTimer {
    fn drop(&mut self) {
        let duration = self.start.elapsed().as_secs_f64();
        STORE_OP_DURATION
            .with_label_values(&[&self.operation, &self.store_type])
            .observe(duration);
    }
}

/// Record a job status transition
pub fn record_job_transition(from: &str, to: &str) {
    JOB_STATUS_TRANSITIONS.with_label_values(&[from, to]).inc();
}

/// Record a job completion
pub fn record_job_completion(tool_name: &str, duration_secs: f64) {
    JOB_DURATION_SECONDS
        .with_label_values(&[tool_name])
        .observe(duration_secs);
}

/// Record a plugin query
pub fn record_plugin_query(plugin_name: &str) {
    PLUGIN_QUERIES_TOTAL.with_label_values(&[plugin_name]).inc();
}

/// Record a plugin apply
pub fn record_plugin_apply(plugin_name: &str, success: bool) {
    PLUGIN_APPLIES_TOTAL
        .with_label_values(&[plugin_name, if success { "true" } else { "false" }])
        .inc();
}

/// Record a checkpoint creation
pub fn record_checkpoint(plugin_name: &str) {
    CHECKPOINTS_CREATED.with_label_values(&[plugin_name]).inc();
}

/// Record an audit entry
pub fn record_audit_entry() {
    AUDIT_ENTRIES_TOTAL.inc();
}

/// Record a store error
pub fn record_store_error(operation: &str, store_type: &str, error_type: &str) {
    STORE_OP_ERRORS
        .with_label_values(&[operation, store_type, error_type])
        .inc();
}

/// Update job counts by status
pub fn update_job_counts(pending: u64, running: u64, completed: u64, failed: u64) {
    JOBS_BY_STATUS
        .with_label_values(&["pending"])
        .set(pending as f64);
    JOBS_BY_STATUS
        .with_label_values(&["running"])
        .set(running as f64);
    JOBS_BY_STATUS
        .with_label_values(&["completed"])
        .set(completed as f64);
    JOBS_BY_STATUS
        .with_label_values(&["failed"])
        .set(failed as f64);
}

/// Get metrics as text for Prometheus scraping
pub fn gather_metrics() -> String {
    use prometheus::Encoder;
    let encoder = prometheus::TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = Vec::new();
    encoder
        .encode(&metric_families, &mut buffer)
        .expect("hardcoded metric definition is valid");
    String::from_utf8(buffer).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_metrics() {
        register_metrics();
        // Should not panic on duplicate registration
        register_metrics();
    }

    #[test]
    fn test_operation_timer() {
        register_metrics();

        {
            let _timer = OperationTimer::new("save_job", "memory");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Timer should have recorded the duration
        // We can't easily check the histogram value, but at least it shouldn't panic
    }

    #[test]
    fn test_record_functions() {
        register_metrics();

        record_job_transition("Pending", "Running");
        record_job_completion("test_tool", 1.5);
        record_plugin_query("lxc");
        record_plugin_apply("lxc", true);
        record_checkpoint("lxc");
        record_audit_entry();
        record_store_error("save", "memory", "lock");
        update_job_counts(1, 2, 3, 4);
    }
}
