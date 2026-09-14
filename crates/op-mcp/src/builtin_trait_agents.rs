//! Built-in Trait Agent Implementations
//!
//! These provide the fallback implementations when D-Bus services aren't available.
//! They use op-agents crate implementations internally.

use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

use super::agents_server::AgentTraitImpl;

// =============================================================================
// MEMORY AGENT
// =============================================================================

/// In-memory implementation of the memory agent
pub struct MemoryAgentImpl {
    memories: RwLock<HashMap<String, MemoryEntry>>,
}

#[derive(Clone)]
struct MemoryEntry {
    value: String,
    tags: Vec<String>,
    created_at: chrono::DateTime<chrono::Utc>,
}

impl MemoryAgentImpl {
    pub fn new() -> Self {
        Self {
            memories: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryAgentImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentTraitImpl for MemoryAgentImpl {
    fn agent_id(&self) -> &str {
        "memory"
    }
    
    async fn execute(&self, operation: &str, args: Value) -> Result<Value> {
        match operation {
            "store" => {
                let key = args["key"].as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'key' parameter"))?;
                let value = args["value"].as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'value' parameter"))?;
                let tags: Vec<String> = args["tags"].as_array()
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default();
                
                let mut memories = self.memories.write().await;
                memories.insert(key.to_string(), MemoryEntry {
                    value: value.to_string(),
                    tags,
                    created_at: chrono::Utc::now(),
                });
                
                debug!("Memory stored: {}", key);
                Ok(json!({ "success": true, "key": key }))
            }
            
            "recall" => {
                let memories = self.memories.read().await;
                
                if let Some(key) = args["key"].as_str() {
                    if let Some(entry) = memories.get(key) {
                        return Ok(json!({
                            "found": true,
                            "key": key,
                            "value": entry.value,
                            "tags": entry.tags,
                        }));
                    } else {
                        return Ok(json!({ "found": false, "key": key }));
                    }
                }
                
                if let Some(query) = args["query"].as_str() {
                    let query_lower = query.to_lowercase();
                    let matches: Vec<_> = memories.iter()
                        .filter(|(k, v)| {
                            k.to_lowercase().contains(&query_lower) ||
                            v.value.to_lowercase().contains(&query_lower) ||
                            v.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
                        })
                        .map(|(k, v)| json!({
                            "key": k,
                            "value": v.value,
                            "tags": v.tags,
                        }))
                        .collect();
                    
                    return Ok(json!({
                        "found": !matches.is_empty(),
                        "query": query,
                        "matches": matches,
                    }));
                }
                
                Err(anyhow::anyhow!("Either 'key' or 'query' parameter required"))
            }
            
            "list" => {
                let memories = self.memories.read().await;
                let limit = args["limit"].as_u64().unwrap_or(100) as usize;
                let filter_tags: Option<Vec<String>> = args["tags"].as_array()
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect());
                
                let mut entries: Vec<_> = memories.iter()
                    .filter(|(_, v)| {
                        if let Some(ref tags) = filter_tags {
                            tags.iter().any(|t| v.tags.contains(t))
                        } else {
                            true
                        }
                    })
                    .take(limit)
                    .map(|(k, v)| json!({
                        "key": k,
                        "value": v.value,
                        "tags": v.tags,
                    }))
                    .collect();
                
                Ok(json!({
                    "count": entries.len(),
                    "memories": entries,
                }))
            }
            
            _ => Err(anyhow::anyhow!("Unknown operation: {}", operation)),
        }
    }
}

// =============================================================================
// SEQUENTIAL THINKING AGENT
// =============================================================================

/// Sequential thinking agent implementation
pub struct SequentialThinkingAgentImpl {
    thoughts: RwLock<Vec<ThoughtStep>>,
}

#[derive(Clone)]
struct ThoughtStep {
    step: usize,
    thought: String,
    timestamp: chrono::DateTime<chrono::Utc>,
}

impl SequentialThinkingAgentImpl {
    pub fn new() -> Self {
        Self {
            thoughts: RwLock::new(Vec::new()),
        }
    }
}

impl Default for SequentialThinkingAgentImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentTraitImpl for SequentialThinkingAgentImpl {
    fn agent_id(&self) -> &str {
        "sequential_thinking"
    }
    
    async fn execute(&self, operation: &str, args: Value) -> Result<Value> {
        match operation {
            "think" => {
                let thought = args["thought"].as_str()
                    .ok_or_else(|| anyhow::anyhow!("Missing 'thought' parameter"))?;
                
                let mut thoughts = self.thoughts.write().await;
                let step = args["step"].as_u64().map(|s| s as usize)
                    .unwrap_or(thoughts.len() + 1);
                
                thoughts.push(ThoughtStep {
                    step,
                    thought: thought.to_string(),
                    timestamp: chrono::Utc::now(),
                });
                
                debug!("Thought step {} recorded", step);
                
                Ok(json!({
                    "success": true,
                    "step": step,
                    "total_thoughts": thoughts.len(),
                }))
            }
            
            "summarize" => {
                let thoughts = self.thoughts.read().await;
                let steps: Vec<_> = thoughts.iter()
                    .map(|t| json!({
                        "step": t.step,
                        "thought": t.thought,
                    }))
                    .collect();
                
                Ok(json!({
                    "total_steps": steps.len(),
                    "thoughts": steps,
                }))
            }
            
            "clear" => {
                let mut thoughts = self.thoughts.write().await;
                let count = thoughts.len();
                thoughts.clear();
                
                Ok(json!({
                    "success": true,
                    "cleared": count,
                }))
            }
            
            _ => Err(anyhow::anyhow!("Unknown operation: {}", operation)),
        }
    }
}

// =============================================================================
// REGISTRATION HELPER
// =============================================================================

/// Register all built-in trait agents with the server
pub async fn register_builtin_agents(server: &super::agents_server::AgentsServer) {
    tracing::info!("Registering built-in trait agent implementations");
    
    // Memory agent
    server.register_trait_agent(Box::new(MemoryAgentImpl::new())).await;
    
    // Sequential thinking
    server.register_trait_agent(Box::new(SequentialThinkingAgentImpl::new())).await;
    
    // TODO: Add more built-in agents as needed
    // server.register_trait_agent(Box::new(RustProAgentImpl::new())).await;
    // server.register_trait_agent(Box::new(PythonProAgentImpl::new())).await;
    
    tracing::info!("Built-in trait agents registered");
}
