//! Shared types for the identity chat interface (spec task 2.2).
//! Rust adaptation of the design's TypeScript data models: branded string
//! types become newtypes, string unions become enums.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

macro_rules! branded {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);
        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

branded!(PrincipalId);
branded!(ChatSessionId);
// Identifies one tool by its plugin method (`plugin/method`).
// A tool IS a plug-in: this is a plugin identifier, not a registry
// key. There is no tool registry and no registry pattern (owner,
// 2026-11-09); `RegistryEntry`/`RegistrySnapshot` were removed with
// the Tool_Registry retirement. When tools are needed, each is a
// one-file plugin (one .rs + one schema JSON).
branded!(ToolKey);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind {
    #[serde(rename = "3tched")]
    ThreeTched,
    Threetched,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    AwaitingConfirmation,
    Pending,
    Unknown,
    Unconfirmed,
    Completed,
    Rejected,
    Denied,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingToolCall {
    pub tool_call_id: String,
    pub tool_key: ToolKey,
    pub plugin_id: String,
    pub method: String,
    pub status: ToolCallStatus,
    pub arguments: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_bytes: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    pub held_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallStatusUpdate {
    pub tool_call_id: String,
    pub status: ToolCallStatus,
    pub seq: u64,
    pub plugin: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_capability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<ReceiptSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authoritative_state: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_back_error: Option<crate::errors::TypedError>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReceiptSummary {
    pub plugin_id: String,
    pub method: String,
    pub subid: String,
    pub operation_id: String,
    pub outcome: String,
    pub success: bool,
    pub event_id: u64,
    pub event_hash: String,
    pub terminal_event_id: u64,
    pub terminal_event_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublicIdentityFields {
    pub principal_id: PrincipalId,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundIdentityRecord {
    pub identity: Option<PublicIdentityFields>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<ReceiptSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatSession {
    pub session_id: ChatSessionId,
    pub principal_id: PrincipalId,
    pub view_kind: ViewKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdentityProfile {
    pub format: String,
    pub principal_id: PrincipalId,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_mode: Option<String>,
    pub deployment_targets: Vec<DeploymentTarget>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeploymentTarget {
    pub kind: DeploymentTargetKind,
    pub mount_paths: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentTargetKind {
    IdentityContainer,
    HostDashboard,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockerRecord {
    pub id: String,
    pub at: String,
    pub branch_display_name: String,
    pub operation: String,
    pub caller: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecurityEvent {
    pub at: String,
    pub kind: String,
    pub channel: String,
    pub credential_class: Option<CredentialClass>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CredentialClass {
    #[serde(rename = "sid1")]
    Sid1,
    #[serde(rename = "oia1")]
    Oia1,
    #[serde(rename = "provider-credential")]
    ProviderCredential,
    #[serde(rename = "identity-key")]
    IdentityKey,
    #[serde(rename = "w3-key")]
    W3Key,
    #[serde(rename = "declared-secret-field")]
    DeclaredSecretField,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionRecordView {
    pub id: String,
    pub prev_hash: String,
    pub exec_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<bool>,
}
