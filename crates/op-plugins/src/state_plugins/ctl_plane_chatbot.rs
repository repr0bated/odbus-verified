//! Control-plane chatbot reasoning episode plugin
//!
//! Declares the canonical schema for the chatbot's reasoning episodes —
//! every field, PII classification, significance, and vectorization contract.
//! THE PLUGIN IS THE SCHEMA: downstream (CozoDB, Accountability UI,
//! gRPC EventChainService) inherits from this definition.
//!
//! Related: REQ-1 through REQ-10 (Control-Plane Chatbot Reasoning Episode
//! Vectorization spec). This plugin covers REQ-2 (episode record fields)
//! and REQ-3 (plugin schema registration).

use anyhow::Result;
use async_trait::async_trait;
use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff, StatePlugin,
};
use op_state_store::PluginSchema;
use crate::state_plugins::chat_router::{ChatInput, ChatMessage, ChatOutput};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use simd_json::prelude::*;
use std::collections::BTreeMap;

const DEFAULT_COLLECTION: &str = "ctl_plane_reasoning_episodes";
const DEFAULT_DEDUP_WINDOW_HRS: u32 = 24;
const DEFAULT_QUEUE_ALERT_THRESHOLD: u32 = 50;
const DEFAULT_EMBEDDING_PLUGIN: &str = "embedding_model";

// ── Config (vectorization pipeline tuning) ──────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CtlPlaneChatbotConfig {
    #[serde(default = "default_embedding_plugin")]
    #[schemars(extend("const" = DEFAULT_EMBEDDING_PLUGIN))]
    pub embedding_plugin: String,
    #[serde(default = "default_collection")]
    /// Reserved legacy collection label; no Qdrant episode index is implemented.
    #[schemars(extend("const" = DEFAULT_COLLECTION), extend("readOnly" = true))]
    pub qdrant_collection: String,
    #[serde(default)]
    pub vector_dims: MatryoshkaDim,
    #[serde(default)]
    #[schemars(extend("const" = "float"))]
    pub output_dtype: OutputDtype,
    #[serde(default)]
    #[schemars(extend("const" = "document"))]
    pub input_type: InputType,
    #[serde(default = "default_dedup_window_hrs")]
    /// Reserved legacy value; content-based episode merging is not implemented.
    #[schemars(extend("const" = DEFAULT_DEDUP_WINDOW_HRS), extend("readOnly" = true))]
    pub dedup_window_hrs: u32,
    #[serde(default = "default_queue_alert_threshold")]
    /// Reserved legacy value; vectorization is explicitly invoked, without a queue.
    #[schemars(extend("const" = DEFAULT_QUEUE_ALERT_THRESHOLD), extend("readOnly" = true))]
    pub queue_alert_threshold: u32,
    #[serde(default)]
    /// Reserved legacy value; each completed native Chat creates one episode.
    #[schemars(extend("const" = "flat"), extend("readOnly" = true))]
    pub nesting_policy: NestingPolicy,
    #[serde(default = "default_true")]
    pub vectorization_enabled: bool,
}

fn default_embedding_plugin() -> String {
    DEFAULT_EMBEDDING_PLUGIN.into()
}
fn default_collection() -> String {
    DEFAULT_COLLECTION.into()
}
fn default_chat_llm_plugin() -> String {
    "large_language_model".into()
}
fn default_true() -> bool {
    true
}

impl Default for CtlPlaneChatbotConfig {
    fn default() -> Self {
        Self {
            embedding_plugin: default_embedding_plugin(),
            qdrant_collection: default_collection(),
            vector_dims: MatryoshkaDim::D1024,
            output_dtype: OutputDtype::Float,
            input_type: InputType::Document,
            dedup_window_hrs: DEFAULT_DEDUP_WINDOW_HRS,
            queue_alert_threshold: DEFAULT_QUEUE_ALERT_THRESHOLD,
            nesting_policy: NestingPolicy::Flat,
            vectorization_enabled: true,
        }
    }
}

// ── Schema state (schemars-derived) ─────────────────────────────────────────

/// Accepted output dimension of the configured embedding owner. Episode
/// vectorization verifies this exact dimension; it does not truncate or migrate
/// existing vectors when the setting changes.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
pub enum MatryoshkaDim {
    #[serde(rename = "256")]
    D256,
    #[serde(rename = "512")]
    D512,
    #[serde(rename = "1024")]
    D1024,
    #[serde(rename = "2048")]
    D2048,
}

