//! Capability resolver - maps requests to agent sequences
//!
//! Takes a request with required capabilities and resolves it
//! to an ordered sequence of agents.

use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, info, warn};

use super::agent_registry::{AgentCapability, AgentDefinition, AgentRegistry};

/// Request that needs capability resolution
#[derive(Debug, Clone)]
pub struct CapabilityRequest {
    /// Explicitly requested capabilities
    pub required_capabilities: Vec<AgentCapability>,

    /// Preferred agents (use these if they provide the capability)
    pub preferred_agents: Vec<String>,

    /// Agents to exclude
    pub excluded_agents: Vec<String>,

    /// Allow parallel execution where possible
    pub allow_parallel: bool,

    /// Maximum agents in sequence
    pub max_agents: usize,

    /// Input data for the request
    pub input: Vec<u8>,
}

impl CapabilityRequest {
    /// Create request with required capabilities
    pub fn new(capabilities: Vec<AgentCapability>, input: Vec<u8>) -> Self {
        Self {
            required_capabilities: capabilities,
            preferred_agents: Vec::new(),
            excluded_agents: Vec::new(),
            allow_parallel: false,
            max_agents: 10,
            input,
        }
    }

    /// Create from capability strings
    pub fn from_strings(cap_strings: &[&str], input: Vec<u8>) -> Self {
        let capabilities: Vec<AgentCapability> = cap_strings
            .iter()
            .filter_map(|s| AgentCapability::parse(s))
            .collect();

        Self::new(capabilities, input)
    }

    /// Builder: prefer specific agents
    pub fn prefer_agents(mut self, agents: &[&str]) -> Self {
        self.preferred_agents = agents.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Builder: exclude specific agents
    pub fn exclude_agents(mut self, agents: &[&str]) -> Self {
        self.excluded_agents = agents.iter().map(|s| s.to_string()).collect();
        self
    }

    /// Builder: allow parallel execution
    pub fn allow_parallel(mut self, allow: bool) -> Self {
        self.allow_parallel = allow;
        self
    }
}

/// Resolved agent sequence
#[derive(Debug, Clone)]
pub struct ResolvedSequence {
    /// Ordered list of agents to execute
    pub agents: Vec<AgentDefinition>,

    /// Capabilities fulfilled by this sequence
    pub fulfilled_capabilities: HashSet<AgentCapability>,

    /// Capabilities that couldn't be fulfilled
    pub missing_capabilities: HashSet<AgentCapability>,

    /// Estimated total latency
    pub estimated_latency_ms: u64,

    /// Groups of agents that can run in parallel
    pub parallel_groups: Vec<Vec<String>>,

    /// Resolution metadata
    pub resolution_path: Vec<String>,
}

impl ResolvedSequence {
    /// Get agent IDs in order
    pub fn agent_ids(&self) -> Vec<String> {
        self.agents.iter().map(|a| a.id.clone()).collect()
    }

    /// Check if all capabilities were fulfilled
    pub fn is_complete(&self) -> bool {
        self.missing_capabilities.is_empty()
    }

    /// Get number of agents
    pub fn len(&self) -> usize {
        self.agents.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }
}

/// Capability resolver
pub struct CapabilityResolver {
    registry: Arc<AgentRegistry>,
}

impl CapabilityResolver {
    /// Create new resolver with registry
    pub fn new(registry: Arc<AgentRegistry>) -> Self {
        Self { registry }
    }

    /// Resolve a capability request to an agent sequence
    pub async fn resolve(&self, request: &CapabilityRequest) -> Result<ResolvedSequence> {
        if request.required_capabilities.is_empty() {
            return Ok(ResolvedSequence {
                agents: Vec::new(),
                fulfilled_capabilities: HashSet::new(),
                missing_capabilities: HashSet::new(),
                estimated_latency_ms: 0,
                parallel_groups: Vec::new(),
                resolution_path: vec!["empty_request".to_string()],
            });
        }

        debug!(
            "Resolving {} capabilities",
            request.required_capabilities.len()
        );

        let mut selected_agents: Vec<AgentDefinition> = Vec::new();
        let mut fulfilled: HashSet<AgentCapability> = HashSet::new();
        let mut resolution_path: Vec<String> = Vec::new();

        // Build candidate pool
        let candidates = self.build_candidate_pool(request).await?;
        resolution_path.push(format!("candidates:{}", candidates.len()));

        // Greedy selection: for each required capability, pick best agent
        let required: HashSet<AgentCapability> =
            request.required_capabilities.iter().copied().collect();

        for cap in &request.required_capabilities {
            // Skip if already fulfilled
            if fulfilled.contains(cap) {
                continue;
            }

            // Find best candidate for this capability
            if let Some(agent) =
                self.select_best_agent(&candidates, *cap, &selected_agents, request)
            {
                resolution_path.push(format!("select:{}->{}", cap.name(), agent.id));

                // Add all capabilities this agent provides
                for provided_cap in &agent.capabilities {
                    fulfilled.insert(*provided_cap);
                }

                selected_agents.push(agent);

                if selected_agents.len() >= request.max_agents {
                    resolution_path.push("max_agents_reached".to_string());
                    break;
                }
            } else {
                resolution_path.push(format!("no_agent_for:{}", cap.name()));
            }
        }

        // Sort agents by priority and dependencies
        self.sort_agents(&mut selected_agents);

        // Calculate missing capabilities
        let missing: HashSet<AgentCapability> = required.difference(&fulfilled).copied().collect();

        // Calculate total latency
        let estimated_latency_ms: u64 =
            selected_agents.iter().map(|a| a.estimated_latency_ms).sum();

        // Build parallel groups if allowed
        let parallel_groups = if request.allow_parallel {
            self.build_parallel_groups(&selected_agents)
        } else {
            Vec::new()
        };

        let sequence = ResolvedSequence {
            agents: selected_agents,
            fulfilled_capabilities: fulfilled,
            missing_capabilities: missing,
            estimated_latency_ms,
            parallel_groups,
            resolution_path,
        };

        if !sequence.missing_capabilities.is_empty() {
            warn!(
                "Could not fulfill capabilities: {:?}",
                sequence.missing_capabilities
            );
        }

        info!(
            "Resolved to {} agents: {:?}",
            sequence.agents.len(),
            sequence.agent_ids()
        );

        Ok(sequence)
    }

