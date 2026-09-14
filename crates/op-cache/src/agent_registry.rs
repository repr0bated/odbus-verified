//! Agent registry with capability definitions
//!
//! Each agent declares its capabilities as an array.
//! The resolver uses these to build agent sequences.

use anyhow::{Context, Result};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Core capabilities an agent can provide
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentCapability {
    // Analysis capabilities
    CodeAnalysis,
    SecurityAudit,
    PerformanceAnalysis,
    DependencyAnalysis,

    // Generation capabilities
    CodeGeneration,
    TestGeneration,
    DocumentationGeneration,
    RefactoringSuggestion,

    // Transformation capabilities
    CodeTransformation,
    FormatConversion,
    LanguageTranslation,

    // Data capabilities
    DataExtraction,
    DataValidation,
    DataEnrichment,
    Embedding,

    // Reasoning capabilities
    Planning,
    Summarization,
    QuestionAnswering,
    Classification,

    // Integration capabilities
    ApiCall,
    DatabaseQuery,
    FileOperation,
    ShellExecution,

    // Custom capability (for extensibility)
    Custom(u32),
}

impl AgentCapability {
    /// Parse capability from string
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "code_analysis" | "analyze_code" => Some(Self::CodeAnalysis),
            "security_audit" | "security" => Some(Self::SecurityAudit),
            "performance_analysis" | "performance" => Some(Self::PerformanceAnalysis),
            "dependency_analysis" | "dependencies" => Some(Self::DependencyAnalysis),
            "code_generation" | "generate_code" => Some(Self::CodeGeneration),
            "test_generation" | "generate_tests" | "tests" => Some(Self::TestGeneration),
            "documentation_generation" | "docs" | "documentation" => {
                Some(Self::DocumentationGeneration)
            }
            "refactoring" | "refactor" => Some(Self::RefactoringSuggestion),
            "code_transformation" | "transform" => Some(Self::CodeTransformation),
            "format_conversion" | "convert" => Some(Self::FormatConversion),
            "language_translation" | "translate" => Some(Self::LanguageTranslation),
            "data_extraction" | "extract" => Some(Self::DataExtraction),
            "data_validation" | "validate" => Some(Self::DataValidation),
            "data_enrichment" | "enrich" => Some(Self::DataEnrichment),
            "embedding" | "embed" => Some(Self::Embedding),
            "planning" | "plan" => Some(Self::Planning),
            "summarization" | "summarize" | "summary" => Some(Self::Summarization),
            "question_answering" | "qa" | "answer" => Some(Self::QuestionAnswering),
            "classification" | "classify" => Some(Self::Classification),
            "api_call" | "api" => Some(Self::ApiCall),
            "database_query" | "db" | "query" => Some(Self::DatabaseQuery),
            "file_operation" | "file" => Some(Self::FileOperation),
            "shell_execution" | "shell" | "exec" => Some(Self::ShellExecution),
            _ => None,
        }
    }

    /// Get capability name
    pub fn name(&self) -> &'static str {
        match self {
            Self::CodeAnalysis => "code_analysis",
            Self::SecurityAudit => "security_audit",
            Self::PerformanceAnalysis => "performance_analysis",
            Self::DependencyAnalysis => "dependency_analysis",
            Self::CodeGeneration => "code_generation",
            Self::TestGeneration => "test_generation",
            Self::DocumentationGeneration => "documentation_generation",
            Self::RefactoringSuggestion => "refactoring",
            Self::CodeTransformation => "code_transformation",
            Self::FormatConversion => "format_conversion",
            Self::LanguageTranslation => "language_translation",
            Self::DataExtraction => "data_extraction",
            Self::DataValidation => "data_validation",
            Self::DataEnrichment => "data_enrichment",
            Self::Embedding => "embedding",
            Self::Planning => "planning",
            Self::Summarization => "summarization",
            Self::QuestionAnswering => "question_answering",
            Self::Classification => "classification",
            Self::ApiCall => "api_call",
            Self::DatabaseQuery => "database_query",
            Self::FileOperation => "file_operation",
            Self::ShellExecution => "shell_execution",
            Self::Custom(_id) => {
                // Return static str for known custom IDs, or generic
                "custom"
            }
        }
    }
}

/// Agent execution priority
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default,
)]
pub enum AgentPriority {
    /// Execute first (e.g., validation, security)
    High = 0,
    /// Normal execution order
    #[default]
    Normal = 1,
    /// Execute last (e.g., formatting, cleanup)
    Low = 2,
}

/// Agent definition with capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Unique agent identifier
    pub id: String,

    /// Human-readable name
    pub name: String,

    /// Description of what the agent does
    pub description: String,

    /// Capabilities this agent provides (array)
    pub capabilities: Vec<AgentCapability>,

    /// Capabilities this agent requires as input
    pub requires: Vec<AgentCapability>,

    /// Execution priority
    pub priority: AgentPriority,

    /// Whether agent can run in parallel with others
    pub parallelizable: bool,

    /// Estimated latency in milliseconds
    pub estimated_latency_ms: u64,

    /// Maximum input size in bytes (0 = unlimited)
    pub max_input_size: usize,

    /// Agent version
    pub version: String,

    /// Whether agent is enabled
    pub enabled: bool,
}

