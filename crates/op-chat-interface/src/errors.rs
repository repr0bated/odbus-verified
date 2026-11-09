//! Typed errors (spec task 2.2, D-19): exactly one of eight kinds,
//! a failing item, and the operator next step looked up by kind.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorKind {
    MissingMount,
    Transport,
    Provider,
    Schema,
    Configuration,
    UnknownOutcome,
    Denial,
    Validation,
}

impl ErrorKind {
    pub fn next_step(self) -> &'static str {
        match self {
            Self::MissingMount => "The identity's 3tchedFS mount is missing or unreadable. Ask the owner to check the mount; the chat will not use another mount.",
            Self::Transport => "A recorded path did not answer within 10 s. Check the named path; the chat will not try another path.",
            Self::Provider => "The model provider failed or did not answer within 60 s. Pending actions are unchanged; resend the message later.",
            Self::Schema => "This operation's sealed schema could not be read. The tool is unavailable; report the plugin and method to the owner.",
            Self::Configuration => "The Identity_Profile or provider configuration was rejected. Fix the named field; no session was started.",
            Self::UnknownOutcome => "The operation may or may not have run. Check the authoritative state before asking again.",
            Self::Denial => "This identity is not allowed to run this operation. The denial is recorded; ask the owner if access is expected.",
            Self::Validation => "The arguments did not match the sealed schema. The model was told which fields failed.",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypedError {
    pub kind: ErrorKind,
    pub failing_item: String,
    pub next_step: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl TypedError {
    pub fn new(kind: ErrorKind, failing_item: impl Into<String>) -> Self {
        Self {
            kind,
            failing_item: failing_item.into(),
            next_step: kind.next_step().to_string(),
            detail: None,
        }
    }
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

impl std::fmt::Display for TypedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.failing_item)
    }
}
impl std::error::Error for TypedError {}
