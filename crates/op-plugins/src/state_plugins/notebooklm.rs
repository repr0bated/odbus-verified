//! NotebookLM plugin — projects NotebookLM (knowledge notebooks) state into the
//! D-Bus tree at `/org/opdbus/v1/plugins/notebooklm`.
//!
//! Upstream binary is jacob-bd `nlm` at `/usr/local/libexec/op-dbus/bin/nlm`. The plugin id stays
//! `notebooklm`. Catalog and auth come from CLI JSON, not a local library.json.

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::SideEffect;
use op_state_store::{CapabilityDecl, PluginSchema};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use simd_json::prelude::*;
use std::path::Path;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;

pub mod outputs;
pub mod provider_outputs;

use outputs::*;
use provider_outputs::*;

/// Manifest of the designated knowledge corpus.
const MANIFEST_PATH: &str = "knowledge/notebooks.manifest.json";
const PROFILE_ROOT: &str = "/home/jeremy/.notebooklm-mcp-cli";
const DEFAULT_NLM_BIN: &str = "/usr/local/libexec/op-dbus/bin/nlm";

/// NotebookLM authentication/profile state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.notebooklm.auth.schema@v1"))]
pub struct NotebookLmAuth {
    /// Whether the NotebookLM CLI profile is present.
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.profile.configured@v1"))]
    pub profile_configured: bool,
    /// Path to the default CLI profile.
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.profile.path@v1"))]
    pub profile_path: String,
    /// Note about session lifetime.
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.profile.note@v1"))]
    pub note: String,
}

/// NotebookLM master notebook reference.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.notebooklm.master.schema@v1"))]
pub struct NotebookLmMaster {
    /// Master notebook identifier.
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.master.id@v1"))]
    pub id: String,
    /// Human-readable notebook title.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.master.title@v1"))]
    pub title: Option<String>,
}

/// NotebookLM designated knowledge corpus.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.notebooklm.corpus.schema@v1"))]
pub struct NotebookLmCorpus {
    /// Path to the corpus manifest file.
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.corpus.manifest@v1"))]
    pub manifest: String,
    /// Number of notebooks designated in the manifest.
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.corpus.designated-count@v1"))]
    pub designated_notebooks: usize,
    /// Maximum source count allowed by the corpus.
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.corpus.source-cap@v1"))]
    pub source_cap: usize,
    /// Destination sinks for the corpus.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.corpus.sinks@v1"))]
    pub sinks: Option<Vec<String>>,
}

/// NotebookLM transport/ingest configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.notebooklm.config.schema@v1"))]
pub struct NotebookLmConfig {
    /// CLI tool name.
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.config.cli@v1"))]
    pub cli: String,
    /// Transport mechanism.
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.config.transport@v1"))]
    pub transport: String,
    /// Ingest pipeline identifier.
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.config.ingest-pipeline@v1"))]
    pub ingest_pipeline: String,
}

