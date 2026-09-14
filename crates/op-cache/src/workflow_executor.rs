//! NUMA-aware workflow execution engine
//!
//! Executes multi-agent workflows with:
//! - Pipeline affinity (all steps on same NUMA node)
//! - Automatic intermediate caching
//! - Parallel step execution where possible
//! - Progress tracking and metrics

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use super::numa::NumaTopology;
use super::workflow_cache::{WorkflowCache, WorkflowCacheConfig};
use super::workflow_tracker::{PromotedWorkflow, WorkflowTracker, WorkflowTrackerConfig};

/// Configuration for workflow executor
#[derive(Debug, Clone)]
pub struct WorkflowExecutorConfig {
    /// Enable NUMA pinning for workflow execution
    pub numa_pinning: bool,
    /// Enable intermediate step caching
    pub enable_caching: bool,
    /// Maximum parallel steps (0 = unlimited)
    pub max_parallel_steps: usize,
    /// Timeout per step in seconds
    pub step_timeout_secs: u64,
    /// Retry failed steps
    pub retry_on_failure: bool,
    /// Maximum retries per step
    pub max_retries: u32,
}

impl Default for WorkflowExecutorConfig {
    fn default() -> Self {
        Self {
            numa_pinning: true,
            enable_caching: true,
            max_parallel_steps: 4,
            step_timeout_secs: 300, // 5 minutes
            retry_on_failure: true,
            max_retries: 2,
        }
    }
}

/// Result of a workflow step execution
#[derive(Debug, Clone)]
pub struct StepResult {
    pub step_index: usize,
    pub agent_id: String,
    pub output: Vec<u8>,
    pub latency_ms: u64,
    pub cached: bool,
    pub retries: u32,
}

/// Result of a complete workflow execution
#[derive(Debug, Clone)]
pub struct WorkflowResult {
    pub workflow_id: String,
    pub steps: Vec<StepResult>,
    pub total_latency_ms: u64,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub numa_node: Option<u32>,
}

impl WorkflowResult {
    /// Get the final output (last step's output)
    pub fn final_output(&self) -> Option<&[u8]> {
        self.steps.last().map(|s| s.output.as_slice())
    }

    /// Calculate cache hit rate
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            0.0
        } else {
            self.cache_hits as f64 / total as f64
        }
    }

    /// Calculate time saved by caching (estimated)
    pub fn estimated_time_saved_ms(&self) -> u64 {
        // Assume cached results are 90% faster
        self.steps
            .iter()
            .filter(|s| s.cached)
            .map(|s| (s.latency_ms as f64 * 9.0) as u64) // 90% of what it would have taken
            .sum()
    }
}

/// Agent function type for workflow steps
pub type AgentFn = Arc<dyn Fn(&[u8]) -> Result<Vec<u8>> + Send + Sync>;

/// Workflow execution progress callback
pub type ProgressCallback = Arc<dyn Fn(usize, usize, &str) + Send + Sync>;

pub struct WorkflowExecutor {
    config: WorkflowExecutorConfig,
    cache: Arc<WorkflowCache>,
    tracker: Arc<WorkflowTracker>,
    numa_topology: NumaTopology,
    agents: RwLock<HashMap<String, AgentFn>>,
    pinned_node: RwLock<Option<u32>>,
}

impl WorkflowExecutor {
    /// Create new workflow executor
    pub async fn new(cache_dir: PathBuf, config: WorkflowExecutorConfig) -> Result<Self> {
        let cache = WorkflowCache::new(cache_dir.clone(), WorkflowCacheConfig::default()).await?;
        let tracker =
            WorkflowTracker::new(cache_dir.clone(), WorkflowTrackerConfig::default()).await?;
        let numa_topology = NumaTopology::detect()?;

        info!(
            "Workflow executor initialized (NUMA nodes: {}, pinning: {})",
            numa_topology.node_count(),
            config.numa_pinning
        );

        Ok(Self {
            config,
            cache: Arc::new(cache),
            tracker: Arc::new(tracker),
            numa_topology,
            agents: RwLock::new(HashMap::new()),
            pinned_node: RwLock::new(None),
        })
    }

    /// Register an agent function
    pub async fn register_agent(&self, agent_id: &str, agent_fn: AgentFn) {
        let mut agents = self.agents.write().await;
        agents.insert(agent_id.to_string(), agent_fn);
        debug!("Registered agent: {}", agent_id);
    }

