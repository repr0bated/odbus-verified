//! Chat response plugin (OD-90 stage 1).
//!
//! Every chat turn ends in a response tool call, never raw model text. The
//! three response methods are those tool calls; this plugin IS their schema.
//!
//! Owner decisions (2026-09-28): a conversation belongs to the asking human's
//! principal and spans logins. Each committed turn records the session it came
//! from as provenance. An agent identity (the chain actor) authors responses
//! into the human's private scope, bound by a bridge-minted `turn_id`. Turns
//! form a per-conversation sub-chain whose order and completeness can be
//! verified from public fields alone (`conversation_id`, `turn_index`,
//! `prev_turn_hash`, `turn_hash`, `content_hash`), without reading content.
//!
//! Content fields carry `x-opdbus-private: true` (OD-91 marker). This plugin
//! only declares the marker; the chain and vector filters that honour it are
//! OD-91's work.

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::collections::BTreeMap;

pub const PLUGIN_ID: &str = "chat_response";
pub const PLUGIN_VERSION: &str = "1.0.0";
/// Agents only: authors the three response methods into a human's scope.
pub const RESPOND_CAPABILITY: &str = "chat_response.respond";
/// Humans, on their own principal scope: start a conversation and read it.
pub const READ_CAPABILITY: &str = "chat_response.read";
/// Content-free commit announcement (owner key, ids and hashes only).
pub const RESPONSE_COMMITTED_SIGNAL: &str = "response_committed";

// ── Response content (private) ───────────────────────────────────────────────

/// How the user interface presents a `respond_to_user` message.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Success,
    Info,
    Warning,
    Error,
}

/// Why a request cannot be performed.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCategory {
    Dangerous,
    NotAllowed,
    MissingInfo,
    PolicyViolation,
    NotSupported,
}

/// The authored content of one response. `content_hash` is the SHA-256 of
/// this value's canonical JSON (object keys sorted), so it commits to the
/// content without revealing it.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResponseContent {
    /// `respond_to_user`: the answer, optionally with a validated Canvas.
    UserResponse {
        #[schemars(
            description = "The message shown to the user [PII]",
            extend("x-opdbus-private" = true)
        )]
        message: String,
        #[schemars(description = "How the message is presented")]
        message_type: MessageType,
        #[serde(default)]
        #[schemars(description = "Tool or method names the agent called to produce this response")]
        related_actions: Vec<String>,
        #[serde(default)]
        #[schemars(
            description = "json-render Canvas spec validated against the sealed json_render Canvas profile [PII]",
            extend("x-opdbus-private" = true)
        )]
        canvas: Option<serde_json::Map<String, serde_json::Value>>,
    },
    /// `request_clarification`: the agent needs more from the user.
    Clarification {
        #[schemars(
            description = "The clarifying question [PII]",
            extend("x-opdbus-private" = true)
        )]
        question: String,
        #[serde(default)]
        #[schemars(
            description = "Why the clarification is needed [PII]",
            extend("x-opdbus-private" = true)
        )]
        context: Option<String>,
        #[serde(default)]
        #[schemars(
            description = "Options the user may choose from [PII]",
            extend("x-opdbus-private" = true)
        )]
        options: Vec<String>,
        #[serde(default)]
        #[schemars(
            description = "Information the agent is missing [PII]",
            extend("x-opdbus-private" = true)
        )]
        required_fields: Vec<String>,
    },
    /// `cannot_perform`: a recorded refusal.
    Refusal {
        #[schemars(
            description = "Why the request cannot be performed [PII]",
            extend("x-opdbus-private" = true)
        )]
        reason: String,
        #[schemars(description = "Refusal category")]
        category: RefusalCategory,
        #[serde(default)]
        #[schemars(
            description = "Alternatives the user could take [PII]",
            extend("x-opdbus-private" = true)
        )]
        alternatives: Vec<String>,
    },
    /// A human's message: the human side of the conversation, recorded in
    /// the same chain as the responses so a conversation reads both ways.
    HumanMessage {
        #[schemars(
            description = "What the human said [PII]",
            extend("x-opdbus-private" = true)
        )]
        message: String,
    },
}

// ── Committed records ────────────────────────────────────────────────────────

