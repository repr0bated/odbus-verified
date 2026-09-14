//! Event Chain - Snowball-style Compliance and Reproducibility Layer
//!
//! Provides tamper-evident audit trail through:
//! - Hash-linked events for every state transition
//! - Merkle tree batching for scale
//! - Schema-aware canonical hashing
//! - Tag-scoped proofs for compliance
//! - Reproducible replay from footprints
//!
//! Each event record contains:
//! - `event_id` (monotonic or UUID)
//! - `prev_hash` (hash of previous event)
//! - `event_hash` = H(prev_hash || canonical_event_payload)
//! - `timestamp`
//! - `actor_id` + `capability_id`
//! - `plugin_id` + `schema_version`
//! - `op` (operation type)
//! - `target` (object path / selector)
//! - `tags_touched` (computed from schema)
//! - `decision` (allow/deny) + `deny_reason`
//! - `input_payload` (bounded, redacted canonical sled payload)
//! - `result_effective_hash` (post-compile)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;
use std::collections::HashMap;

use crate::schema_validator::canonicalize_json;

/// Operation types for state transitions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum OperationType {
    /// Apply an immutable wrapper to a plugin state
    ApplyImmutableWrapper,
    /// Apply a tunable patch
    ApplyTunablePatch,
    /// Schema migration
    Migrate,
    /// Reconcile state with reality
    Reconcile,
    /// Emit a D-Bus signal
    EmitSignal,
    /// Property read
    PropertyGet,
    /// Property write
    PropertySet,
    /// Method invocation
    MethodCall,
    /// Snapshot creation
    CreateSnapshot,
    /// State import
    Import,
    /// State export
    Export,
    /// Custom operation
    Custom(String),
}

/// Decision result for an operation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    Deny,
}

/// How an action came to be initiated — the autonomy provenance dimension.
///
/// Every model action must declare its origin so auditors can distinguish
/// human-instructed execution from autonomous model decisions. This is
/// what makes the trust boundary enforceable and verifiable, not just policy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionOrigin {
    /// A human, user, or parent agent explicitly requested this action.
    Instructed {
        /// Identity of the instructing party (user ID, agent ID, etc.)
        by: String,
        /// Session or conversation context reference.
        session_id: Option<String>,
        /// Hash of the prompt/instruction that triggered this, for traceability.
        prompt_ref: Option<String>,
    },
    /// The model reasoned and decided to act without explicit instruction.
    /// Autonomous actions are subject to stricter policy enforcement.
    Autonomous {
        /// Reference to the vector ID in Qdrant capturing the semantic
        /// context that drove the decision ("why it acted alone").
        reasoning_ref: Option<String>,
        /// Model's self-reported confidence in the decision (0.0–1.0).
        confidence: Option<f32>,
    },
    /// A system event triggered the action (no human or model decision involved).
    Reactive {
        /// Description of the trigger: D-Bus signal path, cron expression,
        /// state change event ID, etc.
        trigger: String,
    },
}

/// Reason for denial
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DenyReason {
    /// Tag was locked by immutable wrapper
    TagLock { tag: String, wrapper_id: String },
    /// Constraint validation failed
    ConstraintFail { constraint: String, message: String },
    /// Missing required capability
    CapabilityMissing { capability: String },
    /// Schema validation failed
    SchemaValidation { errors: Vec<String> },
    /// Read-only field modification attempted
    ReadOnlyViolation { field: String },
    /// Custom denial reason
    Custom { reason: String },
}

/// A single event in the chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainEvent {
    /// Monotonic event ID
    pub event_id: u64,
    /// Hash of the previous event
    pub prev_hash: String,
    /// Hash of this event: H(prev_hash || canonical_payload)
    pub event_hash: String,
    /// Event timestamp
    pub timestamp: DateTime<Utc>,
    /// Actor who initiated the operation
    pub actor_id: String,
    /// Authenticated session correlated with this action. The stable actor is
    /// still `actor_id`; this field is audit metadata, never authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Arrival proof for the authenticated session. This is metadata only and
    /// is never used as an identity, grant key, or payload digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_genesis: Option<String>,
    /// Capability used for the operation
    pub capability_id: Option<String>,
    /// Plugin that owns the state
    pub plugin_id: String,
    /// Schema version at time of event
    pub schema_version: String,
    /// Type of operation
    pub op: OperationType,
    /// Target object path or selector
    pub target: String,
    /// Tags touched by this operation (computed from schema)
    pub tags_touched: Vec<String>,
    /// Decision: allow or deny
    pub decision: Decision,
    /// Reason for denial (if denied)
    pub deny_reason: Option<DenyReason>,
    /// Compatibility digest of `input_payload`. New event hashes are computed
    /// from the canonical payload itself, never from this digest.
    pub input_patch_hash: String,
    /// Bounded, redacted, canonical input carried into Snowball for hashing,
    /// replay, and asynchronous vectorization.
    #[serde(default)]
    pub input_payload: Option<Value>,
    /// Read-only compatibility for persisted pre-payload method events. New
    /// events never populate or serialize this field.
    #[serde(
        default,
        rename = "json_args_footprint",
        skip_serializing_if = "Option::is_none"
    )]
    legacy_args_digest: Option<String>,
    /// The method name invoked by the caller for a method-dispatch event.
    /// `None` for non-method events (property sets, signals, etc.).
    #[serde(default)]
    pub method_name: Option<String>,
    /// Hash of the resulting effective state (if allowed)
    pub result_effective_hash: Option<String>,
    /// Optional delta hash for incremental verification
    pub db_delta_hash: Option<String>,
    /// Reference to a snapshot (if this event creates one)
    pub snapshot_ref: Option<String>,
    /// Autonomy provenance: was this instructed, autonomous, or reactive?
    /// None = legacy event predating this field; treat as unknown.
    pub action_origin: Option<ActionOrigin>,
    /// The user who initiated the conversation that led to this event.
    /// None for purely system/reactive events with no human context.
    pub user_id: Option<String>,
    /// The conversation (chat session) this event belongs to.
    /// Groups the full why→what→who chain for a single session.
    /// Indexed for efficient per-conversation audit queries.
    pub conversation_id: Option<String>,
}

