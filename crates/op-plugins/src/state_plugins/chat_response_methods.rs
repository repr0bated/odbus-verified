//! Typed method contracts and pure rules for `chat_response`: input bounds,
//! the content digest, and the per-conversation turn hash chain.
//!
//! Nothing here reads identity, stores rows or mints ids. The bridge mints
//! `conversation_id`/`turn_id`, verifies the caller, and commits; it builds
//! every record through these functions so one set of rules applies.

use super::chat_response::{
    ChatTurn, ConversationSummary, MessageType, RefusalCategory, ResponseContent,
};
use anyhow::{ensure, Context, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Domain tags inside the hashed canonical arrays.
pub const GENESIS_DOMAIN: &str = "opdbus.chat_response.genesis.v1";
pub const TURN_DOMAIN: &str = "opdbus.chat_response.turn.v1";

pub const MAX_TITLE_BYTES: usize = 256;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
pub const MAX_QUESTION_BYTES: usize = 64 * 1024;
pub const MAX_CONTEXT_BYTES: usize = 64 * 1024;
pub const MAX_REASON_BYTES: usize = 64 * 1024;
pub const MAX_LIST_ITEMS: usize = 64;
pub const MAX_LIST_ITEM_BYTES: usize = 1024;
pub const MAX_ACTION_BYTES: usize = 256;
/// Upper bound before the sealed Canvas profile applies its own (smaller) limits.
pub const MAX_CANVAS_BYTES: usize = 1024 * 1024;
/// Serialized committed turn, content included.
pub const MAX_TURN_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_CONVERSATION_BYTES: usize = 64 * 1024;
const MAX_IDENTITY_BYTES: usize = 512;
const MAX_TIMESTAMP_BYTES: usize = 64;

// ── Method inputs and outputs ────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartConversationInput {
    #[serde(default)]
    #[schemars(
        description = "Optional conversation title [PII]",
        extend("x-opdbus-private" = true)
    )]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StartConversationOutput {
    /// Bridge-minted UUIDv7, bound to the caller's principal.
    pub conversation_id: String,
    /// RFC3339 start time (part of the conversation genesis hash).
    pub started_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RespondToUserInput {
    #[schemars(description = "Bridge-minted turn this response answers")]
    pub turn_id: String,
    #[schemars(
        description = "The message shown to the user [PII]",
        extend("x-opdbus-private" = true)
    )]
    pub message: String,
    #[schemars(description = "How the message is presented")]
    pub message_type: MessageType,
    #[serde(default)]
    #[schemars(description = "Tool or method names called to produce this response")]
    pub related_actions: Vec<String>,
    #[serde(default)]
    #[schemars(
        description = "Optional json-render Canvas spec, validated against the sealed json_render Canvas profile; an invalid canvas rejects the whole response [PII]",
        extend("x-opdbus-private" = true)
    )]
    pub canvas: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RespondToUserOutput {
    pub response_id: String,
    pub conversation_id: String,
    pub turn_index: u64,
    pub turn_hash: String,
    /// Durable revision of the human's scope after this commit.
    pub revision: u64,
    /// Sealed json_render schema hash the canvas was validated against.
    pub canvas_schema_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestClarificationInput {
    #[schemars(description = "Bridge-minted turn this response answers")]
    pub turn_id: String,
    #[schemars(
        description = "The clarifying question [PII]",
        extend("x-opdbus-private" = true)
    )]
    pub question: String,
    #[serde(default)]
    #[schemars(
        description = "Why the clarification is needed [PII]",
        extend("x-opdbus-private" = true)
    )]
    pub context: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "Options the user may choose from [PII]",
        extend("x-opdbus-private" = true)
    )]
    pub options: Vec<String>,
    #[serde(default)]
    #[schemars(
        description = "Information the agent is missing [PII]",
        extend("x-opdbus-private" = true)
    )]
    pub required_fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CannotPerformInput {
    #[schemars(description = "Bridge-minted turn this response answers")]
    pub turn_id: String,
    #[schemars(
        description = "Why the request cannot be performed [PII]",
        extend("x-opdbus-private" = true)
    )]
    pub reason: String,
    #[schemars(description = "Refusal category")]
    pub category: RefusalCategory,
    #[serde(default)]
    #[schemars(
        description = "Alternatives the user could take [PII]",
        extend("x-opdbus-private" = true)
    )]
    pub alternatives: Vec<String>,
}

