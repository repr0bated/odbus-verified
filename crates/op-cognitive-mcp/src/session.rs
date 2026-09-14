//! Session Manager — Conversation Memory (R2, R10)
//!
//! Provides conversation_id-based session tracking for follow-up queries.
//! Sessions are stored in SQLite alongside the memory store for durability.
//! Each session holds the conversation context and query history.
//!
//! Operations: create, get, list, reset, close.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// A conversation session for NotebookLM follow-up queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSession {
    pub id: String,
    pub notebook_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub query_count: u32,
    /// Last N queries for context window (sliding window).
    pub history: Vec<QueryTurn>,
    pub active: bool,
}

/// A single query/answer turn within a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryTurn {
    pub query: String,
    pub answer: String,
    pub timestamp: DateTime<Utc>,
    pub citations_count: u32,
    pub grounded: bool,
}

/// Session manager backed by in-memory DashMap with optional SQLite persistence.
/// Phase 1 uses in-memory only; Phase 3 will add SQLite backing via the
/// CognitiveMemoryStore's pool.
pub struct SessionManager {
    sessions: Arc<DashMap<String, ConversationSession>>,
    /// Maximum turns kept per conversation before eviction.
    max_history: usize,
}

impl SessionManager {
    pub fn new(max_history: usize) -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
            max_history,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(20)
    }

    /// Get or create a session for a conversation_id.
    /// If conversation_id is empty, generates a new one.
    pub fn get_or_create(&self, conversation_id: &str, notebook_id: &str) -> ConversationSession {
        let id = if conversation_id.is_empty() {
            Uuid::new_v4().to_string()
        } else {
            conversation_id.to_string()
        };

        self.sessions
            .entry(id.clone())
            .or_insert_with(|| ConversationSession {
                id: id.clone(),
                notebook_id: notebook_id.to_string(),
                created_at: Utc::now(),
                updated_at: Utc::now(),
                query_count: 0,
                history: Vec::new(),
                active: true,
            })
            .clone()
    }

    /// Append a query turn to the conversation and return the updated session.
    pub fn append_turn(
        &self,
        conversation_id: &str,
        turn: QueryTurn,
    ) -> Result<ConversationSession> {
        let mut entry = self
            .sessions
            .get_mut(conversation_id)
            .context(format!("session '{}' not found", conversation_id))?;

        entry.query_count += 1;
        entry.updated_at = Utc::now();
        entry.history.push(turn);

        // Evict oldest turns beyond max_history
        while entry.history.len() > self.max_history {
            entry.history.remove(0);
        }

        Ok(entry.clone())
    }

    /// List all active sessions.
    pub fn list_sessions(&self) -> Vec<ConversationSession> {
        self.sessions
            .iter()
            .filter(|entry| entry.active)
            .map(|entry| entry.value().clone())
            .collect()
    }

    /// Reset a session (clear history, keep ID).
    pub fn reset_session(&self, conversation_id: &str) -> Result<ConversationSession> {
        let mut entry = self
            .sessions
            .get_mut(conversation_id)
            .context(format!("session '{}' not found", conversation_id))?;

        entry.history.clear();
        entry.query_count = 0;
        entry.updated_at = Utc::now();
        Ok(entry.clone())
    }

    /// Close a session (marks inactive, retains for audit).
    pub fn close_session(&self, conversation_id: &str) -> Result<()> {
        let mut entry = self
            .sessions
            .get_mut(conversation_id)
            .context(format!("session '{}' not found", conversation_id))?;

        entry.active = false;
        entry.updated_at = Utc::now();
        Ok(())
    }

    /// Get a specific session.
    pub fn get_session(&self, conversation_id: &str) -> Option<ConversationSession> {
        self.sessions.get(conversation_id).map(|e| e.clone())
    }

    /// Total session count.
    pub fn count(&self) -> usize {
        self.sessions.len()
    }

    /// Active session count.
    pub fn active_count(&self) -> usize {
        self.sessions.iter().filter(|e| e.active).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_create_session_with_generated_id() {
        let mgr = SessionManager::with_defaults();
        let session = mgr.get_or_create("", "notebook-1");
        assert!(!session.id.is_empty());
        assert_eq!(session.notebook_id, "notebook-1");
        assert!(session.active);
    }

    #[test]
    fn should_reuse_existing_session() {
        let mgr = SessionManager::with_defaults();
        let s1 = mgr.get_or_create("conv-abc", "notebook-1");
        let s2 = mgr.get_or_create("conv-abc", "notebook-1");
        assert_eq!(s1.id, s2.id);
    }

    #[test]
    fn should_append_and_evict_turns() {
        let mgr = SessionManager::new(2);
        let session = mgr.get_or_create("conv-x", "nb-1");
        assert_eq!(session.query_count, 0);

        for i in 0..5 {
            mgr.append_turn(
                "conv-x",
                QueryTurn {
                    query: format!("q{}", i),
                    answer: format!("a{}", i),
                    timestamp: Utc::now(),
                    citations_count: 0,
                    grounded: true,
                },
            )
            .unwrap();
        }

        let updated = mgr.get_session("conv-x").unwrap();
        assert_eq!(updated.query_count, 5);
        // Only last 2 turns kept
        assert_eq!(updated.history.len(), 2);
        assert_eq!(updated.history[0].query, "q3");
    }

    #[test]
    fn should_reset_session() {
        let mgr = SessionManager::with_defaults();
        mgr.get_or_create("conv-r", "nb-1");
        mgr.append_turn(
            "conv-r",
            QueryTurn {
                query: "q".into(),
                answer: "a".into(),
                timestamp: Utc::now(),
                citations_count: 0,
                grounded: true,
            },
        )
        .unwrap();

        let reset = mgr.reset_session("conv-r").unwrap();
        assert_eq!(reset.query_count, 0);
        assert!(reset.history.is_empty());
    }

    #[test]
    fn should_close_session() {
        let mgr = SessionManager::with_defaults();
        mgr.get_or_create("conv-c", "nb-1");
        mgr.close_session("conv-c").unwrap();

        let session = mgr.get_session("conv-c").unwrap();
        assert!(!session.active);
        assert_eq!(mgr.active_count(), 0);
    }
}
