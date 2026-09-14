//! Request orchestrator with capability resolution and workstack routing
//!
//! Integrates capability resolution with workstack execution.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

use super::agent_registry::AgentRegistry;
use super::capability_resolver::{CapabilityRequest, CapabilityResolver, ResolvedSequence};
use super::numa::NumaTopology;
use super::pattern_tracker::{PatternTracker, PatternTrackerConfig};
use super::workstack_cache::{WorkstackCache, WorkstackCacheConfig};

/// Orchestrator configuration
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Minimum agents to trigger workstack routing (default: 2)
    pub workstack_threshold: usize,
    /// Enable intermediate step caching
    pub enable_caching: bool,
    /// Enable NUMA pinning for workstack execution
    pub numa_pinning: bool,
    /// Track patterns for optimization suggestions
    pub track_patterns: bool,
    /// Promotion threshold (calls before suggesting promotion)
    pub promotion_threshold: u32,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            workstack_threshold: 2,
            enable_caching: true,
            numa_pinning: true,
            track_patterns: true,
            promotion_threshold: 3,
        }
    }
}

// OrchestrationResult - different from tool OrchestrationResult
#[derive(Debug, Clone)]
pub struct OrchestrationResult {
    pub request_id: String,
    pub output: Vec<u8>,
    pub steps: Vec<StepResult>,
    pub total_latency_ms: u64,
    pub cache_hits: u32,
    pub cache_misses: u32,
    pub used_workstack: bool,
    pub resolved_agents: Vec<String>,
}

/// Individual step result
#[derive(Debug, Clone)]
pub struct StepResult {
    pub step_index: usize,
    pub agent_id: String,
    pub latency_ms: u64,
    pub cached: bool,
    pub output_size: usize,
}

pub struct Orchestrator {
    config: OrchestratorConfig,
    registry: Arc<AgentRegistry>,
    resolver: CapabilityResolver,
    cache: Arc<WorkstackCache>,
    pattern_tracker: Arc<PatternTracker>,
    #[allow(dead_code)]
    numa_topology: NumaTopology,
}

impl Orchestrator {
    /// Create new orchestrator
    pub async fn new(
        cache_dir: PathBuf,
        config: OrchestratorConfig,
        registry: Arc<AgentRegistry>,
    ) -> Result<Self> {
        let resolver = CapabilityResolver::new(registry.clone());

        let cache_config = WorkstackCacheConfig::default();
        let cache = WorkstackCache::new(cache_dir.clone(), cache_config).await?;

        let tracker_config = PatternTrackerConfig {
            promotion_threshold: config.promotion_threshold,
            track_enabled: config.track_patterns,
            ..Default::default()
        };
        let pattern_tracker = PatternTracker::new(cache_dir.clone(), tracker_config).await?;

        let numa_topology = NumaTopology::detect()?;

        info!(
            "Orchestrator initialized (workstack threshold: {}, NUMA nodes: {})",
            config.workstack_threshold,
            numa_topology.node_count()
        );

        Ok(Self {
            config,
            registry,
            resolver,
            cache: Arc::new(cache),
            pattern_tracker: Arc::new(pattern_tracker),
            numa_topology,
        })
    }

    /// Execute a capability-based request
    ///
    /// This is the main entry point:
    /// 1. Resolve capabilities to agent sequence
    /// 2. Route to workstack if 2+ agents
    /// 3. Cache intermediate results
    /// 4. Track patterns
    pub async fn execute(&self, request: CapabilityRequest) -> Result<OrchestrationResult> {
        let start_time = Instant::now();
        let request_id = uuid::Uuid::new_v4().to_string();

        // Step 1: Resolve capabilities to agents
        let sequence = self.resolver.resolve(&request).await?;

        if sequence.is_empty() {
            return Ok(OrchestrationResult {
                request_id,
                output: request.input,
                steps: Vec::new(),
                total_latency_ms: 0,
                cache_hits: 0,
                cache_misses: 0,
                used_workstack: false,
                resolved_agents: Vec::new(),
            });
        }

        if !sequence.is_complete() {
            warn!(
                "Request has unfulfilled capabilities: {:?}",
                sequence.missing_capabilities
            );
        }

        let agent_ids = sequence.agent_ids();
        let agent_count = agent_ids.len();

        info!(
            "Resolved {} capabilities to {} agents: {:?}",
            request.required_capabilities.len(),
            agent_count,
            agent_ids
        );

        // Step 2: Route based on agent count
        if agent_count >= self.config.workstack_threshold {
            self.execute_workstack(&request_id, sequence, request.input, start_time)
                .await
        } else {
            self.execute_single(&request_id, sequence, request.input, start_time)
                .await
        }
    }

