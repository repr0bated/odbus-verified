//! Incremental native generation with durable deltas and caller-private SHM.
//! No prompt replay on recovery, cancellation, revocation or native failure.

use std::{collections::HashMap, sync::Arc};
use anyhow::{Context, Result, ensure};
use op_plugins::state_plugins::{chat_router::generation::*, runtime_documents::RuntimeDocuments};
use serde_json::{Value, json};
use tokio::sync::{Mutex, oneshot};
use crate::{
    chat_router_generation::{NativeErrorCode, NativeGenerationError, NativeGenerationEvent, NativeGenerationTerminal},
    core::BackendStateUpdate,
    runtime::{BackendContext, BackendOutcome, Runtime, RuntimeError, VerifiedCaller},
};
use super::{decode, generation_admission, native_client, private_scope, unchanged};

type JobKey = (String, String);
#[derive(Clone)]
pub(super) struct GenerationBackend {
    methods: GenerationMethods,
    active: Arc<Mutex<HashMap<JobKey, Option<oneshot::Sender<()>>>>>,
    operation: Arc<Mutex<()>>,
}

impl GenerationBackend {
    pub(super) fn new(documents: Arc<RuntimeDocuments>) -> Self {
        Self { methods: GenerationMethods::new(documents), active: Arc::new(Mutex::new(HashMap::new())), operation: Arc::new(Mutex::new(())) }
    }

    pub(super) async fn dispatch(&self, ctx: &BackendContext<'_>, method: &str, args: &Value) -> Result<BackendOutcome, RuntimeError> {
        self.dispatch_inner(ctx, method, args).await.map_err(RuntimeError::from)
    }