impl Default for MatryoshkaDim {
    fn default() -> Self {
        MatryoshkaDim::D1024
    }
}

impl MatryoshkaDim {
    /// Numeric dimensionality.
    pub fn dims(&self) -> u32 {
        match self {
            MatryoshkaDim::D256 => 256,
            MatryoshkaDim::D512 => 512,
            MatryoshkaDim::D1024 => 1024,
            MatryoshkaDim::D2048 => 2048,
        }
    }
}

/// Historical output formats. The current episode owner supports only `float`;
/// the config contract fixes this value until other formats have backing code.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OutputDtype {
    Float,
    Int8,
    Uint8,
    Binary,
    Ubinary,
}

impl Default for OutputDtype {
    fn default() -> Self {
        OutputDtype::Float
    }
}

/// Retrieval `input_type` (Voyage input_type). Voyage prepends a retrieval prompt
/// based on this: documents embed as `document`, queries as `query`. The Voyage
/// FAQ says **not** to omit it / use `None` for retrieval, so `None` is
/// intentionally not offered here. Stored reasoning episodes are documents →
/// default `document`; the search side uses `query`. Embeddings with and without
/// input_type stay compatible, so this is a quality knob, not a space knob.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum InputType {
    Query,
    Document,
}

impl Default for InputType {
    fn default() -> Self {
        InputType::Document
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NestingPolicy {
    Flat,
    Nested,
}

impl Default for NestingPolicy {
    fn default() -> Self {
        NestingPolicy::Flat
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Goal,
    ToolResult,
    Interrupt,
    Replan,
    SystemEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    ToolCall,
    ResponseEmitted,
    DirectionChange,
    GoalAchieved,
    ConfigSet,
    TaskScheduled,
    Interrupt,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeClass {
    GoalAchieved,
    ConfigSet,
    TaskScheduled,
    Delegated,
    Interrupted,
    DirectionChanged,
    Inconclusive,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
pub enum SignificanceLevel {
    Contextual,
    Signal,
}

impl Default for SignificanceLevel {
    fn default() -> Self {
        SignificanceLevel::Contextual
    }
}

fn default_dedup_window_hrs() -> u32 {
    DEFAULT_DEDUP_WINDOW_HRS
}
fn default_queue_alert_threshold() -> u32 {
    DEFAULT_QUEUE_ALERT_THRESHOLD
}
fn default_running() -> bool {
    true
}
fn default_reasoning_active() -> bool {
    false
}
fn default_embedding_queue_depth() -> u32 {
    0
}
fn default_false() -> bool {
    false
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReasoningEpisode {
    #[schemars(description = "Verified principal that executed this episode [PII]", extend("readOnly" = true))]
    pub principal_id: String,
    #[schemars(description = "Verified identity session that executed this episode [PII]", extend("readOnly" = true))]
    pub session_id: String,
    #[schemars(description = "Immutable verified session genesis binding [PII]", extend("readOnly" = true))]
    pub session_genesis: String,
    #[schemars(description = "Verified principal on whose behalf the episode ran [PII]", extend("readOnly" = true))]
    pub on_behalf_of: String,
    #[schemars(
        description = "Unique ID (UUID v7 for time-ordering)",
        example = &"01912abc-def0-7abc-8def-0123456789ab",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.episode-id@v1")
    )]
    pub episode_id: String,
    #[schemars(
        description = "ISO-8601 timestamp of reasoning entry",
        example = &"2025-05-29T14:30:00Z",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.started-at@v1")
    )]
    pub started_at: String,
    #[schemars(
        description = "ISO-8601 timestamp of reasoning exit",
        example = &"2025-05-29T14:30:05Z",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.ended-at@v1")
    )]
    pub ended_at: String,
    #[schemars(
        description = "Wall-clock duration in milliseconds",
        example = 5000,
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.duration-ms@v1")
    )]
    pub duration_ms: u64,
    #[schemars(
        description = "What caused reasoning to start",
        example = &"goal",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.trigger@v1")
    )]
    pub trigger: Trigger,
    #[schemars(
        description = "What ended reasoning",
        example = &"tool_call",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.exit-reason@v1")
    )]
    pub exit_reason: ExitReason,
    #[schemars(
        description = "High-level goal or prompt active at episode start [PII]",
        example = &"Configure VLAN isolation for tenant-3",
        extend("readOnly" = true),
        extend("x-opdbus-private" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.goal-text@v1")
    )]
    pub goal_text: Option<String>,
    #[schemars(
        description = "Compact natural-language summary of reasoning — primary embedding input [PII]",
        example = &"Evaluated 3 bridge configs, chose br-tenant3 for isolation",
        extend("readOnly" = true),
        extend("x-opdbus-private" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.reasoning-summary@v1")
    )]
    pub reasoning_summary: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "Ordered list of tools/plugins/MCP calls made during the episode",
        example = &["ovs_list_bridges", "ovs_create_bridge"],
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.tools-consulted@v1")
    )]
    pub tools_consulted: Vec<String>,
    #[schemars(
        description = "The decision, plan, or action the episode produced [PII]",
        example = &"Create br-tenant3 with VLAN 103 tagged ports",
        extend("readOnly" = true),
        extend("x-opdbus-private" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.decision-output@v1")
    )]
    pub decision_output: Option<String>,
    #[schemars(
        description = "Classification of episode outcome. goal_achieved/config_set/task_scheduled => Signal significance",
        example = &"config_set",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.outcome-class@v1")
    )]
    pub outcome_class: OutcomeClass,
    #[schemars(
        range(min = 0.0, max = 1.0),
        description = "Optional confidence 0.0-1.0 if the model emits one",
        example = 0.87,
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.confidence@v1")
    )]
    pub confidence: Option<f64>,
    #[schemars(
        description = "Plugin that owns the context being reasoned about",
        example = &"ovsdb_bridge",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.plugin-id@v1")
    )]
    pub plugin_id: Option<String>,
    #[schemars(
        description = "Groups episodes belonging to the same high-level task chain",
        example = &"vlan-isolation-task-3",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.conversation-id@v1")
    )]
    pub conversation_id: Option<String>,
    #[schemars(
        description = "SHA-256 of the typed serialized episode with content_hash set to the empty string; validates immutable episode content and embedding updates",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.content-hash@v1")
    )]
    pub content_hash: String,
    #[serde(default = "default_false")]
    #[schemars(
        description = "If true, goal_text, reasoning_summary and decision_output are omitted before embedding. Native Chat sets this conservatively; no PII classifier or release workflow is implemented",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.pii-flagged@v1")
    )]
    pub pii_flagged: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Present when the episode is an event-triggered agent run (OD-63): status, timing, token usage and the action timeline",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.run@v1")
    )]
    pub run: Option<RunRecord>,
}

