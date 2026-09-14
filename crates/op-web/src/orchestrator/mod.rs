use op_llm::chat::ChatManager;
use op_tools::registry::ToolRegistry;
use std::sync::Arc;

// Export types publicly
pub mod types;
pub use types::*;

// Internal modules (implementation split)
mod execution;
mod formatting;
mod parsing;
mod process;
mod tools;

/// The main orchestrator that coordinates LLM calls and tool execution.
///
/// This struct is the central point of the `orchestrator` module.
/// Implementation details are split across multiple submodules:
/// - `tools.rs`: Tool definition and prompt generation
/// - `parsing.rs`: Parsing tool calls from LLM output
/// - `formatting.rs`: Formatting results for context
/// - `execution.rs`: Executing tools and handling meta-commands
/// - `process.rs`: The main execution loop (`process` and `process_with_llm`)
pub struct UnifiedOrchestrator {
    pub chat_manager: Arc<ChatManager>,
    pub tool_registry: Arc<ToolRegistry>,
    pub config: OrchestratorConfig,
}

impl UnifiedOrchestrator {
    pub fn new(tool_registry: Arc<ToolRegistry>, chat_manager: Arc<ChatManager>) -> Self {
        Self {
            tool_registry,
            chat_manager,
            config: OrchestratorConfig::default(),
        }
    }
}
