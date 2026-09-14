//! Request orchestrator for tool execution and workstack routing
//!
//! Provides orchestration of tool execution with:
//! - Capability-based routing
//! - Workstack execution for multi-tool sequences
//! - Intermediate result caching
//! - Pattern tracking for optimization suggestions

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use op_core::error::Result;
use op_execution_tracker::ExecutionTracker;
use op_plugins::PluginCatalog;
use op_tools::registry::ToolRegistry;

// ============================================================================
// ORCHESTRATOR CONFIG
// ============================================================================

/// Orchestrator configuration
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Minimum tools to trigger workstack routing (default: 2)
    pub workstack_threshold: usize,
    /// Enable intermediate step caching
    pub enable_caching: bool,
    /// Track patterns for optimization suggestions
    pub track_patterns: bool,
    /// Promotion threshold (calls before suggesting promotion)
    pub promotion_threshold: u32,
    /// Maximum concurrent tool executions
    pub max_concurrent: usize,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            workstack_threshold: 2,
            enable_caching: true,
            track_patterns: true,
            promotion_threshold: 3,
            max_concurrent: 10,
        }
    }
}

// ============================================================================
// EXECUTION RESULT
// ============================================================================

/// Workflow execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowResult {
    pub request_id: String,
    pub success: bool,
    pub output: simd_json::OwnedValue,
    pub steps: Vec<StepResult>,
    pub total_latency_ms: u64,
    pub cache_hits: u32,
    pub cache_misses: u32,
    pub used_workstack: bool,
    pub resolved_tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Individual step result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step_index: usize,
    pub tool_name: String,
    pub latency_ms: u64,
    pub cached: bool,
    pub success: bool,
    pub error: Option<String>,
}

// ============================================================================
// PATTERN TRACKING
// ============================================================================

/// Tracked execution pattern
#[derive(Debug, Clone)]
pub struct ExecutionPattern {
    pub tool_sequence: Vec<String>,
    pub call_count: u32,
    pub total_latency_ms: u64,
    pub suggested_name: Option<String>,
}

impl ExecutionPattern {
    pub fn avg_latency_ms(&self) -> u64 {
        if self.call_count == 0 {
            0
        } else {
            self.total_latency_ms / self.call_count as u64
        }
    }
}

/// Pattern tracker for optimization suggestions
pub struct PatternTracker {
    patterns: RwLock<HashMap<String, ExecutionPattern>>,
    promotion_threshold: u32,
}

impl PatternTracker {
    pub fn new(promotion_threshold: u32) -> Self {
        Self {
            patterns: RwLock::new(HashMap::new()),
            promotion_threshold,
        }
    }

    /// Record a tool sequence execution
    pub async fn record(&self, tools: &[String], latency_ms: u64) -> Option<String> {
        let key = tools.join("→");
        let mut patterns = self.patterns.write().await;

        let pattern = patterns.entry(key.clone()).or_insert(ExecutionPattern {
            tool_sequence: tools.to_vec(),
            call_count: 0,
            total_latency_ms: 0,
            suggested_name: None,
        });

        pattern.call_count += 1;
        pattern.total_latency_ms += latency_ms;

        if pattern.call_count >= self.promotion_threshold && pattern.suggested_name.is_none() {
            let name = format!("combined_{}", &Self::hash_sequence(tools)[..8]);
            pattern.suggested_name = Some(name.clone());
            Some(name)
        } else {
            None
        }
    }

    /// Get patterns ready for promotion
    pub async fn get_promotion_candidates(&self) -> Vec<ExecutionPattern> {
        self.patterns
            .read()
            .await
            .values()
            .filter(|p| p.call_count >= self.promotion_threshold)
            .cloned()
            .collect()
    }

    fn hash_sequence(tools: &[String]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(tools.join(":").as_bytes());
        hex::encode(hasher.finalize())
    }
}

// ============================================================================
// INTERMEDIATE CACHE
// ============================================================================

/// Simple in-memory cache for intermediate results
pub struct IntermediateCache {
    cache: RwLock<HashMap<String, CachedResult>>,
    max_entries: usize,
}

#[derive(Clone)]
struct CachedResult {
    output: simd_json::OwnedValue,
    created_at: std::time::Instant,
}

