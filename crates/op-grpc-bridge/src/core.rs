//! Catalog-driven bridge state and its append-only audit ledger.
//!
//! [`BridgeCore::open`] uses durable Cozo records and committed canonical SHM.
//! [`BridgeCore::new`] and [`Journal`] are isolated in-memory contract fixtures;
//! their BLAKE3 verification is not a substitute for the production ledger's
//! canonical `op_state_store::ChainEvent` proofs.

use crate::catalog::Catalog;
use crate::proto::{ChainEvent, ChangeType, Decision, ErrorCode, StateChange, StateFrameKind};
use crate::router::{Handler, Router, RouterError};
use prost_types::{Struct as ProtoStruct, Timestamp, Value as ProtoValue};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

#[path = "core_persistence.rs"]
mod persistence;
#[path = "core_projection.rs"]
mod projection;
#[path = "core_operations.rs"]
mod operations;
pub use operations::MethodOperation;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Current wall-clock time as a protobuf Timestamp.
pub fn now_timestamp() -> Timestamp {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    Timestamp {
        seconds: elapsed.as_secs() as i64,
        nanos: elapsed.subsec_nanos() as i32,
    }
}

/// Canonical JSON bytes for hashing (`serde_json::Map` is a BTreeMap, so key
/// order is stable).
fn canonical_json(value: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_default()
}

