//! Chat session management

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use op_core::ChatMessage;

/// A chat session containing message history
#[derive(Debug, Clone)]
pub struct ChatSession {
    pub id: String,
    pub name: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: HashMap<String, simd_json::OwnedValue>,
    /// Auth session ID (from WireGuard gateway)
    pub auth_session_id: Option<String>,
    /// Is this a controller session (you/chatbot)
    pub is_controller: bool,
    /// WireGuard peer public key
    pub peer_pubkey: Option<String>,
    /// Container ID for memory scoping (work/home/barmitzvah)
    pub container_id: Option<String>,
}

impl ChatSession {
    /// Create a new chat session
    pub fn new() -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name: None,
            messages: Vec::new(),
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
            auth_session_id: None,
            is_controller: false,
            peer_pubkey: None,
            container_id: None,
        }
    }

    /// Create with a specific ID
    pub fn with_id(id: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            name: None,
            messages: Vec::new(),
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
            auth_session_id: None,
            is_controller: false,
            peer_pubkey: None,
            container_id: None,
        }
    }

    /// Create authenticated session (from gateway)
    pub fn authenticated(
        auth_session_id: String,
        is_controller: bool,
        peer_pubkey: Option<String>,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name: None,
            messages: Vec::new(),
            created_at: now,
            updated_at: now,
            metadata: HashMap::new(),
            auth_session_id: Some(auth_session_id),
            is_controller,
            peer_pubkey,
            container_id: None,
        }
    }

    /// Add a message to the session
    pub fn add_message(&mut self, message: ChatMessage) {
        self.messages.push(message);
        self.updated_at = Utc::now();
    }

    /// Get the last N messages
    pub fn last_messages(&self, n: usize) -> &[ChatMessage] {
        let start = self.messages.len().saturating_sub(n);
        &self.messages[start..]
    }

    /// Clear all messages
    pub fn clear(&mut self) {
        self.messages.clear();
        self.updated_at = Utc::now();
    }

    /// Set session name
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = Some(name.into());
        self.updated_at = Utc::now();
    }

    /// Get message count
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }
}

impl Default for ChatSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Session manager for handling multiple chat sessions
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, ChatSession>>>,
    max_sessions: usize,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            max_sessions: 100,
        }
    }

    /// Create with custom max sessions
    pub fn with_max_sessions(max_sessions: usize) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            max_sessions,
        }
    }

    /// Create a new session
    pub async fn create(&self) -> ChatSession {
        let session = ChatSession::new();
        let id = session.id.clone();

        let mut sessions = self.sessions.write().await;

        // Evict oldest session if at capacity
        if sessions.len() >= self.max_sessions {
            if let Some(oldest_id) = sessions
                .values()
                .min_by_key(|s| s.updated_at)
                .map(|s| s.id.clone())
            {
                sessions.remove(&oldest_id);
            }
        }

        sessions.insert(id, session.clone());
        session
    }

    /// Get a session by ID
    pub async fn get(&self, id: &str) -> Option<ChatSession> {
        let sessions = self.sessions.read().await;
        sessions.get(id).cloned()
    }

    /// Get or create a session
    pub async fn get_or_create(&self, id: &str) -> ChatSession {
        {
            let sessions = self.sessions.read().await;
            if let Some(session) = sessions.get(id) {
                return session.clone();
            }
        }

        let session = ChatSession::with_id(id);
        let mut sessions = self.sessions.write().await;
        sessions.insert(id.to_string(), session.clone());
        session
    }

    /// Update a session
    pub async fn update(&self, session: ChatSession) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id.clone(), session);
    }

    /// Add a message to a session
    pub async fn add_message(&self, session_id: &str, message: ChatMessage) -> Option<()> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.add_message(message);
            Some(())
        } else {
            None
        }
    }

    /// Delete a session
    pub async fn delete(&self, id: &str) -> bool {
        let mut sessions = self.sessions.write().await;
        sessions.remove(id).is_some()
    }

    /// List all session IDs
    pub async fn list(&self) -> Vec<String> {
        let sessions = self.sessions.read().await;
        sessions.keys().cloned().collect()
    }

    /// List all sessions with basic info
    pub async fn list_sessions(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.read().await;
        sessions
            .values()
            .map(|s| SessionInfo {
                id: s.id.clone(),
                name: s.name.clone(),
                message_count: s.messages.len(),
                created_at: s.created_at,
                updated_at: s.updated_at,
            })
            .collect()
    }

    /// Get session count
    pub async fn count(&self) -> usize {
        let sessions = self.sessions.read().await;
        sessions.len()
    }

    /// Clear all sessions
    pub async fn clear(&self) {
        let mut sessions = self.sessions.write().await;
        sessions.clear();
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SessionManager {
    fn clone(&self) -> Self {
        Self {
            sessions: Arc::clone(&self.sessions),
            max_sessions: self.max_sessions,
        }
    }
}

/// Basic session info for listing
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub name: Option<String>,
    pub message_count: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_session_creation() {
        let manager = SessionManager::new();
        let session = manager.create().await;
        assert!(!session.id.is_empty());
        assert_eq!(session.message_count(), 0);
    }

    #[tokio::test]
    async fn test_add_message() {
        let manager = SessionManager::new();
        let session = manager.create().await;
        let id = session.id.clone();

        manager.add_message(&id, ChatMessage::user("Hello")).await;

        let updated = manager.get(&id).await.unwrap();
        assert_eq!(updated.message_count(), 1);
    }
}
