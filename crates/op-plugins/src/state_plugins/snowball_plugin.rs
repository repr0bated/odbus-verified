//! `snowball` StatePlugin — exposes the live streaming-snowball audit chain
//! (`op-snowball`) through the `org.opdbus.v1.plugins` surface, reachable over
//! `PluginService.CallMethod`.
//!
//! This is the correct home for the capability the Lovable frontend mistakenly
//! called as a standalone `operation.snowball.v1.SnowballService` gRPC
//! package — there is no such proto and there must not be one. New backend
//! capabilities register here as plugins; the served gRPC surface stays the one
//! shared route-builder.
//!
//! Every canonical method uses the bridge-owned shared Snowball handle. The
//! legacy `StatePlugin` view can read the configured chain, but refuses writes
//! so it cannot create a competing writer lifecycle.

use anyhow::Result;
use async_trait::async_trait;
use op_snowball::StreamingSnowball;
use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff, StatePlugin,
};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::path::PathBuf;

/// Default on-disk location of the streaming snowball when
/// `$OPDBUS_SNOWBALL_PATH` is unset.
const DEFAULT_BASE_PATH: &str = "/var/lib/opdbus/snowball";

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RetentionView {
    /// Hourly retention count.
    #[schemars(extend("x-oscal-subid" = "obs.software.snowball.retention.hourly@v1"))]
    pub hourly: usize,
    /// Daily retention count.
    #[schemars(extend("x-oscal-subid" = "obs.software.snowball.retention.daily@v1"))]
    pub daily: usize,
    /// Weekly retention count.
    #[schemars(extend("x-oscal-subid" = "obs.software.snowball.retention.weekly@v1"))]
    pub weekly: usize,
    /// Quarterly retention count.
    #[schemars(extend("x-oscal-subid" = "obs.software.snowball.retention.quarterly@v1"))]
    pub quarterly: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SnapshotEntry {
    /// Snapshot name.
    #[schemars(extend("x-oscal-subid" = "exp.software.snowball.snapshot.name@v1"))]
    pub name: String,
    /// Snapshot creation timestamp.
    #[schemars(extend("x-oscal-subid" = "exp.software.snowball.snapshot.created@v1"))]
    pub created: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnowballEmptyInput {}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotInput {
    pub snapshot_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SnapshotRecord {
    pub name: String,
    pub counter: u64,
    pub created: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreateSnapshotOutput {
    pub snapshot: SnapshotRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListSnapshotsOutput {
    pub snapshots: Vec<SnapshotRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetSnapshotOutput {
    pub snapshot: SnapshotRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetCurrentStateOutput {
    pub initialized: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SnowballStatsOutput {
    /// Timing records that parsed as complete Snowball blocks.
    pub readable_block_count: u64,
    pub vectorized_block_count: u64,
    pub snapshot_count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_block_number: Option<u64>,
    pub retention: RetentionView,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RollbackOutput {
    /// The Snowball state-directory snapshot used as the source.
    pub snapshot_name: String,
    /// Restored disaster-recovery image only. This does not apply state to any
    /// live provider; provider restoration must use that plugin's own method.
    pub restored_current_state: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetRetentionInput {
    pub hourly: u32,
    pub daily: u32,
    pub weekly: u32,
    pub quarterly: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetRetentionOutput {
    pub retention: RetentionView,
    pub remaining_snapshots: u64,
}

// ── Audit-trail query surface (accountability-audit-trail spec) ──────────────
//
// These types are declared at module scope (not inside `snowball_schema()`
// like the older output structs) because `op-grpc-bridge`'s MutationEngine
// imports them to implement the `query_events` / `verify_chain` dispatch arm.
// The same `EventChain` that the gRPC `EventChainService` serves is read here,
// so the D-Bus/MCP path and the gRPC path never disagree.

/// Input for `query_events` — paginated audit-trail query.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, Default)]
#[schemars(extend("x-oscal-subid" = "sch.service.snowball.query-events-input@v1"))]
pub struct QueryEventsInput {
    /// Filter by plugin_id. Absent or empty means all plugins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    /// Return events with `event_id >= from_event_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_event_id: Option<u64>,
    /// Return events with `event_id <= to_event_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_event_id: Option<u64>,
    /// Max events to return. Default 50, clamped to 100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Filter by decision: `allow`, `deny`, or absent for all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
}

/// One audit event, flattened for transport across the plugin surface.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.snowball.audit-event-record@v1"))]
pub struct AuditEventRecord {
    pub event_id: u64,
    pub event_hash: String,
    pub prev_hash: String,
    /// ISO 8601 / RFC 3339 timestamp.
    pub timestamp: String,
    pub actor_id: String,
    pub capability_id: String,
    pub plugin_id: String,
    pub method_name: String,
    pub operation_type: String,
    pub target: String,
    pub tags_touched: Vec<String>,
    pub decision: String,
    pub input_patch_hash: String,
    pub result_effective_hash: String,
}

/// Output for `query_events`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.snowball.query-events-output@v1"))]
pub struct QueryEventsOutput {
    /// The page of events, oldest-first within the requested range.
    pub events: Vec<AuditEventRecord>,
    /// True when more events match the filter beyond this page.
    pub has_more: bool,
    /// Total number of events currently in the chain.
    pub total_in_chain: u64,
}

/// Semantic retrieval restricted to the authenticated caller's current session.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid" = "sch.service.snowball.search-own-events-input@v1"))]
pub struct SearchOwnEventsInput {
    /// Natural-language question, bounded to 4000 UTF-8 bytes.
    pub query: String,
    /// Maximum results, between 1 and 20; defaults to 10.
    #[serde(default)]
    #[schemars(range(min = 1, max = 20))]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.snowball.evidence-match@v1"))]
pub struct EvidenceMatch {
    /// Similarity is a retrieval ranking, not proof of an explanation.
    pub score: f32,
    /// Metadata resolved from the authoritative chain, never the vector payload.
    pub event: AuditEventRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.snowball.search-own-events-output@v1"))]
pub struct SearchOwnEventsOutput {
    /// Actual Voyage embedding model used for this query.
    pub embedding_model: String,
    pub matches: Vec<EvidenceMatch>,
    /// Index hits rejected because canonical session/event provenance did not match.
    pub rejected_hits: u32,
}

pub const SEARCH_OWN_EVENTS_CAPABILITY: &str = "cap.obs.snowball.events.self.search@v1";

/// Input for `verify_chain` — hash-chain integrity check.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, Default)]
#[schemars(extend("x-oscal-subid" = "sch.service.snowball.verify-chain-input@v1"))]
pub struct VerifyChainInput {
    /// Verify from this event_id. Absent or 0 means from genesis.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_event_id: Option<u64>,
    /// Verify to this event_id. Absent or 0 means to the latest event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_event_id: Option<u64>,
}

/// Output for `verify_chain`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.snowball.verify-chain-output@v1"))]
pub struct VerifyChainOutput {
    /// True when every event in range hashes correctly and links to its predecessor.
    pub valid: bool,
    /// Number of events checked.
    pub events_verified: u64,
    /// One entry per detected integrity violation. Empty when `valid` is true.
    pub errors: Vec<String>,
}

/// Input for `get_projector_stats` — no arguments.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, Default)]
#[schemars(extend("x-oscal-subid" = "sch.service.snowball.projector-stats-input@v1"))]
pub struct GetProjectorStatsInput {}

/// Output for `get_projector_stats` — status of the OD-64 chain-vectorization
/// background loop (`ChainVectorProjector`), previously visible only as a
/// private struct inside `op-grpc-bridge`'s tokio task.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.snowball.projector-stats-output@v1"))]
pub struct ProjectorStatsOutput {
    /// Completed projection passes since the bridge started.
    pub passes: u64,
    /// Chain blocks embedded and written to the vector subvolume so far.
    pub embedded: u64,
    /// Passes that errored (index open failure or a failed embed/write).
    pub failures: u64,
    /// The most recent pass failure, if any pass has failed since start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// Snowball plugin state schema.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.snowball.schema@v1"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct SnowballState {
    /// `active` once the chain exists on disk, `uninitialized` before first write.
    #[schemars(extend("x-oscal-subid" = "obs.software.snowball.status@v1"))]
    pub status: String,
    /// Snowball base path.
    #[schemars(extend("x-oscal-subid" = "exp.software.snowball.base-path@v1"))]
    pub base_path: String,
    /// Snowball snapshot interval.
    #[schemars(extend("x-oscal-subid" = "mut.service.snowball.snapshot-interval@v1"))]
    pub snapshot_interval: String,
    /// Snowball retention policy.
    #[schemars(extend("x-oscal-subid" = "obs.service.snowball.retention@v1"))]
    pub retention: RetentionView,
    /// Number of snapshots.
    #[schemars(extend("x-oscal-subid" = "exp.software.snowball.snapshot-count@v1"))]
    pub snapshot_count: usize,
    /// List of snapshots.
    #[schemars(extend("x-oscal-subid" = "exp.software.snowball.snapshots@v1"))]
    pub snapshots: Vec<SnapshotEntry>,
    /// Current snowball state.
    #[schemars(skip)]
    #[schemars(extend("x-oscal-subid" = "obs.software.snowball.current-state@v1"))]
    pub current_state: Option<Value>,
}

pub struct SnowballPlugin {
    base_path: PathBuf,
}

impl Default for SnowballPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl SnowballPlugin {
    pub fn new() -> Self {
        let base_path = std::env::var("OPDBUS_SNOWBALL_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_BASE_PATH));
        Self { base_path }
    }

    /// Open the on-disk chain. `StreamingSnowball::new` is idempotent: it only
    /// (re)creates subvolumes that do not already exist, so opening an existing
    /// chain is non-destructive. Callers that must not create the chain should
    /// gate on `base_path.exists()` first (see [`read_live`]).
    async fn open(&self) -> Result<StreamingSnowball> {
        StreamingSnowball::new(&self.base_path).await
    }

    /// Read the live chain state. If the chain has not been initialized on disk
    /// yet, report an honest `uninitialized` state rather than creating it as a
    /// side effect of a read.
    async fn read_live(&self) -> Result<SnowballState> {
        if !self.base_path.exists() {
            return Ok(SnowballState {
                status: "uninitialized".to_string(),
                base_path: self.base_path.display().to_string(),
                snapshot_interval: String::new(),
                retention: RetentionView {
                    hourly: 0,
                    daily: 0,
                    weekly: 0,
                    quarterly: 0,
                },
                snapshot_count: 0,
                snapshots: Vec::new(),
                current_state: None,
            });
        }

        let chain = self.open().await?;

        let snapshots: Vec<SnapshotEntry> = chain
            .list_snapshots()
            .await?
            .into_iter()
            .map(|(name, created)| SnapshotEntry { name, created })
            .collect();

        let current_state = chain.current_state().await?;

        let retention = chain.retention_policy().await;

        Ok(SnowballState {
            status: "active".to_string(),
            base_path: chain.base_path().display().to_string(),
            snapshot_interval: chain.snapshot_interval().to_string(),
            retention: RetentionView {
                hourly: retention.hourly,
                daily: retention.daily,
                weekly: retention.weekly,
                quarterly: retention.quarterly,
            },
            snapshot_count: snapshots.len(),
            snapshots,
            current_state,
        })
    }
}

/// Read-live wrapper for callers outside this module (the bridge's typed
/// dispatch, which has no other way to fetch this plugin's current state to
/// publish into the SHM projection after a successful call).
pub async fn current_state() -> Result<SnowballState> {
    SnowballPlugin::new().read_live().await
}

impl SnowballPlugin {
    /// Synchronous shape exemplar used only to derive the [`PluginSchema`]. The
    /// schema describes the *shape* of the plugin's live state output (field
    /// names + types); the data path itself is always live.
    fn schema_exemplar() -> SnowballState {
        SnowballState {
            status: "active".to_string(),
            base_path: DEFAULT_BASE_PATH.to_string(),
            snapshot_interval: "every-15-minutes".to_string(),
            retention: RetentionView {
                hourly: 24,
                daily: 7,
                weekly: 4,
                quarterly: 4,
            },
            snapshot_count: 0,
            snapshots: vec![SnapshotEntry {
                name: "state-000001".to_string(),
                created: "1970-01-01 00:00:00 UTC".to_string(),
            }],
            current_state: None,
        }
    }
}

#[async_trait]
impl StatePlugin for SnowballPlugin {
    fn name(&self) -> &str {
        "snowball"
    }

    fn version(&self) -> &str {
        "2.1.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(snowball_schema())
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        // The only writable projection of an append-only audit chain is its DR
        // `current_state` blob. Emit a real Modify when the desired state
        // differs, otherwise a NoOp.
        let actions = if current == desired {
            vec![StateAction::NoOp {
                resource: "current_state".to_string(),
            }]
        } else {
            vec![StateAction::Modify {
                resource: "current_state".to_string(),
                changes: desired.clone(),
            }]
        };
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        })
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let _ = diff;
        anyhow::bail!(
            "Snowball state mutation requires the canonical shared Snowball method adapter"
        )
    }

    async fn verify_state(&self, desired: &Value) -> Result<bool> {
        let current = simd_json::serde::to_owned_value(self.read_live().await?)?;
        Ok(&current == desired)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        anyhow::bail!("Snowball checkpoints require the canonical shared Snowball method adapter")
    }

    async fn rollback(&self, checkpoint: &Checkpoint) -> Result<()> {
        let _ = checkpoint;
        anyhow::bail!("Snowball rollback requires the canonical shared Snowball method adapter")
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

pub(crate) fn snowball_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(SnowballState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "snowball",
        "2.1.0",
        "Streaming snowball audit chain — snapshots, DR current-state, retention/interval",
        &root,
    );

    // Add methods
    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
    use op_state_store::SideEffect;

    schema.methods.insert(
        "create_snapshot".to_string(),
        method_decl_from_schemars_with_output::<SnowballEmptyInput, CreateSnapshotOutput>(
            "create_snapshot",
            SideEffect::Mutation,
            false,
            "snowball.invoke",
            "mut.service.snowball.snapshot.create@v1",
        ),
    );
    schema.methods.insert(
        "list_snapshots".to_string(),
        method_decl_from_schemars_with_output::<SnowballEmptyInput, ListSnapshotsOutput>(
            "list_snapshots",
            SideEffect::Read,
            true,
            "snowball.read",
            "obs.service.snowball.snapshot.list@v1",
        ),
    );
    schema.methods.insert(
        "get_snapshot".to_string(),
        method_decl_from_schemars_with_output::<SnapshotInput, GetSnapshotOutput>(
            "get_snapshot",
            SideEffect::Read,
            true,
            "snowball.read",
            "obs.service.snowball.snapshot.get@v1",
        ),
    );
    schema.methods.insert(
        "rollback".to_string(),
        method_decl_from_schemars_with_output::<SnapshotInput, RollbackOutput>(
            "rollback",
            SideEffect::Mutation,
            true,
            "snowball.invoke",
            "mut.service.snowball.rollback@v1",
        ),
    );
    schema.methods.insert(
        "get_current_state".to_string(),
        method_decl_from_schemars_with_output::<SnowballEmptyInput, GetCurrentStateOutput>(
            "get_current_state",
            SideEffect::Read,
            true,
            "snowball.read",
            "obs.service.snowball.state.get@v1",
        ),
    );
    schema.methods.insert(
        "set_retention".to_string(),
        method_decl_from_schemars_with_output::<SetRetentionInput, SetRetentionOutput>(
            "set_retention",
            SideEffect::Mutation,
            false,
            "snowball.invoke",
            "mut.service.snowball.retention.set@v1",
        ),
    );
    schema.methods.insert(
        "get_stats".to_string(),
        method_decl_from_schemars_with_output::<SnowballEmptyInput, SnowballStatsOutput>(
            "get_stats",
            SideEffect::Read,
            true,
            "snowball.read",
            "obs.service.snowball.stats@v1",
        ),
    );

    // Audit-trail query surface. MutationEngine dispatches these two through
    // EventChain and the seven native methods above through SnowballMethods.
    schema.methods.insert(
        "query_events".to_string(),
        method_decl_from_schemars_with_output::<QueryEventsInput, QueryEventsOutput>(
            "query_events",
            SideEffect::Read,
            true,
            "snowball.read",
            "obs.service.snowball.events.query@v1",
        ),
    );
    schema.methods.insert(
        "verify_chain".to_string(),
        method_decl_from_schemars_with_output::<VerifyChainInput, VerifyChainOutput>(
            "verify_chain",
            SideEffect::Read,
            true,
            "snowball.read",
            "obs.service.snowball.chain.verify@v1",
        ),
    );
    schema.methods.insert(
        "get_projector_stats".to_string(),
        method_decl_from_schemars_with_output::<GetProjectorStatsInput, ProjectorStatsOutput>(
            "get_projector_stats",
            SideEffect::Read,
            true,
            "snowball.read",
            "obs.service.snowball.projector-stats@v1",
        ),
    );

    schema.methods.insert(
        "search_own_events".to_string(),
        method_decl_from_schemars_with_output::<SearchOwnEventsInput, SearchOwnEventsOutput>(
            "search_own_events",
            SideEffect::Read,
            true,
            SEARCH_OWN_EVENTS_CAPABILITY,
            "obs.service.snowball.events.self.search@v1",
        ),
    );
    schema.capabilities.insert(
        SEARCH_OWN_EVENTS_CAPABILITY.to_string(),
        op_state_store::CapabilityDecl {
            id: SEARCH_OWN_EVENTS_CAPABILITY.to_string(),
            description: "Search canonical audit metadata for the caller's own current session in Accountability mode using Voyage. Does not grant access to other actors or sessions.".to_string(),
        },
    );
    schema.capabilities.insert(
        "snowball.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "snowball.read".to_string(),
            description: "Grants: list_snapshots, get_snapshot, get_current_state, get_stats, query_events, verify_chain, get_projector_stats.".to_string(),
        },
    );
    schema.capabilities.insert(
        "snowball.invoke".to_string(),
        op_state_store::CapabilityDecl {
            id: "snowball.invoke".to_string(),
            description: "Grants: create_snapshot, rollback, set_retention.".to_string(),
        },
    );

    schema
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("snowball", |_ctx| std::sync::Arc::new(SnowballPlugin::new()))
}