/// protobuf `Value` -> JSON. Non-finite doubles become null (JSON has no NaN).
pub fn proto_value_to_json(value: &ProtoValue) -> serde_json::Value {
    use prost_types::value::Kind;
    match &value.kind {
        None | Some(Kind::NullValue(_)) => serde_json::Value::Null,
        Some(Kind::NumberValue(n)) => serde_json::Number::from_f64(*n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Some(Kind::StringValue(s)) => serde_json::Value::String(s.clone()),
        Some(Kind::BoolValue(b)) => serde_json::Value::Bool(*b),
        Some(Kind::StructValue(s)) => serde_json::Value::Object(
            s.fields
                .iter()
                .map(|(k, v)| (k.clone(), proto_value_to_json(v)))
                .collect(),
        ),
        Some(Kind::ListValue(l)) => {
            serde_json::Value::Array(l.values.iter().map(proto_value_to_json).collect())
        }
    }
}

/// JSON -> protobuf `Value`.
pub fn json_to_proto_value(value: &serde_json::Value) -> ProtoValue {
    use prost_types::value::Kind;
    let kind = match value {
        serde_json::Value::Null => Kind::NullValue(0),
        serde_json::Value::Bool(b) => Kind::BoolValue(*b),
        serde_json::Value::Number(n) => Kind::NumberValue(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Kind::StringValue(s.clone()),
        serde_json::Value::Array(items) => Kind::ListValue(prost_types::ListValue {
            values: items.iter().map(json_to_proto_value).collect(),
        }),
        serde_json::Value::Object(map) => Kind::StructValue(json_map_to_proto_struct(map)),
    };
    ProtoValue { kind: Some(kind) }
}

/// JSON object -> protobuf `Struct`.
pub fn json_map_to_proto_struct(
    map: &serde_json::Map<String, serde_json::Value>,
) -> ProtoStruct {
    ProtoStruct {
        fields: map
            .iter()
            .map(|(k, v)| (k.clone(), json_to_proto_value(v)))
            .collect(),
    }
}

fn deep_merge(
    base: &mut serde_json::Map<String, serde_json::Value>,
    patch: &serde_json::Map<String, serde_json::Value>,
) {
    for (key, value) in patch {
        match (base.get_mut(key), value) {
            (Some(serde_json::Value::Object(b)), serde_json::Value::Object(p)) => {
                deep_merge(b, p);
            }
            _ => {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}

// ---------------------------------------------------------------------------
// StateStore
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct StateStoreInner {
    /// `(plugin_id, object_path)` -> member -> value.
    objects: HashMap<(String, String), serde_json::Map<String, serde_json::Value>>,
    /// Monotonic generation, bumped by every accepted state write.
    generation: u64,
}

/// In-memory object state, keyed by `(plugin_id, object_path)`.
#[derive(Debug, Default)]
pub struct StateStore {
    inner: RwLock<StateStoreInner>,
}

impl StateStore {
    /// An empty store at generation 0.
    pub fn new() -> Self {
        Self::default()
    }

    /// Current generation (bumped by every accepted state write).
    pub fn generation(&self) -> u64 {
        self.inner.read().expect("store lock poisoned").generation
    }

    fn hash_map(map: &serde_json::Map<String, serde_json::Value>) -> String {
        blake3::hash(&canonical_json(&serde_json::Value::Object(map.clone())))
            .to_hex()
            .to_string()
    }

    /// Whole-object read plus its effective hash.
    pub fn get_object(
        &self,
        plugin_id: &str,
        object_path: &str,
    ) -> Option<(serde_json::Map<String, serde_json::Value>, String)> {
        let inner = self.inner.read().expect("store lock poisoned");
        inner
            .objects
            .get(&(plugin_id.to_owned(), object_path.to_owned()))
            .map(|map| (map.clone(), Self::hash_map(map)))
    }

    /// Single-member read.
    pub fn get_member(
        &self,
        plugin_id: &str,
        object_path: &str,
        member: &str,
    ) -> Option<serde_json::Value> {
        self.inner
            .read()
            .expect("store lock poisoned")
            .objects
            .get(&(plugin_id.to_owned(), object_path.to_owned()))
            .and_then(|map| map.get(member).cloned())
    }

    /// Effective (content) hash of one object, if present.
    pub fn effective_hash(&self, plugin_id: &str, object_path: &str) -> Option<String> {
        self.inner
            .read()
            .expect("store lock poisoned")
            .objects
            .get(&(plugin_id.to_owned(), object_path.to_owned()))
            .map(Self::hash_map)
    }

    /// Set one member (creating the object if needed). Returns the previous
    /// value, if any, and the new effective hash. Bumps the generation.
    pub fn set_member(
        &self,
        plugin_id: &str,
        object_path: &str,
        member: &str,
        value: serde_json::Value,
    ) -> (Option<serde_json::Value>, String) {
        let mut inner = self.inner.write().expect("store lock poisoned");
        let map = inner
            .objects
            .entry((plugin_id.to_owned(), object_path.to_owned()))
            .or_default();
        let old = map.insert(member.to_owned(), value);
        let hash = Self::hash_map(map);
        inner.generation += 1;
        (old, hash)
    }

    /// Deep-merge an object patch (creating the object if needed). Returns the
    /// new effective hash. Bumps the generation.
    pub fn apply_patch(
        &self,
        plugin_id: &str,
        object_path: &str,
        patch: &serde_json::Map<String, serde_json::Value>,
    ) -> String {
        let mut inner = self.inner.write().expect("store lock poisoned");
        let map = inner
            .objects
            .entry((plugin_id.to_owned(), object_path.to_owned()))
            .or_default();
        deep_merge(map, patch);
        let hash = Self::hash_map(map);
        inner.generation += 1;
        hash
    }

    /// Every object, sorted by `(plugin_id, object_path)`.
    pub fn snapshot(&self) -> Vec<(String, String, serde_json::Map<String, serde_json::Value>)> {
        let inner = self.inner.read().expect("store lock poisoned");
        let mut out: Vec<_> = inner
            .objects
            .iter()
            .map(|((plugin, path), map)| (plugin.clone(), path.clone(), map.clone()))
            .collect();
        out.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
        out
    }
}

// ---------------------------------------------------------------------------
// Journal
// ---------------------------------------------------------------------------

/// One event in the hash chain. `event_hash` is blake3 over the canonical
/// preimage of every other field (see `hash_event`).
#[derive(Clone, Debug)]
pub struct JournalEvent {
    pub event_id: u64,
    pub prev_hash: String,
    pub event_hash: String,
    pub timestamp: Timestamp,
    pub actor_id: String,
    pub capability_id: String,
    pub plugin_id: String,
    pub schema_version: String,
    pub operation_type: String,
    pub target: String,
    pub tags_touched: Vec<String>,
    /// [`Decision`] as i32; the fresh core only journals `ALLOW`.
    pub decision: i32,
    pub input_patch_hash: String,
    pub result_effective_hash: String,
}

/// Everything [`Journal::append`] needs; ids, timestamps and hashes are assigned.
#[derive(Clone, Debug, Default)]
pub struct JournalDraft {
    pub actor_id: String,
    pub capability_id: String,
    pub plugin_id: String,
    pub schema_version: String,
    pub operation_type: String,
    pub target: String,
    pub tags_touched: Vec<String>,
    pub input_patch_hash: String,
    pub result_effective_hash: String,
}

fn hash_event(
    event_id: u64,
    prev_hash: &str,
    timestamp: &Timestamp,
    draft: &JournalDraft,
    decision: i32,
) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&event_id.to_le_bytes());
    hasher.update(prev_hash.as_bytes());
    hasher.update(b"\0");
    hasher.update(&timestamp.seconds.to_le_bytes());
    hasher.update(&timestamp.nanos.to_le_bytes());
    for field in [
        &draft.actor_id,
        &draft.capability_id,
        &draft.plugin_id,
        &draft.schema_version,
        &draft.operation_type,
        &draft.target,
    ] {
        hasher.update(field.as_bytes());
        hasher.update(b"\0");
    }
    for tag in &draft.tags_touched {
        hasher.update(tag.as_bytes());
        hasher.update(b"\0");
    }
    hasher.update(&decision.to_le_bytes());
    hasher.update(draft.input_patch_hash.as_bytes());
    hasher.update(draft.result_effective_hash.as_bytes());
    hasher.finalize().to_hex().to_string()
}

impl From<&JournalEvent> for ChainEvent {
    fn from(event: &JournalEvent) -> Self {
        ChainEvent {
            event_id: event.event_id,
            prev_hash: event.prev_hash.clone(),
            event_hash: event.event_hash.clone(),
            timestamp: Some(event.timestamp.clone()),
            actor_id: event.actor_id.clone(),
            capability_id: event.capability_id.clone(),
            plugin_id: event.plugin_id.clone(),
            schema_version: event.schema_version.clone(),
            operation_type: event.operation_type.clone(),
            target: event.target.clone(),
            tags_touched: event.tags_touched.clone(),
            decision: event.decision,
            deny_reason: None,
            input_patch_hash: event.input_patch_hash.clone(),
            result_effective_hash: event.result_effective_hash.clone(),
        }
    }
}

/// Outcome of [`Journal::verify`].
#[derive(Clone, Debug, Default)]
pub struct ChainVerification {
    /// True when every in-range event hashed and linked correctly
    /// (vacuously true for an empty range).
    pub valid: bool,
    /// In-range events that passed both the hash and linkage checks.
    pub events_verified: u64,
    pub errors: Vec<String>,
}

/// Append-only hash-chained audit log. Event ids start at 1 and increase by 1.
#[derive(Debug, Default)]
pub struct Journal {
    inner: Mutex<Vec<JournalEvent>>,
}

impl Journal {
    /// An empty journal.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of journaled events.
    pub fn len(&self) -> usize {
        self.inner.lock().expect("journal lock poisoned").len()
    }

    /// True when nothing has been journaled yet.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The latest event, if any.
    pub fn tip(&self) -> Option<JournalEvent> {
        self.inner
            .lock()
            .expect("journal lock poisoned")
            .last()
            .cloned()
    }

    /// Append one event, assigning its id, timestamp, linkage and hash.
    pub fn append(&self, draft: JournalDraft) -> JournalEvent {
        let mut events = self.inner.lock().expect("journal lock poisoned");
        let event_id = events.len() as u64 + 1;
        let prev_hash = events
            .last()
            .map(|event| event.event_hash.clone())
            .unwrap_or_default();
        let timestamp = now_timestamp();
        let decision = Decision::Allow as i32;
        let event_hash = hash_event(event_id, &prev_hash, &timestamp, &draft, decision);
        let event = JournalEvent {
            event_id,
            prev_hash,
            event_hash,
            timestamp,
            actor_id: draft.actor_id,
            capability_id: draft.capability_id,
            plugin_id: draft.plugin_id,
            schema_version: draft.schema_version,
            operation_type: draft.operation_type,
            target: draft.target,
            tags_touched: draft.tags_touched,
            decision,
            input_patch_hash: draft.input_patch_hash,
            result_effective_hash: draft.result_effective_hash,
        };
        events.push(event.clone());
        event
    }

    /// Events with `from <= id <= to`, ascending. A bound of 0 means
    /// unbounded on that side (0/0 returns everything).
    pub fn range(&self, from: u64, to: u64) -> Vec<JournalEvent> {
        let events = self.inner.lock().expect("journal lock poisoned");
        let tip = events.len() as u64;
        let lo = if from == 0 { 1 } else { from };
        let hi = if to == 0 || to > tip { tip } else { to };
        if tip == 0 || lo > hi {
            return Vec::new();
        }
        events
            .iter()
            .filter(|event| event.event_id >= lo && event.event_id <= hi)
            .cloned()
            .collect()
    }

    /// Recompute hashes and linkage over `from..=to` (0 = unbounded).
    pub fn verify(&self, from: u64, to: u64) -> ChainVerification {
        let events = self.inner.lock().expect("journal lock poisoned");
        let tip = events.len() as u64;
        let lo = if from == 0 { 1 } else { from };
        let hi = if to == 0 || to > tip { tip } else { to };
        if tip == 0 || lo > hi {
            return ChainVerification {
                valid: true,
                events_verified: 0,
                errors: Vec::new(),
            };
        }
        let mut errors = Vec::new();
        let mut verified = 0u64;
        for event in events.iter().filter(|e| e.event_id >= lo && e.event_id <= hi) {
            let draft = JournalDraft {
                actor_id: event.actor_id.clone(),
                capability_id: event.capability_id.clone(),
                plugin_id: event.plugin_id.clone(),
                schema_version: event.schema_version.clone(),
                operation_type: event.operation_type.clone(),
                target: event.target.clone(),
                tags_touched: event.tags_touched.clone(),
                input_patch_hash: event.input_patch_hash.clone(),
                result_effective_hash: event.result_effective_hash.clone(),
            };
            let recomputed = hash_event(
                event.event_id,
                &event.prev_hash,
                &event.timestamp,
                &draft,
                event.decision,
            );
            if recomputed != event.event_hash {
                errors.push(format!(
                    "event {}: hash mismatch (stored {}, recomputed {})",
                    event.event_id, event.event_hash, recomputed
                ));
                continue;
            }
            let expected_prev = if event.event_id == 1 {
                String::new()
            } else {
                events
                    .get(event.event_id as usize - 2)
                    .map(|prev| prev.event_hash.clone())
                    .unwrap_or_default()
            };
            if event.prev_hash != expected_prev {
                errors.push(format!(
                    "event {}: prev_hash linkage broken",
                    event.event_id
                ));
                continue;
            }
            verified += 1;
        }
        ChainVerification {
            valid: errors.is_empty(),
            events_verified: verified,
            errors,
        }
    }
}

// ---------------------------------------------------------------------------
// BridgeCore
// ---------------------------------------------------------------------------

/// Mutation operation at the core level (proto already translated).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreOperation {
    SetProperty,
    CallMethod,
    ApplyPatch,
    /// The wire value was not a known operation; rejected at admission.
    Unknown,
}

impl CoreOperation {
    fn as_str(self) -> &'static str {
        match self {
            CoreOperation::SetProperty => "SET_PROPERTY",
            CoreOperation::CallMethod => "CALL_METHOD",
            CoreOperation::ApplyPatch => "APPLY_PATCH",
            CoreOperation::Unknown => "UNKNOWN",
        }
    }
}

/// One core-level mutation. Rejections journal nothing.
#[derive(Clone, Debug)]
pub struct CoreMutation {
    pub plugin_id: String,
    pub object_path: String,
    pub operation: CoreOperation,
    pub member_name: String,
    pub value: serde_json::Value,
    pub actor_id: String,
    pub capability_id: String,
    /// Carried for audit; the fresh core does not deduplicate on it.
    pub idempotency_key: String,
}

/// Why a mutation was rejected (nothing was journaled or published).
#[derive(Clone, Debug)]
pub struct CoreRejection {
    pub code: ErrorCode,
    pub message: String,
}

fn reject(code: ErrorCode, message: impl Into<String>) -> CoreRejection {
    CoreRejection {
        code,
        message: message.into(),
    }
}

/// The journaled outcome of one accepted mutation.
#[derive(Clone, Debug)]
pub struct MutationOutcome {
    pub event: JournalEvent,
    pub change: StateChange,
    /// Handler result, for `CALL_METHOD` only.
    pub result: Option<serde_json::Value>,
}

/// Selected only by the admitted runtime, never from a caller's JSON body.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StateScope {
    Public,
    Private { principal_id: String, session_id: String },
}

