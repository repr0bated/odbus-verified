//! Event-driven workflow execution owned by the bridge, under an exact verified
//! principal/session/genesis. Durable Cozo receipts precede dispatch. In-memory
//! state is bounded lifetime coordination, never a second result authority.
//! Pauses/cancellation stop subsequent steps; they cannot undo backend effects.
//! Lost bridge ownership is reported as interrupted and is never auto-replayed.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use op_cozo_store::{CozoGraphShuttle, EpisodeScope, WorkflowExecutionRecord};
use op_plugins::state_plugins::ctl_plane_chatbot::{RunAction, RunRecord, RunStatus};
use op_plugins::state_plugins::ctl_plane_chatbot_methods::{
    CompletedAgentRun, build_completed_agent_run,
};
use op_plugins::state_plugins::workflows_plugin::{
    self, CancelWorkflowInput, PauseWorkflowInput, QueryWorkflowsInput, QueryWorkflowsOutput,
    ResumeWorkflowInput, StartWorkflowInput, StartWorkflowOutput, WorkflowExecutionStatus,
    WorkflowExecutionSummary, WorkflowTransitionOutput,
};
use serde_json::Value;
use tokio::sync::{Mutex, Notify, OnceCell, Semaphore};

use crate::mutation_engine::MutationEngine;

const RUN_CAPACITY: usize = 64;
const RUN_CONCURRENCY: usize = 2;
const STEP_TIMEOUT: Duration = Duration::from_secs(120);
const STEP_NAME_MAX_BYTES: usize = 256;
const ARGS_MAX_BYTES: usize = 16 * 1024;
const MAX_STEPS: usize = 32;
const COGNITIVE_PLUGIN: &str = "cognitive_mcp";

struct Execution {
    record: Mutex<WorkflowExecutionRecord>,
    changed: Notify,
    unavailable: AtomicBool,
}

struct WorkstackRunner {
    engine: Arc<MutationEngine>,
    epoch: String,
    slots: Arc<Semaphore>,
    workers: Arc<Semaphore>,
    active: Mutex<HashMap<String, Arc<Execution>>>,
    store: OnceCell<Arc<CozoGraphShuttle>>,
}

impl WorkstackRunner {
    fn new(engine: Arc<MutationEngine>) -> Self {
        Self {
            engine,
            epoch: uuid::Uuid::new_v4().to_string(),
            slots: Arc::new(Semaphore::new(RUN_CAPACITY)),
            workers: Arc::new(Semaphore::new(RUN_CONCURRENCY)),
            active: Mutex::new(HashMap::new()),
            store: OnceCell::new(),
        }
    }

    async fn store(&self) -> Result<Arc<CozoGraphShuttle>> {
        self.store
            .get_or_try_init(|| async {
                let store = self.engine.ctl_plane_chatbot_episode_cozo().await?;
                let initialize = store.clone();
                tokio::task::spawn_blocking(move || {
                    initialize.initialize_workflow_execution_store()
                })
                .await??;
                Ok::<_, anyhow::Error>(store)
            })
            .await
            .cloned()
    }

    async fn persist(&self, next: WorkflowExecutionRecord, expected: Option<u64>) -> Result<()> {
        let store = self.store().await?;
        tokio::task::spawn_blocking(move || store.put_workflow_execution(&next, expected))
            .await??;
        Ok(())
    }

    /// Caller holds the execution lock across commit. No dispatch or wakeup
    /// follows an uncertain/failed durable write. Query rereads the Cozo row.
    async fn update(
        &self,
        row: &mut WorkflowExecutionRecord,
        status: &str,
        in_flight: bool,
        reason: Option<&str>,
    ) -> Result<()> {
        let mut next = row.clone();
        next.revision = next
            .revision
            .checked_add(1)
            .context("workflow revision overflow")?;
        next.status = status.into();
        next.document["in_flight"] = Value::Bool(in_flight);
        if let Some(reason) = reason {
            next.document["reason"] = Value::String(reason.into());
        }
        if let Err(error) = self.persist(next.clone(), Some(row.revision)).await {
            // A failed barrier can follow a committed write. Surrender live
            // ownership, stop subsequent dispatch, and retain the exact durable
            // phase for scoped diagnosis; never compensate or silently retry.
            if let Some(execution) = self.active.lock().await.remove(&row.execution_id) {
                execution.unavailable.store(true, Ordering::Release);
                execution.changed.notify_one();
            }
            return Err(error);
        }
        *row = next;
        Ok(())
    }

