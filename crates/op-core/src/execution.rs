//! Execution Tracking for Tool and Agent Operations
//!
//! Provides accountability and audit trail for all tool executions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    /// Request received, not yet started
    Pending,
    /// Currently executing
    Running,
    /// Completed successfully
    Completed,
    /// Failed with error
    Failed,
    /// Cancelled by user or system
    Cancelled,
    /// Timed out
    Timeout,
}

/// Record of a single tool/agent execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    /// Unique execution ID
    pub id: String,
    /// Trace ID for correlation across systems
    pub trace_id: String,
    /// Tool or agent name
    pub tool_name: String,
    /// Input arguments (sanitized)
    pub input_summary: Option<simd_json::OwnedValue>,
    /// Execution status
    pub status: ExecutionStatus,
    /// Start time
    pub started_at: DateTime<Utc>,
    /// End time (if completed)
    pub ended_at: Option<DateTime<Utc>>,
    /// Duration in milliseconds
    pub duration_ms: Option<u64>,
    /// Output summary (truncated if large)
    pub output_summary: Option<String>,
    /// Error message if failed
    pub error: Option<String>,
    /// Whether execution was successful
    pub success: bool,
    /// User/session that initiated execution
    pub initiated_by: Option<String>,
    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl ExecutionRecord {
    /// Create a new execution record
    pub fn new(tool_name: &str, trace_id: Option<String>) -> Self {
        let id = Uuid::new_v4().to_string();
        Self {
            id: id.clone(),
            trace_id: trace_id.unwrap_or_else(|| id.clone()),
            tool_name: tool_name.to_string(),
            input_summary: None,
            status: ExecutionStatus::Pending,
            started_at: Utc::now(),
            ended_at: None,
            duration_ms: None,
            output_summary: None,
            error: None,
            success: false,
            initiated_by: None,
            metadata: HashMap::new(),
        }
    }

    /// Mark as running
    pub fn start(&mut self) {
        self.status = ExecutionStatus::Running;
        self.started_at = Utc::now();
    }

    /// Mark as completed successfully
    pub fn complete(&mut self, output: Option<String>) {
        let now = Utc::now();
        self.ended_at = Some(now);
        self.duration_ms = Some((now - self.started_at).num_milliseconds().max(0) as u64);
        self.status = ExecutionStatus::Completed;
        self.success = true;
        self.output_summary = output.map(|s| truncate_string(&s, 1000));
    }

    /// Mark as failed
    pub fn fail(&mut self, error: String) {
        let now = Utc::now();
        self.ended_at = Some(now);
        self.duration_ms = Some((now - self.started_at).num_milliseconds().max(0) as u64);
        self.status = ExecutionStatus::Failed;
        self.success = false;
        self.error = Some(error);
    }

    /// Mark as timed out
    pub fn timeout(&mut self) {
        let now = Utc::now();
        self.ended_at = Some(now);
        self.duration_ms = Some((now - self.started_at).num_milliseconds().max(0) as u64);
        self.status = ExecutionStatus::Timeout;
        self.success = false;
        self.error = Some("Execution timed out".to_string());
    }
}

/// Truncate string to max length
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}... (truncated)", &s[..max_len])
    }
}

/// Execution tracker - maintains history of all executions
#[derive(Clone)]
pub struct ExecutionTracker {
    /// Recent executions (ring buffer)
    records: Arc<RwLock<Vec<ExecutionRecord>>>,
    /// Maximum records to keep
    max_records: usize,
    /// Statistics
    stats: Arc<RwLock<ExecutionStats>>,
}

/// Execution statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionStats {
    pub total_executions: u64,
    pub successful_executions: u64,
    pub failed_executions: u64,
    pub total_duration_ms: u64,
    pub executions_by_tool: HashMap<String, u64>,
    pub failures_by_tool: HashMap<String, u64>,
}

impl ExecutionStats {
    pub fn average_duration_ms(&self) -> f64 {
        if self.total_executions == 0 {
            0.0
        } else {
            self.total_duration_ms as f64 / self.total_executions as f64
        }
    }

    pub fn success_rate(&self) -> f64 {
        if self.total_executions == 0 {
            0.0
        } else {
            self.successful_executions as f64 / self.total_executions as f64 * 100.0
        }
    }
}

impl ExecutionTracker {
    /// Create a new tracker
    pub fn new(max_records: usize) -> Self {
        Self {
            records: Arc::new(RwLock::new(Vec::with_capacity(max_records))),
            max_records,
            stats: Arc::new(RwLock::new(ExecutionStats::default())),
        }
    }

    /// Start tracking a new execution
    pub async fn start_execution(
        &self,
        tool_name: &str,
        input: Option<simd_json::OwnedValue>,
        initiated_by: Option<String>,
    ) -> ExecutionRecord {
        let mut record = ExecutionRecord::new(tool_name, None);
        record.input_summary = input;
        record.initiated_by = initiated_by;
        record.start();

        let mut records = self.records.write().await;
        records.push(record.clone());

        // Trim if over limit
        if records.len() > self.max_records {
            records.remove(0);
        }

        tracing::info!(
            execution_id = %record.id,
            tool = %tool_name,
            "Execution started"
        );

        record
    }