/// Terminal state of an event-triggered agent run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Succeeded,
    Dropped,
    Failed,
    Timeout,
}

/// One action the run performed (or was refused).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunAction {
    /// `plugin.method`.
    pub method: String,
    pub ok: bool,
    pub duration_ms: u64,
    /// Refusal or error, bounded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The run record an event-triggered agent commits with its episode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    pub status: RunStatus,
    pub trigger_id: String,
    /// blake3 digest of the triggering topic (the topic itself may be data).
    pub topic_ref: String,
    pub queued_ms: u64,
    pub duration_ms: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    #[serde(default)]
    pub actions: Vec<RunAction>,
    /// Why the run ended other than `succeeded`, bounded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Significance {
    #[schemars(
        description = "Reasoning episodes are always at least Contextual. goal_achieved/config_set/task_scheduled => Signal",
        example = &"Signal",
        extend("readOnly" = true),
        extend("default" = &"Contextual"),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.significance-level@v1")
    )]
    pub level: SignificanceLevel,
    #[schemars(
        description = "Significance rule that was evaluated",
        example = &"outcome_class in [goal_achieved, config_set, task_scheduled]",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.significance-rule@v1")
    )]
    pub rule: Option<String>,
}

/// One knowledge push the context engine produced for this exact identity
/// scope. Durable in the private episode Cozo owner, projected into the
/// private SHM document, and announced to the UI by a content-free wake.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KnowledgePush {
    #[schemars(description = "Verified principal this push belongs to [PII]", extend("readOnly" = true))]
    pub principal_id: String,
    #[schemars(description = "Verified identity session this push belongs to [PII]", extend("readOnly" = true))]
    pub session_id: String,
    #[schemars(description = "Immutable verified session genesis binding [PII]", extend("readOnly" = true))]
    pub session_genesis: String,
    #[schemars(
        description = "Durable push id: the zero-padded scope revision that committed it, so ids order by commit",
        example = &"00000000000000000042",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.knowledge.push-id@v1")
    )]
    pub push_id: String,
    #[schemars(
        description = "Context-engine push id (idempotency key across retries)",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.knowledge.source-id@v1")
    )]
    pub source_id: String,
    #[schemars(
        description = "ISO-8601 time the context engine produced the push",
        example = &"2026-09-19T14:30:00Z",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.knowledge.created-at@v1")
    )]
    pub created_at: String,
    #[schemars(
        description = "What triggered the push (context engine trigger name)",
        example = &"ErrorAssistance",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.knowledge.trigger@v1")
    )]
    pub trigger: String,
    #[schemars(
        description = "Why the engine pushed it [PII]",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.knowledge.trigger-reason@v1")
    )]
    pub trigger_reason: String,
    #[schemars(
        range(min = 0.0, max = 1.0),
        description = "Engine relevance score 0.0-1.0",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.knowledge.relevance@v1")
    )]
    pub relevance_score: f64,
    #[schemars(
        with = "serde_json::Value",
        description = "Typed knowledge content as the engine emitted it [PII]",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.knowledge.content@v1")
    )]
    pub content: serde_json::Value,
    #[schemars(
        description = "Suggested next action, when the engine proposed one [PII]",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.knowledge.suggested-action@v1")
    )]
    pub suggested_action: Option<String>,
    #[schemars(
        description = "SHA-256 of the typed push serialized with content_hash set to the empty string",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.knowledge.content-hash@v1")
    )]
    pub content_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.ctl-plane-chatbot.schema@v6"))]