    async fn start(
        self: &Arc<Self>,
        input: StartWorkflowInput,
        scope: EpisodeScope,
    ) -> Result<Value> {
        anyhow::ensure!(
            input.workflow_id.len() <= 256
                && workflows_plugin::workflow_summaries()
                    .iter()
                    .any(|workflow| workflow.id == input.workflow_id),
            "unknown workflow"
        );
        let steps = input
            .params
            .as_ref()
            .and_then(|p| p.get("steps"))
            .and_then(Value::as_array)
            .context("params.steps must contain executable bindings")?;
        anyhow::ensure!(
            !steps.is_empty() && steps.len() <= MAX_STEPS,
            "workflow must have 1..={MAX_STEPS} executable steps"
        );
        let steps: Vec<String> = steps
            .iter()
            .map(|value| {
                let step = value
                    .as_str()
                    .context("every workflow step must be a string")?;
                let (agent, operation, _) = parse_step(step)?;
                anyhow::ensure!(
                    op_plugins::state_plugins::cognitive_agents::catalog_agent_method(
                        &agent, &operation
                    )
                    .is_some(),
                    "step is not a cognitive catalog method"
                );
                Ok(step.to_owned())
            })
            .collect::<Result<_>>()?;
        // A reservation is owned before any durable accepted receipt. No silent
        // full/closed/uninitialized queue drop can occur after acceptance.
        let slot = self
            .slots
            .clone()
            .try_acquire_owned()
            .context("workflow execution capacity exhausted")?;
        self.store().await?;
        self.engine.chatbot_episode_publisher()?;
        let execution_id = uuid::Uuid::new_v4().to_string();
        let accepted_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        let row = WorkflowExecutionRecord {
            scope,
            execution_id: execution_id.clone(),
            workflow_id: input.workflow_id.clone(),
            owner_epoch: self.epoch.clone(),
            status: "queued".into(),
            revision: 0,
            document: serde_json::json!({"accepted_at": accepted_at,
                "steps": steps, "completed_steps": 0, "in_flight": false, "actions": [],
                "reason": null, "episode_committed": false}),
        };
        anyhow::ensure!(
            serde_json::to_vec(&row.document)?.len() <= 128 * 1024,
            "serialized workflow execution parameters exceed 131072 bytes"
        );
        let runner = self.clone();
        // Detach ownership from the requesting RPC: disconnect/cancellation
        // after durable commit must not strand the admitted execution.
        tokio::spawn(async move {
            let execution = Arc::new(Execution {
                record: Mutex::new(row.clone()),
                changed: Notify::new(),
                unavailable: AtomicBool::new(false),
            });
            // Provisional ownership is visible to query but lifecycle controls
            // wait until creation's durability barrier has settled.
            let admitted = execution.record.lock().await;
            runner
                .active
                .lock()
                .await
                .insert(execution_id.clone(), execution.clone());
            if let Err(error) = runner.persist(row, None).await {
                execution.unavailable.store(true, Ordering::Release);
                runner.active.lock().await.remove(&execution_id);
                return Err(error);
            }
            drop(admitted);
            let worker = runner.clone();
            let id = execution_id.clone();
            tokio::spawn(async move {
                let result = worker.execute(&execution).await;
                if let Err(error) = result {
                    let _row = execution.record.lock().await;
                    execution.unavailable.store(true, Ordering::Release);
                    worker.active.lock().await.remove(&id);
                    execution.changed.notify_one();
                    tracing::error!(execution_id = %id, %error,
                        "workflow stopped; durable outcome must be reconciled before retry");
                }
                worker.active.lock().await.remove(&id);
                drop(slot);
            });
            Ok::<_, anyhow::Error>(serde_json::to_value(StartWorkflowOutput {
                accepted: true,
                execution_id,
                workflow_id: input.workflow_id,
                status: WorkflowExecutionStatus::Queued,
                accepted_at,
            })?)
        })
        .await
        .context("workflow admission task failed")?
    }

