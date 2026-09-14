//! WorkstackService gRPC implementation
//!
//! Manages workstack execution, status queries, cancellation,
//! rollback, and listing of registered workstacks.

use std::pin::Pin;

use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{debug, info, warn};

use crate::orchestration::proto::op_chat_orchestration::WorkstackInfo as ProtoWorkstackInfo;
use crate::orchestration::proto::op_chat_orchestration::{
    workstack_service_server::WorkstackService, ListWorkstacksRequest, ListWorkstacksResponse,
    WorkstackCancelRequest, WorkstackCancelResponse, WorkstackEvent as ProtoWorkstackEvent,
    WorkstackEventType, WorkstackExecuteRequest, WorkstackExecutionStatus, WorkstackProgress,
    WorkstackRollbackRequest, WorkstackRollbackResponse, WorkstackStatusRequest,
    WorkstackStatusResponse,
};
use crate::orchestration::workstack_executor::{ExecutionStatus, WorkstackEvent as InternalEvent};

use super::OrchestrationServer;

/// Convert internal WorkstackEvent to proto WorkstackEvent
fn to_proto_event(event: &InternalEvent) -> ProtoWorkstackEvent {
    let now_ms = chrono::Utc::now().timestamp_millis();

    match event {
        InternalEvent::Started {
            workstack_id,
            execution_id,
            total_phases,
        } => ProtoWorkstackEvent {
            workstack_id: workstack_id.clone(),
            execution_id: execution_id.clone(),
            event_type: WorkstackEventType::WorkstackEventStarted as i32,
            phase_id: String::new(),
            message: format!("Workstack started with {} phases", total_phases),
            details_json: String::new(),
            timestamp_ms: now_ms,
            progress: Some(WorkstackProgress {
                current_phase: 0,
                total_phases: *total_phases as i32,
                current_phase_id: String::new(),
                percent_complete: 0.0,
            }),
        },
        InternalEvent::PhaseStarted {
            phase_id,
            phase_name,
            index,
            total,
        } => ProtoWorkstackEvent {
            workstack_id: String::new(),
            execution_id: String::new(),
            event_type: WorkstackEventType::WorkstackEventPhaseStarted as i32,
            phase_id: phase_id.clone(),
            message: format!("Phase started: {}", phase_name),
            details_json: String::new(),
            timestamp_ms: now_ms,
            progress: Some(WorkstackProgress {
                current_phase: *index as i32,
                total_phases: *total as i32,
                current_phase_id: phase_id.clone(),
                percent_complete: if *total > 0 {
                    (*index as f32 / *total as f32) * 100.0
                } else {
                    0.0
                },
            }),
        },
        InternalEvent::PhaseProgress {
            phase_id,
            message,
            progress,
        } => ProtoWorkstackEvent {
            workstack_id: String::new(),
            execution_id: String::new(),
            event_type: WorkstackEventType::WorkstackEventPhaseProgress as i32,
            phase_id: phase_id.clone(),
            message: message.clone(),
            details_json: String::new(),
            timestamp_ms: now_ms,
            progress: Some(WorkstackProgress {
                current_phase: 0,
                total_phases: 0,
                current_phase_id: phase_id.clone(),
                percent_complete: *progress,
            }),
        },
        InternalEvent::PhaseCompleted {
            phase_id,
            status,
            duration,
        } => {
            let event_type = match status {
                crate::orchestration::workstack_executor::PhaseStatus::Completed => {
                    WorkstackEventType::WorkstackEventPhaseCompleted
                }
                crate::orchestration::workstack_executor::PhaseStatus::Failed => {
                    WorkstackEventType::WorkstackEventPhaseFailed
                }
                crate::orchestration::workstack_executor::PhaseStatus::Skipped => {
                    WorkstackEventType::WorkstackEventPhaseSkipped
                }
                _ => WorkstackEventType::WorkstackEventPhaseCompleted,
            };
            ProtoWorkstackEvent {
                workstack_id: String::new(),
                execution_id: String::new(),
                event_type: event_type as i32,
                phase_id: phase_id.clone(),
                message: format!(
                    "Phase {}: {} ({}ms)",
                    phase_id,
                    status,
                    duration.as_millis()
                ),
                details_json: format!(
                    "{{\"duration_ms\": {}, \"status\": \"{}\"}}",
                    duration.as_millis(),
                    status
                ),
                timestamp_ms: now_ms,
                progress: None,
            }
        }
        InternalEvent::ToolOutput {
            phase_id,
            tool,
            chunk,
        } => ProtoWorkstackEvent {
            workstack_id: String::new(),
            execution_id: String::new(),
            event_type: WorkstackEventType::WorkstackEventPhaseProgress as i32,
            phase_id: phase_id.clone(),
            message: chunk.content.clone(),
            details_json: format!(
                "{{\"tool\": \"{}\", \"stream\": \"{}\"}}",
                tool, chunk.stream_type
            ),
            timestamp_ms: now_ms,
            progress: None,
        },
        InternalEvent::AgentOutput {
            phase_id,
            agent_id,
            chunk,
        } => ProtoWorkstackEvent {
            workstack_id: String::new(),
            execution_id: String::new(),
            event_type: WorkstackEventType::WorkstackEventPhaseProgress as i32,
            phase_id: phase_id.clone(),
            message: chunk.content.clone(),
            details_json: format!(
                "{{\"agent\": \"{}\", \"stream\": \"{}\"}}",
                agent_id, chunk.stream_type
            ),
            timestamp_ms: now_ms,
            progress: None,
        },
        InternalEvent::Completed {
            workstack_id,
            execution_id,
            success,
            duration,
        } => {
            let event_type = if *success {
                WorkstackEventType::WorkstackEventCompleted
            } else {
                WorkstackEventType::WorkstackEventFailed
            };
            ProtoWorkstackEvent {
                workstack_id: workstack_id.clone(),
                execution_id: execution_id.clone(),
                event_type: event_type as i32,
                phase_id: String::new(),
                message: format!(
                    "Workstack {}: {}ms",
                    if *success { "completed" } else { "failed" },
                    duration.as_millis()
                ),
                details_json: format!(
                    "{{\"success\": {}, \"duration_ms\": {}}}",
                    success,
                    duration.as_millis()
                ),
                timestamp_ms: now_ms,
                progress: Some(WorkstackProgress {
                    current_phase: 0,
                    total_phases: 0,
                    current_phase_id: String::new(),
                    percent_complete: if *success { 100.0 } else { 0.0 },
                }),
            }
        }
        InternalEvent::RollbackStarted { phase_id } => ProtoWorkstackEvent {
            workstack_id: String::new(),
            execution_id: String::new(),
            event_type: WorkstackEventType::WorkstackEventRollbackStarted as i32,
            phase_id: phase_id.clone(),
            message: format!("Rollback started for phase {}", phase_id),
            details_json: String::new(),
            timestamp_ms: now_ms,
            progress: None,
        },
        InternalEvent::RollbackCompleted { phase_id, success } => ProtoWorkstackEvent {
            workstack_id: String::new(),
            execution_id: String::new(),
            event_type: WorkstackEventType::WorkstackEventRollbackCompleted as i32,
            phase_id: phase_id.clone(),
            message: format!(
                "Rollback {}: {}",
                if *success { "succeeded" } else { "failed" },
                phase_id
            ),
            details_json: format!("{{\"success\": {}}}", success),
            timestamp_ms: now_ms,
            progress: None,
        },
        InternalEvent::Error { phase_id, error } => ProtoWorkstackEvent {
            workstack_id: String::new(),
            execution_id: String::new(),
            event_type: WorkstackEventType::WorkstackEventFailed as i32,
            phase_id: phase_id.clone().unwrap_or_default(),
            message: error.clone(),
            details_json: format!("{{\"error\": \"{}\"}}", error.replace('"', "\\\"")),
            timestamp_ms: now_ms,
            progress: None,
        },
    }
}

