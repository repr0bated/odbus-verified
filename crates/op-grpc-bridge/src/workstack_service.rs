//! Authenticated WorkstackService facade on the existing bridge door.
//!
//! Committed definitions are the `workflows` runtime document's `workstacks`
//! array, deserialized as op-chat `Workstack` values (including phases, tools,
//! variables and rollback). No builtins or workflow summary fixtures are used.
//! `list_workstacks(category)` returns protobuf summaries: id, name, description,
//! category, required_agents, phase_count. An absent document yields an empty
//! list; Execute reports Unavailable. Unreadable/malformed documents fail closed.
//! Agent phase bindings are lowered to scoped catalog tool calls; the legacy
//! unauthenticated agent pool is never used. Events are progress notifications;
//! terminal episodes are separately committed through MutationEngine.

use crate::mutation_engine::MutationEngine;
use op_chat::orchestration::proto::op_chat_orchestration::workstack_service_server::WorkstackService;
use op_chat::orchestration::proto::op_chat_orchestration::*;
use op_chat::orchestration::services::OrchestrationServer;
use op_chat::orchestration::{
    AgentPoolConfig, ErrorCode, ExecutionControl, GrpcAgentPool, OrchestrationError, PhaseToolCall,
    ScopeFuture, ScopeVerifier, ToolExecutor, VerifiedScope, Workstack, WorkstackEvent,
    WorkstackEventSink, WorkstackExecutor,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tonic::{Request, Response, Status};

fn failure(code: ErrorCode, message: impl Into<String>) -> OrchestrationError {
    OrchestrationError::new(code, message)
}

struct BridgeVerifier(Arc<MutationEngine>);
impl ScopeVerifier for BridgeVerifier {
    fn verify<'a>(&'a self, session: &'a str) -> ScopeFuture<'a, VerifiedScope> {
        Box::pin(async move {
            if invalid_session(session) {
                return Err(failure(
                    ErrorCode::SessionInvalid,
                    "verified session required",
                ));
            }
            let scope = crate::agent_runs::agent_scope(&self.0, session)
                .await
                .map_err(|_| failure(ErrorCode::SessionInvalid, "session verification failed"))?
                .ok_or_else(|| failure(ErrorCode::SessionInvalid, "session is not verified"))?;
            Ok(VerifiedScope {
                principal_id: scope.principal_id,
                session_id: scope.session_id,
                session_genesis: scope.session_genesis,
            })
        })
    }
    fn authorize_control<'a>(
        &'a self,
        caller: &'a VerifiedScope,
        owner: &'a VerifiedScope,
        _operation: ExecutionControl,
    ) -> ScopeFuture<'a, ()> {
        Box::pin(async move {
            if caller == owner {
                return Ok(());
            }
            // No sealed stronger cross-owner workstack control contract exists.
            // workflows.write is ordinary workflow admission, not that grant.
            Err(failure(
                ErrorCode::PermissionDenied,
                "sealed cross-owner workstack control capability is unavailable",
            ))
        })
    }
}

struct BridgeTools(Arc<MutationEngine>);
impl ToolExecutor for BridgeTools {
    fn execute<'a>(
        &'a self,
        _tool: &'a str,
        _arguments: &'a simd_json::OwnedValue,
    ) -> ScopeFuture<'a, simd_json::OwnedValue> {
        Box::pin(async {
            Err(failure(
                ErrorCode::SessionInvalid,
                "unscoped dispatch refused",
            ))
        })
    }
    fn execute_scoped<'a>(
        &'a self,
        scope: &'a VerifiedScope,
        tool: &'a str,
        arguments: &'a simd_json::OwnedValue,
    ) -> ScopeFuture<'a, simd_json::OwnedValue> {
        Box::pin(async move {
            let (agent, operation) = tool.split_once('.').ok_or_else(|| {
                failure(ErrorCode::InvalidArguments, "step must be agent.operation")
            })?;
            let catalog =
                op_plugins::state_plugins::cognitive_agents::catalog_agent_method(agent, operation)
                    .ok_or_else(|| {
                        failure(
                            ErrorCode::OperationNotSupported,
                            "step is not a sealed catalog method",
                        )
                    })?;
            let name =
                op_plugins::state_plugins::cognitive_agents::agent_method_name(agent, operation);
            let args = serde_json::to_string(arguments)
                .map_err(|e| failure(ErrorCode::Serialization, e.to_string()))?;
            let result = self
                .0
                .dispatch_method_call_with_identity(
                    "cognitive",
                    &name,
                    &args,
                    catalog.declaration.required_capability.as_deref(),
                    &scope.principal_id,
                    Some(&scope.session_id),
                    Some(&scope.session_genesis),
                )
                .await
                .map_err(|e| failure(ErrorCode::ExecutionFailed, e.to_string()))?;
            serde_json::from_value(result.get("result").cloned().unwrap_or(result))
                .map_err(|e| failure(ErrorCode::Serialization, e.to_string()))
        })
    }
}