    async fn transition(
        self: &Arc<Self>,
        method: &str,
        workflow: String,
        execution_id: Option<String>,
        force: bool,
        scope: EpisodeScope,
    ) -> Result<Value> {
        // Select exclusively among this exact session's owned executions. An
        // omitted execution_id is allowed only when selection is unambiguous.
        let candidates: Vec<_> = self.active.lock().await.values().cloned().collect();
        let mut matches = Vec::new();
        for execution in candidates {
            let row = execution.record.lock().await;
            if row.scope == scope
                && row.workflow_id == workflow
                && execution_id
                    .as_ref()
                    .is_none_or(|id| id == &row.execution_id)
            {
                matches.push(execution.clone());
            }
        }
        anyhow::ensure!(
            matches.len() == 1,
            "workflow transition requires one active execution in the exact caller session; specify execution_id when ambiguous"
        );
        let execution = matches.pop().expect("one execution");
        let runner = self.clone();
        let method = method.to_owned();
        // Once admitted, a durable control transition also survives RPC drop.
        tokio::spawn(async move {
            let mut row = execution.record.lock().await;
            anyhow::ensure!(
                !execution.unavailable.load(Ordering::Acquire),
                "workflow execution owner stopped; transition refused"
            );
            let in_flight = row.document["in_flight"].as_bool().unwrap_or(false);
            let next = transition_status(&row.status, &method, in_flight, force)?;
            runner.update(&mut row, next, in_flight, None).await?;
            let output = serde_json::to_value(WorkflowTransitionOutput {
                execution_id: row.execution_id.clone(),
                workflow_id: row.workflow_id.clone(),
                status: parse_status(&row.status)?,
                revision: row.revision,
                in_flight,
            })?;
            drop(row);
            execution.changed.notify_one();
            Ok::<_, anyhow::Error>(output)
        })
        .await
        .context("workflow transition task failed")?
    }

    async fn query(&self, input: QueryWorkflowsInput, scope: EpisodeScope) -> Result<Value> {
        let store = self.store().await?;
        let workflow = input.workflow_id.clone();
        let exact = input.execution_id.clone();
        let is_exact = exact.is_some();
        let rows = tokio::task::spawn_blocking(move || {
            if let Some(id) = exact {
                let row = store.get_workflow_execution(&scope, &id)?;
                Ok::<_, op_cozo_store::CozoError>(
                    row.into_iter()
                        .filter(|row| workflow.as_ref().is_none_or(|id| id == &row.workflow_id))
                        .collect(),
                )
            } else {
                store.list_workflow_executions(&scope, workflow.as_deref(), 200)
            }
        })
        .await??;
        let executions_truncated = !is_exact && rows.len() == 200;
        let mut executions = Vec::with_capacity(rows.len());
        for mut row in rows {
            let owned = row.owner_epoch == self.epoch
                && self.active.lock().await.contains_key(&row.execution_id);
            // The worker may have committed terminal state after the bounded
            // history read but before removing ownership. Read again after
            // observing no owner, so a completed run is not called interrupted.
            if !owned && !is_terminal(&row.status) {
                let store = self.store().await?;
                let scope = row.scope.clone();
                let id = row.execution_id.clone();
                if let Some(current) =
                    tokio::task::spawn_blocking(move || store.get_workflow_execution(&scope, &id))
                        .await??
                {
                    row = current;
                }
            }
            executions.push(execution_summary(&row, owned)?);
        }
        let mut workflows = workflows_plugin::workflow_summaries();
        if let Some(id) = input.workflow_id {
            workflows.retain(|workflow| workflow.id == id);
        }
        Ok(serde_json::to_value(QueryWorkflowsOutput {
            count: workflows.len(),
            workflows,
            executions,
            executions_truncated,
        })?)
    }

