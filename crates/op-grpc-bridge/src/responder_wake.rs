//! Responder wake: notifies designated responders of pending requests.
//!
//! When a human opens a chat turn (or any request via RequestRegistry),
//! the designated responder principal needs to be notified so it can claim
//! and fulfill the request. This module provides that notification channel.
//!
//! The responder wake is separate from the human's doorbell wake:
//! - Human doorbell: `opdbus/wake/<sha256(principal)>` — notifies the human
//!   that their scope has new content (e.g., an agent's response)
//! - Responder wake: `opdbus/responder-wake/<sha256(responder_principal)>` —
//!   notifies an agent that a request awaits their response
//!
//! Payload includes the request_id so the responder can claim it.

use serde::{Deserialize, Serialize};

/// A responder wake notification: tells the designated responder that a
/// request awaits their response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ResponderWake {
    /// The request ID the responder should claim.
    pub request_id: String,
    /// Owner key of the requester (for routing the response back).
    pub owner_key: String,
    /// Principal ID of the requester (for response scope).
    pub owner_principal_id: String,
    /// Conversation ID (for chat turns).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// Brief preview of the request content (for logging/debugging).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

/// Publish a responder wake notification via EMQX.
///
/// This is best-effort and off the critical path — a failed publish only
/// delays the responder's discovery of the pending request. The responder
/// can also poll via `list_pending_for_responder()`.
pub(crate) async fn publish_responder_wake(wake: ResponderWake, responder_principal_id: &str) {
    let args = serde_json::json!({
        "responder_principal_id": responder_principal_id,
        "request_id": wake.request_id,
        "owner_key": wake.owner_key,
        "owner_principal_id": wake.owner_principal_id,
        "conversation_id": wake.conversation_id,
        "preview": wake.preview,
    });

    if let Err(error) =
        op_plugins::state_plugins::emqx::dispatch_emqx_method("publish_responder_wake", &args).await
    {
        tracing::debug!(
            %error,
            "responder wake not published"
        );
    }
}

/// Build a ResponderWake for a chat turn.
pub(crate) fn chat_turn_wake(
    turn: &crate::chat_response_turns::PendingTurn,
    message_preview: Option<&str>,
) -> ResponderWake {
    ResponderWake {
        request_id: turn.turn_id().to_string(),
        owner_key: crate::chat_response_state::owner_key(&turn.owner.principal_id),
        owner_principal_id: turn.owner.principal_id.clone(),
        conversation_id: Some(turn.conversation_id().to_string()),
        preview: message_preview.map(|s| truncate_preview(s, 100)),
    }
}

/// Truncate a preview string to a maximum length, adding ellipsis if needed.
fn truncate_preview(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_truncation() {
        assert_eq!(truncate_preview("short", 100), "short");
        let long = "a".repeat(150);
        let truncated = truncate_preview(&long, 100);
        assert!(truncated.len() <= 100);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn responder_wake_serializes() {
        let wake = ResponderWake {
            request_id: "turn-123".into(),
            owner_key: "ab".repeat(32),
            owner_principal_id: "human-principal".into(),
            conversation_id: Some("conv-456".into()),
            preview: Some("What is the status?".into()),
        };
        let json = serde_json::to_string(&wake).unwrap();
        assert!(json.contains("turn-123"));
        assert!(json.contains("conv-456"));

        // Without optional fields
        let wake_minimal = ResponderWake {
            request_id: "req-789".into(),
            owner_key: "cd".repeat(32),
            owner_principal_id: "other-principal".into(),
            conversation_id: None,
            preview: None,
        };
        let json_minimal = serde_json::to_string(&wake_minimal).unwrap();
        assert!(!json_minimal.contains("conversation_id"));
        assert!(!json_minimal.contains("preview"));
    }
}