/// An authoritative backend observation at the mutation's canonical object path.
#[derive(Debug)]
pub enum BackendStateUpdate {
    Unchanged,
    /// Receipt from the dedicated paired identity owner; never a raw-state write.
    IdentityPublished(op_core::projection_shm::ProjectionCommit),
    Replace(serde_json::Value),
    Merge(serde_json::Value),
    /// Replace supplied fields wholesale, preserving unrelated target fields.
    Fields(serde_json::Value),
    Delete,
}

#[derive(Debug)]
pub struct BackendCommit {
    pub mutation: CoreMutation,
    pub scope: StateScope,
    pub schema_hash: String,
    pub state: BackendStateUpdate,
    pub result: Option<serde_json::Value>,
}

/// An additional real backend observation coupled to the method outcome.
#[derive(Debug)]
pub struct BackendProjection {
    pub plugin_id: String,
    pub object_path: String,
    pub schema_hash: String,
    pub scope: StateScope,
    pub state: BackendStateUpdate,
}

#[derive(Debug)]
pub struct CommittedStateSnapshot {
    pub state: serde_json::Value,
    pub effective_hash: String,
    pub event_id: u64,
    pub generation: u64,
    pub schema_hash: String,
}

/// A wakeup only. Consumers reread SHM using `committed_frame`; this carries no state.
#[derive(Clone, Debug)]
pub struct StateNotice {
    pub scope: StateScope,
    pub plugin_id: String,
    pub event_id: u64,
    /// False for durable audit-only wakes (attempts and non-state terminals).
    pub state_committed: bool,
}