    fn execute<'a>(
        &'a self,
        execution: &'a Execution,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            let started = Instant::now();
            let started_at = chrono::Utc::now();
            loop {
                anyhow::ensure!(
                    !execution.unavailable.load(Ordering::Acquire),
                    "workflow ownership unavailable after persistence failure"
                );
                let notified = execution.changed.notified();
                let row = execution.record.lock().await;
                if is_terminal(&row.status) {
                    break;
                }
                if row.status == "paused" {
                    drop(row);
                    notified.await;
                    continue;
                }
                drop(row);
                // Acquire only for a step; paused runs never occupy worker slots.
                let permit = self
                    .workers
                    .acquire()
                    .await
                    .context("workflow workers closed")?;
                let mut row = execution.record.lock().await;
                anyhow::ensure!(
                    !execution.unavailable.load(Ordering::Acquire),
                    "workflow ownership unavailable after persistence failure"
                );
                if row.status == "paused" {
                    drop(row);
                    drop(permit);
                    continue;
                }
                if is_terminal(&row.status) {
                    break;
                }
                let index = row.document["completed_steps"].as_u64().unwrap_or(0) as usize;
                let steps: Vec<String> = serde_json::from_value(row.document["steps"].clone())?;
                if index == steps.len() {
                    self.update(&mut row, "succeeded", false, None).await?;
                    break;
                }
                // Identity must still be valid immediately before each dispatch.
                if let Err(error) = verified_scope(
                    &self.engine,
                    &row.scope.principal_id,
                    Some(&row.scope.session_id),
                    Some(&row.scope.session_genesis),
                )
                .await
                {
                    self.update(
                        &mut row,
                        "failed",
                        false,
                        Some(&truncate(&error.to_string(), 512)),
                    )
                    .await?;
                    break;
                }
                let scope = row.scope.clone();
                let (agent, operation, args) = parse_step(&steps[index])?;
                let name = op_plugins::state_plugins::cognitive_agents::agent_method_name(
                    &agent, &operation,
                );
                let catalog = op_plugins::state_plugins::cognitive_agents::catalog_agent_method(
                    &agent, &operation,
                )
                .context("workflow catalog binding disappeared")?;
                self.update(&mut row, "running", true, None).await?;
                drop(row);
                let at = Instant::now();
                let outcome = tokio::time::timeout(
                    STEP_TIMEOUT,
                    self.engine.dispatch_method_call_with_identity(
                        COGNITIVE_PLUGIN,
                        &name,
                        &args.to_string(),
                        catalog.declaration.required_capability.as_deref(),
                        &scope.principal_id,
                        Some(&scope.session_id),
                        Some(&scope.session_genesis),
                    ),
                )
                .await;
                drop(permit);
                let mut row = execution.record.lock().await;
                anyhow::ensure!(
                    !execution.unavailable.load(Ordering::Acquire),
                    "in-flight step settled after workflow ownership lost; durable phase requires reconciliation"
                );
                let reported_success = match &outcome {
                    Ok(Ok(envelope)) => Some(crate::agent_workflow::child_success(
                        &catalog.category,
                        envelope.get("result").unwrap_or(envelope),
                    )),
                    _ => None,
                };
                let known_failure = matches!(&reported_success, Some(Ok(false)));
                let (ok, error) = match (&outcome, &reported_success) {
                    (Ok(Ok(_)), Some(Ok(true))) => (true, None),
                    (Ok(Ok(_)), Some(Ok(false))) => (
                        false,
                        Some("step reported an unsuccessful domain result".into()),
                    ),
                    (Ok(Ok(_)), Some(Err(error))) => {
                        (false, Some(truncate(&error.to_string(), 1024)))
                    }
                    (Ok(Err(error)), _) => (false, Some(truncate(&error.to_string(), 1024))),
                    (Err(_), _) => (
                        false,
                        Some(
                            "step deadline exceeded; backend effects/outcome may be unknown".into(),
                        ),
                    ),
                    _ => (false, Some("step outcome unavailable".into())),
                };
                let action = RunAction {
                    method: name,
                    ok,
                    duration_ms: at.elapsed().as_millis() as u64,
                    error,
                };
                let mut candidate = row.clone();
                candidate.document["deadline_exceeded"] = Value::Bool(outcome.is_err());
                candidate.document["actions"]
                    .as_array_mut()
                    .context("workflow actions corrupt")?
                    .push(serde_json::to_value(action)?);
                if ok {
                    candidate.document["completed_steps"] = serde_json::json!(index + 1);
                }
                if let Ok(Ok(envelope)) = &outcome {
                    candidate.document["summary"] = Value::String(truncate(
                        &envelope.get("result").unwrap_or(&Value::Null).to_string(),
                        2048,
                    ));
                }
                let (next, reason) = if known_failure {
                    (
                        "failed",
                        Some(
                            "step reported failure; no later steps dispatched; completed effects are not undone",
                        ),
                    )
                } else if matches!(&reported_success, Some(Err(_))) {
                    (
                        "outcome_unknown",
                        Some("step result omitted its required outcome; no later steps dispatched"),
                    )
                } else {
                    settled_status(&row.status, &outcome)
                };
                self.update(&mut candidate, next, false, reason).await?;
                *row = candidate;
            }
            // Terminal receipt already durably captures actions and known status.
            // Episode publication uses the existing paired private SHM owner.
            let row = execution.record.lock().await.clone();
            let actions: Vec<RunAction> = serde_json::from_value(row.document["actions"].clone())?;
            let status = match row.status.as_str() {
                "succeeded" => RunStatus::Succeeded,
                "cancelled" => RunStatus::Dropped,
                "outcome_unknown" if row.document["deadline_exceeded"].as_bool() == Some(true) => {
                    RunStatus::Timeout
                }
                _ => RunStatus::Failed,
            };
            let reason = row.document["reason"].as_str().map(str::to_owned);
            let record = RunRecord {
                status,
                trigger_id: row.execution_id.clone(),
                topic_ref: blake3::Hash::from(blake3::derive_key(
                    "opdbus workstack execution v1",
                    format!("{}:{}", row.workflow_id, row.execution_id).as_bytes(),
                ))
                .to_hex()
                .to_string(),
                queued_ms: 0,
                duration_ms: started.elapsed().as_millis() as u64,
                tokens_in: 0,
                tokens_out: 0,
                actions,
                reason,
            };
            let episode = build_completed_agent_run(CompletedAgentRun {
                scope: row.scope.clone(),
                trigger_id: row.execution_id.clone(),
                goal_text: row.workflow_id.clone(),
                summary: row.document["summary"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                tools: record
                    .actions
                    .iter()
                    .map(|action| action.method.clone())
                    .collect(),
                started_at,
                ended_at: chrono::Utc::now(),
                run: record,
            })?;
            let committed = self.engine.commit_chatbot_episode(episode).await;
            let mut row = execution.record.lock().await;
            let mut candidate = row.clone();
            candidate.document["episode_committed"] = Value::Bool(committed.is_ok());
            if let Err(error) = committed {
                candidate.document["episode_error"] =
                    Value::String(truncate(&error.to_string(), 512));
            }
            let status = row.status.clone();
            self.update(&mut candidate, &status, false, None).await?;
            *row = candidate;
            Ok(())
        })
    }
}