/// Runtime state of the NotebookLM plugin.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.notebooklm.schema@v1"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct NotebookLmState {
    /// NotebookLM configuration status.
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.status.query@v1"))]
    pub status: String,
    /// Authentication/profile state.
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.auth.query@v1"))]
    pub auth: NotebookLmAuth,
    /// Designated master notebook.
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.master.query@v1"))]
    pub master_notebook: NotebookLmMaster,
    /// Designated knowledge corpus.
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.corpus.query@v1"))]
    pub corpus: NotebookLmCorpus,
    /// Ingest/transport configuration.
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.config.query@v1"))]
    pub config: NotebookLmConfig,
    /// Last notebook id chosen via `select_notebook`.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.service.notebooklm.selected-notebook.query@v1"))]
    pub selected_notebook_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct EmptyInput {}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NotebookIdInput {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub notebook_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NotebookCreateInput {
    pub title: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NotebookRenameInput {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub new_title: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConfirmNotebookInput {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub confirm: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NotebookQueryInput {
    pub question: String,
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryStatusInput {
    pub query_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SourceAddInput {
    pub source_type: String,
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub urls: Option<Vec<String>>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub document_id: Option<String>,
    #[serde(default)]
    pub doc_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SourceIdInput {
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub output_path: Option<String>,
    #[serde(default)]
    pub confirm: Option<bool>,
    #[serde(default)]
    pub source_ids: Option<Vec<String>>,
    #[serde(default)]
    pub new_title: Option<String>,
    #[serde(default)]
    pub skip_freshness: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChatConfigureInput {
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub goal: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub response_length: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChatGetInput {
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub chat_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChatExportInput {
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub chat_id: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub output_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StudioCreateInput {
    pub artifact_type: String,
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub confirm: Option<bool>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub length: Option<String>,
    #[serde(default)]
    pub focus: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StudioStatusInput {
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub artifact_id: Option<String>,
    #[serde(default)]
    pub full: Option<bool>,
    #[serde(default)]
    pub include_details: Option<bool>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub new_title: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StudioArtifactInput {
    #[serde(default)]
    pub notebook_id: Option<String>,
    pub artifact_id: String,
    #[serde(default)]
    pub confirm: Option<bool>,
    #[serde(default)]
    pub instruction: Option<String>,
    #[serde(default)]
    pub export_type: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DownloadArtifactInput {
    pub artifact_type: String,
    pub output_path: String,
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DownloadAllInput {
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub output_dir: Option<String>,
    #[serde(default)]
    pub all_notebooks: Option<bool>,
    #[serde(default)]
    pub skip_existing: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResearchStartInput {
    pub query: String,
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub auto_import: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResearchStatusInput {
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub full: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResearchImportInput {
    pub task_id: String,
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub indices: Option<String>,
    #[serde(default)]
    pub cited_only: Option<bool>,
    #[serde(default)]
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ActionInput {
    pub action: String,
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub confirm: Option<bool>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub note_id: Option<String>,
    #[serde(default)]
    pub label_id: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub new_name: Option<String>,
    #[serde(default)]
    pub emoji: Option<String>,
    #[serde(default)]
    pub unlabeled_only: Option<bool>,
    #[serde(default)]
    pub tags: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
    #[serde(default)]
    pub pipeline_name: Option<String>,
    #[serde(default)]
    pub input_url: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub notebook_names: Option<String>,
    #[serde(default)]
    pub all: Option<bool>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub titles: Option<String>,
    #[serde(default)]
    pub artifact_type: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShareInviteInput {
    pub email: String,
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShareBatchInput {
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub confirm: Option<bool>,
    #[serde(default)]
    pub recipients: Option<serde_json::Value>,
    #[serde(default)]
    pub emails: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SharePublicInput {
    #[serde(default)]
    pub notebook_id: Option<String>,
    #[serde(default)]
    pub off: Option<bool>,
    #[serde(default)]
    pub disable: Option<bool>,
    #[serde(default)]
    pub action: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SaveAuthInput {
    #[serde(default)]
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SetupAuthInput {
    #[serde(default)]
    pub show_browser: Option<bool>,
}

pub struct NotebookLmPlugin;

impl Default for NotebookLmPlugin {
    fn default() -> Self {
        Self
    }
}

impl NotebookLmPlugin {
    pub fn new() -> Self {
        Self
    }

    fn nlm_bin() -> std::path::PathBuf {
        std::env::var("OP_NLM_BIN")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from(DEFAULT_NLM_BIN))
    }

    fn profile_configured() -> bool {
        let root = Path::new(PROFILE_ROOT);
        root.join("metadata.json").exists() || root.is_dir()
    }

    /// Read the designated-corpus manifest if present (best-effort).
    fn read_manifest() -> Option<Value> {
        let mut buf = std::fs::read(MANIFEST_PATH).ok()?;
        simd_json::to_owned_value(&mut buf).ok()
    }

    pub fn current_state() -> NotebookLmState {
        let configured = Self::profile_configured();
        let nlm_present = Self::nlm_bin().is_file();

        let (master, corpus) = match Self::read_manifest() {
            Some(m) => {
                let obj = m.as_object();
                let master_id = obj
                    .and_then(|o| o.get("master_notebook"))
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| "unknown".to_string());
                let designated_count = obj
                    .and_then(|o| o.get("designated"))
                    .and_then(|d| d.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                (
                    NotebookLmMaster {
                        id: master_id,
                        title: Some("Ghostbridge Live!".to_string()),
                    },
                    NotebookLmCorpus {
                        manifest: MANIFEST_PATH.to_string(),
                        designated_notebooks: designated_count,
                        source_cap: 300,
                        sinks: Some(vec![
                            "semantic:qdrant".to_string(),
                            "graph:cozo".to_string(),
                        ]),
                    },
                )
            }
            None => (
                NotebookLmMaster {
                    id: "unknown".to_string(),
                    title: None,
                },
                NotebookLmCorpus {
                    manifest: MANIFEST_PATH.to_string(),
                    designated_notebooks: 0,
                    source_cap: 300,
                    sinks: None,
                },
            ),
        };

        NotebookLmState {
            status: if nlm_present {
                if configured {
                    "ready"
                } else {
                    "authentication_required"
                }
            } else {
                "unavailable"
            }
            .to_string(),
            auth: NotebookLmAuth {
                profile_configured: configured,
                profile_path: PROFILE_ROOT.to_string(),
                note: "jacob-bd nlm login writes cookies under ~/.notebooklm-mcp-cli; live auth_status comes from an authenticated notebook-list probe, not file presence"
                    .to_string(),
            },
            master_notebook: master,
            corpus,
            config: NotebookLmConfig {
                cli: "nlm (notebooklm-mcp-cli@0.11.2)".to_string(),
                transport: DEFAULT_NLM_BIN.to_string(),
                ingest_pipeline: "OD-23 (embedding_model->Qdrant + Cozo graph)".to_string(),
            },
            selected_notebook_id: None,
        }
    }
}

#[async_trait]
impl StatePlugin for NotebookLmPlugin {
    fn name(&self) -> &str {
        "notebooklm"
    }
    fn version(&self) -> &str {
        "5.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(notebooklm_schema())
    }
    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        })
    }
    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }
    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(Self::current_state())?,
            backend_checkpoint: None,
        })
    }
    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }
    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

fn insert_nlm<I: schemars::JsonSchema, O: schemars::JsonSchema>(
    schema: &mut PluginSchema,
    name: &str,
    side_effect: SideEffect,
    idempotent: bool,
    cap: &str,
    subid: &str,
) {
    schema.methods.insert(
        name.to_string(),
        method_decl_from_schemars_with_output::<I, O>(name, side_effect, idempotent, cap, subid),
    );
}

/// NotebookLM schema derived from the typed [`NotebookLmState`] struct via schemars.
pub(crate) fn notebooklm_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(NotebookLmState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "notebooklm",
        "5.0.0",
        "NotebookLM knowledge notebooks via jacob-bd nlm CLI",
        &root,
    );

    insert_nlm::<EmptyInput, NotebookListOutput>(
        &mut schema,
        "notebook_list",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.notebook.list@v1",
    );
    insert_nlm::<NotebookCreateInput, NotebookCreateOutput>(
        &mut schema,
        "notebook_create",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.notebook.create@v1",
    );
    insert_nlm::<NotebookIdInput, NotebookGetOutput>(
        &mut schema,
        "notebook_get",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.notebook.get@v1",
    );
    insert_nlm::<NotebookIdInput, NotebookDescribeOutput>(
        &mut schema,
        "notebook_describe",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.notebook.describe@v1",
    );
    insert_nlm::<NotebookRenameInput, NotebookRenameOutput>(
        &mut schema,
        "notebook_rename",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.notebook.rename@v1",
    );
    insert_nlm::<ConfirmNotebookInput, NotebookDeleteOutput>(
        &mut schema,
        "notebook_delete",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.notebook.delete@v1",
    );
    insert_nlm::<NotebookIdInput, SelectNotebookOutput>(
        &mut schema,
        "select_notebook",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.notebook.select@v1",
    );
    insert_nlm::<NotebookQueryInput, NotebookQueryOutput>(
        &mut schema,
        "notebook_query",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.notebook.query@v1",
    );
    insert_nlm::<NotebookQueryInput, NotebookQueryStartOutput>(
        &mut schema,
        "notebook_query_start",
        SideEffect::Read,
        false,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.query.start@v1",
    );
    insert_nlm::<QueryStatusInput, NotebookQueryStatusOutput>(
        &mut schema,
        "notebook_query_status",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.query.status@v1",
    );
    insert_nlm::<SourceAddInput, SourceAddOutput>(
        &mut schema,
        "source_add",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.source.add@v1",
    );
    insert_nlm::<SourceIdInput, SourceListDriveOutput>(
        &mut schema,
        "source_list_drive",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.source.list@v1",
    );
    insert_nlm::<SourceIdInput, SourceSyncDriveOutput>(
        &mut schema,
        "source_sync_drive",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.source.sync-drive@v1",
    );
    insert_nlm::<SourceIdInput, SourceDeleteOutput>(
        &mut schema,
        "source_delete",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.source.delete@v1",
    );
    insert_nlm::<SourceIdInput, SourceDescribeOutput>(
        &mut schema,
        "source_describe",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.source.describe@v1",
    );
    insert_nlm::<SourceIdInput, SourceGetContentOutput>(
        &mut schema,
        "source_get_content",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.source.content@v1",
    );
    insert_nlm::<SourceIdInput, SourceRenameOutput>(
        &mut schema,
        "source_rename",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.source.rename@v1",
    );
    insert_nlm::<ChatConfigureInput, ChatConfigureOutput>(
        &mut schema,
        "chat_configure",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.chat.configure@v1",
    );
    insert_nlm::<NotebookIdInput, ChatListOutput>(
        &mut schema,
        "chat_list",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.chat.list@v1",
    );
    insert_nlm::<ChatGetInput, ChatGetOutput>(
        &mut schema,
        "chat_get",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.chat.get@v1",
    );
    insert_nlm::<ChatExportInput, ChatExportOutput>(
        &mut schema,
        "chat_export",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.chat.export@v1",
    );
    insert_nlm::<StudioCreateInput, StudioCreateOutput>(
        &mut schema,
        "studio_create",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.studio.create@v1",
    );
    insert_nlm::<StudioStatusInput, StudioStatusOutput>(
        &mut schema,
        "studio_status",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.studio.status@v1",
    );
    insert_nlm::<StudioArtifactInput, StudioDeleteOutput>(
        &mut schema,
        "studio_delete",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.studio.delete@v1",
    );
    insert_nlm::<StudioArtifactInput, StudioReviseOutput>(
        &mut schema,
        "studio_revise",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.studio.revise@v1",
    );
    insert_nlm::<DownloadArtifactInput, DownloadArtifactOutput>(
        &mut schema,
        "download_artifact",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.studio.download@v1",
    );
    insert_nlm::<DownloadAllInput, DownloadAllArtifactsOutput>(
        &mut schema,
        "download_all_artifacts",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.studio.download-all@v1",
    );
    insert_nlm::<StudioArtifactInput, ExportArtifactOutput>(
        &mut schema,
        "export_artifact",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.studio.export@v1",
    );
    insert_nlm::<ResearchStartInput, ResearchStartOutput>(
        &mut schema,
        "research_start",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.research.start@v1",
    );
    insert_nlm::<ResearchStatusInput, ResearchStatusOutput>(
        &mut schema,
        "research_status",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.research.status@v1",
    );
    insert_nlm::<ResearchImportInput, ResearchImportOutput>(
        &mut schema,
        "research_import",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.research.import@v1",
    );
    insert_nlm::<ActionInput, NoteOutput>(
        &mut schema,
        "note",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.note.dispatch@v1",
    );
    insert_nlm::<ActionInput, LabelOutput>(
        &mut schema,
        "label",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.label.dispatch@v1",
    );
    insert_nlm::<NotebookIdInput, NotebookShareStatusOutput>(
        &mut schema,
        "notebook_share_status",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.share.settings@v1",
    );
    insert_nlm::<SharePublicInput, NotebookSharePublicOutput>(
        &mut schema,
        "notebook_share_public",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.share.public@v1",
    );
    insert_nlm::<ShareInviteInput, NotebookShareInviteOutput>(
        &mut schema,
        "notebook_share_invite",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.share.invite@v1",
    );
    insert_nlm::<ShareBatchInput, NotebookShareBatchOutput>(
        &mut schema,
        "notebook_share_batch",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.share.batch@v1",
    );
    insert_nlm::<EmptyInput, RefreshAuthOutput>(
        &mut schema,
        "refresh_auth",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.auth.refresh@v1",
    );
    insert_nlm::<SaveAuthInput, SaveAuthTokensOutput>(
        &mut schema,
        "save_auth_tokens",
        SideEffect::Mutation,
        false,
        "notebooklm.admin",
        "mut.service.plugin.notebooklm.auth.save-tokens@v1",
    );
    insert_nlm::<EmptyInput, ServerInfoOutput>(
        &mut schema,
        "server_info",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.server.info@v1",
    );
    insert_nlm::<ActionInput, BatchOutput>(
        &mut schema,
        "batch",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.batch.run@v1",
    );
    insert_nlm::<ActionInput, CrossNotebookQueryOutput>(
        &mut schema,
        "cross_notebook_query",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.cross.query@v1",
    );
    insert_nlm::<ActionInput, PipelineOutput>(
        &mut schema,
        "pipeline",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.pipeline.run@v1",
    );
    insert_nlm::<ActionInput, TagOutput>(
        &mut schema,
        "tag",
        SideEffect::Mutation,
        false,
        "notebooklm.invoke",
        "mut.service.plugin.notebooklm.tag.dispatch@v1",
    );
    insert_nlm::<EmptyInput, GetHealthOutput>(
        &mut schema,
        "get_health",
        SideEffect::Read,
        true,
        "notebooklm.read",
        "obs.service.plugin.notebooklm.health@v1",
    );
    insert_nlm::<SetupAuthInput, SetupAuthOutput>(
        &mut schema,
        "setup_auth",
        SideEffect::Mutation,
        false,
        "notebooklm.admin",
        "mut.service.plugin.notebooklm.auth.setup@v1",
    );
    insert_nlm::<EmptyInput, ReauthOutput>(
        &mut schema,
        "reauth",
        SideEffect::Mutation,
        false,
        "notebooklm.admin",
        "mut.service.plugin.notebooklm.auth.reauth@v1",
    );

    schema.capabilities.insert(
        "notebooklm.read".to_string(),
        CapabilityDecl {
            id: "notebooklm.read".to_string(),
            description: "Grants: notebook_list, notebook_get, notebook_describe, notebook_query, notebook_query_start, notebook_query_status, source_list_drive, source_describe, source_get_content, chat_list, chat_get, chat_export, studio_status, download_artifact, download_all_artifacts, research_status, notebook_share_status, cross_notebook_query, server_info, get_health.".to_string(),
        },
    );
    schema.capabilities.insert(
        "notebooklm.invoke".to_string(),
        CapabilityDecl {
            id: "notebooklm.invoke".to_string(),
            description: "Grants: select_notebook, notebook_create, notebook_rename, notebook_delete, source_add, source_sync_drive, source_delete, source_rename, chat_configure, studio_create, studio_delete, studio_revise, export_artifact, research_start, research_import, note, label, notebook_share_public, notebook_share_invite, notebook_share_batch, refresh_auth, batch, pipeline, tag.".to_string(),
        },
    );
    schema.capabilities.insert(
        "notebooklm.admin".to_string(),
        CapabilityDecl {
            id: "notebooklm.admin".to_string(),
            description: "Interactive nlm login (setup_auth, reauth) and save_auth_tokens."
                .to_string(),
        },
    );

    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::common::oscal::validate_subid;
    use serde_json::Value as JVal;

    fn collect_subids(value: &JVal, out: &mut Vec<String>) {
        if let Some(obj) = value.as_object() {
            if let Some(JVal::String(subid)) = obj.get("x-oscal-subid") {
                out.push(subid.clone());
            }
            for v in obj.values() {
                collect_subids(v, out);
            }
        }
        if let Some(arr) = value.as_array() {
            for v in arr {
                collect_subids(v, out);
            }
        }
    }

    #[test]
    fn all_subids_are_valid() {
        let root = serde_json::to_value(schemars::schema_for!(NotebookLmState))
            .expect("schemars schema serializes to JSON");
        let mut subids = Vec::new();
        collect_subids(&root, &mut subids);
        assert!(!subids.is_empty(), "expected at least one subid");
        for subid in subids {
            assert!(validate_subid(&subid).is_ok(), "invalid subid: {subid}");
        }
    }

    #[test]
    fn schema_uses_jacob_method_names() {
        let schema = notebooklm_schema();
        assert!(schema.methods.contains_key("notebook_list"));
        assert!(schema.methods.contains_key("notebook_query"));
        assert!(schema.methods.contains_key("get_health"));
        assert!(schema.methods.contains_key("setup_auth"));
        assert!(!schema.methods.contains_key("list_notebooks"));
        assert!(!schema.methods.contains_key("query_notebook"));
        for method in schema.methods.values() {
            assert!(
                validate_subid(&method.subid).is_ok(),
                "invalid method subid {}",
                method.subid
            );
        }
    }
}

inventory::submit! {
    crate::default_registry::PluginReg::new("notebooklm", |_ctx| std::sync::Arc::new(NotebookLmPlugin::new()))
}