/// Outcome of [`BridgeCore::batch_mutate`].
#[derive(Clone, Debug)]
pub struct BatchOutcome {
    /// One entry per input op, in order. Ops never attempted (after an
    /// atomic abort) carry a rejection explaining that.
    pub results: Vec<Result<MutationOutcome, CoreRejection>>,
    /// Index of the first failed op, if any.
    pub failed_index: Option<usize>,
}

struct FixtureCore {
    store: StateStore,
    journal: Journal,
    router: Router,
    state_tx: broadcast::Sender<StateChange>,
    event_tx: broadcast::Sender<ChainEvent>,
}

struct BridgeCoreInner {
    /// Serializes state/journal publication and subscription snapshot boundaries.
    commit: Mutex<()>,
    catalog: Catalog,
    fixture: Option<FixtureCore>,
    persistent: Option<Mutex<persistence::PersistentCore>>,
    notice_tx: Option<broadcast::Sender<StateNotice>>,
}

/// The bridge: catalog truth, state store, journal, router and fan-out.
/// Cheap to clone; all state is shared.
#[derive(Clone)]
pub struct BridgeCore {
    inner: Arc<BridgeCoreInner>,
}

impl BridgeCore {
    /// Isolated in-memory contract fixture. Production must use [`Self::open`].
    pub fn new(catalog: Catalog) -> Self {
        // Depth 1024: slow subscribers lag (and skip) rather than block writers.
        let (state_tx, _) = broadcast::channel(1024);
        let (event_tx, _) = broadcast::channel(1024);
        Self {
            inner: Arc::new(BridgeCoreInner {
                commit: Mutex::new(()),
                catalog,
                fixture: Some(FixtureCore {
                    store: StateStore::new(),
                    journal: Journal::new(),
                    router: Router::new(),
                    state_tx,
                    event_tx,
                }),
                persistent: None,
                notice_tx: None,
            }),
        }
    }

