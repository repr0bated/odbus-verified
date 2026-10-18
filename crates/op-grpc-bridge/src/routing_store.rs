//! Private routing documents over the existing durable Cozo owner.
//!
//! Integration must authenticate the scope and readmit the caller after
//! acquiring RoutingGuard. This module supplies neither identity nor grants.
//! Domain transitions are pure; uncertainty only permits durable readback,
//! never replay of a previous transition or of its external effects.
use std::{collections::BTreeMap, path::PathBuf, sync::{Arc, Mutex as StdMutex, atomic::{AtomicBool, Ordering}}};

use anyhow::{Context, Result, ensure};
use op_core::projection_shm::{PrivateDocumentCommit, PrivateDocumentPublication, PrivateProjectionDirectory};
use op_plugins::state_plugins::{
    routing_decision_methods::{DecisionRecord, DecisionState},
    runtime_documents::{RuntimeDocument, RuntimeDocumentRejected, RuntimeDocuments},
    schema_tag_registry_methods::{RegistryState, validate_text, MAX_ID_BYTES},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, OwnedMutexGuard, broadcast};

use crate::{method_contract_admission, routing_resources::{RoutingResources, RoutingReservation},
    sealed_schema_reader::{ManifestPinnedSchemaDocument, read_manifest_pinned_schema_document}};

const DECISIONS: &str = "routing_decision";
const REGISTRY: &str = "schema_tag_registry";
const FORMAT: u32 = 1;

/// Constructed only from the backend's verified caller. Genesis remains part
/// of storage isolation even though the public decision DTO has no such field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RoutingOwner {
    pub principal_id: String,
    pub session_id: String,
    pub session_genesis: String,
    pub owner_id: String,
}

impl RoutingOwner {
    pub fn new(principal_id: String, session_id: String, session_genesis: String) -> Result<Self> {
        for (field, value) in [("principal_id", &principal_id), ("session_id", &session_id), ("session_genesis", &session_genesis)] {
            validate_text(value, field, MAX_ID_BYTES)?;
        }
        let bytes = serde_json::to_vec(&("routing-owner-v1", &principal_id, &session_id, &session_genesis))?;
        Ok(Self { principal_id, session_id, session_genesis, owner_id: hex::encode(Sha256::digest(bytes)) })
    }
}

/// Explicit stable policy-owner scope configured by integration. Never derive
/// this from the session making an evaluation or administration request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegistryOwner { pub owner_id: String }

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "scope", rename_all = "snake_case", deny_unknown_fields)]
enum Owner { Decisions(RoutingOwner), Registry(RegistryOwner) }

impl Owner {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Decisions(scope) => {
                for (field, value) in [("principal_id", &scope.principal_id), ("session_id", &scope.session_id),
                    ("session_genesis", &scope.session_genesis), ("owner_id", &scope.owner_id)] {
                    validate_text(value, field, MAX_ID_BYTES)?;
                }
            }
            Self::Registry(scope) => validate_text(&scope.owner_id, "registry owner_id", MAX_ID_BYTES)?,
        }
        Ok(())
    }
    fn plugin(&self) -> &'static str { match self { Self::Decisions(_) => DECISIONS, Self::Registry(_) => REGISTRY } }
    fn selector(&self) -> Result<String> {
        self.validate()?;
        let bytes = serde_json::to_vec(&("routing-private-v1", self))?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }
    fn budget_selector(&self) -> Result<String> {
        self.validate()?;
        // Different sessions of one principal share a quota; changing logins
        // must not multiply the per-principal byte/job allowance.
        let bytes = match self {
            Self::Decisions(scope) => serde_json::to_vec(&("routing-principal-budget-v1", &scope.principal_id))?,
            Self::Registry(scope) => serde_json::to_vec(&("routing-registry-budget-v1", &scope.owner_id))?,
        };
        Ok(hex::encode(Sha256::digest(bytes)))
    }
    fn document_key(&self) -> Result<String> { Ok(format!("__routing_private_v1:{}:{}", self.plugin(), self.selector()?)) }
    fn empty(&self) -> StoredState {
        match self {
            Self::Decisions(_) => StoredState::Decisions(DecisionState { revision: 0, records: vec![] }),
            Self::Registry(_) => StoredState::Registry(RegistryState { revision: 0, tags: vec![], policies: vec![], protected_methods: vec![] }),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "state", rename_all = "snake_case", deny_unknown_fields)]