    /// Execute a workflow by ID
    pub async fn execute(
        &self,
        workflow_id: &str,
        input: &[u8],
        progress: Option<ProgressCallback>,
    ) -> Result<WorkflowResult> {
        // Get workflow definition
        let workflow = self
            .tracker
            .get_workflow(workflow_id)?
            .context(format!("Workflow not found: {}", workflow_id))?;

        self.execute_workflow(&workflow, input, progress).await
    }

    /// Execute a workflow from definition
    pub async fn execute_workflow(
        &self,
        workflow: &PromotedWorkflow,
        input: &[u8],
        progress: Option<ProgressCallback>,
    ) -> Result<WorkflowResult> {
        let start_time = Instant::now();
        let _input_hash = self.hash_input(input);

        info!(
            "Executing workflow {} ({} steps)",
            workflow.workflow_id,
            workflow.agent_sequence.len()
        );

        // Pin to NUMA node if enabled
        let numa_node = if self.config.numa_pinning {
            self.pin_to_optimal_node().await?
        } else {
            None
        };

        let mut steps = Vec::new();
        let mut current_input = input.to_vec();
        let mut cache_hits = 0usize;
        let mut cache_misses = 0usize;

        let total_steps = workflow.agent_sequence.len();

        for (step_index, agent_id) in workflow.agent_sequence.iter().enumerate() {
            // Report progress
            if let Some(ref callback) = progress {
                callback(step_index, total_steps, agent_id);
            }

            let step_input_hash = self.hash_input(&current_input);
            let step_start = Instant::now();

            // Try cache first
            let (output, cached) = if self.config.enable_caching {
                match self
                    .cache
                    .get(&workflow.workflow_id, step_index, &step_input_hash)?
                {
                    Some(cached_output) => {
                        debug!(
                            "Cache hit for workflow {} step {}",
                            workflow.workflow_id, step_index
                        );
                        cache_hits += 1;
                        (cached_output, true)
                    }
                    None => {
                        cache_misses += 1;
                        let output = self
                            .execute_step(agent_id, &current_input, step_index)
                            .await?;

                        // Cache the result
                        self.cache.put(
                            &workflow.workflow_id,
                            step_index,
                            &step_input_hash,
                            &output,
                            None,
                        )?;

                        (output, false)
                    }
                }
            } else {
                let output = self
                    .execute_step(agent_id, &current_input, step_index)
                    .await?;
                (output, false)
            };

            let latency_ms = step_start.elapsed().as_millis() as u64;

            steps.push(StepResult {
                step_index,
                agent_id: agent_id.clone(),
                output: output.clone(),
                latency_ms,
                cached,
                retries: 0, // TODO: track retries
            });

            // Output becomes input for next step
            current_input = output;
        }

        let total_latency_ms = start_time.elapsed().as_millis() as u64;

        // Record execution
        self.tracker.record_execution(&workflow.workflow_id)?;

        info!(
            "Workflow {} completed in {}ms (cache: {}/{} hits)",
            workflow.workflow_id, total_latency_ms, cache_hits, total_steps
        );

        Ok(WorkflowResult {
            workflow_id: workflow.workflow_id.clone(),
            steps,
            total_latency_ms,
            cache_hits,
            cache_misses,
            numa_node,
        })
    }

    /// Execute ad-hoc agent sequence (not a saved workflow)
    pub async fn execute_sequence(
        &self,
        agents: &[&str],
        input: &[u8],
        progress: Option<ProgressCallback>,
    ) -> Result<WorkflowResult> {
        let start_time = Instant::now();
        let input_hash = self.hash_input(input);

        // Generate a temporary workflow ID for caching
        let workflow_id = format!("adhoc-{}", &input_hash[..8]);

        info!("Executing ad-hoc sequence ({} agents)", agents.len());

        // Pin to NUMA node if enabled
        let numa_node = if self.config.numa_pinning {
            self.pin_to_optimal_node().await?
        } else {
            None
        };

        let mut steps = Vec::new();
        let mut current_input = input.to_vec();
        let mut cache_hits = 0usize;
        let mut cache_misses = 0usize;
        let total_steps = agents.len();

        for (step_index, agent_id) in agents.iter().enumerate() {
            if let Some(ref callback) = progress {
                callback(step_index, total_steps, agent_id);
            }

            let step_input_hash = self.hash_input(&current_input);
            let step_start = Instant::now();

            // Try cache (even for ad-hoc sequences)
            let (output, cached) = if self.config.enable_caching {
                match self.cache.get(&workflow_id, step_index, &step_input_hash)? {
                    Some(cached_output) => {
                        cache_hits += 1;
                        (cached_output, true)
                    }
                    None => {
                        cache_misses += 1;
                        let output = self
                            .execute_step(agent_id, &current_input, step_index)
                            .await?;
                        self.cache.put(
                            &workflow_id,
                            step_index,
                            &step_input_hash,
                            &output,
                            None,
                        )?;
                        (output, false)
                    }
                }
            } else {
                let output = self
                    .execute_step(agent_id, &current_input, step_index)
                    .await?;
                (output, false)
            };

            let latency_ms = step_start.elapsed().as_millis() as u64;

            steps.push(StepResult {
                step_index,
                agent_id: agent_id.to_string(),
                output: output.clone(),
                latency_ms,
                cached,
                retries: 0,
            });

            current_input = output;
        }

        let total_latency_ms = start_time.elapsed().as_millis() as u64;

        // Record sequence for pattern detection
        if let Some(suggestion) =
            self.tracker
                .record_sequence(agents, &input_hash, total_latency_ms)?
        {
            info!(
                "Pattern detected! Suggest creating workflow '{}' (called {} times)",
                suggestion.suggested_name, suggestion.pattern.call_count
            );
        }

        Ok(WorkflowResult {
            workflow_id,
            steps,
            total_latency_ms,
            cache_hits,
            cache_misses,
            numa_node,
        })
    }

