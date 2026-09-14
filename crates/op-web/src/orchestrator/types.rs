use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

/// Maximum number of conversation turns before forcing completion
pub const MAX_TURNS: usize = 50;

/// Configuration for the orchestrator
#[derive(Clone, Debug)]
pub struct OrchestratorConfig {
    pub default_model: String,
    pub default_provider: String,
    pub max_turns: usize,
    pub system_prompt: Option<String>,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            default_model: "gemini-2.0-flash".to_string(),
            default_provider: "gemini".to_string(),
            max_turns: MAX_TURNS,
            system_prompt: None,
        }
    }
}

/// Events emitted during orchestration for real-time streaming
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorEvent {
    Thinking,
    ToolExecution {
        name: String,
        args: Value,
    },
    ToolResult {
        name: String,
        success: bool,
        result: Option<Value>,
        error: Option<String>,
    },
    Finished {
        success: bool,
        message: String,
        tools_executed: Vec<String>,
    },
    Error {
        message: String,
    },
}

/// Response from tool execution
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub name: String,
    pub success: bool,
    pub result: Option<Value>,
    pub error: Option<String>,
}

/// Final response from orchestration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrchestratorResponse {
    pub success: bool,
    pub message: String,
    pub tools_executed: Vec<String>,
    pub tool_results: Vec<ToolResult>,
    pub turns: usize,
}

impl OrchestratorResponse {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: message.into(),
            tools_executed: vec![],
            tool_results: vec![],
            turns: 0,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            tools_executed: vec![],
            tool_results: vec![],
            turns: 0,
        }
    }
}
