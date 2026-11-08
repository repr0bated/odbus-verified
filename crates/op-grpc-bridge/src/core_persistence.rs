//! One bridge-owned Cozo append ledger; SHM is its committed read projection.
//! No filesystem snapshots, result buffer authority, or identity-store writes.
use anyhow::{Context, Result, bail, ensure};
use op_cozo_store::CozoGraphShuttle;
use op_core::projection_shm::{self, PrivateProjectionDirectory, ProjectionBatchPublication, ProjectionDocuments};
use op_state_store::{ChainEvent as AuditEvent, ChainConfig, EventChain, OperationType};
use op_state_store::method_receipt::MethodRecoveryStatus;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

use super::operations::OperationState;
use super::projection::{fold, path_segments, public_state, scope_key, select_object, state_hash, validate_scope};
use super::*;

const LEDGER: &str = "op-grpc-bridge-ledger-v1";
const RECORD_KIND: &str = "bridge_record_v1";
const MAX_RECORD_BYTES: usize = 64 * 1024 * 1024;

#[path = "core_state_commit.rs"]
mod state_commit;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Publication {
    pub scope: StateScope,
    pub plugin_id: String,
    pub schema_hash: String,
    pub object_path: String,
    pub operation_id: String,
    pub state: Option<Value>,
}

/// Attribution only. The dedicated identity owner remains the sole recovery
/// authority for the paired public metadata and private credentials.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IdentityPublication {
    schema_hash: String,
    state_hash: String,
    generation: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum LedgerAction {
    State { publications: Vec<Publication> },
    Attempt { operation_id: String, schema_hash: String, arguments_digest: String },
    Terminal {
        operation_id: String,
        status: MethodRecoveryStatus,
        code: String,
        publications: Vec<Publication>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        identity_publication: Option<IdentityPublication>,
    },
}

impl LedgerAction {
    pub(super) fn publications(&self) -> &[Publication] {
        match self {
            Self::State { publications } | Self::Terminal { publications, .. } => publications,
            Self::Attempt { .. } => &[],
        }
    }

    fn identity_publication(&self) -> Option<&IdentityPublication> {
        match self {
            Self::Terminal { identity_publication, .. } => identity_publication.as_ref(),
            _ => None,
        }
    }

