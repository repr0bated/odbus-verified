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
}

/// Publish on the responder's existing authenticated owner topic. Failure is
/// returned after input commit and must never trigger a submit replay.
pub(crate) async fn publish_responder_wake(wake: ResponderWake,
    responder: &op_identity::sealed_id::SealedId) -> anyhow::Result<()> {
    let owner_key = crate::runtime_mqtt::owner_key_for_claims(responder)?;
    op_plugins::state_plugins::emqx::dispatch_emqx_method("publish_responder_wake",
        &serde_json::json!({
            "responder_owner_key": owner_key,
            "request_id": wake.request_id,
            "owner_key": wake.owner_key,
            "owner_principal_id": wake.owner_principal_id,
            "conversation_id": wake.conversation_id,
        })).await?;
    Ok(())
}

/// Build a ResponderWake for a chat turn.
pub(crate) fn chat_turn_wake(
    turn: &crate::chat_response_turns::PendingTurn,
    _message_preview: Option<&str>,
) -> ResponderWake {
    ResponderWake {
        request_id: turn.turn_id().to_string(),
        owner_key: {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(turn.owner.principal_id.as_bytes()))
        },
        owner_principal_id: turn.owner.principal_id.clone(),
        conversation_id: Some(turn.conversation_id().to_string()),
    }
}


#[cfg(test)]
mod tests {
    use super::*;


    #[test]
    fn responder_wake_serializes() {
        let wake = ResponderWake {
            request_id: "turn-123".into(),
            owner_key: "ab".repeat(32),
            owner_principal_id: "human-principal".into(),
            conversation_id: Some("conv-456".into()),
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
        };
        let json_minimal = serde_json::to_string(&wake_minimal).unwrap();
        assert!(!json_minimal.contains("conversation_id"));
        assert!(!json_minimal.contains("preview"));
    }
}
