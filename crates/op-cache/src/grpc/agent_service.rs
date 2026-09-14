//! Agent service implementation
//!
//! Manages agent registration, execution, and capability queries.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use tracing::{debug, info, warn};

use super::proto::{
    agent_service_server::AgentService, Agent, Empty, ExecuteAgentChunk, ExecuteAgentRequest,
    ExecuteAgentResponse, FindByCapabilityRequest, FindByCapabilityResponse, GetAgentRequest,
    HealthCheckRequest, HealthCheckResponse, ListAgentsRequest, ListAgentsResponse,
    ListCapabilitiesResponse, RegisterAgentRequest, RegisterAgentResponse, UnregisterAgentRequest,
    UnregisterAgentResponse,
};

/// Agent executor function type
pub type AgentExecutor = Arc<dyn Fn(&[u8]) -> Result<Vec<u8>, String> + Send + Sync>;

/// Registered agent with metadata and executor
struct RegisteredAgent {
    definition: Agent,
    executor: Option<AgentExecutor>,
    #[allow(dead_code)]
    endpoint: Option<String>,
    registered_at: std::time::Instant,
}

pub struct AgentServiceImpl {
    agents: Arc<RwLock<HashMap<String, RegisteredAgent>>>,
    capability_index: Arc<RwLock<HashMap<i32, Vec<String>>>>,
}

