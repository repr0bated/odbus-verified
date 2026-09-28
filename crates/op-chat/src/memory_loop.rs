//! Memory Loop — Hierarchical memory injection + post-turn persistence
//!
//! Architecture:
//! 1. System-wide memory — global CozoDB namespace, all authenticated entities
//! 2. Chatbot soul — the singleton control-plane chatbot's memory identity
//! 3. Container memory — user gets container at registration, soul + domain namespace
//!
//! Post-turn: detect memorable info, write to CozoDB (durable) + Qdrant (semantic)
//! MEMORY_INDEX: one key per namespace, slug-per-line orientation

/// Existing soul-record key for the control-plane chatbot, not a container ID.
pub const CHATBOT_SOUL_ID: &str = "assistant";

use anyhow::{Context, Result};
use chrono::Utc;
use op_cognitive_mcp::memory_store::{CognitiveMemoryStore, EntryQuery, MemoryEntry};
use op_cognitive_mcp::qdrant_shuttle::QdrantSemanticShuttle;
use op_llm::provider::ChatMessage;
use regex::Regex;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// Memory categories for detection and retrieval
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryCategory {
    User,
    Project,
    Feedback,
    Reference,
}

impl MemoryCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryCategory::User => "user",
            MemoryCategory::Project => "project",
            MemoryCategory::Feedback => "feedback",
            MemoryCategory::Reference => "reference",
        }
    }
}

/// Four-layer memory injection result
#[derive(Debug, Clone)]
pub struct SessionMemory {
    pub system_block: String, // system-wide global memory
    pub soul_block: String,   // chatbot soul (chatbot is a user entity)
    pub domain_block: String, // user container categorized memories
    pub index_block: String,  // MEMORY_INDEX orientation
}

impl SessionMemory {
    /// Combine all blocks into system prompt appendix
    pub fn to_system_prompt(&self) -> String {
        let mut parts = Vec::new();
        if !self.system_block.is_empty() {
            parts.push(format!("## System Memory\n{}", self.system_block));
        }
        if !self.soul_block.is_empty() {
            parts.push(format!("## Soul Memory\n{}", self.soul_block));
        }
        if !self.index_block.is_empty() {
            parts.push(format!("## Domain Memory Index\n{}", self.index_block));
        }
        if !self.domain_block.is_empty() {
            parts.push(format!("## Domain Memory\n{}", self.domain_block));
        }
        parts.join("\n\n")
    }
}

/// Memory loop — owns CozoDB + Qdrant for chatbot memory
pub struct MemoryLoop {
    cozo: Arc<CognitiveMemoryStore>,
    qdrant: Option<Arc<QdrantSemanticShuttle>>,
    /// Regex for detecting memorable phrases in conversation
    memorable_regex: Regex,
}

impl MemoryLoop {
    pub fn new(cozo: Arc<CognitiveMemoryStore>) -> Self {
        Self {
            cozo,
            qdrant: None,
            memorable_regex: Regex::new(
                r"(?i)\b(always|never|prefer|my\s+(?:name|email|project)|our\s+project|decided\s+to|goal\s+is|important|remember\s+that|don't\s+forget)\b"
            ).expect("hardcoded regex pattern is valid"),
        }
    }

    pub fn with_qdrant(mut self, qdrant: Arc<QdrantSemanticShuttle>) -> Self {
        self.qdrant = Some(qdrant);
        self
    }

    /// Namespace name for a container's domain memory
    fn namespace_for(container_id: &str) -> String {
        format!("container:{}", container_id)
    }

    // ── Session Start Injection ──────────────────────────────────────────

    /// Load all four memory layers for a session start
    ///
    /// Hierarchy:
    /// 1. System-wide memory (global) — all authenticated entities
    /// 2. Chatbot soul — chatbot is a user entity, session-scoped
    /// 3. User container memory — assigned at registration, soul + domain namespace
    pub async fn inject_session_memory(
        &self,
        chatbot_soul_id: &str,   // e.g. "chatbot" or session-scoped id
        user_container_id: &str, // user's assigned container (work/home/...)
        opening_message: &str,
    ) -> Result<SessionMemory> {
        let user_ns = Self::namespace_for(user_container_id);

        // 1. System-wide memory — global namespace, all users/chatbots
        let system_block = self.load_system_memory().await.unwrap_or_default();

        // 2. Chatbot soul — chatbot is a user entity with its own soul
        let soul_block = self.load_soul(chatbot_soul_id).await.unwrap_or_default();

        // 3. User container domain memory — MEMORY_INDEX + full entries
        let (index_block, domain_block) = self
            .load_domain_memory(&user_ns, Some(opening_message))
            .await?;

        Ok(SessionMemory {
            system_block,
            soul_block,
            domain_block,
            index_block,
        })
    }