impl ChainEvent {
    /// Create a new event with computed hash
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        event_id: u64,
        prev_hash: String,
        actor_id: String,
        plugin_id: String,
        schema_version: String,
        op: OperationType,
        target: String,
        tags_touched: Vec<String>,
        decision: Decision,
        input_patch: &Value,
    ) -> Self {
        let timestamp = Utc::now();
        let input_patch_hash = compute_hash(&canonicalize_json(input_patch));

        let mut event = Self {
            event_id,
            prev_hash: prev_hash.clone(),
            event_hash: String::new(), // Computed below
            timestamp,
            actor_id,
            session_id: None,
            session_genesis: None,
            capability_id: None,
            plugin_id,
            schema_version,
            op,
            target,
            tags_touched,
            decision,
            deny_reason: None,
            input_patch_hash,
            input_payload: Some(canonicalize_json(input_patch)),
            legacy_args_digest: None,
            method_name: None,
            result_effective_hash: None,
            db_delta_hash: None,
            snapshot_ref: None,
            action_origin: None,
            user_id: None,
            conversation_id: None,
        };

        // Compute event hash
        event.event_hash = event.compute_hash();
        event
    }

    /// Compute the hash of this event.
    ///
    /// Public so callers that enrich an already-appended event with additional
    /// fields (e.g. `capability_id`) can recompute the hash after mutation.
    pub fn compute_hash(&self) -> String {
        let canonical = if let Some(input_payload) = self.input_payload.as_ref() {
            simd_json::serde::to_owned_value(&CanonicalEventPayload {
                event_id: self.event_id,
                prev_hash: &self.prev_hash,
                timestamp: self.timestamp,
                actor_id: &self.actor_id,
                session_id: self.session_id.as_deref(),
                session_genesis: self.session_genesis.as_deref(),
                capability_id: self.capability_id.as_deref(),
                plugin_id: &self.plugin_id,
                schema_version: &self.schema_version,
                op: &self.op,
                target: &self.target,
                tags_touched: &self.tags_touched,
                decision: &self.decision,
                deny_reason: self.deny_reason.as_ref(),
                input_payload,
                method_name: self.method_name.as_deref(),
                result_effective_hash: self.result_effective_hash.as_deref(),
            })
            .unwrap_or_default()
        } else {
            // Preserve verification of already-persisted records while keeping
            // all newly-created events on the direct-payload path.
            simd_json::serde::to_owned_value(&LegacyCanonicalEventPayload {
                event_id: self.event_id,
                prev_hash: &self.prev_hash,
                timestamp: self.timestamp,
                actor_id: &self.actor_id,
                capability_id: self.capability_id.as_deref(),
                plugin_id: &self.plugin_id,
                schema_version: &self.schema_version,
                op: &self.op,
                target: &self.target,
                tags_touched: &self.tags_touched,
                decision: &self.decision,
                deny_reason: self.deny_reason.as_ref(),
                input_patch_hash: &self.input_patch_hash,
                json_args_footprint: self.legacy_args_digest.as_deref(),
                method_name: self.method_name.as_deref(),
                result_effective_hash: self.result_effective_hash.as_deref(),
            })
            .unwrap_or_default()
        };
        let canonical = canonicalize_json(&canonical);
        compute_hash(&canonical)
    }

    /// Set the result effective hash after successful operation
    pub fn with_result_hash(mut self, hash: String) -> Self {
        self.result_effective_hash = Some(hash);
        self.event_hash = self.compute_hash();
        self
    }

    /// Set deny reason
    pub fn with_deny_reason(mut self, reason: DenyReason) -> Self {
        self.deny_reason = Some(reason);
        self.event_hash = self.compute_hash();
        self
    }

    /// Set capability ID
    pub fn with_capability(mut self, capability: String) -> Self {
        self.capability_id = Some(capability);
        self.event_hash = self.compute_hash();
        self
    }

    /// Verify this event's hash against its content
    pub fn verify(&self) -> bool {
        if let Some(payload) = self.input_payload.as_ref() {
            if compute_hash(&canonicalize_json(payload)) != self.input_patch_hash {
                return false;
            }
        }
        let computed = self.compute_hash();
        computed == self.event_hash
    }
}

