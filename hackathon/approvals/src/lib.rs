//! Approvals: two-approval root, policy co-signer, break-glass.
//!
//! Implements `docs/APPROVALS-SPEC.md` §§4–7 as pure code: structured requests,
//! canonical argument hashing, the co-signer truth table, and single-use
//! tickets. No I/O, no network, no clock access — `now` is always passed in,
//! so every path is deterministic and testable.

pub mod judge;
pub mod request;
pub mod ticket;

pub use judge::{Judgments, TriState, Verdict, decide, judge};
pub use request::{ApprovalRequest, OperationRef, PolicyRef, RequestError, canonical_hash};
pub use ticket::{ApprovalKind, Ticket, TicketError, TicketStore};

use serde::{Deserialize, Serialize};

/// One recorded approval (or refusal) on a request — spec §4.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub request_id: String,
    pub approver_principal: String,
    pub kind: ApprovalKind,
    pub decision: ApprovalDecision,
    pub reasons: Vec<String>,
    /// `operation_id/event_hash` of the receipt recording this decision.
    pub receipt: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalDecision {
    Approve,
    Refuse,
}

impl Approval {
    /// A refusal carries at least one reason; approvals may carry none.
    pub fn valid(&self) -> bool {
        match self.decision {
            ApprovalDecision::Refuse => !self.reasons.is_empty(),
            ApprovalDecision::Approve => true,
        }
    }
}

#[cfg(test)]
mod acceptance;
