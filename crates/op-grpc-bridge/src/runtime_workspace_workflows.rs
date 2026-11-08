//! Bounded, event-driven workflow execution over the existing episode Cozo.
//! No restart replay, no ambient identity, and no step retry after uncertainty.
use std::{collections::HashMap, sync::{Arc, Mutex as StdMutex, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};
use anyhow::{Context, Result, ensure, bail};
use op_cozo_store::{CozoGraphShuttle, EpisodeScope, WorkflowExecutionRecord};
use op_plugins::state_plugins::{cognitive_agents, ctl_plane_chatbot::{RunAction, RunRecord, RunStatus}, ctl_plane_chatbot_methods::{CompletedAgentRun, build_completed_agent_run}, workflows_plugin::*};
use serde_json::{Value, json};
use tokio::sync::{Mutex, Notify, OnceCell, OwnedSemaphorePermit, Semaphore};
use crate::{core::BackendStateUpdate, runtime::{BackendContext, BackendOutcome, Runtime, RuntimeError, VerifiedCaller}};

#[derive(Clone)]
pub(super) struct Workflows { inner: Arc<Inner> }
struct Inner {
    epoch: String,
    slots: Arc<Semaphore>,
    workers: Semaphore,
    active: StdMutex<HashMap<String, Arc<Execution>>>,
    store: OnceCell<Arc<CozoGraphShuttle>>,
}
struct Execution {
    row: Mutex<WorkflowExecutionRecord>,
    caller: VerifiedCaller,
    started: AtomicBool,
    available: AtomicBool,
    changed: Notify,
    _slot: OwnedSemaphorePermit,
}

struct AdmissionGuard { owner: Arc<Inner>, id: String }
impl Drop for AdmissionGuard {
    fn drop(&mut self) {
        let mut active = self.owner.active.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if active.get(&self.id).is_some_and(|execution| !execution.started.load(Ordering::Acquire)) {
            active.remove(&self.id);
        }
    }
}

pub(super) fn supports(method: &str) -> bool { matches!(method, "query_workflows" | "start_workflow" | "pause_workflow" | "resume_workflow" | "cancel_workflow") }
fn scope(caller: &VerifiedCaller) -> EpisodeScope { EpisodeScope { principal_id: caller.principal_id.clone(), session_id: caller.session_id.clone(), session_genesis: caller.session_genesis.clone() } }
fn terminal(status: &str) -> bool { matches!(status, "succeeded" | "failed" | "cancelled" | "outcome_unknown" | "interrupted") }
fn status(value: &str) -> Result<WorkflowExecutionStatus> { Ok(serde_json::from_value(Value::String(value.into()))?) }

impl Workflows {
    pub(super) fn new() -> Self {
        Self { inner: Arc::new(Inner { epoch: uuid::Uuid::new_v4().to_string(), slots: Arc::new(Semaphore::new(64)), workers: Semaphore::new(2), active: StdMutex::new(HashMap::new()), store: OnceCell::new() }) }
    }

    async fn store(&self, runtime: &Runtime) -> Result<Arc<CozoGraphShuttle>> {
        self.inner.store.get_or_try_init(|| async {
            let store = runtime.chat().episode_store();
            let initialize = store.clone();
            tokio::task::spawn_blocking(move || initialize.initialize_workflow_execution_store()).await??;
            Ok::<_, anyhow::Error>(store)
        }).await.cloned()
    }

    pub(super) async fn dispatch(&self, ctx: &BackendContext<'_>, method: &str, args: &Value) -> Result<BackendOutcome, RuntimeError> {
        let store = self.store(ctx.runtime).await?;
        match method {
            "query_workflows" => {
                let input: QueryWorkflowsInput = serde_json::from_value(args.clone())?;
                let exact = input.execution_id.clone();
                let workflow = input.workflow_id.clone();
                let caller_scope = scope(ctx.caller);
                let rows = tokio::task::spawn_blocking(move || {
                    if let Some(id) = exact { Ok::<_, op_cozo_store::CozoError>(store.get_workflow_execution(&caller_scope, &id)?.into_iter().filter(|row| workflow.as_ref().is_none_or(|id| id == &row.workflow_id)).collect()) }
                    else { store.list_workflow_executions(&caller_scope, workflow.as_deref(), 200) }
                }).await??;
                let executions_truncated = input.execution_id.is_none() && rows.len() == 200;
                let mut executions = Vec::with_capacity(rows.len());
                for mut row in rows {
                    let owned = self.inner.active.lock().map_err(|_| anyhow::anyhow!("workflow owner poisoned"))?.get(&row.execution_id).is_some_and(|execution| execution.started.load(Ordering::Acquire) && execution.available.load(Ordering::Acquire));
                    if !owned && !terminal(&row.status) {
                        let store = self.store(ctx.runtime).await?;
                        let exact_scope = row.scope.clone(); let id = row.execution_id.clone();
                        row = tokio::task::spawn_blocking(move || store.get_workflow_execution(&exact_scope, &id)).await??.context("workflow receipt disappeared")?;
                    }
                    executions.push(summary(&row, owned && row.owner_epoch == self.inner.epoch)?);
                }
                let mut workflows = declared_workflows(ctx.runtime)?;
                if let Some(id) = input.workflow_id { workflows.retain(|workflow| workflow.id == id); }
                let result = serde_json::to_value(QueryWorkflowsOutput { count: workflows.len(), workflows, executions, executions_truncated })?;
                Ok(super::outcome(result, BackendStateUpdate::Unchanged, super::private_scope(ctx)))
            }
            "start_workflow" => self.start(ctx, store, serde_json::from_value(args.clone())?).await,
            "pause_workflow" => { let input: PauseWorkflowInput = serde_json::from_value(args.clone())?; self.transition(ctx, store, method, input.workflow_id, input.execution_id, false).await }
            "resume_workflow" => { let input: ResumeWorkflowInput = serde_json::from_value(args.clone())?; self.transition(ctx, store, method, input.workflow_id, input.execution_id, false).await }
            "cancel_workflow" => { let input: CancelWorkflowInput = serde_json::from_value(args.clone())?; self.transition(ctx, store, method, input.workflow_id, input.execution_id, input.force).await }
            _ => Err(RuntimeError::unsupported(format!("unknown workflow method {method}"))),
        }
    }

    async fn start(&self, ctx: &BackendContext<'_>, store: Arc<CozoGraphShuttle>, input: StartWorkflowInput) -> Result<BackendOutcome, RuntimeError> {
        super::require(input.workflow_id.len() <= 256 && declared_workflows(ctx.runtime)?.iter().any(|workflow| workflow.id == input.workflow_id), "unknown workflow")?;
        let raw = input.params.as_ref().and_then(|value| value.get("steps")).and_then(Value::as_array).context("params.steps must contain executable bindings")?;
        super::require(!raw.is_empty() && raw.len() <= 32, "workflow requires 1-32 steps")?;
        let mut steps = Vec::with_capacity(raw.len());
        for value in raw {
            let step = value.as_str().context("workflow steps must be strings")?;
            let (agent, operation, arguments) = parse_step(step)?;
            let method = cognitive_agents::catalog_agent_method(&agent, &operation).context("workflow step is not a cognitive catalog method")?;
            let name = cognitive_agents::agent_method_name(&agent, &operation);
            ctx.runtime.authorize_nested(ctx.caller, "cognitive", &name).await?;
            let argument_schema = serde_json::to_value(&method.declaration.args)?;
            jsonschema::validator_for(&argument_schema).map_err(|error| anyhow::anyhow!("invalid sealed workflow argument schema: {error}"))?
                .validate(&arguments).map_err(|error| anyhow::anyhow!("invalid workflow step arguments: {error}"))?;
            steps.push(step.to_owned());
        }
        let permit = self.inner.slots.clone().try_acquire_owned().context("workflow execution capacity exhausted")?;
        let execution_id = uuid::Uuid::new_v4().to_string();
        let accepted_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        let row = WorkflowExecutionRecord { scope: scope(ctx.caller), execution_id: execution_id.clone(), workflow_id: input.workflow_id.clone(), owner_epoch: self.inner.epoch.clone(), status: "queued".into(), revision: 0, document: json!({"accepted_at":accepted_at,"steps":steps,"completed_steps":0,"in_flight":false,"actions":[],"reason":null,"episode_committed":false}) };
        super::require(serde_json::to_vec(&row.document)?.len() <= 128 * 1024, "workflow parameters exceed capacity")?;
        ctx.caller.authority.revalidate().await.map_err(|error| RuntimeError::denied(error.to_string()))?;
        let persist = row.clone();
        let authority = ctx.caller.authority.clone();
        let handle = tokio::runtime::Handle::current();
        tokio::task::spawn_blocking(move || {
            handle.block_on(authority.revalidate())?;
            store.put_workflow_execution(&persist, None)?;
            Ok::<_, anyhow::Error>(())
        }).await.map_err(|error| RuntimeError::unknown(error.into()))?.map_err(RuntimeError::unknown)?;
        let execution = Arc::new(Execution { row: Mutex::new(row), caller: ctx.caller.for_background(), started: AtomicBool::new(false), available: AtomicBool::new(true), changed: Notify::new(), _slot: permit });
        self.inner.active.lock().map_err(|_| anyhow::anyhow!("workflow owner poisoned after durable admission"))?.insert(execution_id.clone(), execution);
        let result = serde_json::to_value(StartWorkflowOutput { accepted: true, execution_id: execution_id.clone(), workflow_id: input.workflow_id, status: WorkflowExecutionStatus::Queued, accepted_at })?;
        let mut outcome = super::outcome(result, BackendStateUpdate::Unchanged, super::private_scope(ctx));
        outcome.guard = Some(Box::new(AdmissionGuard { owner: self.inner.clone(), id: execution_id }));
        Ok(outcome)
    }

    pub(super) async fn after_commit(&self, ctx: &BackendContext<'_>, method: &str, result: &Value) -> Result<(), RuntimeError> {
        if method != "start_workflow" { return Ok(()) }
        let id = result["execution_id"].as_str().context("workflow commit omitted execution ID")?;
        let execution = self.inner.active.lock().map_err(|_| anyhow::anyhow!("workflow owner poisoned"))?.get(id).cloned().context("workflow admission owner disappeared after commit")?;
        super::require(scope(&execution.caller) == scope(ctx.caller), "workflow committed caller differs")?;
        ctx.caller.authority.revalidate().await.map_err(|error| RuntimeError::denied(error.to_string()))?;
        super::require(!execution.started.swap(true, Ordering::AcqRel), "workflow owner already started")?;
        let runner = self.clone();
        let runtime = ctx.runtime.clone();
        let id = id.to_owned();
        tokio::spawn(async move {
            if let Err(error) = runner.execute(&runtime, &execution).await {
                execution.available.store(false, Ordering::Release);
                execution.changed.notify_one();
                tracing::error!(execution_id = %id, %error, "workflow stopped with retained durable phase; never auto-replay");
            }
            runner.inner.active.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).remove(&id);
        });
        Ok(())
    }

    async fn transition(&self, ctx: &BackendContext<'_>, store: Arc<CozoGraphShuttle>, method: &str, workflow: String, exact: Option<String>, force: bool) -> Result<BackendOutcome, RuntimeError> {
        let candidates: Vec<_> = self.inner.active.lock().map_err(|_| anyhow::anyhow!("workflow owner poisoned"))?.values().cloned().collect();
        let mut found = Vec::new();
        for execution in candidates {
            let row = execution.row.lock().await;
            if row.scope == scope(ctx.caller) && row.workflow_id == workflow && exact.as_ref().is_none_or(|id| id == &row.execution_id) && execution.started.load(Ordering::Acquire) { found.push(execution.clone()); }
        }
        super::require(found.len() == 1, "workflow transition requires one live execution in the exact caller session")?;
        let execution = found.pop().context("workflow execution missing")?;
        let mut row = execution.row.lock().await;
        super::require(execution.available.load(Ordering::Acquire), "workflow execution owner stopped")?;
        ctx.caller.authority.revalidate().await.map_err(|error| RuntimeError::denied(error.to_string()))?;
        let in_flight = row.document["in_flight"].as_bool().context("workflow in-flight state corrupt")?;
        let next = transition_status(&row.status, method, in_flight, force)?;
        persist_update(store, &execution, &mut row, next, in_flight, None).await.map_err(RuntimeError::unknown)?;
        let result = serde_json::to_value(WorkflowTransitionOutput { execution_id: row.execution_id.clone(), workflow_id: row.workflow_id.clone(), status: status(&row.status)?, revision: row.revision, in_flight })?;
        drop(row);
        execution.changed.notify_one();
        Ok(super::outcome(result, BackendStateUpdate::Unchanged, super::private_scope(ctx)))
    }

    async fn execute(&self, runtime: &Runtime, execution: &Execution) -> Result<()> {
        let started = Instant::now(); let started_at = chrono::Utc::now();
        let store = self.store(runtime).await?;
        loop {
            ensure!(execution.available.load(Ordering::Acquire), "workflow durable owner unavailable");
            let notified = execution.changed.notified();
            let row = execution.row.lock().await;
            if terminal(&row.status) { break }
            if row.status == "paused" { drop(row); notified.await; continue }
            drop(row);
            let permit = self.inner.workers.acquire().await.context("workflow worker pool closed")?;
            let mut row = execution.row.lock().await;
            if terminal(&row.status) { break }
            if row.status == "paused" { drop(row); drop(permit); continue }
            let steps: Vec<String> = serde_json::from_value(row.document["steps"].clone())?;
            let index = row.document["completed_steps"].as_u64().context("workflow completed-step counter corrupt")? as usize;
            if index == steps.len() { persist_update(store.clone(), execution, &mut row, "succeeded", false, None).await?; break }
            let step = steps.get(index).context("workflow step index out of bounds")?;
            let (agent, operation, args) = parse_step(step)?;
            let binding = cognitive_agents::catalog_agent_method(&agent, &operation).context("workflow catalog binding disappeared")?;
            let name = cognitive_agents::agent_method_name(&agent, &operation);
            if let Err(error) = runtime.authorize_nested(&execution.caller, "cognitive", &name).await {
                persist_update(store.clone(), execution, &mut row, "failed", false, Some(&bounded(&error.to_string(), 512))).await?;
                break;
            }
            persist_update(store.clone(), execution, &mut row, "running", true, None).await?;
            drop(row);
            let at = Instant::now();
            let outcome = tokio::time::timeout(Duration::from_secs(120), runtime.call_nested(&execution.caller, "cognitive", &name, args)).await;
            drop(permit);
            let mut row = execution.row.lock().await;
            ensure!(execution.available.load(Ordering::Acquire), "workflow ownership lost while a step was in flight");
            let (success, next, reason, summary) = match &outcome {
                Ok(Ok(result)) => match child_success(&binding.category, &result.result) {
                    Ok(true) => (true, if row.status == "cancel_requested" { "cancelled" } else if row.status == "pause_requested" { "paused" } else { "queued" }, None, Some(bounded(&result.result.to_string(), 2048))),
                    Ok(false) => (false, "failed", Some("step reported failure; completed effects are not undone".into()), None),
                    Err(error) => (false, "outcome_unknown", Some(bounded(&error.to_string(), 1024)), None),
                },
                Ok(Err(error)) => (false, if error.outcome_unknown() { "outcome_unknown" } else { "failed" }, Some(bounded(&error.to_string(), 1024)), None),
                Err(_) => (false, "outcome_unknown", Some("step deadline exceeded; effects may have occurred; no subsequent step or retry".into()), None),
            };
            let mut candidate = row.clone();
            candidate.document["deadline_exceeded"] = Value::Bool(outcome.is_err());
            candidate.document["actions"].as_array_mut().context("workflow actions corrupt")?.push(serde_json::to_value(RunAction { method: name, ok: success, duration_ms: at.elapsed().as_millis() as u64, error: reason.clone() })?);
            if success { candidate.document["completed_steps"] = json!(index + 1); }
            if let Some(summary) = summary { candidate.document["summary"] = Value::String(summary); }
            persist_update(store.clone(), execution, &mut candidate, next, false, reason.as_deref()).await?;
            *row = candidate;
        }
        let row = execution.row.lock().await.clone();
        let actions: Vec<RunAction> = serde_json::from_value(row.document["actions"].clone())?;
        let run = RunRecord {
            status: match row.status.as_str() { "succeeded" => RunStatus::Succeeded, "cancelled" => RunStatus::Dropped, "outcome_unknown" if row.document["deadline_exceeded"].as_bool() == Some(true) => RunStatus::Timeout, _ => RunStatus::Failed },
            trigger_id: row.execution_id.clone(), topic_ref: blake3::Hash::from(blake3::derive_key("opdbus workstack execution v1", format!("{}:{}", row.workflow_id, row.execution_id).as_bytes())).to_hex().to_string(),
            queued_ms: 0, duration_ms: started.elapsed().as_millis() as u64, tokens_in: 0, tokens_out: 0,
            reason: row.document["reason"].as_str().map(str::to_owned), actions,
        };
        let episode = build_completed_agent_run(CompletedAgentRun {
            scope: row.scope.clone(), trigger_id: row.execution_id.clone(), goal_text: row.workflow_id.clone(),
            summary: row.document["summary"].as_str().unwrap_or_default().into(), tools: run.actions.iter().map(|action| action.method.clone()).collect(), started_at, ended_at: chrono::Utc::now(), run,
        })?;
        let published = runtime.chat().commit_episode(runtime, &execution.caller, episode).await;
        let mut row = execution.row.lock().await;
        let mut candidate = row.clone();
        candidate.document["episode_committed"] = Value::Bool(published.is_ok());
        if let Err(error) = published { candidate.document["episode_error"] = Value::String(bounded(&error.to_string(), 512)); }
        let terminal_status = row.status.clone();
        persist_update(store, execution, &mut candidate, &terminal_status, false, None).await?;
        *row = candidate;
        Ok(())
    }
}