    /// Complete an execution
    pub async fn complete_execution(&self, id: &str, output: Option<String>) {
        let mut records = self.records.write().await;
        if let Some(record) = records.iter_mut().find(|r| r.id == id) {
            record.complete(output);

            // Update stats
            let mut stats = self.stats.write().await;
            stats.total_executions += 1;
            stats.successful_executions += 1;
            if let Some(duration) = record.duration_ms {
                stats.total_duration_ms += duration;
            }
            *stats
                .executions_by_tool
                .entry(record.tool_name.clone())
                .or_insert(0) += 1;

            tracing::info!(
                execution_id = %id,
                tool = %record.tool_name,
                duration_ms = ?record.duration_ms,
                "Execution completed successfully"
            );
        }
    }

    /// Fail an execution
    pub async fn fail_execution(&self, id: &str, error: String) {
        let mut records = self.records.write().await;
        if let Some(record) = records.iter_mut().find(|r| r.id == id) {
            record.fail(error.clone());

            // Update stats
            let mut stats = self.stats.write().await;
            stats.total_executions += 1;
            stats.failed_executions += 1;
            if let Some(duration) = record.duration_ms {
                stats.total_duration_ms += duration;
            }
            *stats
                .executions_by_tool
                .entry(record.tool_name.clone())
                .or_insert(0) += 1;
            *stats
                .failures_by_tool
                .entry(record.tool_name.clone())
                .or_insert(0) += 1;

            tracing::error!(
                execution_id = %id,
                tool = %record.tool_name,
                error = %error,
                "Execution failed"
            );
        }
    }

    /// Get recent executions
    pub async fn get_recent(&self, limit: usize) -> Vec<ExecutionRecord> {
        let records = self.records.read().await;
        records.iter().rev().take(limit).cloned().collect()
    }

    /// Get execution by ID
    pub async fn get_execution(&self, id: &str) -> Option<ExecutionRecord> {
        let records = self.records.read().await;
        records.iter().find(|r| r.id == id).cloned()
    }

    /// Get executions for a specific tool
    pub async fn get_by_tool(&self, tool_name: &str, limit: usize) -> Vec<ExecutionRecord> {
        let records = self.records.read().await;
        records
            .iter()
            .filter(|r| r.tool_name == tool_name)
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    /// Get statistics
    pub async fn get_stats(&self) -> ExecutionStats {
        self.stats.read().await.clone()
    }

    /// Get all pending/running executions
    pub async fn get_active(&self) -> Vec<ExecutionRecord> {
        let records = self.records.read().await;
        records
            .iter()
            .filter(|r| {
                r.status == ExecutionStatus::Running || r.status == ExecutionStatus::Pending
            })
            .cloned()
            .collect()
    }
}

impl Default for ExecutionTracker {
    fn default() -> Self {
        Self::new(1000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_execution_tracking() {
        let tracker = ExecutionTracker::new(100);

        let record = tracker
            .start_execution("test_tool", Some(simd_json::json!({"arg": "value"})), None)
            .await;

        assert_eq!(record.status, ExecutionStatus::Running);
        assert_eq!(record.tool_name, "test_tool");

        tracker
            .complete_execution(&record.id, Some("success output".to_string()))
            .await;

        let completed = tracker.get_execution(&record.id).await.unwrap();
        assert_eq!(completed.status, ExecutionStatus::Completed);
        assert!(completed.success);
        assert!(completed.duration_ms.is_some());
    }

    #[tokio::test]
    async fn test_execution_failure() {
        let tracker = ExecutionTracker::new(100);

        let record = tracker.start_execution("failing_tool", None, None).await;

        tracker
            .fail_execution(&record.id, "Something went wrong".to_string())
            .await;

        let failed = tracker.get_execution(&record.id).await.unwrap();
        assert_eq!(failed.status, ExecutionStatus::Failed);
        assert!(!failed.success);
        assert_eq!(failed.error, Some("Something went wrong".to_string()));
    }

    #[tokio::test]
    async fn test_stats() {
        let tracker = ExecutionTracker::new(100);

        // Successful execution
        let r1 = tracker.start_execution("tool1", None, None).await;
        tracker.complete_execution(&r1.id, None).await;

        // Failed execution
        let r2 = tracker.start_execution("tool2", None, None).await;
        tracker.fail_execution(&r2.id, "error".to_string()).await;

        let stats = tracker.get_stats().await;
        assert_eq!(stats.total_executions, 2);
        assert_eq!(stats.successful_executions, 1);
        assert_eq!(stats.failed_executions, 1);
        assert!((stats.success_rate() - 50.0).abs() < 0.01);
    }
}