    /// Build pool of candidate agents
    async fn build_candidate_pool(
        &self,
        request: &CapabilityRequest,
    ) -> Result<Vec<AgentDefinition>> {
        let candidates = self
            .registry
            .find_by_capabilities(&request.required_capabilities)
            .await;

        // Filter out excluded agents
        let candidates: Vec<AgentDefinition> = candidates
            .into_iter()
            .filter(|a| !request.excluded_agents.contains(&a.id))
            .filter(|a| a.enabled)
            .collect();

        Ok(candidates)
    }

    /// Select best agent for a capability
    fn select_best_agent(
        &self,
        candidates: &[AgentDefinition],
        cap: AgentCapability,
        already_selected: &[AgentDefinition],
        request: &CapabilityRequest,
    ) -> Option<AgentDefinition> {
        let selected_ids: HashSet<&String> = already_selected.iter().map(|a| &a.id).collect();

        // Filter to agents that provide this capability and aren't selected
        let mut viable: Vec<&AgentDefinition> = candidates
            .iter()
            .filter(|a| a.provides(cap))
            .filter(|a| !selected_ids.contains(&a.id))
            .collect();

        if viable.is_empty() {
            return None;
        }

        // Score each candidate
        // Higher score = better choice
        let score = |agent: &AgentDefinition| -> i64 {
            let mut s: i64 = 0;

            // Prefer agents that provide more of our required capabilities
            let provided_required = agent
                .capabilities
                .iter()
                .filter(|c| request.required_capabilities.contains(c))
                .count();
            s += (provided_required as i64) * 100;

            // Prefer lower latency
            s -= agent.estimated_latency_ms as i64 / 10;

            // Prefer preferred agents
            if request.preferred_agents.contains(&agent.id) {
                s += 500;
            }

            // Prefer higher priority
            s -= (agent.priority as i64) * 50;

            // Prefer parallelizable if parallel is allowed
            if request.allow_parallel && agent.parallelizable {
                s += 25;
            }

            s
        };

        viable.sort_by_key(|b| std::cmp::Reverse(score(b)));

        viable.first().map(|a| (*a).clone())
    }

    /// Sort agents by priority and dependencies
    fn sort_agents(&self, agents: &mut [AgentDefinition]) {
        // Simple sort by priority
        // TODO: topological sort based on requires/provides
        agents.sort_by_key(|a| a.priority);
    }

    /// Build parallel execution groups
    fn build_parallel_groups(&self, agents: &[AgentDefinition]) -> Vec<Vec<String>> {
        let mut groups: Vec<Vec<String>> = Vec::new();
        let mut current_group: Vec<String> = Vec::new();

        for agent in agents {
            if agent.parallelizable {
                current_group.push(agent.id.clone());
            } else {
                // Flush current parallel group
                if !current_group.is_empty() {
                    groups.push(current_group);
                    current_group = Vec::new();
                }
                // Non-parallel agent is its own group
                groups.push(vec![agent.id.clone()]);
            }
        }

        // Flush remaining
        if !current_group.is_empty() {
            groups.push(current_group);
        }

        groups
    }

    /// Get resolver statistics
    pub async fn stats(&self) -> ResolverStats {
        let registry_stats = self.registry.stats().await;

        ResolverStats {
            available_agents: registry_stats.enabled_agents,
            available_capabilities: registry_stats.total_capabilities,
        }
    }
}

/// Resolver statistics
#[derive(Debug, Clone)]
pub struct ResolverStats {
    pub available_agents: usize,
    pub available_capabilities: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_registry::{AgentExecutor, AgentPriority};

    fn make_test_executor() -> AgentExecutor {
        Arc::new(|input: &[u8]| {
            let input = input.to_vec();
            Box::pin(async move { Ok::<Vec<u8>, anyhow::Error>(input) })
        })
    }

