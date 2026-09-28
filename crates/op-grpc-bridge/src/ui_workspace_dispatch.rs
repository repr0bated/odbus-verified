//! Principal/session-private UI workspace publication owned by MutationEngine.
//!
//! RuntimeDocuments is the durable domain owner. This module publishes its
//! exact complete receipt into the verified owner's private projection and
//! emits metadata-only wakes after the projection commit point.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex as StdMutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};

#[cfg(test)]
use std::sync::atomic::AtomicUsize;

use anyhow::{Context, Result};
use op_core::projection_shm::{
    PrivateDocumentCommit, PrivateDocumentPublication, PrivateProjectionDirectory, ProjectionCommit,
};
use op_plugins::state_plugins::{
    gemma_brain::workspace::{
        UiAccessContext, UiOwnerKey, UiWorkspaceCommit, UiWorkspaceMethods, UiWorkspaceRecovery,
    },
    runtime_documents::{RuntimeDocumentRejected, RuntimeDocuments},
};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, broadcast};

use crate::{
    interceptor::load_capability_grants,
    method_contract_admission,
    mutation_engine::MutationEngine,
    project_dispatch,
    sealed_schema_reader::{
        ManifestPinnedCatalog, ManifestPinnedSchemaDocument, read_manifest_pinned_catalog,
    },
    tched_router_admission::{FacadeOrigin, RouterCaller, readmit_call},
};

const PLUGIN: &str = "gemma_brain";
const RENDERER: &str = "json_render";
const READ_METHOD: &str = "get_ui_workspace";
pub(crate) const UI_WORKSPACE_READ_CAPABILITY: &str = "gemma.read";
const MAX_LIVE_OWNERS: usize = 4096;

pub(crate) const UI_WORKSPACE_METHODS: &[&str] = &[
    "get_ui_workspace",
    "get_ui_spec",
    "get_ui_job",
    "list_ui_specs",
    "create_ui_spec",
    "update_ui_spec",
    "handle_ui_event",
    "delete_ui_spec",
    "delete_ui_job",
    "rebind_ui_renderer",
    "recover_ui_workspace",
    "classify",
    "list_perspectives",
    "list_tags",
    "register_tag",
    "analyze_intent",
];

const READ_METHODS: &[&str] = &[
    "get_ui_workspace",
    "get_ui_spec",
    "get_ui_job",
    "list_ui_specs",
    "classify",
    "list_perspectives",
    "list_tags",
    "analyze_intent",
];

