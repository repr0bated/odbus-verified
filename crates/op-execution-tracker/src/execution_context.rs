use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Execution context for tracking tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionContext {
    /// Unique execution ID
    pub execution_id: String,

    /// Trace ID for distributed tracing
    pub trace_id: String,

    /// Parent execution ID (if this is a sub-execution)
    pub parent_id: Option<String>,

    /// Tool name being executed
    pub tool_name: String,

    /// Execution status
    pub status: ExecutionStatus,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last update timestamp
    pub updated_at: DateTime<Utc>,

    /// Execution metadata
    pub metadata: simd_json::OwnedValue,
}

/// Execution status enum
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionStatus {
    /// Execution has been requested
    Requested,

    /// Execution has been dispatched to executor
    Dispatched,

    /// Execution is currently running
    Running,

    /// Execution completed successfully
    Completed,

    /// Execution failed
    Failed,

    /// Execution was cancelled
    Cancelled,
}

impl fmt::Display for ExecutionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecutionStatus::Requested => write!(f, "Requested"),
            ExecutionStatus::Dispatched => write!(f, "Dispatched"),
            ExecutionStatus::Running => write!(f, "Running"),
            ExecutionStatus::Completed => write!(f, "Completed"),
            ExecutionStatus::Failed => write!(f, "Failed"),
            ExecutionStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// Execution result for tracked tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub result: Option<simd_json::OwnedValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub duration_ms: u64,
    pub finished_at: chrono::DateTime<chrono::Utc>,
}

impl ExecutionContext {
    /// Create new execution context
    pub fn new(tool_name: &str) -> Self {
        let now = Utc::now();
        Self {
            execution_id: Uuid::new_v4().to_string(),
            trace_id: Uuid::new_v4().to_string(),
            parent_id: None,
            tool_name: tool_name.to_string(),
            status: ExecutionStatus::Requested,
            created_at: now,
            updated_at: now,
            metadata: simd_json::json!({}),
        }
    }

    /// Create child execution context
    pub fn new_child(parent: &ExecutionContext, tool_name: &str) -> Self {
        let now = Utc::now();
        Self {
            execution_id: Uuid::new_v4().to_string(),
            trace_id: parent.trace_id.clone(), // Share trace ID for distributed tracing
            parent_id: Some(parent.execution_id.clone()),
            tool_name: tool_name.to_string(),
            status: ExecutionStatus::Requested,
            created_at: now,
            updated_at: now,
            metadata: simd_json::json!({}),
        }
    }

    /// Update status
    pub fn update_status(&mut self, new_status: ExecutionStatus) {
        self.status = new_status;
        self.updated_at = Utc::now();
    }

    /// Set metadata
    pub fn set_metadata(&mut self, metadata: simd_json::OwnedValue) {
        self.metadata = metadata;
        self.updated_at = Utc::now();
    }
}
