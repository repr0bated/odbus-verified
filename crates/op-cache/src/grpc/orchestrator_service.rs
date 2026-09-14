//! Orchestrator service implementation
//!
//! Routes requests to agents based on capabilities,
//! manages workstacks, and tracks patterns.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use tracing::{debug, info};

use super::agent_service::AgentServiceImpl;
use super::cache_service::CacheServiceImpl;
use super::proto::{
    agent_service_server::AgentService, orchestrator_service_server::OrchestratorService, Empty,
    ExecuteAgentsRequest, FindByCapabilityRequest, GetPatternsResponse, OrchestratorRequest,
    OrchestratorResponse, OrchestratorStats, PatternSuggestion, PromotePatternRequest,
    PromotePatternResponse, ResolveRequest, ResolveResponse, WorkstackStepResult,
};

/// Tracked pattern for promotion suggestions
#[derive(Clone)]
struct TrackedPattern {
    pattern_id: String,
    agent_sequence: Vec<String>,
    call_count: u32,
    total_latency_ms: u64,
    #[allow(dead_code)]
    first_seen: Instant,
    last_called: Instant,
    promoted: bool,
}

pub struct OrchestratorServiceImpl {
    agent_service: Arc<AgentServiceImpl>,
    cache_service: Arc<CacheServiceImpl>,
    patterns: Arc<RwLock<HashMap<String, TrackedPattern>>>,
    workstack_threshold: usize,
    enable_caching: bool,
    promotion_threshold: u32,
}

impl OrchestratorServiceImpl {
    pub fn new(agent_service: Arc<AgentServiceImpl>, cache_service: Arc<CacheServiceImpl>) -> Self {
        Self {
            agent_service,
            cache_service,
            patterns: Arc::new(RwLock::new(HashMap::new())),
            workstack_threshold: 2,
            enable_caching: true,
            promotion_threshold: 3,
        }
    }

    pub fn with_config(
        agent_service: Arc<AgentServiceImpl>,
        cache_service: Arc<CacheServiceImpl>,
        workstack_threshold: usize,
        enable_caching: bool,
        promotion_threshold: u32,
    ) -> Self {
        Self {
            agent_service,
            cache_service,
            patterns: Arc::new(RwLock::new(HashMap::new())),
            workstack_threshold,
            enable_caching,
            promotion_threshold,
        }
    }

    /// Resolve capabilities to agent sequence
    async fn resolve_capabilities(
        &self,
        required: &[i32],
        preferred: &[String],
        excluded: &[String],
    ) -> Result<(Vec<super::proto::Agent>, Vec<i32>, Vec<i32>), Status> {
        let mut selected_agents = Vec::new();
        let mut fulfilled = HashSet::new();
        let excluded_set: HashSet<&String> = excluded.iter().collect();
        let preferred_set: HashSet<&String> = preferred.iter().collect();

        for &cap in required {
            if fulfilled.contains(&cap) {
                continue;
            }

            // Find agents for this capability
            let req = Request::new(FindByCapabilityRequest {
                capabilities: vec![cap],
                match_all: false,
            });

            let response: tonic::Response<super::proto::FindByCapabilityResponse> =
                self.agent_service.find_by_capability(req).await?;
            let candidates = response.into_inner().agents;

            // Filter excluded and select best
            let mut viable: Vec<_> = candidates
                .into_iter()
                .filter(|a| !excluded_set.contains(&a.id))
                .filter(|a| {
                    !selected_agents
                        .iter()
                        .any(|s: &super::proto::Agent| s.id == a.id)
                })
                .collect();

            // Sort by preference and latency
            viable.sort_by(|a, b| {
                let a_preferred = preferred_set.contains(&a.id);
                let b_preferred = preferred_set.contains(&b.id);
                match (a_preferred, b_preferred) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.estimated_latency_ms.cmp(&b.estimated_latency_ms),
                }
            });

            if let Some(agent) = viable.first() {
                for c in &agent.capabilities {
                    fulfilled.insert(*c);
                }
                selected_agents.push(agent.clone());
            }
        }

        // Sort by priority
        selected_agents.sort_by_key(|a| a.priority);