/// Canonical payload structure for consistent hashing
#[derive(Serialize)]
struct CanonicalEventPayload<'a> {
    event_id: u64,
    prev_hash: &'a str,
    timestamp: DateTime<Utc>,
    actor_id: &'a str,
    session_id: Option<&'a str>,
    session_genesis: Option<&'a str>,
    capability_id: Option<&'a str>,
    plugin_id: &'a str,
    schema_version: &'a str,
    op: &'a OperationType,
    target: &'a str,
    tags_touched: &'a [String],
    decision: &'a Decision,
    deny_reason: Option<&'a DenyReason>,
    input_payload: &'a Value,
    method_name: Option<&'a str>,
    result_effective_hash: Option<&'a str>,
}

/// Canonical shape used only to verify records written before direct payloads
/// were introduced.
#[derive(Serialize)]
struct LegacyCanonicalEventPayload<'a> {
    event_id: u64,
    prev_hash: &'a str,
    timestamp: DateTime<Utc>,
    actor_id: &'a str,
    capability_id: Option<&'a str>,
    plugin_id: &'a str,
    schema_version: &'a str,
    op: &'a OperationType,
    target: &'a str,
    tags_touched: &'a [String],
    decision: &'a Decision,
    deny_reason: Option<&'a DenyReason>,
    input_patch_hash: &'a str,
    json_args_footprint: Option<&'a str>,
    method_name: Option<&'a str>,
    result_effective_hash: Option<&'a str>,
}

/// Merkle tree node for batch proofs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleNode {
    pub hash: String,
    pub left: Option<Box<MerkleNode>>,
    pub right: Option<Box<MerkleNode>>,
}

/// A batch of events with Merkle root
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventBatch {
    /// Merkle root of all event hashes in this batch
    pub batch_root: String,
    /// Range of event IDs in this batch
    pub first_event_id: u64,
    pub last_event_id: u64,
    /// Hash of the previous batch root (for chaining batches)
    pub prev_batch_root: Option<String>,
    /// Timestamp when batch was finalized
    pub timestamp: DateTime<Utc>,
    /// Number of events in this batch
    pub event_count: usize,
}

impl EventBatch {
    /// Create a new batch from a list of event hashes
    pub fn from_events(events: &[ChainEvent], prev_batch_root: Option<String>) -> Option<Self> {
        if events.is_empty() {
            return None;
        }

        let hashes: Vec<&str> = events.iter().map(|e| e.event_hash.as_str()).collect();
        let batch_root = compute_merkle_root(&hashes);

        Some(Self {
            batch_root,
            first_event_id: events.first()?.event_id,
            last_event_id: events.last()?.event_id,
            prev_batch_root,
            timestamp: Utc::now(),
            event_count: events.len(),
        })
    }

    /// Generate a Merkle proof for a specific event
    pub fn generate_proof(events: &[ChainEvent], event_id: u64) -> Option<MerkleProof> {
        let idx = events.iter().position(|e| e.event_id == event_id)?;
        let hashes: Vec<&str> = events.iter().map(|e| e.event_hash.as_str()).collect();

        let siblings = compute_merkle_proof(&hashes, idx);
        let root = compute_merkle_root(&hashes);

        Some(MerkleProof {
            event_hash: events[idx].event_hash.clone(),
            event_id,
            siblings,
            root,
        })
    }
}

/// Merkle proof for a single event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MerkleProof {
    /// Hash of the event being proved
    pub event_hash: String,
    /// Event ID
    pub event_id: u64,
    /// Sibling hashes needed to reconstruct root
    pub siblings: Vec<(String, bool)>, // (hash, is_right)
    /// Expected root hash
    pub root: String,
}

impl MerkleProof {
    /// Verify this proof
    pub fn verify(&self) -> bool {
        let mut current = self.event_hash.clone();

        for (sibling, is_right) in &self.siblings {
            current = if *is_right {
                compute_hash_pair(&current, sibling)
            } else {
                compute_hash_pair(sibling, &current)
            };
        }

        current == self.root
    }
}

/// Snapshot of plugin state for fast rebuild
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    /// Snapshot ID (hash of content)
    pub snapshot_id: String,
    /// Event ID at which this snapshot was taken
    pub at_event_id: u64,
    /// Plugin ID
    pub plugin_id: String,
    /// Schema version
    pub schema_version: String,
    /// Stub hash
    pub stub_hash: String,
    /// Immutable wrappers hash (or list of wrapper hashes)
    pub immutable_wrappers_hash: String,
    /// Tunable patch hash
    pub tunable_patch_hash: String,
    /// Effective state hash (computed from above)
    pub effective_hash: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// The actual state data
    pub state: Value,
}

impl StateSnapshot {
    /// Create a new snapshot
    pub fn new(at_event_id: u64, plugin_id: String, schema_version: String, state: Value) -> Self {
        let canonical = canonicalize_json(&state);
        let effective_hash = compute_hash(&canonical);

        // For now, stub/wrapper/tunable hashes are derived from effective
        // In a real implementation, these would be tracked separately
        let stub_hash = effective_hash.clone();
        let immutable_wrappers_hash = compute_hash(&simd_json::json!([]));
        let tunable_patch_hash = compute_hash(&simd_json::json!({}));

        let mut snapshot = Self {
            snapshot_id: String::new(),
            at_event_id,
            plugin_id,
            schema_version,
            stub_hash,
            immutable_wrappers_hash,
            tunable_patch_hash,
            effective_hash,
            timestamp: Utc::now(),
            state,
        };

        // Compute snapshot ID from all hashes
        let id_input = format!(
            "{}:{}:{}:{}:{}",
            snapshot.at_event_id,
            snapshot.stub_hash,
            snapshot.immutable_wrappers_hash,
            snapshot.tunable_patch_hash,
            snapshot.effective_hash
        );
        snapshot.snapshot_id = compute_hash_str(&id_input);
        snapshot
    }

