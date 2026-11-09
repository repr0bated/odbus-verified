//! Confirmation_Gate (spec task 8.1): mutations are held server-side
//! until a separate approve request carries a matching D-17 digest.
//! `confirm` is one synchronous compare-and-set; a failed confirm
//! leaves the record unchanged. The executor receives only the held
//! `input_bytes`; client digests serve only for comparison.

use crate::types::{ChatSessionId, PendingToolCall, PrincipalId, ToolCallStatus};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub const CONFIRM_TTL_SECS: u64 = 300;
pub const DIGEST_DOMAIN: &str = "identity-chat-confirm/1";

/// Lowercase hex SHA-256 over the D-17 tuple.
pub fn digest(
    principal_id: &PrincipalId,
    chat_session_id: &ChatSessionId,
    tool_call_id: &str,
    plugin: &str,
    method: &str,
    input_text: &str,
) -> String {
    // D-17: SHA-256 over the JSON array text
    // ["identity-chat-confirm/1", principalId, chatSessionId,
    //  toolCallId, plugin, method, inputText]
    let array = serde_json::json!([
        DIGEST_DOMAIN,
        principal_id.as_str(),
        chat_session_id.as_str(),
        tool_call_id,
        plugin,
        method,
        input_text,
    ]);
    let mut hasher = Sha256::new();
    hasher.update(array.to_string().as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailedCheck {
    NoSuchCall,
    AlreadyConsumed,
    Rejected,
    Expired,
    Discarded,
    SessionMismatch,
    IdentityMismatch,
    DigestMismatch,
}

impl FailedCheck {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoSuchCall => "no-such-call",
            Self::AlreadyConsumed => "already-consumed",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
            Self::Discarded => "discarded",
            Self::SessionMismatch => "session-mismatch",
            Self::IdentityMismatch => "identity-mismatch",
            Self::DigestMismatch => "digest-mismatch",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HoldState {
    Pending,
    Released,
    Rejected,
    Discarded,
}

#[derive(Debug, Clone)]
struct HeldCall {
    call: PendingToolCall,
    principal_id: PrincipalId,
    chat_session_id: ChatSessionId,
    input_bytes: Vec<u8>,
    digest: String,
    state: HoldState,
}

#[derive(Default)]
pub struct ConfirmationGate {
    held: HashMap<String, HeldCall>,
}

impl ConfirmationGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn hold(
        &mut self,
        mut call: PendingToolCall,
        principal_id: PrincipalId,
        chat_session_id: ChatSessionId,
        input_bytes: Vec<u8>,
        now_unix: u64,
    ) -> String {
        let input_text = String::from_utf8_lossy(&input_bytes).into_owned();
        let digest = digest(
            &principal_id,
            &chat_session_id,
            &call.tool_call_id,
            &call.plugin_id,
            &call.method,
            &input_text,
        );
        call.status = ToolCallStatus::AwaitingConfirmation;
        call.input_bytes = Some(input_bytes.clone());
        call.digest = Some(digest.clone());
        call.held_at_unix = now_unix;
        self.held.insert(
            call.tool_call_id.clone(),
            HeldCall {
                call,
                principal_id,
                chat_session_id,
                input_bytes,
                digest: digest.clone(),
                state: HoldState::Pending,
            },
        );
        digest
    }

    /// One synchronous compare-and-set: check and write with nothing in
    /// between. On success returns the held input bytes for the executor.
    pub fn confirm(
        &mut self,
        tool_call_id: &str,
        principal_id: &PrincipalId,
        chat_session_id: &ChatSessionId,
        presented_digest: &str,
        now_unix: u64,
    ) -> Result<Vec<u8>, FailedCheck> {
        let held = self
            .held
            .get_mut(tool_call_id)
            .ok_or(FailedCheck::NoSuchCall)?;
        match held.state {
            HoldState::Released => return Err(FailedCheck::AlreadyConsumed),
            HoldState::Rejected => return Err(FailedCheck::Rejected),
            HoldState::Discarded => return Err(FailedCheck::Discarded),
            HoldState::Pending => {}
        }
        if now_unix.saturating_sub(held.call.held_at_unix) > CONFIRM_TTL_SECS {
            return Err(FailedCheck::Expired);
        }
        if &held.chat_session_id != chat_session_id {
            return Err(FailedCheck::SessionMismatch);
        }
        if &held.principal_id != principal_id {
            return Err(FailedCheck::IdentityMismatch);
        }
        if held.digest != presented_digest {
            return Err(FailedCheck::DigestMismatch);
        }
        held.state = HoldState::Released;
        held.call.status = ToolCallStatus::Pending;
        Ok(held.input_bytes.clone())
    }

    pub fn reject(&mut self, tool_call_id: &str) {
        if let Some(held) = self.held.get_mut(tool_call_id) {
            if held.state == HoldState::Pending {
                held.state = HoldState::Rejected;
                held.call.status = ToolCallStatus::Rejected;
            }
        }
    }

    /// Discard every pending call matching `predicate` (catalog change,
    /// tool denial, session end, identity re-check failure, stream close).
    pub fn discard_where(&mut self, mut predicate: impl FnMut(&PendingToolCall) -> bool) {
        for held in self.held.values_mut() {
            if held.state == HoldState::Pending && predicate(&held.call) {
                held.state = HoldState::Discarded;
                held.call.status = ToolCallStatus::Rejected;
            }
        }
    }

    /// Expire holds older than 300 s on the caller-supplied clock (D-21).
    pub fn expire_due(&mut self, now_unix: u64) {
        for held in self.held.values_mut() {
            if held.state == HoldState::Pending
                && now_unix.saturating_sub(held.call.held_at_unix) > CONFIRM_TTL_SECS
            {
                held.state = HoldState::Discarded;
                held.call.status = ToolCallStatus::Rejected;
            }
        }
    }

    pub fn status(&self, tool_call_id: &str) -> Option<ToolCallStatus> {
        self.held.get(tool_call_id).map(|h| h.call.status)
    }
}