enum StoredState { Decisions(DecisionState), Registry(RegistryState) }

impl StoredState {
    fn revision(&self) -> u64 { match self { Self::Decisions(s) => s.revision, Self::Registry(s) => s.revision } }
    fn set_revision(&mut self, revision: u64) { match self { Self::Decisions(s) => s.revision = revision, Self::Registry(s) => s.revision = revision } }
    fn value(&self) -> Result<serde_json::Value> {
        Ok(match self { Self::Decisions(s) => serde_json::to_value(s)?, Self::Registry(s) => serde_json::to_value(s)? })
    }
    fn from_projection(owner: &Owner, bytes: &[u8]) -> Result<Self> {
        Ok(match owner { Owner::Decisions(_) => Self::Decisions(serde_json::from_slice(bytes)?),
            Owner::Registry(_) => Self::Registry(serde_json::from_slice(bytes)?) })
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredDocument { format: u32, owner: Owner, state: StoredState }

struct OwnerSlot {
    operation: Arc<Mutex<()>>,
    activated: AtomicBool,
    wakes: broadcast::Sender<RoutingWake>,
}

#[derive(Clone, Debug)]
pub(crate) struct RoutingWake {
    pub plugin_id: &'static str,
    pub revision: u64,
    pub generation: u64,
    pub schema_hash: String,
}

struct Inner {
    documents: Arc<RuntimeDocuments>, root: PrivateProjectionDirectory, catalog: PathBuf,
    resources: Arc<RoutingResources>, owners: StdMutex<BTreeMap<String, Arc<OwnerSlot>>>,
}

#[derive(Clone)]
pub(crate) struct RoutingStore { inner: Arc<Inner> }

/// Hold this through final caller/capability/policy readmission, then transfer
/// it into exactly one store operation. Dropping it before that writes nothing.
pub(crate) struct RoutingGuard {
    store: Arc<Inner>, owner: Owner, selector: String, pin: Arc<ManifestPinnedSchemaDocument>,
    slot: Arc<OwnerSlot>, _operation: OwnedMutexGuard<()>, reservation: RoutingReservation,
    retained_registry: Option<Box<RoutingGuard>>,
}

impl RoutingGuard {
    /// Acquire/read registry first, then acquire the decision guard, and move
    /// registry exclusion here. Both survive cancellation of the caller's
    /// future until the decision publication worker finishes cleanup.
    pub fn retain_registry(&mut self, registry: RoutingGuard) -> Result<()> {
        ensure!(matches!(&self.owner, Owner::Decisions(_)) && matches!(&registry.owner, Owner::Registry(_))
            && Arc::ptr_eq(&self.store, &registry.store) && self.retained_registry.is_none()
            && registry.retained_registry.is_none(), "invalid retained routing policy guard");
        self.retained_registry = Some(Box::new(registry));
        Ok(())
    }
}

impl RoutingStore {
    pub fn new(documents: Arc<RuntimeDocuments>, root: PrivateProjectionDirectory,
        catalog: PathBuf, resources: Arc<RoutingResources>) -> Self {
        Self { inner: Arc::new(Inner { documents, root, catalog, resources, owners: StdMutex::new(BTreeMap::new()) }) }
    }
    pub fn resources(&self) -> &Arc<RoutingResources> { &self.inner.resources }

    pub async fn lock_decisions(&self, owner: RoutingOwner) -> Result<RoutingGuard> { self.lock(Owner::Decisions(owner)).await }
    pub async fn lock_registry(&self, owner: RegistryOwner) -> Result<RoutingGuard> { self.lock(Owner::Registry(owner)).await }

    /// Reserve before integration parses/copies an untrusted method payload.
    /// Use the returned token for lock_*_reserved, not a second reservation.
    pub fn reserve_decisions(&self, owner: &RoutingOwner) -> Result<RoutingReservation> { self.reserve(&Owner::Decisions(owner.clone())) }
    pub fn reserve_registry(&self, owner: &RegistryOwner) -> Result<RoutingReservation> { self.reserve(&Owner::Registry(owner.clone())) }
    pub async fn lock_decisions_reserved(&self, owner: RoutingOwner, reservation: RoutingReservation) -> Result<RoutingGuard> {
        self.lock_reserved(Owner::Decisions(owner), reservation).await
    }
    pub async fn lock_registry_reserved(&self, owner: RegistryOwner, reservation: RoutingReservation) -> Result<RoutingGuard> {
        self.lock_reserved(Owner::Registry(owner), reservation).await
    }

    fn reserve(&self, owner: &Owner) -> Result<RoutingReservation> {
        let limits = self.inner.resources.limits();
        self.inner.resources.reserve(&owner.budget_selector()?,
            limits.max_document_bytes.checked_mul(6).context("routing workspace overflow")?, self.inner.resources.audit_work_bytes())
    }

    async fn lock(&self, owner: Owner) -> Result<RoutingGuard> {
        let reservation = self.reserve(&owner)?;
        self.lock_reserved(owner, reservation).await
    }

    async fn lock_reserved(&self, owner: Owner, mut reservation: RoutingReservation) -> Result<RoutingGuard> {
        let selector = owner.selector()?;
        let limits = self.inner.resources.limits();
        reservation.require_owner(&self.inner.resources, &owner.budget_selector()?,
            limits.max_document_bytes.checked_mul(6).context("routing workspace overflow")?, self.inner.resources.audit_work_bytes())?;
        let slot = {
            let mut owners = self.inner.owners.lock().map_err(|_| anyhow::anyhow!("routing owner slots poisoned"))?;
            if let Some(slot) = owners.get(&selector) { slot.clone() } else {
                ensure!(owners.len() < limits.max_owners, "routing_owner_capacity");
                let (wakes, _) = broadcast::channel(32);
                let slot = Arc::new(OwnerSlot { operation: Arc::new(Mutex::new(())), activated: AtomicBool::new(false), wakes });
                owners.insert(selector.clone(), slot.clone()); slot
            }
        };
        let operation = tokio::time::timeout(std::time::Duration::from_millis(limits.queue_wait_ms), slot.operation.clone().lock_owned())
            .await.context("routing_queue_timeout")?;
        reservation.begin_work()?;
        let store = self.clone();
        let plugin = owner.plugin();
        let pin = tokio::task::spawn_blocking(move || store.pin(plugin)).await??;
        Ok(RoutingGuard { store: self.inner.clone(), owner, selector, pin: Arc::new(pin), slot, _operation: operation, reservation, retained_registry: None })
    }

    fn pin(&self, plugin: &str) -> Result<ManifestPinnedSchemaDocument> {
        let pin = read_manifest_pinned_schema_document(&self.inner.catalog, plugin)?;
        method_contract_admission::admit_compiled_state_contract(&pin, plugin)?;
        Ok(pin)
    }
    fn recheck(&self, guard: &RoutingGuard) -> Result<()> {
        ensure!(Arc::ptr_eq(&guard.store, &self.inner), "routing guard belongs to another store");
        self.recheck_pin(&guard.owner, &guard.pin)?;
        if let Some(registry) = &guard.retained_registry { self.recheck(registry)?; }
        Ok(())
    }
    fn recheck_pin(&self, owner: &Owner, expected: &ManifestPinnedSchemaDocument) -> Result<()> {
        let current = self.pin(owner.plugin())?;
        ensure!(current.schema_hash == expected.schema_hash && current.blob.schema_json == expected.blob.schema_json,
            "routing sealed contract changed while queued");
        Ok(())
    }

    fn validate(&self, owner: &Owner, state: &StoredState, pin: &ManifestPinnedSchemaDocument) -> Result<()> {
        let limits = self.inner.resources.limits();
        match (owner, state) {
            (Owner::Decisions(scope), StoredState::Decisions(state)) => {
                state.validate_for_owner(&scope.owner_id)?;
                ensure!(state.records.len() <= limits.max_records, "routing_record_capacity");
                for record in &state.records {
                    ensure!(record.verified_identity.principal_id == scope.principal_id
                        && record.verified_identity.session_id == scope.session_id, "routing decision crosses verified session scope");
                    ensure!(record.canonical_mappings.len() <= limits.max_evidence_fanout, "routing_evidence_capacity");
                }
            }
            (Owner::Registry(scope), StoredState::Registry(state)) => {
                state.validate_for_owner(&scope.owner_id)?;
                ensure!(state.tags.len() <= limits.max_tags && state.policies.len() <= limits.max_policies
                    && state.protected_methods.len() <= limits.max_markers, "routing_registry_capacity");
                for policy in &state.policies { ensure!(policy.canonical_mappings.len() <= limits.max_evidence_fanout, "routing_evidence_capacity"); }
            }
            _ => anyhow::bail!("routing document kind differs from owner"),
        }
        let value = state.value()?;
        let owned = simd_json::serde::to_owned_value(value)?;
        let result = pin.schema.validate(&owned);
        ensure!(result.valid && result.warnings.is_empty(), "routing private state differs from sealed contract");
        pin.schema.validate_exact_schema_fields(&owned).map_err(|_| anyhow::anyhow!("routing exact state contract invalid"))?;
        Ok(())
    }

    fn decode(&self, owner: &Owner, row: RuntimeDocument, pin: &ManifestPinnedSchemaDocument) -> Result<StoredState> {
        bounded_bytes(&row.document, self.inner.resources.limits().max_document_bytes)?;
        let doc: StoredDocument = serde_json::from_value(row.document)?;
        ensure!(doc.format == FORMAT && &doc.owner == owner && doc.state.revision() == row.revision,
            "routing durable document owner/revision mismatch");
        self.validate(owner, &doc.state, pin)?;
        Ok(doc.state)
    }

    fn prepare_bytes(&self, guard: &mut RoutingGuard, state: &StoredState, previous: Option<&[u8]>) -> Result<Vec<u8>> {
        self.validate(&guard.owner, state, &guard.pin)?;
        let bytes = bounded_bytes(&state.value()?, self.inner.resources.limits().max_document_bytes)?;
        if let Some(previous) = previous {
            let old = StoredState::from_projection(&guard.owner, previous)?;
            self.validate(&guard.owner, &old, &guard.pin)?;
            ensure!(state.revision() >= old.revision()
                && (state.revision() != old.revision() || bytes == previous), "routing projection revision regressed or changed without durable revision");
        }
        self.recheck(guard)?;
        guard.reservation.retain_document(&guard.selector, bytes.len())?;
        Ok(bytes)
    }

    fn directory(&self, guard: &RoutingGuard) -> Result<PrivateProjectionDirectory> {
        let directory = self.inner.root.prepare_child(&guard.selector)?;
        directory.ensure_distinct_from(std::path::Path::new(&op_core::projection_shm::shm_state_dir()))?;
        Ok(directory)
    }

    /// Rebuild from Cozo only. The exact pending ID is used when present;
    /// otherwise create a fresh publication ID for cold-boot reconstruction.
    fn activate(&self, guard: &mut RoutingGuard, runtime: &tokio::runtime::Handle) -> Result<()> {
        self.recheck(guard)?;
        let directory = self.directory(guard)?;
        let pending = directory.pending_document()?;
        if guard.slot.activated.load(Ordering::Acquire) && pending.is_none() { return Ok(()); }
        guard.slot.activated.store(false, Ordering::Release);
        let plugin = guard.owner.plugin();
        let key = guard.owner.document_key()?;
        let recover = |previous: Option<&[u8]>| -> Result<PrivateDocumentCommit<StoredState>> {
            let row = runtime.block_on(self.inner.documents.read_durable(&key))?;
            let state = match row { Some(row) => self.decode(&guard.owner, row, &guard.pin)?, None => guard.owner.empty() };
            let bytes = self.prepare_bytes(guard, &state, previous)?;
            Ok(PrivateDocumentCommit::Publish { current: Some(bytes), durable: state })
        };
        let publication = if let Some(pending) = pending {
            ensure!(pending.plugin_id == plugin, "routing pending publication owner mismatch");
            let mut recover = recover;
            directory.recover_document_with_decision(plugin, &pending.operation_id, || recover(None))?
        } else {
            directory.commit_document_durable_with_decision(plugin, &uuid::Uuid::new_v4().to_string(), recover)?
        };
        match publication {
            PrivateDocumentPublication::Committed { projection, durable } => {
                guard.slot.activated.store(true, Ordering::Release);
                self.wake(guard, durable.revision(), projection.generation);
                Ok(())
            }
            PrivateDocumentPublication::Pending { .. } => anyhow::bail!("routing_recovery_pending; do not replay the original mutation"),
        }
    }

    fn wake(&self, guard: &RoutingGuard, revision: u64, generation: u64) {
        let _ = guard.slot.wakes.send(RoutingWake { plugin_id: guard.owner.plugin(), revision, generation,
            schema_hash: guard.pin.schema_hash.clone() });
    }

    pub fn subscribe(&self, guard: &RoutingGuard) -> Result<broadcast::Receiver<RoutingWake>> {
        ensure!(Arc::ptr_eq(&guard.store, &self.inner), "routing guard belongs to another store");
        Ok(guard.slot.wakes.subscribe())
    }

    pub async fn read_decisions<A>(&self, guard: RoutingGuard, final_admission: A) -> Result<DecisionState>
    where A: FnOnce() -> Result<()> + Send + 'static {
        ensure!(matches!(&guard.owner, Owner::Decisions(_)), "decision read requires decision owner");
        match self.read(guard, final_admission).await? { StoredState::Decisions(state) => Ok(state), _ => unreachable!() }
    }
    pub async fn read_registry<A>(&self, guard: RoutingGuard, final_admission: A) -> Result<RegistryState>
    where A: FnOnce() -> Result<()> + Send + 'static {
        ensure!(matches!(&guard.owner, Owner::Registry(_)), "registry read requires registry owner");
        match self.read(guard, final_admission).await? { StoredState::Registry(state) => Ok(state), _ => unreachable!() }
    }