    /// Verify snapshot integrity
    pub fn verify(&self) -> bool {
        let canonical = canonicalize_json(&self.state);
        let computed = compute_hash(&canonical);
        computed == self.effective_hash
    }
}

/// The event chain - append-only ledger
pub struct EventChain {
    /// All events in order
    events: Vec<ChainEvent>,
    /// Finalized batches
    batches: Vec<EventBatch>,
    /// Snapshots for fast rebuild
    snapshots: HashMap<String, StateSnapshot>,
    /// Configuration
    config: ChainConfig,
    /// Genesis hash (first prev_hash)
    genesis_hash: String,
}

/// Configuration for the event chain
#[derive(Debug, Clone)]
pub struct ChainConfig {
    /// Number of events per batch
    pub batch_size: usize,
    /// Whether to auto-batch when batch_size is reached
    pub auto_batch: bool,
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self {
            batch_size: 1000,
            auto_batch: true,
        }
    }
}

impl EventChain {
    /// Create a new event chain
    pub fn new(config: ChainConfig) -> Self {
        Self {
            events: Vec::new(),
            batches: Vec::new(),
            snapshots: HashMap::new(),
            config,
            genesis_hash: compute_hash_str("genesis"),
        }
    }

    /// Get the hash of the last event (or genesis)
    pub fn last_hash(&self) -> &str {
        self.events
            .last()
            .map(|e| e.event_hash.as_str())
            .unwrap_or(&self.genesis_hash)
    }

    /// Get the next event ID
    pub fn next_event_id(&self) -> u64 {
        self.events.last().map(|e| e.event_id + 1).unwrap_or(1)
    }

    /// Append a new event to the chain
    pub fn append(&mut self, mut event: ChainEvent) -> &ChainEvent {
        // Ensure prev_hash matches
        event.prev_hash = self.last_hash().to_string();
        event.event_id = self.next_event_id();
        event.event_hash = event.compute_hash();

        self.events.push(event);

        // Auto-batch if configured
        if self.config.auto_batch && self.unbatched_count() >= self.config.batch_size {
            self.create_batch();
        }

        self.events.last().expect("event just pushed")
    }

