//! Single-use execution tickets: docs/APPROVALS-SPEC.md §§6–7.
//!
//! A ticket is issued only after both approvals (or break-glass) and is bound
//! to `request_id`, `arguments_hash`, `executor`, and a short TTL. The store
//! is in-memory; persistence (sealed plugin vs. receipted service store) is
//! open question §10 — the binding rules here do not change either way.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ApprovalKind {
    Owner,
    Cosigner,
    Human,
    BreakGlass,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    pub id: String,
    pub request_id: String,
    pub arguments_hash: String,
    pub executor: String,
    pub expires_at: u64,
    pub used: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TicketError {
    NotFound,
    AlreadyUsed,
    ArgumentsMismatch,
    ExecutorMismatch,
    Expired,
    RequestMismatch,
}

#[derive(Default)]
pub struct TicketStore {
    tickets: HashMap<String, Ticket>,
}

impl TicketStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Issue after both approvals are recorded. `ttl_s` caps lifetime;
    /// the ticket never outlives the request itself.
    pub fn issue(
        &mut self,
        request: &crate::ApprovalRequest,
        now: u64,
        ttl_s: u64,
    ) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let expires_at = (now + ttl_s).min(request.expires_at);
        self.tickets.insert(
            id.clone(),
            Ticket {
                id: id.clone(),
                request_id: request.id.clone(),
                arguments_hash: request.arguments_hash.clone(),
                executor: request.executor.clone(),
                expires_at,
                used: false,
            },
        );
        id
    }

    /// Consume exactly once. Every binding is re-checked against the live
    /// request material the executor presents — a mismatch is refused.
    pub fn consume(
        &mut self,
        ticket_id: &str,
        request_id: &str,
        arguments_hash: &str,
        executor: &str,
        now: u64,
    ) -> Result<(), TicketError> {
        let ticket = self
            .tickets
            .get_mut(ticket_id)
            .ok_or(TicketError::NotFound)?;
        if ticket.used {
            return Err(TicketError::AlreadyUsed);
        }
        if now > ticket.expires_at {
            return Err(TicketError::Expired);
        }
        if ticket.request_id != request_id {
            return Err(TicketError::RequestMismatch);
        }
        if ticket.arguments_hash != arguments_hash {
            return Err(TicketError::ArgumentsMismatch);
        }
        if ticket.executor != executor {
            return Err(TicketError::ExecutorMismatch);
        }
        ticket.used = true;
        Ok(())
    }
}
