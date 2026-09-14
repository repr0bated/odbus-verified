//! Execution Tracking Record
//!
//! Provides accountability and audit trail for all tool executions.
//! - ExecutionTiming for precise timing capture
//! - Builder pattern for ExecutionRecord creation
//! - Hash-based execution fingerprinting

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use simd_json::OwnedValue as Value;
use simd_json::ValueBuilder;
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

// ============================================================================
// EXECUTION STATUS
// ============================================================================

/// Execution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    /// Request received, not yet started
    #[default]
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

// ============================================================================
// EXECUTION TIMING
// ============================================================================

/// High-precision execution timing
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ExecutionTiming {
    /// Wall-clock start time
    pub started_at: DateTime<Utc>,
    /// Wall-clock end time
    pub ended_at: Option<DateTime<Utc>>,
    /// Monotonic nanoseconds (for ordering)
    pub monotonic_ns: u128,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Duration in nanoseconds (compatibility alias)
    pub duration_ns: u128,
    /// Wall-clock nanoseconds (compatibility alias)
    pub wallclock_ns: u128,
}

impl ExecutionTiming {
    /// Capture the start of an execution
    pub fn capture_start() -> (Instant, Self) {
        let now = Instant::now();
        let monotonic = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let timing = Self {
            started_at: Utc::now(),
            ended_at: None,
            monotonic_ns: monotonic,
            duration_ms: 0,
            duration_ns: 0,
            wallclock_ns: monotonic,
        };
        (now, timing)
    }

    /// Complete the timing with duration
    pub fn complete(mut self, start: Instant) -> Self {
        let elapsed = start.elapsed();
        self.ended_at = Some(Utc::now());
        self.duration_ms = elapsed.as_millis() as u64;
        self.duration_ns = elapsed.as_nanos();
        self.wallclock_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        self
    }
}

// ============================================================================
// EXECUTION RECORD
// ============================================================================

/// Record of a single tool/agent execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    /// Unique execution ID
    pub id: String,
    /// Trace ID for correlation across systems
    pub trace_id: String,
    /// Tool or agent name
    pub tool_name: String,
    /// Input arguments
    pub input: Value,
    /// Output value
    pub output: Value,
    /// Execution status
    pub status: ExecutionStatus,
    /// Execution timing
    pub timing: ExecutionTiming,
    /// Policy ID governing this execution
    pub policy_id: String,
    /// Plugin core hash for determinism
    pub plugin_core_hash: String,
    /// Tunable hash for determinism
    pub tunable_hash: String,
    /// Previous execution hash for chaining
    pub prev_hash: String,
    /// This execution's hash
    pub exec_hash: String,
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
    /// Create a new execution record with default values
    pub fn new(tool_name: &str, trace_id: Option<String>) -> Self {
        let id = Uuid::new_v4().to_string();
        Self {
            id: id.clone(),
            trace_id: trace_id.unwrap_or_else(|| id.clone()),
            tool_name: tool_name.to_string(),
            input: Value::null(),
            output: Value::null(),
            status: ExecutionStatus::Pending,
            timing: ExecutionTiming::default(),
            policy_id: String::new(),
            plugin_core_hash: String::new(),
            tunable_hash: String::new(),
            prev_hash: String::new(),
            exec_hash: String::new(),
            output_summary: None,
            error: None,
            success: false,
            initiated_by: None,
            metadata: HashMap::new(),
        }
    }

    /// Create a builder for ExecutionRecord
    pub fn builder(tool_name: &str) -> ExecutionRecordBuilder {
        ExecutionRecordBuilder::new(tool_name)
    }

    /// Mark as running
    pub fn start(&mut self) {
        self.status = ExecutionStatus::Running;
        self.timing.started_at = Utc::now();
    }

    /// Mark as completed successfully
    pub fn complete(&mut self, output: Option<String>) {
        let now = Utc::now();
        self.timing.ended_at = Some(now);
        self.timing.duration_ms = (now - self.timing.started_at).num_milliseconds().max(0) as u64;
        self.status = ExecutionStatus::Completed;
        self.success = true;
        self.output_summary = output.map(|s| truncate_string(&s, 1000));
    }

    /// Mark as failed
    pub fn fail(&mut self, error: String) {
        let now = Utc::now();
        self.timing.ended_at = Some(now);
        self.timing.duration_ms = (now - self.timing.started_at).num_milliseconds().max(0) as u64;
        self.status = ExecutionStatus::Failed;
        self.success = false;
        self.error = Some(error);
    }

    /// Mark as timed out
    pub fn timeout(&mut self) {
        let now = Utc::now();
        self.timing.ended_at = Some(now);
        self.timing.duration_ms = (now - self.timing.started_at).num_milliseconds().max(0) as u64;
        self.status = ExecutionStatus::Timeout;
        self.success = false;
        self.error = Some("Execution timed out".to_string());
    }

    /// Mark as cancelled
    pub fn cancel(&mut self) {
        let now = Utc::now();
        self.timing.ended_at = Some(now);
        self.timing.duration_ms = (now - self.timing.started_at).num_milliseconds().max(0) as u64;
        self.status = ExecutionStatus::Cancelled;
        self.success = false;
        self.error = Some("Execution cancelled".to_string());
    }

    // ================= COMPATIBILITY ACCESSORS =================

    /// Alias for id (compatibility)
    pub fn execution_id(&self) -> &str {
        &self.id
    }

    /// Alias for tool_name (compatibility)
    pub fn tool(&self) -> &str {
        &self.tool_name
    }

    /// Alias for exec_hash (compatibility)
    pub fn hash(&self) -> &str {
        &self.exec_hash
    }

    /// Verify hash integrity
    pub fn verify_integrity(&self) -> bool {
        let computed = hash_execution(&self.tool_name, &self.input, &self.output, &self.prev_hash);
        computed == self.exec_hash
    }
}