/// One committed turn in the human's private principal scope: either the
/// human's own message or an agent's response to it. The conversation chain
/// runs through both, so the record shows what was asked as well as answered.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ChatTurn {
    #[schemars(
        description = "Human principal whose private scope holds this turn [PII]",
        extend("readOnly" = true, "x-opdbus-private" = true)
    )]
    pub principal_id: String,
    #[schemars(
        description = "Verified human session that asked (provenance only; the scope is the principal) [PII]",
        extend("readOnly" = true, "x-opdbus-private" = true)
    )]
    pub session_id: String,
    #[schemars(
        description = "Genesis of the asking session (provenance only) [PII]",
        extend("readOnly" = true, "x-opdbus-private" = true)
    )]
    pub session_genesis: String,
    #[schemars(
        description = "Bridge-minted conversation id (public)",
        extend("readOnly" = true)
    )]
    pub conversation_id: String,
    #[schemars(
        description = "Bridge-minted turn id: the human message's own id, or the turn the response answered (public)",
        extend("readOnly" = true)
    )]
    pub turn_id: String,
    #[schemars(
        description = "Zero-based position of this turn in its conversation (public)",
        extend("readOnly" = true)
    )]
    pub turn_index: u64,
    #[schemars(
        description = "Bridge-minted id of this committed turn",
        extend("readOnly" = true)
    )]
    pub response_id: String,
    #[schemars(
        description = "Principal that authored this turn: the human for a human message, the answering agent for a response (the chain actor)",
        extend("readOnly" = true)
    )]
    pub responder_principal_id: String,
    #[schemars(
        description = "RFC3339 time the bridge opened the turn (for a human message, when it was received)",
        extend("readOnly" = true)
    )]
    pub opened_at: String,
    #[schemars(
        description = "RFC3339 time the response was committed",
        extend("readOnly" = true)
    )]
    pub committed_at: String,
    #[schemars(
        description = "turn_hash of the previous turn, or the conversation genesis_hash for turn 0 (public)",
        extend("readOnly" = true)
    )]
    pub prev_turn_hash: String,
    #[schemars(
        description = "SHA-256 over the canonical JSON array [\"opdbus.chat_response.turn.v1\", prev_turn_hash, conversation_id, turn_index, turn_id, content_hash, responder_principal_id] (public)",
        extend("readOnly" = true)
    )]
    pub turn_hash: String,
    #[schemars(
        description = "SHA-256 of the canonical JSON of content (public commitment to private content)",
        extend("readOnly" = true)
    )]
    pub content_hash: String,
    #[schemars(
        description = "Global chain event id of the call that carried this turn: the response method, or the human's chat request",
        extend("readOnly" = true)
    )]
    pub request_event_id: u64,
    #[schemars(
        description = "Global chain event_hash of the call that carried this turn",
        extend("readOnly" = true)
    )]
    pub request_event_hash: String,
    #[schemars(
        description = "Sealed json_render schema hash the canvas was validated against",
        extend("readOnly" = true)
    )]
    pub canvas_schema_hash: Option<String>,
    #[schemars(
        description = "The authored turn content [PII]",
        extend("readOnly" = true),
        extend("x-opdbus-private" = true)
    )]
    pub content: ResponseContent,
}

/// A conversation in a human's private principal scope.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConversationSummary {
    #[schemars(
        description = "Human principal that owns the conversation [PII]",
        extend("readOnly" = true, "x-opdbus-private" = true)
    )]
    pub principal_id: String,
    #[schemars(
        description = "Bridge-minted conversation id (public)",
        extend("readOnly" = true)
    )]
    pub conversation_id: String,
    #[schemars(
        description = "Optional user-chosen title [PII]",
        extend("readOnly" = true),
        extend("x-opdbus-private" = true)
    )]
    pub title: Option<String>,
    #[schemars(
        description = "RFC3339 time the conversation started",
        extend("readOnly" = true)
    )]
    pub started_at: String,
    #[schemars(
        description = "Verified session that started the conversation (provenance only) [PII]",
        extend("readOnly" = true, "x-opdbus-private" = true)
    )]
    pub session_id: String,
    #[schemars(
        description = "Genesis of the starting session (provenance only) [PII]",
        extend("readOnly" = true, "x-opdbus-private" = true)
    )]
    pub session_genesis: String,
    #[schemars(
        description = "SHA-256 over the canonical JSON array [\"opdbus.chat_response.genesis.v1\", conversation_id, principal_id, started_at]; prev_turn_hash of turn 0",
        extend("readOnly" = true)
    )]
    pub genesis_hash: String,
    #[schemars(
        description = "Committed turns in this conversation",
        extend("readOnly" = true)
    )]
    pub turn_count: u64,
    #[schemars(
        description = "turn_hash of the newest committed turn, or genesis_hash when empty",
        extend("readOnly" = true)
    )]
    pub head_turn_hash: String,
    #[schemars(
        description = "RFC3339 time of the newest committed turn, or started_at",
        extend("readOnly" = true)
    )]
    pub updated_at: String,
}