    fn effective_hash(&self, plugin: &str) -> Result<Option<String>> {
        if let Some(publication) = self.publications().iter().find(|publication| publication.plugin_id == plugin) {
            return state_hash(publication.state.as_ref()).map(Some);
        }
        Ok((plugin == "identity_sled").then(|| self.identity_publication())
            .flatten().map(|publication| publication.state_hash.clone()))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LedgerRecord {
    version: u32,
    pub owner: StateScope,
    pub event: AuditEvent,
    pub action: LedgerAction,
}

struct ProjectionHead {
    event_id: u64,
    event_hash: String,
    schema_hash: String,
    state_hash: String,
}

struct IdentityProjectionHead {
    head: ProjectionHead,
    generation: u64,
}

pub(super) struct PersistentCore {
    db: CozoGraphShuttle,
    catalog_dir: PathBuf,
    state_dir: String,
    private_root: PrivateProjectionDirectory,
    tip: Option<AuditEvent>,
    heads: BTreeMap<(StateScope, String), ProjectionHead>,
    identity_head: Option<IdentityProjectionHead>,
    pub(super) operations: BTreeMap<(StateScope, String), OperationState>,
    failure: Option<String>,
}

pub(super) struct StateCommit {
    pub event: AuditEvent,
    pub published: Vec<(StateScope, String, u64)>,
    pub scope: StateScope,
    pub object_path: String,
}

fn genesis_hash() -> String {
    EventChain::new(ChainConfig { auto_batch: false, ..Default::default() })
        .last_hash().to_owned()
}

fn binding(owner: &StateScope, action: &LedgerAction) -> Result<String> {
    Ok(blake3::hash(&serde_json::to_vec(&(owner, action))?).to_hex().to_string())
}

pub(super) fn journal_event(event: &AuditEvent) -> JournalEvent {
    JournalEvent {
        event_id: event.event_id,
        prev_hash: event.prev_hash.clone(),
        event_hash: event.event_hash.clone(),
        timestamp: Timestamp {
            seconds: event.timestamp.timestamp(),
            nanos: event.timestamp.timestamp_subsec_nanos() as i32,
        },
        actor_id: event.actor_id.clone(),
        capability_id: event.capability_id.clone().unwrap_or_default(),
        plugin_id: event.plugin_id.clone(),
        schema_version: event.schema_version.clone(),
        operation_type: format!("{:?}", event.op),
        target: event.target.clone(),
        tags_touched: event.tags_touched.clone(),
        decision: if event.decision == op_state_store::Decision::Allow {
            Decision::Allow as i32
        } else { Decision::Deny as i32 },
        input_patch_hash: event.input_patch_hash.clone(),
        result_effective_hash: event.result_effective_hash.clone().unwrap_or_default(),
    }
}

impl PersistentCore {
    fn open(catalog: &Catalog, database_path: PathBuf) -> Result<Self> {
        let catalog_dir = std::env::var_os("OP_BLOB_CATALOG_DIR").map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(op_blob::catalog::DEFAULT_SHM_DIR));
        let state_dir = projection_shm::shm_state_dir();
        projection_shm::prepare_projection_directory_in(&state_dir)?;
        ensure!(projection_shm::pending_identity_publication_in(&state_dir)?.is_none(),
            "identity publication is pending; its identity owner must recover it before bridge activation");
        let private_root = PrivateProjectionDirectory::prepare(
            &std::path::Path::new(&state_dir).join(".bridge-private"),
        )?;
        private_root.ensure_distinct_from(std::path::Path::new(&state_dir))?;
        let mut owner = Self {
            db: CozoGraphShuttle::new_persistent_private(database_path)?,
            catalog_dir, state_dir, private_root, tip: None,
            heads: BTreeMap::new(), identity_head: None,
            operations: BTreeMap::new(), failure: None,
        };
        for plugin in catalog.plugins() {
            owner.pin(catalog, &plugin.id, &plugin.schema_hash)?;
        }
        // Replay verifies every persisted event and scope binding. Only each
        // scope/plugin's final complete tree is retained for SHM reconstruction.
        let mut latest = BTreeMap::new();
        let replay_db = owner.db.clone();
        Self::walk_records(&replay_db, |record| {
            owner.accept_metadata(&record)?;
            let event_id = record.event.event_id;
            match record.action {
                LedgerAction::State { publications }
                | LedgerAction::Terminal { publications, .. } => {
                    for publication in publications {
                        latest.insert((publication.scope.clone(), publication.plugin_id.clone()), (event_id, publication));
                    }
                }
                _ => {}
            }
            Ok(())
        })?;
        // Do not clear another owner's pending marker, particularly identity's
        // credential publication. An interrupted bridge marker must name an
        // exact durable record (or an earlier retained durable tree).
        if let Some(pending) = projection_shm::pending_projection_publication_in(&owner.state_dir)? {
            owner.recover_pending(catalog, &StateScope::Public, &pending, &latest)?;
        }
        let private_scopes: std::collections::BTreeSet<_> = owner.heads.keys()
            .chain(owner.operations.keys())
            .filter_map(|(scope, _)| matches!(scope, StateScope::Private { .. }).then_some(scope.clone()))
            .collect();
        for scope in private_scopes {
            if let Some(directory) = owner.private_directory(&scope, false)? {
                if let Some(pending) = directory.pending_document()? {
                    owner.recover_pending(catalog, &scope, &pending, &latest)?;
                }
            }
        }
        let mut recovered = BTreeMap::<StateScope, ProjectionDocuments>::new();
        for (_, (_, publication)) in latest {
            if catalog.plugin(&publication.plugin_id).is_none() { continue; }
            owner.pin(catalog, &publication.plugin_id, &publication.schema_hash)?;
            recovered.entry(publication.scope).or_default().insert(publication.plugin_id,
                publication.state.as_ref().map(serde_json::to_vec).transpose()?);
        }
        for (scope, documents) in recovered {
            owner.publish_recovered(&scope, documents)?;
        }
        Ok(owner)
    }

    pub(super) fn require_available(&self) -> Result<()> {
        if let Some(reason) = &self.failure {
            bail!("bridge durable outcome unresolved; reopen for recovery: {reason}");
        }
        Ok(())
    }

    pub(super) fn at_event_id(&self) -> u64 {
        self.tip.as_ref().map_or(0, |event| event.event_id)
    }

    pub(super) fn pin(&self, catalog: &Catalog, plugin_id: &str, expected: &str) -> Result<op_state_store::PluginSchema> {
        let selected = catalog.plugin(plugin_id).context("plugin absent from admitted catalog")?;
        ensure!(selected.schema_hash == expected && selected.sealed_schema_json.is_some(), "state schema pin mismatch");
        let current = Catalog::sealed_plugin(&self.catalog_dir, plugin_id)?;
        ensure!(current.schema_hash == selected.schema_hash
            && current.sealed_schema_json == selected.sealed_schema_json,
            "sealed plugin changed since bridge admission");
        serde_json::from_value(current.schema.context("sealed plugin schema unavailable")?).map_err(Into::into)
    }

    fn private_directory(&self, scope: &StateScope, create: bool) -> Result<Option<PrivateProjectionDirectory>> {
        ensure!(matches!(scope, StateScope::Private { .. }), "public state is not a private namespace");
        let key = scope_key(scope)?;
        if create { Ok(Some(self.private_root.prepare_child(&key)?)) }
        else { self.private_root.open_child(&key) }
    }

    fn walk_records(db: &CozoGraphShuttle, mut visit: impl FnMut(LedgerRecord) -> Result<()>) -> Result<()> {
        let mut sequence = 0_i64;
        let mut previous = genesis_hash();
        db.visit_session_events_durable(LEDGER, |row| {
            (|| -> Result<()> {
                ensure!(row.seq == sequence && row.kind == RECORD_KIND && row.content.len() <= MAX_RECORD_BYTES,
                    "bridge ledger sequence or record encoding invalid");
                let record: LedgerRecord = serde_json::from_str(&row.content).context("decode bridge ledger record")?;
                ensure!(record.version == 1 && record.event.event_id == sequence as u64 + 1
                    && record.event.prev_hash == previous && record.event.verify(),
                    "bridge ledger event hash or linkage invalid");
                validate_scope(&record.owner)?;
                let payload = record.event.input_payload.as_ref().context("bridge event binding absent")?;
                ensure!(serde_json::to_value(payload)? == serde_json::json!({"record_digest": binding(&record.owner, &record.action)?}),
                    "bridge ledger state/scope binding invalid");
                if let StateScope::Private { principal_id, session_id } = &record.owner {
                    ensure!(&record.event.actor_id == principal_id && record.event.session_id.as_ref() == Some(session_id),
                        "bridge ledger actor/session binding invalid");
                }
                let expected_hash = record.action.effective_hash(&record.event.plugin_id)?;
                ensure!(record.event.result_effective_hash == expected_hash, "bridge ledger state digest invalid");
                if let Some(publication) = record.action.identity_publication() {
                    ensure!(publication.generation > 0 && !publication.schema_hash.is_empty()
                        && blake3::Hash::from_hex(&publication.state_hash).is_ok(),
                        "identity publication attribution is invalid");
                }
                for publication in record.action.publications() {
                    ensure!(publication.state.as_ref().is_none_or(Value::is_object), "durable state root is not an object");
                    validate_scope(&publication.scope)?;
                    path_segments(&publication.plugin_id, &publication.object_path)?;
                }
                previous = record.event.event_hash.clone();
                sequence = sequence.checked_add(1).context("bridge ledger sequence exhausted")?;
                visit(record)?;
                Ok(())
            })().map_err(|error| op_cozo_store::CozoError::Other(error.to_string()))
        })?;
        Ok(())
    }

    pub(super) fn accept_metadata(&mut self, record: &LedgerRecord) -> Result<()> {
        self.accept_operation(record)?;
        for publication in record.action.publications() {
            self.heads.insert((publication.scope.clone(), publication.plugin_id.clone()), ProjectionHead {
                event_id: record.event.event_id,
                event_hash: record.event.event_hash.clone(),
                schema_hash: publication.schema_hash.clone(),
                state_hash: state_hash(publication.state.as_ref())?,
            });
        }
        if let Some(publication) = record.action.identity_publication() {
            self.identity_head = Some(IdentityProjectionHead {
                head: ProjectionHead {
                    event_id: record.event.event_id,
                    event_hash: record.event.event_hash.clone(),
                    schema_hash: publication.schema_hash.clone(),
                    state_hash: publication.state_hash.clone(),
                },
                generation: publication.generation,
            });
        }
        self.tip = Some(record.event.clone());
        Ok(())
    }

    pub(super) fn append(
        &mut self,
        catalog: &Catalog,
        owner: StateScope,
        op: &CoreMutation,
        schema_hash: &str,
        action: LedgerAction,
    ) -> Result<AuditEvent> {
        self.require_available()?;
        validate_scope(&owner)?;
        let schema = self.pin(catalog, &op.plugin_id, schema_hash)?;
        let payload = simd_json::json!({"record_digest": binding(&owner, &action)?});
        let kind = match &action {
            LedgerAction::Attempt { .. } => OperationType::MethodCall,
            LedgerAction::Terminal { .. } => OperationType::Custom("method_outcome".into()),
            LedgerAction::State { .. } => OperationType::PropertySet,
        };
        let decision = if matches!(&action, LedgerAction::Terminal { status: MethodRecoveryStatus::Failed, .. }) {
            op_state_store::Decision::Deny
        } else { op_state_store::Decision::Allow };
        let event_id = self.at_event_id().checked_add(1).filter(|id| *id <= i64::MAX as u64)
            .context("bridge event identity exhausted")?;
        let mut event = AuditEvent::new(event_id,
            self.tip.as_ref().map_or_else(genesis_hash, |event| event.event_hash.clone()),
            op.actor_id.clone(), op.plugin_id.clone(), schema.version, kind,
            format!("{}:{}", op.object_path, op.member_name), schema.tags, decision, &payload);
        if let StateScope::Private { principal_id, session_id } = &owner {
            ensure!(op.actor_id == *principal_id, "operation actor differs from verified principal");
            event.session_id = Some(session_id.clone());
        }
        event.capability_id = Some(op.capability_id.clone());
        event.method_name = (!op.member_name.is_empty()).then(|| op.member_name.clone());
        event.result_effective_hash = action.effective_hash(&op.plugin_id)?;
        event.event_hash = event.compute_hash();
        let record = LedgerRecord { version: 1, owner, event, action };
        let encoded = serde_json::to_string(&record)?;
        ensure!(encoded.len() <= MAX_RECORD_BYTES, "bridge ledger record exceeds byte bound");
        let appended = self.db.append_session_event(LEDGER, RECORD_KIND, &op.capability_id,
            &encoded, record.event.timestamp.timestamp());
        match appended {
            Ok(sequence) if sequence as u64 + 1 == event_id => {
                if let Err(error) = self.accept_metadata(&record) {
                    self.failure = Some("durable ledger metadata reconciliation failed".into());
                    return Err(error);
                }
                Ok(record.event)
            }
            Ok(_) => {
                self.failure = Some("durable ledger event identity mismatch".into());
                bail!("bridge ledger identity mismatch; outcome unknown; do not retry");
            }
            Err(_) => {
                self.failure = Some("Cozo append or persistence barrier uncertain".into());
                bail!("bridge ledger append outcome unknown; reopen for recovery; do not retry");
            }
        }
    }

    fn recover_pending(
        &self,
        catalog: &Catalog,
        scope: &StateScope,
        pending: &projection_shm::PendingDocumentPublication,
        latest: &BTreeMap<(StateScope, String), (u64, Publication)>,
    ) -> Result<()> {
        // Only bridge-generated publication tokens can be reconciled here.
        ensure!(pending.operation_id.starts_with("bridge-"), "pending state publication belongs to another owner");
        let ids = if pending.plugin_ids.is_empty() {
            vec![pending.plugin_id.clone()]
        } else { pending.plugin_ids.clone() };
        let mut documents = ProjectionDocuments::new();
        for id in &ids {
            let (_, publication) = latest.get(&(scope.clone(), id.clone()))
                .context("pending bridge publication has no durable predecessor; explicit recovery required")?;
            self.pin(catalog, id, &publication.schema_hash)?;
            documents.insert(id.clone(), publication.state.as_ref().map(serde_json::to_vec).transpose()?);
        }
        let result = match scope {
            StateScope::Public => projection_shm::recover_projection_batch_in(&self.state_dir,
                &ids, &pending.operation_id, || Ok((documents, ())))?,
            StateScope::Private { .. } => self.private_directory(scope, false)?.context("private namespace absent")?
                .recover_documents(&ids, &pending.operation_id, || Ok((documents, ())))?,
        };
        match result {
            ProjectionBatchPublication::Committed { .. } => Ok(()),
            ProjectionBatchPublication::Pending { error, .. } => Err(error.context("bridge projection remains pending")),
        }
    }

    fn publish_recovered(&self, scope: &StateScope, documents: ProjectionDocuments) -> Result<()> {
        let token = format!("bridge-{}", uuid::Uuid::new_v4());
        let ids: Vec<_> = documents.keys().cloned().collect();
        let result = match scope {
            StateScope::Public => projection_shm::commit_projection_batch_durable_in(&self.state_dir,
                &ids, &token, |_| Ok((documents, ())))?,
            StateScope::Private { .. } => self.private_directory(scope, true)?.expect("prepared namespace")
                .commit_documents_durable(&ids, &token, |_| Ok((documents, ())))?,
        };
        match result {
            ProjectionBatchPublication::Committed { .. } => Ok(()),
            ProjectionBatchPublication::Pending { error, .. } => Err(error.context("bridge state recovery publication pending")),
        }
    }

    pub(super) fn commit(
        &mut self,
        catalog: &Catalog,
        owner: StateScope,
        commit: BackendCommit,
        terminal: Option<(String, MethodRecoveryStatus, String)>,
    ) -> Result<StateCommit> {
        self.commit_with_updates(catalog, owner, commit, Vec::new(), terminal)
    }

    fn snapshot(&self, catalog: &Catalog, scope: &StateScope, plugin: &str, path: &str, redact_public: bool) -> Result<Option<CommittedStateSnapshot>> {
        self.require_available()?;
        validate_scope(scope)?;
        path_segments(plugin, path)?;
        let selected = catalog.plugin(plugin).context("plugin absent from sealed catalog")?;
        let schema = self.pin(catalog, plugin, &selected.schema_hash)?;
        let ids = [plugin.to_owned()];
        let snapshot = match scope {
            StateScope::Public => projection_shm::read_projection_snapshot_in(&self.state_dir, &ids)?,
            StateScope::Private { .. } => {
                let Some(directory) = self.private_directory(scope, false)? else {
                    ensure!(!self.heads.contains_key(&(scope.clone(), plugin.to_owned())),
                        "durable private namespace is missing; explicit recovery required");
                    return Ok(None);
                };
                directory.read_snapshot(&ids)?
            }
        };
        self.pin(catalog, plugin, &selected.schema_hash)?;
        let Some(bytes) = snapshot.states.get(plugin) else {
            if let Some(head) = self.heads.get(&(scope.clone(), plugin.to_owned())) {
                ensure!(head.state_hash == state_hash(None)?, "durable state is missing from committed SHM");
            }
            return Ok(None);
        };
        let raw: Value = serde_json::from_slice(bytes)?;
        let head = self.heads.get(&(scope.clone(), plugin.to_owned()));
        let mut event_id = head.map_or(0, |head| head.event_id);
        if let Some(head) = head {
            ensure!(head.schema_hash == selected.schema_hash && head.state_hash == state_hash(Some(&raw))?,
                "committed SHM differs from the durable bridge state");
        } else if *scope == StateScope::Public && plugin == "identity_sled" {
            if let Some(identity) = &self.identity_head {
                // A dedicated owner may legitimately publish newer metadata
                // outside this ledger. Never fence or misattribute that state.
                if identity.head.schema_hash == selected.schema_hash
                    && snapshot.generation >= identity.generation
                    && identity.head.state_hash == state_hash(Some(&raw))? {
                    event_id = identity.head.event_id;
                }
            }
        }
        let value = match scope {
            StateScope::Public if redact_public => {
                let Some(public) = public_state(plugin, &schema, raw)? else { return Ok(None) };
                public
            }
            _ => raw,
        };
        let Some(object) = select_object(&value, plugin, path)? else { return Ok(None) };
        Ok(Some(CommittedStateSnapshot {
            effective_hash: state_hash(Some(object))?, state: object.clone(),
            event_id,
            generation: snapshot.generation,
            schema_hash: selected.schema_hash.clone(),
        }))
    }

    fn frame(&self, catalog: &Catalog, scope: &StateScope, plugin: &str, initial: bool) -> Result<Option<StateChange>> {
        let path = op_plugins::canonical::plugin_path(plugin);
        let snapshot = self.snapshot(catalog, scope, plugin, &path, true)?;
        let head = self.heads.get(&(scope.clone(), plugin.to_owned())).or_else(|| {
            (*scope == StateScope::Public && plugin == "identity_sled")
                .then_some(self.identity_head.as_ref()).flatten()
                .filter(|identity| snapshot.as_ref().is_some_and(|state| state.event_id == identity.head.event_id))
                .map(|identity| &identity.head)
        });
        if snapshot.is_none() && (initial || head.is_none()) { return Ok(None) }
        let entry = catalog.plugin(plugin).context("state plugin absent from catalog")?;
        // No frame for a sealed private-only policy on the public channel.
        if matches!(scope, StateScope::Public) && snapshot.is_none() {
            let schema = self.pin(catalog, plugin, &entry.schema_hash)?;
            if public_state(plugin, &schema, serde_json::json!({}))?.is_none() { return Ok(None) }
        }
        let generation = match &snapshot {
            Some(snapshot) => snapshot.generation,
            None => match scope {
                StateScope::Public => projection_shm::read_projection_snapshot_in(&self.state_dir, &[plugin.to_owned()])?.generation,
                StateScope::Private { .. } => self.private_directory(scope, false)?.context("private state namespace absent")?
                    .read_snapshot(&[plugin.to_owned()])?.generation,
            },
        };
        Ok(Some(StateChange {
            change_id: uuid::Uuid::new_v4().to_string(),
            event_id: head.map_or(0, |head| head.event_id),
            plugin_id: plugin.to_owned(), object_path: path,
            change_type: if snapshot.is_some() { ChangeType::PropertySet as i32 } else { ChangeType::ObjectRemoved as i32 },
            member_name: String::new(), old_value: None,
            new_value: snapshot.as_ref().map(|snapshot| json_to_proto_value(&snapshot.state)),
            tags_touched: entry.tags.clone(), event_hash: head.map(|head| head.event_hash.clone()).unwrap_or_default(),
            timestamp: Some(now_timestamp()), actor_id: String::new(),
            frame_kind: if initial { StateFrameKind::InitialState as i32 } else { StateFrameKind::Update as i32 },
            schema_hash: entry.schema_hash.clone(), catalog_hash: catalog.hash().to_owned(),
            state_generation: generation,
        }))
    }
}

impl BridgeCore {
    /// Open the bridge-owned RocksDB/Cozo ledger, verify it, and reconstruct its
    /// committed SHM projections before returning. Never falls back to memory.
    pub fn open(catalog: Catalog, database_path: PathBuf) -> Result<Self> {
        let persistent = PersistentCore::open(&catalog, database_path)?;
        let (notice_tx, _) = broadcast::channel(1024);
        Ok(Self { inner: Arc::new(BridgeCoreInner {
            commit: Mutex::new(()), catalog, fixture: None,
            persistent: Some(Mutex::new(persistent)), notice_tx: Some(notice_tx),
        }) })
    }

