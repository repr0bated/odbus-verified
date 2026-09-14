//! AgentLifecycle gRPC service implementation.

use std::pin::Pin;

use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use crate::orchestration::proto::op_chat_orchestration::{
    agent_lifecycle_server::AgentLifecycle, AgentInfo, AgentStatus, AgentStatusEvent,
    EndSessionRequest, EndSessionResponse, HealthCheckRequest, HealthCheckResponse,
    ShutdownRequest, ShutdownResponse, StartSessionRequest, StartSessionResponse,
    WatchAgentsRequest,
};

use super::{OrchestrationServer, SessionInfo};

#[tonic::async_trait]
impl AgentLifecycle for OrchestrationServer {
    /// Start a session: delegates to pool.init_session(), records session info,
    /// and returns the list of started agents.
    async fn start_session(
        &self,
        request: Request<StartSessionRequest>,
    ) -> Result<Response<StartSessionResponse>, Status> {
        let req = request.into_inner();
        let session_id = if req.session_id.is_empty() {
            uuid::Uuid::new_v4().to_string()
        } else {
            req.session_id.clone()
        };

        info!(session_id = %session_id, client = %req.client_name, "StartSession");

        let metadata_map: Option<std::collections::HashMap<String, String>> =
            if req.metadata.is_empty() {
                None
            } else {
                Some(req.metadata.clone())
            };

        match self
            .pool
            .init_session(&session_id, &req.client_name, metadata_map)
            .await
        {
            Ok(started_agents) => {
                let now_ms = chrono::Utc::now().timestamp_millis();

                // Build AgentInfo list
                let agent_infos: Vec<AgentInfo> = started_agents
                    .iter()
                    .map(|agent_id| AgentInfo {
                        agent_id: agent_id.clone(),
                        agent_type: agent_id.clone(),
                        status: AgentStatus::Running.into(),
                        priority: 0,
                        operations: vec![],
                        started_at_ms: now_ms,
                    })
                    .collect();

                // Record session locally
                {
                    let info = SessionInfo {
                        session_id: session_id.clone(),
                        client_name: req.client_name.clone(),
                        started_at: now_ms,
                        agent_ids: started_agents.clone(),
                        metadata: req.metadata.clone(),
                    };
                    self.sessions.write().await.insert(session_id.clone(), info);
                }

                // Broadcast agent started events
                for agent_id in &started_agents {
                    let _ = self.agent_events.send(AgentStatusEvent {
                        agent_id: agent_id.clone(),
                        status: AgentStatus::Running.into(),
                        timestamp_ms: now_ms,
                        message: "Agent started".to_string(),
                        metadata: Default::default(),
                    });
                }

                Ok(Response::new(StartSessionResponse {
                    success: true,
                    session_id,
                    started_agents: agent_infos,
                    failed_agents: vec![],
                    started_at_ms: now_ms,
                }))
            }
            Err(e) => {
                warn!(error = %e, "StartSession failed");
                Err(Status::internal(format!("Failed to start session: {}", e)))
            }
        }
    }

    /// End a session: delegates to pool.shutdown_session(), removes local state.
    async fn end_session(
        &self,
        request: Request<EndSessionRequest>,
    ) -> Result<Response<EndSessionResponse>, Status> {
        let req = request.into_inner();
        info!(session_id = %req.session_id, "EndSession");

        // Get the session info before removing
        let session_info = self.sessions.write().await.remove(&req.session_id);

        match self.pool.shutdown_session(&req.session_id).await {
            Ok(duration) => {
                let stopped_agents = session_info.map(|s| s.agent_ids).unwrap_or_default();

                // Broadcast agent stopped events
                let now_ms = chrono::Utc::now().timestamp_millis();
                for agent_id in &stopped_agents {
                    let _ = self.agent_events.send(AgentStatusEvent {
                        agent_id: agent_id.clone(),
                        status: AgentStatus::Stopped.into(),
                        timestamp_ms: now_ms,
                        message: "Session ended".to_string(),
                        metadata: Default::default(),
                    });
                }

                Ok(Response::new(EndSessionResponse {
                    success: true,
                    stopped_agents,
                    session_duration_ms: duration.as_millis() as i64,
                }))
            }
            Err(e) => {
                warn!(error = %e, "EndSession failed");
                Err(Status::internal(format!("Failed to end session: {}", e)))
            }
        }
    }

