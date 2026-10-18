//! Bridge-side chat turn registry: the only issuer of `turn_id`.
//!
//! The model never chooses a conversation or a turn. The bridge opens a turn
//! for a verified human in one of that human's conversations and designates
//! the agent principal that may answer it. A response is admitted only while
//! its turn is pending and unexpired, only from the designated responder, and
//! only once: the claim below serializes concurrent answers in this process,
//! and the durable owner rejects a second row for the same `turn_id`.
//!
//! Pending turns live in memory. A bridge restart drops them, so a late
//! response is refused (fail closed) rather than attributed to a guess.
//!
//! This module is a thin wrapper over the generic [`RequestRegistry`] with
//! chat-specific context (conversation_id, turn_index).

use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};

use crate::request_registry::{
    DEFAULT_TTL_SECONDS, PendingRequest, RequestClaim, RequestContext, RequestOwner,
    RequestRegistry,
};

/// How long an opened turn waits for its response.
pub(crate) const TURN_TTL_SECONDS: i64 = DEFAULT_TTL_SECONDS;

/// The verified human a turn belongs to. The scope key is `principal_id`;
/// the session fields are provenance recorded on the committed turn.
pub(crate) type TurnOwner = RequestOwner;

/// Chat-specific context for a pending turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatTurnContext {
    pub conversation_id: String,
    /// The conversation head when the turn opened; the response must land here.
    pub turn_index: u64,
}

impl RequestContext for ChatTurnContext {
    fn uniqueness_key(&self) -> Option<String> {
        // At most one open turn per conversation.
        Some(self.conversation_id.clone())
    }

    fn validate(&self) -> Result<()> {
        op_plugins::state_plugins::chat_response_methods::validate_uuid(
            "conversation_id",
            &self.conversation_id,
        )?;
        Ok(())
    }
}

/// A pending chat turn awaiting a response.
pub(crate) type PendingTurn = PendingRequest<ChatTurnContext>;

/// Exclusive right to answer one turn.
///
/// This is a newtype wrapper to add backward-compatible methods.
pub(crate) struct TurnClaim(RequestClaim<ChatTurnContext>);

impl TurnClaim {
    /// Get the pending turn this claim is for.
    pub(crate) fn turn(&self) -> &PendingTurn {
        self.0.request()
    }

    /// Mark the turn as answered and settle the claim.
    pub(crate) fn answered(self) {
        self.0.fulfilled();
    }

    /// Release the claim without answering (returns turn to pending).
    pub(crate) fn release(self) {
        self.0.release();
    }
}

/// Chat turn registry backed by the generic request registry.
///
/// The registry is wrapped in Arc internally to support the claim pattern
/// where the claim holds a reference back to the registry.
pub(crate) struct TurnRegistry {
    inner: Arc<RequestRegistry<ChatTurnContext>>,
}

impl Default for TurnRegistry {
    fn default() -> Self {
        Self {
            inner: Arc::new(RequestRegistry::default()),
        }
    }
}

impl TurnRegistry {
    /// Open a turn now with the default lifetime.
    pub(crate) fn open(
        &self,
        owner: TurnOwner,
        conversation_id: &str,
        turn_index: u64,
        responder_principal_id: &str,
    ) -> Result<PendingTurn> {
        self.open_at(
            owner,
            conversation_id,
            turn_index,
            responder_principal_id,
            Utc::now(),
            Duration::seconds(TURN_TTL_SECONDS),
        )
    }

    /// Withdraw a turn that was reserved but never offered to its responder,
    /// because the human message it follows failed to commit. Only a turn
    /// still pending is removed; one being answered or answered stays.
    pub(crate) fn cancel(&self, turn_id: &str) {
        self.inner.cancel(turn_id);
    }

    /// Open a turn at `now` for `ttl`. At most one open turn per conversation:
    /// its index is the durable head, so the conversation chain stays gapless.
    pub(crate) fn open_at(
        &self,
        owner: TurnOwner,
        conversation_id: &str,
        turn_index: u64,
        responder_principal_id: &str,
        now: DateTime<Utc>,
        ttl: Duration,
    ) -> Result<PendingTurn> {
        self.inner.open_at(
            owner,
            responder_principal_id,
            ChatTurnContext {
                conversation_id: conversation_id.to_string(),
                turn_index,
            },
            now,
            ttl,
        )
    }

    /// Claim a pending turn for its designated responder.
    pub(crate) fn claim(&self, turn_id: &str, caller: &str) -> Result<TurnClaim> {
        self.claim_at(turn_id, caller, Utc::now())
    }

