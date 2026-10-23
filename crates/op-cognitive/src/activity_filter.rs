//! Chatbot Activity Filter — Schema-Derived
//!
//! Significance is derived directly from the plugin schema. There is no
//! separate filter config: the schema IS the filter.
//!
//! # Signal derivation rules (from PluginSchema)
//!
//! | Schema condition                              | Significance      |
//! |-----------------------------------------------|-------------------|
//! | schema tag `"noise"` or `"overkill"`          | Noise  (suppress) |
//! | schema tag `"immutable"` + write op           | Signal            |
//! | field in `immutable_paths` + write op         | Signal            |
//! | field `read_only: true` + write op (violation)| Signal            |
//! | constraint failure on any field               | Signal            |
//! | tunable field write                           | Contextual        |
//! | field read (non-sensitive)                    | Routine           |
//! | `Autonomous` origin, any op                   | Signal (override) |
//! | health check / debug probe                    | Noise             |
//!
//! Users suppress unwanted events by tagging their plugin schema with
//! `"noise"` or `"overkill"` — no separate filter config needed.
//!
//! # Deduplication
//!
//! Exact content-hash dedup within the same actor, conversation and operation
//! context in a sliding arrival-time window. Tool calls bypass
//! dedup (idempotent retries still matter). Window size is a single
//! runtime tunable, not a per-plugin concern.

use anyhow::Result;
use chrono::{DateTime, Utc};
use op_state_store::plugin_schema::PluginSchema;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Significance tier. Ordered: Signal > Contextual > Routine > Noise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Significance {
    Noise,
    Routine,
    Contextual,
    Signal,
}

/// Derive significance for an operation directly from the plugin schema.
///
/// `field` is the specific field being touched, if known.
/// `is_write` distinguishes reads from writes for read_only checks.
/// `constraint_failed` should be true if validation rejected the operation.
/// `autonomous` — model acted without instruction; always upgrades to Signal.
pub fn derive_significance(
    schema: &PluginSchema,
    field: Option<&str>,
    is_write: bool,
    constraint_failed: bool,
    autonomous: bool,
) -> Significance {
    // Autonomous always Signal — we always want to know when the model acted alone
    if autonomous {
        return Significance::Signal;
    }

    // Constraint failure is always Signal regardless of field
    if constraint_failed {
        return Significance::Signal;
    }

    // Schema-level noise tags suppress everything from this plugin
    if schema.tags.iter().any(|t| t == "noise" || t == "overkill") {
        return Significance::Noise;
    }

    // Fully immutable schema — any write is Signal
    if is_write && schema.tags.iter().any(|t| t == "immutable") {
        return Significance::Signal;
    }

    if let Some(field_name) = field {
        // Field in immutable_paths — write is Signal
        let raw_name = field_name.trim_start_matches('/');
        let matches_immutable = schema.immutable_paths.iter().any(|p| {
            p == field_name
                || p.trim_start_matches('/') == raw_name
                || p == &format!("/tunable/{raw_name}")
        });
        if is_write && matches_immutable {
            return Significance::Signal;
        }

        // read_only field write — this is a violation attempt, always Signal
        if is_write
            && let Some(field_schema) = schema
                .fields
                .get(raw_name)
                .or_else(|| schema.fields.get(field_name))
            && field_schema.read_only
        {
            return Significance::Signal;
        }
    }

    // Writes to tunable fields are Contextual
    if is_write {
        return Significance::Contextual;
    }

    // Reads are Routine
    Significance::Routine
}

/// Operation kind — used for the hard-suppress gate before schema lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    ToolCall,
    MemoryWrite,
    MemoryRead,
    AutonomousDecision,
    IntentClassification,
    StateMutation,
    PolicyDecision,
    SignalEmit,
    HealthCheck,
    DebugRead,
    WorkflowStep,
    SessionLifecycle,
}