impl AgentServiceImpl {
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
            capability_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a local agent with an executor function
    pub async fn register_local(
        &self,
        agent: Agent,
        executor: AgentExecutor,
    ) -> Result<(), String> {
        let agent_id = agent.id.clone();
        let capabilities = agent.capabilities.clone();

        {
            let mut agents = self.agents.write().await;
            agents.insert(
                agent_id.clone(),
                RegisteredAgent {
                    definition: agent,
                    executor: Some(executor),
                    endpoint: None,
                    registered_at: Instant::now(),
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

        info!("Registered local agent: {}", agent_id);
        Ok(())
    }

    /// Execute agent locally
    async fn execute_local(&self, agent_id: &str, input: &[u8]) -> Result<Vec<u8>, Status> {
        let agents = self.agents.read().await;
        let agent = agents
            .get(agent_id)
            .ok_or_else(|| Status::not_found(format!("Agent not found: {}", agent_id)))?;

        let executor = agent
            .executor
            .as_ref()
            .ok_or_else(|| Status::failed_precondition("Agent has no local executor"))?;

        executor(input).map_err(|e| Status::internal(format!("Agent execution failed: {}", e)))
    }
}

impl Default for AgentServiceImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[tonic::async_trait]
impl AgentService for AgentServiceImpl {
    async fn register(
        &self,
        request: Request<RegisterAgentRequest>,
    ) -> Result<Response<RegisterAgentResponse>, Status> {
        let req = request.into_inner();
        let agent = req
            .agent
            .ok_or_else(|| Status::invalid_argument("Agent definition required"))?;

        let agent_id = agent.id.clone();
        let capabilities = agent.capabilities.clone();

        // Store agent
        {
            let mut agents = self.agents.write().await;
            agents.insert(
                agent_id.clone(),
                RegisteredAgent {
                    definition: agent,
                    executor: None, // Remote agents don't have local executors
                    endpoint: if req.endpoint.is_empty() {
                        None
                    } else {
                        Some(req.endpoint)
                    },
                    registered_at: Instant::now(),
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

        info!("Registered agent via gRPC: {}", agent_id);

        Ok(Response::new(RegisterAgentResponse {
            success: true,
            agent_id,
            error: String::new(),
        }))
    }

    async fn unregister(
        &self,
        request: Request<UnregisterAgentRequest>,
    ) -> Result<Response<UnregisterAgentResponse>, Status> {
        let req = request.into_inner();

        let removed = {
            let mut agents = self.agents.write().await;
            agents.remove(&req.agent_id)
        };

        if let Some(agent) = removed {
            // Remove from capability index
            let mut index = self.capability_index.write().await;
            for cap in &agent.definition.capabilities {
                if let Some(agents) = index.get_mut(cap) {
                    agents.retain(|id| id != &req.agent_id);
                }
            }

            info!("Unregistered agent: {}", req.agent_id);

            Ok(Response::new(UnregisterAgentResponse {
                success: true,
                removed_agent: Some(agent.definition),
            }))
        } else {
            Ok(Response::new(UnregisterAgentResponse {
                success: false,
                removed_agent: None,
            }))
        }
    }

    async fn execute(
        &self,
        request: Request<ExecuteAgentRequest>,
    ) -> Result<Response<ExecuteAgentResponse>, Status> {
        let req = request.into_inner();
        let start = Instant::now();

        debug!("Executing agent: {}", req.agent_id);

        match self.execute_local(&req.agent_id, &req.input).await {
            Ok(output) => {
                let latency_ms = start.elapsed().as_millis() as u64;
                Ok(Response::new(ExecuteAgentResponse {
                    output,
                    latency_ms,
                    success: true,
                    error: String::new(),
                    metadata: HashMap::new(),
                }))
            }
            Err(e) => Ok(Response::new(ExecuteAgentResponse {
                output: Vec::new(),
                latency_ms: start.elapsed().as_millis() as u64,
                success: false,
                error: e.message().to_string(),
                metadata: HashMap::new(),
            })),
        }
    }

    type ExecuteStreamStream =
        tokio_stream::wrappers::ReceiverStream<Result<ExecuteAgentChunk, Status>>;

    async fn execute_stream(
        &self,
        request: Request<ExecuteAgentRequest>,
    ) -> Result<Response<Self::ExecuteStreamStream>, Status> {
        let req = request.into_inner();
        let (tx, rx) = tokio::sync::mpsc::channel(32);

        let agents = self.agents.clone();
        let agent_id = req.agent_id.clone();
        let input = req.input;

        tokio::spawn(async move {
            let agents_guard = agents.read().await;
            if let Some(agent) = agents_guard.get(&agent_id) {
                if let Some(executor) = &agent.executor {
                    match executor(&input) {
                        Ok(output) => {
                            // Send output in chunks
                            let chunk_size = 64 * 1024; // 64KB chunks

                            for (sequence, chunk) in output.chunks(chunk_size).enumerate() {
                                let sequence = sequence as u64;
                                let is_final = sequence * chunk_size as u64 + chunk.len() as u64
                                    >= output.len() as u64;

                                let _ = tx
                                    .send(Ok(ExecuteAgentChunk {
                                        chunk: chunk.to_vec(),
                                        is_final,
                                    }))
                                    .await;
                            }
                        }
                        Err(e) => {
                            warn!("Agent {} execution failed: {}", agent_id, e);
                        }
                    }
                }
            }
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            rx,
        )))
    }

    async fn get_agent(
        &self,
        request: Request<GetAgentRequest>,
    ) -> Result<Response<Agent>, Status> {
        let req = request.into_inner();
        let agents = self.agents.read().await;

        agents
            .get(&req.agent_id)
            .map(|a| Response::new(a.definition.clone()))
            .ok_or_else(|| Status::not_found(format!("Agent not found: {}", req.agent_id)))
    }

    async fn list_agents(
        &self,
        request: Request<ListAgentsRequest>,
    ) -> Result<Response<ListAgentsResponse>, Status> {
        let req = request.into_inner();
        let agents = self.agents.read().await;

        let agent_list: Vec<Agent> = agents
            .values()
            .filter(|a| !req.enabled_only || a.definition.enabled)
            .map(|a| a.definition.clone())
            .collect();

        Ok(Response::new(ListAgentsResponse { agents: agent_list }))
    }

    async fn find_by_capability(
        &self,
        request: Request<FindByCapabilityRequest>,
    ) -> Result<Response<FindByCapabilityResponse>, Status> {
        let req = request.into_inner();
        let index = self.capability_index.read().await;
        let agents = self.agents.read().await;

        let matching_ids: Vec<String> = if req.match_all {
            // Agent must have ALL requested capabilities
            let mut sets: Vec<std::collections::HashSet<&String>> = Vec::new();
            for cap in &req.capabilities {
                if let Some(ids) = index.get(cap) {
                    sets.push(ids.iter().collect());
                } else {
                    // Capability not found, no agents match
                    return Ok(Response::new(FindByCapabilityResponse {
                        agents: Vec::new(),
                    }));
                }
            }

            if sets.is_empty() {
                Vec::new()
            } else {
                let first = sets.remove(0);
                first
                    .into_iter()
                    .filter(|id| sets.iter().all(|s| s.contains(id)))
                    .cloned()
                    .collect()
            }
        } else {
            // Agent can have ANY of the requested capabilities
            let mut seen = std::collections::HashSet::new();
            let mut result = Vec::new();
            for cap in &req.capabilities {
                if let Some(ids) = index.get(cap) {
                    for id in ids {
                        if seen.insert(id.clone()) {
                            result.push(id.clone());
                        }
                    }
                }
            }
            result
        };

        let matching_agents: Vec<Agent> = matching_ids
            .iter()
            .filter_map(|id| agents.get(id).map(|a| a.definition.clone()))
            .filter(|a| a.enabled)
            .collect();

        Ok(Response::new(FindByCapabilityResponse {
            agents: matching_agents,
        }))
    }

    async fn list_capabilities(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<ListCapabilitiesResponse>, Status> {
        let index = self.capability_index.read().await;

        let capabilities: Vec<i32> = index.keys().copied().collect();
        let capability_agent_count: HashMap<i32, i32> = index
            .iter()
            .map(|(cap, agents)| (*cap, agents.len() as i32))
            .collect();

        Ok(Response::new(ListCapabilitiesResponse {
            capabilities,
            capability_agent_count,
        }))
    }

    async fn health_check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        let req = request.into_inner();
        let agents = self.agents.read().await;

        if let Some(agent) = agents.get(&req.agent_id) {
            let uptime = agent.registered_at.elapsed().as_secs();
            Ok(Response::new(HealthCheckResponse {
                healthy: agent.definition.enabled,
                status: if agent.definition.enabled {
                    "healthy".to_string()
                } else {
                    "disabled".to_string()
                },
                uptime_seconds: uptime,
            }))
        } else {
            Ok(Response::new(HealthCheckResponse {
                healthy: false,
                status: "not_found".to_string(),
                uptime_seconds: 0,
            }))
        }
    }
}
