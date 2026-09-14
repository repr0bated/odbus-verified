use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use super::plugin_scaffold_helpers::{
    method_decl_from_schemars, method_decl_from_schemars_with_output,
};

/// Runtime state of the workflow automation plugin.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.workflows.schema@v1"))]
#[schemars(extend("x-oscal-category" = "software"))]
pub struct WorkflowsState {
    /// Operational status.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.plugin.workflows.status@v1"))]
    pub status: String,
    /// Declared workflow pipelines.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "exp.software.plugin.workflows.workflows@v1"))]
    pub workflows: serde_json::Value,
    /// Workflow scheduler configuration.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "sch.software.plugin.workflows.config@v1"))]
    pub config: serde_json::Value,
}

/// Input struct for StartWorkflow method.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StartWorkflowInput {
    /// Workflow ID to start
    pub workflow_id: String,
    /// Optional parameters
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QueryWorkflowsInput {
    pub workflow_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WorkflowSummary {
    pub id: String,
    pub name: String,
    pub steps: Vec<String>,
    pub triggers: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct QueryWorkflowsOutput {
    pub workflows: Vec<WorkflowSummary>,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StartWorkflowOutput {
    pub accepted: bool,
    pub execution_id: String,
    pub workflow_id: String,
    pub status: String,
    pub accepted_at: String,
}

/// Input struct for PauseWorkflow method.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PauseWorkflowInput {
    /// Workflow ID to pause
    pub workflow_id: String,
}

/// Input struct for ResumeWorkflow method.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ResumeWorkflowInput {
    /// Workflow ID to resume
    pub workflow_id: String,
}

/// Input struct for CancelWorkflow method.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CancelWorkflowInput {
    /// Workflow ID to cancel
    pub workflow_id: String,
    /// Force cancel even if running
    #[serde(default)]
    pub force: bool,
}

impl Default for WorkflowsState {
    fn default() -> Self {
        Self {
            status: "active".to_string(),
            workflows: serde_json::json!([{"id": "deploy", "name": "Deploy Pipeline", "steps": ["build", "test", "deploy"], "triggers": ["push", "manual"], "status": "idle"}, {"id": "backup", "name": "System Backup", "steps": ["snapshot", "archive", "verify"], "triggers": ["cron"], "status": "idle"}, {"id": "embedding-sync", "name": "Embedding Pipeline Sync", "steps": ["ingest", "chunk", "embed", "store"], "triggers": ["webhook"], "status": "idle"}]),
            config: serde_json::json!({"max_concurrent": 4, "timeout_secs": 3600, "retry_count": 2, "notification_channel": "telegram"}),
        }
    }
}

pub struct WorkflowsPlugin;
impl Default for WorkflowsPlugin {
    fn default() -> Self {
        Self
    }
}
impl WorkflowsPlugin {
    pub fn new() -> Self {
        Self
    }
    pub(crate) fn current_state() -> WorkflowsState {
        WorkflowsState {
            status: "active".to_string(),
            workflows: serde_json::json!([{"id": "deploy", "name": "Deploy Pipeline", "steps": ["build", "test", "deploy"], "triggers": ["push", "manual"], "status": "idle"}, {"id": "backup", "name": "System Backup", "steps": ["snapshot", "archive", "verify"], "triggers": ["cron"], "status": "idle"}, {"id": "embedding-sync", "name": "Embedding Pipeline Sync", "steps": ["ingest", "chunk", "embed", "store"], "triggers": ["webhook"], "status": "idle"}]),
            config: serde_json::json!({"max_concurrent": 4, "timeout_secs": 3600, "retry_count": 2, "notification_channel": "telegram"}),
        }
    }
}
#[async_trait]
impl StatePlugin for WorkflowsPlugin {
    fn name(&self) -> &str {
        "workflows"
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(workflows_schema())
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

/// Derived `workflows` schema from the typed [`WorkflowsState`] struct via schemars.
pub(crate) fn workflows_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(WorkflowsState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "workflows",
        "1.0.0",
        "Workflow automation — pipelines, triggers, execution",
        &root,
    );
    let state = simd_json::serde::to_owned_value(&WorkflowsState::default())
        .expect("WorkflowsState default serializes");
    super::schemars_adapter::apply_state_defaults(&mut schema, &state);

    // StartWorkflow method
    schema.methods.insert(
        "start_workflow".to_string(),
        method_decl_from_schemars_with_output::<StartWorkflowInput, StartWorkflowOutput>(
            "StartWorkflow",
            op_state_store::SideEffect::Mutation,
            false,
            "workflows.write",
            "mut.software.plugin.workflows.start@v1",
        ),
    );

    schema.methods.insert(
        "query_workflows".to_string(),
        method_decl_from_schemars_with_output::<QueryWorkflowsInput, QueryWorkflowsOutput>(
            "query_workflows",
            op_state_store::SideEffect::Read,
            true,
            "workflows.read",
            "obs.software.plugin.workflows.query@v1",
        ),
    );

    // PauseWorkflow method
    schema.methods.insert(
        "pause_workflow".to_string(),
        method_decl_from_schemars_with_output::<
            PauseWorkflowInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "PauseWorkflow",
            op_state_store::SideEffect::Mutation,
            false,
            "workflows.write",
            "mut.software.plugin.workflows.pause@v1",
        ),
    );