impl AgentDefinition {
    /// Create new agent definition
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            description: String::new(),
            capabilities: Vec::new(),
            requires: Vec::new(),
            priority: AgentPriority::Normal,
            parallelizable: false,
            estimated_latency_ms: 100,
            max_input_size: 0,
            version: "1.0.0".to_string(),
            enabled: true,
        }
    }

    /// Builder: add description
    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    /// Builder: add capability
    pub fn with_capability(mut self, cap: AgentCapability) -> Self {
        if !self.capabilities.contains(&cap) {
            self.capabilities.push(cap);
        }
        self
    }

    /// Builder: add multiple capabilities
    pub fn with_capabilities(mut self, caps: &[AgentCapability]) -> Self {
        for cap in caps {
            if !self.capabilities.contains(cap) {
                self.capabilities.push(*cap);
            }
        }
        self
    }

    /// Builder: add requirement
    pub fn requires_capability(mut self, cap: AgentCapability) -> Self {
        if !self.requires.contains(&cap) {
            self.requires.push(cap);
        }
        self
    }

    /// Builder: set priority
    pub fn with_priority(mut self, priority: AgentPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Builder: set parallelizable
    pub fn parallelizable(mut self, parallel: bool) -> Self {
        self.parallelizable = parallel;
        self
    }

    /// Builder: set estimated latency
    pub fn with_latency(mut self, ms: u64) -> Self {
        self.estimated_latency_ms = ms;
        self
    }

    /// Check if agent provides a capability
    pub fn provides(&self, cap: AgentCapability) -> bool {
        self.capabilities.contains(&cap)
    }

    /// Check if agent requires a capability
    pub fn needs(&self, cap: AgentCapability) -> bool {
        self.requires.contains(&cap)
    }

    /// Get all provided capabilities as set
    pub fn capability_set(&self) -> HashSet<AgentCapability> {
        self.capabilities.iter().copied().collect()
    }
}

/// Agent executor function type
pub type AgentExecutor = Arc<dyn Fn(&[u8]) -> BoxFuture<'static, Result<Vec<u8>>> + Send + Sync>;

/// Registered agent with executor
pub struct RegisteredAgent {
    pub definition: AgentDefinition,
    pub executor: AgentExecutor,
}

/// Agent registry - stores all agents and their capabilities
pub struct AgentRegistry {
    agents: RwLock<HashMap<String, RegisteredAgent>>,
    capability_index: RwLock<HashMap<AgentCapability, Vec<String>>>,
}

impl AgentRegistry {
    /// Create new empty registry
    pub fn new() -> Self {
        Self {
            agents: RwLock::new(HashMap::new()),
            capability_index: RwLock::new(HashMap::new()),
        }
    }

    /// Register an agent with its executor
    pub async fn register(
        &self,
        definition: AgentDefinition,
        executor: AgentExecutor,
    ) -> Result<()> {
        let agent_id = definition.id.clone();
        let capabilities = definition.capabilities.clone();

        // Store agent
        {
            let mut agents = self.agents.write().await;
            agents.insert(
                agent_id.clone(),
                RegisteredAgent {
                    definition,
                    executor,
                },
            );
        }

        // Update capability index
        {
            let mut index = self.capability_index.write().await;
            for cap in capabilities {
                index
                    .entry(cap)
                    .or_insert_with(Vec::new)
                    .push(agent_id.clone());
            }
        }

        info!("Registered agent: {}", agent_id);
        Ok(())
    }

    /// Unregister an agent
    pub async fn unregister(&self, agent_id: &str) -> Result<Option<AgentDefinition>> {
        let removed = {
            let mut agents = self.agents.write().await;
            agents.remove(agent_id)
        };

        if let Some(agent) = &removed {
            // Remove from capability index
            let mut index = self.capability_index.write().await;
            for cap in &agent.definition.capabilities {
                if let Some(agents) = index.get_mut(cap) {
                    agents.retain(|id| id != agent_id);
                }
            }
            info!("Unregistered agent: {}", agent_id);
            Ok(Some(agent.definition.clone()))
        } else {
            Ok(None)
        }
    }

    /// Get agent definition by ID
    pub async fn get(&self, agent_id: &str) -> Option<AgentDefinition> {
        let agents = self.agents.read().await;
        agents.get(agent_id).map(|a| a.definition.clone())
    }

    /// Get agent executor by ID
    pub async fn get_executor(&self, agent_id: &str) -> Option<AgentExecutor> {
        let agents = self.agents.read().await;
        agents.get(agent_id).map(|a| a.executor.clone())
    }

    /// Find agents that provide a capability
    pub async fn find_by_capability(&self, cap: AgentCapability) -> Vec<AgentDefinition> {
        let index = self.capability_index.read().await;
        let agents = self.agents.read().await;

        index
            .get(&cap)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| agents.get(id).map(|a| a.definition.clone()))
                    .filter(|def| def.enabled)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Find agents that provide any of the given capabilities
    pub async fn find_by_capabilities(&self, caps: &[AgentCapability]) -> Vec<AgentDefinition> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();

