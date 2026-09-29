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

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, ensure};
use chrono::{DateTime, Duration, Utc};

/// How long an opened turn waits for its response.
pub(crate) const TURN_TTL_SECONDS: i64 = 15 * 60;
/// Bounds on open turns (pending or being answered).
pub(crate) const MAX_OPEN_TURNS: usize = 4096;
pub(crate) const MAX_OPEN_TURNS_PER_PRINCIPAL: usize = 16;

/// The verified human a turn belongs to. The scope key is `principal_id`;
/// the session fields are provenance recorded on the committed turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TurnOwner {
    pub principal_id: String,
    pub session_id: String,
    pub session_genesis: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingTurn {
    pub turn_id: String,
    pub owner: TurnOwner,
    pub conversation_id: String,
    /// The conversation head when the turn opened; the response must land here.
    pub turn_index: u64,
    pub responder_principal_id: String,
    pub opened_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnState {
    Pending,
    Answering,
    Answered,
}

struct Entry {
    turn: PendingTurn,
    state: TurnState,
}

#[derive(Default)]
pub(crate) struct TurnRegistry {
    turns: Mutex<HashMap<String, Entry>>,
}

impl TurnRegistry {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        // A poisoned map still holds consistent entries: every mutation is a
        // single insert/remove/state assignment.
        self.turns
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn collect_expired(turns: &mut HashMap<String, Entry>, now: DateTime<Utc>) {
        // An answer in flight keeps its entry until the commit settles.
        turns.retain(|_, entry| entry.state == TurnState::Answering || entry.turn.expires_at > now);
    }

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
        let mut turns = self.lock();
        if turns
            .get(turn_id)
            .is_some_and(|entry| entry.state == TurnState::Pending)
        {
            turns.remove(turn_id);
        }
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
        ensure!(
            !owner.principal_id.is_empty()
                && !owner.session_id.is_empty()
                && !owner.session_genesis.is_empty(),
            "turn owner must be a verified identity"
        );
        ensure!(
            !responder_principal_id.is_empty(),
            "turn responder principal is required"
        );
        ensure!(
            responder_principal_id != owner.principal_id,
            "a human cannot be the designated responder of their own turn"
        );
        ensure!(
            ttl > Duration::zero() && ttl <= Duration::hours(1),
            "turn lifetime must be positive and at most one hour"
        );
        op_plugins::state_plugins::chat_response_methods::validate_uuid(
            "conversation_id",
            conversation_id,
        )?;
        let mut turns = self.lock();
        Self::collect_expired(&mut turns, now);
        ensure!(
            !turns.values().any(|entry| {
                entry.state != TurnState::Answered
                    && entry.turn.owner.principal_id == owner.principal_id
                    && entry.turn.conversation_id == conversation_id
            }),
            "the conversation already has an open turn"
        );
        ensure!(turns.len() < MAX_OPEN_TURNS, "too many open chat turns");
        ensure!(
            turns
                .values()
                .filter(|entry| {
                    entry.state != TurnState::Answered
                        && entry.turn.owner.principal_id == owner.principal_id
                })
                .count()
                < MAX_OPEN_TURNS_PER_PRINCIPAL,
            "too many open chat turns for this principal"
        );
        let turn = PendingTurn {
            turn_id: uuid::Uuid::now_v7().to_string(),
            owner,
            conversation_id: conversation_id.to_string(),
            turn_index,
            responder_principal_id: responder_principal_id.to_string(),
            opened_at: now,
            expires_at: now + ttl,
        };
        turns.insert(
            turn.turn_id.clone(),
            Entry {
                turn: turn.clone(),
                state: TurnState::Pending,
            },
        );
        Ok(turn)
    }

    pub(crate) fn claim(self: &Arc<Self>, turn_id: &str, caller: &str) -> Result<TurnClaim> {
        self.claim_at(turn_id, caller, Utc::now())
    }

    /// Claim a pending turn for its designated responder. Nothing changes
    /// when the claim is refused.
    pub(crate) fn claim_at(
        self: &Arc<Self>,
        turn_id: &str,
        caller: &str,
        now: DateTime<Utc>,
    ) -> Result<TurnClaim> {
        let mut turns = self.lock();
        let entry = turns
            .get_mut(turn_id)
            .context("chat turn is not open: the bridge never opened it or it was dropped")?;
        match entry.state {
            TurnState::Pending => {}
            TurnState::Answering => anyhow::bail!("chat turn is already being answered"),
            TurnState::Answered => anyhow::bail!("chat turn is already answered"),
        }
        if entry.turn.expires_at <= now {
            turns.remove(turn_id);
            anyhow::bail!("chat turn has expired");
        }
        ensure!(
            entry.turn.responder_principal_id == caller,
            "caller is not the designated responder of this chat turn"
        );
        entry.state = TurnState::Answering;
        Ok(TurnClaim {
            registry: self.clone(),
            turn: entry.turn.clone(),
            settled: false,
        })
    }

    fn settle(&self, turn_id: &str, answered: bool) {
        let mut turns = self.lock();
        if let Some(entry) = turns.get_mut(turn_id) {
            entry.state = if answered {
                TurnState::Answered
            } else {
                TurnState::Pending
            };
        }
    }

    #[cfg(test)]
    pub(crate) fn is_open(&self, turn_id: &str) -> bool {
        self.lock()
            .get(turn_id)
            .is_some_and(|entry| entry.state != TurnState::Answered)
    }
}