    /// Metadata-only durable wakes. Consumers must filter scope and use the
    /// state_committed bit before emitting state-change doorbells.
    pub fn subscribe_commits(&self) -> broadcast::Receiver<StateNotice> {
        self.notices().subscribe()
    }

    /// Explicit recovery for an admitted domain-recovery method. Reads never
    /// call this. Unknown multi-namespace outcomes require a full reopen, while
    /// a missing durable predecessor is an error rather than an invented tree.
    pub fn recover_projection_from_ledger(&self, scope: &StateScope, plugin_id: &str) -> Result<()> {
        validate_scope(scope)?;
        ensure!(plugin_id != "identity_sled", "identity recovery belongs to its paired owner");
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let mut owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        let mut latest = None;
        PersistentCore::walk_records(&owner.db, |record| {
            let publications = match record.action {
                LedgerAction::State { publications } | LedgerAction::Terminal { publications, .. } => publications,
                LedgerAction::Attempt { .. } => Vec::new(),
            };
            for publication in publications {
                if &publication.scope == scope && publication.plugin_id == plugin_id {
                    latest = Some((record.event.event_id, publication));
                }
            }
            Ok(())
        })?;
        let (event_id, publication) = latest.context("no durable bridge tree exists for explicit recovery")?;
        owner.pin(&self.inner.catalog, plugin_id, &publication.schema_hash)?;
        let pending = match scope {
            StateScope::Public => projection_shm::pending_projection_publication_in(&owner.state_dir)?,
            StateScope::Private { .. } => owner.private_directory(scope, false)?
                .map(|directory| directory.pending_document()).transpose()?.flatten(),
        };
        let outcome = if let Some(pending) = pending {
            ensure!(pending.plugin_id == plugin_id && (pending.plugin_ids.is_empty()
                || (pending.plugin_ids.len() == 1 && pending.plugin_ids[0] == plugin_id)),
                "multi-plugin pending publication requires full durable reopen");
            owner.recover_pending(&self.inner.catalog, scope, &pending,
                &BTreeMap::from([((scope.clone(), plugin_id.to_owned()), (event_id, publication))]))
        } else {
            let documents = BTreeMap::from([(plugin_id.to_owned(),
                publication.state.as_ref().map(serde_json::to_vec).transpose()?)]);
            owner.publish_recovered(scope, documents)
        };
        if outcome.is_err() {
            owner.failure = Some("explicit projection recovery did not commit".into());
        }
        outcome
    }