/// Payload of `response_committed`. Content-free: an owner key, ids and
/// hashes only, so the chain can verify a conversation without its content.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResponseCommittedSignal {
    /// `sha256(principal_id)`: the private projection owner key.
    pub owner_key: String,
    pub conversation_id: String,
    pub turn_id: String,
    pub response_id: String,
    pub turn_index: u64,
    pub prev_turn_hash: String,
    pub turn_hash: String,
    pub content_hash: String,
    /// Durable revision of the owner's scope after the commit.
    pub revision: u64,
    /// The admitted response method call on the global chain.
    pub request_event_id: u64,
    pub request_event_hash: String,
}

// ── Plugin state (private, principal-scoped) ─────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.service.chat-response.schema@v1"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct ChatResponseState {
    #[serde(default)]
    #[schemars(
        with = "serde_json::Value",
        description = "Private principal-scoped conversations (most recently active bounded window), keyed by conversation_id; absent from public projections",
        extend("readOnly" = true)
    )]
    pub conversations: BTreeMap<String, ConversationSummary>,
    #[serde(default)]
    #[schemars(
        description = "Total durable conversations in the principal scope",
        extend("readOnly" = true)
    )]
    pub conversation_count: u64,
    #[serde(default)]
    #[schemars(
        description = "True when the private snapshot omits older conversations",
        extend("readOnly" = true)
    )]
    pub conversations_truncated: bool,
    #[serde(default)]
    #[schemars(
        with = "serde_json::Value",
        description = "Private committed turns (newest bounded window across conversations), keyed by turn_id; absent from public projections",
        extend("readOnly" = true)
    )]
    pub turns: BTreeMap<String, ChatTurn>,
    #[serde(default)]
    #[schemars(
        description = "Total durable turns in the principal scope",
        extend("readOnly" = true)
    )]
    pub turn_count: u64,
    #[serde(default)]
    #[schemars(
        description = "True when the private snapshot omits older turns",
        extend("readOnly" = true)
    )]
    pub turns_truncated: bool,
    #[serde(default)]
    #[schemars(
        description = "Durable monotonic revision of the principal scope; advanced by every conversation and turn commit",
        extend("readOnly" = true)
    )]
    pub revision: u64,
}

// ── Plugin ───────────────────────────────────────────────────────────────────

pub struct ChatResponsePlugin;

impl ChatResponsePlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ChatResponsePlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatePlugin for ChatResponsePlugin {
    fn name(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = chat_response_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }

    fn is_available(&self) -> bool {
        true
    }

    fn unavailable_reason(&self) -> String {
        String::new()
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        let _ = (current, desired);
        anyhow::bail!(
            "chat_response state is private and principal-scoped; it changes only through its typed methods"
        )
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let _ = diff;
        anyhow::bail!(
            "generic chat_response state apply is unsupported; responses commit through the bridge turn registry"
        )
    }

    async fn verify_state(&self, desired: &Value) -> Result<bool> {
        let _ = desired;
        anyhow::bail!("generic chat_response verification is unsupported")
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        anyhow::bail!("chat_response has no generic checkpoint owner")
    }

