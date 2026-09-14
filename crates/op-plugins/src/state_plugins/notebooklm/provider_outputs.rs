//! Typed response contracts for the pinned `nlm` 0.11.2 CLI.
//!
//! The field names and nullable/optional behavior here come from the installed
//! `notebooklm_tools.services` TypedDicts and CLI JSON formatters. The bridge
//! normalizes root arrays and provider-specific variants before encoding the
//! generated protobuf responses. Keep variable provider data in `Value` fields;
//! a flattened arbitrary map produces an empty generated protobuf message.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotebookSummary {
    pub id: String,
    pub title: String,
    pub source_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotebookListOutput {
    pub notebooks: Vec<NotebookSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotebookSource {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotebookGetOutput {
    pub notebook_id: String,
    pub title: String,
    pub source_count: usize,
    pub url: String,
    pub sources: Vec<NotebookSource>,
    pub emoji: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SuggestedTopic {
    pub question: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotebookDescribeOutput {
    pub summary: String,
    pub suggested_topics: Vec<SuggestedTopic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotebookCreateOutput {
    pub notebook_id: String,
    pub title: String,
    pub url: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotebookDeleteOutput {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotebookQueryOutput {
    pub answer: String,
    pub question: String,
    pub conversation_id: Option<String>,
    pub sources_used: Vec<serde_json::Value>,
    pub citations: serde_json::Map<String, serde_json::Value>,
    pub references: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceAddItem {
    pub source_type: String,
    pub source_id: String,
    pub title: String,
}

/// Single-source and bulk `source add --json` results share one flat contract.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceAddOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub results: Option<Vec<SourceAddItem>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceDeleteOutput {
    pub status: String,
    pub deleted_source_ids: Vec<String>,
    pub deleted_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceDescribeOutput {
    pub summary: String,
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceGetContentOutput {
    pub content: Option<String>,
    pub title: Option<String>,
    pub source_type: Option<String>,
    pub char_count: Option<usize>,
    /// Writing to output_path returns the CLI's text receipt.
    pub status: Option<String>,
    pub output_format: Option<String>,
    pub message: Option<String>,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DriveSource {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub source_type: String,
    pub url: String,
    pub status: Option<i64>,
    pub is_stale: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive_doc_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SourceListDriveOutput {
    pub sources: Vec<DriveSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChatSession {
    pub conversation_id: String,
    pub turn_count: usize,
    pub preview: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChatListOutput {
    pub notebook_id: String,
    pub notebook_title: String,
    pub sessions: Vec<ChatSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChatGetOutput {
    pub notebook_id: String,
    pub notebook_title: String,
    pub conversation_id: String,
    pub turn_count: usize,
    pub transcript: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Artifact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub artifact_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_style_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub infographic_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slide_deck_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flashcard_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<usize>,
}

/// Normal and mind-map creation responses are represented by optional flat
/// fields. The bridge converts upstream `mind_map_json` into `mind_map`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StudioCreateOutput {
    pub artifact_type: String,
    pub artifact_id: String,
    pub status: Option<String>,
    pub title: Option<String>,
    pub root_name: Option<String>,
    pub children_count: Option<usize>,
    pub mind_map: Option<serde_json::Value>,
    pub message: String,
}

/// Status is normally an artifact array; rename uses the receipt fields.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StudioStatusOutput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<Vec<Artifact>>,
    pub status: Option<String>,
    pub output_format: Option<String>,
    pub message: Option<String>,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DownloadItem {
    pub artifact_id: Option<String>,
    pub artifact_type: String,
    pub title: String,
    pub path: String,
    /// Provider download outcome; this is not an invented acknowledgment.
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SkippedArtifact {
    pub artifact_id: Option<String>,
    pub artifact_type: String,
    pub title: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotebookSweepItem {
    pub notebook_id: String,
    pub notebook_title: String,
    pub output_dir: Option<String>,
    pub downloaded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub error: Option<String>,
}

/// Single-notebook and all-notebooks download results are key-discriminated.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DownloadAllArtifactsOutput {
    pub notebook_id: Option<String>,
    pub notebook_title: Option<String>,
    pub output_dir: String,
    pub items: Option<Vec<DownloadItem>>,
    pub skipped: Option<Vec<SkippedArtifact>>,
    pub total_artifacts: Option<usize>,
    pub notebooks: Option<Vec<NotebookSweepItem>>,
    pub total_notebooks: Option<usize>,
    pub downloaded: usize,
    pub failed: usize,
    pub errored_notebooks: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExportArtifactOutput {
    pub status: String,
    pub notebook_id: String,
    pub artifact_id: String,
    pub export_type: String,
    pub url: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Label {
    pub id: String,
    pub name: String,
    pub emoji: String,
    pub source_ids: Vec<String>,
}

/// Flat union for label list, mutation, and delete JSON branches.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LabelOutput {
    pub status: Option<String>,
    pub notebook_id: Option<String>,
    pub labels: Option<Vec<Label>>,
    pub count: Option<usize>,
    pub label_id: Option<String>,
    pub deleted_label_ids: Option<Vec<String>>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub preview: String,
}

/// Flat union for note list and the bridge's mutation text receipts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NoteOutput {
    pub status: Option<String>,
    pub notebook_id: Option<String>,
    pub notes: Option<Vec<Note>>,
    pub count: Option<usize>,
    pub message: Option<String>,
    pub output_format: Option<String>,
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Collaborator {
    pub email: String,
    pub role: String,
    pub is_pending: bool,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NotebookShareStatusOutput {
    pub notebook_id: String,
    pub is_public: bool,
    pub access_level: String,
    pub public_link: Option<String>,
    pub collaborators: Vec<Collaborator>,
    pub collaborator_count: usize,
}