impl OpKind {
    /// Operations that are always Noise regardless of schema.
    /// These never reach the snowball or Qdrant.
    pub fn is_always_noise(&self) -> bool {
        matches!(self, OpKind::HealthCheck | OpKind::DebugRead)
    }

    /// Operations that are always at least Contextual regardless of schema.
    pub fn is_always_contextual(&self) -> bool {
        matches!(
            self,
            OpKind::ToolCall | OpKind::SignalEmit | OpKind::PolicyDecision
        )
    }
}

/// An event produced by chatbot or agent activity, ready to be filtered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityEvent {
    pub id: String,
    pub timestamp: DateTime<Utc>,

    /// The user who initiated the conversation this event belongs to.
    pub user_id: Option<String>,

    /// The conversation (chat session) this event belongs to.
    /// Groups the full why→what→who chain for a single session.
    pub conversation_id: Option<String>,

    /// The actor (chatbot, agent ID, cron, etc.)
    pub actor_id: String,

    pub op_kind: OpKind,

    /// True when the model acted without explicit instruction.
    pub autonomous: bool,

    /// Model confidence if autonomous (0.0–1.0).
    pub confidence: Option<f32>,

    /// Plugin that owns the state being touched, if applicable.
    pub plugin_id: Option<String>,

    /// Specific field being touched, if applicable.
    pub field: Option<String>,

    /// True if this is a write operation (vs read).
    pub is_write: bool,

    /// True if a schema constraint failed on this operation.
    pub constraint_failed: bool,

    pub memory_ref: Option<String>,
    pub tool_name: Option<String>,

    /// SHA-256 of the canonical serialised payload. Used for exact dedup.
    pub content_hash: String,

    /// Text summary for embedding / Qdrant upsert.
    pub summary: String,

    pub payload: serde_json::Value,
}

