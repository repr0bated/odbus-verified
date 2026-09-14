//! JSON flat-file chat session store.
//!
//! No SQLite, no drift. Desired state = file contents.
//! Every mutation rewrites the entire file atomically (write+rename).

use anyhow::{bail, Result};
use op_llm::provider::ChatMessage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

const DEFAULT_PATH: &str = "/var/lib/op-dbus/chat-sessions.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedSession {
    messages: Vec<ChatMessage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    container_id: Option<String>,
    created_at: String,
    updated_at: String,
}

/// In-memory projection of chat sessions with atomic JSON persistence.
pub struct ChatSessionStore {
    path: PathBuf,
    data: RwLock<HashMap<String, PersistedSession>>,
}

impl ChatSessionStore {
    pub async fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let data = if path.exists() {
            match tokio::fs::read_to_string(&path).await {
                Ok(contents) => {
                    match serde_json::from_str::<HashMap<String, PersistedSession>>(&contents) {
                        Ok(d) => {
                            info!(sessions = d.len(), "Loaded chat sessions from JSON");
                            d
                        }
                        Err(e) => {
                            warn!(error = %e, "Corrupt chat JSON, starting fresh");
                            HashMap::new()
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Failed to read chat JSON, starting fresh");
                    HashMap::new()
                }
            }
        } else {
            info!("No chat sessions file found, starting fresh");
            HashMap::new()
        };

        Ok(Self {
            path,
            data: RwLock::new(data),
        })
    }

    pub async fn default_store() -> Result<Self> {
        Self::new(DEFAULT_PATH).await
    }

    pub async fn list_sessions(&self) -> Vec<(String, usize, String)> {
        let guard = self.data.read().await;
        guard
            .iter()
            .map(|(id, session)| {
                let title = session
                    .messages
                    .first()
                    .map(|m| m.content.chars().take(50).collect())
                    .unwrap_or_else(|| "Empty session".to_string());
                (id.clone(), session.messages.len(), title)
            })
            .collect()
    }

    pub async fn get_messages(&self, session_id: &str) -> Option<Vec<ChatMessage>> {
        let guard = self.data.read().await;
        guard.get(session_id).map(|s| s.messages.clone())
    }

    pub async fn get_container_id(&self, session_id: &str) -> Option<String> {
        let guard = self.data.read().await;
        guard.get(session_id).and_then(|s| s.container_id.clone())
    }

    pub async fn set_container_id(&self, session_id: &str, container_id: String) -> Result<()> {
        let mut guard = self.data.write().await;
        if let Some(session) = guard.get_mut(session_id) {
            session.container_id = Some(container_id);
            session.updated_at = chrono::Utc::now().to_rfc3339();
        }
        drop(guard);
        self.flush().await
    }

    pub async fn create_session(&self, session_id: String, _title: Option<String>) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut guard = self.data.write().await;
        guard.insert(
            session_id,
            PersistedSession {
                messages: vec![],
                container_id: None,
                created_at: now.clone(),
                updated_at: now,
            },
        );
        drop(guard);
        self.flush().await?;
        Ok(())
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<bool> {
        let mut guard = self.data.write().await;
        let removed = guard.remove(session_id).is_some();
        drop(guard);
        if removed {
            self.flush().await?;
        }
        Ok(removed)
    }

    pub async fn append_message(&self, session_id: &str, message: ChatMessage) -> Result<()> {
        let mut guard = self.data.write().await;
        if let Some(session) = guard.get_mut(session_id) {
            session.messages.push(message);
            session.updated_at = chrono::Utc::now().to_rfc3339();
            drop(guard);
            self.flush().await?;
            Ok(())
        } else {
            bail!("Session not found: {}", session_id);
        }
    }

    pub async fn get_history(&self, session_id: &str) -> Option<Vec<ChatMessage>> {
        let guard = self.data.read().await;
        guard.get(session_id).map(|s| s.messages.clone())
    }

    /// Atomic flush: write to temp file, then rename.
    async fn flush(&self) -> Result<()> {
        let guard = self.data.read().await;
        let json = serde_json::to_string_pretty(&*guard)?;
        drop(guard);

        let tmp = self.path.with_extension("tmp");
        tokio::fs::write(&tmp, json).await?;
        tokio::fs::rename(&tmp, &self.path).await?;

        debug!(path = %self.path.display(), "Flushed chat sessions to JSON");
        Ok(())
    }
}
