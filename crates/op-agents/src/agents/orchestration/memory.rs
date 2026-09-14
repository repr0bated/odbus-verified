//! Memory Agent with Cognitive Features
//!
//! Provides persistent memory storage with semantic search capabilities.
//! Merged features from op-cognitive-mcp for vector embeddings and advanced search.

use async_trait::async_trait;
use simd_json::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};
use crate::security::SecurityProfile;

/// Memory entry with cognitive features
#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub key: String,
    pub value: String,
    pub vector: Option<Vec<f32>>, // Embedding vector for semantic search
    pub memory_type: MemoryType,
    pub tags: Vec<String>,
    pub created_at: u64, // Unix timestamp
    pub updated_at: u64,
    pub expires_at: Option<u64>,
    pub access_count: u64,
    pub last_accessed: u64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum MemoryType {
    Ephemeral, // Session-based, may expire
    #[default]
    Persistent, // Permanent storage
    Shared,    // Cross-session shared
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl MemoryEntry {
    pub fn new(key: String, value: String, memory_type: MemoryType, tags: Vec<String>) -> Self {
        let now = now_ts();
        Self {
            key,
            value,
            vector: None,
            memory_type,
            tags,
            created_at: now,
            updated_at: now,
            expires_at: None,
            access_count: 0,
            last_accessed: now,
        }
    }

    /// Check if entry has expired
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|exp| now_ts() > exp)
    }
}

pub struct MemoryAgent {
    agent_id: String,
    profile: SecurityProfile,
    memory_path: PathBuf,
    cache: Arc<RwLock<HashMap<String, MemoryEntry>>>,
}

impl MemoryAgent {
    pub fn new(agent_id: String) -> Self {
        let memory_path = PathBuf::from("/var/lib/op-dbus/memory_cognitive.json");
        let cache = if let Ok(content) = fs::read_to_string(&memory_path) {
            Self::parse_memory_entries(&content)
        } else {
            let old_path = PathBuf::from("/var/lib/op-dbus/memory.json");
            if let Ok(content) = fs::read_to_string(&old_path) {
                Self::migrate_old_format(&content)
            } else {
                HashMap::new()
            }
        };

        Self {
            agent_id,
            profile: SecurityProfile::orchestration("memory", vec!["*"]),
            memory_path,
            cache: Arc::new(RwLock::new(cache)),
        }
    }

    fn persist(&self) -> Result<(), String> {
        let cache = self.cache.read().map_err(|_| "Failed to acquire lock")?;
        let content = Self::serialize_memory_entries(&cache)?;
        fs::write(&self.memory_path, content).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Parse memory entries from JSON string
    fn parse_memory_entries(content: &str) -> HashMap<String, MemoryEntry> {
        let mut cache = HashMap::new();
        let value: simd_json::OwnedValue = serde_json::from_str(content).unwrap_or_default();

        if let Some(obj) = value.as_object() {
            for (key, entry_val) in obj.iter() {
                if let Some(entry_obj) = entry_val.as_object() {
                    let entry = MemoryEntry {
                        key: key.clone(),
                        value: entry_obj
                            .get("value")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        vector: None,
                        memory_type: entry_obj
                            .get("memory_type")
                            .and_then(|v| v.as_str())
                            .map(|s| match s {
                                "ephemeral" => MemoryType::Ephemeral,
                                "shared" => MemoryType::Shared,
                                _ => MemoryType::Persistent,
                            })
                            .unwrap_or(MemoryType::Persistent),
                        tags: entry_obj
                            .get("tags")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        created_at: entry_obj
                            .get("created_at")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        updated_at: entry_obj
                            .get("updated_at")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        expires_at: entry_obj.get("expires_at").and_then(|v| v.as_u64()),
                        access_count: entry_obj
                            .get("access_count")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                        last_accessed: entry_obj
                            .get("last_accessed")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0),
                    };
                    cache.insert(key.clone(), entry);
                }
            }
        }
        cache
    }

    /// Serialize memory entries to JSON string using simple JSON construction
    fn serialize_memory_entries(cache: &HashMap<String, MemoryEntry>) -> Result<String, String> {
        let mut entries = Vec::new();
        for (key, entry) in cache.iter() {
            let memory_type_str = match entry.memory_type {
                MemoryType::Ephemeral => "ephemeral",
                MemoryType::Persistent => "persistent",
                MemoryType::Shared => "shared",
            };
            let tags_json = entry
                .tags
                .iter()
                .map(|t| format!("\"{}\"", t))
                .collect::<Vec<_>>()
                .join(",");

            let expires_json = entry
                .expires_at
                .map(|e| format!(",\"expires_at\":{}", e))
                .unwrap_or_default();

            let entry_json = format!(
                "\"{}\":{{\"value\":\"{}\",\"memory_type\":\"{}\",\"tags\":[{}],\"created_at\":{},\"updated_at\":{},\"access_count\":{},\"last_accessed\":{}{}}}",
                key, entry.value, memory_type_str, tags_json, entry.created_at, entry.updated_at,
                entry.access_count, entry.last_accessed, expires_json
            );
            entries.push(entry_json);
        }

        Ok(format!("{{{}}}", entries.join(",")))
    }