    // ResumeWorkflow method
    schema.methods.insert(
        "resume_workflow".to_string(),
        method_decl_from_schemars_with_output::<
            ResumeWorkflowInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "ResumeWorkflow",
            op_state_store::SideEffect::Mutation,
            false,
            "workflows.write",
            "mut.software.plugin.workflows.resume@v1",
        ),
    );

    // CancelWorkflow method
    schema.methods.insert(
        "cancel_workflow".to_string(),
        method_decl_from_schemars_with_output::<
            CancelWorkflowInput,
            super::plugin_scaffold_helpers::AckOutput,
        >(
            "CancelWorkflow",
            op_state_store::SideEffect::Mutation,
            false,
            "workflows.write",
            "mut.software.plugin.workflows.cancel@v1",
        ),
    );

    schema.capabilities.insert(
        "workflows.read".to_string(),
        CapabilityDecl {
            id: "workflows.read".to_string(),
            description: "Grants: query_workflows.".to_string(),
        },
    );
    schema.capabilities.insert(
        "workflows.write".to_string(),
        CapabilityDecl {
            id: "workflows.write".to_string(),
            description:
                "Grants: start_workflow, pause_workflow, resume_workflow, cancel_workflow."
                    .to_string(),
        },
    );

    schema
}

const DEFAULT_WORKFLOW_JOURNAL: &str = "/var/lib/op-dbus/workflows/run-queue.jsonl";
static JOURNAL_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn workflow_summaries() -> Vec<WorkflowSummary> {
    WorkflowsPlugin::current_state()
        .workflows
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|workflow| serde_json::from_value(workflow.clone()).ok())
        .collect()
}

fn journal_path() -> PathBuf {
    std::env::var_os("OP_WORKFLOW_JOURNAL_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_WORKFLOW_JOURNAL))
}

fn append_workflow_request(path: &Path, record: &serde_json::Value) -> Result<()> {
    let _guard = JOURNAL_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| anyhow::anyhow!("workflow journal lock poisoned"))?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("workflow journal has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    serde_json::to_writer(&mut file, record)?;
    file.write_all(b"\n")?;
    file.sync_data()?;
    Ok(())
}

/// Runtime dispatcher for the workflow PluginSchema. Enqueue is durable and
/// local; it performs no broker/provider hop on the HOT admission path.
pub async fn dispatch_workflows_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    match method {
        "query_workflows" => {
            let input: QueryWorkflowsInput = serde_json::from_value(args.clone())?;
            let mut workflows = workflow_summaries();
            if let Some(id) = input.workflow_id {
                workflows.retain(|workflow| workflow.id == id);
            }
            let count = workflows.len();
            Ok(serde_json::to_value(QueryWorkflowsOutput {
                workflows,
                count,
            })?)
        }
        "start_workflow" => {
            let input: StartWorkflowInput = serde_json::from_value(args.clone())?;
            if !workflow_summaries()
                .iter()
                .any(|workflow| workflow.id == input.workflow_id)
            {
                anyhow::bail!("unknown workflow '{}'", input.workflow_id);
            }
            let output = StartWorkflowOutput {
                accepted: true,
                execution_id: uuid::Uuid::new_v4().to_string(),
                workflow_id: input.workflow_id,
                status: "queued".to_string(),
                accepted_at: chrono::Utc::now().to_rfc3339(),
            };
            let record = serde_json::json!({
                "schema_version": 1,
                "execution_id": output.execution_id,
                "workflow_id": output.workflow_id,
                "status": output.status,
                "accepted_at": output.accepted_at,
                "params": input.params
            });
            let path = journal_path();
            tokio::task::spawn_blocking(move || append_workflow_request(&path, &record)).await??;
            Ok(serde_json::to_value(output)?)
        }
        "pause_workflow" | "resume_workflow" | "cancel_workflow" => {
            anyhow::bail!(
                "workflows.{method} is declared but no local executor owns that transition"
            )
        }
        other => anyhow::bail!("unknown workflows method '{other}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::common::oscal::validate_subid;
    use crate::state_plugins::schemars_adapter::schema_diffs;
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
        let root = serde_json::to_value(schemars::schema_for!(WorkflowsState))
            .expect("schemars schema serializes to JSON");
        let mut subids = Vec::new();
        collect_subids(&root, &mut subids);
        assert!(!subids.is_empty(), "expected at least one subid");
        for subid in subids {
            assert!(validate_subid(&subid).is_ok(), "invalid subid: {subid}");
        }
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("workflows", |_ctx| std::sync::Arc::new(WorkflowsPlugin::new()))
}