/// Outcome of the filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterDecision {
    /// Emit to snowball AND Qdrant vector search.
    Emit(Significance),
    /// Emit to snowball only — payload/summary stripped before Qdrant upsert.
    /// Used for PII-tagged plugin fields: auditable but not searchable.
    EmitChainOnly(Significance),
    Suppress(SuppressReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SuppressReason {
    AlwaysNoise,
    SchemaTaggedNoise,
    BelowMinSignificance,
    ExactDuplicate,
}

/// Check whether a plugin schema or the specific field touched is tagged PII.
///
/// PII events reach the snowball (audit trail) but are stripped before Qdrant upsert.
/// Tag the schema-level with `"pii"` to mark the entire plugin,
/// or tag individual fields with `"pii"` in their description/metadata to mark specific fields.
pub fn is_pii(schema: &PluginSchema, field: Option<&str>) -> bool {
    if schema.tags.iter().any(|t| t == "pii") {
        return true;
    }
    if let Some(field_name) = field
        && let Some(field_schema) = schema
            .fields
            .get(field_name.trim_start_matches('/'))
            .or_else(|| schema.fields.get(field_name))
    {
        return field_schema.description.to_lowercase().contains("[pii]")
            || field_schema.constraints.iter().any(|c| {
                matches!(c, op_state_store::plugin_schema::Constraint::Custom { validator }
                        if validator == "pii")
            });
    }
    false
}

struct WindowEntry {
    received_at: Instant,
    key: DedupKey,
}

#[derive(PartialEq, Eq)]
struct DedupKey {
    user_id: Option<String>,
    conversation_id: Option<String>,
    actor_id: String,
    plugin_id: Option<String>,
    field: Option<String>,
    op_kind: OpKind,
    autonomous: bool,
    is_write: bool,
    constraint_failed: bool,
    memory_ref: Option<String>,
    tool_name: Option<String>,
    content_hash: String,
}

impl From<&ActivityEvent> for DedupKey {
    fn from(event: &ActivityEvent) -> Self {
        Self {
            user_id: event.user_id.clone(),
            conversation_id: event.conversation_id.clone(),
            actor_id: event.actor_id.clone(),
            plugin_id: event.plugin_id.clone(),
            field: event.field.clone(),
            op_kind: event.op_kind.clone(),
            autonomous: event.autonomous,
            is_write: event.is_write,
            constraint_failed: event.constraint_failed,
            memory_ref: event.memory_ref.clone(),
            tool_name: event.tool_name.clone(),
            content_hash: event.content_hash.clone(),
        }
    }
}

/// Runtime tunables — the only config outside the plugin schema.
/// Kept minimal: just the dedup window and minimum significance floor.
#[derive(Debug, Clone)]
pub struct FilterTunables {
    /// Minimum significance to emit. Default: Contextual.
    pub min_significance: Significance,
    /// Sliding dedup window duration in seconds. Default: 300.
    pub dedup_window_secs: i64,
    /// Max entries in dedup window. Default: 500.
    pub dedup_window_max: usize,
}

impl Default for FilterTunables {
    fn default() -> Self {
        Self {
            min_significance: Significance::Contextual,
            dedup_window_secs: 300,
            dedup_window_max: 500,
        }
    }
}

pub struct ActivityFilter {
    tunables: Arc<RwLock<FilterTunables>>,
    window: Arc<RwLock<VecDeque<WindowEntry>>>,
}

impl ActivityFilter {
    pub fn new(tunables: FilterTunables) -> Self {
        Self {
            tunables: Arc::new(RwLock::new(tunables)),
            window: Arc::new(RwLock::new(VecDeque::new())),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(FilterTunables::default())
    }

    pub async fn set_tunables(&self, t: FilterTunables) {
        *self.tunables.write().await = t;
    }

    /// Evaluate an event against the plugin schema + tunables.
    /// Pass `schema = None` for events with no associated plugin (tool calls, etc.)
    pub async fn evaluate(
        &self,
        event: &ActivityEvent,
        schema: Option<&PluginSchema>,
    ) -> Result<FilterDecision> {
        let tunables = self.tunables.read().await.clone();

        // Gate 1 — always-noise op kinds
        if event.op_kind.is_always_noise() {
            return Ok(FilterDecision::Suppress(SuppressReason::AlwaysNoise));
        }

        // Gate 2 — derive significance from plugin schema
        let autonomous = event.autonomous || event.op_kind == OpKind::AutonomousDecision;
        let sig = if let Some(schema) = schema {
            let schema_sig = derive_significance(
                schema,
                event.field.as_deref(),
                event.is_write,
                event.constraint_failed,
                autonomous,
            );

            // Schema said Noise — respect it
            if schema_sig == Significance::Noise {
                return Ok(FilterDecision::Suppress(SuppressReason::SchemaTaggedNoise));
            }

            // Always-contextual ops can't fall below Contextual
            if event.op_kind.is_always_contextual() {
                schema_sig.max(Significance::Contextual)
            } else {
                schema_sig
            }
        } else {
            // No schema — use op kind alone
            if autonomous {
                Significance::Signal
            } else if event.op_kind.is_always_contextual() {
                Significance::Contextual
            } else {
                Significance::Routine
            }
        };

        if sig < tunables.min_significance {
            return Ok(FilterDecision::Suppress(
                SuppressReason::BelowMinSignificance,
            ));
        }

        // Gate 3 — atomically expire, compare and remember scoped events.
        // Arrival time is monotonic and independent of an event's supplied clock.
        {
            let mut w = self.window.write().await;
            let now = Instant::now();
            let duration = Duration::from_secs(tunables.dedup_window_secs.max(0) as u64);
            w.retain(|entry| now.duration_since(entry.received_at) < duration);
            while w.len() > tunables.dedup_window_max {
                w.pop_front();
            }
            // Tool calls bypass dedup — retries are meaningful signal.
            if event.op_kind != OpKind::ToolCall
                && tunables.dedup_window_max > 0
                && !duration.is_zero()
            {
                let key = DedupKey::from(event);
                if w.iter().any(|entry| entry.key == key) {
                    return Ok(FilterDecision::Suppress(SuppressReason::ExactDuplicate));
                }
                while w.len() >= tunables.dedup_window_max {
                    w.pop_front();
                }
                w.push_back(WindowEntry {
                    received_at: now,
                    key,
                });
            }
        }

        // PII gate — chain yes, Qdrant no
        let pii = schema.is_some_and(|s| is_pii(s, event.field.as_deref()));
        if pii {
            return Ok(FilterDecision::EmitChainOnly(sig));
        }

        Ok(FilterDecision::Emit(sig))
    }

    pub async fn window_len(&self) -> usize {
        self.window.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_state_store::plugin_schema::{FieldSchema, FieldType, PluginSchemaBuilder};

    fn signal_event() -> ActivityEvent {
        ActivityEvent {
            id: "event".into(),
            timestamp: Utc::now(),
            user_id: Some("user".into()),
            conversation_id: Some("conversation".into()),
            actor_id: "actor".into(),
            op_kind: OpKind::AutonomousDecision,
            autonomous: false,
            confidence: None,
            plugin_id: Some("plugin".into()),
            field: None,
            is_write: false,
            constraint_failed: false,
            memory_ref: None,
            tool_name: None,
            content_hash: "same-payload".into(),
            summary: "decision".into(),
            payload: serde_json::json!({}),
        }
    }

    #[tokio::test]
    async fn concurrent_duplicates_emit_once() {
        let filter = ActivityFilter::with_defaults();
        let event = signal_event();
        let (first, second) = tokio::join!(
            filter.evaluate(&event, None),
            filter.evaluate(&event, None),
        );
        let decisions = [first.unwrap(), second.unwrap()];
        assert_eq!(decisions.iter().filter(|d| **d == FilterDecision::Emit(Significance::Signal)).count(), 1);
        assert_eq!(decisions.iter().filter(|d| **d == FilterDecision::Suppress(SuppressReason::ExactDuplicate)).count(), 1);
    }

    #[tokio::test]
    async fn duplicate_payload_keeps_distinct_conversations() {
        let filter = ActivityFilter::with_defaults();
        let mut event = signal_event();
        assert_eq!(filter.evaluate(&event, None).await.unwrap(), FilterDecision::Emit(Significance::Signal));
        event.conversation_id = Some("another-conversation".into());
        assert_eq!(filter.evaluate(&event, None).await.unwrap(), FilterDecision::Emit(Significance::Signal));
    }

    #[tokio::test]
    async fn supplied_timestamp_does_not_change_arrival_window() {
        let filter = ActivityFilter::with_defaults();
        let mut event = signal_event();
        event.timestamp = DateTime::UNIX_EPOCH;
        assert_eq!(filter.evaluate(&event, None).await.unwrap(), FilterDecision::Emit(Significance::Signal));
        assert_eq!(filter.evaluate(&event, None).await.unwrap(), FilterDecision::Suppress(SuppressReason::ExactDuplicate));
    }

    #[tokio::test]
    async fn autonomous_kind_overrides_noise_schema() {
        let filter = ActivityFilter::with_defaults();
        assert_eq!(filter.evaluate(&signal_event(), Some(&noise_schema())).await.unwrap(), FilterDecision::Emit(Significance::Signal));
    }

    #[test]
    fn pii_lookup_accepts_top_level_pointer() {
        let mut schema = PluginSchemaBuilder::new("profile").build();
        let field = FieldSchema {
            field_type: FieldType::String,
            required: false,
            description: "[PII] Contact address".into(),
            default: None,
            example: None,
            constraints: Vec::new(),
            read_only: false,
            read_only_when: None,
        };
        schema.fields.insert("email".into(), field);
        assert!(is_pii(&schema, Some("/email")));
        assert!(!is_pii(&schema, Some("/unrelated/email")));
    }

    fn noise_schema() -> PluginSchema {
        PluginSchemaBuilder::new("test")
            .version("1.0")
            .description("noise plugin")
            .tag("noise")
            .build()
    }

    fn immutable_schema() -> PluginSchema {
        PluginSchemaBuilder::new("test")
            .version("1.0")
            .description("immutable plugin")
            .fully_immutable()
            .build()
    }

    #[test]
    fn test_noise_tag_suppresses() {
        let schema = noise_schema();
        let sig = derive_significance(&schema, None, true, false, false);
        assert_eq!(sig, Significance::Noise);
    }

    #[test]
    fn test_immutable_write_is_signal() {
        let schema = immutable_schema();
        let sig = derive_significance(&schema, None, true, false, false);
        assert_eq!(sig, Significance::Signal);
    }

    #[test]
    fn test_autonomous_always_signal() {
        let schema = noise_schema(); // even noise schema can't suppress autonomous
        let sig = derive_significance(&schema, None, false, false, true);
        assert_eq!(sig, Significance::Signal);
    }

    #[test]
    fn test_constraint_fail_always_signal() {
        let schema = noise_schema();
        let sig = derive_significance(&schema, None, false, true, false);
        assert_eq!(sig, Significance::Signal);
    }

    #[test]
    fn test_read_is_routine() {
        let schema = PluginSchemaBuilder::new("t").build();
        let sig = derive_significance(&schema, Some("field_x"), false, false, false);
        assert_eq!(sig, Significance::Routine);
    }

    #[test]
    fn test_pii_tag_detected_schema_level() {
        let schema = PluginSchemaBuilder::new("user-profile")
            .version("1.0")
            .description("user profile")
            .tag("pii")
            .build();
        assert!(is_pii(&schema, None));
        assert!(is_pii(&schema, Some("email")));
    }

    #[test]
    fn test_non_pii_schema_not_flagged() {
        let schema = PluginSchemaBuilder::new("metrics").build();
        assert!(!is_pii(&schema, None));
    }

    #[tokio::test]
    async fn test_pii_schema_emits_chain_only() {
        let filter = ActivityFilter::with_defaults();
        let schema = PluginSchemaBuilder::new("user-profile")
            .version("1.0")
            .description("user profile")
            .tag("pii")
            .build();
        let event = ActivityEvent {
            id: "pii1".into(),
            timestamp: Utc::now(),
            user_id: Some("u1".into()),
            conversation_id: Some("c1".into()),
            actor_id: "bot".into(),
            op_kind: OpKind::StateMutation,
            autonomous: false,
            confidence: None,
            plugin_id: Some("user-profile".into()),
            field: Some("email".into()),
            is_write: true,
            constraint_failed: false,
            memory_ref: None,
            tool_name: None,
            content_hash: "pii-hash-1".into(),
            summary: "update email".into(),
            payload: serde_json::json!({"email": "user@example.com"}),
        };
        let d = filter.evaluate(&event, Some(&schema)).await.unwrap();
        assert_eq!(d, FilterDecision::EmitChainOnly(Significance::Contextual));
    }

    #[tokio::test]
    async fn test_health_check_suppressed() {
        let filter = ActivityFilter::with_defaults();
        let event = ActivityEvent {
            id: "1".into(),
            timestamp: Utc::now(),
            user_id: None,
            conversation_id: None,
            actor_id: "bot".into(),
            op_kind: OpKind::HealthCheck,
            autonomous: false,
            confidence: None,
            plugin_id: None,
            field: None,
            is_write: false,
            constraint_failed: false,
            memory_ref: None,
            tool_name: None,
            content_hash: "h1".into(),
            summary: "ping".into(),
            payload: serde_json::json!({}),
        };
        let d = filter.evaluate(&event, None).await.unwrap();
        assert_eq!(d, FilterDecision::Suppress(SuppressReason::AlwaysNoise));
    }
}