        for cap in caps {
            for agent in self.find_by_capability(*cap).await {
                if !seen.contains(&agent.id) {
                    seen.insert(agent.id.clone());
                    result.push(agent);
                }
            }
        }

        result
    }

    /// Find the best agent for a capability (lowest latency, enabled)
    pub async fn find_best_for_capability(&self, cap: AgentCapability) -> Option<AgentDefinition> {
        self.find_by_capability(cap)
            .await
            .into_iter()
            .min_by_key(|a| a.estimated_latency_ms)
    }

    /// Get all registered agents
    pub async fn list_all(&self) -> Vec<AgentDefinition> {
        let agents = self.agents.read().await;
        agents.values().map(|a| a.definition.clone()).collect()
    }

    /// Get all capabilities provided by registered agents
    pub async fn list_capabilities(&self) -> Vec<AgentCapability> {
        let index = self.capability_index.read().await;
        index.keys().copied().collect()
    }

    /// Check if a capability is available
    pub async fn has_capability(&self, cap: AgentCapability) -> bool {
        let index = self.capability_index.read().await;
        index.get(&cap).map(|v| !v.is_empty()).unwrap_or(false)
    }

    /// Get registry statistics
    pub async fn stats(&self) -> RegistryStats {
        let agents = self.agents.read().await;
        let index = self.capability_index.read().await;

        let enabled_count = agents.values().filter(|a| a.definition.enabled).count();

        RegistryStats {
            total_agents: agents.len(),
            enabled_agents: enabled_count,
            disabled_agents: agents.len() - enabled_count,
            total_capabilities: index.len(),
        }
    }

    /// Execute an agent by ID
    pub async fn execute(&self, agent_id: &str, input: &[u8]) -> Result<Vec<u8>> {
        let executor = {
            let agents = self.agents.read().await;
            agents
                .get(agent_id)
                .map(|a| a.executor.clone())
                .context(format!("Agent not found: {}", agent_id))?
        };

        executor(input).await
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry statistics
#[derive(Debug, Clone)]
pub struct RegistryStats {
    pub total_agents: usize,
    pub enabled_agents: usize,
    pub disabled_agents: usize,
    pub total_capabilities: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_executor() -> AgentExecutor {
        Arc::new(|input: &[u8]| {
            let input = input.to_vec();
            Box::pin(async move { Ok(input) })
        })
    }

    #[tokio::test]
    async fn test_agent_registration() {
        let registry = AgentRegistry::new();

        let agent = AgentDefinition::new("test_agent", "Test Agent")
            .with_capability(AgentCapability::CodeAnalysis)
            .with_capability(AgentCapability::TestGeneration);

        registry
            .register(agent, make_test_executor())
            .await
            .unwrap();

        let retrieved = registry.get("test_agent").await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().capabilities.len(), 2);
    }

    #[tokio::test]
    async fn test_find_by_capability() {
        let registry = AgentRegistry::new();

        let agent1 = AgentDefinition::new("analyzer", "Code Analyzer")
            .with_capability(AgentCapability::CodeAnalysis);

        let agent2 = AgentDefinition::new("tester", "Test Generator")
            .with_capability(AgentCapability::TestGeneration)
            .with_capability(AgentCapability::CodeAnalysis);

        registry
            .register(agent1, make_test_executor())
            .await
            .unwrap();
        registry
            .register(agent2, make_test_executor())
            .await
            .unwrap();

        let analyzers = registry
            .find_by_capability(AgentCapability::CodeAnalysis)
            .await;
        assert_eq!(analyzers.len(), 2);

        let testers = registry
            .find_by_capability(AgentCapability::TestGeneration)
            .await;
        assert_eq!(testers.len(), 1);
    }

    #[tokio::test]
    async fn test_agent_builder() {
        let agent = AgentDefinition::new("builder_test", "Builder Test")
            .with_description("A test agent")
            .with_capabilities(&[
                AgentCapability::CodeAnalysis,
                AgentCapability::SecurityAudit,
            ])
            .requires_capability(AgentCapability::DataExtraction)
            .with_priority(AgentPriority::High)
            .parallelizable(true)
            .with_latency(50);

        assert_eq!(agent.capabilities.len(), 2);
        assert_eq!(agent.requires.len(), 1);
        assert_eq!(agent.priority, AgentPriority::High);
        assert!(agent.parallelizable);
        assert_eq!(agent.estimated_latency_ms, 50);
    }

    #[tokio::test]
    async fn test_capability_parsing() {
        assert_eq!(
            AgentCapability::parse("code_analysis"),
            Some(AgentCapability::CodeAnalysis)
        );
        assert_eq!(
            AgentCapability::parse("tests"),
            Some(AgentCapability::TestGeneration)
        );
        assert_eq!(AgentCapability::parse("unknown_capability"), None);
    }
}