pub(crate) fn is_ui_workspace_method(method: &str) -> bool {
    UI_WORKSPACE_METHODS.contains(&method)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UiWorkspaceWake {
    pub plugin_id: String,
    pub generation: u64,
    pub document_revision: u64,
    pub schema_hash: String,
}

struct OwnerSlot {
    operation: Arc<Mutex<()>>,
    wake: broadcast::Sender<UiWorkspaceWake>,
    activated: AtomicBool,
    #[cfg(test)]
    queued: AtomicUsize,
}

impl OwnerSlot {
    fn new() -> Self {
        let (wake, _) = broadcast::channel(32);
        Self {
            operation: Arc::new(Mutex::new(())),
            wake,
            activated: AtomicBool::new(false),
            #[cfg(test)]
            queued: AtomicUsize::new(0),
        }
    }
}

#[derive(Clone)]
pub(crate) struct UiWorkspacePublisher {
    private_root: PrivateProjectionDirectory,
    documents: Arc<RuntimeDocuments>,
    catalog_dir: PathBuf,
    owners: Arc<StdMutex<HashMap<String, Weak<OwnerSlot>>>>,
    activations: Arc<StdMutex<ActivationCache>>,
}

#[derive(Default)]
struct ActivationCache {
    active: HashSet<String>,
    order: VecDeque<String>,
}

impl ActivationCache {
    fn contains(&self, owner: &str) -> bool {
        self.active.contains(owner)
    }

    fn set(&mut self, owner: &str, active: bool) {
        self.order.retain(|candidate| candidate != owner);
        self.active.remove(owner);
        if !active {
            return;
        }
        while self.active.len() >= MAX_LIVE_OWNERS {
            if let Some(evicted) = self.order.pop_front() {
                self.active.remove(&evicted);
            }
        }
        self.active.insert(owner.to_owned());
        self.order.push_back(owner.to_owned());
    }
}

struct ContractPins {
    catalog_dir: PathBuf,
    catalog_hash: String,
    gemma_hash: String,
    gemma_json: String,
    gemma_schema: PluginSchema,
    renderer_hash: String,
    renderer_json: String,
}

impl ContractPins {
    fn from_catalog(catalog_dir: &Path, catalog: ManifestPinnedCatalog) -> Result<Self> {
        let catalog_hash = catalog.catalog_hash.clone();
        let gemma = catalog
            .plugins
            .get(PLUGIN)
            .context("gemma_brain is absent from the sealed catalog")?;
        let renderer = catalog
            .plugins
            .get(RENDERER)
            .context("json_render is absent from the sealed catalog")?;
        method_contract_admission::admit_compiled_state_contract(gemma, PLUGIN)?;
        method_contract_admission::admit_compiled_state_contract(renderer, RENDERER)?;
        method_contract_admission::admit_pinned_compiled_method(
            gemma,
            PLUGIN,
            READ_METHOD,
            Some(UI_WORKSPACE_READ_CAPABILITY),
        )?;
        validate_schema_contract(gemma, PLUGIN)?;
        validate_schema_contract(renderer, RENDERER)?;
        Ok(Self {
            catalog_dir: catalog_dir.to_owned(),
            catalog_hash,
            gemma_hash: gemma.schema_hash.clone(),
            gemma_json: gemma.blob.schema_json.clone(),
            gemma_schema: gemma.schema.clone(),
            renderer_hash: renderer.schema_hash.clone(),
            renderer_json: renderer.blob.schema_json.clone(),
        })
    }

    fn recheck(&self) -> Result<String> {
        let catalog = read_manifest_pinned_catalog(&self.catalog_dir)?;
        let current = Self::from_catalog(&self.catalog_dir, catalog)?;
        anyhow::ensure!(
            current.gemma_hash == self.gemma_hash
                && current.gemma_json == self.gemma_json
                && current.renderer_hash == self.renderer_hash
                && current.renderer_json == self.renderer_json,
            "UI workspace plugin or renderer seal changed; explicit recovery is required"
        );
        Ok(current.catalog_hash)
    }
}

fn validate_schema_contract(document: &ManifestPinnedSchemaDocument, plugin: &str) -> Result<()> {
    anyhow::ensure!(
        document.schema.name == plugin && document.blob.manifest.plugin_id == plugin,
        "pinned UI contract has the wrong plugin identity"
    );
    Ok(())
}

fn pin_contracts(catalog_dir: &Path, admitted: Option<&PluginSchema>) -> Result<ContractPins> {
    let pins = ContractPins::from_catalog(catalog_dir, read_manifest_pinned_catalog(catalog_dir)?)?;
    if let Some(admitted) = admitted {
        anyhow::ensure!(
            serde_json::to_value(&pins.gemma_schema)? == serde_json::to_value(admitted)?,
            "admitted gemma_brain schema differs from the current sealed plugin"
        );
    }
    Ok(pins)
}

fn validate_plugin_state(schema: &PluginSchema, state: &Value) -> Result<()> {
    let owned = simd_json::serde::to_owned_value(state.clone())?;
    let validation = schema.validate(&owned);
    anyhow::ensure!(
        validation.valid && validation.warnings.is_empty(),
        "UI workspace projection does not match the exact gemma_brain state contract"
    );
    schema
        .validate_exact_schema_fields(&owned)
        .map_err(|_| anyhow::anyhow!("UI workspace exact child contract validation failed"))?;
    // This strict domain decode validates map keys, revisions, provenance,
    // graph limits and every retained child without consulting Cozo.
    UiWorkspaceRecovery::from_plugin_state(state)?;
    Ok(())
}

fn decode_committed(bytes: &[u8], schema: &PluginSchema) -> Result<UiWorkspaceRecovery> {
    let state: Value = serde_json::from_slice(bytes)
        .context("committed private UI workspace is not valid JSON")?;
    validate_plugin_state(schema, &state)?;
    UiWorkspaceRecovery::from_plugin_state(&state)
}

fn compare_revision(previous: Option<&[u8]>, current: &[u8], schema: &PluginSchema) -> Result<()> {
    let candidate = decode_committed(current, schema)?;
    if let Some(previous) = previous {
        let prior = decode_committed(previous, schema)?;
        anyhow::ensure!(
            candidate.document_revision >= prior.document_revision,
            "UI workspace document revision regressed"
        );
        anyhow::ensure!(
            candidate.document_revision != prior.document_revision || current == previous,
            "UI workspace bytes changed without a document revision change"
        );
    }
    Ok(())
}

struct VerifiedOwner {
    access: UiAccessContext,
    key: UiOwnerKey,
}

async fn verify_owner(
    engine: &MutationEngine,
    actor: &str,
    session: Option<&str>,
    genesis: Option<&str>,
    capability: &str,
) -> Result<VerifiedOwner> {
    let session = project_dispatch::verified_session(engine, actor, session, genesis).await?;
    anyhow::ensure!(
        load_capability_grants(actor).contains(capability),
        "current UI workspace capability grant is required"
    );
    let access = UiAccessContext {
        principal_id: actor.to_owned(),
        session_id: session,
        session_genesis: Some(
            genesis
                .context("private Gemma workspace requires verified genesis")?
                .to_owned(),
        ),
    };
    let key = access.owner_key()?;
    Ok(VerifiedOwner { access, key })
}

impl UiWorkspacePublisher {
    pub(crate) fn new(
        private_root: PrivateProjectionDirectory,
        documents: Arc<RuntimeDocuments>,
        catalog_dir: PathBuf,
    ) -> Self {
        Self {
            private_root,
            documents,
            catalog_dir,
            owners: Arc::new(StdMutex::new(HashMap::new())),
            activations: Arc::new(StdMutex::new(ActivationCache::default())),
        }
    }

    fn is_activated(&self, selector: &str) -> Result<bool> {
        Ok(self
            .activations
            .lock()
            .map_err(|_| anyhow::anyhow!("UI workspace activation table is poisoned"))?
            .contains(selector))
    }

    fn mark_activated(&self, selector: &str, active: bool) -> Result<()> {
        self.activations
            .lock()
            .map_err(|_| anyhow::anyhow!("UI workspace activation table is poisoned"))?
            .set(selector, active);
        Ok(())
    }

    fn owner_slot(&self, key: &UiOwnerKey) -> Result<Arc<OwnerSlot>> {
        let selector = key.namespace_component();
        let mut owners = self
            .owners
            .lock()
            .map_err(|_| anyhow::anyhow!("UI workspace owner table is poisoned"))?;
        owners.retain(|_, owner| owner.strong_count() != 0);
        if let Some(owner) = owners.get(selector).and_then(Weak::upgrade) {
            return Ok(owner);
        }
        anyhow::ensure!(
            owners.len() < MAX_LIVE_OWNERS,
            "UI workspace live owner capacity reached"
        );
        let owner = Arc::new(OwnerSlot::new());
        owner
            .activated
            .store(self.is_activated(selector)?, Ordering::Release);
        owners.insert(selector.to_owned(), Arc::downgrade(&owner));
        Ok(owner)
    }

    #[cfg(test)]
    pub(crate) async fn lock_owner_for_test(
        &self,
        key: &UiOwnerKey,
    ) -> Result<UiWorkspaceTestGate> {
        let slot = self.owner_slot(key)?;
        let guard = slot.operation.clone().lock_owned().await;
        Ok(UiWorkspaceTestGate {
            slot,
            _guard: guard,
        })
    }

    fn methods(&self, pins: &ContractPins) -> Result<Arc<UiWorkspaceMethods>> {
        Ok(Arc::new(UiWorkspaceMethods::from_manifest_pinned_renderer(
            self.documents.clone(),
            Arc::<str>::from(pins.renderer_json.clone()),
            pins.renderer_hash.clone(),
        )?))
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
        operation_id: &str,
        facade_origin: Option<FacadeOrigin>,
    ) -> Result<Value> {
        anyhow::ensure!(is_ui_workspace_method(method), "not a UI workspace method");
        let declaration = admitted
            .methods
            .get(method)
            .context("admitted gemma_brain method is absent")?;
        let required_capability = declaration
            .required_capability
            .as_deref()
            .context("UI workspace method has no required capability")?;
        let owner = verify_owner(engine, actor, session, genesis, required_capability).await?;
        let pins = tokio::task::spawn_blocking({
            let admitted = admitted.clone();
            let catalog = self.catalog_dir.clone();
            move || pin_contracts(&catalog, Some(&admitted))
        })
        .await
        .context("UI workspace contract worker failed")??;
        let methods = self.methods(&pins)?;
        let slot = self.owner_slot(&owner.key)?;
        let caller = RouterCaller {
            actor: owner.access.principal_id.clone(),
            capability: Some(required_capability.to_owned()),
            session: Some(owner.access.session_id.clone()),
            genesis: owner.access.session_genesis.clone(),
        };

        if READ_METHODS.contains(&method) {
            anyhow::ensure!(
                slot.activated.load(Ordering::Acquire),
                "UI workspace activation is required"
            );
            let _guard = slot.operation.lock().await;
            readmit_workspace_call(engine, &pins, method, &caller, facade_origin.as_ref()).await?;
            return self
                .read_committed_locked(&owner.key, &pins, &methods, method, args)
                .await;
        }

        let recovery = method == "recover_ui_workspace";
        if !recovery && method != "register_tag" {
            anyhow::ensure!(
                slot.activated.load(Ordering::Acquire),
                "UI workspace activation is required"
            );
        }
        self.publish_owned_admitted(
            slot,
            owner,
            pins,
            methods,
            method.to_owned(),
            args.clone(),
            operation_id.to_owned(),
            recovery,
            Some(QueuedWorkspaceAdmission {
                engine: engine.clone(),
                caller,
                facade_origin,
            }),
        )
        .await
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    async fn publish_owned(
        &self,
        slot: Arc<OwnerSlot>,
        owner: VerifiedOwner,
        pins: ContractPins,
        methods: Arc<UiWorkspaceMethods>,
        method: String,
        args: Value,
        operation_id: String,
        recovery: bool,
    ) -> Result<Value> {
        self.publish_owned_admitted(
            slot,
            owner,
            pins,
            methods,
            method,
            args,
            operation_id,
            recovery,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn publish_owned_admitted(
        &self,
        slot: Arc<OwnerSlot>,
        owner: VerifiedOwner,
        pins: ContractPins,
        methods: Arc<UiWorkspaceMethods>,
        method: String,
        args: Value,
        operation_id: String,
        recovery: bool,
        admission: Option<QueuedWorkspaceAdmission>,
    ) -> Result<Value> {
        let root = self.private_root.clone();
        let owner_selector = owner.key.namespace_component().to_owned();
        let activation_selector = owner_selector.clone();
        let operation_lock = slot.operation.clone();
        let runtime = tokio::runtime::Handle::current();
        let wake_schema_hash = pins.gemma_hash.clone();
        let publisher = self.clone();
        let worker_slot = slot.clone();
        let worker = tokio::spawn(async move {
            // Keep serialization through activation and wake publication. The
            // owned task survives caller cancellation after native submission.
            #[cfg(test)]
            worker_slot.queued.fetch_add(1, Ordering::AcqRel);
            let _guard = operation_lock.lock_owned().await;
            #[cfg(test)]
            worker_slot.queued.fetch_sub(1, Ordering::AcqRel);
            if let Some(admission) = &admission {
                readmit_workspace_call(
                    &admission.engine,
                    &pins,
                    &method,
                    &admission.caller,
                    admission.facade_origin.as_ref(),
                )
                .await?;
            }
            let publication = tokio::task::spawn_blocking(move || -> Result<_> {
                let directory = root.prepare_child(&owner_selector)?;
                if recovery {
                    let pending = directory.pending_document()?;
                    let recover = |previous: Option<&[u8]>| -> Result<PrivateDocumentCommit<PublicationReceipt>> {
                        let recovered = runtime.block_on(methods.recover_snapshot(&owner.access))?;
                        let durable = PublicationReceipt::Recovery(recovered.clone());
                        let prepared = (|| -> Result<Option<Vec<u8>>> {
                            let state = recovered
                                .as_ref()
                                .map(UiWorkspaceRecovery::plugin_state)
                                .transpose()?;
                            if let Some(state) = &state {
                                validate_plugin_state(&pins.gemma_schema, state)?;
                            }
                            pins.recheck()?;
                            let bytes = state.as_ref().map(serde_json::to_vec).transpose()?;
                            if let Some(bytes) = &bytes {
                                compare_revision(previous, bytes, &pins.gemma_schema)?;
                            } else {
                                anyhow::ensure!(
                                    previous.is_none(),
                                    "durable UI workspace disappeared during activation"
                                );
                            }
                            Ok(bytes)
                        })();
                        Ok(match prepared {
                            Ok(current) => PrivateDocumentCommit::Publish { current, durable },
                            Err(_) => PrivateDocumentCommit::RetainPending {
                                durable,
                                error: anyhow::anyhow!(
                                    "UI workspace recovered row failed post-durable contract validation"
                                ),
                            },
                        })
                    };
                    if let Some(pending) = pending {
                        anyhow::ensure!(
                            pending.plugin_id == PLUGIN,
                            "private namespace pending owner mismatch"
                        );
                        directory.recover_document_with_decision(
                            PLUGIN,
                            &pending.operation_id,
                            || recover(None),
                        )
                    } else {
                        directory.commit_document_durable_with_decision(
                            PLUGIN,
                            &operation_id,
                            recover,
                        )
                    }
                } else {
                    let minted =
                        (method == "create_ui_spec").then(|| uuid::Uuid::new_v4().to_string());
                    directory.commit_document_durable_with_decision(
                        PLUGIN,
                        &operation_id,
                        |previous| {
                            let commit = match runtime.block_on(methods.dispatch_mutation(
                                &owner.access,
                                &method,
                                &args,
                                minted.as_deref(),
                            )) {
                                Ok(commit) => commit,
                                Err(error) => {
                                    return match error.downcast::<RuntimeDocumentRejected>() {
                                        Ok(rejected) => Ok(PrivateDocumentCommit::Abort {
                                            error: rejected.into_cause(),
                                        }),
                                        Err(error) => Err(error),
                                    };
                                }
                            };
                            let durable = PublicationReceipt::Mutation(Box::new(commit.clone()));
                            let prepared = (|| -> Result<Vec<u8>> {
                                let _ = commit.method_result()?;
                                let state = commit.plugin_state()?;
                                validate_plugin_state(&pins.gemma_schema, &state)?;
                                let bytes = serde_json::to_vec(&state)?;
                                compare_revision(previous, &bytes, &pins.gemma_schema)?;
                                pins.recheck()?;
                                Ok(bytes)
                            })();
                            Ok(match prepared {
                                Ok(bytes) => PrivateDocumentCommit::Publish {
                                    current: Some(bytes),
                                    durable,
                                },
                                Err(_) => PrivateDocumentCommit::RetainPending {
                                    durable,
                                    error: anyhow::anyhow!(
                                        "UI workspace commit failed post-durable contract validation"
                                    ),
                                },
                            })
                        },
                    )
                }
            })
            .await
            .context("UI workspace publication worker failed")??;

            match publication {
                PrivateDocumentPublication::Committed {
                    projection,
                    durable,
                } => {
                    worker_slot.activated.store(true, Ordering::Release);
                    publisher.mark_activated(&activation_selector, true)?;
                    match durable_receipt(durable, recovery)? {
                        DurableReceipt::Mutation(commit) => {
                            send_wake(
                                &worker_slot,
                                &projection,
                                commit.document_revision,
                                &wake_schema_hash,
                            );
                            commit.method_result()
                        }
                        DurableReceipt::Recovery(recovered) => {
                            if let Some(recovered) = &recovered {
                                send_wake(
                                    &worker_slot,
                                    &projection,
                                    recovered.document_revision,
                                    &wake_schema_hash,
                                );
                            }
                            Ok(serde_json::json!({"workspace": recovered}))
                        }
                    }
                }
                PrivateDocumentPublication::Pending {
                    operation_id,
                    durable,
                    error: _,
                } => {
                    worker_slot.activated.store(false, Ordering::Release);
                    publisher.mark_activated(&activation_selector, false)?;
                    let revision = durable.as_ref().and_then(PublicationReceipt::revision);
                    if let Some(revision) = revision {
                        anyhow::bail!(
                            "ui_workspace_committed_unpublished operation_id={operation_id} document_revision={revision}"
                        )
                    }
                    anyhow::bail!(
                        "ui_workspace_publication_pending operation_id={operation_id} document_revision=unknown"
                    )
                }
            }
        });
        worker
            .await
            .context("UI workspace publication owner failed")?
    }

    async fn read_committed_locked(
        &self,
        key: &UiOwnerKey,
        pins: &ContractPins,
        methods: &UiWorkspaceMethods,
        method: &str,
        args: &Value,
    ) -> Result<Value> {
        let Some(directory) = self.private_root.open_child(key.namespace_component())? else {
            anyhow::bail!("UI workspace activation is incomplete");
        };
        anyhow::ensure!(
            directory.pending_document()?.is_none(),
            "UI workspace publication is pending recovery"
        );
        let mut snapshot = directory.read_snapshot(&[PLUGIN.to_owned()])?;
        let recovery = snapshot
            .states
            .remove(PLUGIN)
            .map(|bytes| decode_committed(&bytes, &pins.gemma_schema))
            .transpose()?;
        pins.recheck()?;
        methods.dispatch_committed_read(method, args, recovery)
    }

    /// Subscribe before the initial private read. StateSync owns path filtering
    /// and frame diffs; this ticket grants access only to this verified owner.
    pub(crate) async fn subscribe_workspace(
        &self,
        engine: &MutationEngine,
        actor: &str,
        session: Option<&str>,
        genesis: Option<&str>,
        declared_capability: &str,
    ) -> Result<UiWorkspaceSubscription> {
        anyhow::ensure!(
            declared_capability == UI_WORKSPACE_READ_CAPABILITY,
            "private UI workspace subscription requires the exact gemma.read header"
        );
        let catalog = self.catalog_dir.clone();
        let pins = tokio::task::spawn_blocking(move || pin_contracts(&catalog, None))
            .await
            .context("UI workspace contract worker failed")??;
        anyhow::ensure!(
            pins.gemma_schema.methods[READ_METHOD]
                .required_capability
                .as_deref()
                == Some(UI_WORKSPACE_READ_CAPABILITY),
            "sealed workspace read capability changed"
        );
        let owner = verify_owner(
            engine,
            actor,
            session,
            genesis,
            UI_WORKSPACE_READ_CAPABILITY,
        )
        .await?;
        let slot = self.owner_slot(&owner.key)?;
        anyhow::ensure!(
            slot.activated.load(Ordering::Acquire),
            "UI workspace activation is required"
        );
        let receiver = slot.wake.subscribe();
        let _guard = slot.operation.lock().await;
        let initial = self.read_receipt_locked(&owner.key, &pins).await?;
        drop(_guard);
        Ok(UiWorkspaceSubscription {
            actor: owner.access.principal_id,
            session: owner.access.session_id,
            genesis: genesis.map(str::to_owned),
            owner_key: owner.key,
            slot,
            receiver,
            initial,
        })
    }

    async fn read_receipt_locked(
        &self,
        key: &UiOwnerKey,
        pins: &ContractPins,
    ) -> Result<CommittedUiWorkspace> {
        let directory = self
            .private_root
            .open_child(key.namespace_component())?
            .context("UI workspace activation is incomplete")?;
        anyhow::ensure!(
            directory.pending_document()?.is_none(),
            "UI workspace publication is pending recovery"
        );
        let snapshot = directory.read_snapshot(&[PLUGIN.to_owned()])?;
        let workspace = snapshot
            .states
            .get(PLUGIN)
            .map(|bytes| decode_committed(bytes, &pins.gemma_schema))
            .transpose()?;
        let catalog_hash = pins.recheck()?;
        Ok(CommittedUiWorkspace {
            generation: snapshot.generation,
            schema_hash: pins.gemma_hash.clone(),
            schema_json: pins.gemma_json.clone(),
            renderer_schema_hash: pins.renderer_hash.clone(),
            renderer_schema_json: pins.renderer_json.clone(),
            catalog_hash,
            workspace,
        })
    }

    pub(crate) async fn refresh_subscription(
        &self,
        engine: &MutationEngine,
        subscription: &UiWorkspaceSubscription,
    ) -> Result<CommittedUiWorkspace> {
        let _guard = subscription.slot.operation.lock().await;
        let (owner, pins) = self
            .revalidate_subscription_pins_locked(engine, subscription)
            .await?;
        self.read_receipt_locked(&owner.key, &pins).await
    }

    /// Revalidate a private ticket for heartbeat/pre-emission use without a
    /// projection read. This is authorization and seal checking, not polling.
    pub(crate) async fn revalidate_subscription(
        &self,
        engine: &MutationEngine,
        subscription: &UiWorkspaceSubscription,
    ) -> Result<()> {
        let _guard = subscription.slot.operation.lock().await;
        self.revalidate_subscription_pins_locked(engine, subscription)
            .await
            .map(|_| ())
    }

    async fn revalidate_subscription_pins_locked(
        &self,
        engine: &MutationEngine,
        subscription: &UiWorkspaceSubscription,
    ) -> Result<(VerifiedOwner, ContractPins)> {
        let owner = verify_owner(
            engine,
            &subscription.actor,
            Some(&subscription.session),
            subscription.genesis.as_deref(),
            UI_WORKSPACE_READ_CAPABILITY,
        )
        .await?;
        anyhow::ensure!(
            owner.key.namespace_component() == subscription.owner_key.namespace_component(),
            "UI workspace subscription owner changed"
        );
        anyhow::ensure!(
            subscription.slot.activated.load(Ordering::Acquire),
            "UI workspace activation is required"
        );
        let catalog = self.catalog_dir.clone();
        let pins = tokio::task::spawn_blocking(move || pin_contracts(&catalog, None))
            .await
            .context("UI workspace contract worker failed")??;
        anyhow::ensure!(
            pins.gemma_hash == subscription.initial.schema_hash
                && pins.gemma_json == subscription.initial.schema_json
                && pins.renderer_hash == subscription.initial.renderer_schema_hash
                && pins.renderer_json == subscription.initial.renderer_schema_json
                && pins.catalog_hash == subscription.initial.catalog_hash,
            "UI workspace subscription contract changed; resubscribe after explicit recovery"
        );
        Ok((owner, pins))
    }
}

#[cfg(test)]
pub(crate) struct UiWorkspaceTestGate {
    slot: Arc<OwnerSlot>,
    _guard: tokio::sync::OwnedMutexGuard<()>,
}

#[cfg(test)]
impl UiWorkspaceTestGate {
    pub(crate) async fn wait_until_queued(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while self.slot.queued.load(Ordering::Acquire) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("workspace operation reached its owner queue");
    }
}

struct QueuedWorkspaceAdmission {
    engine: MutationEngine,
    caller: RouterCaller,
    facade_origin: Option<FacadeOrigin>,
}

async fn readmit_workspace_call(
    engine: &MutationEngine,
    pins: &ContractPins,
    method: &str,
    caller: &RouterCaller,
    facade_origin: Option<&FacadeOrigin>,
) -> Result<()> {
    readmit_call(engine, &pins.gemma_schema, PLUGIN, method, caller).await?;
    if let Some(origin) = facade_origin {
        origin.validate_target(PLUGIN, method)?;
        origin.readmit(engine, caller).await?;
    }
    Ok(())
}

enum PublicationReceipt {
    Mutation(Box<UiWorkspaceCommit>),
    Recovery(Option<UiWorkspaceRecovery>),
}

impl PublicationReceipt {
    fn revision(&self) -> Option<u64> {
        match self {
            Self::Mutation(value) => Some(value.document_revision),
            Self::Recovery(value) => value.as_ref().map(|value| value.document_revision),
        }
    }
}

enum DurableReceipt {
    Mutation(Box<UiWorkspaceCommit>),
    Recovery(Option<UiWorkspaceRecovery>),
}

fn durable_receipt(receipt: PublicationReceipt, recovery: bool) -> Result<DurableReceipt> {
    match (receipt, recovery) {
        (PublicationReceipt::Mutation(value), false) => Ok(DurableReceipt::Mutation(value)),
        (PublicationReceipt::Recovery(value), true) => Ok(DurableReceipt::Recovery(value)),
        _ => anyhow::bail!("UI workspace publication receipt kind mismatch"),
    }
}

fn send_wake(
    slot: &OwnerSlot,
    projection: &ProjectionCommit,
    document_revision: u64,
    schema_hash: &str,
) {
    if projection.previous != projection.current {
        let _ = slot.wake.send(UiWorkspaceWake {
            plugin_id: PLUGIN.to_owned(),
            generation: projection.generation,
            document_revision,
            schema_hash: schema_hash.to_owned(),
        });
    }
}

#[derive(Debug, Clone)]
pub(crate) struct CommittedUiWorkspace {
    pub generation: u64,
    pub schema_hash: String,
    pub schema_json: String,
    pub renderer_schema_hash: String,
    pub renderer_schema_json: String,
    pub catalog_hash: String,
    pub workspace: Option<UiWorkspaceRecovery>,
}

impl CommittedUiWorkspace {
    /// Complete flat gemma_brain workspace state for private path selection.
    /// This is already owner-scoped and contract-validated; callers must not
    /// insert it into the generic public cache or broadcast channel.
    pub(crate) fn plugin_state(&self) -> Result<Option<Value>> {
        self.workspace
            .as_ref()
            .map(UiWorkspaceRecovery::plugin_state)
            .transpose()
    }
}

pub(crate) struct UiWorkspaceSubscription {
    actor: String,
    session: String,
    genesis: Option<String>,
    owner_key: UiOwnerKey,
    slot: Arc<OwnerSlot>,
    pub receiver: broadcast::Receiver<UiWorkspaceWake>,
    pub initial: CommittedUiWorkspace,
}

#[cfg(test)]
#[path = "ui_workspace_dispatch_tests.rs"]
mod tests;