#[schemars(extend("x-oscal-category" = "llm"))]
pub struct CtlPlaneChatbotState {
    #[serde(default)]
    #[schemars(with = "serde_json::Value", description = "Private identity-scoped durable episode snapshot; absent from public projections", extend("readOnly" = true))]
    pub episodes: BTreeMap<String, ReasoningEpisode>,
    #[serde(default)]
    #[schemars(description = "Total durable episodes in the exact private identity scope", extend("readOnly" = true))]
    pub episode_count: u64,
    #[serde(default)]
    #[schemars(description = "True when the private SHM snapshot omits older durable episodes", extend("readOnly" = true))]
    pub episodes_truncated: bool,
    #[serde(default)]
    #[schemars(description = "Durable monotonic revision of the private identity scope; advanced by every episode and knowledge commit", extend("readOnly" = true))]
    pub episode_revision: u64,
    #[serde(default)]
    #[schemars(with = "serde_json::Value", description = "Private identity-scoped durable knowledge pushes (newest bounded window); absent from public projections", extend("readOnly" = true))]
    pub knowledge: BTreeMap<String, KnowledgePush>,
    #[serde(default)]
    #[schemars(description = "Total durable knowledge pushes in the exact private identity scope", extend("readOnly" = true))]
    pub knowledge_count: u64,
    #[serde(default)]
    #[schemars(description = "True when the private SHM snapshot omits older durable knowledge pushes", extend("readOnly" = true))]
    pub knowledge_truncated: bool,
    #[serde(default = "default_chat_llm_plugin")]
    #[schemars(
        description = "Reserved legacy field without an active config owner. Native Chat routing is selected through the tched_router contract.",
        example = &"large_language_model",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "mut.software.plugin.ctl-plane-chatbot.llm-plugin@v1")
    )]
    pub llm_plugin: String,
    #[serde(default)]
    #[schemars(
        description = "Reserved legacy field; no chatbot-specific model override is implemented. Native Chat uses tched_router selection.",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "mut.software.plugin.ctl-plane-chatbot.chat-model@v1")
    )]
    pub chat_model: String,
    #[serde(default = "default_embedding_plugin")]
    #[schemars(
        description = "Embedding surface this chatbot delegates to for vectorizing reasoning episodes (default embedding_model). The model/provider/dimensions live in that plugin; the chatbot holds no embedding model of its own.",
        example = &"embedding_model",
        extend("const" = DEFAULT_EMBEDDING_PLUGIN),
        extend("x-oscal-subid" = "mut.software.plugin.ctl-plane-chatbot.embedding-plugin@v1")
    )]
    pub embedding_plugin: String,
    #[serde(default = "default_collection")]
    #[schemars(
        description = "Reserved legacy label. Episodes and vectors are stored in the private Cozo owner; no Qdrant episode index is implemented.",
        extend("readOnly" = true),
        extend("const" = DEFAULT_COLLECTION),
        extend("x-oscal-subid" = "mut.software.plugin.ctl-plane-chatbot.qdrant-collection@v1")
    )]
    pub qdrant_collection: String,
    #[serde(default)]
    #[schemars(
        description = "Required embedding output dimension. The result must match the embedding owner's configured dimension; existing vectors are not automatically truncated or migrated.",
        extend("x-oscal-subid" = "mut.software.plugin.ctl-plane-chatbot.vector-dims@v1")
    )]
    pub vector_dims: MatryoshkaDim,
    #[serde(default)]
    #[schemars(
        description = "Stored vector format. Only float is implemented by the private episode owner.",
        extend("const" = "float"),
        extend("x-opdbus-exact-json-schema" = true),
        extend("x-oscal-subid" = "mut.software.plugin.ctl-plane-chatbot.output-dtype@v1")
    )]
    pub output_dtype: OutputDtype,
    #[serde(default)]
    #[schemars(
        description = "Retrieval input_type (Voyage). Documents embed as 'document', queries as 'query' — Voyage prepends a retrieval prompt. None/omitted is discouraged for retrieval so it is not offered. Stored episodes default to document.",
        extend("const" = "document"),
        extend("x-opdbus-exact-json-schema" = true),
        extend("x-oscal-subid" = "mut.software.plugin.ctl-plane-chatbot.input-type@v1")
    )]
    pub input_type: InputType,
    #[serde(default = "default_dedup_window_hrs")]
    #[schemars(
        description = "Reserved legacy value. Content-based episode merging is not implemented; each completed native Chat has its own immutable episode.",
        extend("readOnly" = true),
        extend("const" = DEFAULT_DEDUP_WINDOW_HRS),
        extend("x-opdbus-exact-json-schema" = true),
        extend("x-oscal-subid" = "mut.software.plugin.ctl-plane-chatbot.dedup-window-hrs@v1")
    )]
    pub dedup_window_hrs: u32,
    #[serde(default = "default_queue_alert_threshold")]
    #[schemars(
        description = "Reserved legacy value. Vectorization is explicitly invoked; no background embedding queue or alerting is implemented.",
        extend("readOnly" = true),
        extend("const" = DEFAULT_QUEUE_ALERT_THRESHOLD),
        extend("x-opdbus-exact-json-schema" = true),
        extend("x-oscal-subid" = "mut.software.plugin.ctl-plane-chatbot.queue-alert-threshold@v1")
    )]
    pub queue_alert_threshold: u32,
    #[serde(default)]
    #[schemars(
        description = "Reserved legacy value. Trigger nesting is not implemented; each completed native Chat creates one episode.",
        extend("readOnly" = true),
        extend("const" = "flat"),
        extend("x-opdbus-exact-json-schema" = true),
        extend("x-oscal-subid" = "mut.software.plugin.ctl-plane-chatbot.nesting-policy@v1")
    )]
    pub nesting_policy: NestingPolicy,
    #[serde(default = "default_true")]
    #[schemars(
        description = "Enable explicit episode vectorization through the canonical embedding_model owner",
        extend("x-oscal-subid" = "mut.software.plugin.ctl-plane-chatbot.vectorization-enabled@v1")
    )]
    pub vectorization_enabled: bool,
    #[serde(default = "default_running")]
    #[schemars(
        description = "Whether the chatbot is currently active",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.running@v1")
    )]
    pub running: bool,
    #[serde(default = "default_reasoning_active")]
    #[schemars(
        description = "Whether the chatbot is currently in reasoning state (REQ-1)",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.reasoning-active@v1")
    )]
    pub reasoning_active: bool,
    #[serde(default = "default_embedding_queue_depth")]
    #[schemars(
        description = "Reserved observation; no embedding queue producer is implemented, so this field is absent from committed episode state",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.embedding-queue-depth@v1")
    )]
    pub embedding_queue_depth: u32,
    #[schemars(
        description = "Reserved observation; no aggregate last-vectorized timestamp producer is implemented",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.last-vectorized-at@v1")
    )]
    pub last_vectorized_at: Option<String>,
    #[schemars(
        description = "Reserved legacy observation. Episode vectors are scoped by principal, session and genesis in the private Cozo store; they are not written into SealedId",
        example = &"a1b2c3d4-e5f6-7890-abcd-ef0123456789",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.vector-id@v1")
    )]
    pub vector_id: Option<String>,
    #[schemars(
        description = "REQ-3: Always at least Contextual. goal_achieved/config_set/task_scheduled => Signal",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.software.plugin.ctl-plane-chatbot.significance@v1")
    )]
    pub significance: Option<Significance>,
}