    fn fixture(&self) -> &FixtureCore {
        self.inner.fixture.as_ref().expect("fixture-only API; production uses admitted durable state APIs")
    }

    fn notices(&self) -> &broadcast::Sender<StateNotice> {
        self.inner.notice_tx.as_ref().expect("durable production notifier required")
    }

    /// The route source of truth.
    pub fn catalog(&self) -> &Catalog {
        &self.inner.catalog
    }

    /// The isolated fixture's object state store.
    pub fn store(&self) -> &StateStore {
        &self.fixture().store
    }

    /// The isolated fixture's BLAKE3 audit journal.
    pub fn journal(&self) -> &Journal {
        &self.fixture().journal
    }

    /// The isolated fixture's method router.
    pub fn router(&self) -> &Router {
        &self.fixture().router
    }

    /// Register (or replace) an isolated fixture method handler.
    pub fn register_handler(
        &self,
        plugin_id: impl Into<String>,
        method: impl Into<String>,
        handler: Handler,
    ) {
        self.router().register(plugin_id, method, handler);
    }

    /// Fixture event-buffer subscription. Production uses `subscribe_commits`.
    pub fn subscribe_state(&self) -> broadcast::Receiver<StateChange> {
        assert!(self.inner.persistent.is_none(), "production subscriptions must reread committed SHM");
        self.fixture().state_tx.subscribe()
    }

    /// Fixture audit-buffer subscription. Production uses scoped `journal_events`.
    pub fn subscribe_events(&self) -> broadcast::Receiver<ChainEvent> {
        assert!(self.inner.persistent.is_none(), "production audit subscriptions require scoped durable reads");
        self.fixture().event_tx.subscribe()
    }

    /// Latest journaled event id, or 0 when the journal is empty.
    pub fn at_event_id(&self) -> u64 {
        if let Some(persistent) = &self.inner.persistent {
            return persistent.lock().expect("persistent core lock poisoned").at_event_id();
        }
        self.journal().tip().map(|t| t.event_id).unwrap_or(0)
    }

    /// Whole-object read plus its effective hash.
    pub fn get_object(
        &self,
        plugin_id: &str,
        object_path: &str,
    ) -> Option<(serde_json::Map<String, serde_json::Value>, String)> {
        assert!(self.inner.persistent.is_none(), "production reads use fallible committed_snapshot with admitted scope");
        let _commit = self.inner.commit.lock().expect("commit lock poisoned");
        self.store().get_object(plugin_id, object_path)
    }

    /// Read state, hash and resume cursor from the same committed snapshot.
    pub fn get_object_snapshot(
        &self,
        plugin_id: &str,
        object_path: &str,
    ) -> Option<(serde_json::Map<String, serde_json::Value>, String, u64)> {
        assert!(self.inner.persistent.is_none(), "production reads use fallible committed_snapshot with admitted scope");
        let _commit = self.inner.commit.lock().expect("commit lock poisoned");
        let (state, hash) = self.store().get_object(plugin_id, object_path)?;
        let event_id = self.journal().tip().map(|event| event.event_id).unwrap_or(0);
        Some((state, hash, event_id))
    }