/// Receipt of `request_clarification` and `cannot_perform`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResponseCommitOutput {
    pub response_id: String,
    pub conversation_id: String,
    pub turn_index: u64,
    pub turn_hash: String,
    /// Durable revision of the human's scope after this commit.
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetTurnResponseInput {
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetTurnResponseOutput {
    /// Absent when no response is committed for this turn in the caller's scope.
    pub turn: Option<ChatTurn>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListConversationsInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListConversationsOutput {
    /// Most recently active first, bounded.
    pub conversations: Vec<ConversationSummary>,
    pub conversation_count: u64,
    pub conversations_truncated: bool,
    /// Durable revision of the caller's scope.
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetConversationInput {
    pub conversation_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetConversationOutput {
    pub conversation: ConversationSummary,
    /// Newest bounded window in ascending `turn_index` order.
    pub turns: Vec<ChatTurn>,
    /// True when older turns are omitted; `first_turn_index` is the oldest returned.
    pub turns_truncated: bool,
    pub first_turn_index: u64,
}

// ── Canonical hashing ────────────────────────────────────────────────────────

/// JSON with every object's keys sorted, independent of map ordering features.
pub fn canonical_json(value: &serde_json::Value) -> Vec<u8> {
    fn sorted(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(object) => {
                let mut keys: Vec<&String> = object.keys().collect();
                keys.sort();
                let mut map = serde_json::Map::new();
                for key in keys {
                    map.insert(key.clone(), sorted(&object[key]));
                }
                serde_json::Value::Object(map)
            }
            serde_json::Value::Array(items) => {
                serde_json::Value::Array(items.iter().map(sorted).collect())
            }
            other => other.clone(),
        }
    }
    // Serializing a freshly built map inserts keys in sorted order whether
    // the map is ordered by key or by insertion.
    serde_json::to_vec(&sorted(value)).expect("JSON values serialize")
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// `sha256(canonical([GENESIS_DOMAIN, conversation_id, principal_id, started_at]))`.
pub fn conversation_genesis_hash(
    conversation_id: &str,
    principal_id: &str,
    started_at: &str,
) -> String {
    sha256_hex(&canonical_json(&serde_json::json!([
        GENESIS_DOMAIN,
        conversation_id,
        principal_id,
        started_at
    ])))
}

/// `sha256(canonical(content))` — commits to private content publicly.
pub fn content_hash(content: &ResponseContent) -> Result<String> {
    Ok(sha256_hex(&canonical_json(&serde_json::to_value(content)?)))
}

/// `sha256(canonical([TURN_DOMAIN, prev_turn_hash, conversation_id,
/// turn_index, turn_id, content_hash, responder_principal_id]))`.
pub fn turn_hash(
    prev_turn_hash: &str,
    conversation_id: &str,
    turn_index: u64,
    turn_id: &str,
    content_hash: &str,
    responder_principal_id: &str,
) -> String {
    sha256_hex(&canonical_json(&serde_json::json!([
        TURN_DOMAIN,
        prev_turn_hash,
        conversation_id,
        turn_index,
        turn_id,
        content_hash,
        responder_principal_id
    ])))
}

// ── Field rules ──────────────────────────────────────────────────────────────

/// Canonical lowercase hyphenated UUID, as the bridge mints them.
pub fn validate_uuid(field: &str, value: &str) -> Result<()> {
    let parsed = uuid::Uuid::parse_str(value).with_context(|| format!("{field} is not a UUID"))?;
    ensure!(
        parsed.hyphenated().to_string() == value,
        "{field} must be a canonical lowercase hyphenated UUID"
    );
    Ok(())
}

pub fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn validate_hash(field: &str, value: &str) -> Result<()> {
    ensure!(
        is_sha256_hex(value),
        "{field} must be a 64-character lowercase SHA-256"
    );
    Ok(())
}

fn validate_identity(field: &str, value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= MAX_IDENTITY_BYTES
            && value.trim() == value
            && !value.chars().any(char::is_control),
        "{field} must be 1..={MAX_IDENTITY_BYTES} bytes without control characters"
    );
    Ok(())
}

fn validate_timestamp(field: &str, value: &str) -> Result<()> {
    ensure!(
        value.len() <= MAX_TIMESTAMP_BYTES,
        "{field} exceeds {MAX_TIMESTAMP_BYTES} bytes"
    );
    chrono::DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("{field} is not RFC3339"))?;
    Ok(())
}

/// Free text: bounded, non-empty after trimming, no NUL or other control
/// characters except newline, carriage return and tab.
fn validate_text(field: &str, value: &str, maximum: usize) -> Result<()> {
    ensure!(
        !value.trim().is_empty(),
        "{field} must not be empty or only whitespace"
    );
    ensure!(value.len() <= maximum, "{field} exceeds {maximum} bytes");
    ensure!(
        !value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t')),
        "{field} contains control characters"
    );
    Ok(())
}

fn validate_items(field: &str, values: &[String], item_bytes: usize) -> Result<()> {
    ensure!(
        values.len() <= MAX_LIST_ITEMS,
        "{field} holds at most {MAX_LIST_ITEMS} items"
    );
    for value in values {
        ensure!(
            !value.trim().is_empty()
                && value.len() <= item_bytes
                && !value.chars().any(char::is_control),
            "{field} items must be 1..={item_bytes} bytes without control characters"
        );
    }
    Ok(())
}

pub fn validate_title(title: Option<&str>) -> Result<()> {
    if let Some(title) = title {
        ensure!(
            !title.trim().is_empty()
                && title.len() <= MAX_TITLE_BYTES
                && !title.chars().any(char::is_control),
            "title must be 1..={MAX_TITLE_BYTES} bytes without control characters"
        );
    }
    Ok(())
}

/// Bounds for the response content. The Canvas itself is checked by the
/// sealed json_render profile in the bridge, before any commit.
pub fn validate_content(content: &ResponseContent) -> Result<()> {
    match content {
        ResponseContent::UserResponse {
            message,
            related_actions,
            canvas,
            ..
        } => {
            validate_text("message", message, MAX_MESSAGE_BYTES)?;
            validate_items("related_actions", related_actions, MAX_ACTION_BYTES)?;
            if let Some(canvas) = canvas {
                ensure!(
                    serde_json::to_vec(canvas)?.len() <= MAX_CANVAS_BYTES,
                    "canvas exceeds {MAX_CANVAS_BYTES} bytes"
                );
            }
        }
        ResponseContent::Clarification {
            question,
            context,
            options,
            required_fields,
        } => {
            validate_text("question", question, MAX_QUESTION_BYTES)?;
            if let Some(context) = context {
                validate_text("context", context, MAX_CONTEXT_BYTES)?;
            }
            validate_items("options", options, MAX_LIST_ITEM_BYTES)?;
            validate_items("required_fields", required_fields, MAX_LIST_ITEM_BYTES)?;
        }
        ResponseContent::Refusal {
            reason,
            alternatives,
            ..
        } => {
            validate_text("reason", reason, MAX_REASON_BYTES)?;
            validate_items("alternatives", alternatives, MAX_LIST_ITEM_BYTES)?;
        }
        ResponseContent::HumanMessage { message } => {
            validate_text("message", message, MAX_MESSAGE_BYTES)?;
        }
    }
    Ok(())
}

impl ResponseContent {
    /// Whether this is the human's side of the conversation.
    pub fn is_human_message(&self) -> bool {
        matches!(self, Self::HumanMessage { .. })
    }
}

impl RespondToUserInput {
    /// The turn id and the content this call would commit.
    pub fn into_content(self) -> Result<(String, ResponseContent)> {
        let content = ResponseContent::UserResponse {
            message: self.message,
            message_type: self.message_type,
            related_actions: self.related_actions,
            canvas: self.canvas,
        };
        validate_uuid("turn_id", &self.turn_id)?;
        validate_content(&content)?;
        Ok((self.turn_id, content))
    }
}

impl RequestClarificationInput {
    pub fn into_content(self) -> Result<(String, ResponseContent)> {
        let content = ResponseContent::Clarification {
            question: self.question,
            context: self.context,
            options: self.options,
            required_fields: self.required_fields,
        };
        validate_uuid("turn_id", &self.turn_id)?;
        validate_content(&content)?;
        Ok((self.turn_id, content))
    }
}

impl CannotPerformInput {
    pub fn into_content(self) -> Result<(String, ResponseContent)> {
        let content = ResponseContent::Refusal {
            reason: self.reason,
            category: self.category,
            alternatives: self.alternatives,
        };
        validate_uuid("turn_id", &self.turn_id)?;
        validate_content(&content)?;
        Ok((self.turn_id, content))
    }
}

impl ResponseContent {
    pub fn canvas(&self) -> Option<&serde_json::Map<String, serde_json::Value>> {
        match self {
            Self::UserResponse { canvas, .. } => canvas.as_ref(),
            _ => None,
        }
    }
}

// ── Records ──────────────────────────────────────────────────────────────────

/// Everything the bridge knows when a conversation starts.
pub struct NewConversation {
    pub principal_id: String,
    pub session_id: String,
    pub session_genesis: String,
    pub conversation_id: String,
    pub title: Option<String>,
    pub started_at: String,
}

pub fn build_conversation(input: NewConversation) -> Result<ConversationSummary> {
    let genesis_hash = conversation_genesis_hash(
        &input.conversation_id,
        &input.principal_id,
        &input.started_at,
    );
    let summary = ConversationSummary {
        principal_id: input.principal_id,
        conversation_id: input.conversation_id,
        title: input.title,
        updated_at: input.started_at.clone(),
        started_at: input.started_at,
        session_id: input.session_id,
        session_genesis: input.session_genesis,
        head_turn_hash: genesis_hash.clone(),
        genesis_hash,
        turn_count: 0,
    };
    validate_conversation(&summary)?;
    Ok(summary)
}

pub fn validate_conversation(summary: &ConversationSummary) -> Result<()> {
    validate_identity("principal_id", &summary.principal_id)?;
    validate_identity("session_id", &summary.session_id)?;
    validate_identity("session_genesis", &summary.session_genesis)?;
    validate_uuid("conversation_id", &summary.conversation_id)?;
    validate_title(summary.title.as_deref())?;
    validate_timestamp("started_at", &summary.started_at)?;
    validate_timestamp("updated_at", &summary.updated_at)?;
    validate_hash("genesis_hash", &summary.genesis_hash)?;
    validate_hash("head_turn_hash", &summary.head_turn_hash)?;
    ensure!(
        summary.genesis_hash
            == conversation_genesis_hash(
                &summary.conversation_id,
                &summary.principal_id,
                &summary.started_at
            ),
        "conversation genesis_hash does not match its conversation"
    );
    ensure!(
        summary.turn_count != 0 || summary.head_turn_hash == summary.genesis_hash,
        "an empty conversation's head must be its genesis"
    );
    ensure!(
        serde_json::to_vec(summary)?.len() <= MAX_CONVERSATION_BYTES,
        "conversation exceeds {MAX_CONVERSATION_BYTES} bytes"
    );
    Ok(())
}

/// Everything the bridge knows when a response commits. `prev_turn_hash`
/// and `turn_index` come from the durable head inside the commit.
pub struct NewTurn {
    pub principal_id: String,
    pub session_id: String,
    pub session_genesis: String,
    pub conversation_id: String,
    pub turn_id: String,
    pub turn_index: u64,
    pub prev_turn_hash: String,
    pub response_id: String,
    pub responder_principal_id: String,
    pub opened_at: String,
    pub committed_at: String,
    pub request_event_id: u64,
    pub request_event_hash: String,
    pub canvas_schema_hash: Option<String>,
    pub content: ResponseContent,
}

pub fn build_turn(input: NewTurn) -> Result<ChatTurn> {
    let content_hash = content_hash(&input.content)?;
    let turn_hash = turn_hash(
        &input.prev_turn_hash,
        &input.conversation_id,
        input.turn_index,
        &input.turn_id,
        &content_hash,
        &input.responder_principal_id,
    );
    let turn = ChatTurn {
        principal_id: input.principal_id,
        session_id: input.session_id,
        session_genesis: input.session_genesis,
        conversation_id: input.conversation_id,
        turn_id: input.turn_id,
        turn_index: input.turn_index,
        response_id: input.response_id,
        responder_principal_id: input.responder_principal_id,
        opened_at: input.opened_at,
        committed_at: input.committed_at,
        prev_turn_hash: input.prev_turn_hash,
        turn_hash,
        content_hash,
        request_event_id: input.request_event_id,
        request_event_hash: input.request_event_hash,
        canvas_schema_hash: input.canvas_schema_hash,
        content: input.content,
    };
    validate_turn(&turn)?;
    Ok(turn)
}

/// Structural and digest checks of one committed turn.
pub fn validate_turn(turn: &ChatTurn) -> Result<()> {
    validate_identity("principal_id", &turn.principal_id)?;
    validate_identity("session_id", &turn.session_id)?;
    validate_identity("session_genesis", &turn.session_genesis)?;
    validate_identity("responder_principal_id", &turn.responder_principal_id)?;
    validate_uuid("conversation_id", &turn.conversation_id)?;
    validate_uuid("turn_id", &turn.turn_id)?;
    validate_uuid("response_id", &turn.response_id)?;
    validate_timestamp("opened_at", &turn.opened_at)?;
    validate_timestamp("committed_at", &turn.committed_at)?;
    validate_hash("prev_turn_hash", &turn.prev_turn_hash)?;
    validate_hash("turn_hash", &turn.turn_hash)?;
    validate_hash("content_hash", &turn.content_hash)?;
    ensure!(
        !turn.request_event_hash.is_empty()
            && turn.request_event_hash.len() <= 128
            && turn
                .request_event_hash
                .bytes()
                .all(|b| b.is_ascii_hexdigit()),
        "request_event_hash must be a hexadecimal chain hash"
    );
    match (&turn.canvas_schema_hash, turn.content.canvas()) {
        (Some(hash), Some(_)) => validate_hash("canvas_schema_hash", hash)?,
        (None, None) => {}
        _ => anyhow::bail!("canvas_schema_hash must be present exactly when a canvas is"),
    }
    validate_content(&turn.content)?;
    // A human message is authored by the human whose scope holds it; a
    // response never is, so neither side can pass as the other.
    ensure!(
        turn.content.is_human_message() == (turn.responder_principal_id == turn.principal_id),
        "a human message must be authored by its own principal, and a response by an agent"
    );
    ensure!(
        turn.content_hash == content_hash(&turn.content)?,
        "turn content_hash does not match its content"
    );
    ensure!(
        turn.turn_hash
            == turn_hash(
                &turn.prev_turn_hash,
                &turn.conversation_id,
                turn.turn_index,
                &turn.turn_id,
                &turn.content_hash,
                &turn.responder_principal_id,
            ),
        "turn_hash does not match the turn's chain fields"
    );
    ensure!(
        serde_json::to_vec(turn)?.len() <= MAX_TURN_BYTES,
        "turn exceeds {MAX_TURN_BYTES} bytes"
    );
    Ok(())
}

/// Verify a contiguous run of turns against its conversation, from the
/// public fields only: indices ascend without gaps from `first_index`, each
/// `prev_turn_hash` is the previous `turn_hash` (the genesis for turn 0), and
/// when the run reaches the head it ends at `head_turn_hash`.
pub fn verify_turn_chain(
    conversation: &ConversationSummary,
    turns: &[ChatTurn],
    first_index: u64,
) -> Result<()> {
    let mut expected_prev: Option<&str> =
        (first_index == 0).then_some(conversation.genesis_hash.as_str());
    for (offset, turn) in turns.iter().enumerate() {
        ensure!(
            turn.conversation_id == conversation.conversation_id
                && turn.principal_id == conversation.principal_id,
            "turn belongs to another conversation or principal"
        );
        ensure!(
            turn.turn_index == first_index + offset as u64,
            "turn indices are not contiguous"
        );
        if let Some(prev) = expected_prev {
            ensure!(
                turn.prev_turn_hash == prev,
                "turn {} does not chain to its predecessor",
                turn.turn_index
            );
        }
        ensure!(
            turn.turn_hash
                == turn_hash(
                    &turn.prev_turn_hash,
                    &turn.conversation_id,
                    turn.turn_index,
                    &turn.turn_id,
                    &turn.content_hash,
                    &turn.responder_principal_id,
                ),
            "turn {} hash does not match its public fields",
            turn.turn_index
        );
        expected_prev = Some(turn.turn_hash.as_str());
    }
    if first_index + turns.len() as u64 == conversation.turn_count {
        ensure!(
            expected_prev == Some(conversation.head_turn_hash.as_str())
                || (turns.is_empty() && first_index != 0),
            "turn chain does not end at the conversation head"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRINCIPAL: &str = "1f0b6a52-6cb2-4a5e-8f0c-0123456789ab";
    const AGENT: &str = "1262e982-14c1-44d4-0771-7ce74213211b";

    fn uuid() -> String {
        uuid::Uuid::now_v7().to_string()
    }

    fn conversation() -> ConversationSummary {
        build_conversation(NewConversation {
            principal_id: PRINCIPAL.into(),
            session_id: "session-a".into(),
            session_genesis: "ab".repeat(32),
            conversation_id: uuid(),
            title: Some("private title".into()),
            started_at: "2026-09-28T10:00:00.000Z".into(),
        })
        .unwrap()
    }

    fn message(text: &str) -> ResponseContent {
        ResponseContent::UserResponse {
            message: text.into(),
            message_type: MessageType::Info,
            related_actions: vec!["workflows.query_workflows".into()],
            canvas: None,
        }
    }

    fn next_turn(
        conversation: &mut ConversationSummary,
        session: &str,
        content: ResponseContent,
    ) -> ChatTurn {
        let turn = build_turn(NewTurn {
            principal_id: conversation.principal_id.clone(),
            session_id: session.into(),
            session_genesis: "cd".repeat(32),
            conversation_id: conversation.conversation_id.clone(),
            turn_id: uuid(),
            turn_index: conversation.turn_count,
            prev_turn_hash: conversation.head_turn_hash.clone(),
            response_id: uuid(),
            responder_principal_id: AGENT.into(),
            opened_at: "2026-09-28T10:00:01.000Z".into(),
            committed_at: "2026-09-28T10:00:02.000Z".into(),
            request_event_id: 42,
            request_event_hash: "ef".repeat(32),
            canvas_schema_hash: None,
            content,
        })
        .unwrap();
        conversation.turn_count += 1;
        conversation.head_turn_hash = turn.turn_hash.clone();
        turn
    }

    #[test]
    fn canonical_json_sorts_keys_at_every_depth() {
        let left = serde_json::json!({"b": 1, "a": {"d": [ {"z": 1, "y": 2} ], "c": null}});
        assert_eq!(
            String::from_utf8(canonical_json(&left)).unwrap(),
            r#"{"a":{"c":null,"d":[{"y":2,"z":1}]},"b":1}"#
        );
    }

    #[test]
    fn genesis_and_turn_hashes_chain_and_detect_tampering() {
        let mut conversation = conversation();
        assert_eq!(conversation.head_turn_hash, conversation.genesis_hash);
        let first = next_turn(&mut conversation, "session-a", message("one"));
        // A later login of the same principal continues the same chain.
        let second = next_turn(&mut conversation, "session-b", message("two"));
        assert_eq!(first.turn_index, 0);
        assert_eq!(first.prev_turn_hash, conversation.genesis_hash);
        assert_eq!(second.turn_index, 1);
        assert_eq!(second.prev_turn_hash, first.turn_hash);
        assert_ne!(first.session_id, second.session_id);
        verify_turn_chain(&conversation, &[first.clone(), second.clone()], 0).unwrap();
        verify_turn_chain(&conversation, std::slice::from_ref(&second), 1).unwrap();

        // Omission, reordering and content substitution are all detected.
        assert!(verify_turn_chain(&conversation, std::slice::from_ref(&second), 0).is_err());
        assert!(verify_turn_chain(&conversation, &[second.clone(), first.clone()], 0).is_err());
        let mut forged = second.clone();
        forged.content = message("forged");
        assert!(validate_turn(&forged).is_err());
        forged.content_hash = content_hash(&forged.content).unwrap();
        assert!(
            validate_turn(&forged).is_err(),
            "turn_hash binds content_hash"
        );
        assert!(verify_turn_chain(&conversation, &[first, forged], 0).is_err());
    }

    #[test]
    fn content_hash_is_independent_of_canvas_key_order() {
        let mut ordered = serde_json::Map::new();
        ordered.insert("root".into(), serde_json::json!("root"));
        ordered.insert("elements".into(), serde_json::json!({"b": 1, "a": 2}));
        let mut reversed = serde_json::Map::new();
        reversed.insert("elements".into(), serde_json::json!({"a": 2, "b": 1}));
        reversed.insert("root".into(), serde_json::json!("root"));
        let with = |canvas| ResponseContent::UserResponse {
            message: "m".into(),
            message_type: MessageType::Success,
            related_actions: vec![],
            canvas: Some(canvas),
        };
        assert_eq!(
            content_hash(&with(ordered)).unwrap(),
            content_hash(&with(reversed)).unwrap()
        );
    }

    #[test]
    fn inputs_are_bounded_and_deny_unknown_fields() {
        let turn_id = uuid();
        assert!(
            serde_json::from_value::<RespondToUserInput>(serde_json::json!({
                "turn_id": turn_id, "message": "m", "message_type": "info", "success": true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<RespondToUserInput>(serde_json::json!({
                "turn_id": turn_id, "message": "m", "message_type": "loud"
            }))
            .is_err()
        );
        let valid: RespondToUserInput = serde_json::from_value(serde_json::json!({
            "turn_id": turn_id, "message": "hello\nworld", "message_type": "success"
        }))
        .unwrap();
        valid.into_content().unwrap();
        for bad in [
            serde_json::json!({"turn_id": "not-a-uuid", "message": "m", "message_type": "info"}),
            serde_json::json!({"turn_id": turn_id.to_uppercase(), "message": "m", "message_type": "info"}),
            serde_json::json!({"turn_id": turn_id, "message": "  ", "message_type": "info"}),
            serde_json::json!({"turn_id": turn_id, "message": "a\u{0}b", "message_type": "info"}),
            serde_json::json!({"turn_id": turn_id, "message": "m", "message_type": "info",
                "related_actions": vec!["x"; MAX_LIST_ITEMS + 1]}),
        ] {
            let input: RespondToUserInput = serde_json::from_value(bad).unwrap();
            assert!(input.into_content().is_err());
        }
        let refusal: CannotPerformInput = serde_json::from_value(serde_json::json!({
            "turn_id": turn_id, "reason": "r", "category": "policy_violation"
        }))
        .unwrap();
        assert!(matches!(
            refusal.into_content().unwrap().1,
            ResponseContent::Refusal {
                category: RefusalCategory::PolicyViolation,
                ..
            }
        ));
        let clarification: RequestClarificationInput = serde_json::from_value(serde_json::json!({
            "turn_id": turn_id, "question": "which bridge?", "options": ["br0", "br1"]
        }))
        .unwrap();
        clarification.into_content().unwrap();
        assert!(
            serde_json::from_value::<ListConversationsInput>(serde_json::json!({"limit": 5}))
                .is_err()
        );
        validate_title(Some(&"t".repeat(MAX_TITLE_BYTES + 1))).unwrap_err();
    }

    #[test]
    fn canvas_schema_hash_must_accompany_exactly_a_canvas() {
        let mut conversation = conversation();
        let mut turn = next_turn(&mut conversation, "session-a", message("plain"));
        turn.canvas_schema_hash = Some("aa".repeat(32));
        assert!(validate_turn(&turn).is_err());
    }
}