#[derive(Clone)]
struct RunOwner {
    scope: VerifiedScope,
    workstack: String,
    started: chrono::DateTime<chrono::Utc>,
}
struct EpisodeSink {
    engine: Arc<MutationEngine>,
    owners: Arc<Mutex<HashMap<String, RunOwner>>>,
}
impl WorkstackEventSink for EpisodeSink {
    fn on_event(&self, scope: &VerifiedScope, id: &str, event: &WorkstackEvent) {
        if let WorkstackEvent::Started { workstack_id, .. } = event {
            self.owners.lock().expect("run owners").insert(
                id.into(),
                RunOwner {
                    scope: scope.clone(),
                    workstack: workstack_id.clone(),
                    started: chrono::Utc::now(),
                },
            );
        }
        if let WorkstackEvent::Completed {
            success, duration, ..
        } = event
        {
            let Some(owner) = self.owners.lock().expect("run owners").get(id).cloned() else {
                return;
            };
            let engine = self.engine.clone();
            let id = id.to_string();
            let success = *success;
            let duration_ms = duration.as_millis() as u64;
            tokio::spawn(async move {
                use op_plugins::state_plugins::ctl_plane_chatbot::{RunRecord, RunStatus};
                use op_plugins::state_plugins::ctl_plane_chatbot_methods::{
                    CompletedAgentRun, build_completed_agent_run,
                };
                let row = build_completed_agent_run(CompletedAgentRun {
                    scope: op_cozo_store::EpisodeScope {
                        principal_id: owner.scope.principal_id,
                        session_id: owner.scope.session_id,
                        session_genesis: owner.scope.session_genesis,
                    },
                    trigger_id: id.clone(),
                    goal_text: owner.workstack,
                    summary: if success {
                        "Workstack completed"
                    } else {
                        "Workstack failed or cancelled"
                    }
                    .into(),
                    tools: vec![],
                    started_at: owner.started,
                    ended_at: chrono::Utc::now(),
                    run: RunRecord {
                        status: if success {
                            RunStatus::Succeeded
                        } else {
                            RunStatus::Failed
                        },
                        trigger_id: id.clone(),
                        topic_ref: blake3::hash(id.as_bytes()).to_hex().to_string(),
                        queued_ms: 0,
                        duration_ms,
                        tokens_in: 0,
                        tokens_out: 0,
                        actions: vec![],
                        reason: None,
                    },
                });
                match row {
                    Ok(row) => {
                        if let Err(error) = engine.commit_chatbot_episode(row).await {
                            tracing::warn!(execution_id = %id, %error, "workstack episode commit failed");
                        }
                    }
                    Err(error) => {
                        tracing::warn!(execution_id = %id, %error, "workstack episode invalid")
                    }
                }
            });
        }
    }
}

