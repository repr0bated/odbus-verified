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
branded!(ToolKey);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind {
    #[serde(rename = "3tched")]
    ThreeTched,
    Threetched,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogPin {
    pub view_id: String,
    pub source_catalog_hash: String,
    pub source_catalog_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffect {
    Read,
    Mutation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnavailableReason {
    Absent,
    Undetermined,
    ViewAbsent,
    Schema,
    StickyDenied,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub tool_key: ToolKey,
    pub plugin_id: String,
    pub method: String,
    pub args: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_capability: Option<String>,
    pub side_effect: SideEffect,
    pub idempotent: bool,
    pub subid: String,
    pub view_kind: ViewKind,
    pub offered: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<UnavailableReason>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RegistrySnapshot {
    pub pin: CatalogPin,
    pub entries: Vec<RegistryEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HeldCapabilityResult {
    Held { capabilities: Vec<String> },
    Denied { detail: String },
    Unavailable { detail: String },
    Undetermined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallStatus {
    Pending,
    AwaitingConfirmation,
    Running,
    Succeeded,
    Failed,
    Denied,
    Unknown,
    Undetermined,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingToolCall {
    pub tool_key: ToolKey,
    pub status: ToolCallStatus,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCallStatusUpdate {
    pub tool_key: ToolKey,
    pub status: ToolCallStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
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