    /// Read committed policy while retaining its owner exclusion. Take this
    /// guard before starting a decision CAS; its final admission callback must
    /// use the retained revision and must not acquire another routing guard
    /// while the shared RuntimeDocuments writer is held.
    pub async fn read_registry_retained<A>(&self, guard: RoutingGuard, final_admission: A) -> Result<(RegistryState, RoutingGuard)>
    where A: FnOnce() -> Result<()> + Send + 'static {
        ensure!(matches!(&guard.owner, Owner::Registry(_)), "registry read requires registry owner");
        let (state, guard) = self.read_retained(guard, final_admission).await?;
        match state { StoredState::Registry(state) => Ok((state, guard)), _ => unreachable!() }
    }

    async fn read<A>(&self, guard: RoutingGuard, final_admission: A) -> Result<StoredState>
    where A: FnOnce() -> Result<()> + Send + 'static {
        self.read_retained(guard, final_admission).await.map(|(state, _guard)| state)
    }

    async fn read_retained<A>(&self, mut guard: RoutingGuard, final_admission: A) -> Result<(StoredState, RoutingGuard)>
    where A: FnOnce() -> Result<()> + Send + 'static {
        self.recheck(&guard)?;
        let store = self.clone(); let runtime = tokio::runtime::Handle::current();
        // The worker owns the lock and charges even when its caller disappears.
        tokio::spawn(async move {
            tokio::task::spawn_blocking(move || -> Result<(StoredState, RoutingGuard)> {
                store.activate(&mut guard, &runtime)?;
                let snapshot = store.directory(&guard)?.read_snapshot(&[guard.owner.plugin().to_owned()])?;
                let bytes = snapshot.states.get(guard.owner.plugin()).context("routing committed state missing")?;
                let state = StoredState::from_projection(&guard.owner, bytes)?;
                store.validate(&guard.owner, &state, &guard.pin)?;
                store.recheck(&guard)?;
                // Revalidate after reconstruction/read waits and before any
                // private rows leave the owned worker. Never reacquire this
                // same owner's operation guard in the callback.
                final_admission()?;
                Ok((state, guard))
            }).await.context("routing read worker failed")?
        }).await.context("routing read owner failed")?
    }

    pub async fn record_decision<A>(&self, guard: RoutingGuard, operation_id: String, record: DecisionRecord, final_admission: A) -> Result<DecisionRecord>
    where A: FnOnce() -> Result<()> + Send + 'static {
        record.validate()?;
        ensure!(matches!(&guard.owner, Owner::Decisions(_)), "decision mutation requires decision owner");
        self.mutate(guard, operation_id, move |state| {
            let StoredState::Decisions(state) = state else { anyhow::bail!("decision state required") };
            ensure!(!state.records.iter().any(|r| r.decision_id == record.decision_id), "decision_id already recorded");
            state.records.push(record.clone()); Ok(record)
        }, final_admission).await
    }

    /// `change` must be a pure, once-only domain transition. Resolved mapping,
    /// tag, and OpenFlow references must already have been server-authored.
    pub async fn mutate_registry<R, F, A>(&self, guard: RoutingGuard, operation_id: String, change: F, final_admission: A) -> Result<R>
    where R: Send + 'static, F: FnOnce(&mut RegistryState) -> Result<R> + Send + 'static,
        A: FnOnce() -> Result<()> + Send + 'static {
        ensure!(matches!(&guard.owner, Owner::Registry(_)), "registry mutation requires registry owner");
        self.mutate(guard, operation_id, move |state| {
            let StoredState::Registry(state) = state else { anyhow::bail!("registry state required") };
            change(state)
        }, final_admission).await
    }

    async fn mutate<R, F, A>(&self, mut guard: RoutingGuard, operation_id: String, change: F, final_admission: A) -> Result<R>
    where R: Send + 'static, F: FnOnce(&mut StoredState) -> Result<R> + Send + 'static,
        A: FnOnce() -> Result<()> + Send + 'static {
        ensure!(uuid::Uuid::parse_str(&operation_id)?.to_string() == operation_id, "noncanonical routing operation_id");
        self.recheck(&guard)?;
        let store = self.clone(); let runtime = tokio::runtime::Handle::current();
        tokio::spawn(async move {
            tokio::task::spawn_blocking(move || -> Result<R> {
                store.activate(&mut guard, &runtime)?;
                let directory = store.directory(&guard)?;
                let plugin = guard.owner.plugin(); let key = guard.owner.document_key()?;
                guard.slot.activated.store(false, Ordering::Release);
                let publication = directory.commit_document_durable_with_decision(plugin, &operation_id, |previous| {
                    store.recheck(&guard)?;
                    let owner = guard.owner.clone(); let pin = guard.pin.clone(); let worker = store.clone();
                    let registry_pin = guard.retained_registry.as_ref().map(|registry| (registry.owner.clone(), registry.pin.clone()));
                    let old = runtime.block_on(store.inner.documents.read_durable(&key))?;
                    let expected_revision = old.as_ref().map_or(0, |row| row.revision);
                    let next_revision = expected_revision.checked_add(1).context("routing revision exhausted")?;
                    let result = runtime.block_on(store.inner.documents.update_at_revision_with_result(&key, expected_revision, move |old| {
                        // RuntimeDocuments has acquired its shared writer and
                        // transferred it into this separate blocking worker.
                        // No native durable CAS has been submitted yet.
                        final_admission()?;
                        worker.recheck_pin(&owner, &pin)?;
                        if let Some((owner, pin)) = registry_pin { worker.recheck_pin(&owner, &pin)?; }
                        let mut state = match old { Some(document) => worker.decode(&owner, RuntimeDocument { revision: expected_revision, document }, &pin)?, None => owner.empty() };
                        state.set_revision(next_revision);
                        let receipt = change(&mut state)?;
                        ensure!(state.revision() == next_revision, "routing transition changed server-authored revision");
                        worker.validate(&owner, &state, &pin)?;
                        let document = serde_json::to_value(StoredDocument { format: FORMAT, owner, state })?;
                        bounded_bytes(&document, worker.inner.resources.limits().max_document_bytes)?;
                        Ok((document, receipt))
                    }));
                    let (row, receipt) = match result {
                        Ok(result) => result,
                        Err(error) => return match error.downcast::<RuntimeDocumentRejected>() {
                            Ok(rejected) => Ok(PrivateDocumentCommit::Abort { error: rejected.into_cause() }),
                            Err(error) => Err(error),
                        },
                    };
                    let prepared = store.decode(&guard.owner, row, &guard.pin)
                        .and_then(|state| store.prepare_bytes(&mut guard, &state, previous).map(|bytes| (state, bytes)));
                    Ok(match prepared {
                        Ok((state, bytes)) => PrivateDocumentCommit::Publish { current: Some(bytes), durable: (receipt, state.revision()) },
                        Err(_) => PrivateDocumentCommit::RetainPending { durable: (receipt, next_revision),
                            error: anyhow::anyhow!("routing durable commit requires private publication recovery") },
                    })
                })?;
                match publication {
                    PrivateDocumentPublication::Committed { projection, durable: (receipt, revision) } => {
                        guard.slot.activated.store(true, Ordering::Release);
                        store.wake(&guard, revision, projection.generation); Ok(receipt)
                    }
                    PrivateDocumentPublication::Pending { .. } => anyhow::bail!("routing_publication_pending; reconcile through admitted reads; do not retry automatically"),
                }
            }).await.context("routing mutation worker failed")?
        }).await.context("routing mutation owner failed")?
    }
}