    pub fn committed_snapshot(&self, scope: &StateScope, plugin_id: &str, object_path: &str) -> Result<Option<CommittedStateSnapshot>> {
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.snapshot(&self.inner.catalog, scope, plugin_id, object_path, true)
    }

    /// Native policy evaluation only. Never serialize this raw observation to
    /// a consumer; the caller must first admit the exact target method.
    pub(crate) fn backend_snapshot(&self, scope: &StateScope, plugin_id: &str, object_path: &str) -> Result<Option<CommittedStateSnapshot>> {
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.snapshot(&self.inner.catalog, scope, plugin_id, object_path, false)
    }

    pub fn committed_subscription(&self, scope: &StateScope, include_initial_state: bool)
        -> Result<(Vec<StateChange>, broadcast::Receiver<StateNotice>)> {
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        validate_scope(scope)?;
        let receiver = self.notices().subscribe();
        let mut frames = Vec::new();
        if include_initial_state {
            for plugin in self.inner.catalog.plugins() {
                if matches!(scope, StateScope::Private { .. }) && plugin.id == "identity_sled" { continue; }
                if let Some(frame) = owner.frame(&self.inner.catalog, scope, &plugin.id, true)? { frames.push(frame); }
            }
        }
        Ok((frames, receiver))
    }