    async fn setup_test_registry() -> Arc<AgentRegistry> {
        let registry = Arc::new(AgentRegistry::new());

        // Code analyzer
        let analyzer = AgentDefinition::new("analyzer", "Code Analyzer")
            .with_capabilities(&[
                AgentCapability::CodeAnalysis,
                AgentCapability::DependencyAnalysis,
            ])
            .with_priority(AgentPriority::High)
            .with_latency(50);

        registry
            .register(analyzer, make_test_executor())
            .await
            .unwrap();

        // Test generator (requires analysis first)
        let tester = AgentDefinition::new("tester", "Test Generator")
            .with_capability(AgentCapability::TestGeneration)
            .requires_capability(AgentCapability::CodeAnalysis)
            .with_priority(AgentPriority::Normal)
            .with_latency(100);

        registry
            .register(tester, make_test_executor())
            .await
            .unwrap();

        // Security auditor
        let security = AgentDefinition::new("security", "Security Auditor")
            .with_capability(AgentCapability::SecurityAudit)
            .with_priority(AgentPriority::High)
            .parallelizable(true)
            .with_latency(75);

        registry
            .register(security, make_test_executor())
            .await
            .unwrap();

        // Doc generator
        let docs = AgentDefinition::new("docs", "Documentation Generator")
            .with_capability(AgentCapability::DocumentationGeneration)
            .requires_capability(AgentCapability::CodeAnalysis)
            .with_priority(AgentPriority::Low)
            .with_latency(80);

        registry.register(docs, make_test_executor()).await.unwrap();

        registry
    }

    #[tokio::test]
    async fn test_simple_resolution() {
        let registry = setup_test_registry().await;
        let resolver = CapabilityResolver::new(registry);

        let request = CapabilityRequest::new(vec![AgentCapability::CodeAnalysis], b"test".to_vec());

        let sequence = resolver.resolve(&request).await.unwrap();

        assert_eq!(sequence.agents.len(), 1);
        assert_eq!(sequence.agents[0].id, "analyzer");
        assert!(sequence.is_complete());
    }

    #[tokio::test]
    async fn test_multi_capability_resolution() {
        let registry = setup_test_registry().await;
        let resolver = CapabilityResolver::new(registry);

        let request = CapabilityRequest::new(
            vec![
                AgentCapability::CodeAnalysis,
                AgentCapability::TestGeneration,
                AgentCapability::SecurityAudit,
            ],
            b"test".to_vec(),
        );

        let sequence = resolver.resolve(&request).await.unwrap();

        assert_eq!(sequence.agents.len(), 3);
        assert!(sequence.is_complete());

        // Check priority ordering (High agents first)
        assert!(sequence.agents[0].priority <= sequence.agents[1].priority);
    }

    #[tokio::test]
    async fn test_agent_reuse() {
        let registry = setup_test_registry().await;
        let resolver = CapabilityResolver::new(registry);

        // Analyzer provides both CodeAnalysis and DependencyAnalysis
        let request = CapabilityRequest::new(
            vec![
                AgentCapability::CodeAnalysis,
                AgentCapability::DependencyAnalysis,
            ],
            b"test".to_vec(),
        );

        let sequence = resolver.resolve(&request).await.unwrap();

        // Should only need one agent (analyzer provides both)
        assert_eq!(sequence.agents.len(), 1);
        assert!(sequence.is_complete());
    }

    #[tokio::test]
    async fn test_missing_capability() {
        let registry = setup_test_registry().await;
        let resolver = CapabilityResolver::new(registry);

        let request = CapabilityRequest::new(
            vec![AgentCapability::Embedding], // Not provided by any agent
            b"test".to_vec(),
        );

        let sequence = resolver.resolve(&request).await.unwrap();

        assert!(!sequence.is_complete());
        assert!(sequence
            .missing_capabilities
            .contains(&AgentCapability::Embedding));
    }

    #[tokio::test]
    async fn test_preferred_agent() {
        let registry = setup_test_registry().await;

        // Add another analyzer
        let alt_analyzer = AgentDefinition::new("alt_analyzer", "Alternative Analyzer")
            .with_capability(AgentCapability::CodeAnalysis)
            .with_latency(25); // Faster

        registry
            .register(alt_analyzer, make_test_executor())
            .await
            .unwrap();

        let resolver = CapabilityResolver::new(registry);

        // Prefer the original analyzer even though alt is faster
        let request = CapabilityRequest::new(vec![AgentCapability::CodeAnalysis], b"test".to_vec())
            .prefer_agents(&["analyzer"]);

        let sequence = resolver.resolve(&request).await.unwrap();

        assert_eq!(sequence.agents[0].id, "analyzer");
    }

    #[tokio::test]
    async fn test_excluded_agent() {
        let registry = setup_test_registry().await;
        let resolver = CapabilityResolver::new(registry);

        let request = CapabilityRequest::new(vec![AgentCapability::CodeAnalysis], b"test".to_vec())
            .exclude_agents(&["analyzer"]);

        let sequence = resolver.resolve(&request).await.unwrap();

        // No agent can fulfill (only analyzer has this capability)
        assert!(!sequence.is_complete());
    }
}
