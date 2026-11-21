//! Scoped execution-control records and receipt-proof materialization.
//!
//! One operation context/status model, redacted wakeups and bounded active/
//! unresolved retention. BridgeCore owns durable proofs and cumulative aggregate
//! snapshots; this crate neither executes tools nor establishes semantic approval.

pub mod execution_context;

pub mod execution_tracker;

pub mod metrics;

pub mod telemetry;

pub mod record;

pub use execution_context::{DelegationAttribution, ExecutionContext, ExecutionResult};

pub use execution_tracker::{ExecutionEvent, ExecutionStats, ExecutionTracker};

pub use metrics::ExecutionMetrics;

pub use telemetry::ExecutionTelemetry;

pub use record::{ExecutionProof, ExecutionRecord, ExecutionStatus};