    pub fn committed_frame(&self, scope: &StateScope, notice: &StateNotice) -> Result<Option<StateChange>> {
        if scope != &notice.scope || !notice.state_committed { return Ok(None) }
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.frame(&self.inner.catalog, scope, &notice.plugin_id, false)
    }

    pub fn journal_events(&self, scope: &StateScope, from: u64, to: u64) -> Result<Vec<JournalEvent>> {
        validate_scope(scope)?;
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().context("durable bridge required")?
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        let mut events = Vec::new();
        PersistentCore::walk_records(&owner.db, |record| {
            if &record.owner == scope && (from == 0 || record.event.event_id >= from)
                && (to == 0 || record.event.event_id <= to) { events.push(journal_event(&record.event)); }
            Ok(())
        })?;
        Ok(events)
    }

    pub fn verify_journal(&self, from: u64, to: u64) -> Result<ChainVerification> {
        if self.inner.persistent.is_none() { return Ok(self.journal().verify(from, to)); }
        let _boundary = self.inner.commit.lock().map_err(|_| anyhow::anyhow!("bridge commit owner poisoned"))?;
        let owner = self.inner.persistent.as_ref().expect("checked persistent owner")
            .lock().map_err(|_| anyhow::anyhow!("durable bridge owner poisoned"))?;
        owner.require_available()?;
        let mut count = 0;
        PersistentCore::walk_records(&owner.db, |record| {
            if (from == 0 || record.event.event_id >= from) && (to == 0 || record.event.event_id <= to) { count += 1; }
            Ok(())
        })?;
        Ok(ChainVerification { valid: true, events_verified: count, errors: Vec::new() })
    }

