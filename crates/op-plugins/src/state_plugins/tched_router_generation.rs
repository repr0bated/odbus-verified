//! The tched_router-owned private generation contract and durable transitions.
//! Native execution and SHM publication belong to MutationEngine. This module
//! never contacts a model, writes SHM, grants access, or stores a raw prompt.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};

use super::super::{
    gemma_brain::workspace::UiAccessContext,
    plugin_scaffold_helpers::method_decl_from_schemars_with_output,
    runtime_documents::RuntimeDocuments,
};

pub const START_CAPABILITY: &str = "cap.software.tched-router.generation.start@v1";
pub const READ_CAPABILITY: &str = "cap.software.tched-router.generation.read@v1";
pub const CANCEL_CAPABILITY: &str = "cap.software.tched-router.generation.cancel@v1";
pub const DELETE_CAPABILITY: &str = "cap.software.tched-router.generation.delete@v1";
pub const RECOVER_CAPABILITY: &str = "cap.software.tched-router.generation.recover@v1";
pub const MAX_JOBS: usize = 32;
pub const MAX_ACTIVE_JOBS: usize = 4;
pub const MAX_PROMPT_BYTES: usize = 64 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 256 * 1024;
/// Aggregate UTF-8 output retained across this owner's jobs. JSON can expand
/// one control byte to six bytes; 1 MiB output plus bounded job metadata fits
/// below the 9 MiB encoded document ceiling and existing stream frame budget.
pub const MAX_OWNER_OUTPUT_BYTES: usize = 1024 * 1024;
pub const MAX_CHUNK_BYTES: usize = 16 * 1024;
/// Opportunistic publication groups never exceed one fragment's byte limit.
/// Fragments retain their original ordering/count; the group is one durable
/// state transition, not a timer-buffered or unary response.
pub const MAX_BATCH_FRAGMENTS: usize = 64;
pub const MAX_FRAMES: u32 = 4096;
pub const MAX_DOCUMENT_BYTES: usize = 9 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartGenerationInput {
    pub agent_alias: String,
    pub prompt: String,
}