    /// Execute a single step with retry support
    async fn execute_step(
        &self,
        agent_id: &str,
        input: &[u8],
        step_index: usize,
    ) -> Result<Vec<u8>> {
        let agents = self.agents.read().await;
        let agent_fn = agents
            .get(agent_id)
            .context(format!("Agent not found: {}", agent_id))?;

        let mut last_error = None;
        let max_attempts = if self.config.retry_on_failure {
            self.config.max_retries + 1
        } else {
            1
        };

        for attempt in 0..max_attempts {
            match agent_fn(input) {
                Ok(output) => {
                    if attempt > 0 {
                        debug!(
                            "Step {} ({}) succeeded after {} retries",
                            step_index, agent_id, attempt
                        );
                    }
                    return Ok(output);
                }
                Err(e) => {
                    warn!(
                        "Step {} ({}) failed (attempt {}/{}): {}",
                        step_index,
                        agent_id,
                        attempt + 1,
                        max_attempts,
                        e
                    );
                    last_error = Some(e);

                    if attempt < max_attempts - 1 {
                        // Exponential backoff
                        tokio::time::sleep(tokio::time::Duration::from_millis(
                            100 * 2u64.pow(attempt),
                        ))
                        .await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Unknown error in step execution")))
    }

    /// Pin workflow execution to optimal NUMA node
    async fn pin_to_optimal_node(&self) -> Result<Option<u32>> {
        if !self.numa_topology.is_numa_system() {
            return Ok(None);
        }

        let optimal_node = self.numa_topology.optimal_node();

        // Store pinned node
        {
            let mut pinned = self.pinned_node.write().await;
            *pinned = Some(optimal_node);
        }

        // Apply CPU affinity
        let cpus = self.numa_topology.cpus_for_node(optimal_node);
        if !cpus.is_empty() {
            let cpu_list = cpus
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(",");

            // Best effort - don't fail if taskset unavailable
            let _ = tokio::process::Command::new("taskset")
                .args(["-cp", &cpu_list, &std::process::id().to_string()])
                .output()
                .await;

            debug!(
                "Pinned workflow execution to NUMA node {} (CPUs: {})",
                optimal_node, cpu_list
            );
        }

        Ok(Some(optimal_node))
    }

    /// Hash input for cache keying
    fn hash_input(&self, input: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(input);
        format!("{:x}", hasher.finalize())
    }

    /// Get executor statistics
    pub async fn stats(&self) -> Result<ExecutorStats> {
        let tracker_stats = self.tracker.stats()?;
        let cache_stats = self.cache.stats()?;
        let agents = self.agents.read().await;

        Ok(ExecutorStats {
            registered_agents: agents.len(),
            promoted_workflows: tracker_stats.promoted_count as usize,
            pending_promotions: tracker_stats.pending_promotion as usize,
            total_workflow_executions: tracker_stats.total_workflow_executions,
            cache_entries: cache_stats.total_entries,
            cache_size_bytes: cache_stats.total_size_bytes,
            cache_hit_rate: cache_stats.hit_rate,
            numa_nodes: self.numa_topology.node_count(),
            numa_pinning_enabled: self.config.numa_pinning,
        })
    }

    /// Get promotion suggestions
    pub fn get_promotion_suggestions(
        &self,
    ) -> Result<Vec<super::workflow_tracker::PromotionSuggestion>> {
        self.tracker.get_promotion_candidates()
    }

    /// Promote a pattern to workflow
    pub fn promote_pattern(
        &self,
        pattern: &super::workflow_tracker::WorkflowPattern,
    ) -> Result<String> {
        self.tracker.promote_pattern(pattern)
    }

    /// Get all promoted workflows
    pub fn get_workflows(&self) -> Result<Vec<PromotedWorkflow>> {
        self.tracker.get_promoted_workflows()
    }

    /// Invalidate workflow cache
    pub fn invalidate_workflow_cache(&self, workflow_id: &str) -> Result<usize> {
        self.cache.invalidate_workflow(workflow_id)
    }

    /// Cleanup expired cache entries
    pub fn cleanup_cache(&self) -> Result<super::workflow_cache::CleanupResult> {
        self.cache.cleanup_expired()
    }
}

/// Executor statistics
#[derive(Debug, Clone)]
pub struct ExecutorStats {
    pub registered_agents: usize,
    pub promoted_workflows: usize,
    pub pending_promotions: usize,
    pub total_workflow_executions: u64,
    pub cache_entries: u64,
    pub cache_size_bytes: u64,
    pub cache_hit_rate: f64,
    pub numa_nodes: usize,
    pub numa_pinning_enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_echo_agent() -> AgentFn {
        Arc::new(|input: &[u8]| Ok(input.to_vec()))
    }

    fn make_transform_agent(suffix: &'static str) -> AgentFn {
        Arc::new(move |input: &[u8]| {
            let mut output = input.to_vec();
            output.extend_from_slice(suffix.as_bytes());
            Ok(output)
        })
    }

    #[tokio::test]
    async fn test_executor_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkflowExecutorConfig::default();
        let executor = WorkflowExecutor::new(temp_dir.path().to_path_buf(), config).await;
        assert!(executor.is_ok());
    }

    #[tokio::test]
    async fn test_agent_registration() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkflowExecutorConfig::default();
        let executor = WorkflowExecutor::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        executor.register_agent("echo", make_echo_agent()).await;
        executor
            .register_agent("transform", make_transform_agent("_suffix"))
            .await;

        let stats = executor.stats().await.unwrap();
        assert_eq!(stats.registered_agents, 2);
    }

    #[tokio::test]
    async fn test_sequence_execution() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkflowExecutorConfig {
            numa_pinning: false, // Disable for tests
            ..Default::default()
        };
        let executor = WorkflowExecutor::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        executor.register_agent("echo", make_echo_agent()).await;
        executor
            .register_agent("add_a", make_transform_agent("A"))
            .await;
        executor
            .register_agent("add_b", make_transform_agent("B"))
            .await;

        let result = executor
            .execute_sequence(&["echo", "add_a", "add_b"], b"input", None)
            .await
            .unwrap();

        assert_eq!(result.steps.len(), 3);
        assert_eq!(result.final_output().unwrap(), b"inputAB");
    }

    #[tokio::test]
    async fn test_caching() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkflowExecutorConfig {
            numa_pinning: false,
            enable_caching: true,
            ..Default::default()
        };
        let executor = WorkflowExecutor::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        executor.register_agent("echo", make_echo_agent()).await;

        // First execution - cache miss
        let result1 = executor
            .execute_sequence(&["echo"], b"test", None)
            .await
            .unwrap();
        assert_eq!(result1.cache_misses, 1);
        assert_eq!(result1.cache_hits, 0);

        // Second execution - cache hit
        let result2 = executor
            .execute_sequence(&["echo"], b"test", None)
            .await
            .unwrap();
        assert_eq!(result2.cache_hits, 1);
    }

    #[tokio::test]
    async fn test_pattern_detection() {
        let temp_dir = TempDir::new().unwrap();
        let config = WorkflowExecutorConfig {
            numa_pinning: false,
            ..Default::default()
        };
        let executor = WorkflowExecutor::new(temp_dir.path().to_path_buf(), config)
            .await
            .unwrap();

        executor.register_agent("agent_a", make_echo_agent()).await;
        executor.register_agent("agent_b", make_echo_agent()).await;

        let sequence = &["agent_a", "agent_b"];

        // Execute multiple times to trigger pattern detection
        for i in 0..4 {
            let input = format!("input_{}", i);
            executor
                .execute_sequence(sequence, input.as_bytes(), None)
                .await
                .unwrap();
        }

        // Check for promotion suggestions
        let suggestions = executor.get_promotion_suggestions().unwrap();
        assert!(!suggestions.is_empty());
    }
}