    pub fn commit_backend_state(&self, mut commit: BackendCommit) -> std::result::Result<MutationOutcome, CoreRejection> {
        let result = commit.result.take();
        let _boundary = self.inner.commit.lock().map_err(|_| reject(ErrorCode::Internal, "bridge commit owner poisoned"))?;
        let mut owner = self.inner.persistent.as_ref().ok_or_else(|| reject(ErrorCode::Internal, "durable bridge required"))?
            .lock().map_err(|_| reject(ErrorCode::Internal, "durable bridge owner poisoned"))?;
        let state = owner.commit(&self.inner.catalog, commit.scope.clone(), commit, None)
            .map_err(|_| reject(ErrorCode::Internal, "authoritative state commit failed; inspect durable recovery before retry"))?;
        let event = journal_event(&state.event);
        let change = owner.frame(&self.inner.catalog, &state.scope, &state.event.plugin_id, false)
            .map_err(|_| reject(ErrorCode::Internal, "committed state projection unavailable"))?
            .unwrap_or_else(|| self.metadata_frame(&state));
        self.notify_committed(&state);
        Ok(MutationOutcome { event, change, result })
    }

    pub(super) fn notify_committed(&self, state: &StateCommit) {
        for (scope, plugin_id, _) in &state.published {
            let _ = self.notices().send(StateNotice {
                scope: scope.clone(), plugin_id: plugin_id.clone(), event_id: state.event.event_id,
                state_committed: true,
            });
        }
    }

    fn metadata_frame(&self, state: &StateCommit) -> StateChange {
        StateChange {
            change_id: uuid::Uuid::new_v4().to_string(), event_id: state.event.event_id,
            plugin_id: state.event.plugin_id.clone(), object_path: state.object_path.clone(),
            change_type: ChangeType::MethodCall as i32, member_name: state.event.method_name.clone().unwrap_or_default(),
            old_value: None, new_value: None, tags_touched: state.event.tags_touched.clone(),
            event_hash: state.event.event_hash.clone(), timestamp: Some(journal_event(&state.event).timestamp),
            actor_id: state.event.actor_id.clone(), frame_kind: StateFrameKind::Update as i32,
            schema_hash: self.inner.catalog.plugin(&state.event.plugin_id).map(|plugin| plugin.schema_hash.clone()).unwrap_or_default(),
            catalog_hash: self.inner.catalog.hash().to_owned(),
            state_generation: state.published.iter().find(|(scope, plugin, _)| scope == &state.scope && plugin == &state.event.plugin_id)
                .map_or(0, |(_, _, generation)| *generation),
        }
    }
}