async fn persist_update(store: Arc<CozoGraphShuttle>, execution: &Execution, row: &mut WorkflowExecutionRecord, status: &str, in_flight: bool, reason: Option<&str>) -> Result<()> {
    let expected = row.revision;
    let mut next = row.clone();
    next.revision = expected.checked_add(1).context("workflow revision overflow")?;
    next.status = status.into(); next.document["in_flight"] = Value::Bool(in_flight);
    if let Some(reason) = reason { next.document["reason"] = Value::String(reason.into()); }
    let committed = next.clone();
    let result = tokio::task::spawn_blocking(move || store.put_workflow_execution(&committed, Some(expected))).await;
    match result {
        Ok(Ok(())) => { *row = next; Ok(()) }
        Ok(Err(error)) => { execution.available.store(false, Ordering::Release); execution.changed.notify_one(); Err(error.into()) }
        Err(error) => { execution.available.store(false, Ordering::Release); execution.changed.notify_one(); Err(error.into()) }
    }
}

fn declared_workflows(runtime: &Runtime) -> Result<Vec<WorkflowSummary>> {
    let schema = super::schema(runtime.catalog(), "workflows")?;
    let state: WorkflowsState = serde_json::from_value(serde_json::to_value(op_plugins::projection_seed_state_from_schema(&schema))?)?;
    Ok(serde_json::from_value(state.workflows)?)
}