impl StartGenerationInput {
    pub fn validate(&self) -> Result<()> {
        validate_id(&self.agent_alias)?;
        ensure!(
            !self.prompt.trim().is_empty() && self.prompt.len() <= MAX_PROMPT_BYTES,
            "generation prompt is empty or exceeds its bound"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationJobInput {
    pub job_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationRevisionInput {
    pub job_id: String,
    pub expected_revision: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationEmptyInput {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GenerationStatus {
    Queued,
    Running,
    CancelRequested,
    Completed,
    Cancelled,
    Failed,
    Interrupted,
}
impl GenerationStatus {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Cancelled | Self::Failed | Self::Interrupted
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GenerationFailure {
    NativeError,
    NativeDisconnected,
    NativeAborted,
    ToolApprovalRequired,
    OutputLimit,
    Deadline,
    AdmissionWithdrawn,
    ProcessInterrupted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-opdbus-exact-json-schema" = true))]
pub struct GenerationJob {
    /// OP-DBUS job ID, never a fabricated native task ID.
    pub job_id: String,
    pub revision: u64,
    pub agent_alias: String,
    pub status: GenerationStatus,
    pub output: String,
    pub committed_chunks: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_model: Option<String>,
    pub prompt_digest: String,
    pub source_schema_hash: String,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<GenerationFailure>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationState {
    pub generation_revision: u64,
    pub generation_jobs: BTreeMap<String, GenerationJob>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationJobOutput {
    pub document_revision: u64,
    pub job: Option<GenerationJob>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationMutationOutput {
    pub document_revision: u64,
    pub job: GenerationJob,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationWorkspaceOutput {
    pub state: Option<GenerationState>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationDeleteOutput {
    pub document_revision: u64,
    pub job_id: String,
    pub deleted_revision: u64,
}

#[derive(Clone)]
pub struct GenerationOwner {
    principal: String,
    session: String,
    selector: String,
}
impl GenerationOwner {
    /// Reuse the established length-prefixed principal/session selector. The
    /// generation directory and Cozo document prefix separate domain storage;
    /// constructing this value itself does not authenticate either identifier.
    pub fn new(principal: String, session: String) -> Result<Self> {
        let key = UiAccessContext {
            principal_id: principal.clone(),
            session_id: session.clone(),
            session_genesis: None,
        }
        .owner_key()?;
        Ok(Self {
            principal,
            session,
            selector: key.namespace_component().to_owned(),
        })
    }
    pub fn selector(&self) -> &str {
        &self.selector
    }
    /// Opaque document selector for trusted owner recovery/inspection. This is
    /// not an authorization credential and contains no raw principal/session.
    pub fn document_id(&self) -> String {
        format!("tched_generation:v1:{}", self.selector)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredGenerations {
    format_version: u32,
    principal: String,
    session: String,
    jobs: BTreeMap<String, GenerationJob>,
}
impl StoredGenerations {
    fn empty(owner: &GenerationOwner) -> Self {
        Self {
            format_version: 1,
            principal: owner.principal.clone(),
            session: owner.session.clone(),
            jobs: BTreeMap::new(),
        }
    }
    fn decode(owner: &GenerationOwner, value: Value) -> Result<Self> {
        ensure!(
            serde_json::to_vec(&value)?.len() <= MAX_DOCUMENT_BYTES,
            "generation document exceeds its bound"
        );
        let stored: Self = serde_json::from_value(value)
            .map_err(|_| anyhow::anyhow!("invalid durable generation document"))?;
        ensure!(
            stored.format_version == 1
                && stored.principal == owner.principal
                && stored.session == owner.session,
            "generation document owner or format mismatch"
        );
        validate_jobs(&stored.jobs)?;
        Ok(stored)
    }
}

impl GenerationState {
    /// Pure preflight before the publisher enters its durable pending gate.
    /// The durable transition independently enforces the same aggregate bound.
    pub fn preflight_append(&self, id: &str, chunk: &str) -> Result<()> {
        self.preflight_append_batch(id, &[chunk.to_owned()])
    }

    pub fn preflight_append_batch(&self, id: &str, fragments: &[String]) -> Result<()> {
        let (bytes, count) = fragment_batch_size(fragments)?;
        validate_jobs(&self.generation_jobs)?;
        let job = self
            .generation_jobs
            .get(id)
            .context("generation job is absent")?;
        ensure!(
            job.status == GenerationStatus::Running
                && job
                    .committed_chunks
                    .checked_add(count)
                    .is_some_and(|total| total <= MAX_FRAMES),
            "generation chunk cannot be appended"
        );
        ensure!(
            job.output
                .len()
                .checked_add(bytes)
                .is_some_and(|size| size <= MAX_OUTPUT_BYTES)
                && output_bytes(&self.generation_jobs)?
                    .checked_add(bytes)
                    .is_some_and(|size| size <= MAX_OWNER_OUTPUT_BYTES),
            "generation owner or job output limit reached"
        );
        Ok(())
    }
    pub fn from_plugin_state(value: &Value) -> Result<Self> {
        ensure!(
            serde_json::to_vec(value)?.len() <= MAX_DOCUMENT_BYTES,
            "generation projection exceeds its bound"
        );
        let state: Self = serde_json::from_value(value.clone())
            .map_err(|_| anyhow::anyhow!("invalid generation projection"))?;
        ensure!(
            state.generation_revision > 0,
            "generation document revision is absent"
        );
        validate_jobs(&state.generation_jobs)?;
        Ok(state)
    }
    pub fn plugin_state(&self) -> Result<Value> {
        Ok(serde_json::to_value(self)?)
    }
}

fn fragment_batch_size(fragments: &[String]) -> Result<(usize, u32)> {
    ensure!(
        !fragments.is_empty() && fragments.len() <= MAX_BATCH_FRAGMENTS,
        "generation fragment batch count exceeds its bound"
    );
    let bytes = fragments.iter().try_fold(0usize, |sum, fragment| {
        ensure!(
            !fragment.is_empty() && fragment.len() <= MAX_CHUNK_BYTES,
            "native generation fragment exceeds its bound"
        );
        sum.checked_add(fragment.len())
            .context("generation fragment batch size overflow")
    })?;
    ensure!(
        bytes <= MAX_CHUNK_BYTES,
        "generation fragment batch exceeds its byte bound"
    );
    Ok((
        bytes,
        u32::try_from(fragments.len()).context("generation fragment count overflow")?,
    ))
}

fn output_bytes(jobs: &BTreeMap<String, GenerationJob>) -> Result<usize> {
    jobs.values().try_fold(0usize, |sum, job| {
        sum.checked_add(job.output.len())
            .context("generation output size overflow")
    })
}
fn validate_jobs(jobs: &BTreeMap<String, GenerationJob>) -> Result<()> {
    ensure!(jobs.len() <= MAX_JOBS, "generation job capacity exceeded");
    ensure!(
        output_bytes(jobs)? <= MAX_OWNER_OUTPUT_BYTES,
        "generation owner output limit reached"
    );
    ensure!(
        jobs.values().filter(|job| !job.status.terminal()).count() <= MAX_ACTIVE_JOBS,
        "active generation job capacity exceeded"
    );
    for (id, job) in jobs {
        validate_id(id)?;
        validate_id(&job.agent_alias)?;
        ensure!(
            id == &job.job_id
                && job.revision > 0
                && job.committed_chunks <= MAX_FRAMES
                && job.output.len() <= MAX_OUTPUT_BYTES,
            "invalid generation job bounds or identity"
        );
        for hash in [&job.prompt_digest, &job.source_schema_hash] {
            ensure!(
                hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
                "invalid generation provenance"
            );
        }
        ensure!(
            job.created_at >= 0 && job.updated_at >= job.created_at,
            "invalid generation timestamps"
        );
        for label in [&job.reported_provider, &job.reported_model]
            .into_iter()
            .flatten()
        {
            ensure!(
                !label.is_empty() && label.len() <= 256,
                "invalid generation attribution"
            );
        }
        ensure!(
            matches!(
                job.status,
                GenerationStatus::Failed | GenerationStatus::Interrupted
            ) == job.failure.is_some(),
            "generation terminal reason does not match status"
        );
    }
    Ok(())
}

pub fn validate_id(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
        "invalid generation identifier"
    );
    Ok(())
}
fn now() -> Result<i64> {
    let value = chrono::Utc::now().timestamp();
    ensure!(value >= 0, "generation clock predates epoch");
    Ok(value)
}
fn advance(job: &mut GenerationJob) -> Result<()> {
    job.revision = job
        .revision
        .checked_add(1)
        .context("generation job revision overflow")?;
    job.updated_at = now()?.max(job.updated_at);
    Ok(())
}

#[derive(Clone)]
pub struct GenerationMethods {
    documents: Arc<RuntimeDocuments>,
}
impl GenerationMethods {
    pub fn new(documents: Arc<RuntimeDocuments>) -> Self {
        Self { documents }
    }

    pub async fn recover(&self, owner: &GenerationOwner) -> Result<Option<GenerationState>> {
        let Some(row) = self.documents.read_durable(&owner.document_id()).await? else {
            return Ok(None);
        };
        let stored = StoredGenerations::decode(owner, row.document)?;
        if stored.jobs.values().all(|job| job.status.terminal()) {
            return Ok(Some(GenerationState {
                generation_revision: row.revision,
                generation_jobs: stored.jobs,
            }));
        }
        let recovered = self
            .update(owner, Some(row.revision), |stored| {
                for job in stored
                    .jobs
                    .values_mut()
                    .filter(|job| !job.status.terminal())
                {
                    job.status = GenerationStatus::Interrupted;
                    job.failure = Some(GenerationFailure::ProcessInterrupted);
                    advance(job)?;
                }
                Ok(())
            })
            .await?;
        Ok(Some(recovered))
    }

    pub async fn create(
        &self,
        owner: &GenerationOwner,
        input: &StartGenerationInput,
        id: &str,
        schema_hash: &str,
    ) -> Result<GenerationState> {
        input.validate()?;
        validate_id(id)?;
        ensure!(
            schema_hash.len() == 64 && schema_hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid generation schema pin"
        );
        let timestamp = now()?;
        let job = GenerationJob {
            job_id: id.to_owned(),
            revision: 1,
            agent_alias: input.agent_alias.clone(),
            status: GenerationStatus::Queued,
            output: String::new(),
            committed_chunks: 0,
            reported_provider: None,
            reported_model: None,
            prompt_digest: format!("{:x}", Sha256::digest(input.prompt.as_bytes())),
            source_schema_hash: schema_hash.to_owned(),
            created_at: timestamp,
            updated_at: timestamp,
            failure: None,
        };
        self.update(owner, None, move |stored| {
            ensure!(
                !stored.jobs.contains_key(&job.job_id),
                "generation job already exists"
            );
            ensure!(
                stored.jobs.len() < MAX_JOBS,
                "generation job capacity reached; delete a terminal job"
            );
            stored.jobs.insert(job.job_id.clone(), job);
            Ok(())
        })
        .await
    }

    pub async fn running(&self, owner: &GenerationOwner, id: &str) -> Result<GenerationState> {
        self.change_job(owner, id, None, |job| {
            ensure!(
                job.status == GenerationStatus::Queued,
                "generation job is not queued"
            );
            job.status = GenerationStatus::Running;
            advance(job)
        })
        .await
    }

    pub async fn append(
        &self,
        owner: &GenerationOwner,
        id: &str,
        chunk: String,
    ) -> Result<GenerationState> {
        self.append_batch(owner, id, vec![chunk]).await
    }

    /// One durable revision preserves all original nonempty fragment counts.
    pub async fn append_batch(
        &self,
        owner: &GenerationOwner,
        id: &str,
        fragments: Vec<String>,
    ) -> Result<GenerationState> {
        let (bytes, count) = fragment_batch_size(&fragments)?;
        self.change_job(owner, id, None, move |job| {
            ensure!(
                job.status == GenerationStatus::Running,
                "generation job is not running"
            );
            ensure!(
                job.committed_chunks
                    .checked_add(count)
                    .is_some_and(|total| total <= MAX_FRAMES)
                    && job
                        .output
                        .len()
                        .checked_add(bytes)
                        .is_some_and(|size| size <= MAX_OUTPUT_BYTES),
                "generation output limit reached"
            );
            for fragment in fragments {
                job.output.push_str(&fragment);
            }
            job.committed_chunks = job
                .committed_chunks
                .checked_add(count)
                .context("generation committed fragment count overflow")?;
            advance(job)
        })
        .await
    }

    pub async fn request_cancel(
        &self,
        owner: &GenerationOwner,
        id: &str,
        expected: u64,
    ) -> Result<GenerationState> {
        self.change_job(owner, id, Some(expected), |job| {
            ensure!(!job.status.terminal(), "generation job is already terminal");
            job.status = GenerationStatus::CancelRequested;
            advance(job)
        })
        .await
    }

    /// The admitted stateless protocol guarantees final text equals its deltas.
    /// Completion cannot replace committed output with a buffered unary result.
    pub async fn complete(
        &self,
        owner: &GenerationOwner,
        id: &str,
        output: String,
        provider: Option<String>,
        model: Option<String>,
    ) -> Result<GenerationState> {
        ensure!(
            output.len() <= MAX_OUTPUT_BYTES,
            "generation final output exceeds its bound"
        );
        self.change_job(owner, id, None, move |job| {
            ensure!(
                job.status == GenerationStatus::Running,
                "generation is not running"
            );
            ensure!(
                job.output == output,
                "native completion differs from committed deltas"
            );
            job.reported_provider = provider;
            job.reported_model = model;
            job.status = GenerationStatus::Completed;
            job.failure = None;
            advance(job)
        })
        .await
    }

    pub async fn finish(
        &self,
        owner: &GenerationOwner,
        id: &str,
        status: GenerationStatus,
        reason: Option<GenerationFailure>,
    ) -> Result<GenerationState> {
        ensure!(
            status.terminal(),
            "generation completion status is not terminal"
        );
        self.change_job(owner, id, None, move |job| {
            ensure!(!job.status.terminal(), "generation job is already terminal");
            ensure!(
                status != GenerationStatus::Completed || job.status == GenerationStatus::Running,
                "generation completion is not valid after cancellation"
            );
            job.status = status;
            job.failure = reason;
            advance(job)
        })
        .await
    }

    pub async fn delete(
        &self,
        owner: &GenerationOwner,
        id: &str,
        expected: u64,
    ) -> Result<GenerationState> {
        validate_id(id)?;
        let id = id.to_owned();
        self.update(owner, None, move |stored| {
            let job = stored.jobs.get(&id).context("generation job is absent")?;
            ensure!(
                job.revision == expected && expected > 0,
                "generation job revision conflict"
            );
            ensure!(
                job.status.terminal(),
                "active generation job cannot be deleted"
            );
            stored.jobs.remove(&id);
            Ok(())
        })
        .await
    }

    async fn change_job<F>(
        &self,
        owner: &GenerationOwner,
        id: &str,
        expected: Option<u64>,
        change: F,
    ) -> Result<GenerationState>
    where
        F: FnOnce(&mut GenerationJob) -> Result<()> + Send + 'static,
    {
        validate_id(id)?;
        let id = id.to_owned();
        self.update(owner, None, move |stored| {
            let job = stored
                .jobs
                .get_mut(&id)
                .context("generation job is absent")?;
            if let Some(expected) = expected {
                ensure!(
                    expected > 0 && expected == job.revision,
                    "generation job revision conflict"
                );
            }
            change(job)
        })
        .await
    }

    async fn update<F>(
        &self,
        owner: &GenerationOwner,
        revision: Option<u64>,
        change: F,
    ) -> Result<GenerationState>
    where
        F: FnOnce(&mut StoredGenerations) -> Result<()> + Send + 'static,
    {
        let id = owner.document_id();
        let owner = owner.clone();
        let update = move |previous| {
            let mut stored = match previous {
                Some(value) => StoredGenerations::decode(&owner, value)?,
                None => StoredGenerations::empty(&owner),
            };
            change(&mut stored)?;
            validate_jobs(&stored.jobs)?;
            let value = serde_json::to_value(&stored)?;
            ensure!(
                serde_json::to_vec(&value)?.len() <= MAX_DOCUMENT_BYTES,
                "generation document exceeds its bound"
            );
            Ok((value, stored.jobs))
        };
        let (row, jobs) = if let Some(revision) = revision {
            self.documents
                .update_at_revision_with_result(&id, revision, update)
                .await?
        } else {
            self.documents.update_with_result(&id, update).await?
        };
        Ok(GenerationState {
            generation_revision: row.revision,
            generation_jobs: jobs,
        })
    }
}

/// Classify the compiled execution path from these declarations. Runtime
/// existence and admission still come from the caller's pinned sealed plugin.
pub fn is_method(method: &str) -> bool {
    static METHODS: std::sync::LazyLock<std::collections::BTreeSet<String>> =
        std::sync::LazyLock::new(|| {
            let mut schema = op_state_store::PluginSchema::builder("tched_router").build();
            declare_methods(&mut schema);
            schema.methods.into_keys().collect()
        });
    METHODS.contains(method)
}

pub fn declare_methods(schema: &mut op_state_store::PluginSchema) {
    use op_state_store::{CapabilityDecl, SideEffect};
    macro_rules! declare {
        ($name:literal, $input:ty, $output:ty, $side:ident, $cap:ident, $subid:literal) => {
            schema.methods.insert(
                $name.into(),
                method_decl_from_schemars_with_output::<$input, $output>(
                    $name,
                    SideEffect::$side,
                    false,
                    $cap,
                    $subid,
                ),
            );
        };
    }
    declare!(
        "start_generation",
        StartGenerationInput,
        GenerationMutationOutput,
        Mutation,
        START_CAPABILITY,
        "mut.service.tched-router.generation.start@v1"
    );
    declare!(
        "get_generation_job",
        GenerationJobInput,
        GenerationJobOutput,
        Read,
        READ_CAPABILITY,
        "obs.service.tched-router.generation.get@v1"
    );
    declare!(
        "get_generation_jobs",
        GenerationEmptyInput,
        GenerationWorkspaceOutput,
        Read,
        READ_CAPABILITY,
        "obs.service.tched-router.generation.list@v1"
    );
    declare!(
        "cancel_generation",
        GenerationRevisionInput,
        GenerationMutationOutput,
        Mutation,
        CANCEL_CAPABILITY,
        "mut.service.tched-router.generation.cancel@v1"
    );
    declare!(
        "delete_generation_job",
        GenerationRevisionInput,
        GenerationDeleteOutput,
        Mutation,
        DELETE_CAPABILITY,
        "mut.service.tched-router.generation.delete@v1"
    );
    declare!(
        "recover_generation_jobs",
        GenerationEmptyInput,
        GenerationWorkspaceOutput,
        Mutation,
        RECOVER_CAPABILITY,
        "mut.service.tched-router.generation.recover@v1"
    );
    for (id, description) in [
        (
            START_CAPABILITY,
            "Start native generation for this verified principal and session",
        ),
        (
            READ_CAPABILITY,
            "Read this verified principal and session's private generation jobs",
        ),
        (
            CANCEL_CAPABILITY,
            "Request cancellation of this verified principal and session's generation job",
        ),
        (
            DELETE_CAPABILITY,
            "Delete a terminal generation job owned by this principal and session",
        ),
        (
            RECOVER_CAPABILITY,
            "Recover this principal and session's durable generation jobs without replaying prompts",
        ),
    ] {
        schema.capabilities.insert(
            id.into(),
            CapabilityDecl {
                id: id.into(),
                description: description.into(),
            },
        );
    }
    for method in ["get_generation_job", "get_generation_jobs"] {
        schema.methods.get_mut(method).unwrap().idempotent = true;
    }
}

#[cfg(test)]
#[path = "tched_router_generation_tests.rs"]
mod tests;