// ============================================================================
// EXECUTION RECORD BUILDER
// ============================================================================

/// Builder pattern for creating execution records
pub struct ExecutionRecordBuilder {
    tool_name: String,
    input: Value,
    output: Value,
    policy_id: String,
    plugin_core_hash: String,
    tunable_hash: String,
    timing: ExecutionTiming,
    prev_hash: String,
    initiated_by: Option<String>,
    metadata: HashMap<String, String>,
}

impl ExecutionRecordBuilder {
    pub fn new(tool_name: &str) -> Self {
        Self {
            tool_name: tool_name.to_string(),
            input: Value::null(),
            output: Value::null(),
            policy_id: "default".to_string(),
            plugin_core_hash: String::new(),
            tunable_hash: String::new(),
            timing: ExecutionTiming::default(),
            prev_hash: String::new(),
            initiated_by: None,
            metadata: HashMap::new(),
        }
    }

    pub fn input(mut self, input: Value) -> Self {
        self.input = input;
        self
    }

    pub fn output(mut self, output: Value) -> Self {
        self.output = output;
        self
    }

    pub fn policy_id(mut self, policy_id: &str) -> Self {
        self.policy_id = policy_id.to_string();
        self
    }

    pub fn plugin_core_hash(mut self, hash: &str) -> Self {
        self.plugin_core_hash = hash.to_string();
        self
    }

    pub fn tunable_hash(mut self, hash: &str) -> Self {
        self.tunable_hash = hash.to_string();
        self
    }

    pub fn timing(mut self, timing: ExecutionTiming) -> Self {
        self.timing = timing;
        self
    }

    pub fn prev_hash(mut self, hash: &str) -> Self {
        self.prev_hash = hash.to_string();
        self
    }

    pub fn initiated_by(mut self, user: &str) -> Self {
        self.initiated_by = Some(user.to_string());
        self
    }

    pub fn metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }

    pub fn build(self) -> ExecutionRecord {
        let id = Uuid::new_v4().to_string();
        let exec_hash = hash_execution(&self.tool_name, &self.input, &self.output, &self.prev_hash);

        ExecutionRecord {
            id: id.clone(),
            trace_id: id,
            tool_name: self.tool_name,
            input: self.input,
            output: self.output.clone(),
            status: ExecutionStatus::Completed,
            timing: self.timing,
            policy_id: self.policy_id,
            plugin_core_hash: self.plugin_core_hash,
            tunable_hash: self.tunable_hash,
            prev_hash: self.prev_hash,
            exec_hash,
            output_summary: Some(truncate_string(
                &simd_json::to_string(&self.output).unwrap_or_default(),
                1000,
            )),
            error: None,
            success: true,
            initiated_by: self.initiated_by,
            metadata: self.metadata,
        }
    }
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Truncate string to max length
fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}... (truncated)", &s[..max_len])
    }
}

/// Compute execution hash for deterministic fingerprinting
pub fn hash_execution(tool_name: &str, input: &Value, output: &Value, prev_hash: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tool_name.as_bytes());
    hasher.update(simd_json::to_vec(input).unwrap_or_default());
    hasher.update(simd_json::to_vec(output).unwrap_or_default());
    hasher.update(prev_hash.as_bytes());
    hex::encode(hasher.finalize())
}