        let fulfilled_vec: Vec<i32> = fulfilled.into_iter().collect();
        let missing: Vec<i32> = required
            .iter()
            .filter(|c| !fulfilled_vec.contains(c))
            .copied()
            .collect();

        Ok((selected_agents, fulfilled_vec, missing))
    }

    /// Execute workstack with caching
    async fn execute_workstack(
        &self,
        workstack_id: &str,
        agent_ids: &[String],
        input: Vec<u8>,
    ) -> Result<(Vec<u8>, Vec<WorkstackStepResult>), Status> {
        let mut current_input = input;
        let mut steps = Vec::new();

        for (step_index, agent_id) in agent_ids.iter().enumerate() {
            let step_input_hash = Self::hash_bytes(&current_input);
            let step_start = Instant::now();

            // Try cache first
            let (output, cached) = if self.enable_caching {
                let cache_result = self
                    .cache_service
                    .get_step_internal(workstack_id, step_index as u32, &step_input_hash)
                    .await;

                match cache_result {
                    Some(cached_output) => {
                        debug!("Cache hit: {} step {}", workstack_id, step_index);
                        (cached_output, true)
                    }
                    None => {
                        // Execute agent
                        let exec_req = Request::new(super::proto::ExecuteAgentRequest {
                            agent_id: agent_id.clone(),
                            input: current_input.clone(),
                            context: HashMap::new(),
                            timeout_ms: 0,
                        });

                        let exec_response = self.agent_service.execute(exec_req).await?;
                        let result = exec_response.into_inner();

                        if !result.success {
                            return Err(Status::internal(format!(
                                "Agent {} failed: {}",
                                agent_id, result.error
                            )));
                        }

                        // Cache result
                        self.cache_service
                            .put_step_internal(
                                workstack_id,
                                step_index as u32,
                                &step_input_hash,
                                &result.output,
                            )
                            .await;

                        (result.output, false)
                    }
                }
            } else {
                // Caching disabled, execute directly
                let exec_req = Request::new(super::proto::ExecuteAgentRequest {
                    agent_id: agent_id.clone(),
                    input: current_input.clone(),
                    context: HashMap::new(),
                    timeout_ms: 0,
                });

                let exec_response = self.agent_service.execute(exec_req).await?;
                let result = exec_response.into_inner();

                if !result.success {
                    return Err(Status::internal(format!(
                        "Agent {} failed: {}",
                        agent_id, result.error
                    )));
                }

                (result.output, false)
            };

            let latency_ms = step_start.elapsed().as_millis() as u64;

            steps.push(WorkstackStepResult {
                step_index: step_index as u32,
                agent_id: agent_id.clone(),
                output: output.clone(),
                latency_ms,
                cached,
                output_size: output.len() as u64,
                success: true,
                error: String::new(),
            });

            current_input = output;
        }

        Ok((current_input, steps))
    }

    /// Track pattern for potential promotion
    async fn track_pattern(
        &self,
        agent_ids: &[String],
        latency_ms: u64,
    ) -> Option<PatternSuggestion> {
        let pattern_id = Self::hash_sequence(agent_ids);
        let now = Instant::now();

        let mut patterns = self.patterns.write().await;

        let pattern = patterns
            .entry(pattern_id.clone())
            .or_insert_with(|| TrackedPattern {
                pattern_id: pattern_id.clone(),
                agent_sequence: agent_ids.to_vec(),
                call_count: 0,
                total_latency_ms: 0,
                first_seen: now,
                last_called: now,
                promoted: false,
            });

        pattern.call_count += 1;
        pattern.total_latency_ms += latency_ms;
        pattern.last_called = now;

        if pattern.call_count >= self.promotion_threshold && !pattern.promoted {
            let avg_latency = pattern.total_latency_ms / pattern.call_count as u64;
            let suggested_name = Self::generate_workstack_name(&pattern.agent_sequence);

            return Some(PatternSuggestion {
                pattern_id: pattern.pattern_id.clone(),
                agent_sequence: pattern.agent_sequence.clone(),
                call_count: pattern.call_count,
                avg_latency_ms: avg_latency,
                suggested_name,
                confidence_score: Self::calculate_confidence(pattern),
                estimated_time_saved_ms: (avg_latency as f64 * 0.4 * pattern.call_count as f64)
                    as u64,
            });
        }

        None
    }

    fn hash_bytes(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    fn hash_sequence(agents: &[String]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(agents.join("→").as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn generate_workstack_name(agents: &[String]) -> String {
        if agents.is_empty() {
            return "unnamed".to_string();
        }
        let first = &agents[0];
        let last = agents.last().unwrap();
        if agents.len() == 2 {
            format!("{}-to-{}", first, last)
        } else {
            format!("{}-to-{}-{}step", first, last, agents.len())
        }
    }

    fn calculate_confidence(pattern: &TrackedPattern) -> f64 {
        let recency = pattern.last_called.elapsed().as_secs_f64() / 86400.0;
        let frequency = (pattern.call_count as f64 / 3.0).min(2.0) / 2.0;
        let recency_score = (1.0 - recency / 7.0).max(0.0);
        (frequency * 0.6 + recency_score * 0.4).min(1.0)
    }
}

#[tonic::async_trait]
impl OrchestratorService for OrchestratorServiceImpl {
    async fn execute(
        &self,
        request: Request<OrchestratorRequest>,
    ) -> Result<Response<OrchestratorResponse>, Status> {
        let req = request.into_inner();
        let start = Instant::now();
        let request_id = if req.request_id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            req.request_id
        };

        // Resolve capabilities to agents
        let (agents, fulfilled, missing) = self
            .resolve_capabilities(
                &req.required_capabilities,
                &req.preferred_agents,
                &req.excluded_agents,
            )
            .await?;

        if agents.is_empty() {
            return Ok(Response::new(OrchestratorResponse {
                request_id,
                output: req.input,
                steps: Vec::new(),
                total_latency_ms: 0,
                cache_hits: 0,
                cache_misses: 0,
                used_workstack: false,
                resolved_agents: Vec::new(),
                fulfilled_capabilities: fulfilled,
                missing_capabilities: missing,
            }));
        }

        let agent_ids: Vec<String> = agents.iter().map(|a| a.id.clone()).collect();
        let use_workstack = agent_ids.len() >= self.workstack_threshold;

        info!(
            "Executing request {} with {} agents (workstack: {})",
            request_id,
            agent_ids.len(),
            use_workstack
        );

        let (output, steps) = if use_workstack {
            let workstack_id = format!("ws-{}", &Self::hash_bytes(&req.input)[..12]);
            self.execute_workstack(&workstack_id, &agent_ids, req.input)
                .await?
        } else {
            // Single agent execution
            let exec_req = Request::new(super::proto::ExecuteAgentRequest {
                agent_id: agent_ids[0].clone(),
                input: req.input,
                context: HashMap::new(),
                timeout_ms: 0,
            });

            let result = self.agent_service.execute(exec_req).await?.into_inner();

            let step = WorkstackStepResult {
                step_index: 0,
                agent_id: agent_ids[0].clone(),
                output: result.output.clone(),
                latency_ms: result.latency_ms,
                cached: false,
                output_size: result.output.len() as u64,
                success: result.success,
                error: result.error,
            };

            (result.output, vec![step])
        };

        let total_latency_ms = start.elapsed().as_millis() as u64;
        let cache_hits = steps.iter().filter(|s| s.cached).count() as u32;
        let cache_misses = steps.iter().filter(|s| !s.cached).count() as u32;

        // Track pattern if workstack
        if use_workstack {
            if let Some(suggestion) = self.track_pattern(&agent_ids, total_latency_ms).await {
                info!(
                    "🔥 Pattern '{}' detected ({} calls)",
                    suggestion.suggested_name, suggestion.call_count
                );
            }
        }

        Ok(Response::new(OrchestratorResponse {
            request_id,
            output,
            steps,
            total_latency_ms,
            cache_hits,
            cache_misses,
            used_workstack: use_workstack,
            resolved_agents: agent_ids,
            fulfilled_capabilities: fulfilled,
            missing_capabilities: missing,
        }))
    }

    type ExecuteStreamStream =
        tokio_stream::wrappers::ReceiverStream<Result<WorkstackStepResult, Status>>;

    async fn execute_stream(
        &self,
        request: Request<OrchestratorRequest>,
    ) -> Result<Response<Self::ExecuteStreamStream>, Status> {
        // Similar to execute but streams each step result
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        let req = request.into_inner();

        let agent_service = self.agent_service.clone();
        let cache_service = self.cache_service.clone();
        let enable_caching = self.enable_caching;

        let (agents, _, _) = self
            .resolve_capabilities(
                &req.required_capabilities,
                &req.preferred_agents,
                &req.excluded_agents,
            )
            .await?;

        let agent_ids: Vec<String> = agents.iter().map(|a| a.id.clone()).collect();

        tokio::spawn(async move {
            let workstack_id = format!("ws-{}", &Self::hash_bytes(&req.input)[..12]);
            let mut current_input = req.input;

            for (step_index, agent_id) in agent_ids.iter().enumerate() {
                let step_input_hash = Self::hash_bytes(&current_input);
                let step_start = Instant::now();

                let (output, cached) = if enable_caching {
                    let cache_result = cache_service
                        .get_step_internal(&workstack_id, step_index as u32, &step_input_hash)
                        .await;

                    match cache_result {
                        Some(cached_output) => (cached_output, true),
                        None => {
                            let exec_req = Request::new(super::proto::ExecuteAgentRequest {
                                agent_id: agent_id.clone(),
                                input: current_input.clone(),
                                context: HashMap::new(),
                                timeout_ms: 0,
                            });

                            match agent_service.execute(exec_req).await {
                                Ok(resp) => {
                                    let result = resp.into_inner();
                                    if result.success {
                                        cache_service
                                            .put_step_internal(
                                                &workstack_id,
                                                step_index as u32,
                                                &step_input_hash,
                                                &result.output,
                                            )
                                            .await;
                                        (result.output, false)
                                    } else {
                                        let _ = tx.send(Err(Status::internal(result.error))).await;
                                        return;
                                    }
                                }
                                Err(e) => {
                                    let _ = tx.send(Err(e)).await;
                                    return;
                                }
                            }
                        }
                    }
                } else {
                    let exec_req = Request::new(super::proto::ExecuteAgentRequest {
                        agent_id: agent_id.clone(),
                        input: current_input.clone(),
                        context: HashMap::new(),
                        timeout_ms: 0,
                    });

                    match agent_service.execute(exec_req).await {
                        Ok(resp) => {
                            let result = resp.into_inner();
                            if result.success {
                                (result.output, false)
                            } else {
                                let _ = tx.send(Err(Status::internal(result.error))).await;
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e)).await;
                            return;
                        }
                    }
                };

                let step = WorkstackStepResult {
                    step_index: step_index as u32,
                    agent_id: agent_id.clone(),
                    output: output.clone(),
                    latency_ms: step_start.elapsed().as_millis() as u64,
                    cached,
                    output_size: output.len() as u64,
                    success: true,
                    error: String::new(),
                };

                if tx.send(Ok(step)).await.is_err() {
                    return;
                }

                current_input = output;
            }
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }

    async fn execute_agents(
        &self,
        request: Request<ExecuteAgentsRequest>,
    ) -> Result<Response<OrchestratorResponse>, Status> {
        let req = request.into_inner();
        let start = Instant::now();
        let request_id = if req.request_id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            req.request_id
        };

        let use_workstack = req.agent_ids.len() >= self.workstack_threshold;

        let (output, steps) = if use_workstack {
            let workstack_id = format!("ws-{}", &Self::hash_bytes(&req.input)[..12]);
            self.execute_workstack(&workstack_id, &req.agent_ids, req.input)
                .await?
        } else if !req.agent_ids.is_empty() {
            let exec_req = Request::new(super::proto::ExecuteAgentRequest {
                agent_id: req.agent_ids[0].clone(),
                input: req.input,
                context: HashMap::new(),
                timeout_ms: 0,
            });

            let result = self.agent_service.execute(exec_req).await?.into_inner();
            let step = WorkstackStepResult {
                step_index: 0,
                agent_id: req.agent_ids[0].clone(),
                output: result.output.clone(),
                latency_ms: result.latency_ms,
                cached: false,
                output_size: result.output.len() as u64,
                success: result.success,
                error: result.error,
            };
            (result.output, vec![step])
        } else {
            return Err(Status::invalid_argument("No agents specified"));
        };

        let cache_hits = steps.iter().filter(|s| s.cached).count() as u32;
        let cache_misses = steps.iter().filter(|s| !s.cached).count() as u32;

        Ok(Response::new(OrchestratorResponse {
            request_id,
            output,
            steps,
            total_latency_ms: start.elapsed().as_millis() as u64,
            cache_hits,
            cache_misses,
            used_workstack: use_workstack,
            resolved_agents: req.agent_ids,
            fulfilled_capabilities: Vec::new(),
            missing_capabilities: Vec::new(),
        }))
    }

    async fn resolve(
        &self,
        request: Request<ResolveRequest>,
    ) -> Result<Response<ResolveResponse>, Status> {
        let req = request.into_inner();

        let (agents, fulfilled, missing) = self
            .resolve_capabilities(
                &req.required_capabilities,
                &req.preferred_agents,
                &req.excluded_agents,
            )
            .await?;

        let estimated_latency_ms: u64 = agents.iter().map(|a| a.estimated_latency_ms).sum();

        Ok(Response::new(ResolveResponse {
            agents,
            fulfilled_capabilities: fulfilled,
            missing_capabilities: missing,
            estimated_latency_ms,
            resolution_path: Vec::new(),
        }))
    }

    async fn get_patterns(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<GetPatternsResponse>, Status> {
        let patterns = self.patterns.read().await;

        let suggestions: Vec<PatternSuggestion> = patterns
            .values()
            .filter(|p| p.call_count >= self.promotion_threshold && !p.promoted)
            .map(|p| {
                let avg_latency = if p.call_count > 0 {
                    p.total_latency_ms / p.call_count as u64
                } else {
                    0
                };
                PatternSuggestion {
                    pattern_id: p.pattern_id.clone(),
                    agent_sequence: p.agent_sequence.clone(),
                    call_count: p.call_count,
                    avg_latency_ms: avg_latency,
                    suggested_name: Self::generate_workstack_name(&p.agent_sequence),
                    confidence_score: Self::calculate_confidence(p),
                    estimated_time_saved_ms: (avg_latency as f64 * 0.4 * p.call_count as f64)
                        as u64,
                }
            })
            .collect();

        Ok(Response::new(GetPatternsResponse {
            patterns: suggestions,
        }))
    }

    async fn promote_pattern(
        &self,
        request: Request<PromotePatternRequest>,
    ) -> Result<Response<PromotePatternResponse>, Status> {
        let req = request.into_inner();

        let mut patterns = self.patterns.write().await;

        if let Some(pattern) = patterns.get_mut(&req.pattern_id) {
            pattern.promoted = true;
            let workstack_id = format!("WS-{}", &pattern.pattern_id[..8]);

            info!(
                "Promoted pattern {} to workstack {}",
                req.pattern_id, workstack_id
            );

            Ok(Response::new(PromotePatternResponse {
                success: true,
                workstack_id,
                error: String::new(),
            }))
        } else {
            Ok(Response::new(PromotePatternResponse {
                success: false,
                workstack_id: String::new(),
                error: "Pattern not found".to_string(),
            }))
        }
    }

    async fn get_stats(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<OrchestratorStats>, Status> {
        let agents_response = self
            .agent_service
            .list_agents(Request::new(super::proto::ListAgentsRequest {
                enabled_only: false,
            }))
            .await?
            .into_inner();

        let caps_response = self
            .agent_service
            .list_capabilities(Request::new(Empty {}))
            .await?
            .into_inner();

        let cache_stats = self.cache_service.get_stats_internal().await;
        let patterns = self.patterns.read().await;

        let promoted_count = patterns.values().filter(|p| p.promoted).count() as u32;
        let tracked_count = patterns.len() as u32;

        Ok(Response::new(OrchestratorStats {
            registered_agents: agents_response.agents.len() as u32,
            enabled_agents: agents_response.agents.iter().filter(|a| a.enabled).count() as u32,
            available_capabilities: caps_response.capabilities.len() as u32,
            tracked_patterns: tracked_count,
            promoted_patterns: promoted_count,
            cache_entries: cache_stats.total_entries,
            cache_hit_rate: cache_stats.hit_rate,
            numa_nodes: 1, // TODO: get from actual NUMA topology
        }))
    }
}