    /// Read a property after any in-progress atomic state batch commits.
    pub fn get_property(
        &self,
        plugin_id: &str,
        object_path: &str,
        member: &str,
    ) -> Option<serde_json::Value> {
        assert!(self.inner.persistent.is_none(), "production property reads require admitted committed_snapshot");
        let _commit = self.inner.commit.lock().expect("commit lock poisoned");
        self.store().get_member(plugin_id, object_path, member)
    }

    /// Capture hydration and its live receiver at one commit boundary.
    pub(crate) fn state_subscription(
        &self,
        include_state: bool,
        include_schema: bool,
    ) -> (
        Vec<StateChange>,
        Vec<StateChange>,
        broadcast::Receiver<StateChange>,
    ) {
        assert!(self.inner.persistent.is_none(), "production subscriptions use fallible committed_subscription");
        let _commit = self.inner.commit.lock().expect("commit lock poisoned");
        let live = self.subscribe_state();
        let initial = if include_state {
            self.initial_frames_locked()
        } else {
            Vec::new()
        };
        let schemas = if include_schema {
            self.schema_frames_locked()
        } else {
            Vec::new()
        };
        (initial, schemas, live)
    }

    /// History ends before any event that can arrive on the returned receiver.
    pub(crate) fn event_subscription(
        &self,
        from: u64,
    ) -> (Vec<JournalEvent>, broadcast::Receiver<ChainEvent>) {
        assert!(self.inner.persistent.is_none(), "production audit reads require scoped durable journal_events");
        let _commit = self.inner.commit.lock().expect("commit lock poisoned");
        let live = self.subscribe_events();
        let history = if from == 0 {
            Vec::new()
        } else {
            self.journal().range(from, 0)
        };
        (history, live)
    }

    /// Side-effect-free admission. Every check [`mutate`](Self::mutate) can
    /// fail without running a handler lives here, so atomic batches can
    /// validate the whole batch before applying anything.
    fn validate(&self, op: &CoreMutation) -> Result<(), CoreRejection> {
        let plugin = self.inner.catalog.plugin(&op.plugin_id).ok_or_else(|| {
            reject(
                ErrorCode::NotFound,
                format!("unknown plugin '{}'", op.plugin_id),
            )
        })?;
        match op.operation {
            CoreOperation::Unknown => Err(reject(
                ErrorCode::ValidationFailed,
                "unknown operation type",
            )),
            CoreOperation::SetProperty => {
                if op.member_name.is_empty() {
                    return Err(reject(
                        ErrorCode::ValidationFailed,
                        "member_name is required for SET_PROPERTY",
                    ));
                }
                Ok(())
            }
            CoreOperation::ApplyPatch => {
                if !op.value.is_object() {
                    return Err(reject(
                        ErrorCode::ValidationFailed,
                        "APPLY_PATCH value must be a JSON object",
                    ));
                }
                Ok(())
            }
            CoreOperation::CallMethod => {
                if op.member_name.is_empty() {
                    return Err(reject(
                        ErrorCode::ValidationFailed,
                        "member_name is required for CALL_METHOD",
                    ));
                }
                if !plugin.methods.is_empty()
                    && !self
                        .inner
                        .catalog
                        .declares(&op.plugin_id, &op.member_name)
                {
                    return Err(reject(
                        ErrorCode::ValidationFailed,
                        format!(
                            "method '{}' is not declared by plugin '{}'",
                            op.member_name, op.plugin_id
                        ),
                    ));
                }
                if !self.router().has(&op.plugin_id, &op.member_name) {
                    return Err(reject(
                        ErrorCode::NotFound,
                        format!(
                            "no handler registered for '{} {}'",
                            op.plugin_id, op.member_name
                        ),
                    ));
                }
                Ok(())
            }
        }
    }

    /// Validate, apply, journal and publish one mutation.
    pub fn mutate(&self, op: CoreMutation) -> Result<MutationOutcome, CoreRejection> {
        if self.inner.persistent.is_some() {
            return Err(reject(ErrorCode::PermissionDenied,
                "production writes require admitted backend execution and authoritative state readback"));
        }
        self.validate(&op)?;

        // Method calls dispatch before the store write so a handler failure
        // rejects the mutation without journaling anything. Run user handlers
        // outside the commit lock so a handler may itself call the core.
        let (change_type, result) = match op.operation {
            CoreOperation::CallMethod => {
                let args = match &op.value {
                    serde_json::Value::Array(items) => items.clone(),
                    serde_json::Value::Null => Vec::new(),
                    single => vec![single.clone()],
                };
                let value = self.router()
                    .call(&op.plugin_id, &op.member_name, &args)
                    .map_err(|err| match err {
                        RouterError::UnknownPlugin(_) | RouterError::UnknownMethod { .. } => {
                            reject(ErrorCode::NotFound, err.to_string())
                        }
                        RouterError::HandlerFailed { .. } => {
                            reject(ErrorCode::Internal, err.to_string())
                        }
                    })?;
                (ChangeType::MethodCall, Some(value))
            }
            CoreOperation::SetProperty | CoreOperation::ApplyPatch => {
                (ChangeType::PropertySet, None)
            }
            CoreOperation::Unknown => {
                return Err(reject(ErrorCode::ValidationFailed, "unknown operation type"));
            }
        };

        let _commit = self.inner.commit.lock().expect("commit lock poisoned");
        Ok(self.commit_mutation(op, change_type, result))
    }