    /// Load system-wide global memory from CozoDB
    async fn load_system_memory(&self) -> Result<String> {
        let entry = self
            .cozo
            .retrieve_entry("system:global", "memory")
            .await
            .context("system memory lookup failed")?;

        Ok(entry.map(|e| format!("{}", e.value)).unwrap_or_default())
    }

    /// Load soul for a user entity (chatbot or container) from CognitiveMemoryStore
    async fn load_soul(&self, soul_id: &str) -> Result<String> {
        let entry = self
            .cozo
            .retrieve_entry("soul", soul_id)
            .await
            .context("soul lookup failed")?;

        Ok(entry.map(|e| format!("{}", e.value)).unwrap_or_default())
    }

    /// Load domain memory: MEMORY_INDEX orientation + full entries (+ optional semantic query)
    async fn load_domain_memory(
        &self,
        namespace: &str,
        query_text: Option<&str>,
    ) -> Result<(String, String)> {
        // MEMORY_INDEX key gives orientation
        let index = self
            .cozo
            .retrieve_entry(namespace, "MEMORY_INDEX")
            .await
            .context("MEMORY_INDEX lookup")?;

        let index_block = index
            .map(|e| e.value.as_str().map(|s| s.to_string()).unwrap_or_default())
            .unwrap_or_default();

        // Query full entries (excluding index itself)
        let entries = self
            .cozo
            .query_entries(EntryQuery {
                namespace_id: Some(namespace.to_string()),
                key_pattern: None,
                tags: None,
                limit: Some(50),
                offset: Some(0),
            })
            .await
            .context("domain memory query")?;

        // Filter out MEMORY_INDEX and format
        let domain_block = Self::format_entries(&entries);

        // If Qdrant available and query text provided, do semantic boost
        if let (Some(qdrant), Some(query)) = (&self.qdrant, query_text) {
            match Self::semantic_boost(qdrant, namespace, query, &entries).await {
                Ok(boosted) => return Ok((index_block, boosted)),
                Err(e) => warn!("semantic boost failed, using full entries: {}", e),
            }
        }

        Ok((index_block, domain_block))
    }

    /// Semantic boost: embed query, search Qdrant, return matched entries in priority order
    async fn semantic_boost(
        qdrant: &Arc<QdrantSemanticShuttle>,
        namespace: &str,
        query: &str,
        entries: &[MemoryEntry],
    ) -> Result<String> {
        if entries.is_empty() {
            return Ok(String::new());
        }

        let container_id = namespace.strip_prefix("container:").unwrap_or(namespace);
        let embedding = qdrant
            .embed_query_text(query)
            .await
            .context("failed to vectorize the session memory query")?;
        let hits = qdrant
            .search_user_memory(embedding, container_id, entries.len() as u64)
            .await
            .context("failed to search vectorized session memory")?;

        // Qdrant determines priority, while CozoDB remains the source of truth
        // for the value and tags injected into the prompt. Ignore stale vector
        // hits, de-duplicate entry keys, then append unmatched Cozo entries so
        // semantic retrieval boosts relevant memory without hiding the rest.
        let mut prioritized = Vec::with_capacity(entries.len());
        for hit in hits {
            let Some(entry_key) = hit
                .payload
                .get("entry_key")
                .and_then(|value| value.as_str())
            else {
                continue;
            };
            if prioritized
                .iter()
                .any(|entry: &&MemoryEntry| entry.key.as_str() == entry_key.as_str())
            {
                continue;
            }
            if let Some(entry) = entries
                .iter()
                .find(|entry| entry.key.as_str() == entry_key.as_str())
            {
                prioritized.push(entry);
            }
        }
        for entry in entries {
            if !prioritized.iter().any(|current| current.key == entry.key) {
                prioritized.push(entry);
            }
        }

        Ok(Self::format_entries(
            &prioritized.into_iter().cloned().collect::<Vec<_>>(),
        ))
    }

