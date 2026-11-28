//! Queue a turn identifier for the designated container's FUSE responder.
//! The datagram carries no message, selection or identity credential.

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

/// Notify the provisioned responder after the human turn is committed.
pub(crate) async fn publish_responder_wake(wake: ResponderWake,
    responder: &op_identity::sealed_id::SealedId) -> anyhow::Result<()> {
    use anyhow::{Context, ensure};
    let principal = std::env::var("OP_CONTROL_PLANE_CHATBOT_PRINCIPAL_ID")
        .context("designated responder is not configured")?;
    ensure!(responder.principal_id == principal, "responder is not the provisioned service identity");
    let turn = uuid::Uuid::parse_str(&wake.request_id)?;
    ensure!(turn.to_string() == wake.request_id, "invalid responder turn identifier");

    let socket = tokio::net::UnixDatagram::unbound()?;
    let bytes = wake.request_id.as_bytes();
    let sent = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        socket.send_to(bytes, "/run/opdbus/lungs/chat-responder.sock"),
    ).await.context("responder queue deadline")?
        .context("container responder queue is unavailable")?;
    ensure!(sent == bytes.len(), "responder queue accepted a partial identifier");
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
