//! Global operator cron surface backed by the v0.8.4 chat-router gateway.
//!
//! The plugin owns this schema. The native gateway owns the install-wide job
//! store and applies the configured agent's risk/tool policy at execution.
//! OP-DBUS verifies the current SID1 session and exact method capability before
//! dispatch; those credentials are never forwarded to the gateway.

use anyhow::{Result, bail};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
pub const MAX_CRON_JOBS: usize = 4096;
pub const MAX_CRON_HISTORY: u32 = 100;
pub const MAX_CRON_TEXT_BYTES: usize = 256 * 1024;
pub const MAX_CRON_TOOL_NAMES: usize = 256;
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CronScope {
    GlobalDaemon,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CronEventSequencing {
    Unsequenced,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CronEventDelivery {
    BestEffort,
}

/// Integrity of the native wakeup stream, independent of the full snapshot
/// that follows each wakeup. v0.8.4 has no event IDs or replay cursor, so OP-DBUS
/// does not claim that every autonomous execution notification was delivered.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CronEventStreamIntegrity {
    pub sequencing: CronEventSequencing,
    pub delivery: CronEventDelivery,
}

impl Default for CronEventStreamIntegrity {
    fn default() -> Self {
        Self {
            sequencing: CronEventSequencing::Unsequenced,
            delivery: CronEventDelivery::BestEffort,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum CronSchedule {
    Cron {
        expr: String,
        #[serde(default)]
        tz: Option<String>,
    },
    At {
        at: String,
    },
    Every {
        every_ms: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CronJobType {
    Shell,
    Agent,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CronSessionTarget {
    Isolated,
    Main,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CronShellOutputFormat {
    Wrapped,
    Raw,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CronDelivery {
    pub mode: String,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default = "default_true")]
    pub best_effort: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CronJobRecord {
    pub id: String,
    pub expression: String,
    pub schedule: CronSchedule,
    pub command: String,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    pub job_type: CronJobType,
    pub session_target: CronSessionTarget,
    #[serde(default)]
    pub model: Option<String>,
    pub agent_alias: String,
    pub enabled: bool,
    pub delivery: CronDelivery,
    pub delete_after_run: bool,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    pub uses_memory: bool,
    pub source: String,
    pub shell_output_format: CronShellOutputFormat,
    pub created_at: String,
    pub next_run: String,
    #[serde(default)]
    pub last_run: Option<String>,
    #[serde(default)]
    pub last_status: Option<String>,
    #[serde(default)]
    pub last_output: Option<String>,
}

/// Complete authoritative cron projection. Global scheduler settings are not
/// included because they are not among the retained nine methods and the old
/// plugin values were fabricated.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid" = "sch.service.plugin.cron.schema@v2"))]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct CronStateV2 {
    #[schemars(extend("x-oscal-subid" = "obs.service.cron.scope@v2"))]
    pub scope: CronScope,
    /// Native `/api/events` is a wakeup transport only. State below always
    /// comes from a complete bounded `/api/cron` readback.
    pub event_stream: CronEventStreamIntegrity,
    #[schemars(extend("x-oscal-subid" = "mut.service.cron.manage-jobs@v1"))]
    pub jobs: Vec<CronJobRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CronRunRecord {
    pub id: i64,
    pub job_id: String,
    pub started_at: String,
    pub finished_at: String,
    pub status: String,
    #[serde(default)]
    pub output: Option<String>,
    #[serde(default)]
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListJobsInput {}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListJobsOutput {
    pub scope: CronScope,
    pub jobs: Vec<CronJobRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobIdInput {
    pub job_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetJobOutput {
    pub scope: CronScope,
    pub job: CronJobRecord,
}

fn default_history_limit() -> u32 {
    20
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetHistoryInput {
    pub job_id: String,
    #[serde(default = "default_history_limit")]
    #[schemars(range(min = 1, max = 100))]
    pub limit: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetHistoryOutput {
    pub scope: CronScope,
    pub job_id: String,
    pub runs: Vec<CronRunRecord>,
}

/// v0.8.4's gateway create endpoint accepts cron expressions only. The native
/// runtime also has `at`/`every` variants, but they are not admitted here.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "job_type", rename_all = "lowercase", deny_unknown_fields)]
pub enum CreateJobInput {
    Agent {
        agent: String,
        #[serde(default)]
        name: Option<String>,
        schedule: String,
        #[serde(default)]
        tz: Option<String>,
        prompt: String,
        #[serde(default)]
        delivery: Option<CronDelivery>,
        #[serde(default)]
        session_target: Option<CronSessionTarget>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        allowed_tools: Option<Vec<String>>,
        #[serde(default)]
        delete_after_run: Option<bool>,
        #[serde(default)]
        uses_memory: Option<bool>,
    },
    Shell {
        agent: String,
        #[serde(default)]
        name: Option<String>,
        schedule: String,
        #[serde(default)]
        tz: Option<String>,
        command: String,
        #[serde(default)]
        delivery: Option<CronDelivery>,
        #[serde(default)]
        shell_output_format: Option<CronShellOutputFormat>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateJobInput {
    pub job_id: String,
    /// Required only when changing a shell command; the provider ignores it
    /// for other patches.
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub schedule: Option<String>,
    #[serde(default)]
    pub tz: Option<String>,
    #[serde(default)]
    pub clear_tz: Option<bool>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub uses_memory: Option<bool>,
    #[serde(default)]
    pub shell_output_format: Option<CronShellOutputFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JobMutationOutput {
    pub scope: CronScope,
    pub disposition: MutationDisposition,
    /// Provider-returned post-mutation job. v0.8.4 obtains this from its store
    /// after create/update, so this is actual readback rather than an echo.
    #[serde(default)]
    pub job: Option<CronJobRecord>,
    #[serde(default)]
    pub diagnostic: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MutationDisposition {
    Confirmed,
    ProviderCompletedUnverified,
    Indeterminate,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeleteJobOutput {
    pub scope: CronScope,
    pub job_id: String,
    /// True only after the provider returned `status=ok`.
    pub provider_completed: bool,
    pub disposition: MutationDisposition,
    /// `Some(true)` means a follow-up global list confirmed absence. `None`
    /// means readback failed after provider completion; the delete is not
    /// relabelled as failed or rolled back.
    #[serde(default)]
    pub absent_on_readback: Option<bool>,
    #[serde(default)]
    pub readback_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunDisposition {
    Completed,
    Indeterminate,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ManualRunResult {
    pub status: String,
    pub job_id: String,
    pub success: bool,
    pub output: String,
    pub duration_ms: i64,
    pub started_at: String,
    pub finished_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunJobOutput {
    pub scope: CronScope,
    pub job_id: String,
    pub disposition: RunDisposition,
    #[serde(default)]
    pub result: Option<ManualRunResult>,
    /// Sanitized uncertainty detail, including a bounded native transport
    /// timeout or failure to publish observed state after a completed run.
    #[serde(default)]
    pub diagnostic: Option<String>,
}

pub struct CronPlugin;
impl Default for CronPlugin {
    fn default() -> Self {
        Self
    }
}
impl CronPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for CronPlugin {
    fn name(&self) -> &str {
        "cron"
    }
    fn version(&self) -> &str {
        "2.1.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = cron_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }
    async fn calculate_diff(&self, _: &Value, _: &Value) -> Result<StateDiff> {
        bail!("cron state is observed from the native gateway; use a declared cron method")
    }
    async fn apply_state(&self, _: &StateDiff) -> Result<ApplyResult> {
        bail!("cron has no generic apply operation; use a declared cron method")
    }
    async fn verify_state(&self, _: &Value) -> Result<bool> {
        bail!("cron has no generic desired-state verification operation")
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        bail!("cron does not own native gateway checkpoints")
    }
    async fn rollback(&self, _: &Checkpoint) -> Result<()> {
        bail!("cron does not own native gateway rollback")
    }
    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: false,
            supports_verification: false,
            atomic_operations: false,
        }
    }
}

pub(crate) fn cron_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(CronStateV2))
        .expect("cron state schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "cron",
        "2.1.0",
        "Global operator view and management of the native chat-router cron store",
        &root,
    );
    macro_rules! method {
        ($name:literal, $input:ty, $output:ty, $effect:expr_2021, $idem:expr_2021, $cap:literal, $subid:literal) => {
            schema.methods.insert(
                $name.into(),
                super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
                    $input,
                    $output,
                >($name, $effect, $idem, $cap, $subid),
            );
        };
    }
    method!(
        "list_jobs",
        ListJobsInput,
        ListJobsOutput,
        SideEffect::Read,
        true,
        "cron.read",
        "obs.service.cron.job.list@v1"
    );
    method!(
        "get_job",
        JobIdInput,
        GetJobOutput,
        SideEffect::Read,
        true,
        "cron.read",
        "obs.service.cron.job.get@v1"
    );
    method!(
        "get_history",
        GetHistoryInput,
        GetHistoryOutput,
        SideEffect::Read,
        true,
        "cron.read",
        "obs.service.cron.history.get@v1"
    );
    method!(
        "create_job",
        CreateJobInput,
        JobMutationOutput,
        SideEffect::Mutation,
        false,
        "cron.invoke",
        "mut.service.cron.job.create@v1"
    );
    method!(
        "update_job",
        UpdateJobInput,
        JobMutationOutput,
        SideEffect::Mutation,
        false,
        "cron.invoke",
        "mut.service.cron.job.update@v1"
    );
    method!(
        "delete_job",
        JobIdInput,
        DeleteJobOutput,
        SideEffect::Mutation,
        false,
        "cron.invoke",
        "mut.service.cron.job.delete@v1"
    );
    method!(
        "enable_job",
        JobIdInput,
        JobMutationOutput,
        SideEffect::Mutation,
        true,
        "cron.invoke",
        "mut.service.cron.job.enable@v1"
    );
    method!(
        "disable_job",
        JobIdInput,
        JobMutationOutput,
        SideEffect::Mutation,
        true,
        "cron.invoke",
        "mut.service.cron.job.disable@v1"
    );
    method!(
        "run_job",
        JobIdInput,
        RunJobOutput,
        SideEffect::Mutation,
        false,
        "cron.invoke",
        "mut.service.cron.job.run@v1"
    );
    schema.capabilities.insert("cron.read".into(), CapabilityDecl {
        id: "cron.read".into(),
        description: "Global operator observation of the native daemon cron job store and run history. Job prompts, commands, delivery targets, and outputs may be present.".into(),
    });
    schema.capabilities.insert("cron.invoke".into(), CapabilityDecl {
        id: "cron.invoke".into(),
        description: "Global operator management and manual execution of native daemon cron jobs. Creating an enabled job delegates future autonomous runs to its configured native agent risk/tool policy; later runs do not re-check the originating OP-DBUS grant.".into(),
    });
    // The global job store may contain prompts, commands, delivery targets and
    // outputs. No field is exposed on the shared public projection by default.
    schema.example = Some(simd_json::json!({
        "scope":"global_daemon",
        "event_stream":{"sequencing":"unsequenced","delivery":"best_effort"},
        "jobs":[]
    }));
    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_methods_are_exact_and_typed() {
        let schema = cron_schema();
        let expected = [
            "create_job",
            "delete_job",
            "disable_job",
            "enable_job",
            "get_history",
            "get_job",
            "list_jobs",
            "run_job",
            "update_job",
        ];
        let mut actual = schema
            .methods
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        actual.sort_unstable();
        assert_eq!(actual, expected);
        assert!(
            schema
                .methods
                .values()
                .all(|decl| decl.args != simd_json::json!(null))
        );
        assert!(schema.methods.values().all(|decl| {
            decl.returns
                .as_ref()
                .is_some_and(|schema| schema != &simd_json::json!(null))
        }));
        assert!(schema.public_state.paths.is_empty());
    }
    #[test]
    fn unknown_and_cross_variant_create_fields_fail() {
        assert!(
            serde_json::from_value::<CreateJobInput>(serde_json::json!({
                "job_type":"agent", "agent":"operator", "schedule":"*/5 * * * *",
                "prompt":"digest", "command":"echo bypass"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<JobIdInput>(serde_json::json!({
                "job_id":"id", "sealed_id":"caller-supplied"
            }))
            .is_err()
        );
    }
    #[test]
    fn global_state_example_matches_contract() {
        let state: CronStateV2 = serde_json::from_value(serde_json::json!({
            "scope":"global_daemon",
            "event_stream":{"sequencing":"unsequenced","delivery":"best_effort"},
            "jobs":[]
        }))
        .unwrap();
        assert!(state.jobs.is_empty());
        assert_eq!(state.event_stream, CronEventStreamIntegrity::default());
    }
}

inventory::submit! {
    crate::default_registry::PluginReg::new("cron", |_ctx| std::sync::Arc::new(CronPlugin::new()))
}