    async fn rollback(&self, checkpoint: &Checkpoint) -> Result<()> {
        let _ = checkpoint;
        anyhow::bail!("committed chat turns are immutable; rollback is unsupported")
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

/// Every method with its subid, side effect and capability. The schema, the
/// bridge dispatch and the tests all read this one table.
pub const METHODS: [(&str, &str, bool); 7] = [
    // (method, subid, is_mutation)
    (
        "start_conversation",
        "mut.service.chat-response.conversation.start@v1",
        true,
    ),
    (
        "respond_to_user",
        "mut.service.chat-response.user.respond@v1",
        true,
    ),
    (
        "request_clarification",
        "mut.service.chat-response.clarification.request@v1",
        true,
    ),
    (
        "cannot_perform",
        "mut.service.chat-response.refusal.record@v1",
        true,
    ),
    (
        "get_turn_response",
        "obs.service.chat-response.turn.get@v1",
        false,
    ),
    (
        "list_conversations",
        "obs.service.chat-response.conversation.list@v1",
        false,
    ),
    (
        "get_conversation",
        "obs.service.chat-response.conversation.get@v1",
        false,
    ),
];

pub const RESPONSE_COMMITTED_SUBID: &str = "evt.service.chat-response.response.committed@v1";

/// The capability a method requires: the three response methods need
/// `chat_response.respond`; starting and reading need `chat_response.read`.
pub fn required_capability(method: &str) -> Option<&'static str> {
    match method {
        "respond_to_user" | "request_clarification" | "cannot_perform" => Some(RESPOND_CAPABILITY),
        "start_conversation" | "get_turn_response" | "list_conversations" | "get_conversation" => {
            Some(READ_CAPABILITY)
        }
        _ => None,
    }
}

pub(crate) fn chat_response_schema() -> PluginSchema {
    use super::chat_response_methods::{
        CannotPerformInput, GetConversationInput, GetConversationOutput, GetTurnResponseInput,
        GetTurnResponseOutput, ListConversationsInput, ListConversationsOutput,
        RequestClarificationInput, RespondToUserInput, RespondToUserOutput, ResponseCommitOutput,
        StartConversationInput, StartConversationOutput,
    };
    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
    use op_state_store::{MethodDecl, SideEffect};

    let root = serde_json::to_value(schemars::schema_for!(ChatResponseState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_ID,
        PLUGIN_VERSION,
        "Chat responses as sealed tool calls: agents answer bridge-minted turns into the asking human's private principal-scoped conversations, chained by public turn hashes",
        &root,
    );
    // The finite FieldType view cannot keep the typed records' constraints;
    // replace the adapter placeholders with the complete schemas derived from
    // the same Rust types (as ctl_plane_chatbot does for its episodes).
    schema
        .fields
        .get_mut("conversations")
        .expect("conversation snapshot field")
        .field_type = op_state_store::FieldType::exact_json_schema(
        serde_json::to_value(schemars::schema_for!(BTreeMap<String, ConversationSummary>))
            .expect("conversation map schema serializes"),
    )
    .expect("conversation snapshot schema is self-contained");
    schema
        .fields
        .get_mut("turns")
        .expect("turn snapshot field")
        .field_type = op_state_store::FieldType::exact_json_schema(
        serde_json::to_value(schemars::schema_for!(BTreeMap<String, ChatTurn>))
            .expect("turn map schema serializes"),
    )
    .expect("turn snapshot schema is self-contained");

    schema.signals.push(op_state_store::SignalDecl {
        name: RESPONSE_COMMITTED_SIGNAL.to_string(),
        payload: Some(
            simd_json::serde::to_owned_value(schemars::schema_for!(ResponseCommittedSignal))
                .expect("signal schema serializes"),
        ),
        subid: RESPONSE_COMMITTED_SUBID.to_string(),
    });

    fn declare<I: schemars::JsonSchema, O: schemars::JsonSchema>(
        name: &str,
        subid: &str,
        mutation: bool,
    ) -> MethodDecl {
        method_decl_from_schemars_with_output::<I, O>(
            name,
            if mutation {
                SideEffect::Mutation
            } else {
                SideEffect::Read
            },
            !mutation,
            required_capability(name).expect("declared chat_response method has a capability"),
            subid,
        )
    }
    for (name, subid, mutation) in METHODS {
        let declaration = match name {
            "start_conversation" => {
                declare::<StartConversationInput, StartConversationOutput>(name, subid, mutation)
            }
            "respond_to_user" => {
                declare::<RespondToUserInput, RespondToUserOutput>(name, subid, mutation)
            }
            "request_clarification" => {
                declare::<RequestClarificationInput, ResponseCommitOutput>(name, subid, mutation)
            }
            "cannot_perform" => {
                declare::<CannotPerformInput, ResponseCommitOutput>(name, subid, mutation)
            }
            "get_turn_response" => {
                declare::<GetTurnResponseInput, GetTurnResponseOutput>(name, subid, mutation)
            }
            "list_conversations" => {
                declare::<ListConversationsInput, ListConversationsOutput>(name, subid, mutation)
            }
            "get_conversation" => {
                declare::<GetConversationInput, GetConversationOutput>(name, subid, mutation)
            }
            other => unreachable!("undeclared chat_response method {other}"),
        };
        schema.methods.insert(name.to_string(), declaration);
    }

    schema.capabilities.insert(
        RESPOND_CAPABILITY.to_string(),
        op_state_store::CapabilityDecl {
            id: RESPOND_CAPABILITY.to_string(),
            description: "Agents only. Grants: respond_to_user, request_clarification, cannot_perform — each answers one bridge-minted turn whose designated responder is the caller, into the asking human's scope.".to_string(),
        },
    );
    schema.capabilities.insert(
        READ_CAPABILITY.to_string(),
        op_state_store::CapabilityDecl {
            id: READ_CAPABILITY.to_string(),
            description: "Humans, on their own principal scope only. Grants: start_conversation, get_turn_response, list_conversations, get_conversation.".to_string(),
        },
    );
    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn private_paths(value: &serde_json::Value, path: &str, out: &mut Vec<String>) {
        if let Some(object) = value.as_object() {
            if object.get("x-opdbus-private") == Some(&serde_json::Value::Bool(true)) {
                out.push(path.to_string());
            }
            for (key, child) in object {
                private_paths(child, &format!("{path}/{key}"), out);
            }
        } else if let Some(items) = value.as_array() {
            for (index, child) in items.iter().enumerate() {
                private_paths(child, &format!("{path}/{index}"), out);
            }
        }
    }

    fn marked_properties(value: &serde_json::Value) -> HashSet<String> {
        let mut paths = Vec::new();
        private_paths(value, "", &mut paths);
        paths
            .iter()
            .filter_map(|path| path.rsplit('/').next().map(str::to_string))
            .collect()
    }

    #[test]
    fn schema_declares_every_method_with_typed_io_capability_and_valid_unique_subid() {
        let schema = ChatResponsePlugin::new().schema().unwrap();
        assert_eq!(schema.name, PLUGIN_ID);
        assert_eq!(schema.version, PLUGIN_VERSION);
        assert_eq!(schema.methods.len(), METHODS.len());
        let mut subids = HashSet::new();
        for (name, subid, mutation) in METHODS {
            let declaration = &schema.methods[name];
            assert_eq!(declaration.subid, subid);
            assert!(subids.insert(subid), "duplicate subid {subid}");
            assert!(
                crate::state_plugins::common::oscal::validate_subid(subid).is_ok(),
                "invalid subid {subid}"
            );
            assert_eq!(
                declaration.side_effect,
                if mutation {
                    op_state_store::SideEffect::Mutation
                } else {
                    op_state_store::SideEffect::Read
                }
            );
            assert_eq!(declaration.idempotent, !mutation);
            let capability = declaration.required_capability.as_deref().unwrap();
            assert!(schema.capabilities.contains_key(capability));
            // Typed input and output: no AckOutput, no success flag.
            let args = serde_json::to_value(&declaration.args).unwrap();
            let returns = serde_json::to_value(declaration.returns.as_ref().unwrap()).unwrap();
            assert_eq!(args["type"], "object", "{name} input is an object");
            assert_eq!(returns["type"], "object", "{name} output is an object");
            assert_eq!(
                args["additionalProperties"],
                serde_json::json!(false),
                "{name} input denies unknown fields"
            );
            assert!(returns["properties"].get("success").is_none());
            assert!(!returns["properties"].as_object().unwrap().is_empty());
        }
        for method in ["respond_to_user", "request_clarification", "cannot_perform"] {
            assert_eq!(
                schema.methods[method].required_capability.as_deref(),
                Some(RESPOND_CAPABILITY)
            );
        }
        for method in [
            "start_conversation",
            "get_turn_response",
            "list_conversations",
            "get_conversation",
        ] {
            assert_eq!(
                schema.methods[method].required_capability.as_deref(),
                Some(READ_CAPABILITY)
            );
        }
        let signal = schema
            .signals
            .iter()
            .find(|signal| signal.name == RESPONSE_COMMITTED_SIGNAL)
            .unwrap();
        assert_eq!(signal.subid, RESPONSE_COMMITTED_SUBID);
        assert!(crate::state_plugins::common::oscal::validate_subid(&signal.subid).is_ok());
        assert!(!subids.contains(signal.subid.as_str()));
        assert!(
            crate::state_plugins::common::oscal::validate_subid(&schema.subids["__schema__"])
                .is_ok()
        );
        // The respond output carries canvas_schema_hash; the others do not.
        let respond = serde_json::to_value(schema.methods["respond_to_user"].returns.as_ref())
            .unwrap()
            .to_string();
        assert!(respond.contains("canvas_schema_hash"));
        for method in ["request_clarification", "cannot_perform"] {
            let returns = serde_json::to_value(schema.methods[method].returns.as_ref())
                .unwrap()
                .to_string();
            assert!(!returns.contains("canvas_schema_hash"), "{method}");
            for field in ["response_id", "conversation_id", "turn_index", "turn_hash"] {
                assert!(returns.contains(field), "{method} returns {field}");
            }
        }
    }

    #[test]
    fn every_content_field_carries_the_private_marker() {
        let schema = ChatResponsePlugin::new().schema().unwrap();
        let expectations: [(&str, &[&str]); 4] = [
            ("start_conversation", &["title"]),
            ("respond_to_user", &["message", "canvas"]),
            (
                "request_clarification",
                &["question", "context", "options", "required_fields"],
            ),
            ("cannot_perform", &["reason", "alternatives"]),
        ];
        for (method, fields) in expectations {
            let args = serde_json::to_value(&schema.methods[method].args).unwrap();
            let marked = marked_properties(&args);
            for field in fields {
                assert!(marked.contains(*field), "{method}.{field} is not private");
            }
            // Routing and presentation fields stay public.
            for public in ["turn_id", "message_type", "category", "related_actions"] {
                assert!(!marked.contains(public), "{method}.{public} marked private");
            }
        }
        // The committed records mark the same content fields.
        let turn = serde_json::to_value(schemars::schema_for!(ChatTurn)).unwrap();
        let marked = marked_properties(&turn);
        for field in [
            "content",
            "message",
            "canvas",
            "question",
            "context",
            "options",
            "required_fields",
            "reason",
            "alternatives",
        ] {
            assert!(marked.contains(field), "ChatTurn {field} is not private");
        }
        for public in [
            "conversation_id",
            "turn_index",
            "prev_turn_hash",
            "turn_hash",
            "content_hash",
        ] {
            assert!(
                !marked.contains(public),
                "ChatTurn {public} must stay public"
            );
        }
        let summary = serde_json::to_value(schemars::schema_for!(ConversationSummary)).unwrap();
        assert!(marked_properties(&summary).contains("title"));
        // The signal is content-free: nothing in it is private content.
        let signal = serde_json::to_value(schemars::schema_for!(ResponseCommittedSignal)).unwrap();
        assert!(marked_properties(&signal).is_empty());
        for content in [
            "message", "question", "reason", "canvas", "title", "content",
        ] {
            assert!(
                signal["properties"].get(content).is_none(),
                "signal carries {content}"
            );
        }
    }

    #[test]
    fn state_schema_has_no_public_projection() {
        let schema = ChatResponsePlugin::new().schema().unwrap();
        assert!(schema.public_state.paths.is_empty());
        let state = simd_json::serde::to_owned_value(ChatResponseState::default()).unwrap();
        assert!(schema.validate(&state).valid);
        assert!(schema.project_public_state(&state).unwrap().is_none());
    }

    #[tokio::test]
    async fn generic_lifecycle_hooks_refuse_state_mutation() {
        let plugin = ChatResponsePlugin::new();
        let state = simd_json::json!({});
        assert!(plugin.calculate_diff(&state, &state).await.is_err());
        assert!(plugin.verify_state(&state).await.is_err());
        assert!(plugin.create_checkpoint().await.is_err());
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_ID, |_ctx| std::sync::Arc::new(ChatResponsePlugin::new()))
}