    /// Create a new event and append it
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        actor_id: String,
        plugin_id: String,
        schema_version: String,
        op: OperationType,
        target: String,
        tags_touched: Vec<String>,
        decision: Decision,
        input_patch: &Value,
    ) -> &ChainEvent {
        let event = ChainEvent::new(
            self.next_event_id(),
            self.last_hash().to_string(),
            actor_id,
            plugin_id,
            schema_version,
            op,
            target,
            tags_touched,
            decision,
            input_patch,
        );
        self.append(event)
    }

    /// Record a method-dispatch event with the full accountability surface.
    ///
    /// This is the record path for `SchemaEngine.mutate` method dispatches
    /// (Requirement 5.5 / VAL-DISPATCH-005 / VAL-NFR-003). The event carries:
    /// - `actor_id` — who initiated the call
    /// - `plugin_id` — target plugin
    /// - `method_name` — the invoked method name
    /// - `capability_id` — the capability used (or `None`)
    /// - `input_payload` — bounded, redacted canonical method arguments
    ///
    /// The append occurs before the dispatch returns success to the caller
    /// (the caller holds the `EventChain` write lock during `record`).
    pub fn record_method_call(
        &mut self,
        actor_id: String,
        plugin_id: String,
        method_name: String,
        capability_id: Option<String>,
        json_args: &str,
    ) -> &ChainEvent {
        self.record_method_call_with_identity(
            actor_id,
            None,
            None,
            plugin_id,
            method_name,
            capability_id,
            json_args,
        )
    }

    /// Record a method event with verified session correlation metadata.
    /// `actor_id` remains the stable principal used for authorization.
    #[allow(clippy::too_many_arguments)]
    pub fn record_method_call_with_identity(
        &mut self,
        actor_id: String,
        session_id: Option<String>,
        session_genesis: Option<String>,
        plugin_id: String,
        method_name: String,
        capability_id: Option<String>,
        json_args: &str,
    ) -> &ChainEvent {
        let target = format!("/org/opdbus/v1/plugins/{}", plugin_id);
        let method_args = sanitize_method_args(json_args);
        let input_patch = simd_json::json!({ "method": method_name, "args": method_args });
        let mut event = ChainEvent::new(
            self.next_event_id(),
            self.last_hash().to_string(),
            actor_id,
            plugin_id,
            "1.0.0".to_string(),
            OperationType::MethodCall,
            target,
            vec![],
            Decision::Allow,
            &input_patch,
        );
        event.session_id = session_id;
        event.session_genesis = session_genesis;
        event.capability_id = capability_id;
        event.method_name = Some(method_name);
        // Recompute hash now that capability_id and method_name are set.
        event.event_hash = event.compute_hash();
        self.append(event)
    }

    /// Replay a persisted event back into the chain during startup rebuild.
    ///
    /// Unlike [`append`], this preserves the event's stored `event_id`,
    /// `prev_hash`, and `event_hash` verbatim — the durable record is the
    /// authority, and recomputing would break the hash linkage that makes the
    /// trail tamper-evident. Events must be replayed in ascending `event_id`
    /// order; out-of-order or duplicate ids are rejected so a corrupted
    /// directory cannot silently produce a chain that fails verification.
    ///
    /// OSCAL subid: `src.service.event-chain.rebuild@v1`
    pub fn replay_event(&mut self, event: ChainEvent) -> Result<(), String> {
        if let Some(last) = self.events.last() {
            if event.event_id <= last.event_id {
                return Err(format!(
                    "replay out of order: event {} follows {}",
                    event.event_id, last.event_id
                ));
            }
        }
        self.events.push(event);
        Ok(())
    }

    /// Replay a persisted event from its JSON representation.
    ///
    /// Accepts either a bare serialized [`ChainEvent`] object or a
    /// streaming-snowball timing record that carries one under
    /// `data.metadata.audit_event`. Returns the replayed `event_id`.
    ///
    /// OSCAL subid: `src.service.event-chain.rebuild@v1`
    pub fn replay_from_footprint(&mut self, json: &serde_json::Value) -> Result<u64, String> {
        let data = json.get("data");
        let candidate = data
            .and_then(|d| d.get("payload"))
            .or_else(|| {
                data.and_then(|d| d.get("metadata"))
                    .and_then(|m| m.get("audit_event"))
            })
            .unwrap_or(json);

        let event: ChainEvent = serde_json::from_value(candidate.clone())
            .map_err(|e| format!("not a ChainEvent record: {e}"))?;
        let event_id = event.event_id;
        self.replay_event(event)?;
        Ok(event_id)
    }

    /// Verify a contiguous range of the chain.
    ///
    /// `from == 0` means "from genesis"; `to == 0` means "through the latest
    /// event". Each event in range is checked for (a) its own hash integrity
    /// and (b) correct linkage to its predecessor. Linkage for the first event
    /// in the range is checked against the real preceding event in the chain
    /// (or the genesis hash when the range starts at the head), so verifying a
    /// sub-range does not report a false break at its lower bound.
    pub fn verify_range(&self, from: u64, to: u64) -> ChainVerificationResult {
        let mut result = ChainVerificationResult {
            valid: true,
            events_verified: 0,
            batches_verified: 0,
            errors: Vec::new(),
        };

        let mut expected_prev = self.genesis_hash.clone();
        for event in &self.events {
            let in_range =
                (from == 0 || event.event_id >= from) && (to == 0 || event.event_id <= to);

            if in_range {
                if event.prev_hash != expected_prev {
                    result.valid = false;
                    result.errors.push(format!(
                        "Event {} has wrong prev_hash: expected {}, got {}",
                        event.event_id, expected_prev, event.prev_hash
                    ));
                }
                if !event.verify() {
                    result.valid = false;
                    result
                        .errors
                        .push(format!("Event {} hash verification failed", event.event_id));
                }
                result.events_verified += 1;
            }

            expected_prev = event.event_hash.clone();
        }

        result
    }

    /// Get number of unbatched events
    fn unbatched_count(&self) -> usize {
        let last_batched = self.batches.last().map(|b| b.last_event_id).unwrap_or(0);
        self.events
            .iter()
            .filter(|e| e.event_id > last_batched)
            .count()
    }

    /// Create a batch from unbatched events
    pub fn create_batch(&mut self) -> Option<&EventBatch> {
        let last_batched = self.batches.last().map(|b| b.last_event_id).unwrap_or(0);
        let unbatched: Vec<_> = self
            .events
            .iter()
            .filter(|e| e.event_id > last_batched)
            .cloned()
            .collect();

        if unbatched.is_empty() {
            return None;
        }

        let prev_root = self.batches.last().map(|b| b.batch_root.clone());
        let batch = EventBatch::from_events(&unbatched, prev_root)?;
        self.batches.push(batch);
        self.batches.last()
    }

    /// Create a snapshot at the current state
    pub fn create_snapshot(
        &mut self,
        plugin_id: String,
        schema_version: String,
        state: Value,
    ) -> &StateSnapshot {
        let event_id = self.events.last().map(|e| e.event_id).unwrap_or(0);
        let snapshot = StateSnapshot::new(event_id, plugin_id, schema_version, state);
        let id = snapshot.snapshot_id.clone();
        self.snapshots.insert(id.clone(), snapshot);
        self.snapshots.get(&id).expect("snapshot just inserted")
    }

    /// Verify the entire chain
    pub fn verify_chain(&self) -> ChainVerificationResult {
        let mut result = ChainVerificationResult {
            valid: true,
            events_verified: 0,
            batches_verified: 0,
            errors: Vec::new(),
        };

        // Verify event chain
        let mut expected_prev = self.genesis_hash.clone();
        for event in &self.events {
            if event.prev_hash != expected_prev {
                result.valid = false;
                result.errors.push(format!(
                    "Event {} has wrong prev_hash: expected {}, got {}",
                    event.event_id, expected_prev, event.prev_hash
                ));
            }

            if !event.verify() {
                result.valid = false;
                result
                    .errors
                    .push(format!("Event {} hash verification failed", event.event_id));
            }

            expected_prev = event.event_hash.clone();
            result.events_verified += 1;
        }

        // Verify batch chain
        for batch in &self.batches {
            // Get events in this batch
            let batch_events: Vec<_> = self
                .events
                .iter()
                .filter(|e| e.event_id >= batch.first_event_id && e.event_id <= batch.last_event_id)
                .collect();

            let hashes: Vec<&str> = batch_events.iter().map(|e| e.event_hash.as_str()).collect();
            let computed_root = compute_merkle_root(&hashes);

            if computed_root != batch.batch_root {
                result.valid = false;
                result.errors.push(format!(
                    "Batch {}-{} root mismatch: expected {}, computed {}",
                    batch.first_event_id, batch.last_event_id, batch.batch_root, computed_root
                ));
            }

            result.batches_verified += 1;
        }

        result
    }

    /// Query events by tag
    pub fn events_touching_tag(&self, tag: &str) -> Vec<&ChainEvent> {
        self.events
            .iter()
            .filter(|e| e.tags_touched.contains(&tag.to_string()))
            .collect()
    }

    /// Query events by plugin
    pub fn events_for_plugin(&self, plugin_id: &str) -> Vec<&ChainEvent> {
        self.events
            .iter()
            .filter(|e| e.plugin_id == plugin_id)
            .collect()
    }

    /// Prove that a tag was never touched by tunable writes
    pub fn prove_tag_immutability(&self, tag: &str) -> TagImmutabilityProof {
        let tunable_touches: Vec<_> = self
            .events
            .iter()
            .filter(|e| {
                matches!(e.op, OperationType::ApplyTunablePatch)
                    && e.tags_touched.contains(&tag.to_string())
                    && e.decision == Decision::Allow
            })
            .collect();

        TagImmutabilityProof {
            tag: tag.to_string(),
            is_immutable: tunable_touches.is_empty(),
            violations: tunable_touches.iter().map(|e| e.event_id).collect(),
            total_events_checked: self.events.len(),
        }
    }

    /// Get all events
    pub fn events(&self) -> &[ChainEvent] {
        &self.events
    }

    /// Get mutable access to all events.
    ///
    /// Used by callers that need to enrich the last-appended event with
    /// additional fields (e.g. `capability_id`) after [`record`] has
    /// appended it. The caller MUST recompute `event_hash` after mutating
    /// any field that participates in the canonical hash payload.
    pub fn events_mut(&mut self) -> &mut [ChainEvent] {
        &mut self.events
    }

    /// Get all batches
    pub fn batches(&self) -> &[EventBatch] {
        &self.batches
    }

    /// Get a snapshot by ID
    pub fn get_snapshot(&self, id: &str) -> Option<&StateSnapshot> {
        self.snapshots.get(id)
    }
}