/// Enforce the encoded payload ceiling while writing, before a large Vec is
/// allocated. The reservation also covers bounded copies made by native CAS.
fn bounded_bytes(value: &serde_json::Value, maximum: usize) -> Result<Vec<u8>> {
    struct Writer { bytes: Vec<u8>, maximum: usize }
    impl std::io::Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let size = self.bytes.len().checked_add(bytes.len()).filter(|size| *size <= self.maximum)
                .ok_or_else(|| std::io::Error::other("routing_document_capacity"))?;
            self.bytes.try_reserve_exact(size - self.bytes.len()).map_err(std::io::Error::other)?;
            self.bytes.extend_from_slice(bytes); Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    let mut writer = Writer { bytes: Vec::new(), maximum };
    serde_json::to_writer(&mut writer, value)?; Ok(writer.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_plugins::state_plugins::schema_tag_registry_methods::TagDescriptor;
    #[test]
    fn scope_keys_preserve_tuple_boundaries_genesis_and_registry_authority() {
        let a = Owner::Decisions(RoutingOwner { principal_id: "ab".into(), session_id: "c".into(), session_genesis: "g1".into(), owner_id: "owner".into() });
        let b = Owner::Decisions(RoutingOwner { principal_id: "a".into(), session_id: "bc".into(), session_genesis: "g1".into(), owner_id: "owner".into() });
        let mut c = a.clone();
        if let Owner::Decisions(scope) = &mut c { scope.session_genesis = "g2".into(); }
        let registry = Owner::Registry(RegistryOwner { owner_id: "owner".into() });
        assert_ne!(a.document_key().unwrap(), b.document_key().unwrap());
        assert_ne!(a.document_key().unwrap(), c.document_key().unwrap());
        assert_ne!(a.selector().unwrap(), registry.selector().unwrap());
        assert_eq!(a.budget_selector().unwrap(), c.budget_selector().unwrap());
    }
    #[test]
    fn document_byte_limit_is_enforced_during_serialization() {
        let value = serde_json::json!({"opaque": "x".repeat(1024)});
        assert!(bounded_bytes(&value, 32).is_err());
        assert!(bounded_bytes(&value, 2048).is_ok());
    }

    #[tokio::test]
    async fn retained_pending_restart_and_empty_shm_boot_reconstruct_without_transition_replay() {
        let _shm = crate::human_principal_dispatch::tests::temp_shm_with_schemas(&[
            op_plugins::state_plugins::routing_decision::routing_decision_schema(),
            op_plugins::state_plugins::schema_tag_registry::schema_tag_registry_schema(),
        ]);
        let temporary = tempfile::tempdir().unwrap();
        let documents = Arc::new(RuntimeDocuments::new(op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap()));
        let catalog = crate::sealed_schema_reader::blob_catalog_dir_from_env();
        let root = PrivateProjectionDirectory::prepare(&temporary.path().join("first-private")).unwrap();
        let store = RoutingStore::new(documents.clone(), root.clone(), catalog.clone(),
            RoutingResources::new(Default::default()).unwrap());
        let owner = RegistryOwner { owner_id: "configured-policy-owner".into() };
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let transitions = calls.clone();
        let guard = store.lock_registry(owner.clone()).await.unwrap();
        let mut wakes = store.subscribe(&guard).unwrap();
        let revision = store.mutate_registry(guard, uuid::Uuid::new_v4().to_string(), move |state| {
            transitions.fetch_add(1, Ordering::AcqRel);
            state.tags.push(TagDescriptor { tag_id: 7, name: "private-tag".into(), description: "fixture".into(),
                default_disposition: None, revision: 1, associations: vec![], openflow_binding: None });
            Ok(state.revision)
        }, || Ok(())).await.unwrap();
        assert_eq!(revision, 1);
        while let Ok(wake) = wakes.try_recv() { assert_eq!(wake.plugin_id, REGISTRY); assert!(!wake.schema_hash.is_empty()); }

        // Process restart retains an exact pending operation. No domain
        // mutation is run by this artificial interrupted publication.
        let selector = Owner::Registry(owner.clone()).selector().unwrap();
        let child = root.prepare_child(&selector).unwrap();
        let operation = uuid::Uuid::new_v4().to_string();
        let pending: PrivateDocumentPublication<()> = child.commit_document_durable_with_decision(REGISTRY, &operation,
            |_| Err(anyhow::anyhow!("fixture interrupted publication"))).unwrap();
        assert!(matches!(pending, PrivateDocumentPublication::Pending { .. }));
        assert_eq!(child.pending_document().unwrap().unwrap().operation_id, operation);
        let restored = store.read_registry(store.lock_registry(owner.clone()).await.unwrap(), || Ok(())).await.unwrap();
        assert_eq!(restored.revision, 1); assert_eq!(restored.tags.len(), 1);
        assert!(child.pending_document().unwrap().is_none());
        assert_eq!(calls.load(Ordering::Acquire), 1);

        // Cold boot has durable rows but no manifest, marker or SHM document.
        let cold_root = PrivateProjectionDirectory::prepare(&temporary.path().join("cold-private")).unwrap();
        assert!(cold_root.open_child(&selector).unwrap().is_none());
        let cold = RoutingStore::new(documents, cold_root, catalog,
            RoutingResources::new(Default::default()).unwrap());
        let restored = cold.read_registry(cold.lock_registry(owner.clone()).await.unwrap(), || Ok(())).await.unwrap();
        assert_eq!(restored.revision, 1); assert_eq!(restored.tags[0].tag_id, 7);
        assert_eq!(calls.load(Ordering::Acquire), 1);

        // A failed pure transition aborts before native submission, retaining
        // the same committed revision instead of fabricating a successful row.
        let error = cold.mutate_registry(cold.lock_registry(owner.clone()).await.unwrap(),
            uuid::Uuid::new_v4().to_string(), |_state| -> Result<()> { anyhow::bail!("fixture rejected transition") }, || Ok(())).await;
        assert!(error.is_err());
        assert_eq!(cold.read_registry(cold.lock_registry(owner.clone()).await.unwrap(), || Ok(())).await.unwrap().revision, 1);

        let domain_calls = calls.clone();
        let denied = cold.mutate_registry(cold.lock_registry(owner.clone()).await.unwrap(), uuid::Uuid::new_v4().to_string(),
            move |_state| { domain_calls.fetch_add(1, Ordering::AcqRel); Ok(()) },
            || anyhow::bail!("fixture final admission revoked")).await;
        assert!(denied.is_err());
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert!(cold.read_registry(cold.lock_registry(owner.clone()).await.unwrap(),
            || anyhow::bail!("fixture private read revoked")).await.is_err());
        assert_eq!(cold.read_registry(cold.lock_registry(owner).await.unwrap(), || Ok(())).await.unwrap().revision, 1);
    }

    #[tokio::test]
    async fn cancelled_caller_cannot_release_an_in_progress_native_writer_owner() {
        let _shm = crate::human_principal_dispatch::tests::temp_shm_with_schemas(&[
            op_plugins::state_plugins::schema_tag_registry::schema_tag_registry_schema(),
        ]);
        let temporary = tempfile::tempdir().unwrap();
        let documents = Arc::new(RuntimeDocuments::new(op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap()));
        let store = RoutingStore::new(documents,
            PrivateProjectionDirectory::prepare(&temporary.path().join("private")).unwrap(),
            crate::sealed_schema_reader::blob_catalog_dir_from_env(), RoutingResources::new(
                crate::routing_resources::RoutingLimits { queue_wait_ms: 10, ..Default::default() }).unwrap());
        let owner = RegistryOwner { owner_id: "configured-policy-owner".into() };
        let guard = store.lock_registry(owner.clone()).await.unwrap();
        let mut wakes = store.subscribe(&guard).unwrap();
        let (entered_tx, entered_rx) = std::sync::mpsc::sync_channel(1);
        let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
        let worker_store = store.clone();
        let caller = tokio::spawn(async move {
            worker_store.mutate_registry(guard, uuid::Uuid::new_v4().to_string(), move |state| {
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
                Ok(state.revision)
            }, || Ok(())).await
        });
        tokio::task::spawn_blocking(move || entered_rx.recv_timeout(std::time::Duration::from_secs(2))).await.unwrap().unwrap();
        while wakes.try_recv().is_ok() {}
        caller.abort();
        assert!(store.lock_registry(owner.clone()).await.is_err(), "detached native owner must still hold exclusion");
        release_tx.send(()).unwrap();
        let wake = tokio::time::timeout(std::time::Duration::from_secs(2), wakes.recv()).await.unwrap().unwrap();
        assert_eq!(wake.revision, 1); assert!(wake.generation > 0);
        let state = store.read_registry(store.lock_registry(owner).await.unwrap(), || Ok(())).await.unwrap();
        assert_eq!(state.revision, 1);
    }
}