/// Convert internal ExecutionStatus to proto
fn to_proto_status(status: ExecutionStatus) -> i32 {
    match status {
        ExecutionStatus::Pending => WorkstackExecutionStatus::WorkstackExecutionPending as i32,
        ExecutionStatus::Running => WorkstackExecutionStatus::WorkstackExecutionRunning as i32,
        ExecutionStatus::Completed => WorkstackExecutionStatus::WorkstackExecutionCompleted as i32,
        ExecutionStatus::Failed => WorkstackExecutionStatus::WorkstackExecutionFailed as i32,
        ExecutionStatus::Cancelled => WorkstackExecutionStatus::WorkstackExecutionCancelled as i32,
        ExecutionStatus::RollingBack => {
            WorkstackExecutionStatus::WorkstackExecutionRollingBack as i32
        }
        ExecutionStatus::RolledBack => {
            WorkstackExecutionStatus::WorkstackExecutionRolledBack as i32
        }
    }
}

#[tonic::async_trait]
impl WorkstackService for OrchestrationServer {
    type ExecuteStream = Pin<
        Box<dyn tokio_stream::Stream<Item = Result<ProtoWorkstackEvent, Status>> + Send + 'static>,
    >;

    async fn execute(
        &self,
        request: Request<WorkstackExecuteRequest>,
    ) -> Result<Response<Self::ExecuteStream>, Status> {
        let req = request.into_inner();
        let session_id = if req.session_id.is_empty() {
            "default".to_string()
        } else {
            req.session_id.clone()
        };
        let workstack_id = req.workstack_id.clone();

        info!(
            workstack = %workstack_id,
            session = %session_id,
            dry_run = req.dry_run,
            "Executing workstack"
        );

        // Verify workstack exists
        if self.workstack_executor.get(&workstack_id).await.is_none() {
            return Err(Status::not_found(format!(
                "Workstack not found: {}",
                workstack_id
            )));
        }

        let (proto_tx, proto_rx) = tokio::sync::mpsc::channel(128);
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(128);

        let executor = self.workstack_executor.clone();
        let variables = req.variables;

        // Spawn event forwarder: reads internal events and converts to proto
        let forward_tx = proto_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                let proto_event = to_proto_event(&event);
                if forward_tx.send(Ok(proto_event)).await.is_err() {
                    break;
                }
            }
        });

        // Spawn executor in a Send-safe way: the WorkstackExecutor's internal
        // iterator is not Send across await points, so we use a helper task.
        {
            tokio::task::spawn_local(async move {
                let result = executor
                    .execute(&session_id, &workstack_id, variables, Some(event_tx))
                    .await;

                if let Err(e) = result {
                    let _ = proto_tx
                        .send(Ok(ProtoWorkstackEvent {
                            workstack_id: workstack_id.clone(),
                            execution_id: String::new(),
                            event_type: WorkstackEventType::WorkstackEventFailed as i32,
                            phase_id: String::new(),
                            message: format!("Execution error: {}", e),
                            details_json: String::new(),
                            timestamp_ms: chrono::Utc::now().timestamp_millis(),
                            progress: None,
                        }))
                        .await;
                }
            });
        }

        Ok(Response::new(Box::pin(ReceiverStream::new(proto_rx))))
    }

    async fn get_status(
        &self,
        request: Request<WorkstackStatusRequest>,
    ) -> Result<Response<WorkstackStatusResponse>, Status> {
        let req = request.into_inner();
        debug!(execution_id = %req.execution_id, "Getting workstack status");

        match self.workstack_executor.get_status(&req.execution_id).await {
            Some(status) => Ok(Response::new(WorkstackStatusResponse {
                found: true,
                execution_id: req.execution_id,
                workstack_id: String::new(),
                status: to_proto_status(status),
                progress: None,
                phases: vec![],
                started_at_ms: 0,
                completed_at_ms: 0,
            })),
            None => Ok(Response::new(WorkstackStatusResponse {
                found: false,
                execution_id: req.execution_id,
                workstack_id: String::new(),
                status: WorkstackExecutionStatus::WorkstackExecutionPending as i32,
                progress: None,
                phases: vec![],
                started_at_ms: 0,
                completed_at_ms: 0,
            })),
        }
    }

    async fn cancel(
        &self,
        request: Request<WorkstackCancelRequest>,
    ) -> Result<Response<WorkstackCancelResponse>, Status> {
        let req = request.into_inner();
        info!(execution_id = %req.execution_id, rollback = req.rollback, "Cancelling workstack");

        match self.workstack_executor.cancel(&req.execution_id).await {
            Ok(was_running) => Ok(Response::new(WorkstackCancelResponse {
                success: was_running,
                will_rollback: req.rollback && was_running,
            })),
            Err(e) => {
                warn!(error = %e, "Cancel failed");
                Ok(Response::new(WorkstackCancelResponse {
                    success: false,
                    will_rollback: false,
                }))
            }
        }
    }

    async fn rollback(
        &self,
        request: Request<WorkstackRollbackRequest>,
    ) -> Result<Response<WorkstackRollbackResponse>, Status> {
        let req = request.into_inner();
        info!(
            execution_id = %req.execution_id,
            rollback_to = %req.rollback_to_phase,
            "Rolling back workstack"
        );

        // The WorkstackExecutor handles rollback internally during execution.
        // For post-execution rollback, we mark the execution as rolling back.
        // This is a best-effort operation since the executor manages its own rollback.
        match self.workstack_executor.get_status(&req.execution_id).await {
            Some(status) => {
                if status == ExecutionStatus::Failed || status == ExecutionStatus::Completed {
                    // Cannot rollback completed/failed executions post-hoc without
                    // re-running. Return a graceful response.
                    Ok(Response::new(WorkstackRollbackResponse {
                        success: false,
                        rolled_back_phases: vec![],
                        errors: vec![format!(
                            "Cannot rollback execution in {:?} state. Rollback is only available during execution.",
                            status
                        )],
                    }))
                } else if status == ExecutionStatus::Running {
                    // Cancel first, which triggers internal rollback
                    let _ = self.workstack_executor.cancel(&req.execution_id).await;
                    Ok(Response::new(WorkstackRollbackResponse {
                        success: true,
                        rolled_back_phases: vec![],
                        errors: vec![],
                    }))
                } else {
                    Ok(Response::new(WorkstackRollbackResponse {
                        success: false,
                        rolled_back_phases: vec![],
                        errors: vec![format!("Execution is in {:?} state", status)],
                    }))
                }
            }
            None => Ok(Response::new(WorkstackRollbackResponse {
                success: false,
                rolled_back_phases: vec![],
                errors: vec![format!("Execution not found: {}", req.execution_id)],
            })),
        }
    }

    async fn list(
        &self,
        request: Request<ListWorkstacksRequest>,
    ) -> Result<Response<ListWorkstacksResponse>, Status> {
        let req = request.into_inner();
        debug!(category = %req.category, "Listing workstacks");

        let all_workstacks = self.workstack_executor.list().await;

        let filtered: Vec<ProtoWorkstackInfo> = all_workstacks
            .into_iter()
            .filter(|ws| {
                if req.category.is_empty() {
                    true
                } else {
                    ws.category.as_deref() == Some(&req.category)
                }
            })
            .map(|ws| ProtoWorkstackInfo {
                id: ws.id,
                name: ws.name,
                description: ws.description,
                category: ws.category.unwrap_or_default(),
                required_agents: ws.required_agents,
                phase_count: ws.phase_count as i32,
            })
            .collect();

        Ok(Response::new(ListWorkstacksResponse {
            workstacks: filtered,
        }))
    }
}