    /// Claim a pending turn for its designated responder at a specific time.
    /// Nothing changes when the claim is refused.
    pub(crate) fn claim_at(
        &self,
        turn_id: &str,
        caller: &str,
        now: DateTime<Utc>,
    ) -> Result<TurnClaim> {
        self.inner.claim_at(turn_id, caller, now).map(TurnClaim)
    }

    /// List pending turns for a given responder principal.
    pub(crate) fn list_pending_for_responder(
        &self,
        responder_principal_id: &str,
    ) -> Vec<PendingTurn> {
        self.inner
            .list_pending_for_responder(responder_principal_id)
    }

    #[cfg(test)]
    pub(crate) fn is_open(&self, turn_id: &str) -> bool {
        self.inner.is_open(turn_id)
    }
}

// Convenience accessors for PendingTurn to maintain backward compatibility.
impl PendingTurn {
    /// The turn ID (alias for request_id).
    pub(crate) fn turn_id(&self) -> &str {
        &self.request_id
    }

    /// The conversation this turn belongs to.
    pub(crate) fn conversation_id(&self) -> &str {
        &self.context.conversation_id
    }

    /// The turn index in the conversation.
    pub(crate) fn turn_index(&self) -> u64 {
        self.context.turn_index
    }

    /// Create a pending turn for a human message (not going through the registry).
    /// This is used when committing a human's own message to the conversation.
    pub(crate) fn for_human_message(
        owner: TurnOwner,
        conversation_id: &str,
        turn_index: u64,
    ) -> Self {
        let now = chrono::Utc::now();
        Self {
            request_id: uuid::Uuid::now_v7().to_string(),
            owner: owner.clone(),
            responder_principal_id: owner.principal_id.clone(),
            opened_at: now,
            expires_at: now,
            context: ChatTurnContext {
                conversation_id: conversation_id.to_string(),
                turn_index,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HUMAN: &str = "human-principal";
    const AGENT: &str = "agent-principal";

    fn owner(session: &str) -> TurnOwner {
        TurnOwner {
            principal_id: HUMAN.into(),
            session_id: session.into(),
            session_genesis: "ab".repeat(32),
        }
    }

    fn conversation() -> String {
        uuid::Uuid::now_v7().to_string()
    }

    #[test]
    fn turn_opens_with_valid_conversation() {
        let registry = TurnRegistry::default();
        let conversation = conversation();
        let turn = registry.open(owner("s1"), &conversation, 0, AGENT).unwrap();
        assert_eq!(
            uuid::Uuid::parse_str(turn.turn_id())
                .unwrap()
                .get_version_num(),
            7
        );
        assert_eq!(turn.conversation_id(), conversation);
        assert_eq!(turn.turn_index(), 0);
        assert_eq!(turn.owner.principal_id, HUMAN);
        assert_eq!(turn.responder_principal_id, AGENT);
    }

    #[test]
    fn invalid_conversation_id_rejected() {
        let registry = TurnRegistry::default();
        assert!(registry.open(owner("s1"), "not-a-uuid", 0, AGENT).is_err());
    }

    #[test]
    fn one_open_turn_per_conversation() {
        let registry = TurnRegistry::default();
        let conversation = conversation();
        registry.open(owner("s1"), &conversation, 0, AGENT).unwrap();
        assert!(registry.open(owner("s1"), &conversation, 0, AGENT).is_err());
    }

    #[test]
    fn list_pending_for_responder() {
        let registry = TurnRegistry::default();
        let c1 = conversation();
        let c2 = conversation();
        let t1 = registry.open(owner("s1"), &c1, 0, AGENT).unwrap();
        let t2 = registry.open(owner("s2"), &c2, 0, AGENT).unwrap();
        let _t3 = registry
            .open(owner("s3"), &conversation(), 0, "other")
            .unwrap();

        let pending = registry.list_pending_for_responder(AGENT);
        assert_eq!(pending.len(), 2);
        let ids: Vec<_> = pending.iter().map(|t| t.turn_id()).collect();
        assert!(ids.contains(&t1.turn_id()));
        assert!(ids.contains(&t2.turn_id()));
    }

    #[test]
    fn cancel_removes_pending_turn() {
        let registry = TurnRegistry::default();
        let conversation = conversation();
        let turn = registry.open(owner("s1"), &conversation, 0, AGENT).unwrap();
        assert!(registry.is_open(turn.turn_id()));
        registry.cancel(turn.turn_id());
        assert!(!registry.is_open(turn.turn_id()));
        // Can open again after cancel.
        registry.open(owner("s1"), &conversation, 0, AGENT).unwrap();
    }
}
