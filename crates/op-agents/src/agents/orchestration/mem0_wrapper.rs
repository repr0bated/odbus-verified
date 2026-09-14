//! Mem0 Wrapper Agent - Temporarily Disabled
//!
//! This agent wraps the Mem0 Python library for semantic memory.
//! Currently disabled pending embedder configuration (needs a supported embeddings backend).
//!
//! To re-enable:
//! 1. Set up HuggingFace embeddings with proper cache paths, OR
//! 2. Provide OPENAI_API_KEY for OpenAI embeddings

use async_trait::async_trait;
use simd_json::json;
use std::collections::HashMap;
use std::sync::Mutex;
use tracing::warn;

use crate::agents::base::{AgentTask, AgentTrait, TaskResult};

/// Mem0 wrapper state
struct Mem0State {
    initialized: bool,
    available: bool,
    last_error: Option<String>,
}

impl Default for Mem0State {
    fn default() -> Self {
        Self {
            initialized: false,
            available: false,
            last_error: Some(
                "Mem0 temporarily disabled - pending embedder configuration".to_string(),
            ),
        }
    }
}

/// Mem0 Wrapper Agent
pub struct Mem0WrapperAgent {
    id: String,
    state: Mutex<Mem0State>,
    python_path: String,
    mem0_dir: String,
    profile: crate::security::SecurityProfile,
}

impl Mem0WrapperAgent {
    pub fn new(id: String) -> Self {
        let python_path =
            std::env::var("PYTHON_PATH").unwrap_or_else(|_| "/usr/bin/python3".to_string());
        let mem0_dir =
            std::env::var("MEM0_DIR").unwrap_or_else(|_| "/var/lib/op-dbus/.mem0".to_string());

        Self {
            id,
            state: Mutex::new(Mem0State::default()),
            python_path,
            mem0_dir,
            profile: crate::security::SecurityProfile::orchestration("mem0", vec!["*"]),
        }
    }
}

#[async_trait]
impl AgentTrait for Mem0WrapperAgent {
    fn agent_type(&self) -> &str {
        "mem0"
    }

    fn name(&self) -> &str {
        "Mem0 Memory Agent"
    }

    fn description(&self) -> &str {
        "Semantic memory using Mem0 (temporarily disabled)"
    }

    fn operations(&self) -> Vec<String> {
        vec![
            "add".to_string(),
            "search".to_string(),
            "get_all".to_string(),
            "delete".to_string(),
            "update".to_string(),
        ]
    }

    fn security_profile(&self) -> &crate::security::SecurityProfile {
        &self.profile
    }

    async fn execute(&self, task: AgentTask) -> Result<TaskResult, String> {
        // Return graceful "not available" response
        let error_msg = "Mem0 temporarily disabled - pending embedder configuration. \
                         To enable: configure HuggingFace embeddings or provide OPENAI_API_KEY";

        warn!("Mem0 agent called but disabled: {}", task.operation);

        Ok(TaskResult {
            success: false,
            operation: task.operation,
            data: json!({
                "available": false,
                "error": error_msg,
                "hint": "Use memory_remember/memory_recall for key-value memory instead"
            })
            .to_string(),
            metadata: {
                let mut m = HashMap::new();
                m.insert("status".to_string(), json!("disabled"));
                m
            },
        })
    }
}