    fn format_entries(entries: &[MemoryEntry]) -> String {
        let mut lines = Vec::new();
        for e in entries.iter().filter(|e| e.key != "MEMORY_INDEX") {
            let cat = e.tags.first().map(|t| t.as_str()).unwrap_or("memory");
            let preview = match &e.value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            let truncated = if preview.len() > 200 {
                format!("{}...", &preview[..200])
            } else {
                preview
            };
            lines.push(format!("- [{}] {}: {}", cat, e.key, truncated));
        }
        lines.join("\n")
    }

    // ── Post-Turn Memory Detection ───────────────────────────────────────

    /// Spawn non-blocking post-turn memory task
    pub fn spawn_post_turn_memory_task(
        &self,
        container_id: String,
        user_message: String,
        assistant_response: String,
        tool_calls: Vec<(String, Value)>,
    ) -> JoinHandle<Result<()>> {
        let cozo = Arc::clone(&self.cozo);
        let qdrant = self.qdrant.as_ref().map(Arc::clone);
        let regex = self.memorable_regex.clone();

        tokio::spawn(async move {
            let result = Self::post_turn_memory(
                &cozo,
                qdrant.as_ref(),
                &regex,
                &container_id,
                &user_message,
                &assistant_response,
                &tool_calls,
            )
            .await;
            if let Err(e) = &result {
                warn!("Post-turn memory task failed: {}", e);
            }
            result
        })
    }

    async fn post_turn_memory(
        cozo: &Arc<CognitiveMemoryStore>,
        qdrant: Option<&Arc<QdrantSemanticShuttle>>,
        regex: &Regex,
        container_id: &str,
        user_message: &str,
        assistant_response: &str,
        tool_calls: &[(String, Value)],
    ) -> Result<()> {
        let ns = Self::namespace_for(container_id);
        let mut new_memories: Vec<(String, MemoryCategory, Value)> = Vec::new();

        // Detect memorable patterns in user message
        if regex.is_match(user_message) {
            let cat = Self::categorize(user_message);
            let key = Self::memory_key(user_message);
            new_memories.push((key, cat, json!(user_message)));
        }

        // Detect memorable patterns in assistant response (preferences, decisions)
        if regex.is_match(assistant_response) {
            let cat = MemoryCategory::Feedback;
            let key = format!("feedback-{}", Utc::now().timestamp());
            new_memories.push((key, cat, json!(assistant_response)));
        }

        // Tool results as reference memory
        for (tool_name, args) in tool_calls.iter().filter(|(n, _)| {
            n.starts_with("ovs_") || n.starts_with("sv_") || n.starts_with("incus_")
        }) {
            let key = format!("ref-{}-{}", tool_name, Utc::now().timestamp());
            new_memories.push((
                key,
                MemoryCategory::Reference,
                json!({"tool": tool_name, "args": args}),
            ));
        }

        let detected_memories = new_memories.len();
        let mut stored_memories = Vec::with_capacity(detected_memories);

        // Write to CozoDB (durable). The returned entry id is also the stable
        // Qdrant point id, keeping repeated writes aligned across both stores.
        for (key, cat, value) in &new_memories {
            match cozo
                .store_entry(
                    &ns,
                    key,
                    value.clone(),
                    vec![cat.as_str().to_string()],
                    None,
                )
                .await
            {
                Ok(entry) => {
                    debug!("Stored memory: {} in {}", key, ns);
                    stored_memories.push((key.clone(), *cat, value.clone(), entry.id));
                }
                Err(e) => warn!("Failed to store memory entry {}: {}", key, e),
            }
        }

        // Update MEMORY_INDEX
        if !stored_memories.is_empty() {
            let index_entries = stored_memories
                .iter()
                .map(|(key, cat, value, _)| (key.clone(), *cat, value.clone()))
                .collect::<Vec<_>>();
            Self::update_memory_index(cozo, &ns, &index_entries).await?;
        }

        // Mirror successfully persisted memories into Qdrant. Semantic storage
        // is best-effort: a Voyage/Qdrant outage must not roll back durable Cozo
        // memory or prevent the remaining memories from being attempted.
        if let Some(qdrant) = qdrant {
            for (key, _, value, point_id) in &stored_memories {
                let content = Self::memory_content(value);
                if content.trim().is_empty() {
                    continue;
                }

                let result: Result<()> = async {
                    let vector = qdrant
                        .embed_document(&content)
                        .await
                        .with_context(|| format!("failed to vectorize memory '{key}'"))?;
                    qdrant
                        .upsert_user_memory(point_id.clone(), vector, container_id, key, &content)
                        .await
                        .with_context(|| format!("failed to upsert vectorized memory '{key}'"))
                }
                .await;

                if let Err(error) = result {
                    warn!(entry_key = key, %error, "Semantic memory mirror failed");
                }
            }
        }

        info!(
            container_id,
            namespace = ns,
            detected_memories,
            stored_memories = stored_memories.len(),
            "Post-turn memory persisted"
        );

        Ok(())
    }