    /// Execute with explicit agent IDs (bypass resolution)
    pub async fn execute_agents(
        &self,
        agent_ids: &[&str],
        input: Vec<u8>,
    ) -> Result<OrchestrationResult> {
        let start_time = Instant::now();
        let request_id = uuid::Uuid::new_v4().to_string();

        if agent_ids.is_empty() {
            return Ok(OrchestrationResult {
                request_id,
                output: input,
                steps: Vec::new(),
                total_latency_ms: 0,
                cache_hits: 0,
                cache_misses: 0,
                used_workstack: false,
                resolved_agents: Vec::new(),
            });
        }

        let agent_count = agent_ids.len();

        if agent_count >= self.config.workstack_threshold {
            self.execute_workstack_by_ids(&request_id, agent_ids, input, start_time)
                .await
        } else {
            self.execute_single_by_id(&request_id, agent_ids[0], input, start_time)
                .await
        }
    }

    /// Execute single agent (direct)
    async fn execute_single(
        &self,
        request_id: &str,
        sequence: ResolvedSequence,
        input: Vec<u8>,
        start_time: Instant,
    ) -> Result<OrchestrationResult> {
        let agent = sequence.agents.first().context("No agent in sequence")?;

        debug!("Executing single agent: {}", agent.id);

        let step_start = Instant::now();
        let output = self.registry.execute(&agent.id, &input).await?;
        let latency_ms = step_start.elapsed().as_millis() as u64;

        let step = StepResult {
            step_index: 0,
            agent_id: agent.id.clone(),
            latency_ms,
            cached: false,
            output_size: output.len(),
        };

        Ok(OrchestrationResult {
            request_id: request_id.to_string(),
            output,
            steps: vec![step],
            total_latency_ms: start_time.elapsed().as_millis() as u64,
            cache_hits: 0,
            cache_misses: 1,
            used_workstack: false,
            resolved_agents: sequence.agent_ids(),
        })
    }

    /// Execute single agent by ID
    async fn execute_single_by_id(
        &self,
        request_id: &str,
        agent_id: &str,
        input: Vec<u8>,
        start_time: Instant,
    ) -> Result<OrchestrationResult> {
        debug!("Executing single agent: {}", agent_id);

        let step_start = Instant::now();
        let output = self.registry.execute(agent_id, &input).await?;
        let latency_ms = step_start.elapsed().as_millis() as u64;

        let step = StepResult {
            step_index: 0,
            agent_id: agent_id.to_string(),
            latency_ms,
            cached: false,
            output_size: output.len(),
        };

        Ok(OrchestrationResult {
            request_id: request_id.to_string(),
            output,
            steps: vec![step],
            total_latency_ms: start_time.elapsed().as_millis() as u64,
            cache_hits: 0,
            cache_misses: 1,
            used_workstack: false,
            resolved_agents: vec![agent_id.to_string()],
        })
    }

    /// Execute multi-agent via workstack
    async fn execute_workstack(
        &self,
        request_id: &str,
        sequence: ResolvedSequence,
        input: Vec<u8>,
        start_time: Instant,
    ) -> Result<OrchestrationResult> {
        let agent_ids = sequence.agent_ids();
        let agent_refs: Vec<&str> = agent_ids.iter().map(|s| s.as_str()).collect();

        self.execute_workstack_by_ids(request_id, &agent_refs, input, start_time)
            .await
    }