    /// Health check: queries the pool for agent connection health.
    async fn health_check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        let req = request.into_inner();

        match self.pool.health_check(&req.agent_id).await {
            Ok(health) => {
                let status = if health.connected {
                    AgentStatus::Running
                } else {
                    AgentStatus::Stopped
                };

                let mut metrics = std::collections::HashMap::new();
                if req.include_metrics {
                    metrics.insert(
                        "request_count".to_string(),
                        health.request_count.to_string(),
                    );
                    metrics.insert("error_count".to_string(), health.error_count.to_string());
                    metrics.insert(
                        "circuit_state".to_string(),
                        format!("{:?}", health.circuit_state),
                    );
                }

                Ok(Response::new(HealthCheckResponse {
                    healthy: health.connected,
                    agent_id: health.agent_id,
                    status: status.into(),
                    uptime_ms: health.uptime.map(|d| d.as_millis() as i64).unwrap_or(0),
                    metrics,
                    last_error: String::new(),
                }))
            }
            Err(e) => Ok(Response::new(HealthCheckResponse {
                healthy: false,
                agent_id: req.agent_id,
                status: AgentStatus::Error.into(),
                uptime_ms: 0,
                metrics: Default::default(),
                last_error: e.to_string(),
            })),
        }
    }

    // -- streaming -----------------------------------------------------------

    type WatchAgentsStream =
        Pin<Box<dyn Stream<Item = Result<AgentStatusEvent, Status>> + Send + 'static>>;

    /// Watch agent status changes via a server-streaming response.
    ///
    /// Subscribes to the shared `agent_events` broadcast channel and filters
    /// events by session/agent IDs specified in the request.
    async fn watch_agents(
        &self,
        request: Request<WatchAgentsRequest>,
    ) -> Result<Response<Self::WatchAgentsStream>, Status> {
        let req = request.into_inner();
        let mut rx = self.agent_events.subscribe();
        let filter_agents: Vec<String> = req.agent_ids;
        let include_heartbeats = req.include_heartbeats;

        let (tx, stream_rx) = tokio::sync::mpsc::channel(64);

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        // Filter by agent ID if requested
                        if !filter_agents.is_empty() && !filter_agents.contains(&event.agent_id) {
                            continue;
                        }

                        // Skip heartbeats if not requested
                        if !include_heartbeats && event.message.to_lowercase().contains("heartbeat")
                        {
                            continue;
                        }

                        if tx.send(Ok(event)).await.is_err() {
                            // Client disconnected
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(lagged = n, "WatchAgents broadcast lagged");
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        });

        let stream = tokio_stream::wrappers::ReceiverStream::new(stream_rx);
        Ok(Response::new(Box::pin(stream) as Self::WatchAgentsStream))
    }

    /// Graceful shutdown: iterate all tracked sessions and shut each one down.
    async fn shutdown(
        &self,
        request: Request<ShutdownRequest>,
    ) -> Result<Response<ShutdownResponse>, Status> {
        let req = request.into_inner();
        info!(force = req.force, "Shutdown requested");

        let session_ids: Vec<String> = { self.sessions.read().await.keys().cloned().collect() };

        let mut agents_stopped: i32 = 0;
        let mut errors: Vec<String> = Vec::new();

        for session_id in &session_ids {
            // Remove session locally
            let session_info = self.sessions.write().await.remove(session_id);

            match self.pool.shutdown_session(session_id).await {
                Ok(_) => {
                    if let Some(info) = session_info {
                        agents_stopped += info.agent_ids.len() as i32;
                    }
                }
                Err(e) => {
                    errors.push(format!("Session {}: {}", session_id, e));
                }
            }
        }

        // Broadcast shutdown event
        let now_ms = chrono::Utc::now().timestamp_millis();
        let _ = self.agent_events.send(AgentStatusEvent {
            agent_id: "system".to_string(),
            status: AgentStatus::Stopped.into(),
            timestamp_ms: now_ms,
            message: "System shutdown".to_string(),
            metadata: Default::default(),
        });

        Ok(Response::new(ShutdownResponse {
            success: errors.is_empty(),
            agents_stopped,
            errors,
        }))
    }
}