    /// Commit an already validated operation while holding `inner.commit`.
    /// State-only operations cannot fail after validation; handlers have
    /// already run (and are forbidden inside atomic batches).
    fn commit_mutation(
        &self,
        op: CoreMutation,
        change_type: ChangeType,
        result: Option<serde_json::Value>,
    ) -> MutationOutcome {
        let plugin = self
            .inner
            .catalog
            .plugin(&op.plugin_id)
            .expect("validated plugin exists");

        let (old_value, effective_hash) = match op.operation {
            CoreOperation::SetProperty => {
                let (old, hash) = self.store().set_member(
                    &op.plugin_id,
                    &op.object_path,
                    &op.member_name,
                    op.value.clone(),
                );
                (old, hash)
            }
            CoreOperation::ApplyPatch => {
                let hash = self.store().apply_patch(
                    &op.plugin_id,
                    &op.object_path,
                    op.value.as_object().expect("validated patch is an object"),
                );
                (None, hash)
            }
            CoreOperation::CallMethod => {
                // Calls don't write state; stamp the current object hash.
                let hash = self.store().effective_hash(&op.plugin_id, &op.object_path)
                    .unwrap_or_else(|| blake3::hash(b"{}").to_hex().to_string());
                (None, hash)
            }
            CoreOperation::Unknown => {
                unreachable!("only validated operations can commit");
            }
        };

        let input_patch_hash = blake3::hash(&canonical_json(&op.value)).to_hex().to_string();
        let event = self.journal().append(JournalDraft {
            actor_id: op.actor_id.clone(),
            capability_id: op.capability_id.clone(),
            plugin_id: op.plugin_id.clone(),
            schema_version: plugin.version.clone(),
            operation_type: op.operation.as_str().to_string(),
            target: format!("{}:{}", op.object_path, op.member_name),
            tags_touched: plugin.tags.clone(),
            input_patch_hash,
            result_effective_hash: effective_hash,
        });

        let change = StateChange {
            change_id: uuid::Uuid::new_v4().to_string(),
            event_id: event.event_id,
            plugin_id: op.plugin_id.clone(),
            object_path: op.object_path.clone(),
            change_type: change_type as i32,
            member_name: op.member_name.clone(),
            old_value: old_value.as_ref().map(json_to_proto_value),
            new_value: Some(json_to_proto_value(&op.value)),
            tags_touched: event.tags_touched.clone(),
            event_hash: event.event_hash.clone(),
            timestamp: Some(event.timestamp.clone()),
            actor_id: op.actor_id.clone(),
            frame_kind: StateFrameKind::Update as i32,
            schema_hash: plugin.schema_hash.clone(),
            catalog_hash: self.inner.catalog.hash().to_string(),
            state_generation: self.store().generation(),
        };
        let _ = self.fixture().state_tx.send(change.clone());
        let _ = self.fixture().event_tx.send(ChainEvent::from(&event));
        MutationOutcome {
            event,
            change,
            result,
        }
    }