fn parse_step(step: &str) -> Result<(String, String, Value)> {
    let (binding, args) = step.split_once('|').unwrap_or((step, ""));
    ensure!(binding.len() <= 256 && args.len() <= 16 * 1024, "workflow step exceeds byte bounds");
    let (agent, operation) = binding.trim().split_once('.').context("expected agent.operation binding")?;
    ensure!(!agent.trim().is_empty() && !operation.trim().is_empty(), "empty workflow binding");
    let arguments = if args.trim().is_empty() { json!({}) } else { serde_json::from_str(args)? };
    ensure!(arguments.is_object(), "workflow step arguments must be an object");
    Ok((agent.trim().into(), operation.trim().into(), arguments))
}

fn child_success(category: &str, result: &Value) -> Result<bool> {
    let exit = result.get("exit_code").and_then(Value::as_i64);
    if category == "execution" { return Ok(exit.context("execution result omitted exit_code")? == 0) }
    Ok(exit.map(|code| code == 0).or_else(|| result.get("success").and_then(Value::as_bool)).unwrap_or(true))
}

fn transition_status(status: &str, method: &str, in_flight: bool, force: bool) -> Result<&'static str> {
    ensure!(!terminal(status), "workflow execution is terminal");
    match method {
        "pause_workflow" if matches!(status, "queued" | "running" | "paused" | "pause_requested") => Ok(if in_flight { "pause_requested" } else { "paused" }),
        "resume_workflow" if matches!(status, "paused" | "pause_requested") => Ok(if in_flight { "running" } else { "queued" }),
        "cancel_workflow" => { ensure!(!in_flight || force, "in-flight cancellation requires force=true; completed effects cannot be undone"); Ok(if in_flight { "cancel_requested" } else { "cancelled" }) }
        _ => bail!("invalid workflow transition from {status}"),
    }
}

fn summary(row: &WorkflowExecutionRecord, owned: bool) -> Result<WorkflowExecutionSummary> {
    let interrupted = !owned && !terminal(&row.status);
    Ok(WorkflowExecutionSummary { execution_id: row.execution_id.clone(), workflow_id: row.workflow_id.clone(), status: if interrupted { WorkflowExecutionStatus::Interrupted } else { status(&row.status)? }, revision: row.revision,
        accepted_at: row.document["accepted_at"].as_str().context("workflow accepted timestamp missing")?.into(), completed_steps: row.document["completed_steps"].as_u64().context("workflow completed-step counter missing")? as usize, in_flight: row.document["in_flight"].as_bool().context("workflow phase missing")?,
        reason: if interrupted { Some("execution owner unavailable; retained phase may have caused effects; never auto-replayed".into()) } else { row.document["reason"].as_str().map(str::to_owned) },
        episode_committed: row.document["episode_committed"].as_bool().unwrap_or(false), episode_error: row.document["episode_error"].as_str().map(str::to_owned) })
}

fn bounded(text: &str, maximum: usize) -> String {
    let mut end = text.len().min(maximum); while !text.is_char_boundary(end) { end -= 1; } text[..end].to_owned()
}