fn is_terminal(status: &str) -> bool {
    matches!(
        status,
        "succeeded" | "failed" | "cancelled" | "outcome_unknown" | "interrupted"
    )
}

fn transition_status(
    status: &str,
    method: &str,
    in_flight: bool,
    force: bool,
) -> Result<&'static str> {
    anyhow::ensure!(!is_terminal(status), "execution is terminal");
    match method {
        "pause_workflow"
            if matches!(status, "queued" | "running" | "paused" | "pause_requested") =>
        {
            Ok(if in_flight {
                "pause_requested"
            } else {
                "paused"
            })
        }
        "resume_workflow" if matches!(status, "paused" | "pause_requested") => {
            Ok(if in_flight { "running" } else { "queued" })
        }
        "cancel_workflow" => {
            anyhow::ensure!(
                !in_flight || force,
                "running workflow cancellation requires force=true; backend effects are not undone"
            );
            Ok(if in_flight {
                "cancel_requested"
            } else {
                "cancelled"
            })
        }
        _ => Err(anyhow!("invalid workflow transition from {status}")),
    }
}

fn settled_status(
    status: &str,
    outcome: &std::result::Result<Result<Value>, tokio::time::error::Elapsed>,
) -> (&'static str, Option<&'static str>) {
    match outcome {
        Err(_) => (
            "outcome_unknown",
            Some(
                "step timed out; backend effects/outcome may be unknown; no later steps dispatched",
            ),
        ),
        Ok(Err(_)) => (
            "outcome_unknown",
            Some(
                "dispatch failed; admission/backend/publication failure not distinguishable here; backend effects/outcome may be unknown; no later steps dispatched",
            ),
        ),
        Ok(Ok(_)) if status == "cancel_requested" => (
            "cancelled",
            Some("cancelled after in-flight step settled; completed effects are not undone"),
        ),
        Ok(Ok(_)) if status == "pause_requested" => ("paused", None),
        Ok(Ok(_)) => ("queued", None),
    }
}

fn parse_status(status: &str) -> Result<WorkflowExecutionStatus> {
    Ok(serde_json::from_value(Value::String(status.into()))?)
}