    async fn dispatch_inner(&self, ctx: &BackendContext<'_>, method: &str, args: &Value) -> Result<BackendOutcome> {
        let owner = GenerationOwner::new(ctx.caller.principal_id.clone(), ctx.caller.session_id.clone())?;
        let _operation = self.operation.lock().await;
        ctx.caller.authority.revalidate().await?;
        if method == "start_generation" {
            let input: StartGenerationInput = decode(args)?;
            input.validate()?;
            let context = ctx.runtime.call_nested(ctx.caller, "project", "get_chat_context", json!({})).await?;
            let mode: op_plugins::state_plugins::project::ChatMode = serde_json::from_value(context.result.get("context").unwrap_or(&context.result)["mode"].clone()).context("committed chat mode is invalid")?;
            let allowed_alias = match mode { op_plugins::state_plugins::project::ChatMode::ControlPlane => "control_plane", op_plugins::state_plugins::project::ChatMode::Accountability => "accountability", _ => "dashboard" };
            ensure!(input.agent_alias == allowed_alias, "generation agent must match the caller's committed chat mode");
            let client = native_client(ctx, allowed_alias).await?;
            let admission = generation_admission(ctx).await?;
            let hash = ctx.runtime.catalog().plugin("chat_router").context("sealed chat_router is absent")?.schema_hash.clone();
            let id = uuid::Uuid::now_v7().to_string();
            let state = self.methods.create(&owner, &input, &id, &hash).await.map_err(RuntimeError::unknown)?;
            let job = state.generation_jobs.get(&id).context("durable generation omitted the created job")?.clone();
            ctx.runtime.publish_backend_state(ctx.caller, "chat_router", BackendStateUpdate::Fields(serde_json::to_value(&state)?), private_scope(ctx.caller)).await?;
            let (cancel, cancellation) = oneshot::channel();
            let key = (owner.selector().to_owned(), id.clone());
            self.active.lock().await.insert(key.clone(), Some(cancel));
            let worker = self.clone();
            let runtime = ctx.runtime.clone();
            let caller = ctx.caller.for_background();
            tokio::spawn(async move {
                let result = worker.run(&runtime, &caller, &owner, &id, client, input, admission, cancellation).await;
                if let Err(error) = result {
                    // Never replay an ambiguous provider effect. Preserve all
                    // committed fragments and record a durable failed terminal.
                    let _operation = worker.operation.lock().await;
                    if let Ok(Some(snapshot)) = worker.methods.snapshot(&owner).await {
                        if snapshot.generation_jobs.get(&id).is_some_and(|job| !job.status.terminal()) {
                            if let Ok(state) = worker.methods.finish(&owner, &id, GenerationStatus::Failed, Some(GenerationFailure::NativeError)).await {
                                if let Err(publication) = publish(&runtime, &caller, &state).await {
                                    tracing::error!(job_id=%id, error=%publication, "durable generation failure publication pending");
                                }
                            }
                        }
                    }
                    tracing::warn!(job_id=%id, error=%error, "native generation ended without replay");
                }
                worker.active.lock().await.remove(&key);
            });
            // State is already committed; a late parent merge must not replace a
            // newer native fragment with this earlier queued observation.
            return Ok(unchanged(ctx, serde_json::to_value(GenerationMutationOutput { document_revision: state.generation_revision, job })?));
        }
        if method == "recover_generation_jobs" {
            ensure!(!self.active.lock().await.keys().any(|(selector, _)| selector == owner.selector()), "cannot recover a generation owned by a live worker");
            let state = self.methods.recover(&owner).await.map_err(RuntimeError::unknown)?;
            let result = serde_json::to_value(GenerationWorkspaceOutput { state: state.clone() })?;
            if let Some(state) = &state { publish(ctx.runtime, ctx.caller, state).await?; }
            return Ok(unchanged(ctx, result));
        }
        if method == "delete_generation_job" {
            let input: GenerationRevisionInput = decode(args)?;
            let state = self.methods.delete(&owner, &input.job_id, input.expected_revision).await.map_err(RuntimeError::unknown)?;
            publish(ctx.runtime, ctx.caller, &state).await?;
            return Ok(unchanged(ctx, serde_json::to_value(GenerationDeleteOutput { document_revision: state.generation_revision, job_id: input.job_id, deleted_revision: input.expected_revision })?));
        }
        if method == "cancel_generation" {
            let input: GenerationRevisionInput = decode(args)?;
            let key = (owner.selector().to_owned(), input.job_id.clone());
            ensure!(self.active.lock().await.contains_key(&key), "generation has no live worker; recover interrupted jobs instead");
            let before = self.methods.snapshot(&owner).await?.context("generation document disappeared")?;
            let admitted = before.generation_jobs.get(&input.job_id).context("generation job is absent")?.status != GenerationStatus::Queued;
            let state = self.methods.request_cancel(&owner, &input.job_id, input.expected_revision).await.map_err(RuntimeError::unknown)?;
            publish(ctx.runtime, ctx.caller, &state).await?;
            if let Some(cancel) = self.active.lock().await.get_mut(&key).and_then(Option::take) { let _ = cancel.send(()); }
            if !admitted {
                let job = state.generation_jobs.get(&input.job_id).context("cancelled generation job disappeared")?.clone();
                return Ok(unchanged(ctx, serde_json::to_value(GenerationMutationOutput { document_revision: state.generation_revision, job })?));
            }
            return Err(RuntimeError::unknown(anyhow::anyhow!("cancellation requested after native generation admission; provider outcome is unknown and must not be replayed")).into());
        }
        let state = self.methods.snapshot(&owner).await?;
        let result = match method {
            "get_generation_jobs" => serde_json::to_value(GenerationWorkspaceOutput { state: state.clone() })?,
            "get_generation_job" => {
                let input: GenerationJobInput = decode(args)?;
                validate_id(&input.job_id)?;
                serde_json::to_value(GenerationJobOutput { document_revision: state.as_ref().map_or(0, |state| state.generation_revision), job: state.as_ref().and_then(|state| state.generation_jobs.get(&input.job_id)).cloned() })?
            }
            _ => anyhow::bail!("unknown chat_router generation method {method}"),
        };
        if let Some(state) = &state { publish(ctx.runtime, ctx.caller, state).await?; }
        Ok(unchanged(ctx, result))
    }