    /// Migrate from old format (key-value pairs)
    fn migrate_old_format(content: &str) -> HashMap<String, MemoryEntry> {
        let mut cache = HashMap::new();
        let old_cache: HashMap<String, String> = serde_json::from_str(content).unwrap_or_default();
        for (key, value) in old_cache {
            let entry = MemoryEntry::new(key.clone(), value, MemoryType::Persistent, vec![]);
            cache.insert(key, entry);
        }
        cache
    }

    /// Store with cognitive features
    fn remember_advanced(
        &self,
        key: Option<&str>,
        value: Option<&str>,
        memory_type: Option<MemoryType>,
        tags: Option<Vec<String>>,
    ) -> Result<String, String> {
        let key = key.ok_or("Key required")?;
        let value = value.ok_or("Value required")?;
        let memory_type = memory_type.unwrap_or(MemoryType::Persistent);
        let tags = tags.unwrap_or_default();

        let entry = MemoryEntry::new(
            key.to_string(),
            value.to_string(),
            memory_type.clone(),
            tags,
        );

        {
            let mut cache = self.cache.write().map_err(|_| "Failed to acquire lock")?;
            cache.insert(key.to_string(), entry);
        }
        self.persist()?;

        Ok(format!("Remembered: {} (type: {:?})", key, memory_type))
    }

    /// Simple remember (backward compatible)
    fn remember(&self, key: Option<&str>, value: Option<&str>) -> Result<String, String> {
        self.remember_advanced(key, value, None, None)
    }

    /// Recall with access tracking
    fn recall(&self, key: Option<&str>) -> Result<String, String> {
        let key = key.ok_or("Key required")?;

        let mut cache = self.cache.write().map_err(|_| "Failed to acquire lock")?;

        // Check for expired entries and remove them
        let expired_keys: Vec<String> = cache
            .iter()
            .filter(|(_, entry)| entry.is_expired())
            .map(|(k, _)| k.clone())
            .collect();
        for k in expired_keys {
            cache.remove(&k);
        }

        // Exact match with access tracking
        if let Some(entry) = cache.get_mut(key) {
            if !entry.is_expired() {
                entry.access_count += 1;
                entry.last_accessed = now_ts();
                let value = entry.value.clone();
                let count = entry.access_count;
                drop(cache);
                let _ = self.persist();
                return Ok(format!(
                    "Recalled (exact): {} = {} (accessed: {} times)",
                    key, value, count
                ));
            }
        }

        // Fuzzy search
        let matches: Vec<(String, String, u64)> = cache
            .iter()
            .filter(|(k, _)| k.contains(key))
            .map(|(k, v)| (k.clone(), v.value.clone(), v.access_count))
            .collect();

        if matches.is_empty() {
            Err(format!("Nothing found for '{}'", key))
        } else {
            let result = matches
                .iter()
                .map(|(k, v, count)| format!("{} = {} (accessed: {} times)", k, v, count))
                .collect::<Vec<_>>()
                .join("\n");
            Ok(format!("Recalled (matches):\n{}", result))
        }
    }

    /// Semantic search using scoring
    fn semantic_search(&self, query: Option<&str>, limit: Option<usize>) -> Result<String, String> {
        let query = query.ok_or("Query required")?;
        let limit = limit.unwrap_or(5);

        let cache = self.cache.read().map_err(|_| "Failed to acquire lock")?;

        // Score entries by fuzzy match and access count
        let mut scored: Vec<(String, String, f32)> = cache
            .iter()
            .filter(|(_, entry)| !entry.is_expired())
            .map(|(k, entry)| {
                let mut score = 0.0f32;

                if k.contains(query) {
                    score += 1.0;
                }
                if entry.value.contains(query) {
                    score += 0.5;
                }
                if entry.tags.iter().any(|t| t.contains(query)) {
                    score += 0.8;
                }
                score += (entry.access_count as f32) * 0.01;

                (k.clone(), entry.value.clone(), score)
            })
            .filter(|(_, _, score)| *score > 0.0)
            .collect();

        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());

        if scored.is_empty() {
            return Err(format!("No semantic matches for '{}'", query));
        }

        let results = scored
            .into_iter()
            .take(limit)
            .map(|(k, v, score)| format!("[score: {:.2}] {} = {}", score, k, v))
            .collect::<Vec<_>>()
            .join("\n");

