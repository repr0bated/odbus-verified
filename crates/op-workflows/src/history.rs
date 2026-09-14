//! Workflow History - Durable Event Log
//!
//! Implements the Event Sourcing pattern for workflows.
//! Every state change is recorded as an immutable event.
//! Replaying these events reconstructs the workflow state.

use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// A single event in the workflow history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEvent {
    /// Incremental event ID (1, 2, 3...)
    pub event_id: u64,
    /// Timestamp (UTC)
    pub timestamp: u64,
    /// The type of event and its data
    pub event_type: EventType,
}

impl HistoryEvent {
    pub fn new(event_id: u64, event_type: EventType) -> Self {
        Self {
            event_id,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            event_type,
        }
    }
}

/// Types of events that can occur in a workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    /// Workflow execution started
    WorkflowExecutionStarted {
        workflow_type: String,
        workflow_id: String,
        inputs: Value,
    },
    /// Workflow execution completed
    WorkflowExecutionCompleted { result: Value },
    /// Workflow execution failed
    WorkflowExecutionFailed {
        error: String,
        details: Option<String>,
    },

    /// A node (task) was scheduled
    NodeTaskScheduled {
        node_id: String,
        node_type: String,
        inputs: Value,
    },
    /// A node task started execution (worker picked it up)
    NodeTaskStarted { node_id: String, attempt: u32 },
    /// A node task completed successfully
    NodeTaskCompleted { node_id: String, result: Value },
    /// A node task failed
    NodeTaskFailed {
        node_id: String,
        error: String,
        retryable: bool,
    },

    /// A timer was started
    TimerStarted {
        timer_id: String,
        duration_secs: u64,
    },
    /// A timer fired
    TimerFired { timer_id: String },

    /// A signal was received (external event)
    SignalReceived { signal_name: String, payload: Value },

    /// A marker recorded by the workflow (custom data)
    MarkerRecorded { marker_name: String, details: Value },
}

/// The full history of a workflow execution
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowHistory {
    pub events: Vec<HistoryEvent>,
}

impl WorkflowHistory {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// Add an event to the history
    pub fn add(&mut self, event_type: EventType) {
        let event_id = (self.events.len() as u64) + 1;
        self.events.push(HistoryEvent::new(event_id, event_type));
    }

    /// Get the last event ID
    pub fn last_event_id(&self) -> u64 {
        self.events.len() as u64
    }
}