    /// Execute workstack by agent IDs
    async fn execute_workstack_by_ids(
        &self,
        request_id: &str,
        agent_ids: &[&str],
        input: Vec<u8>,
        start_time: Instant,
    ) -> Result<OrchestrationResult> {
        let workstack_id = format!("ws-{}", &Self::hash_sequence(agent_ids, &input)[..12]);

        info!(
            "Routing to workstack: {} ({} agents)",
            workstack_id,
            agent_ids.len()
        );

        let mut steps = Vec::new();
        let mut current_input = input.clone();
        let mut cache_hits = 0usize;
        let mut cache_misses = 0usize;

        for (step_index, agent_id) in agent_ids.iter().enumerate() {
            let step_input_hash = Self::hash_bytes(&current_input);
            let step_start = Instant::now();

            // Try cache first
            let (output, cached) = if self.config.enable_caching {
                match self
                    .cache
                    .get(&workstack_id, step_index, &step_input_hash)?
                {
                    Some(cached_output) => {
                        debug!(
                            "Cache hit: {} step {} ({})",
                            workstack_id, step_index, agent_id
                        );
                        cache_hits += 1;
                        (cached_output, true)
                    }
                    None => {
                        cache_misses += 1;
                        let output = self.registry.execute(agent_id, &current_input).await?;

                        // Cache result
                        self.cache.put(
                            &workstack_id,
                            step_index,
                            &step_input_hash,
                            &output,
                            None,
                        )?;

                        (output, false)
                    }
                }
            } else {
                (
                    self.registry.execute(agent_id, &current_input).await?,
                    false,
                )
            };

            let latency_ms = step_start.elapsed().as_millis() as u64;

            steps.push(StepResult {
                step_index,
                agent_id: agent_id.to_string(),
                latency_ms,
                cached,
                output_size: output.len(),
            });

            current_input = output;
        }

        let total_latency_ms = start_time.elapsed().as_millis() as u64;

        // Track pattern
        if self.config.track_patterns {
            let input_hash = Self::hash_bytes(&input);
            if let Some(suggestion) =
                self.pattern_tracker
                    .record_sequence(agent_ids, &input_hash, total_latency_ms)?
            {
                info!(
                    "🔥 Pattern detected: '{}' called {} times",
                    suggestion.suggested_name, suggestion.pattern.call_count
                );
            }
        }

        Ok(OrchestrationResult {
            request_id: request_id.to_string(),
            output: current_input,
            steps,
            total_latency_ms,
            cache_hits: cache_hits as u32,
            cache_misses: cache_misses as u32,
            used_workstack: true,
            resolved_agents: agent_ids.iter().map(|s| s.to_string()).collect(),
        })
    }

    fn hash_bytes(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    fn hash_sequence(agents: &[&str], input: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(agents.join("→").as_bytes());
        hasher.update(input);
        format!("{:x}", hasher.finalize())
    }

    /// Get orchestrator statistics
    pub async fn stats(&self) -> Result<OrchestratorStats> {
        let registry_stats = self.registry.stats().await;
        let resolver_stats = self.resolver.stats().await;
        let cache_stats = self.cache.stats()?;
        let pattern_stats = self.pattern_tracker.stats()?;

        Ok(OrchestratorStats {
            registered_agents: registry_stats.total_agents,
            enabled_agents: registry_stats.enabled_agents,
            available_capabilities: resolver_stats.available_capabilities,
            tracked_patterns: pattern_stats.total_patterns,
            promoted_patterns: pattern_stats.promoted_count,
            cache_entries: cache_stats.total_entries,
            cache_hit_rate: cache_stats.hit_rate,
        })
    }

    /// Get the agent registry
    pub fn registry(&self) -> &Arc<AgentRegistry> {
        &self.registry
    }

    /// Get promotion candidates
    pub fn get_promotion_candidates(
        &self,
    ) -> Result<Vec<super::pattern_tracker::PromotionSuggestion>> {
        self.pattern_tracker.get_promotion_candidates()
    }
}

/// Orchestrator statistics
#[derive(Debug, Clone)]
pub struct OrchestratorStats {
    pub registered_agents: usize,
    pub enabled_agents: usize,
    pub available_capabilities: usize,
    pub tracked_patterns: u32,
    pub promoted_patterns: u32,
    pub cache_entries: u64,
    pub cache_hit_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_registry::{AgentCapability, AgentDefinition, AgentExecutor};

