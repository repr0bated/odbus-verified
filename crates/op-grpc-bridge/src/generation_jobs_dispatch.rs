//! MutationEngine-owned private generation publication. The only value source
//! exposed to readers is the complete committed owner SHM document. Native
//! chunks are inputs to durable transitions, never an alternate stream/cache.
use crate::{
    generation_admission::{GenerationCaller, GenerationOrigin, GenerationPins, PLUGIN},
    mutation_engine::MutationEngine,
    tched_router_generation_native::{
        NativeErrorCode, NativeGenerationAdmission, NativeGenerationError, NativeGenerationEvent,
        NativeGenerationTask, NativeGenerationTerminal,
    },
};
use anyhow::{Context, Result};
use futures::future::BoxFuture;
use op_core::projection_shm::{
    PrivateDocumentCommit, PrivateDocumentPublication, PrivateProjectionDirectory,
};
use op_plugins::state_plugins::{runtime_documents::RuntimeDocuments, tched_router::generation::*};
use op_state_store::PluginSchema;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex,
    },
};
use tokio::sync::{broadcast, watch, Mutex};

const MAX_OWNERS: usize = 256;
const MAX_OWNER_CALLS: usize = 32;
#[cfg(test)]
#[path = "generation_jobs_tests.rs"]
mod tests;
pub(crate) fn is_generation_method(method: &str) -> bool {
    op_plugins::state_plugins::tched_router::generation::is_method(method)
}

