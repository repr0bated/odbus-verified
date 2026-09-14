use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;
use simd_json::ValueBuilder;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{info, warn};

use crate::record::{ExecutionRecord, ExecutionStatus};

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

/// Event emitted when execution state changes
#[derive(Clone, Debug)]
pub enum ExecutionEvent {
    Started(Box<ExecutionRecord>),
    Completed(String, bool),                // execution_id, success
    StatusUpdated(String, ExecutionStatus), // execution_id, new_status
}

/// Execution tracker for monitoring tool executions
#[derive(Clone)]
pub struct ExecutionTracker {
    /// History of executions (ring buffer)
    records: Arc<RwLock<Vec<ExecutionRecord>>>,
    /// Maximum history size
    max_history: usize,
    /// Statistics
    stats: Arc<RwLock<ExecutionStats>>,
    /// Event broadcaster
    event_sender: broadcast::Sender<ExecutionEvent>,
}

impl ExecutionTracker {
    /// Create new execution tracker
    pub fn new(max_history: usize) -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self {
            records: Arc::new(RwLock::new(Vec::with_capacity(max_history))),
            max_history,
            stats: Arc::new(RwLock::new(ExecutionStats::default())),
            event_sender: tx,
        }
    }

    /// Subscribe to execution events
    pub fn subscribe(&self) -> broadcast::Receiver<ExecutionEvent> {
        self.event_sender.subscribe()
    }

    /// Start tracking a new execution (Legacy compatibility API)
    pub async fn start_execution(
        &self,
        tool_name: &str,
        input: Option<Value>,
        initiated_by: Option<String>,
    ) -> ExecutionRecord {
        let mut record = ExecutionRecord::new(tool_name, None);
        record.input = input.unwrap_or(Value::null());
        record.initiated_by = initiated_by;
        record.start();

        let mut records = self.records.write().await;
        records.push(record.clone());

        // Trim if over limit
        if records.len() > self.max_history {
            records.remove(0);
        }

        // Notify subscribers
        let _ = self
            .event_sender
            .send(ExecutionEvent::Started(Box::new(record.clone())));

        info!(execution_id = %record.id, tool = %tool_name, "Execution started");

        record
    }

    /// Complete an execution (Legacy compatibility API)
    pub async fn complete_execution(&self, id: &str, output: Option<String>) {
        let mut records = self.records.write().await;
        if let Some(record) = records.iter_mut().find(|r| r.id == id) {
            record.complete(output);

            // Update stats
            let mut stats = self.stats.write().await;
            stats.total_executions += 1;
            stats.successful_executions += 1;
            if record.timing.duration_ms > 0 {
                stats.total_duration_ms += record.timing.duration_ms;
            }
            *stats
                .executions_by_tool
                .entry(record.tool_name.clone())
                .or_insert(0) += 1;

            // Notify subscribers
            let _ = self
                .event_sender
                .send(ExecutionEvent::Completed(id.to_string(), true));

            info!(
                execution_id = %id,
                tool = %record.tool_name,
                duration_ms = record.timing.duration_ms,
                "Execution completed successfully"
            );
        }
    }

    /// Fail an execution (Legacy compatibility API)
    pub async fn fail_execution(&self, id: &str, error: String) {
        let mut records = self.records.write().await;
        if let Some(record) = records.iter_mut().find(|r| r.id == id) {
            record.fail(error.clone());

            // Update stats
            let mut stats = self.stats.write().await;
            stats.total_executions += 1;
            stats.failed_executions += 1;
            if record.timing.duration_ms > 0 {
                stats.total_duration_ms += record.timing.duration_ms;
            }
            *stats
                .executions_by_tool
                .entry(record.tool_name.clone())
                .or_insert(0) += 1;
            *stats
                .failures_by_tool
                .entry(record.tool_name.clone())
                .or_insert(0) += 1;

            // Notify subscribers
            let _ = self
                .event_sender
                .send(ExecutionEvent::Completed(id.to_string(), false));

            warn!(
                execution_id = %id,
                tool = %record.tool_name,
                error = %error,
                "Execution failed"
            );
        }
    }

    /// Get execution record by ID
    pub async fn get_execution(&self, id: &str) -> Option<ExecutionRecord> {
        let records = self.records.read().await;
        let result = records.iter().find(|r| r.id == id).cloned();
        result
    }

    /// List active executions
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

    /// List recent completed executions
    pub async fn get_recent(&self, limit: usize) -> Vec<ExecutionRecord> {
        let records = self.records.read().await;
        records
            .as_slice()
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }

    /// Alias for get_recent (compatibility)
    pub async fn list_recent_completed(&self, limit: usize) -> Vec<ExecutionRecord> {
        self.get_recent(limit).await
    }

    /// Get statistics snapshot
    pub async fn get_stats(&self) -> ExecutionStats {
        self.stats.read().await.clone()
    }
}

impl Default for ExecutionTracker {
    fn default() -> Self {
        Self::new(1000)
    }
}