fn execution_summary(
    row: &WorkflowExecutionRecord,
    owned: bool,
) -> Result<WorkflowExecutionSummary> {
    let interrupted = !owned && !is_terminal(&row.status);
    Ok(WorkflowExecutionSummary {
        execution_id: row.execution_id.clone(),
        workflow_id: row.workflow_id.clone(),
        status: if interrupted {
            WorkflowExecutionStatus::Interrupted
        } else {
            parse_status(&row.status)?
        },
        revision: row.revision,
        accepted_at: row.document["accepted_at"]
            .as_str()
            .unwrap_or_default()
            .into(),
        completed_steps: row.document["completed_steps"].as_u64().unwrap_or(0) as usize,
        in_flight: row.document["in_flight"].as_bool().unwrap_or(false),
        reason: if interrupted {
            Some("execution owner unavailable; last recorded phase retained; backend outcome may be unknown; never auto-replayed".into())
        } else {
            row.document["reason"].as_str().map(str::to_owned)
        },
        episode_committed: row.document["episode_committed"].as_bool().unwrap_or(false),
        episode_error: row.document["episode_error"].as_str().map(str::to_owned),
    })
}

static RUNNER: OnceLock<Arc<WorkstackRunner>> = OnceLock::new();

pub(crate) fn start_once(engine: Arc<MutationEngine>) {
    RUNNER.get_or_init(|| Arc::new(WorkstackRunner::new(engine)));
}

async fn verified_scope(
    engine: &MutationEngine,
    actor: &str,
    session: Option<&str>,
    genesis: Option<&str>,
) -> Result<EpisodeScope> {
    let session = session.context("workflow requires a verified session")?;
    let scope = crate::agent_runs::agent_scope(engine, session)
        .await?
        .context("workflow session is not a current verified identity")?;
    anyhow::ensure!(
        scope.principal_id == actor && Some(scope.session_genesis.as_str()) == genesis,
        "workflow caller differs from verified session"
    );
    Ok(scope)
}

/// Both cognitive HOT facade and workflows owner enter this same handler
/// after their own sealed declaration/capability admission. Every step is
/// separately readmitted under the exact original caller, never a service user.
pub(crate) async fn dispatch(
    engine: &MutationEngine,
    method: &str,
    args: &Value,
    actor: &str,
    session: Option<&str>,
    genesis: Option<&str>,
) -> Result<Value> {
    let runner = RUNNER
        .get()
        .context("workflow execution owner is not initialized")?;
    let scope = verified_scope(engine, actor, session, genesis).await?;
    match method {
        "query_workflows" => {
            runner
                .query(serde_json::from_value(args.clone())?, scope)
                .await
        }
        "start_workflow" => {
            runner
                .start(serde_json::from_value(args.clone())?, scope)
                .await
        }
        "pause_workflow" => {
            let input: PauseWorkflowInput = serde_json::from_value(args.clone())?;
            runner
                .transition(method, input.workflow_id, input.execution_id, false, scope)
                .await
        }
        "resume_workflow" => {
            let input: ResumeWorkflowInput = serde_json::from_value(args.clone())?;
            runner
                .transition(method, input.workflow_id, input.execution_id, false, scope)
                .await
        }
        "cancel_workflow" => {
            let input: CancelWorkflowInput = serde_json::from_value(args.clone())?;
            runner
                .transition(
                    method,
                    input.workflow_id,
                    input.execution_id,
                    input.force,
                    scope,
                )
                .await
        }
        _ => Err(anyhow!("unknown workflows method {method}")),
    }
}

pub(crate) fn parse_step(step: &str) -> Result<(String, String, Value)> {
    let (binding, args_json) = match step.split_once('|') {
        Some((binding, args_json)) => {
            anyhow::ensure!(
                args_json.len() <= ARGS_MAX_BYTES,
                "step args exceed {ARGS_MAX_BYTES} bytes"
            );
            (binding.trim(), args_json.trim())
        }
        None => (step.trim(), ""),
    };
    anyhow::ensure!(
        binding.len() <= STEP_NAME_MAX_BYTES,
        "step binding exceeds {STEP_NAME_MAX_BYTES} bytes"
    );
    let (agent, operation) = binding
        .split_once('.')
        .context("expected executable agent.operation binding")?;
    let (agent, operation) = (agent.trim(), operation.trim());
    anyhow::ensure!(
        !agent.is_empty() && !operation.is_empty(),
        "empty step binding"
    );
    let args = if args_json.is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(args_json).context("step args are not JSON")?
    };
    anyhow::ensure!(args.is_object(), "step args must be a JSON object");
    Ok((agent.into(), operation.into(), args))
}

fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        text.to_owned()
    } else {
        let mut cut = max;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &text[..cut])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workstack_parses_binding_with_inline_args() {
        let (agent, operation, args) =
            parse_step(r#" rust-pro . build | {"path": "crates/op-web", "note": "a|b"} "#)
                .expect("step parse");
        assert_eq!(agent, "rust-pro");
        assert_eq!(operation, "build");
        assert_eq!(
            args,
            serde_json::json!({"path": "crates/op-web", "note": "a|b"})
        );
        for step in ["rust-pro.build", "rust-pro.build|", "rust-pro.build| {}"] {
            let (agent, operation, args) = parse_step(step).unwrap();
            assert_eq!((agent.as_str(), operation.as_str()), ("rust-pro", "build"));
            assert_eq!(args, serde_json::json!({}));
        }
    }

    #[test]
    fn workstack_refuses_missing_bindings_and_invalid_arguments() {
        for step in [
            "",
            "build",
            ".build",
            "rust-pro.",
            " . |{}",
            "rust-pro.build|{",
            "rust-pro.build|[]",
            "rust-pro.build|null",
            "rust-pro.build|true",
            "rust-pro.build|42",
            r#"rust-pro.build|"text""#,
        ] {
            assert!(parse_step(step).is_err(), "accepted {step:?}");
        }
        assert!(parse_step("deploy.spill|{}").is_ok());
    }

    #[test]
    fn workstack_bounds_binding_and_arguments_independently() {
        let binding = format!("a.{}", "b".repeat(STEP_NAME_MAX_BYTES - 2));
        assert!(parse_step(&binding).is_ok());
        assert!(parse_step(&format!("{binding}b")).is_err());
        let args = format!(r#"{{"text":"{}"}}"#, "x".repeat(ARGS_MAX_BYTES - 11));
        assert_eq!(args.len(), ARGS_MAX_BYTES);
        assert!(parse_step(&format!("rust-pro.build|{args}")).is_ok());
        assert!(parse_step(&format!("rust-pro.build|{args} ")).is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn workstack_no_verified_session_means_no_execution_or_episode() {
        use crate::human_principal_dispatch::tests::{
            temp_cozo, temp_shm_with_schemas, test_engine,
        };
        use op_plugins::state_plugins::identity_sled::IdentitySledPlugin;
        use op_state::StatePlugin as _;
        let _cozo = temp_cozo();
        let _shm = temp_shm_with_schemas(&[IdentitySledPlugin::new().schema().unwrap()]);
        let engine = test_engine();
        let identities = crate::identity_sled_dispatch::IdentitySledStore::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        );
        engine
            .install_identity_sled_store_for_test(identities.clone())
            .unwrap();
        crate::identity_sled_dispatch::recover_identity_startup_with_store(&engine, &identities)
            .await
            .unwrap();
        let store = Arc::new(CozoGraphShuttle::new_in_memory().unwrap());
        engine
            .configure_chatbot_episode_store(store.clone())
            .unwrap();
        let scope = EpisodeScope {
            principal_id: "principal".into(),
            session_id: "no-such-session".into(),
            session_genesis: "genesis".into(),
        };
        assert!(
            crate::agent_runs::agent_scope(&engine, &scope.session_id)
                .await
                .unwrap()
                .is_none()
        );
        for session in [None, Some(scope.session_id.as_str())] {
            assert!(
                verified_scope(
                    &engine,
                    &scope.principal_id,
                    session,
                    Some(&scope.session_genesis)
                )
                .await
                .is_err()
            );
        }
        assert!(
            store
                .list_scoped_episodes(&scope, None, 10)
                .unwrap()
                .is_empty()
        );
        store.initialize_workflow_execution_store().unwrap();
        assert!(
            store
                .list_workflow_executions(&scope, None, 10)
                .unwrap()
                .is_empty()
        );
    }

    fn record() -> WorkflowExecutionRecord {
        WorkflowExecutionRecord {
            scope: EpisodeScope {
                principal_id: "principal".into(),
                session_id: "session".into(),
                session_genesis: "genesis".into(),
            },
            execution_id: "execution".into(),
            workflow_id: "deploy".into(),
            owner_epoch: "owner".into(),
            status: "running".into(),
            revision: 1,
            document: serde_json::json!({"accepted_at": "2026-10-05T00:00:00Z", "in_flight": true, "completed_steps": 1}),
        }
    }

    #[test]
    fn workflow_controls_wait_for_backend_settlement_without_claiming_undo() {
        let success = Ok(Ok(serde_json::json!({"result": {"done": true}})));
        assert_eq!(
            transition_status("running", "pause_workflow", true, false).unwrap(),
            "pause_requested"
        );
        assert_eq!(settled_status("pause_requested", &success).0, "paused");
        assert_eq!(
            transition_status("pause_requested", "resume_workflow", true, false).unwrap(),
            "running"
        );
        assert!(transition_status("running", "cancel_workflow", true, false).is_err());
        assert_eq!(
            transition_status("running", "cancel_workflow", true, true).unwrap(),
            "cancel_requested"
        );
        let (status, reason) = settled_status("cancel_requested", &success);
        assert_eq!(status, "cancelled");
        assert!(reason.unwrap().contains("not undone"));
        let failure = Ok(Err(anyhow!(
            "backend returned after effect, receipt unavailable"
        )));
        assert_eq!(
            settled_status("cancel_requested", &failure).0,
            "outcome_unknown"
        );
        for status in [
            "succeeded",
            "failed",
            "cancelled",
            "outcome_unknown",
            "interrupted",
        ] {
            assert!(transition_status(status, "resume_workflow", false, false).is_err());
        }
    }

    #[test]
    fn workflow_lost_owner_is_interrupted_and_preserves_last_phase() {
        let row = record();
        let summary = execution_summary(&row, false).unwrap();
        assert_eq!(summary.status, WorkflowExecutionStatus::Interrupted);
        assert!(summary.in_flight);
        assert_eq!(summary.completed_steps, 1);
        assert!(!summary.episode_committed);
        assert!(summary.episode_error.is_none());
        assert!(summary.reason.unwrap().contains("never auto-replayed"));
        let mut terminal = row.clone();
        terminal.status = "succeeded".into();
        terminal.document["in_flight"] = Value::Bool(false);
        terminal.document["episode_committed"] = Value::Bool(false);
        terminal.document["episode_error"] =
            Value::String("private SHM publication unavailable".into());
        let summary = execution_summary(&terminal, false).unwrap();
        assert!(!summary.episode_committed);
        assert_eq!(
            summary.episode_error.as_deref(),
            Some("private SHM publication unavailable")
        );
        assert_eq!(
            execution_summary(&terminal, false).unwrap().status,
            WorkflowExecutionStatus::Succeeded
        );
    }

    #[tokio::test]
    async fn workflow_reserved_capacity_refuses_before_any_durable_acceptance() {
        let runner = Arc::new(WorkstackRunner::new(
            crate::human_principal_dispatch::tests::test_engine(),
        ));
        let _all = runner
            .slots
            .clone()
            .acquire_many_owned(RUN_CAPACITY as u32)
            .await
            .unwrap();
        let method = op_plugins::state_plugins::cognitive_agents::catalog_agent_methods()
            .next()
            .unwrap();
        let args = StartWorkflowInput {
            workflow_id: "deploy".into(),
            params: Some(serde_json::json!({
            "steps": [format!("{}.{}|{{}}", method.agent_type, method.operation)] })),
        };
        let error = runner.start(args, record().scope).await.unwrap_err();
        assert!(error.to_string().contains("capacity exhausted"));
        assert!(runner.store.get().is_none());
        assert!(runner.active.lock().await.is_empty());
    }

    #[tokio::test]
    async fn workflow_closed_owner_refuses_control_on_a_stale_selected_arc() {
        let runner = Arc::new(WorkstackRunner::new(
            crate::human_principal_dispatch::tests::test_engine(),
        ));
        let row = record();
        let execution = Arc::new(Execution {
            record: Mutex::new(row.clone()),
            changed: Notify::new(),
            unavailable: AtomicBool::new(true),
        });
        runner
            .active
            .lock()
            .await
            .insert(row.execution_id.clone(), execution);
        let error = runner
            .transition(
                "pause_workflow",
                row.workflow_id,
                Some(row.execution_id),
                false,
                row.scope,
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("owner stopped"));
        assert!(runner.store.get().is_none());
    }
}