/// Result of chain verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainVerificationResult {
    pub valid: bool,
    pub events_verified: usize,
    pub batches_verified: usize,
    pub errors: Vec<String>,
}

/// Proof that a tag was never modified by tunable writes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagImmutabilityProof {
    pub tag: String,
    pub is_immutable: bool,
    pub violations: Vec<u64>,
    pub total_events_checked: usize,
}

// =============================================================================
// Hash utilities
// =============================================================================

const MAX_METHOD_ARGS_BYTES: usize = 64 * 1024;
const MAX_METHOD_ARGS_PREVIEW_BYTES: usize = 48 * 1024;

/// Parse, recursively redact, and bound a method payload before it enters the
/// authoritative chain. The bound fallback contains redacted JSON text rather
/// than a digest, so Snowball and its vector projection still receive payload
/// content and never have to hash a hash.
fn sanitize_method_args(json_args: &str) -> Value {
    let Ok(mut parsed) = serde_json::from_str::<serde_json::Value>(json_args) else {
        return simd_json::json!({ "_invalid_json": true });
    };
    redact_sensitive_json(&mut parsed);
    let encoded = serde_json::to_vec(&parsed).unwrap_or_default();
    if encoded.len() > MAX_METHOD_ARGS_BYTES {
        let mut end = encoded.len().min(MAX_METHOD_ARGS_PREVIEW_BYTES);
        while end > 0 && std::str::from_utf8(&encoded[..end]).is_err() {
            end -= 1;
        }
        let preview = String::from_utf8_lossy(&encoded[..end]).into_owned();
        parsed = serde_json::json!({
            "_truncated": true,
            "original_redacted_bytes": encoded.len(),
            "redacted_json_preview": preview
        });
    }
    simd_json::serde::to_owned_value(&parsed).unwrap_or_default()
}

fn redact_sensitive_json(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, nested) in object {
                if sensitive_payload_key(key) {
                    *nested = serde_json::Value::String("[REDACTED]".to_string());
                } else {
                    redact_sensitive_json(nested);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for nested in values {
                redact_sensitive_json(nested);
            }
        }
        _ => {}
    }
}

fn sensitive_payload_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    matches!(
        normalized.as_str(),
        "authorization"
            | "cookie"
            | "password"
            | "passwd"
            | "secret"
            | "token"
            | "access_token"
            | "refresh_token"
            | "api_key"
            | "private_key"
            | "client_secret"
    ) || normalized.ends_with("_password")
        || normalized.ends_with("_secret")
        || normalized.ends_with("_token")
        || normalized.ends_with("_api_key")
        || normalized.ends_with("_private_key")
}