/// Production implements this over the existing configured native router
/// client. Fixtures inject the same native WS transport at a temporary server.
pub(crate) trait GenerationBackend: Send + Sync {
    fn spawn(
        &self,
        input: StartGenerationInput,
        native_session: String,
        gate: Arc<dyn NativeGenerationAdmission>,
    ) -> Result<NativeGenerationTask>;
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct GenerationWake {
    pub generation: u64,
    pub document_revision: Option<u64>,
}
struct OwnerSlot {
    operation: Arc<Mutex<()>>,
    calls: Arc<tokio::sync::Semaphore>,
    activated: AtomicBool,
    wakes: broadcast::Sender<GenerationWake>,
    workers: StdMutex<HashMap<String, watch::Sender<bool>>>,
    worker_finished: tokio::sync::Notify,
}
struct WorkerRegistration {
    slot: Arc<OwnerSlot>,
    id: String,
}
impl Drop for WorkerRegistration {
    fn drop(&mut self) {
        if let Ok(mut workers) = self.slot.workers.lock() {
            workers.remove(&self.id);
        }
        self.slot.worker_finished.notify_waiters();
    }
}
impl OwnerSlot {
    fn new() -> Self {
        let (wakes, _) = broadcast::channel(32);
        Self {
            operation: Arc::new(Mutex::new(())),
            calls: Arc::new(tokio::sync::Semaphore::new(MAX_OWNER_CALLS)),
            activated: AtomicBool::new(false),
            wakes,
            workers: StdMutex::new(HashMap::new()),
            worker_finished: tokio::sync::Notify::new(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct GenerationPublisher {
    root: PrivateProjectionDirectory,
    methods: GenerationMethods,
    catalog: PathBuf,
    backend: Arc<dyn GenerationBackend>,
    owners: Arc<StdMutex<HashMap<String, Arc<OwnerSlot>>>>,
}

#[derive(Clone)]
enum Action {
    Recover,
    Create {
        input: StartGenerationInput,
        id: String,
    },
    Running(String),
    Append {
        id: String,
        fragments: Vec<String>,
    },
    Complete {
        id: String,
        output: String,
        provider: Option<String>,
        model: Option<String>,
    },
    Cancel {
        id: String,
        revision: u64,
    },
    Finish {
        id: String,
        status: GenerationStatus,
        reason: Option<GenerationFailure>,
    },
    Delete {
        id: String,
        revision: u64,
    },
}
impl Action {
    async fn apply(
        self,
        methods: &GenerationMethods,
        owner: &GenerationOwner,
        hash: &str,
    ) -> Result<Option<GenerationState>> {
        let state = match self {
            Self::Recover => return methods.recover(owner).await,
            Self::Create { input, id } => methods.create(owner, &input, &id, hash).await?,
            Self::Running(id) => methods.running(owner, &id).await?,
            Self::Append { id, fragments } => methods.append_batch(owner, &id, fragments).await?,
            Self::Complete {
                id,
                output,
                provider,
                model,
            } => {
                methods
                    .complete(owner, &id, output, provider, model)
                    .await?
            }
            Self::Cancel { id, revision } => methods.request_cancel(owner, &id, revision).await?,
            Self::Finish { id, status, reason } => {
                methods.finish(owner, &id, status, reason).await?
            }
            Self::Delete { id, revision } => methods.delete(owner, &id, revision).await?,
        };
        Ok(Some(state))
    }
}

#[derive(Clone)]
pub(crate) struct CommittedGeneration {
    pub generation: u64,
    pub schema_hash: String,
    pub schema_json: String,
    pub catalog_hash: String,
    pub state: Option<GenerationState>,
}
impl CommittedGeneration {
    pub fn plugin_state(&self) -> Result<Option<Value>> {
        self.state
            .as_ref()
            .map(GenerationState::plugin_state)
            .transpose()
    }
}
pub(crate) struct GenerationSubscription {
    caller: GenerationCaller,
    owner: GenerationOwner,
    pins: GenerationPins,
    slot: Arc<OwnerSlot>,
    pub receiver: broadcast::Receiver<GenerationWake>,
    pub initial: CommittedGeneration,
}

fn validate_state(pins: &GenerationPins, state: &GenerationState) -> Result<Vec<u8>> {
    let value = state.plugin_state()?;
    GenerationState::from_plugin_state(&value)?;
    let owned = simd_json::serde::to_owned_value(value)?;
    let validation = pins.document.schema.validate(&owned);
    anyhow::ensure!(
        validation.valid && validation.warnings.is_empty(),
        "generation projection does not match its pinned state contract"
    );
    pins.document
        .schema
        .validate_exact_schema_fields(&owned)
        .map_err(|_| anyhow::anyhow!("generation child contract failed"))?;
    Ok(simd_json::to_vec(&owned)?)
}
fn decode_state(bytes: &[u8], pins: &GenerationPins) -> Result<GenerationState> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| anyhow::anyhow!("invalid committed generation JSON"))?;
    let state = GenerationState::from_plugin_state(&value)?;
    validate_state(pins, &state)?;
    Ok(state)
}

impl GenerationPublisher {
    pub(crate) fn new(
        root: PrivateProjectionDirectory,
        documents: Arc<RuntimeDocuments>,
        catalog: PathBuf,
        backend: Arc<dyn GenerationBackend>,
    ) -> Self {
        Self {
            root,
            methods: GenerationMethods::new(documents),
            catalog,
            backend,
            owners: Arc::new(StdMutex::new(HashMap::new())),
        }
    }
    fn slot(&self, owner: &GenerationOwner) -> Result<Arc<OwnerSlot>> {
        let mut slots = self
            .owners
            .lock()
            .map_err(|_| anyhow::anyhow!("generation owner table unavailable"))?;
        if let Some(slot) = slots.get(owner.selector()) {
            return Ok(slot.clone());
        }
        anyhow::ensure!(
            slots.len() < MAX_OWNERS,
            "generation live owner capacity reached"
        );
        let slot = Arc::new(OwnerSlot::new());
        slots.insert(owner.selector().into(), slot.clone());
        Ok(slot)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn dispatch(
        &self,
        engine: &MutationEngine,
        admitted: &PluginSchema,
        method: &str,
        args: &Value,
        actor: &str,
        session: Option<&str>,
        genesis: Option<&str>,
        origin: Option<GenerationOrigin>,
    ) -> Result<Value> {
        anyhow::ensure!(is_generation_method(method), "unknown generation method");
        let caller = GenerationCaller {
            actor: actor.into(),
            session: session
                .context("generation requires verified session")?
                .into(),
            genesis: genesis
                .context("generation requires verified genesis")?
                .into(),
        };
        let capability = admitted
            .methods
            .get(method)
            .and_then(|m| m.required_capability.as_ref())
            .context("generation capability is absent")?
            .clone();
        let owner = caller.verify(engine, &capability).await?;
        let pins =
            GenerationPins::load(&self.catalog, Some(admitted), method, &capability, origin)?;
        let slot = self.slot(&owner)?;
        let permit = slot
            .calls
            .clone()
            .try_acquire_owned()
            .map_err(|_| anyhow::anyhow!("generation owner call queue is full"))?;
        let publisher = self.clone();
        let engine = engine.clone();
        let method = method.to_owned();
        let args = args.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _guard = slot.operation.clone().lock_owned().await;
            pins.check(&engine, &caller, &method, &capability).await?;
            publisher
                .dispatch_locked(
                    &engine,
                    &caller,
                    &owner,
                    &pins,
                    &slot,
                    &method,
                    &capability,
                    args,
                )
                .await
        })
        .await
        .map_err(|_| anyhow::anyhow!("generation dispatch owner failed"))?
    }

    #[allow(clippy::too_many_arguments)]
    async fn dispatch_locked(
        &self,
        engine: &MutationEngine,
        caller: &GenerationCaller,
        owner: &GenerationOwner,
        pins: &GenerationPins,
        slot: &Arc<OwnerSlot>,
        method: &str,
        capability: &str,
        args: Value,
    ) -> Result<Value> {
        if method == "recover_generation_jobs" {
            let _: GenerationEmptyInput = serde_json::from_value(args)
                .map_err(|_| anyhow::anyhow!("invalid generation recovery input"))?;
            anyhow::ensure!(
                slot.workers
                    .lock()
                    .map_err(|_| anyhow::anyhow!("generation workers unavailable"))?
                    .is_empty(),
                "cancel active generation workers before recovery"
            );
            let receipt = self
                .commit_locked(
                    engine,
                    caller,
                    owner,
                    pins,
                    slot,
                    method,
                    capability,
                    Action::Recover,
                    false,
                )
                .await?;
            return Ok(serde_json::to_value(GenerationWorkspaceOutput {
                state: receipt.state,
            })?);
        }
        anyhow::ensure!(
            slot.activated.load(Ordering::Acquire),
            "explicit generation recovery is required"
        );
        let current = self.read_locked(owner, pins)?;
        pins.check(engine, caller, method, capability).await?;
        match method {
            "get_generation_jobs" => {
                let _: GenerationEmptyInput = serde_json::from_value(args)
                    .map_err(|_| anyhow::anyhow!("invalid generation read input"))?;
                Ok(serde_json::to_value(GenerationWorkspaceOutput {
                    state: current.state,
                })?)
            }
            "get_generation_job" => {
                let input: GenerationJobInput = serde_json::from_value(args)
                    .map_err(|_| anyhow::anyhow!("invalid generation job input"))?;
                validate_id(&input.job_id)?;
                let revision = current.state.as_ref().map_or(0, |s| s.generation_revision);
                let job = current
                    .state
                    .and_then(|mut state| state.generation_jobs.remove(&input.job_id));
                Ok(serde_json::to_value(GenerationJobOutput {
                    document_revision: revision,
                    job,
                })?)
            }
            "start_generation" => {
                let input: StartGenerationInput = serde_json::from_value(args)
                    .map_err(|_| anyhow::anyhow!("invalid generation start input"))?;
                input.validate()?;
                if let Some(state) = &current.state {
                    anyhow::ensure!(
                        state.generation_jobs.len() < MAX_JOBS
                            && state
                                .generation_jobs
                                .values()
                                .filter(|j| !j.status.terminal())
                                .count()
                                < MAX_ACTIVE_JOBS,
                        "generation job capacity reached"
                    );
                }
                pins.check(engine, caller, method, capability).await?;
                let id = uuid::Uuid::new_v4().to_string();
                let receipt = self
                    .commit_locked(
                        engine,
                        caller,
                        owner,
                        pins,
                        slot,
                        method,
                        capability,
                        Action::Create {
                            input: input.clone(),
                            id: id.clone(),
                        },
                        false,
                    )
                    .await?;
                let output = mutation_output(&receipt, &id)?;
                let (cancel, cancel_rx) = watch::channel(false);
                slot.workers
                    .lock()
                    .map_err(|_| anyhow::anyhow!("generation workers unavailable"))?
                    .insert(id.clone(), cancel);
                self.spawn_worker(
                    engine.clone(),
                    caller.clone(),
                    owner.clone(),
                    pins.clone(),
                    slot.clone(),
                    input,
                    id,
                    cancel_rx,
                );
                Ok(output)
            }
            "cancel_generation" | "delete_generation_job" => {
                let input: GenerationRevisionInput = serde_json::from_value(args)
                    .map_err(|_| anyhow::anyhow!("invalid generation revision input"))?;
                validate_id(&input.job_id)?;
                let job = current
                    .state
                    .as_ref()
                    .and_then(|state| state.generation_jobs.get(&input.job_id))
                    .context("generation job is absent")?;
                anyhow::ensure!(
                    input.expected_revision > 0 && job.revision == input.expected_revision,
                    "generation job revision conflict"
                );
                if method == "cancel_generation" {
                    anyhow::ensure!(!job.status.terminal(), "generation job is already terminal");
                    anyhow::ensure!(
                        slot.workers
                            .lock()
                            .map_err(|_| anyhow::anyhow!("generation workers unavailable"))?
                            .contains_key(&input.job_id),
                        "generation worker is absent; explicit recovery is required"
                    );
                    let receipt = self
                        .commit_locked(
                            engine,
                            caller,
                            owner,
                            pins,
                            slot,
                            method,
                            capability,
                            Action::Cancel {
                                id: input.job_id.clone(),
                                revision: input.expected_revision,
                            },
                            false,
                        )
                        .await?;
                    if let Some(cancel) = slot
                        .workers
                        .lock()
                        .map_err(|_| anyhow::anyhow!("generation workers unavailable"))?
                        .get(&input.job_id)
                    {
                        let _ = cancel.send(true);
                    }
                    mutation_output(&receipt, &input.job_id)
                } else {
                    anyhow::ensure!(
                        job.status.terminal(),
                        "active generation job cannot be deleted"
                    );
                    let receipt = self
                        .commit_locked(
                            engine,
                            caller,
                            owner,
                            pins,
                            slot,
                            method,
                            capability,
                            Action::Delete {
                                id: input.job_id.clone(),
                                revision: input.expected_revision,
                            },
                            false,
                        )
                        .await?;
                    Ok(serde_json::to_value(GenerationDeleteOutput {
                        document_revision: receipt
                            .state
                            .context("generation deletion receipt missing")?
                            .generation_revision,
                        job_id: input.job_id,
                        deleted_revision: input.expected_revision,
                    })?)
                }
            }
            _ => anyhow::bail!("unknown generation method"),
        }
    }

    fn read_locked(
        &self,
        owner: &GenerationOwner,
        pins: &GenerationPins,
    ) -> Result<CommittedGeneration> {
        let directory = self
            .root
            .open_child(owner.selector())?
            .context("generation activation is incomplete")?;
        anyhow::ensure!(
            directory.pending_document()?.is_none(),
            "generation publication awaits explicit recovery"
        );
        let snapshot = directory.read_snapshot(&[PLUGIN.into()])?;
        let state = snapshot
            .states
            .get(PLUGIN)
            .map(|bytes| decode_state(bytes, pins))
            .transpose()?;
        let catalog_hash = pins.check_catalog("get_generation_jobs", READ_CAPABILITY)?;
        Ok(CommittedGeneration {
            generation: snapshot.generation,
            schema_hash: pins.document.schema_hash.clone(),
            schema_json: pins.document.blob.schema_json.clone(),
            catalog_hash,
            state,
        })
    }

    /// Caller owns slot.operation for the entire commit -> activation -> wake
    /// sequence. The outer task is owned, so dropping a client cannot strand a
    /// successful durable effect before publication/notification.
    #[allow(clippy::too_many_arguments)]
    async fn commit_locked(
        &self,
        engine: &MutationEngine,
        caller: &GenerationCaller,
        owner: &GenerationOwner,
        pins: &GenerationPins,
        slot: &OwnerSlot,
        method: &str,
        capability: &str,
        action: Action,
        withdrawn: bool,
    ) -> Result<CommittedGeneration> {
        let root = self.root.clone();
        let methods = self.methods.clone();
        let owner = owner.clone();
        let pins = pins.clone();
        let engine = engine.clone();
        let caller = caller.clone();
        let method = method.to_owned();
        let capability = capability.to_owned();
        let runtime = tokio::runtime::Handle::current();
        let operation = uuid::Uuid::new_v4().to_string();
        let recovery = matches!(action, Action::Recover);
        let receipt_pins = pins.clone();
        let result=tokio::task::spawn_blocking(move || -> Result<_> {
            let directory=root.prepare_child(owner.selector())?;
            // One-way order: generation owner/file lock -> canonical identity
            // read lock. Identity writers do not acquire generation locks or
            // join workers; the same-namespace recursive read is forbidden.
            directory.ensure_distinct_from(std::path::Path::new(&op_core::projection_shm::shm_state_dir()))?;
            let apply=|previous:Option<&[u8]>|->Result<PrivateDocumentCommit<Option<GenerationState>>> {
                if !withdrawn {runtime.block_on(pins.check(&engine,&caller,&method,&capability))?;}
                let state=runtime.block_on(action.apply(&methods,&owner,&pins.document.schema_hash))?;
                let prepared=(||->Result<Option<Vec<u8>>> {
                    anyhow::ensure!(!withdrawn,"generation admission was withdrawn");
                    runtime.block_on(pins.check(&engine,&caller,&method,&capability))?;
                    let bytes=state.as_ref().map(|state|validate_state(&pins,state)).transpose()?;
                    if let (Some(previous),Some(state))=(previous,state.as_ref()) {
                        let previous=decode_state(previous,&pins)?;
                        anyhow::ensure!(state.generation_revision>=previous.generation_revision,"generation durable revision regressed");
                        anyhow::ensure!(state.generation_revision!=previous.generation_revision || state==&previous,"generation values changed without a revision");
                    }
                    Ok(bytes)
                })();
                Ok(match prepared {Ok(current)=>PrivateDocumentCommit::Publish{current,durable:state},Err(_)=>PrivateDocumentCommit::RetainPending{durable:state,error:anyhow::anyhow!("generation post-durable admission or projection validation failed")}})
            };
            if recovery
                && let Some(pending)=directory.pending_document()? {
                    anyhow::ensure!(pending.plugin_id==PLUGIN,"generation pending owner mismatch");
                    return directory.recover_document_with_decision(PLUGIN,&pending.operation_id,||apply(None));
                }
            directory.commit_document_durable_with_decision(PLUGIN,&operation,apply)
        }).await.map_err(|_|anyhow::anyhow!("generation publication worker failed"));
        let result = match result {
            Ok(Ok(result)) => result,
            _ => {
                slot.activated.store(false, Ordering::Release);
                anyhow::bail!("generation publication failed; explicit recovery is required");
            }
        };
        match result {
            PrivateDocumentPublication::Committed {
                projection,
                durable,
            } => {
                slot.activated.store(true, Ordering::Release);
                let receipt = CommittedGeneration {
                    generation: projection.generation,
                    schema_hash: receipt_pins.document.schema_hash.clone(),
                    schema_json: receipt_pins.document.blob.schema_json.clone(),
                    catalog_hash: receipt_pins.catalog_hash.clone(),
                    state: durable,
                };
                if projection.previous != projection.current {
                    let _ = slot.wakes.send(GenerationWake {
                        generation: receipt.generation,
                        document_revision: receipt.state.as_ref().map(|s| s.generation_revision),
                    });
                }
                Ok(receipt)
            }
            PrivateDocumentPublication::Pending {
                operation_id,
                durable,
                ..
            } => {
                slot.activated.store(false, Ordering::Release);
                let revision = durable.flatten().map(|state| state.generation_revision);
                anyhow::bail!("generation_publication_pending operation_id={operation_id} document_revision={revision:?}")
            }
        }
    }
}

fn mutation_output(receipt: &CommittedGeneration, id: &str) -> Result<Value> {
    let state = receipt
        .state
        .as_ref()
        .context("generation mutation receipt missing")?;
    let job = state
        .generation_jobs
        .get(id)
        .context("generation job receipt missing")?
        .clone();
    Ok(serde_json::to_value(GenerationMutationOutput {
        document_revision: state.generation_revision,
        job,
    })?)
}

/// Collect only fragments already available now. No await, timer or fill wait.
/// At most one non-fitting fragment is retained ahead of the native queue; it
/// must be processed before any later fragment or the terminal result. A valid
/// prefix commits before an over-limit successor is rejected on the next turn.
fn ready_fragment_batch(
    current: &GenerationState,
    id: &str,
    first: String,
    carry: &mut Option<String>,
    mut next_ready: impl FnMut() -> Option<String>,
) -> Result<Vec<String>> {
    anyhow::ensure!(carry.is_none(), "generation fragment carry is occupied");
    current.preflight_append(id, &first)?;
    let mut fragments = vec![first];
    // Count dequeue attempts as well as nonempty fragments, so an empty-frame
    // burst cannot turn this synchronous section into an unbounded loop.
    for _ in 1..MAX_BATCH_FRAGMENTS {
        let Some(fragment) = next_ready() else { break };
        if fragment.is_empty() {
            continue;
        }
        fragments.push(fragment);
        if current.preflight_append_batch(id, &fragments).is_err() {
            *carry = fragments.pop();
            break;
        }
    }
    Ok(fragments)
}

struct PromptAdmission {
    publisher: GenerationPublisher,
    engine: MutationEngine,
    caller: GenerationCaller,
    pins: GenerationPins,
    slot: Arc<OwnerSlot>,
    owner: GenerationOwner,
    job_id: String,
    sealed_identity: String,
}
impl NativeGenerationAdmission for PromptAdmission {
    fn sealed_identity(&self) -> Option<&str> {
        Some(&self.sealed_identity)
    }