    #[allow(clippy::too_many_arguments)]
    async fn run(&self, runtime: &Runtime, caller: &VerifiedCaller, owner: &GenerationOwner, id: &str, client: crate::chat_router_runtime::ChatRouterRuntimeClient, input: StartGenerationInput, admission: Arc<dyn crate::chat_router_generation::NativeGenerationAdmission>, mut cancellation: oneshot::Receiver<()>) -> Result<()> {
        let _selection = crate::chat_router_runtime::ChatRouterRuntimeClient::selection_guard().await;
        caller.authority.revalidate().await?;
        runtime.authorize_nested(caller, "chat_router", "start_generation").await?;
        {
            let _operation = self.operation.lock().await;
            let current = self.methods.snapshot(owner).await?.context("generation document disappeared")?;
            if current.generation_jobs.get(id).is_some_and(|job| job.status == GenerationStatus::CancelRequested) {
                let state = self.methods.finish(owner, id, GenerationStatus::Cancelled, None).await?;
                publish(runtime, caller, &state).await?;
                return Ok(());
            }
            let state = self.methods.running(owner, id).await?;
            publish(runtime, caller, &state).await?;
        }
        let mut task = client.spawn_stateless_generation(input, uuid::Uuid::new_v4().to_string(), admission)?;
        let mut cancel_requested = false;
        loop {
            tokio::select! {
                _ = &mut cancellation, if !cancel_requested => {
                    cancel_requested = true;
                    task.cancel();
                }
                event = task.recv_event() => {
                    let Some(NativeGenerationEvent::TextDelta { delta }) = event else { break; };
                    let _operation = self.operation.lock().await;
                    let current = self.methods.snapshot(owner).await?.context("generation document disappeared")?;
                    if current.generation_jobs.get(id).is_some_and(|job| job.status == GenerationStatus::CancelRequested) {
                        cancel_requested = true;
                        task.cancel();
                        continue;
                    }
                    runtime.authorize_nested(caller, "chat_router", "start_generation").await?;
                    let state = self.methods.append(owner, id, delta).await?;
                    publish(runtime, caller, &state).await?;
                }
            }
        }
        let terminal = task.wait().await;
        let _operation = self.operation.lock().await;
        let current = self.methods.snapshot(owner).await?.context("generation document disappeared")?;
        cancel_requested |= current.generation_jobs.get(id).is_some_and(|job| job.status == GenerationStatus::CancelRequested);
        let state = match terminal {
            Ok(NativeGenerationTerminal::Done(done)) if !cancel_requested => self.methods.complete(owner, id, done.full_response, done.provider, done.model).await?,
            Ok(NativeGenerationTerminal::Cancelled) => self.methods.finish(owner, id, GenerationStatus::Cancelled, None).await?,
            Ok(NativeGenerationTerminal::Done(_)) => self.methods.finish(owner, id, GenerationStatus::Interrupted, Some(GenerationFailure::NativeAborted)).await?,
            Err(error) => {
                let reason = match error {
                    NativeGenerationError::Timeout => GenerationFailure::Deadline,
                    NativeGenerationError::Cancelled => GenerationFailure::NativeAborted,
                    NativeGenerationError::Transport => GenerationFailure::NativeDisconnected,
                    NativeGenerationError::Terminal(NativeErrorCode::Auth) => GenerationFailure::AdmissionWithdrawn,
                    NativeGenerationError::Terminal(NativeErrorCode::ApprovalRequired) => GenerationFailure::ToolApprovalRequired,
                    _ => GenerationFailure::NativeError,
                };
                self.methods.finish(owner, id, GenerationStatus::Failed, Some(reason)).await?
            }
        };
        publish(runtime, caller, &state).await
    }
}

async fn publish(runtime: &Runtime, caller: &VerifiedCaller, state: &GenerationState) -> Result<()> {
    runtime.publish_backend_state(caller, "chat_router", BackendStateUpdate::Fields(serde_json::to_value(state)?), private_scope(caller)).await?;
    Ok(())
}