/// Compute hash of a JSON value
fn compute_hash(value: &Value) -> String {
    let canonical_str = simd_json::to_string(value).unwrap_or_default();
    format!("{:x}", md5::compute(canonical_str.as_bytes()))
}

/// Compute hash of a string
fn compute_hash_str(s: &str) -> String {
    format!("{:x}", md5::compute(s.as_bytes()))
}

/// Compute hash of two hashes concatenated
fn compute_hash_pair(left: &str, right: &str) -> String {
    compute_hash_str(&format!("{}{}", left, right))
}

/// Compute Merkle root from a list of hashes
fn compute_merkle_root(hashes: &[&str]) -> String {
    if hashes.is_empty() {
        return compute_hash_str("empty");
    }
    if hashes.len() == 1 {
        return hashes[0].to_string();
    }

    let mut level: Vec<String> = hashes.iter().map(|s| s.to_string()).collect();

    while level.len() > 1 {
        let mut next_level = Vec::new();
        for chunk in level.chunks(2) {
            if chunk.len() == 2 {
                next_level.push(compute_hash_pair(&chunk[0], &chunk[1]));
            } else {
                // Odd number: duplicate last hash
                next_level.push(compute_hash_pair(&chunk[0], &chunk[0]));
            }
        }
        level = next_level;
    }

    level.into_iter().next().unwrap_or_default()
}