    fn categorize(text: &str) -> MemoryCategory {
        let lower = text.to_lowercase();
        if lower.contains("project") || lower.contains("goal") || lower.contains("decided") {
            MemoryCategory::Project
        } else if lower.contains("prefer") || lower.contains("always") || lower.contains("never") {
            MemoryCategory::User
        } else if lower.contains("correct") || lower.contains("wrong") || lower.contains("should") {
            MemoryCategory::Feedback
        } else {
            MemoryCategory::Reference
        }
    }

    fn memory_key(text: &str) -> String {
        // Generate a stable key from first few words
        let words: Vec<&str> = text.split_whitespace().take(4).collect();
        let slug = words.join("-").to_lowercase();
        let slug = Regex::new(r"[^a-z0-9-]")
            .expect("hardcoded regex pattern is valid")
            .replace_all(&slug, "");
        format!("{}-{}", slug, Utc::now().timestamp())
    }

    fn memory_content(value: &Value) -> String {
        value
            .as_str()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_default())
    }

    async fn update_memory_index(
        cozo: &Arc<CognitiveMemoryStore>,
        namespace: &str,
        new_entries: &[(String, MemoryCategory, Value)],
    ) -> Result<()> {
        // Read existing index
        let existing = cozo
            .retrieve_entry(namespace, "MEMORY_INDEX")
            .await
            .unwrap_or_default();

        let mut lines: Vec<String> = existing
            .map(|e| e.value.as_str().map(|s| s.to_string()).unwrap_or_default())
            .unwrap_or_default()
            .lines()
            .map(|s| s.to_string())
            .collect();

        // Append new entries as one-liner slugs
        for (key, cat, value) in new_entries {
            let preview = match value {
                Value::String(s) => {
                    if s.len() > 60 {
                        format!("{}...", &s[..60])
                    } else {
                        s.clone()
                    }
                }
                other => other.to_string().chars().take(60).collect(),
            };
            lines.push(format!("{} — [{}] {}", key, cat.as_str(), preview));
        }

        // Deduplicate and limit
        lines.sort();
        lines.dedup();
        if lines.len() > 100 {
            let skip = lines.len() - 100;
            lines = lines.into_iter().skip(skip).collect();
        }

        let index_value = json!(lines.join("\n"));
        cozo.store_entry(
            namespace,
            "MEMORY_INDEX",
            index_value,
            vec!["index".to_string()],
            None,
        )
        .await
        .context("update MEMORY_INDEX")?;

        Ok(())
    }
}

// ── Integration Helpers ──────────────────────────────────────────────────

/// Inject session memory into a ChatMessage vec (as system messages)
pub fn inject_memory_into_messages(messages: &mut Vec<ChatMessage>, memory: &SessionMemory) {
    let prompt = memory.to_system_prompt();
    if !prompt.is_empty() {
        // Insert as first system message (after any existing system messages)
        let sys_msg = ChatMessage::system(prompt);
        // Find position after existing system messages
        let pos = messages
            .iter()
            .position(|m| m.role != "system")
            .unwrap_or(0);
        messages.insert(pos, sys_msg);
    }
}