// ── Plugin struct ────────────────────────────────────────────────────────────

pub struct CtlPlaneChatbotPlugin;

impl CtlPlaneChatbotPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for CtlPlaneChatbotPlugin {
    fn default() -> Self {
        Self::new()
    }
}

// ── StatePlugin impl ─────────────────────────────────────────────────────────

#[async_trait]
impl StatePlugin for CtlPlaneChatbotPlugin {
    fn name(&self) -> &str {
        "ctl_plane_chatbot"
    }
    fn version(&self) -> &str {
        "4.2.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = ctl_plane_chatbot_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }

    fn is_available(&self) -> bool {
        // Always available — the chatbot is the control plane itself
        true
    }

    fn unavailable_reason(&self) -> String {
        String::new()
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        let cur: CtlPlaneChatbotConfig = simd_json::serde::from_owned_value(current.clone())?;
        let des: CtlPlaneChatbotConfig = simd_json::serde::from_owned_value(desired.clone())?;

        let mut actions = Vec::new();
        macro_rules! field_diff {
            ($field:ident, $key:expr_2021) => {
                if cur.$field != des.$field {
                    actions.push(StateAction::Modify {
                        resource: $key.into(),
                        changes: simd_json::serde::to_owned_value(&des.$field)?,
                    });
                }
            };
        }
        field_diff!(embedding_plugin, "embedding_plugin");
        field_diff!(qdrant_collection, "qdrant_collection");
        field_diff!(vector_dims, "vector_dims");
        field_diff!(output_dtype, "output_dtype");
        field_diff!(input_type, "input_type");
        field_diff!(dedup_window_hrs, "dedup_window_hrs");
        field_diff!(queue_alert_threshold, "queue_alert_threshold");
        field_diff!(nesting_policy, "nesting_policy");
        field_diff!(vectorization_enabled, "vectorization_enabled");

        Ok(StateDiff {
            plugin: self.name().into(),
            actions,
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: format!("{:x}", md5::compute(simd_json::to_string(current)?)),
                desired_hash: format!("{:x}", md5::compute(simd_json::to_string(desired)?)),
            },
        })
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let _ = diff;
        anyhow::bail!(
            "generic ctl_plane_chatbot state apply is unsupported; use the canonical set_config method with durable revision CAS"
        )
    }