struct Facade {
    verifier: Arc<BridgeVerifier>,
    server: OrchestrationServer,
    owners: Arc<Mutex<HashMap<String, RunOwner>>>,
}
static FACADE: OnceLock<Arc<Facade>> = OnceLock::new();
pub(crate) fn start_once(engine: Arc<MutationEngine>) {
    FACADE.get_or_init(|| {
        let verifier = Arc::new(BridgeVerifier(engine.clone()));
        let owners = Arc::new(Mutex::new(HashMap::new()));
        let pool = Arc::new(GrpcAgentPool::new(AgentPoolConfig::default()));
        let executor = Arc::new(
            WorkstackExecutor::new(pool.clone(), Arc::new(BridgeTools(engine.clone())))
                .with_scope_verifier(verifier.clone())
                .with_event_sink(Arc::new(EpisodeSink {
                    engine,
                    owners: owners.clone(),
                })),
        );
        Arc::new(Facade {
            verifier,
            server: OrchestrationServer::new(pool, executor),
            owners,
        })
    });
}
fn facade() -> Result<Arc<Facade>, Status> {
    FACADE
        .get()
        .cloned()
        .ok_or_else(|| Status::unavailable("workstack facade has not started"))
}
fn invalid_session(session: &str) -> bool {
    session.trim().is_empty()
        || matches!(
            session.trim().to_ascii_lowercase().as_str(),
            "default" | "anonymous"
        )
}
impl Facade {
    async fn caller<T>(&self, request: &Request<T>) -> Result<VerifiedScope, Status> {
        let identity = crate::interceptor::bridge_capability_identity(request.extensions())
            .ok_or_else(|| Status::unauthenticated("verified interceptor identity required"))?;
        let scope = self
            .verifier
            .verify(&identity.session_id)
            .await
            .map_err(Status::from)?;
        if scope.principal_id != identity.principal_id
            || scope.session_genesis != identity.session_genesis
        {
            return Err(Status::unauthenticated(
                "interceptor identity does not match verified session",
            ));
        }
        Ok(scope)
    }
    fn owner(&self, id: &str) -> Result<RunOwner, Status> {
        self.owners
            .lock()
            .expect("run owners")
            .get(id)
            .cloned()
            .ok_or_else(|| Status::not_found("execution not found"))
    }
}
async fn definitions() -> Result<Vec<Workstack>, Status> {
    let documents = op_plugins::state_plugins::runtime_documents::runtime_documents()
        .await
        .map_err(|_| Status::unavailable("committed workstack documents unavailable"))?;
    let Some(row) = documents
        .read("workflows")
        .await
        .map_err(|_| Status::unavailable("committed workstack document unreadable"))?
    else {
        return Ok(vec![]);
    };
    let Some(defs) = row.document.get("workstacks") else {
        return Ok(vec![]);
    };
    serde_json::from_value(defs.clone())
        .map_err(|_| Status::unavailable("committed workstacks are malformed"))
}
fn lower_agents(mut definition: Workstack) -> Result<Workstack, Status> {
    for phase in &mut definition.phases {
        if !phase.agents.is_empty() {
            let operation = phase
                .agent_operation
                .as_deref()
                .ok_or_else(|| Status::unavailable("committed agent phase has no operation"))?;
            for agent in phase.agents.drain(..) {
                phase.tools.push(PhaseToolCall {
                    tool: format!("{agent}.{operation}"),
                    arguments: phase
                        .agent_arguments
                        .clone()
                        .unwrap_or_else(|| simd_json::json!({})),
                    store_as: None,
                    retries: 0,
                });
            }
        }
    }
    Ok(definition)
}

