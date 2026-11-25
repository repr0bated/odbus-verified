//! Safe control projection; native effect proofs remain in BridgeCore.
use chrono::{DateTime, Utc};
use op_state_store::method_receipt::{MethodEventProof, valid_event_hash};
use serde::{Deserialize, Serialize};
use crate::execution_context::ExecutionContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStatus {
    #[default]
    Proposed,
    Checking,
    ReadyForAdmission,
    Admitted,
    Running,
    /// Native receipt exists; domain/relevance/disclosure release is pending.
    ReceiptCompleted,
    Completed,
    DomainFailed,
    FailedBeforeEffect,
    Denied,
    CancelRequested,
    Cancelled,
    Interrupted,
    OutcomeUnknown,
    NeedsReview,
}

impl ExecutionStatus {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::DomainFailed | Self::FailedBeforeEffect
            | Self::Denied | Self::Cancelled)
    }
}

impl std::fmt::Display for ExecutionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionProof {
    pub operation_id: String,
    pub attempt: MethodEventProof,
    pub terminal: Option<MethodEventProof>,
}

impl ExecutionProof {
    pub fn validate(&self, operation_id: &str) -> anyhow::Result<()> {
        anyhow::ensure!(self.operation_id == operation_id && self.attempt.event_id > 0,
            "execution proof attribution is invalid");
        for proof in std::iter::once(&self.attempt).chain(self.terminal.iter()) {
            anyhow::ensure!(valid_event_hash(&proof.event_hash),
                "execution proof hash is invalid");
        }
        anyhow::ensure!(self.terminal.as_ref().is_none_or(|terminal|
            terminal.event_id > self.attempt.event_id), "execution terminal proof order is invalid");
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub context: ExecutionContext,
    pub status: ExecutionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub proof: Option<ExecutionProof>,
    /// Existing core event cursor. Broadcasts are only wakeups.
    pub cursor: Option<u64>,
}

impl ExecutionRecord {
    pub fn new(context: ExecutionContext) -> anyhow::Result<Self> {
        context.validate()?;
        let now = Utc::now();
        Ok(Self { context, status: ExecutionStatus::Proposed,
            created_at: now, updated_at: now, duration_ms: 0, proof: None, cursor: None })
    }
}