    /// Atomic batches support state writes only: validate all before committing
    /// any. Arbitrary method handlers have no rollback contract, so an atomic
    /// batch containing one is rejected before any handler or write runs.
    /// Non-atomic batches attempt every operation independently.
    pub fn batch_mutate(&self, ops: Vec<CoreMutation>, atomic: bool) -> BatchOutcome {
        if self.inner.persistent.is_some() {
            let failed_index = if ops.is_empty() { None } else { Some(0) };
            return BatchOutcome {
                results: ops.into_iter().map(|_| Err(reject(ErrorCode::PermissionDenied,
                    "production batches require admitted transactional backend execution"))).collect(),
                failed_index,
            };
        }
        if atomic {
            if let Some((failed, rejection)) = ops
                .iter()
                .map(|op| {
                    self.validate(op)?;
                    if op.operation == CoreOperation::CallMethod {
                        return Err(reject(
                            ErrorCode::ValidationFailed,
                            "CALL_METHOD is not supported in atomic batches: handlers are not transactional",
                        ));
                    }
                    Ok(())
                })
                .enumerate()
                .filter_map(|(index, check)| check.err().map(|rej| (index, rej)))
                .next()
            {
                let results = ops
                    .iter()
                    .enumerate()
                    .map(|(index, _)| {
                        if index == failed {
                            Err(rejection.clone())
                        } else {
                            Err(reject(
                                ErrorCode::ValidationFailed,
                                format!("not attempted: atomic batch aborted at op {failed}"),
                            ))
                        }
                    })
                    .collect();
                return BatchOutcome {
                    results,
                    failed_index: Some(failed),
                };
            }
            let _commit = self.inner.commit.lock().expect("commit lock poisoned");
            return BatchOutcome {
                results: ops
                    .into_iter()
                    .map(|op| Ok(self.commit_mutation(op, ChangeType::PropertySet, None)))
                    .collect(),
                failed_index: None,
            };
        }
        let mut results = Vec::with_capacity(ops.len());
        let mut failed_index = None;
        for (index, op) in ops.into_iter().enumerate() {
            match self.mutate(op) {
                Ok(outcome) => results.push(Ok(outcome)),
                Err(rejection) => {
                    if failed_index.is_none() {
                        failed_index = Some(index);
                    }
                    results.push(Err(rejection));
                }
            }
        }
        BatchOutcome {
            results,
            failed_index,
        }
    }

    /// One `INITIAL_STATE` frame per stored object, for subscribers that
    /// asked for the present state. Not journaled (`event_id` 0).
    pub fn initial_frames(&self) -> Vec<StateChange> {
        assert!(self.inner.persistent.is_none(), "production hydration uses fallible committed_subscription");
        let _commit = self.inner.commit.lock().expect("commit lock poisoned");
        self.initial_frames_locked()
    }

    fn initial_frames_locked(&self) -> Vec<StateChange> {
        let catalog_hash = self.inner.catalog.hash().to_string();
        let generation = self.store().generation();
        self.store().snapshot()
            .into_iter()
            .map(|(plugin_id, object_path, map)| {
                let plugin = self.inner.catalog.plugin(&plugin_id);
                StateChange {
                    change_id: uuid::Uuid::new_v4().to_string(),
                    event_id: 0,
                    plugin_id,
                    object_path,
                    change_type: ChangeType::Unspecified as i32,
                    member_name: String::new(),
                    old_value: None,
                    new_value: Some(json_to_proto_value(&serde_json::Value::Object(map))),
                    tags_touched: plugin.map(|p| p.tags.clone()).unwrap_or_default(),
                    event_hash: String::new(),
                    timestamp: Some(now_timestamp()),
                    actor_id: String::new(),
                    frame_kind: StateFrameKind::InitialState as i32,
                    schema_hash: plugin.map(|p| p.schema_hash.clone()).unwrap_or_default(),
                    catalog_hash: catalog_hash.clone(),
                    state_generation: generation,
                }
            })
            .collect()
    }

    /// One fixture contract frame per catalog plugin, including fixture schema
    /// summaries. Production schema services use the exact pinned catalog.
    pub fn schema_frames(&self) -> Vec<StateChange> {
        let _commit = self.inner.commit.lock().expect("commit lock poisoned");
        self.schema_frames_locked()
    }

    fn schema_frames_locked(&self) -> Vec<StateChange> {
        let catalog_hash = self.inner.catalog.hash().to_string();
        let generation = self.store().generation();
        self.inner
            .catalog
            .plugins()
            .iter()
            .map(|plugin| {
                let schema = plugin.schema.clone().unwrap_or_else(|| {
                    serde_json::json!({
                        "id": plugin.id,
                        "name": plugin.name,
                        "version": plugin.version,
                        "schema_hash": plugin.schema_hash,
                        "methods": plugin.methods.iter().map(|m| &m.name).collect::<Vec<_>>(),
                    })
                });
                StateChange {
                    change_id: uuid::Uuid::new_v4().to_string(),
                    event_id: 0,
                    plugin_id: plugin.id.clone(),
                    object_path: String::new(),
                    change_type: ChangeType::SchemaMigration as i32,
                    member_name: plugin.schema_hash.clone(),
                    old_value: None,
                    new_value: Some(json_to_proto_value(&schema)),
                    tags_touched: plugin.tags.clone(),
                    event_hash: String::new(),
                    timestamp: Some(now_timestamp()),
                    actor_id: String::new(),
                    frame_kind: StateFrameKind::InitialState as i32,
                    schema_hash: plugin.schema_hash.clone(),
                    catalog_hash: catalog_hash.clone(),
                    state_generation: generation,
                }
            })
            .collect()
    }
}