    async fn verify_state(&self, desired: &Value) -> Result<bool> {
        let _ = desired;
        anyhow::bail!(
            "generic ctl_plane_chatbot verification is unsupported; use the canonical durable get_config receipt"
        )
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        anyhow::bail!(
            "generic ctl_plane_chatbot checkpoints are unsupported; no canonical durable checkpoint owner is declared"
        )
    }

    async fn rollback(&self, checkpoint: &Checkpoint) -> Result<()> {
        let _ = checkpoint;
        anyhow::bail!(
            "generic ctl_plane_chatbot rollback is unsupported; use an explicit set_config revision transition"
        )
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: false,
            atomic_operations: false,
        }
    }
}

pub(crate) fn ctl_plane_chatbot_schema() -> PluginSchema {
    use super::ctl_plane_chatbot_methods::{
        ClassifySignificanceInput, ClassifySignificanceOutput, GetConfigInput, GetConfigOutput,
        GetDoorbellInput, GetDoorbellOutput, GetEpisodeInput, GetEpisodeOutput, ListEpisodesInput,
        ListEpisodesOutput, ListKnowledgeInput, ListKnowledgeOutput, QueryContextInput,
        QueryContextOutput, SetConfigInput, SetConfigOutput, VectorizeInput, VectorizeOutput,
    };
    let root = serde_json::to_value(schemars::schema_for!(CtlPlaneChatbotState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "ctl_plane_chatbot",
        "4.2.0",
        "Control-plane chatbot reasoning episode contract, PII-aware significance rules, durable singleton configuration, and typed embedding facade",
        &root,
    );
    // Finite FieldType maps cannot retain every constraint of the typed
    // episode. Replace the adapter placeholder with its complete schema,
    // derived from the same Rust type before the plugin is ever published.
    schema
        .fields
        .get_mut("episodes")
        .expect("episode snapshot field")
        .field_type = op_state_store::FieldType::exact_json_schema(
        serde_json::to_value(schemars::schema_for!(BTreeMap<String, ReasoningEpisode>))
            .expect("episode map schema serializes"),
    )
    .expect("episode snapshot schema is self-contained");
    schema
        .fields
        .get_mut("knowledge")
        .expect("knowledge snapshot field")
        .field_type = op_state_store::FieldType::exact_json_schema(
        serde_json::to_value(schemars::schema_for!(BTreeMap<String, KnowledgePush>))
            .expect("knowledge map schema serializes"),
    )
    .expect("knowledge snapshot schema is self-contained");

    // Durable writes are announced as signals so the chain (and the vector
    // projection, OD-64) records the committed document under its own
    // schema — the schema is the noise filter for what gets embedded.
    schema.signals.push(op_state_store::SignalDecl {
        name: "episode_committed".to_string(),
        payload: Some(
            simd_json::serde::to_owned_value(schemars::schema_for!(ReasoningEpisode))
                .expect("episode schema serializes"),
        ),
        subid: "evt.service.ctl-plane-chatbot.episode.committed@v1".to_string(),
    });
    schema.signals.push(op_state_store::SignalDecl {
        name: "knowledge_committed".to_string(),
        payload: Some(
            simd_json::serde::to_owned_value(schemars::schema_for!(KnowledgePush))
                .expect("knowledge schema serializes"),
        ),
        subid: "evt.service.ctl-plane-chatbot.knowledge.committed@v1".to_string(),
    });

    // Add methods
    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
    use op_state_store::SideEffect;

    schema.methods.insert(
        "get_config".to_string(),
        method_decl_from_schemars_with_output::<GetConfigInput, GetConfigOutput>(
            "get_config",
            SideEffect::Read,
            true,
            "chatbot.read",
            "obs.service.ctl-plane-chatbot.config.get@v1",
        ),
    );
    schema.methods.insert(
        "set_config".to_string(),
        method_decl_from_schemars_with_output::<SetConfigInput, SetConfigOutput>(
            "set_config",
            SideEffect::Mutation,
            false,
            "chatbot.invoke",
            "mut.service.ctl-plane-chatbot.config.set@v1",
        ),
    );
    schema.methods.insert(
        "list_episodes".to_string(),
        method_decl_from_schemars_with_output::<ListEpisodesInput, ListEpisodesOutput>(
            "list_episodes",
            SideEffect::Read,
            true,
            "chatbot.read",
            "obs.service.ctl-plane-chatbot.episode.list@v1",
        ),
    );
    schema.methods.insert(
        "get_episode".to_string(),
        method_decl_from_schemars_with_output::<GetEpisodeInput, GetEpisodeOutput>(
            "get_episode",
            SideEffect::Read,
            true,
            "chatbot.read",
            "obs.service.ctl-plane-chatbot.episode.get@v1",
        ),
    );
    schema.methods.insert(
        "list_knowledge".to_string(),
        method_decl_from_schemars_with_output::<ListKnowledgeInput, ListKnowledgeOutput>(
            "list_knowledge",
            SideEffect::Read,
            true,
            "chatbot.read",
            "obs.service.ctl-plane-chatbot.knowledge.list@v1",
        ),
    );
    schema.methods.insert(
        "get_doorbell".to_string(),
        method_decl_from_schemars_with_output::<GetDoorbellInput, GetDoorbellOutput>(
            "get_doorbell",
            SideEffect::Read,
            true,
            "chatbot.read",
            "obs.service.ctl-plane-chatbot.doorbell.get@v1",
        ),
    );
    schema.methods.insert(
        "query_context".to_string(),
        method_decl_from_schemars_with_output::<QueryContextInput, QueryContextOutput>(
            "query_context",
            SideEffect::Read,
            true,
            "chatbot.read",
            "obs.service.ctl-plane-chatbot.context.query@v1",
        ),
    );
    schema.methods.insert(
        "classify_significance".to_string(),
        method_decl_from_schemars_with_output::<
            ClassifySignificanceInput,
            ClassifySignificanceOutput,
        >(
            "classify_significance",
            SideEffect::Read,
            true,
            "chatbot.invoke",
            "mut.service.ctl-plane-chatbot.classify@v1",
        ),
    );
    schema.methods.insert(
        "vectorize".to_string(),
        method_decl_from_schemars_with_output::<VectorizeInput, VectorizeOutput>(
            "vectorize",
            SideEffect::Mutation,
            false,
            "chatbot.invoke",
            "mut.service.ctl-plane-chatbot.vectorize@v1",
        ),
    );
    schema.methods.insert(
        "Chat".to_string(),
        method_decl_from_schemars_with_output::<ChatInput, ChatOutput>(
            "Chat",
            SideEffect::Mutation,
            false,
            "chatbot.invoke",
            "mut.service.ctl-plane-chatbot.chat@v1",
        ),
    );

    schema.dependencies.push("embedding_model".to_string());

    schema.capabilities.insert(
        "chatbot.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "chatbot.read".to_string(),
            description: "Grants: get_config, list_episodes, get_episode, query_context."
                .to_string(),
        },
    );
    schema.capabilities.insert(
        "chatbot.invoke".to_string(),
        op_state_store::CapabilityDecl {
            id: "chatbot.invoke".to_string(),
            description: "Grants: set_config, classify_significance, vectorize, Chat.".to_string(),
        },
    );

    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v3_methods_are_typed_and_pure_classification_is_idempotent() {
        let schema = ctl_plane_chatbot_schema();
        assert_eq!(schema.version, "4.2.0");
        for method in [
            "get_config",
            "set_config",
            "list_episodes",
            "get_episode",
            "get_doorbell",
            "list_knowledge",
            "query_context",
            "classify_significance",
            "vectorize",
            "Chat",
        ] {
            let declaration = &schema.methods[method];
            assert!(declaration.returns.is_some());
            assert_ne!(declaration.args, simd_json::json!(null));
        }
        let classify = &schema.methods["classify_significance"];
        assert_eq!(classify.side_effect, op_state_store::SideEffect::Read);
        assert!(classify.idempotent);
        let vectorize = &schema.methods["vectorize"];
        let args = simd_json::to_string(&vectorize.args).unwrap();
        let returns = simd_json::to_string(vectorize.returns.as_ref().unwrap()).unwrap();
        assert!(args.contains("episode"));
        assert!(returns.contains("vector"));
        assert!(returns.contains("config_revision"));
        assert!(!returns.contains("vector_id"));
    }

    #[test]
    fn complete_config_fields_are_a_valid_partial_root_without_fake_runtime_state() {
        let schema = ctl_plane_chatbot_schema();
        let state = super::super::ctl_plane_chatbot_methods::config_state_fields(
            &CtlPlaneChatbotConfig::default(),
        )
        .unwrap();
        let state = simd_json::serde::to_owned_value(state).unwrap();
        assert!(schema.validate(&state).valid);
        assert!(state.get("running").is_none());
        assert!(state.get("reasoning_episode").is_none());
        assert!(state.get("vector_id").is_none());
    }

    #[tokio::test]
    async fn generic_lifecycle_hooks_never_claim_durable_config_effects() {
        let plugin = CtlPlaneChatbotPlugin::new();
        let current = simd_json::serde::to_owned_value(CtlPlaneChatbotConfig::default()).unwrap();
        let desired_config = CtlPlaneChatbotConfig {
            vectorization_enabled: false,
            ..CtlPlaneChatbotConfig::default()
        };
        let desired = simd_json::serde::to_owned_value(desired_config).unwrap();
        let diff = plugin.calculate_diff(&current, &desired).await.unwrap();
        assert_eq!(diff.actions.len(), 1, "pure diff remains descriptive only");
        assert!(plugin.apply_state(&diff).await.is_err());
        assert!(plugin.verify_state(&desired).await.is_err());
        assert!(plugin.create_checkpoint().await.is_err());
        let checkpoint = Checkpoint {
            id: "untrusted-fixture".into(),
            plugin: plugin.name().into(),
            timestamp: 0,
            state_snapshot: simd_json::json!(null),
            backend_checkpoint: None,
        };
        assert!(plugin.rollback(&checkpoint).await.is_err());
        let capabilities = StatePlugin::capabilities(&plugin);
        assert!(!capabilities.supports_rollback);
        assert!(!capabilities.supports_checkpoints);
        assert!(!capabilities.supports_verification);
        assert!(!capabilities.atomic_operations);
    }

    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(CtlPlaneChatbotState)).unwrap();
        let mut subids = Vec::new();
        collect_subids(&raw, &mut subids);
        for subid in subids {
            assert!(
                crate::state_plugins::common::oscal::validate_subid(&subid).is_ok(),
                "invalid subid: {subid}"
            );
        }
    }

    fn collect_subids(value: &serde_json::Value, out: &mut Vec<String>) {
        if let Some(obj) = value.as_object() {
            if let Some(subid) = obj.get("x-oscal-subid").and_then(|v| v.as_str()) {
                out.push(subid.to_string());
            }
            for v in obj.values() {
                collect_subids(v, out);
            }
        }
        if let Some(arr) = value.as_array() {
            for v in arr {
                collect_subids(v, out);
            }
        }
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("ctl_plane_chatbot", |_ctx| std::sync::Arc::new(CtlPlaneChatbotPlugin::new()))
}