/// Exclusive right to answer one turn. Dropping it without `answered()`
/// returns the turn to pending (the durable owner still refuses a second
/// row, so a retry after an uncertain commit cannot answer twice).
pub(crate) struct TurnClaim {
    registry: Arc<TurnRegistry>,
    turn: PendingTurn,
    settled: bool,
}

impl TurnClaim {
    pub(crate) fn turn(&self) -> &PendingTurn {
        &self.turn
    }

    pub(crate) fn answered(mut self) {
        self.settled = true;
        self.registry.settle(&self.turn.turn_id, true);
    }

    pub(crate) fn release(mut self) {
        self.settled = true;
        self.registry.settle(&self.turn.turn_id, false);
    }
}

impl Drop for TurnClaim {
    fn drop(&mut self) {
        if !self.settled {
            self.registry.settle(&self.turn.turn_id, false);
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
    fn only_the_designated_responder_answers_an_open_turn_once() {
        let registry = Arc::new(TurnRegistry::default());
        let conversation = conversation();
        let turn = registry.open(owner("s1"), &conversation, 0, AGENT).unwrap();
        assert_eq!(
            uuid::Uuid::parse_str(&turn.turn_id)
                .unwrap()
                .get_version_num(),
            7
        );
        assert!(registry.claim(&turn.turn_id, "someone-else").is_err());
        assert!(registry.claim(&turn.turn_id, HUMAN).is_err());
        let claim = registry.claim(&turn.turn_id, AGENT).unwrap();
        assert_eq!(claim.turn().owner.principal_id, HUMAN);
        assert!(
            registry.claim(&turn.turn_id, AGENT).is_err(),
            "concurrent answer is refused while one is in flight"
        );
        // A failed commit returns the turn to pending.
        drop(claim);
        let claim = registry.claim(&turn.turn_id, AGENT).unwrap();
        claim.answered();
        let error = registry
            .claim(&turn.turn_id, AGENT)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("already answered"), "{error}");
        assert!(
            registry
                .claim(&uuid::Uuid::now_v7().to_string(), AGENT)
                .is_err(),
            "an unopened turn is refused"
        );
        // Once answered, the conversation can open its next turn.
        registry.open(owner("s2"), &conversation, 1, AGENT).unwrap();
    }

    #[test]
    fn expired_turns_are_refused_and_collected() {
        let registry = Arc::new(TurnRegistry::default());
        let conversation = conversation();
        let opened = Utc::now() - Duration::minutes(30);
        let turn = registry
            .open_at(
                owner("s1"),
                &conversation,
                0,
                AGENT,
                opened,
                Duration::minutes(15),
            )
            .unwrap();
        let error = registry
            .claim(&turn.turn_id, AGENT)
            .err()
            .unwrap()
            .to_string();
        assert!(error.contains("expired"), "{error}");
        assert!(!registry.is_open(&turn.turn_id));
        // An expired turn does not block the conversation.
        registry.open(owner("s1"), &conversation, 0, AGENT).unwrap();
    }

    #[test]
    fn one_open_turn_per_conversation_and_bounded_per_principal() {
        let registry = Arc::new(TurnRegistry::default());
        let conversation = conversation();
        registry.open(owner("s1"), &conversation, 0, AGENT).unwrap();
        assert!(registry.open(owner("s1"), &conversation, 0, AGENT).is_err());
        for _ in 1..MAX_OPEN_TURNS_PER_PRINCIPAL {
            registry
                .open(owner("s1"), &self::conversation(), 0, AGENT)
                .unwrap();
        }
        assert!(
            registry
                .open(owner("s1"), &self::conversation(), 0, AGENT)
                .is_err()
        );
        assert!(
            registry
                .open(owner("s1"), &self::conversation(), 0, HUMAN)
                .is_err(),
            "a human cannot answer their own turn"
        );
        assert!(registry.open(owner("s1"), "not-a-uuid", 0, AGENT).is_err());
    }
}
