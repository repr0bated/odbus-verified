//! Wire DTOs and size bounds shared by the threetched runtime chat clients.
//!
//! This module owns no PluginSchema, method declarations, capabilities, or
//! dispatch. Native method authority remains with the captured threetched
//! contract; callers retain their own admission and identity checks.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// Maximum serialized request bytes accepted by native runtime chat callers.
pub const MAX_CHAT_REQUEST_BYTES: usize = 1024 * 1024;
/// Maximum response bytes accepted from native runtime chat callers.
pub const MAX_CHAT_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// One chat turn carried between authenticated bridge flows and the runtime.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RuntimeChatMessage {
    /// OpenAI-compatible message role such as `system`, `user`, or `assistant`.
    pub role: String,
    /// Text content carried by this conversation turn.
    pub content: String,
}

/// Chat request shared by EMQX agent execution and cognitive runtime callers.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChatInput {
    /// Compatibility form for callers sending a single user turn.
    #[serde(default)]
    pub message: String,
    /// Full ordered conversation. When non-empty this takes precedence over `message`.
    #[serde(default)]
    pub messages: Vec<RuntimeChatMessage>,
    /// Provider id, route, or alias. Empty uses the caller's selected provider.
    #[serde(default)]
    pub provider: String,
    /// Model id or route hint. Empty uses the caller's selected model.
    #[serde(default)]
    pub model: String,
    /// Optional correlation label scoped by the verified principal/session; never identity.
    #[serde(default)]
    pub conversation_id: Option<String>,
}

/// Chat result shared by EMQX agent execution and cognitive runtime callers.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChatOutput {
    /// Assistant response text.
    pub content: String,
    /// Upstream provider selected when the A2A request was submitted.
    /// A2A does not report attribution for an upstream fallback.
    pub provider: String,
    /// Upstream model selected when the A2A request was submitted.
    pub model: String,
    /// Provider finish reason, or an empty string when none was supplied.
    pub finish_reason: String,
    /// Provider-specific token usage object.
    pub usage: JsonValue,
}