    fn check(&self) -> BoxFuture<'_, std::result::Result<(), NativeGenerationError>> {
        Box::pin(async move {
            let _guard = self.slot.operation.lock().await;
            let admitted = async {
                self.pins
                    .check(
                        &self.engine,
                        &self.caller,
                        "start_generation",
                        START_CAPABILITY,
                    )
                    .await?;
                anyhow::ensure!(
                    self.slot.activated.load(Ordering::Acquire),
                    "generation is unavailable"
                );
                let state = self
                    .publisher
                    .read_locked(&self.owner, &self.pins)?
                    .state
                    .context("generation document absent")?;
                anyhow::ensure!(
                    state
                        .generation_jobs
                        .get(&self.job_id)
                        .is_some_and(|job| job.status == GenerationStatus::Running),
                    "generation was cancelled before prompt"
                );
                self.pins
                    .check(
                        &self.engine,
                        &self.caller,
                        "start_generation",
                        START_CAPABILITY,
                    )
                    .await?;
                Ok::<_, anyhow::Error>(())
            }
            .await;
            admitted.map_err(|_| NativeGenerationError::Terminal(NativeErrorCode::Auth))
        })
    }
}

impl GenerationPublisher {
    #[allow(clippy::too_many_arguments)]
    fn spawn_worker(
        &self,
        engine: MutationEngine,
        caller: GenerationCaller,
        owner: GenerationOwner,
        pins: GenerationPins,
        slot: Arc<OwnerSlot>,
        input: StartGenerationInput,
        id: String,
        cancel: watch::Receiver<bool>,
    ) {
        let publisher = self.clone();
        tokio::spawn(async move {
            let _registration = WorkerRegistration {
                slot: slot.clone(),
                id: id.clone(),
            };
            let result = publisher
                .run_worker(&engine, &caller, &owner, &pins, &slot, input, &id, cancel)
                .await;
            // Safe metadata only: no prompt/output, native diagnostic, owner ID
            // or credentials. Publication failures retain their pending gate.
            if result.is_err() {
                tracing::warn!(plugin=PLUGIN,method="start_generation",job_id=%id,"generation worker stopped; committed state or explicit recovery determines outcome");
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_worker(
        &self,
        engine: &MutationEngine,
        caller: &GenerationCaller,
        owner: &GenerationOwner,
        pins: &GenerationPins,
        slot: &Arc<OwnerSlot>,
        input: StartGenerationInput,
        id: &str,
        mut cancel: watch::Receiver<bool>,
    ) -> Result<()> {
        let sealed_identity = crate::native_sled_identity::credential_for(
            &caller.actor,
            &caller.session,
            &caller.genesis,
        )?;
        let mut changes = engine.change_tx().subscribe();
        {
            let _guard = slot.operation.lock().await;
            if *cancel.borrow() {
                self.commit_locked(
                    engine,
                    caller,
                    owner,
                    pins,
                    slot,
                    "start_generation",
                    START_CAPABILITY,
                    Action::Finish {
                        id: id.into(),
                        status: GenerationStatus::Cancelled,
                        reason: None,
                    },
                    false,
                )
                .await?;
                return Ok(());
            }
            if pins
                .check(engine, caller, "start_generation", START_CAPABILITY)
                .await
                .is_err()
            {
                return self
                    .withdraw_locked(engine, caller, owner, pins, slot, id)
                    .await;
            }
            self.commit_locked(
                engine,
                caller,
                owner,
                pins,
                slot,
                "start_generation",
                START_CAPABILITY,
                Action::Running(id.into()),
                false,
            )
            .await?;
        }
        let gate = Arc::new(PromptAdmission {
            publisher: self.clone(),
            engine: engine.clone(),
            caller: caller.clone(),
            pins: pins.clone(),
            slot: slot.clone(),
            owner: owner.clone(),
            job_id: id.into(),
            sealed_identity,
        });
        // Never accept a caller-supplied native session or reuse a conversation.
        let native_session = uuid::Uuid::new_v4().to_string();
        let mut native = match self.backend.spawn(input, native_session, gate) {
            Ok(native) => native,
            Err(_) => {
                let _guard = slot.operation.lock().await;
                self.commit_locked(
                    engine,
                    caller,
                    owner,
                    pins,
                    slot,
                    "start_generation",
                    START_CAPABILITY,
                    Action::Finish {
                        id: id.into(),
                        status: GenerationStatus::Failed,
                        reason: Some(GenerationFailure::NativeError),
                    },
                    false,
                )
                .await?;
                return Ok(());
            }
        };
        let mut carry = None;
        loop {
            enum Wake {
                Chunk(Option<NativeGenerationEvent>),
                Cancel,
                Revalidate,
            }
            let wake = if let Some(delta) = carry.take() {
                Wake::Chunk(Some(NativeGenerationEvent::TextDelta { delta }))
            } else {
                tokio::select! {
                    event=native.recv_event()=>Wake::Chunk(event),
                    changed=cancel.changed()=>{let _=changed; Wake::Cancel},
                    change=changes.recv()=>match change {
                        Ok(change) if change.plugin_id=="identity_sled" || change.plugin_id=="human_principal" || change.change_type==crate::mutation_engine::ChangeType::SchemaMigration=>Wake::Revalidate,
                        Err(broadcast::error::RecvError::Lagged(_))=>Wake::Revalidate,
                        _=>continue,
                    },
                }
            };
            let _guard = slot.operation.lock().await;
            if pins
                .check(engine, caller, "start_generation", START_CAPABILITY)
                .await
                .is_err()
            {
                native.cancel();
                drop(native);
                return self
                    .withdraw_locked(engine, caller, owner, pins, slot, id)
                    .await;
            }
            if !slot.activated.load(Ordering::Acquire) {
                native.cancel();
                anyhow::bail!("generation publication is pending");
            }
            match wake {
                Wake::Cancel => {
                    native.cancel();
                    self.commit_locked(
                        engine,
                        caller,
                        owner,
                        pins,
                        slot,
                        "start_generation",
                        START_CAPABILITY,
                        Action::Finish {
                            id: id.into(),
                            status: GenerationStatus::Cancelled,
                            reason: None,
                        },
                        false,
                    )
                    .await?;
                    // Cancelled describes the local job term. It does not
                    // claim provider rollback or a nonexistent native task ID.
                    return Ok(());
                }
                Wake::Revalidate => {}
                Wake::Chunk(Some(NativeGenerationEvent::TextDelta { delta })) => {
                    if *cancel.borrow() {
                        continue;
                    }
                    if delta.is_empty() {
                        continue;
                    }
                    let current = self
                        .read_locked(owner, pins)?
                        .state
                        .context("generation state absent")?;
                    let job = current
                        .generation_jobs
                        .get(id)
                        .context("generation job absent")?;
                    if job.status == GenerationStatus::CancelRequested {
                        continue;
                    }
                    let fragments = ready_fragment_batch(&current, id, delta, &mut carry, || {
                        match native.try_recv_event() {
                            Ok(NativeGenerationEvent::TextDelta { delta }) => Some(delta),
                            Err(
                                tokio::sync::mpsc::error::TryRecvError::Empty
                                | tokio::sync::mpsc::error::TryRecvError::Disconnected,
                            ) => None,
                        }
                    });
                    let fragments = match fragments {
                        Ok(fragments) => fragments,
                        Err(_) => {
                            native.cancel();
                            self.commit_locked(
                                engine,
                                caller,
                                owner,
                                pins,
                                slot,
                                "start_generation",
                                START_CAPABILITY,
                                Action::Finish {
                                    id: id.into(),
                                    status: GenerationStatus::Failed,
                                    reason: Some(GenerationFailure::OutputLimit),
                                },
                                false,
                            )
                            .await?;
                            return Ok(());
                        }
                    };
                    self.commit_locked(
                        engine,
                        caller,
                        owner,
                        pins,
                        slot,
                        "start_generation",
                        START_CAPABILITY,
                        Action::Append {
                            id: id.into(),
                            fragments,
                        },
                        false,
                    )
                    .await?;
                }
                Wake::Chunk(None) => {
                    // The adapter closes its event sender only after its
                    // terminal result; all preceding chunks are committed.
                    let terminal = native.wait().await;
                    let action = if *cancel.borrow() {
                        Action::Finish {
                            id: id.into(),
                            status: GenerationStatus::Cancelled,
                            reason: None,
                        }
                    } else {
                        match terminal {
                            Ok(NativeGenerationTerminal::Done(done)) => Action::Complete {
                                id: id.into(),
                                output: done.full_response,
                                provider: done.provider,
                                model: done.model,
                            },
                            Ok(NativeGenerationTerminal::Cancelled) => Action::Finish {
                                id: id.into(),
                                status: GenerationStatus::Failed,
                                reason: Some(GenerationFailure::NativeAborted),
                            },
                            Err(NativeGenerationError::Terminal(code)) => Action::Finish {
                                id: id.into(),
                                status: GenerationStatus::Failed,
                                reason: Some(native_failure(code)),
                            },
                            Err(NativeGenerationError::Timeout) => Action::Finish {
                                id: id.into(),
                                status: GenerationStatus::Failed,
                                reason: Some(GenerationFailure::Deadline),
                            },
                            Err(NativeGenerationError::Cancelled) => Action::Finish {
                                id: id.into(),
                                status: GenerationStatus::Failed,
                                reason: Some(GenerationFailure::NativeAborted),
                            },
                            Err(_) => Action::Finish {
                                id: id.into(),
                                status: GenerationStatus::Failed,
                                reason: Some(GenerationFailure::NativeDisconnected),
                            },
                        }
                    };
                    self.commit_locked(
                        engine,
                        caller,
                        owner,
                        pins,
                        slot,
                        "start_generation",
                        START_CAPABILITY,
                        action,
                        false,
                    )
                    .await?;
                    return Ok(());
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn withdraw_locked(
        &self,
        engine: &MutationEngine,
        caller: &GenerationCaller,
        owner: &GenerationOwner,
        pins: &GenerationPins,
        slot: &OwnerSlot,
        id: &str,
    ) -> Result<()> {
        // No new native action is authorized here. Finish the accepted worker's
        // durable term under the pending gate, deliberately withhold SHM and
        // wakes, and require current authenticated recovery before future reads.
        self.commit_locked(
            engine,
            caller,
            owner,
            pins,
            slot,
            "start_generation",
            START_CAPABILITY,
            Action::Finish {
                id: id.into(),
                status: GenerationStatus::Interrupted,
                reason: Some(GenerationFailure::AdmissionWithdrawn),
            },
            true,
        )
        .await?;
        anyhow::bail!("generation admission withdrawn")
    }

    pub(crate) async fn subscribe_jobs(
        &self,
        engine: &MutationEngine,
        caller: GenerationCaller,
        declared: &str,
    ) -> Result<GenerationSubscription> {
        anyhow::ensure!(
            declared == READ_CAPABILITY,
            "exact private generation read capability declaration is required"
        );
        let owner = caller.verify(engine, READ_CAPABILITY).await?;
        let pins = GenerationPins::load(
            &self.catalog,
            None,
            "get_generation_jobs",
            READ_CAPABILITY,
            None,
        )?;
        let slot = self.slot(&owner)?;
        let receiver = slot.wakes.subscribe();
        let _guard = slot.operation.lock().await;
        pins.check(engine, &caller, "get_generation_jobs", READ_CAPABILITY)
            .await?;
        anyhow::ensure!(
            slot.activated.load(Ordering::Acquire),
            "explicit generation recovery is required"
        );
        let initial = self.read_locked(&owner, &pins)?;
        pins.check(engine, &caller, "get_generation_jobs", READ_CAPABILITY)
            .await?;
        drop(_guard);
        Ok(GenerationSubscription {
            caller,
            owner,
            pins,
            slot,
            receiver,
            initial,
        })
    }
    pub(crate) async fn revalidate_subscription(
        &self,
        engine: &MutationEngine,
        ticket: &GenerationSubscription,
    ) -> Result<()> {
        let _guard = ticket.slot.operation.lock().await;
        ticket
            .pins
            .check(
                engine,
                &ticket.caller,
                "get_generation_jobs",
                READ_CAPABILITY,
            )
            .await?;
        anyhow::ensure!(
            ticket.slot.activated.load(Ordering::Acquire),
            "generation publication is unavailable"
        );
        Ok(())
    }
    pub(crate) async fn refresh_subscription(
        &self,
        engine: &MutationEngine,
        ticket: &GenerationSubscription,
    ) -> Result<CommittedGeneration> {
        let _guard = ticket.slot.operation.lock().await;
        ticket
            .pins
            .check(
                engine,
                &ticket.caller,
                "get_generation_jobs",
                READ_CAPABILITY,
            )
            .await?;
        anyhow::ensure!(
            ticket.slot.activated.load(Ordering::Acquire),
            "generation publication is unavailable"
        );
        let receipt = self.read_locked(&ticket.owner, &ticket.pins)?;
        ticket
            .pins
            .check(
                engine,
                &ticket.caller,
                "get_generation_jobs",
                READ_CAPABILITY,
            )
            .await?;
        Ok(receipt)
    }
}

fn native_failure(code: NativeErrorCode) -> GenerationFailure {
    match code {
        NativeErrorCode::ApprovalRequired => GenerationFailure::ToolApprovalRequired,
        NativeErrorCode::Bounds => GenerationFailure::OutputLimit,
        NativeErrorCode::Timeout => GenerationFailure::Deadline,
        NativeErrorCode::Auth => GenerationFailure::AdmissionWithdrawn,
        _ => GenerationFailure::NativeError,
    }
}
