//! Pure Tool_Call status machine (spec task 7.1). No model-text input:
//! events are confirmation events, operation results, receipts and
//! read-backs only. `completed` requires Receipt plus verified
//! read-back; otherwise `unconfirmed`.

use crate::types::ToolCallStatus;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusEvent {
    ValidatedMutation,
    ValidatedRead,
    ValidationOrNotOffered,
    ConfirmationAccepted,
    Rejected,
    ReceiptWithVerifiedReadBack,
    ReceiptWithoutVerifiedReadBack,
    Denial,
    NonDenialError,
    NoReceiptTimeout,
}

pub fn is_terminal(status: ToolCallStatus) -> bool {
    matches!(
        status,
        ToolCallStatus::Completed
            | ToolCallStatus::Rejected
            | ToolCallStatus::Denied
            | ToolCallStatus::Failed
    )
}

/// The diagram has no explicit initial state in the enum; callers start
/// from `Pending` and `ValidatedMutation` moves a fresh call to
/// `AwaitingConfirmation`. Terminal states absorb every event.
pub fn next(status: ToolCallStatus, event: StatusEvent) -> ToolCallStatus {
    use StatusEvent::*;
    use ToolCallStatus::*;
    if is_terminal(status) {
        return status;
    }
    match (status, event) {
        (Pending, ValidatedMutation) => AwaitingConfirmation,
        (Pending, ValidatedRead) => Pending,
        (_, ValidationOrNotOffered) => Failed,
        (AwaitingConfirmation, ConfirmationAccepted) => Pending,
        (AwaitingConfirmation, Rejected) => Rejected,
        (Pending, ReceiptWithVerifiedReadBack) => Completed,
        (Pending, ReceiptWithoutVerifiedReadBack) => Unconfirmed,
        (Pending, Denial) => Denied,
        (Pending, NonDenialError) => Failed,
        (Pending, NoReceiptTimeout) => Unknown,
        (Unknown, ReceiptWithVerifiedReadBack) => Completed,
        (Unknown, ReceiptWithoutVerifiedReadBack) => Unconfirmed,
        (Unknown, Denial) => Denied,
        (Unknown, NonDenialError) => Failed,
        (current, _) => current,
    }
}

/// Per-call sequence numbers: `seq` increases per `toolCallId`.
#[derive(Default)]
pub struct StatusTracker {
    calls: HashMap<String, (ToolCallStatus, u64)>,
}

impl StatusTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, tool_call_id: &str, mutation: bool) -> (ToolCallStatus, u64) {
        let status = if mutation {
            ToolCallStatus::AwaitingConfirmation
        } else {
            ToolCallStatus::Pending
        };
        self.calls
            .insert(tool_call_id.to_string(), (status, 1));
        (status, 1)
    }

    pub fn apply(&mut self, tool_call_id: &str, event: StatusEvent) -> Option<(ToolCallStatus, u64)> {
        let (status, seq) = self.calls.get_mut(tool_call_id)?;
        *status = next(*status, event);
        *seq += 1;
        Some((*status, *seq))
    }

    pub fn status(&self, tool_call_id: &str) -> Option<ToolCallStatus> {
        self.calls.get(tool_call_id).map(|(s, _)| *s)
    }
}