#[derive(Clone, Default)]
pub(crate) struct WorkstackServiceImpl;
impl WorkstackServiceImpl {
    pub(crate) async fn list_workstacks(
        &self,
        category: &str,
    ) -> Result<ListWorkstacksResponse, Status> {
        Ok(ListWorkstacksResponse {
            workstacks: definitions()
                .await?
                .into_iter()
                .filter(|ws| category.is_empty() || ws.category.as_deref() == Some(category))
                .map(|ws| WorkstackInfo {
                    id: ws.id,
                    name: ws.name,
                    description: ws.description,
                    category: ws.category.unwrap_or_default(),
                    required_agents: ws.required_agents,
                    phase_count: ws.phases.len() as i32,
                })
                .collect(),
        })
    }
}
#[tonic::async_trait]
impl WorkstackService for WorkstackServiceImpl {
    type ExecuteStream = <OrchestrationServer as WorkstackService>::ExecuteStream;
    async fn execute(
        &self,
        request: Request<WorkstackExecuteRequest>,
    ) -> Result<Response<Self::ExecuteStream>, Status> {
        if invalid_session(&request.get_ref().session_id) {
            return Err(Status::unauthenticated(
                "explicit verified session required",
            ));
        }
        let facade = facade()?;
        let caller = facade.caller(&request).await?;
        if caller.session_id != request.get_ref().session_id {
            return Err(Status::unauthenticated(
                "request session differs from caller",
            ));
        }
        if request.get_ref().dry_run {
            return Err(Status::unimplemented("dry run is not supported"));
        }
        let definitions = definitions().await?;
        if definitions.is_empty() {
            return Err(Status::unavailable("no committed workstack definitions"));
        }
        let definition = definitions
            .into_iter()
            .find(|ws| ws.id == request.get_ref().workstack_id)
            .ok_or_else(|| Status::not_found("committed workstack not found"))?;
        facade
            .server
            .workstack_executor
            .register(lower_agents(definition)?)
            .await;
        facade.server.execute(request).await
    }
    async fn get_status(
        &self,
        request: Request<WorkstackStatusRequest>,
    ) -> Result<Response<WorkstackStatusResponse>, Status> {
        let facade = facade()?;
        let caller = facade.caller(&request).await?;
        let owner = facade.owner(&request.get_ref().execution_id)?;
        if caller != owner.scope {
            return Err(Status::permission_denied(
                "execution belongs to another scope",
            ));
        }
        let mut response = facade.server.get_status(request).await?;
        response.get_mut().workstack_id = owner.workstack;
        response.get_mut().started_at_ms = owner.started.timestamp_millis();
        Ok(response)
    }
    async fn cancel(
        &self,
        request: Request<WorkstackCancelRequest>,
    ) -> Result<Response<WorkstackCancelResponse>, Status> {
        let facade = facade()?;
        let caller = facade.caller(&request).await?;
        let req = request.into_inner();
        let owner = facade.owner(&req.execution_id)?;
        facade
            .verifier
            .authorize_control(&caller, &owner.scope, ExecutionControl::Cancel)
            .await
            .map_err(Status::from)?;
        if req.rollback {
            return Err(Status::unimplemented(
                "cancel with rollback is not supported by the executor",
            ));
        }
        let success = facade
            .server
            .workstack_executor
            .cancel_scoped(&caller.session_id, &req.execution_id)
            .await
            .map_err(Status::from)?;
        Ok(Response::new(WorkstackCancelResponse {
            success,
            will_rollback: false,
        }))
    }
    async fn rollback(
        &self,
        request: Request<WorkstackRollbackRequest>,
    ) -> Result<Response<WorkstackRollbackResponse>, Status> {
        let facade = facade()?;
        let caller = facade.caller(&request).await?;
        let owner = facade.owner(&request.get_ref().execution_id)?;
        facade
            .verifier
            .authorize_control(&caller, &owner.scope, ExecutionControl::Rollback)
            .await
            .map_err(Status::from)?;
        Err(Status::unimplemented(
            "explicit rollback is not supported by the executor; failure rollback remains automatic",
        ))
    }
    async fn list(
        &self,
        request: Request<ListWorkstacksRequest>,
    ) -> Result<Response<ListWorkstacksResponse>, Status> {
        facade()?.caller(&request).await?;
        Ok(Response::new(
            self.list_workstacks(&request.get_ref().category).await?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn workstack_service_cross_owner_control_fails_closed() {
        let verifier = BridgeVerifier(crate::human_principal_dispatch::tests::test_engine());
        let caller = VerifiedScope {
            principal_id: "principal".into(),
            session_id: "session".into(),
            session_genesis: "genesis".into(),
        };
        let mut owner = caller.clone();
        owner.session_genesis = "previous-genesis".into();
        for operation in [ExecutionControl::Cancel, ExecutionControl::Rollback] {
            assert!(
                verifier
                    .authorize_control(&caller, &caller, operation)
                    .await
                    .is_ok()
            );
            assert_eq!(
                verifier
                    .authorize_control(&caller, &owner, operation)
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::PermissionDenied
            );
        }
    }

    #[tokio::test]
    async fn workstack_service_rejects_anonymous_before_registry_access() {
        for session in ["", " ", "default", "anonymous", "DEFAULT"] {
            let result = WorkstackServiceImpl
                .execute(Request::new(WorkstackExecuteRequest {
                    session_id: session.into(),
                    ..Default::default()
                }))
                .await;
            assert_eq!(result.err().unwrap().code(), tonic::Code::Unauthenticated);
        }
    }
    #[test]
    fn workstack_service_lowers_agent_phases_without_unscoped_pool() {
        let mut ws = Workstack::new("test", "test", "fixture");
        ws.phases.push(op_chat::orchestration::WorkstackPhase {
            agents: vec!["rust-pro".into()],
            agent_operation: Some("check".into()),
            ..Default::default()
        });
        let ws = lower_agents(ws).unwrap();
        assert!(ws.phases[0].agents.is_empty());
        assert_eq!(ws.phases[0].tools[0].tool, "rust-pro.check");
    }
}