impl IntermediateCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            max_entries,
        }
    }

    pub async fn get(&self, key: &str) -> Option<simd_json::OwnedValue> {
        let cache = self.cache.read().await;
        cache.get(key).map(|c| c.output.clone())
    }

    pub async fn put(&self, key: String, output: simd_json::OwnedValue) {
        let mut cache = self.cache.write().await;

        // Evict oldest if over limit
        if cache.len() >= self.max_entries {
            if let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, v)| v.created_at)
                .map(|(k, _)| k.clone())
            {
                cache.remove(&oldest_key);
            }
        }

        cache.insert(
            key,
            CachedResult {
                output,
                created_at: std::time::Instant::now(),
            },
        );
    }

    pub async fn stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        CacheStats {
            total_entries: cache.len(),
            max_entries: self.max_entries,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub max_entries: usize,
}

// ============================================================================
// ORCHESTRATOR
// ============================================================================

/// Main orchestrator for tool execution
pub struct Orchestrator {
    config: OrchestratorConfig,
    tool_registry: Arc<ToolRegistry>,
    /// Shared plugin catalog view used for lookup/routing only.
    ///
    /// The orchestrator is not allowed to become a second source of truth for
    /// plugin schema. It only consumes the already-registered catalog entries.
    #[allow(dead_code)]
    plugin_catalog: Arc<PluginCatalog>,
    execution_tracker: Arc<ExecutionTracker>,
    pattern_tracker: Arc<PatternTracker>,
    cache: Arc<IntermediateCache>,
}

impl Orchestrator {
    /// Create a new orchestrator
    pub fn new(
        config: OrchestratorConfig,
        tool_registry: Arc<ToolRegistry>,
        plugin_catalog: Arc<PluginCatalog>,
    ) -> Self {
        let pattern_tracker = PatternTracker::new(config.promotion_threshold);
        let cache = IntermediateCache::new(1000);
        let execution_tracker = ExecutionTracker::new(1000);

        Self {
            config,
            tool_registry,
            plugin_catalog,
            execution_tracker: Arc::new(execution_tracker),
            pattern_tracker: Arc::new(pattern_tracker),
            cache: Arc::new(cache),
        }
    }

    /// Execute a single tool
    pub async fn execute_tool(
        &self,
        tool_name: &str,
        input: simd_json::OwnedValue,
        session_id: Option<String>,
    ) -> Result<WorkflowResult> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let start_time = Instant::now();

        // Get the tool
        let tool = self
            .tool_registry
            .get(tool_name)
            .await
            .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", tool_name))?;

        // Start tracking
        let exec_record = self
            .execution_tracker
            .start_execution(tool_name, Some(input.clone()), session_id)
            .await;

        // Execute
        let result = tool.execute(input.clone()).await;

        // Record result
        match &result {
            Ok(output) => {
                self.execution_tracker
                    .complete_execution(
                        &exec_record.id,
                        Some(simd_json::to_string(output).unwrap_or_default()),
                    )
                    .await;
            }
            Err(e) => {
                self.execution_tracker
                    .fail_execution(&exec_record.id, e.to_string())
                    .await;
            }
        }

        let output = result?;
        let latency_ms = start_time.elapsed().as_millis() as u64;

