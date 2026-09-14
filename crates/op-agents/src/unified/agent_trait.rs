//! Unified Agent Trait
//!
//! Single trait that all agents implement, with clear capability declarations.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

use crate::security::SecurityProfile;

/// Agent category - determines what the agent can do
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentCategory {
    /// Can execute code/commands with sandboxing
    Execution,
    /// LLM-only, provides expertise without code execution
    Persona,
    /// Coordinates other agents for complex workflows
    Orchestration,
}

/// Specific capabilities an agent has
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentCapability {
    // Execution capabilities
    RunCode { language: String },
    RunCommand { commands: Vec<String> },
    ReadFiles,
    WriteFiles,
    NetworkAccess,

    // Persona capabilities (LLM augmentation)
    CodeReview,
    ArchitectureDesign,
    SecurityAudit,
    Documentation,
    Debugging,
    Optimization,

    // Orchestration capabilities
    DelegateToAgents { agents: Vec<String> },
    ParallelExecution,
    WorkflowManagement,
}

/// Task request to an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    /// Operation to perform
    pub operation: String,
    /// Arguments for the operation
    pub args: Value,
    /// Context from conversation/session
    pub context: Option<String>,
    /// Files to include
    pub files: Vec<FileContext>,
}

/// File context for agent operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileContext {
    pub path: String,
    pub content: String,
}

/// Response from an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    /// Whether the operation succeeded
    pub success: bool,
    /// Result data
    pub data: Value,
    /// Human-readable message
    pub message: String,
    /// Files modified/created
    pub files_changed: Vec<String>,
    /// Suggested follow-up actions
    pub suggestions: Vec<String>,
}

impl AgentResponse {
    pub fn success(data: Value, message: impl Into<String>) -> Self {
        Self {
            success: true,
            data,
            message: message.into(),
            files_changed: vec![],
            suggestions: vec![],
        }
    }

    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: simd_json::json!(null),
            message: message.into(),
            files_changed: vec![],
            suggestions: vec![],
        }
    }

    pub fn with_files(mut self, files: Vec<String>) -> Self {
        self.files_changed = files;
        self
    }

    pub fn with_suggestions(mut self, suggestions: Vec<String>) -> Self {
        self.suggestions = suggestions;
        self
    }
}

/// Unified Agent Trait
///
/// All agents implement this trait, regardless of category.
#[async_trait]
pub trait UnifiedAgent: Send + Sync {
    // =========================================================================
    // IDENTITY
    // =========================================================================

    /// Unique agent identifier (e.g., "python-executor", "django-expert")
    fn id(&self) -> &str;

    /// Human-readable name
    fn name(&self) -> &str;

    /// Description of what this agent does
    fn description(&self) -> &str;

    /// Agent category
    fn category(&self) -> AgentCategory;

    /// Agent capabilities
    fn capabilities(&self) -> Vec<AgentCapability>;

    // =========================================================================
    // PROMPTS (embedded, not separate markdown files)
    // =========================================================================

    /// System prompt for LLM interactions
    /// This is the "persona" that was previously in markdown files
    fn system_prompt(&self) -> &str;

    /// Additional context/knowledge to inject
    fn knowledge_base(&self) -> Option<&str> {
        None
    }

    /// Example interactions for few-shot learning
    fn examples(&self) -> Vec<(&str, &str)> {
        vec![]
    }

    // =========================================================================
    // SECURITY (for execution agents)
    // =========================================================================

    /// Security profile (only meaningful for execution agents)
    fn security_profile(&self) -> Option<&SecurityProfile> {
        None
    }

    /// Whether this agent requires root/elevated privileges
    fn requires_root(&self) -> bool {
        false
    }

    // =========================================================================
    // OPERATIONS
    // =========================================================================

    /// List of operations this agent can perform
    fn operations(&self) -> Vec<&str>;

    /// Execute an operation
    async fn execute(&self, request: AgentRequest) -> AgentResponse;

    /// Check if agent can handle a specific operation
    fn can_handle(&self, operation: &str) -> bool {
        self.operations().contains(&operation)
    }

    // =========================================================================
    // LIFECYCLE
    // =========================================================================

    /// Initialize the agent (called once on startup)
    async fn initialize(&self) -> Result<(), String> {
        Ok(())
    }

    /// Shutdown the agent (called on cleanup)
    async fn shutdown(&self) -> Result<(), String> {
        Ok(())
    }

    /// Health check
    fn is_healthy(&self) -> bool {
        true
    }
}

/// Extension trait for agent metadata
pub trait AgentMetadata: UnifiedAgent {
    /// Get full metadata as JSON
    fn metadata(&self) -> Value {
        simd_json::json!({
            "id": self.id(),
            "name": self.name(),
            "description": self.description(),
            "category": self.category(),
            "capabilities": self.capabilities().iter().collect::<Vec<_>>(),
            "operations": self.operations(),
            "requires_root": self.requires_root(),
            "has_security_profile": self.security_profile().is_some(),
        })
    }
}

impl<T: ?Sized + UnifiedAgent> AgentMetadata for T {}
