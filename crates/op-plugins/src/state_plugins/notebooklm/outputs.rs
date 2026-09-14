//! Response contracts for the pinned jacob-bd nlm provider and bridge receipts.
//!
//! Keep named top-level fields: a flattened arbitrary map becomes an empty
//! generated protobuf message and cannot carry a successful provider response.

use super::provider_outputs::NotebookQueryOutput;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! command_outputs {
    ($($name:ident),+ $(,)?) => {$(
        /// Receipt for an nlm operation with human-readable stdout.
        /// Completion does not imply an inferred provider-side state change.
        #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            pub status: String,
            pub output_format: String,
            pub message: String,
            pub diagnostic: String,
        }
    )+};
}

command_outputs!(
    NotebookRenameOutput,
    SourceSyncDriveOutput,
    SourceRenameOutput,
    ChatConfigureOutput,
    ChatExportOutput,
    StudioDeleteOutput,
    StudioReviseOutput,
    DownloadArtifactOutput,
    ResearchStartOutput,
    ResearchStatusOutput,
    ResearchImportOutput,
    NotebookSharePublicOutput,
    NotebookShareInviteOutput,
    NotebookShareBatchOutput,
    BatchOutput,
    CrossNotebookQueryOutput,
    PipelineOutput,
    TagOutput,
);

macro_rules! auth_outputs {
    ($($name:ident),+ $(,)?) => {$(
        /// Result of an actual authenticated catalog probe, not profile presence.
        #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name {
            pub provider: String,
            pub auth_status: String,
            pub authenticated: bool,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub notebook_count: Option<u64>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub diagnostic: Option<String>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            pub hint: Option<String>,
        }
    )+};
}

auth_outputs!(
    GetHealthOutput,
    ServerInfoOutput,
    SetupAuthOutput,
    ReauthOutput,
    RefreshAuthOutput,
    SaveAuthTokensOutput,
);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectNotebookOutput {
    pub selected_notebook_id: String,
    pub session_id: String,
}

/// Query jobs are private to the verified principal/session and retained in
/// the bridge process until expiration. A restart invalidates these job IDs.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotebookQueryStartOutput {
    pub query_id: String,
    pub notebook_id: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotebookQueryState {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotebookQueryFailure {
    ProviderFailed,
    TimedOut,
    InvalidResponse,
    WorkerFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotebookQueryStatusOutput {
    pub query_id: String,
    pub notebook_id: String,
    pub expires_at: i64,
    pub state: NotebookQueryState,
    pub result: Option<NotebookQueryOutput>,
    pub failure: Option<NotebookQueryFailure>,
}