        Ok(WorkflowResult {
            request_id,
            success: true,
            output,
            steps: vec![StepResult {
                step_index: 0,
                tool_name: tool_name.to_string(),
                latency_ms,
                cached: false,
                success: true,
                error: None,
            }],
            total_latency_ms: latency_ms,
            cache_hits: 0,
            cache_misses: 1,
            used_workstack: false,
            resolved_tools: vec![tool_name.to_string()],
            error: None,
        })
    }

    /// Execute a sequence of tools (workstack)
    pub async fn execute_sequence(
        &self,
        tool_names: &[&str],
        initial_input: simd_json::OwnedValue,
        session_id: Option<String>,
    ) -> Result<WorkflowResult> {
        let request_id = uuid::Uuid::new_v4().to_string();
        let start_time = Instant::now();

        if tool_names.is_empty() {
            return Ok(WorkflowResult {
                request_id,
                success: true,
                output: initial_input,
                steps: Vec::new(),
                total_latency_ms: 0,
                cache_hits: 0,
                cache_misses: 0,
                used_workstack: false,
                resolved_tools: Vec::new(),
                error: None,
            });
        }

        // Single tool - direct execution
        if tool_names.len() < self.config.workstack_threshold {
            return self
                .execute_tool(tool_names[0], initial_input, session_id)
                .await;
        }

        // Multi-tool workstack execution
        let mut steps = Vec::new();
        let mut current_input = initial_input;
        let mut cache_hits = 0usize;
        let mut cache_misses = 0usize;

        let workstack_id = format!(
            "ws-{}",
            &Self::hash_sequence_with_input(tool_names, &current_input)[..12]
        );

        for (step_index, tool_name) in tool_names.iter().enumerate() {
            let step_start = Instant::now();
            let cache_key = format!(
                "{}:{}:{}",
                workstack_id,
                step_index,
                Self::hash_input(&current_input)
            );

            // Try cache first
            let (output, cached) = if self.config.enable_caching {
                if let Some(cached_output) = self.cache.get(&cache_key).await {
                    cache_hits += 1;
                    (cached_output, true)
                } else {
                    cache_misses += 1;
                    let tool = self
                        .tool_registry
                        .get(tool_name)
                        .await
                        .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", tool_name))?;
                    let result = tool.execute(current_input.clone()).await?;

                    // Cache the result
                    self.cache.put(cache_key, result.clone()).await;

                    (result, false)
                }
            } else {
                cache_misses += 1;
                let tool = self
                    .tool_registry
                    .get(tool_name)
                    .await
                    .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", tool_name))?;
                let result = tool.execute(current_input.clone()).await?;
                (result, false)
            };

            let latency_ms = step_start.elapsed().as_millis() as u64;

            steps.push(StepResult {
                step_index,
                tool_name: tool_name.to_string(),
                latency_ms,
                cached,
                success: true,
                error: None,
            });

            current_input = output;
        }

        let total_latency_ms = start_time.elapsed().as_millis() as u64;

        // Track pattern
        if self.config.track_patterns {
            let tool_vec: Vec<String> = tool_names.iter().map(|s| s.to_string()).collect();
            if let Some(suggested_name) = self
                .pattern_tracker
                .record(&tool_vec, total_latency_ms)
                .await
            {
                tracing::info!(
                    "🔥 Pattern detected: '{}' ready for promotion",
                    suggested_name
                );
            }
        }

        Ok(WorkflowResult {
            request_id,
            success: true,
            output: current_input,
            steps,
            total_latency_ms,
            cache_hits: cache_hits as u32,
            cache_misses: cache_misses as u32,
            used_workstack: true,
            resolved_tools: tool_names.iter().map(|s| s.to_string()).collect(),
            error: None,
        })
    }

    /// Get orchestrator statistics
    pub async fn stats(&self) -> OrchestratorStats {
        let exec_stats = self.execution_tracker.get_stats().await;
        let cache_stats = self.cache.stats().await;
        let promotion_candidates = self.pattern_tracker.get_promotion_candidates().await;

        OrchestratorStats {
            total_executions: exec_stats.total_executions,
            successful_executions: exec_stats.successful_executions,
            failed_executions: exec_stats.failed_executions,
            avg_latency_ms: exec_stats.average_duration_ms(),
            cache_entries: cache_stats.total_entries,
            promotion_candidates: promotion_candidates.len(),
        }
    }

    fn hash_input(input: &simd_json::OwnedValue) -> String {
        let mut hasher = Sha256::new();
        hasher.update(simd_json::to_string(input).unwrap_or_default().as_bytes());
        hex::encode(hasher.finalize())[..16].to_string()
    }

    fn hash_sequence_with_input(tools: &[&str], input: &simd_json::OwnedValue) -> String {
        let mut hasher = Sha256::new();
        hasher.update(tools.join("→").as_bytes());
        hasher.update(simd_json::to_string(input).unwrap_or_default().as_bytes());
        hex::encode(hasher.finalize())
    }
}

/// Orchestrator statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorStats {
    pub total_executions: u64,
    pub successful_executions: u64,
    pub failed_executions: u64,
    pub avg_latency_ms: f64,
    pub cache_entries: usize,
    pub promotion_candidates: usize,
}
