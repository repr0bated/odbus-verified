//! Unified Agent Registry
//!
//! Single registry for all agent types with lazy loading.

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use super::agent_trait::{AgentCategory, AgentMetadata, UnifiedAgent};
use super::execution::EXECUTION_AGENTS;
use super::orchestration::ORCHESTRATION_AGENTS;
use super::persona::PERSONA_AGENTS;

/// Unified registry for all agents
pub struct UnifiedAgentRegistry {
    /// Loaded agents (lazily instantiated)
    agents: RwLock<HashMap<String, Arc<dyn UnifiedAgent>>>,
    /// Agent factories
    factories: HashMap<&'static str, fn() -> Box<dyn UnifiedAgent>>,
}

impl UnifiedAgentRegistry {
    /// Create a new registry with all default agents
    pub fn new() -> Self {
        let mut factories: HashMap<&'static str, fn() -> Box<dyn UnifiedAgent>> = HashMap::new();

        // Register all execution agents
        for (id, factory) in EXECUTION_AGENTS.iter() {
            factories.insert(*id, *factory);
        }

        // Register all persona agents
        for (id, factory) in PERSONA_AGENTS.iter() {
            factories.insert(*id, *factory);
        }

        // Register all orchestration agents
        for (id, factory) in ORCHESTRATION_AGENTS.iter() {
            factories.insert(*id, *factory);
        }

        // Migrated legacy catalog IDs (overrides only when key already present)
        for (id, factory) in super::legacy_catalog::migrated_factories() {
            factories.entry(id).or_insert(factory);
        }

        Self {
            agents: RwLock::new(HashMap::new()),
            factories,
        }
    }

    /// Resolve an agent id, accepting underscore or hyphen forms.
    pub fn get_normalized(&self, id: &str) -> Option<Arc<dyn UnifiedAgent>> {
        self.get(id).or_else(|| {
            let alt = if id.contains('_') {
                id.replace('_', "-")
            } else {
                id.replace('-', "_")
            };
            self.get(&alt)
        })
    }

    /// Get an agent by ID (lazy loading)
    pub fn get(&self, id: &str) -> Option<Arc<dyn UnifiedAgent>> {
        // Check if already loaded
        {
            let agents = self.agents.read();
            if let Some(agent) = agents.get(id) {
                return Some(Arc::clone(agent));
            }
        }

        // Try to load from factory
        if let Some(factory) = self.factories.get(id) {
            let agent: Arc<dyn UnifiedAgent> = Arc::from(factory());
            let mut agents = self.agents.write();
            agents.insert(id.to_string(), Arc::clone(&agent));
            return Some(agent);
        }

        None
    }

    /// List all available agent IDs
    pub fn list_ids(&self) -> Vec<&str> {
        self.factories.keys().copied().collect()
    }

    /// List agents by category
    pub fn list_by_category(&self, category: AgentCategory) -> Vec<&str> {
        self.factories
            .keys()
            .filter(|id| {
                if let Some(agent) = self.get(id) {
                    agent.category() == category
                } else {
                    false
                }
            })
            .copied()
            .collect()
    }

    /// Get metadata for all agents
    pub fn all_metadata(&self) -> Vec<simd_json::OwnedValue> {
        self.factories
            .keys()
            .filter_map(|id| self.get(id).map(|agent| agent.metadata()))
            .collect()
    }

    /// Register a custom agent
    pub fn register(&mut self, id: &'static str, factory: fn() -> Box<dyn UnifiedAgent>) {
        self.factories.insert(id, factory);
    }

    /// Get count of registered agents
    pub fn count(&self) -> usize {
        self.factories.len()
    }

    /// Get count by category
    pub fn count_by_category(&self) -> HashMap<AgentCategory, usize> {
        let mut counts = HashMap::new();
        counts.insert(AgentCategory::Execution, 0);
        counts.insert(AgentCategory::Persona, 0);
        counts.insert(AgentCategory::Orchestration, 0);

        for id in self.factories.keys() {
            if let Some(agent) = self.get(id) {
                *counts.entry(agent.category()).or_insert(0) += 1;
            }
        }

        counts
    }
}

impl Default for UnifiedAgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global registry instance
pub static GLOBAL_REGISTRY: LazyLock<UnifiedAgentRegistry> =
    LazyLock::new(UnifiedAgentRegistry::new);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = UnifiedAgentRegistry::new();
        assert!(registry.count() > 0);
    }

    #[test]
    fn test_get_agent() {
        let registry = UnifiedAgentRegistry::new();
        let agent = registry.get("python-executor");
        assert!(agent.is_some());
        assert_eq!(agent.unwrap().id(), "python-executor");
    }

    #[test]
    fn test_list_by_category() {
        let registry = UnifiedAgentRegistry::new();
        let executors = registry.list_by_category(AgentCategory::Execution);
        assert!(!executors.is_empty());
        assert!(executors.contains(&"python-executor"));
    }
}
