use tracing::{info, instrument};

use crate::execution_context::{ExecutionContext, ExecutionResult};

/// Execution telemetry for distributed tracing
/// Simplified to use tracing instead of OpenTelemetry directly
pub struct ExecutionTelemetry {
    /// Service name for tracing
    service_name: String,
}

impl ExecutionTelemetry {
    /// Create new telemetry service
    pub fn new(service_name: &str) -> Self {
        Self {
            service_name: service_name.to_string(),
        }
    }

    /// Start execution span
    #[instrument(skip(self, context), fields(
        operation_id = %context.operation_id,
        trace_id = %context.trace_id,
        tool_name = %context.tool_name,
        service = %self.service_name
    ))]
    pub fn start_execution_span(&self, context: &ExecutionContext) {
        info!(
            parent_operation_id = ?context.parent_operation_id,
            "Started execution trace span"
        );
    }

    /// End execution span
    #[instrument(skip(self, context, result), fields(
        operation_id = %context.operation_id,
        tool_name = %context.tool_name,
        success = result.success,
        duration_ms = result.duration_ms,
        service = %self.service_name
    ))]
    pub fn end_execution_span(&self, context: &ExecutionContext, result: &ExecutionResult) {
        if result.success {
            info!(
                outcome = "domain_response",
                "Execution completed successfully"
            );
        } else {
            info!(
                outcome = "domain_error",
                "Execution failed"
            );
        }
    }

    /// Record execution event
    #[instrument(skip(self, context, event_name, _details), fields(
        operation_id = %context.operation_id,
        tool_name = %context.tool_name,
        service = %self.service_name
    ))]
    pub fn record_event(&self, context: &ExecutionContext, event_name: &str, _details: &str) {
        info!(
            recognized_event = matches!(event_name, "reserved" | "admitted" | "completed" | "unknown"),
            "Recorded execution event"
        );
    }
}
