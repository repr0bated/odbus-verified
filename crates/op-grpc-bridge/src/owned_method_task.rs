//! Cancellation ownership for already admitted canonical methods.
//!
//! This helper grants no authority. Canonical admission precedes its only
//! production callsite, and backend-specific checks still precede effects.
use std::future::Future;
use tracing::instrument::WithSubscriber;

pub(super) struct OperationMetadata {
    pub operation_id: String,
    pub event_id: u64,
    pub plugin_id: String,
    pub method: String,
}

pub(super) fn spawn<F>(
    operation: OperationMetadata,
    work: F,
) -> tokio::task::JoinHandle<anyhow::Result<serde_json::Value>>
where
    F: Future<Output = anyhow::Result<serde_json::Value>> + Send + 'static,
{
    tokio::spawn(async move {
        let outcome = work.await;
        if outcome.is_err() {
            // Caller cancellation can discard the JoinHandle. Report safe
            // operation metadata from the owner itself, never raw errors,
            // opaque arguments/results, principal IDs or credentials.
            tracing::error!(
                code = "admitted_method_failed",
                operation_id = %operation.operation_id,
                event_id = operation.event_id,
                plugin_id = %operation.plugin_id,
                method = %operation.method,
                "Admitted method did not complete; authoritative state may require reconciliation"
            );
        }
        outcome
    }.with_current_subscriber())
}

#[cfg(test)]
#[path = "owned_method_task_tests.rs"]
mod tests;