/// Compute Merkle proof siblings for a specific index
fn compute_merkle_proof(hashes: &[&str], index: usize) -> Vec<(String, bool)> {
    if hashes.len() <= 1 {
        return Vec::new();
    }

    let mut siblings = Vec::new();
    let mut level: Vec<String> = hashes.iter().map(|s| s.to_string()).collect();
    let mut idx = index;

    while level.len() > 1 {
        let sibling_idx = if idx.is_multiple_of(2) {
            idx + 1
        } else {
            idx - 1
        };
        let is_right = idx.is_multiple_of(2);

        if sibling_idx < level.len() {
            siblings.push((level[sibling_idx].clone(), is_right));
        } else {
            // Odd number: duplicate
            siblings.push((level[idx].clone(), is_right));
        }

        // Build next level
        let mut next_level = Vec::new();
        for chunk in level.chunks(2) {
            if chunk.len() == 2 {
                next_level.push(compute_hash_pair(&chunk[0], &chunk[1]));
            } else {
                next_level.push(compute_hash_pair(&chunk[0], &chunk[0]));
            }
        }

        idx /= 2;
        level = next_level;
    }

    siblings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_chain_basic() {
        let mut chain = EventChain::new(ChainConfig::default());

        chain.record(
            "user1".to_string(),
            "lxc".to_string(),
            "2.0.0".to_string(),
            OperationType::ApplyTunablePatch,
            "/containers/100".to_string(),
            vec!["container".to_string()],
            Decision::Allow,
            &simd_json::json!({"memory": 1024}),
        );

        assert_eq!(chain.events().len(), 1);
        assert!(chain.verify_chain().valid);
    }

    #[test]
    fn test_event_chain_integrity() {
        let mut chain = EventChain::new(ChainConfig::default());

        for i in 0..5 {
            chain.record(
                "user1".to_string(),
                "lxc".to_string(),
                "2.0.0".to_string(),
                OperationType::PropertySet,
                format!("/containers/{}", i),
                vec!["container".to_string()],
                Decision::Allow,
                &simd_json::json!({"value": i}),
            );
        }

        let result = chain.verify_chain();
        assert!(result.valid);
        assert_eq!(result.events_verified, 5);
    }

    #[test]
    fn test_merkle_root() {
        let hashes = vec!["a", "b", "c", "d"];
        let root = compute_merkle_root(&hashes);
        assert!(!root.is_empty());

        // Same hashes should produce same root
        let root2 = compute_merkle_root(&hashes);
        assert_eq!(root, root2);
    }

    #[test]
    fn test_merkle_proof() {
        let hashes = vec!["a", "b", "c", "d"];
        let root = compute_merkle_root(&hashes);

        let proof_siblings = compute_merkle_proof(&hashes, 2);

        // Verify proof manually
        let mut current = "c".to_string();
        for (sibling, is_right) in &proof_siblings {
            current = if *is_right {
                compute_hash_pair(&current, sibling)
            } else {
                compute_hash_pair(sibling, &current)
            };
        }

        assert_eq!(current, root);
    }

    #[test]
    fn test_tag_immutability_proof() {
        let mut chain = EventChain::new(ChainConfig::default());

        // Record events with different tags
        chain.record(
            "user1".to_string(),
            "lxc".to_string(),
            "2.0.0".to_string(),
            OperationType::ApplyTunablePatch,
            "/containers/100".to_string(),
            vec!["container".to_string()],
            Decision::Allow,
            &simd_json::json!({}),
        );

        chain.record(
            "user1".to_string(),
            "lxc".to_string(),
            "2.0.0".to_string(),
            OperationType::ApplyImmutableWrapper,
            "/containers/100".to_string(),
            vec!["security".to_string()],
            Decision::Allow,
            &simd_json::json!({}),
        );

        // Security tag should be immutable (no tunable touches)
        let proof = chain.prove_tag_immutability("security");
        assert!(proof.is_immutable);

        // Container tag was touched by tunable
        let proof = chain.prove_tag_immutability("container");
        assert!(!proof.is_immutable);
    }

    #[test]
    fn test_batch_creation() {
        let config = ChainConfig {
            batch_size: 3,
            auto_batch: false,
        };
        let mut chain = EventChain::new(config);

        for i in 0..5 {
            chain.record(
                "user1".to_string(),
                "lxc".to_string(),
                "2.0.0".to_string(),
                OperationType::PropertySet,
                format!("/test/{}", i),
                vec![],
                Decision::Allow,
                &simd_json::json!({}),
            );
        }

        let batch = chain.create_batch().unwrap();
        assert_eq!(batch.event_count, 5);
        assert_eq!(batch.first_event_id, 1);
        assert_eq!(batch.last_event_id, 5);
    }

    #[test]
    fn test_verify_range_subrange_and_full() {
        let mut chain = EventChain::new(ChainConfig::default());
        for i in 0..5 {
            chain.record_method_call(
                "user1".to_string(),
                "snowball".to_string(),
                format!("m{i}"),
                Some("snowball.read".to_string()),
                "{}",
            );
        }

        // Full range (0,0) matches verify_chain's event count.
        let full = chain.verify_range(0, 0);
        assert!(full.valid, "full range must verify: {:?}", full.errors);
        assert_eq!(full.events_verified, 5);

        // A sub-range verifies without reporting a false linkage break at its
        // lower bound.
        let sub = chain.verify_range(3, 4);
        assert!(sub.valid, "subrange must verify: {:?}", sub.errors);
        assert_eq!(sub.events_verified, 2);
    }

    #[test]
    fn test_replay_from_footprint_round_trip() {
        let mut original = EventChain::new(ChainConfig::default());
        for i in 0..3 {
            original.record_method_call(
                "actor".to_string(),
                "cognitive_mcp".to_string(),
                format!("get_health{i}"),
                None,
                "{\"a\":1}",
            );
        }

        // Persist each event the way the durability path does, then rebuild.
        let mut rebuilt = EventChain::new(ChainConfig::default());
        for event in original.events() {
            let record = serde_json::json!({
                "data": { "metadata": { "audit_event": serde_json::to_value(event).unwrap() } }
            });
            let id = rebuilt.replay_from_footprint(&record).expect("replay");
            assert_eq!(id, event.event_id);
        }

        assert_eq!(rebuilt.events().len(), 3);
        // Hash linkage survives the round trip, so the rebuilt chain verifies.
        let result = rebuilt.verify_range(0, 0);
        assert!(
            result.valid,
            "rebuilt chain must verify: {:?}",
            result.errors
        );
        // The next event continues the chain rather than restarting at 1.
        assert_eq!(rebuilt.next_event_id(), 4);
        assert_eq!(rebuilt.last_hash(), original.last_hash());
    }

    #[test]
    fn test_replay_rejects_out_of_order() {
        let mut source = EventChain::new(ChainConfig::default());
        source.record_method_call("a".into(), "p".into(), "m1".into(), None, "{}");
        source.record_method_call("a".into(), "p".into(), "m2".into(), None, "{}");
        let events: Vec<ChainEvent> = source.events().to_vec();

        let mut chain = EventChain::new(ChainConfig::default());
        chain.replay_event(events[1].clone()).expect("first replay");
        // event_id 1 after event_id 2 is a corrupted ordering — must be refused.
        assert!(chain.replay_event(events[0].clone()).is_err());
    }

    #[test]
    fn method_event_carries_identity_metadata_and_hashes_direct_payload() {
        let mut chain = EventChain::new(ChainConfig::default());
        let event = chain
            .record_method_call_with_identity(
                "937d6d2b-ecae-ed53-f3a2-d7bd09f544ff".into(),
                Some("session-7".into()),
                Some("genesis-7".into()),
                "cognitive_mcp".into(),
                "memory_store".into(),
                Some("cognitive_mcp.invoke".into()),
                r#"{"text":"remember this","password":"do-not-persist"}"#,
            )
            .clone();

        assert_eq!(event.actor_id, "937d6d2b-ecae-ed53-f3a2-d7bd09f544ff");
        assert_eq!(event.session_id.as_deref(), Some("session-7"));
        assert_eq!(event.session_genesis.as_deref(), Some("genesis-7"));
        let payload = event.input_payload.as_ref().expect("direct payload");
        assert_eq!(payload["method"], "memory_store");
        assert_eq!(payload["args"]["text"], "remember this");
        assert_eq!(payload["args"]["password"], "[REDACTED]");
        assert!(event.verify());

        let mut tampered = event;
        tampered.session_genesis = Some("different-genesis".into());
        assert!(!tampered.verify(), "session metadata must be hash-bound");
    }

    #[test]
    fn test_snapshot() {
        let mut chain = EventChain::new(ChainConfig::default());

        chain.record(
            "user1".to_string(),
            "lxc".to_string(),
            "2.0.0".to_string(),
            OperationType::ApplyTunablePatch,
            "/containers".to_string(),
            vec![],
            Decision::Allow,
            &simd_json::json!({}),
        );

        let state = simd_json::json!({
            "containers": [{"id": "100", "running": true}]
        });

        let snapshot = chain.create_snapshot("lxc".to_string(), "2.0.0".to_string(), state);

        assert!(snapshot.verify());
        assert_eq!(snapshot.at_event_id, 1);
    }
}