    fn make_echo_executor() -> AgentExecutor {
        Arc::new(|input: &[u8]| {
            let input = input.to_vec();
            Box::pin(async move { Ok::<Vec<u8>, anyhow::Error>(input) })
        })
    }

    fn make_transform_executor(suffix: &'static str) -> AgentExecutor {
        Arc::new(move |input: &[u8]| {
            let mut output = input.to_vec();
            output.extend_from_slice(suffix.as_bytes());
            Box::pin(async move { Ok::<Vec<u8>, anyhow::Error>(output) })
        })
    }

    async fn setup_test_orchestrator() -> (Orchestrator, tempfile::TempDir) {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let registry = Arc::new(AgentRegistry::new());

        // Register test agents
        let analyzer = AgentDefinition::new("analyzer", "Code Analyzer")
            .with_capability(AgentCapability::CodeAnalysis)
            .with_capability(AgentCapability::DependencyAnalysis);
        registry
            .register(analyzer, make_echo_executor())
            .await
            .unwrap();

        let tester = AgentDefinition::new("tester", "Test Generator")
            .with_capability(AgentCapability::TestGeneration);
        registry
            .register(tester, make_transform_executor("_TESTS"))
            .await
            .unwrap();

        let security = AgentDefinition::new("security", "Security Auditor")
            .with_capability(AgentCapability::SecurityAudit);
        registry
            .register(security, make_transform_executor("_SEC"))
            .await
            .unwrap();

        let config = OrchestratorConfig {
            numa_pinning: false,
            ..Default::default()
        };

        let orchestrator = Orchestrator::new(temp_dir.path().to_path_buf(), config, registry)
            .await
            .unwrap();

        // Keep the directory owner alive for the lifetime of the database
        // connections. Dropping it moves/deletes the files and causes SQLite
        // error 1032 ("database file has moved") on the first write.
        (orchestrator, temp_dir)
    }

    #[tokio::test]
    async fn test_single_capability_resolution() {
        let (orchestrator, _temp_dir) = setup_test_orchestrator().await;

        let request =
            CapabilityRequest::new(vec![AgentCapability::CodeAnalysis], b"test input".to_vec());

        let result = orchestrator.execute(request).await.unwrap();

        assert!(!result.used_workstack);
        assert_eq!(result.resolved_agents, vec!["analyzer"]);
        assert_eq!(result.output, b"test input");
    }

    #[tokio::test]
    async fn test_multi_capability_workstack() {
        let (orchestrator, _temp_dir) = setup_test_orchestrator().await;

        let request = CapabilityRequest::new(
            vec![
                AgentCapability::CodeAnalysis,
                AgentCapability::TestGeneration,
            ],
            b"code".to_vec(),
        );

        let result = orchestrator.execute(request).await.unwrap();

        assert!(result.used_workstack);
        assert_eq!(result.resolved_agents.len(), 2);
        // Output should have TESTS suffix from tester agent
        assert!(result.output.ends_with(b"_TESTS"));
    }

    #[tokio::test]
    async fn test_direct_agent_execution() {
        let (orchestrator, _temp_dir) = setup_test_orchestrator().await;

        let result = orchestrator
            .execute_agents(&["analyzer", "tester", "security"], b"input".to_vec())
            .await
            .unwrap();

        assert!(result.used_workstack);
        assert_eq!(result.steps.len(), 3);
        assert!(result.output.ends_with(b"_SEC")); // Last agent
    }
}