        Ok(format!(
            "Semantic search results for '{}':\n{}",
            query, results
        ))
    }

    /// Query by tags
    fn query_by_tags(&self, tags: Option<Vec<String>>) -> Result<String, String> {
        let tags = tags.ok_or("Tags required")?;
        let cache = self.cache.read().map_err(|_| "Failed to acquire lock")?;

        let matches: Vec<(String, String)> = cache
            .iter()
            .filter(|(_, entry)| tags.iter().all(|tag| entry.tags.contains(tag)))
            .map(|(k, entry)| (k.clone(), entry.value.clone()))
            .collect();

        if matches.is_empty() {
            Err(format!("No entries with tags: {:?}", tags))
        } else {
            let result = matches
                .iter()
                .map(|(k, v)| format!("{} = {}", k, v))
                .collect::<Vec<_>>()
                .join("\n");
            Ok(format!("Tagged entries:\n{}", result))
        }
    }

    /// Forget by key
    fn forget(&self, key: Option<&str>) -> Result<String, String> {
        let key = key.ok_or("Key required")?;

        {
            let mut cache = self.cache.write().map_err(|_| "Failed to acquire lock")?;
            cache.remove(key);
        }
        self.persist()?;

        Ok(format!("Forgotten: {}", key))
    }

    /// List all entries
    fn list(&self) -> Result<String, String> {
        let cache = self.cache.read().map_err(|_| "Failed to acquire lock")?;

        if cache.is_empty() {
            return Ok("No memories stored".to_string());
        }

        let entries: Vec<String> = cache
            .iter()
            .map(|(k, entry)| {
                let tags = if entry.tags.is_empty() {
                    String::new()
                } else {
                    format!(" [tags: {}]", entry.tags.join(", "))
                };
                let vector_status = if entry.vector.is_some() {
                    " [vector]"
                } else {
                    ""
                };
                format!("{} = {}{}{}", k, entry.value, tags, vector_status)
            })
            .collect();

        Ok(format!(
            "Stored memories ({}):\n{}",
            entries.len(),
            entries.join("\n")
        ))
    }

    /// Get memory statistics
    fn stats(&self) -> Result<String, String> {
        let cache = self.cache.read().map_err(|_| "Failed to acquire lock")?;

        let total = cache.len();
        let with_vectors = cache.values().filter(|e| e.vector.is_some()).count();
        let ephemeral = cache
            .values()
            .filter(|e| e.memory_type == MemoryType::Ephemeral)
            .count();
        let persistent = cache
            .values()
            .filter(|e| e.memory_type == MemoryType::Persistent)
            .count();
        let shared = cache
            .values()
            .filter(|e| e.memory_type == MemoryType::Shared)
            .count();
        let expired = cache.values().filter(|e| e.is_expired()).count();

        Ok(format!(
            "Memory Statistics:\nTotal entries: {}\nWith vectors: {}\nEphemeral: {}\nPersistent: {}\nShared: {}\nExpired: {}",
            total, with_vectors, ephemeral, persistent, shared, expired
        ))
    }

    /// Cleanup expired entries
    fn cleanup(&self) -> Result<String, String> {
        let mut cache = self.cache.write().map_err(|_| "Failed to acquire lock")?;

        let before = cache.len();
        let expired_keys: Vec<String> = cache
            .iter()
            .filter(|(_, entry)| entry.is_expired())
            .map(|(k, _)| k.clone())
            .collect();

        for k in expired_keys {
            cache.remove(&k);
        }

        let removed = before - cache.len();
        drop(cache);
        self.persist()?;

        Ok(format!("Cleaned up {} expired entries", removed))
    }
}

#[async_trait]
impl AgentTrait for MemoryAgent {
    fn agent_type(&self) -> &str {
        "memory"
    }
    fn name(&self) -> &str {
        "Memory Agent"
    }
    fn description(&self) -> &str {
        "Cognitive memory with semantic search, tags, and expiration"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "remember".to_string(),
            "remember_advanced".to_string(),
            "recall".to_string(),
            "semantic_search".to_string(),
            "query_by_tags".to_string(),
            "forget".to_string(),
            "list".to_string(),
            "stats".to_string(),
            "cleanup".to_string(),
        ]
    }

    fn security_profile(&self) -> &SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        let result = match task.operation.as_str() {
            "remember" => self.remember(task.path.as_deref(), task.args.as_deref()),
            "remember_advanced" => {
                let memory_type =
                    task.config
                        .get("memory_type")
                        .and_then(|v| v.as_str())
                        .map(|s| match s {
                            "ephemeral" => MemoryType::Ephemeral,
                            "shared" => MemoryType::Shared,
                            _ => MemoryType::Persistent,
                        });
                let tags = task
                    .config
                    .get("tags")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    });
                self.remember_advanced(
                    task.path.as_deref(),
                    task.args.as_deref(),
                    memory_type,
                    tags,
                )
            }
            "recall" => self.recall(task.path.as_deref().or(task.args.as_deref())),
            "semantic_search" => {
                let limit = task
                    .config
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as usize);
                self.semantic_search(task.path.as_deref().or(task.args.as_deref()), limit)
            }
            "query_by_tags" => {
                let tags = task
                    .config
                    .get("tags")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect()
                    });
                self.query_by_tags(tags)
            }
            "forget" => self.forget(task.path.as_deref().or(task.args.as_deref())),
            "list" => self.list(),
            "stats" => self.stats(),
            "cleanup" => self.cleanup(),
            _ => Err(format!("Unknown operation: {}", task.operation)),
        };

        match result {
            Ok(data) => Ok(TaskResult::success(&task.operation, data)),
            Err(e) => Ok(TaskResult::failure(&task.operation, e)),
        }
    }
}
