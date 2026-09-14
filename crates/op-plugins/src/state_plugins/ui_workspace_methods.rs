//! Durable, principal/session-owned UI generation jobs and validated specs.
//!
//! Cozo (through `RuntimeDocuments`) is durable mutable-domain storage. This
//! adapter performs no SHM publication; MutationEngine publishes the returned
//! committed projection before sending a scoped wakeup.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::super::json_render::{
    canvas_profile::PinnedCanvasValidator,
    validation_bounds::{check_json, JsonLimits},
};
use super::super::runtime_documents::{RuntimeDocumentRejected, RuntimeDocuments};
use super::methods::{self as taxonomy_methods, GemmaTaxonomy, RegisterTagOutput};

const DOCUMENT_PREFIX: &str = "ui_workspace:v1:";
const OWNER_DOMAIN: &[u8] = b"opdbus.ui-workspace.owner.v1\0";
const MAX_JOBS: usize = 32;
const MAX_SPECS: usize = 200;
const MAX_SPEC_BYTES: usize = 1024 * 1024;
const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
const MAX_ELEMENTS: usize = 4096;
const MAX_TAGS: usize = 16;
const MAX_TAG_BYTES: usize = 64;
const MAX_ID_BYTES: usize = 128;

fn reject_precommit<T>(result: Result<T>) -> Result<T> {
    result.map_err(|error| RuntimeDocumentRejected::new(error).into())
}

#[path = "ui_workspace_contract.rs"]
mod contract;
pub use contract::*;

/// A derived storage selector, never a caller-supplied authority or wire ID.
#[derive(Clone, PartialEq, Eq)]
pub struct UiOwnerKey(String);
impl UiOwnerKey {
    pub fn namespace_component(&self) -> &str {
        &self.0
    }
}

impl UiAccessContext {
    pub fn owner_key(&self) -> Result<UiOwnerKey> {
        Ok(UiOwnerKey(Owner::new(self)?.owner_key))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiAccessContext {
    pub principal_id: String,
    pub session_id: String,
    /// Present for authenticated private Gemma state. Legacy generation
    /// helpers may omit it and remain in their separate owner domain.
    pub session_genesis: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiJobStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiJobFailureCode {
    ProviderUnavailable,
    InvalidProviderOutput,
    CatalogChanged,
    /// The process lost the provider execution term. The prompt is not
    /// persisted, so recovery cannot reconstruct or silently resume it.
    Interrupted,
    PublicationFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-opdbus-exact-json-schema" = true))]
pub struct UiGenerationJob {
    pub id: String,
    pub revision: u64,
    pub status: UiJobStatus,
    pub requested_at: i64,
    pub updated_at: i64,
    pub prompt_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_spec_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<UiJobFailureCode>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-opdbus-exact-json-schema" = true))]
pub struct UiSpecRecord {
    pub id: String,
    pub revision: u64,
    pub catalog_hash: String,
    pub spec: Value,
    pub tags: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct StoredUiWorkspace {
    format_version: u32,
    principal_id: String,
    session_id: String,
    #[serde(default)]
    session_genesis: Option<String>,
    owner_key: String,
    catalog_hash: String,
    jobs: BTreeMap<String, UiGenerationJob>,
    specs: BTreeMap<String, UiSpecRecord>,
    #[serde(default)]
    taxonomy: GemmaTaxonomy,
}

/// Private values suitable for MutationEngine's principal-scoped projection.
/// Raw principal/session identifiers never enter the projection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiWorkspaceProjection {
    pub catalog_hash: String,
    pub jobs: BTreeMap<String, UiGenerationJob>,
    pub specs: BTreeMap<String, UiSpecRecord>,
    pub tags: BTreeMap<String, super::methods::GemmaTag>,
    pub taxonomy_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiMutationReceipt {
    SpecCreated {
        spec: UiSpecRecord,
    },
    JobCreated {
        job: UiGenerationJob,
        /// Terminal jobs removed to make room. Their referenced specs remain.
        pruned_job_ids: Vec<String>,
    },
    JobRunning {
        job: UiGenerationJob,
    },
    JobFailed {
        job: UiGenerationJob,
    },
    SpecCompleted {
        job: UiGenerationJob,
        spec: UiSpecRecord,
    },
    SpecUpdated {
        spec: UiSpecRecord,
    },
    SpecDeleted {
        spec_id: String,
        deleted_revision: u64,
    },
    JobDeleted {
        job_id: String,
        deleted_revision: u64,
    },
    RendererRebound {
        previous_catalog_hash: String,
        catalog_hash: String,
    },
    TagRegistered {
        output: RegisterTagOutput,
    },
    EventHandled {
        spec: UiSpecRecord,
        event: String,
        action: String,
    },
}

/// Exact Cozo commit returned to MutationEngine. `projection` is derived from
/// the same row returned by `update_with_result`, never a later racy read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiWorkspaceCommit {
    pub document_revision: u64,
    pub receipt: UiMutationReceipt,
    pub projection: UiWorkspaceProjection,
}

/// Durable row recovered from the injected `RuntimeDocuments` owner.
///
/// This receipt proves only that the retained workspace passed its immutable
/// format, owner, provenance, and aggregate-size checks at `document_revision`.
/// A retained spec whose `catalog_hash` differs from the validator's current
/// renderer-contract hash remains eligible for deletion and audit display, but
/// is not admitted for new rendering or mutation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UiWorkspaceRecovery {
    pub document_revision: u64,
    pub projection: UiWorkspaceProjection,
}

pub trait UiSpecValidator: Send + Sync {
    /// Immutable hash of the exact sealed renderer contract used for new-spec
    /// validation by this handle. This should not be a hash of unrelated
    /// plugins in the wider catalog.
    fn catalog_hash(&self) -> &str;

    /// Pure, deterministic validation against the injected pinned catalog.
    /// Implementations must perform no I/O and consult no mutable catalog or
    /// caller-selected schema/component registry.
    fn validate_spec(&self, spec: &Value) -> Result<()>;

    fn apply_event(&self, _spec: &Value, _element_id: &str, _event: &str) -> Result<Value> {
        anyhow::bail!("renderer does not implement server event reduction")
    }
}

impl UiSpecValidator for PinnedCanvasValidator {
    fn catalog_hash(&self) -> &str {
        self.schema_hash()
    }
    fn validate_spec(&self, spec: &Value) -> Result<()> {
        self.validate(spec)
    }
    fn apply_event(&self, spec: &Value, element_id: &str, event: &str) -> Result<Value> {
        PinnedCanvasValidator::apply_event(self, spec, element_id, event)
    }
}

pub struct UiWorkspaceMethods {
    documents: Arc<RuntimeDocuments>,
    validator: Arc<dyn UiSpecValidator>,
    catalog_hash: String,
}

impl UiWorkspaceMethods {
    /// The bridge supplies original bytes/hash from one manifest-pinned
    /// json_render blob after compiled-state admission. No runtime hash or
    /// fallback registry is constructed here.
    pub fn from_manifest_pinned_renderer(
        documents: Arc<RuntimeDocuments>,
        schema_json: Arc<str>,
        schema_hash: String,
    ) -> Result<Self> {
        Self::new(
            documents,
            Arc::new(PinnedCanvasValidator::from_manifest_pinned_schema(
                schema_json,
                schema_hash,
            )?),
        )
    }

    fn new(documents: Arc<RuntimeDocuments>, validator: Arc<dyn UiSpecValidator>) -> Result<Self> {
        let catalog_hash = validator.catalog_hash().to_string();
        validate_digest("catalog_hash", &catalog_hash)?;
        Ok(Self {
            documents,
            validator,
            catalog_hash,
        })
    }

    pub fn catalog_hash(&self) -> &str {
        &self.catalog_hash
    }

    /// Provider-independent creation; the engine, not the caller, mints spec_id.
    pub async fn create_spec(
        &self,
        access: &UiAccessContext,
        spec_id: &str,
        spec: Value,
        tags: Vec<String>,
    ) -> Result<UiWorkspaceCommit> {
        reject_precommit(validate_id("spec_id", spec_id))?;
        reject_precommit(validate_tags(&tags))?;
        reject_precommit(validate_spec_bounds(&spec))?;
        reject_precommit(self.validator.validate_spec(&spec))?;
        let id = spec_id.to_owned();
        let catalog_hash = self.catalog_hash.clone();
        self.update(
            reject_precommit(Owner::new(access))?,
            move |mut workspace| {
                ensure!(
                    workspace.specs.len() < MAX_SPECS,
                    "UI spec capacity reached"
                );
                ensure!(!workspace.specs.contains_key(&id), "UI spec already exists");
                let now = now_seconds()?;
                let record = UiSpecRecord {
                    id: id.clone(),
                    revision: 1,
                    catalog_hash,
                    spec,
                    tags,
                    created_at: now,
                    updated_at: now,
                };
                workspace.specs.insert(id, record.clone());
                Ok((workspace, UiMutationReceipt::SpecCreated { spec: record }))
            },
        )
        .await
    }

    /// Explicitly delete terminal job history; never cancel or remove an
    /// active provider job, and never implicitly delete its result spec.
    pub async fn delete_job(
        &self,
        access: &UiAccessContext,
        job_id: &str,
        expected_revision: u64,
    ) -> Result<UiWorkspaceCommit> {
        reject_precommit(validate_id("job_id", job_id))?;
        reject_precommit(validate_expected_revision(expected_revision))?;
        let job_id = job_id.to_owned();
        self.update_with_admission(
            reject_precommit(Owner::new(access))?,
            UpdateAdmission::RetainedCleanup,
            move |mut workspace| {
                let job = workspace.jobs.get(&job_id).context("UI job not found")?;
                ensure!(job.revision == expected_revision, "stale UI job revision");
                ensure!(
                    matches!(job.status, UiJobStatus::Completed | UiJobStatus::Failed),
                    "active UI job cannot be deleted"
                );
                workspace.jobs.remove(&job_id);
                Ok((
                    workspace,
                    UiMutationReceipt::JobDeleted {
                        job_id,
                        deleted_revision: expected_revision,
                    },
                ))
            },
        )
        .await
    }

    /// Explicit rebind only for a truly empty existing workspace. Neither
    /// artifacts nor history are silently removed or given a new provenance.
    /// Exact document revision is checked inside RuntimeDocuments' owned CAS.
    pub async fn rebind_renderer(
        &self,
        access: &UiAccessContext,
        expected_document_revision: u64,
        expected_previous_catalog_hash: &str,
    ) -> Result<UiWorkspaceCommit> {
        reject_precommit(validate_expected_revision(expected_document_revision))?;
        reject_precommit(validate_digest(
            "expected renderer hash",
            expected_previous_catalog_hash,
        ))?;
        let owner = reject_precommit(Owner::new(access))?;
        let previous_hash = expected_previous_catalog_hash.to_owned();
        let new_hash = self.catalog_hash.clone();
        reject_precommit(
            (previous_hash != new_hash)
                .then_some(())
                .context("renderer contract is unchanged"),
        )?;
        let document_id = owner.document_id.clone();
        let (row, (receipt, projection)) = self
            .documents
            .update_at_revision_with_result(&document_id, expected_document_revision, move |old| {
                let mut workspace =
                    decode_retained_workspace(&owner, old.context("UI workspace absent")?)?;
                ensure!(
                    workspace.catalog_hash == previous_hash,
                    "stale renderer provenance"
                );
                ensure!(
                    workspace.jobs.is_empty() && workspace.specs.is_empty(),
                    "renderer rebind requires an empty workspace"
                );
                workspace.catalog_hash = new_hash.clone();
                validate_retained_workspace(&owner, &workspace)?;
                let projection = workspace.projection();
                let document = serde_json::to_value(workspace)?;
                validate_document_bound(&document)?;
                Ok((
                    document,
                    (
                        UiMutationReceipt::RendererRebound {
                            previous_catalog_hash: previous_hash,
                            catalog_hash: new_hash,
                        },
                        projection,
                    ),
                ))
            })
            .await?;
        Ok(UiWorkspaceCommit {
            document_revision: row.revision,
            receipt,
            projection,
        })
    }

    /// Durable-domain management view. Canonical runtime methods and streams
    /// read engine-committed private SHM; they must not substitute this read.
    pub async fn list_specs(
        &self,
        access: &UiAccessContext,
        input: UiListSpecsInput,
    ) -> Result<UiListSpecsOutput> {
        let limit = if input.limit == 0 {
            50
        } else {
            input.limit as usize
        };
        ensure!(limit <= MAX_SPECS, "UI list limit exceeds capacity");
        if let Some(cursor) = &input.after {
            validate_id("cursor", cursor)?;
        }
        let snapshot = self.snapshot(access).await?;
        let mut page = page_specs(
            snapshot.as_ref(),
            input.after.as_deref(),
            limit,
            &self.catalog_hash,
        );
        for item in &mut page.specs {
            item.render_admitted = self.record_is_render_admitted(&item.record);
        }
        Ok(page)
    }

    /// Pure presentation over a record selected from committed private SHM.
    /// Matching a stored hash alone never validates corrupted/foreign props.
    pub fn spec_view(&self, spec: Option<UiSpecRecord>) -> UiSpecOutput {
        let render_admitted = spec
            .as_ref()
            .is_some_and(|record| self.record_is_render_admitted(record));
        UiSpecOutput {
            spec,
            render_admitted,
        }
    }

    fn record_is_render_admitted(&self, record: &UiSpecRecord) -> bool {
        record.catalog_hash == self.catalog_hash
            && self.validator.validate_spec(&record.spec).is_ok()
    }

    pub async fn snapshot(
        &self,
        access: &UiAccessContext,
    ) -> Result<Option<UiWorkspaceProjection>> {
        let owner = Owner::new(access)?;
        self.documents
            .read(&owner.document_id)
            .await?
            .map(|row| self.retained_projection(&owner, row.document))
            .transpose()
    }

    /// Reconcile an uncertain publication from the exact durable Cozo row.
    /// MutationEngine may publish this projection with the returned revision;
    /// it must never substitute a later ordinary read for a successful commit
    /// receipt or publish an older revision over a newer owner-scoped SHM row.
    pub async fn recover_snapshot(
        &self,
        access: &UiAccessContext,
    ) -> Result<Option<UiWorkspaceRecovery>> {
        let owner = Owner::new(access)?;
        self.documents
            .read_durable(&owner.document_id)
            .await?
            .map(|row| {
                Ok(UiWorkspaceRecovery {
                    document_revision: row.revision,
                    projection: self.retained_projection(&owner, row.document)?,
                })
            })
            .transpose()
    }

    pub async fn get_job(
        &self,
        access: &UiAccessContext,
        job_id: &str,
    ) -> Result<Option<UiGenerationJob>> {
        validate_id("job_id", job_id)?;
        Ok(self
            .snapshot(access)
            .await?
            .and_then(|snapshot| snapshot.jobs.get(job_id).cloned()))
    }

    pub async fn get_spec(
        &self,
        access: &UiAccessContext,
        spec_id: &str,
    ) -> Result<Option<UiSpecRecord>> {
        // This is a retained-artifact management read. The record's immutable
        // catalog_hash must equal the workspace provenance, but callers must
        // not treat it as approval to render under the current catalog.
        validate_id("spec_id", spec_id)?;
        Ok(self
            .snapshot(access)
            .await?
            .and_then(|snapshot| snapshot.specs.get(spec_id).cloned()))
    }

    pub async fn create_job(
        &self,
        access: &UiAccessContext,
        job_id: &str,
        prompt_digest: &str,
    ) -> Result<UiWorkspaceCommit> {
        reject_precommit(validate_id("job_id", job_id))?;
        reject_precommit(validate_digest("prompt_digest", prompt_digest))?;
        let owner = reject_precommit(Owner::new(access))?;
        let catalog_hash = self.catalog_hash.clone();
        let job_id = job_id.to_string();
        let prompt_digest = prompt_digest.to_string();
        self.update(owner, move |mut workspace| {
            ensure!(
                !workspace.jobs.contains_key(&job_id),
                "UI job already exists"
            );
            let pruned_job_ids = retain_job_capacity(&mut workspace.jobs)?;
            let now = now_seconds()?;
            let job = UiGenerationJob {
                id: job_id.clone(),
                revision: 1,
                status: UiJobStatus::Pending,
                requested_at: now,
                updated_at: now,
                prompt_digest,
                result_spec_id: None,
                error_code: None,
            };
            ensure!(workspace.catalog_hash == catalog_hash, "catalog changed");
            workspace.jobs.insert(job_id, job.clone());
            Ok((
                workspace,
                UiMutationReceipt::JobCreated {
                    job,
                    pruned_job_ids,
                },
            ))
        })
        .await
    }

    pub async fn mark_job_running(
        &self,
        access: &UiAccessContext,
        job_id: &str,
        expected_revision: u64,
    ) -> Result<UiWorkspaceCommit> {
        reject_precommit(validate_id("job_id", job_id))?;
        reject_precommit(validate_expected_revision(expected_revision))?;
        let owner = reject_precommit(Owner::new(access))?;
        let job_id = job_id.to_string();
        self.update(owner, move |mut workspace| {
            let job = workspace
                .jobs
                .get_mut(&job_id)
                .context("UI job not found")?;
            ensure!(job.revision == expected_revision, "stale UI job revision");
            ensure!(job.status == UiJobStatus::Pending, "UI job is not pending");
            job.revision = checked_next(job.revision, "UI job revision")?;
            job.status = UiJobStatus::Running;
            job.updated_at = now_seconds()?;
            let job = job.clone();
            Ok((workspace, UiMutationReceipt::JobRunning { job }))
        })
        .await
    }

    pub async fn fail_job(
        &self,
        access: &UiAccessContext,
        job_id: &str,
        expected_revision: u64,
        error_code: UiJobFailureCode,
    ) -> Result<UiWorkspaceCommit> {
        reject_precommit(validate_id("job_id", job_id))?;
        reject_precommit(validate_expected_revision(expected_revision))?;
        let owner = reject_precommit(Owner::new(access))?;
        let job_id = job_id.to_string();
        let current_catalog = self.catalog_hash.clone();
        let admission = if error_code == UiJobFailureCode::CatalogChanged {
            UpdateAdmission::RetainedCatalogChange
        } else {
            UpdateAdmission::CurrentCatalog
        };
        self.update_with_admission(owner, admission, move |mut workspace| {
            if error_code == UiJobFailureCode::CatalogChanged {
                ensure!(
                    workspace.catalog_hash != current_catalog,
                    "UI catalog has not changed"
                );
            }
            let job = workspace
                .jobs
                .get_mut(&job_id)
                .context("UI job not found")?;
            ensure!(job.revision == expected_revision, "stale UI job revision");
            ensure!(
                matches!(job.status, UiJobStatus::Pending | UiJobStatus::Running),
                "UI job is already terminal"
            );
            job.revision = checked_next(job.revision, "UI job revision")?;
            job.status = UiJobStatus::Failed;
            job.updated_at = now_seconds()?;
            job.result_spec_id = None;
            job.error_code = Some(error_code);
            let job = job.clone();
            Ok((workspace, UiMutationReceipt::JobFailed { job }))
        })
        .await
    }

    pub async fn complete_job(
        &self,
        access: &UiAccessContext,
        job_id: &str,
        expected_job_revision: u64,
        spec_id: &str,
        spec: Value,
        tags: Vec<String>,
    ) -> Result<UiWorkspaceCommit> {
        reject_precommit(validate_id("job_id", job_id))?;
        reject_precommit(validate_id("spec_id", spec_id))?;
        reject_precommit(validate_expected_revision(expected_job_revision))?;
        reject_precommit(validate_tags(&tags))?;
        reject_precommit(validate_spec_bounds(&spec))?;

        let owner = reject_precommit(Owner::new(access))?;
        let job_id = job_id.to_string();
        let spec_id = spec_id.to_string();
        let catalog_hash = self.catalog_hash.clone();
        self.update(owner, move |mut workspace| {
            ensure!(
                workspace.specs.len() < MAX_SPECS,
                "UI spec capacity reached"
            );
            ensure!(
                !workspace.specs.contains_key(&spec_id),
                "UI spec already exists"
            );
            let job = workspace
                .jobs
                .get_mut(&job_id)
                .context("UI job not found")?;
            ensure!(
                job.revision == expected_job_revision,
                "stale UI job revision"
            );
            ensure!(job.status == UiJobStatus::Running, "UI job is not running");
            let now = now_seconds()?;
            let record = UiSpecRecord {
                id: spec_id.clone(),
                revision: 1,
                catalog_hash,
                spec,
                tags,
                created_at: now,
                updated_at: now,
            };
            job.revision = checked_next(job.revision, "UI job revision")?;
            job.status = UiJobStatus::Completed;
            job.updated_at = now;
            job.result_spec_id = Some(spec_id.clone());
            job.error_code = None;
            let job = job.clone();
            workspace.specs.insert(spec_id, record.clone());
            Ok((
                workspace,
                UiMutationReceipt::SpecCompleted { job, spec: record },
            ))
        })
        .await
    }

    pub async fn update_spec_properties(
        &self,
        access: &UiAccessContext,
        spec_id: &str,
        expected_revision: u64,
        element_id: &str,
        properties: Value,
    ) -> Result<UiWorkspaceCommit> {
        reject_precommit(validate_id("spec_id", spec_id))?;
        reject_precommit(validate_id("element_id", element_id))?;
        reject_precommit(validate_expected_revision(expected_revision))?;
        reject_precommit(
            properties
                .is_object()
                .then_some(())
                .context("component properties must be an object"),
        )?;
        let current = reject_precommit(
            self.get_spec(access, spec_id)
                .await
                .and_then(|spec| spec.context("UI spec not found")),
        )?;
        reject_precommit(
            (current.revision == expected_revision)
                .then_some(())
                .context("stale UI spec revision"),
        )?;
        let mut candidate = current.spec.clone();
        let element = reject_precommit(
            candidate
                .get_mut("elements")
                .and_then(Value::as_object_mut)
                .and_then(|elements| elements.get_mut(element_id))
                .and_then(Value::as_object_mut)
                .context("UI element not found"),
        )?;
        element.insert("props".to_string(), properties);
        reject_precommit(validate_spec_bounds(&candidate))?;

        let owner = reject_precommit(Owner::new(access))?;
        let spec_id = spec_id.to_string();
        let catalog_hash = self.catalog_hash.clone();
        self.update(owner, move |mut workspace| {
            let record = workspace
                .specs
                .get_mut(&spec_id)
                .context("UI spec not found")?;
            ensure!(
                record.revision == expected_revision,
                "stale UI spec revision"
            );
            ensure!(
                record.catalog_hash == catalog_hash,
                "UI spec catalog changed"
            );
            record.revision = checked_next(record.revision, "UI spec revision")?;
            record.spec = candidate;
            record.updated_at = now_seconds()?;
            let record = record.clone();
            Ok((workspace, UiMutationReceipt::SpecUpdated { spec: record }))
        })
        .await
    }

    pub async fn handle_event(
        &self,
        access: &UiAccessContext,
        spec_id: &str,
        expected_revision: u64,
        element_id: &str,
        event: &str,
    ) -> Result<UiWorkspaceCommit> {
        reject_precommit(validate_id("spec_id", spec_id))?;
        reject_precommit(validate_expected_revision(expected_revision))?;
        reject_precommit(validate_id("element_id", element_id))?;
        reject_precommit(validate_id("event", event))?;
        let current = reject_precommit(
            self.get_spec(access, spec_id)
                .await
                .and_then(|spec| spec.context("UI spec not found")),
        )?;
        reject_precommit(
            (current.revision == expected_revision)
                .then_some(())
                .context("stale UI spec revision"),
        )?;
        reject_precommit(
            (current.catalog_hash == self.catalog_hash)
                .then_some(())
                .context("UI spec catalog changed"),
        )?;
        let candidate =
            reject_precommit(self.validator.apply_event(&current.spec, element_id, event))?;
        reject_precommit(validate_spec_bounds(&candidate))?;

        let owner = reject_precommit(Owner::new(access))?;
        let spec_id = spec_id.to_owned();
        let event = event.to_owned();
        let catalog_hash = self.catalog_hash.clone();
        self.update(owner, move |mut workspace| {
            let record = workspace
                .specs
                .get_mut(&spec_id)
                .context("UI spec not found")?;
            ensure!(
                record.revision == expected_revision,
                "stale UI spec revision"
            );
            ensure!(
                record.catalog_hash == catalog_hash,
                "UI spec catalog changed"
            );
            record.revision = checked_next(record.revision, "UI spec revision")?;
            record.spec = candidate;
            record.updated_at = now_seconds()?;
            let spec = record.clone();
            Ok((
                workspace,
                UiMutationReceipt::EventHandled {
                    spec,
                    event,
                    action: "setState".into(),
                },
            ))
        })
        .await
    }

    pub async fn delete_spec(
        &self,
        access: &UiAccessContext,
        spec_id: &str,
        expected_revision: u64,
    ) -> Result<UiWorkspaceCommit> {
        reject_precommit(validate_id("spec_id", spec_id))?;
        reject_precommit(validate_expected_revision(expected_revision))?;
        let owner = reject_precommit(Owner::new(access))?;
        let spec_id = spec_id.to_string();
        self.update_with_admission(
            owner,
            UpdateAdmission::RetainedCleanup,
            move |mut workspace| {
                let record = workspace.specs.get(&spec_id).context("UI spec not found")?;
                ensure!(
                    record.revision == expected_revision,
                    "stale UI spec revision"
                );
                workspace.specs.remove(&spec_id);
                for job in workspace.jobs.values_mut() {
                    if job.result_spec_id.as_deref() == Some(spec_id.as_str()) {
                        job.result_spec_id = None;
                        job.revision = checked_next(job.revision, "UI job revision")?;
                        job.updated_at = now_seconds()?;
                    }
                }
                Ok((
                    workspace,
                    UiMutationReceipt::SpecDeleted {
                        spec_id,
                        deleted_revision: expected_revision,
                    },
                ))
            },
        )
        .await
    }

    async fn update<F>(&self, owner: Owner, change: F) -> Result<UiWorkspaceCommit>
    where
        F: FnOnce(StoredUiWorkspace) -> Result<(StoredUiWorkspace, UiMutationReceipt)>
            + Send
            + 'static,
    {
        self.update_with_admission(owner, UpdateAdmission::CurrentCatalog, change)
            .await
    }

    async fn update_with_admission<F>(
        &self,
        owner: Owner,
        admission: UpdateAdmission,
        change: F,
    ) -> Result<UiWorkspaceCommit>
    where
        F: FnOnce(StoredUiWorkspace) -> Result<(StoredUiWorkspace, UiMutationReceipt)>
            + Send
            + 'static,
    {
        let expected_catalog = self.catalog_hash.clone();
        let validator = Arc::clone(&self.validator);
        let document_id = owner.document_id.clone();
        let decode_owner = owner.clone();
        let (row, (receipt, projection)) = self
            .documents
            .update_with_result(&document_id, move |old| {
                let workspace = match old {
                    Some(value) => decode_retained_workspace(&decode_owner, value)?,
                    None => StoredUiWorkspace::new(&decode_owner, &expected_catalog),
                };
                admit_update(admission, &expected_catalog, &workspace)?;
                let retained_catalog = workspace.catalog_hash.clone();
                let (workspace, receipt) = change(workspace)?;
                validate_retained_workspace(&decode_owner, &workspace)?;
                if admission == UpdateAdmission::CurrentCatalog {
                    admit_current_catalog(&expected_catalog, &workspace)?;
                    validate_specs(validator.as_ref(), &workspace)?;
                } else {
                    ensure!(
                        workspace.catalog_hash == retained_catalog,
                        "retained UI catalog provenance is immutable"
                    );
                }
                let projection = workspace.projection();
                let document = serde_json::to_value(&workspace)?;
                validate_document_bound(&document)?;
                Ok((document, (receipt, projection)))
            })
            .await?;
        Ok(UiWorkspaceCommit {
            document_revision: row.revision,
            receipt,
            projection,
        })
    }

    fn retained_projection(&self, owner: &Owner, value: Value) -> Result<UiWorkspaceProjection> {
        let workspace = decode_retained_workspace(owner, value)?;
        Ok(workspace.projection())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateAdmission {
    CurrentCatalog,
    RetainedCatalogChange,
    RetainedCleanup,
}

#[derive(Debug, Clone)]
struct Owner {
    principal_id: String,
    session_id: String,
    session_genesis: Option<String>,
    owner_key: String,
    document_id: String,
}

impl Owner {
    fn new(access: &UiAccessContext) -> Result<Self> {
        validate_identity("principal_id", &access.principal_id)?;
        validate_identity("session_id", &access.session_id)?;
        if let Some(genesis) = &access.session_genesis {
            validate_identity("session_genesis", genesis)?;
        }
        let mut hash = Sha256::new();
        hash.update(OWNER_DOMAIN);
        update_length_prefixed(&mut hash, access.principal_id.as_bytes());
        update_length_prefixed(&mut hash, access.session_id.as_bytes());
        update_length_prefixed(
            &mut hash,
            access
                .session_genesis
                .as_deref()
                .unwrap_or_default()
                .as_bytes(),
        );
        let owner_key = format!("{:x}", hash.finalize());
        Ok(Self {
            principal_id: access.principal_id.clone(),
            session_id: access.session_id.clone(),
            session_genesis: access.session_genesis.clone(),
            document_id: format!("{DOCUMENT_PREFIX}{owner_key}"),
            owner_key,
        })
    }
}

impl StoredUiWorkspace {
    fn new(owner: &Owner, catalog_hash: &str) -> Self {
        Self {
            format_version: 1,
            principal_id: owner.principal_id.clone(),
            session_id: owner.session_id.clone(),
            session_genesis: owner.session_genesis.clone(),
            owner_key: owner.owner_key.clone(),
            catalog_hash: catalog_hash.to_string(),
            jobs: BTreeMap::new(),
            specs: BTreeMap::new(),
            taxonomy: GemmaTaxonomy::default(),
        }
    }

    fn projection(&self) -> UiWorkspaceProjection {
        UiWorkspaceProjection {
            catalog_hash: self.catalog_hash.clone(),
            jobs: self.jobs.clone(),
            specs: self.specs.clone(),
            tags: self.taxonomy.tags.clone(),
            taxonomy_revision: self.taxonomy.revision,
        }
    }
}

fn decode_retained_workspace(owner: &Owner, value: Value) -> Result<StoredUiWorkspace> {
    validate_document_bound(&value)?;
    let workspace: StoredUiWorkspace = serde_json::from_value(value)
        .map_err(|_| anyhow::anyhow!("invalid stored UI workspace"))?;
    validate_retained_workspace(owner, &workspace)?;
    Ok(workspace)
}

fn validate_retained_workspace(owner: &Owner, workspace: &StoredUiWorkspace) -> Result<()> {
    ensure!(
        workspace.format_version == 1,
        "unsupported UI workspace version"
    );
    ensure!(
        workspace.principal_id == owner.principal_id
            && workspace.session_id == owner.session_id
            && workspace.session_genesis == owner.session_genesis
            && workspace.owner_key == owner.owner_key,
        "UI workspace owner mismatch"
    );
    validate_workspace_content(&workspace.catalog_hash, &workspace.jobs, &workspace.specs)?;
    taxonomy_methods::validate_taxonomy(&workspace.taxonomy)
}

fn validate_workspace_content(
    catalog_hash: &str,
    jobs: &BTreeMap<String, UiGenerationJob>,
    specs: &BTreeMap<String, UiSpecRecord>,
) -> Result<()> {
    validate_digest("workspace catalog_hash", catalog_hash)?;
    ensure!(jobs.len() <= MAX_JOBS, "UI job capacity exceeded");
    ensure!(specs.len() <= MAX_SPECS, "UI spec capacity exceeded");
    for (id, job) in jobs {
        ensure!(id == &job.id, "UI job map key mismatch");
        validate_id("job_id", id)?;
        validate_digest("prompt_digest", &job.prompt_digest)?;
        ensure!(job.revision > 0, "invalid UI job revision");
        if let Some(spec_id) = &job.result_spec_id {
            validate_id("result_spec_id", spec_id)?;
            ensure!(
                specs.contains_key(spec_id),
                "UI job references an absent spec"
            );
        }
        match job.status {
            UiJobStatus::Pending | UiJobStatus::Running => ensure!(
                job.result_spec_id.is_none() && job.error_code.is_none(),
                "active UI job contains terminal fields"
            ),
            UiJobStatus::Completed => ensure!(
                job.error_code.is_none(),
                "completed UI job contains an error"
            ),
            UiJobStatus::Failed => ensure!(
                job.result_spec_id.is_none() && job.error_code.is_some(),
                "failed UI job has invalid terminal fields"
            ),
        }
    }
    for (id, spec) in specs {
        ensure!(id == &spec.id, "UI spec map key mismatch");
        validate_id("spec_id", id)?;
        ensure!(spec.revision > 0, "invalid UI spec revision");
        validate_digest("UI spec catalog_hash", &spec.catalog_hash)?;
        ensure!(
            spec.catalog_hash == catalog_hash,
            "UI spec provenance differs from its retained workspace"
        );
        validate_tags(&spec.tags)?;
        validate_spec_bounds(&spec.spec)?;
    }
    Ok(())
}

fn admit_current_catalog(catalog_hash: &str, workspace: &StoredUiWorkspace) -> Result<()> {
    ensure!(
        workspace.catalog_hash == catalog_hash,
        "UI renderer contract changed; retained artifacts require recovery or deletion"
    );
    Ok(())
}

fn admit_update(
    admission: UpdateAdmission,
    catalog_hash: &str,
    workspace: &StoredUiWorkspace,
) -> Result<()> {
    match admission {
        UpdateAdmission::CurrentCatalog => admit_current_catalog(catalog_hash, workspace)?,
        UpdateAdmission::RetainedCatalogChange => ensure!(
            workspace.catalog_hash != catalog_hash,
            "UI catalog has not changed"
        ),
        UpdateAdmission::RetainedCleanup => {}
    }
    Ok(())
}

/// Count the encoded aggregate without allocating another document-sized
/// buffer. `RuntimeDocuments` already returns a parsed JSON value; this gate
/// runs before decoding that value into the workspace domain type.
fn validate_document_bound(value: &Value) -> Result<()> {
    // The workspace envelope adds a bounded number of typed object layers
    // around each separately depth64-bounded spec.
    check_json(
        value,
        JsonLimits {
            max_depth: 72,
            max_nodes: 524_288,
            max_bytes: MAX_DOCUMENT_BYTES,
        },
    )
    .context("UI workspace exceeds aggregate read/write bound")
}

fn validate_specs(validator: &dyn UiSpecValidator, workspace: &StoredUiWorkspace) -> Result<()> {
    for record in workspace.specs.values() {
        validator
            .validate_spec(&record.spec)
            .with_context(|| format!("stored UI spec '{}' is invalid", record.id))?;
    }
    Ok(())
}

fn retain_job_capacity(jobs: &mut BTreeMap<String, UiGenerationJob>) -> Result<Vec<String>> {
    let mut pruned_job_ids = Vec::new();
    while jobs.len() >= MAX_JOBS {
        let candidate = jobs
            .values()
            .filter(|job| matches!(job.status, UiJobStatus::Completed | UiJobStatus::Failed))
            .min_by(|left, right| {
                (left.requested_at, left.id.as_str()).cmp(&(right.requested_at, right.id.as_str()))
            })
            .map(|job| job.id.clone())
            .context("UI job capacity reached with no terminal job eligible for retention")?;
        jobs.remove(&candidate);
        pruned_job_ids.push(candidate);
    }
    Ok(pruned_job_ids)
}

fn update_length_prefixed(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_be_bytes());
    hash.update(bytes);
}

fn validate_identity(field: &str, value: &str) -> Result<()> {
    ensure!(!value.is_empty(), "{field} is required");
    ensure!(value.len() <= 512, "{field} is too long");
    ensure!(!value.chars().any(char::is_control), "{field} is invalid");
    Ok(())
}

fn validate_id(field: &str, value: &str) -> Result<()> {
    ensure!(!value.is_empty(), "{field} is required");
    ensure!(value.len() <= MAX_ID_BYTES, "{field} is too long");
    ensure!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')),
        "{field} is invalid"
    );
    Ok(())
}

fn validate_digest(field: &str, value: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{field} must be a lowercase SHA-256 digest"
    );
    Ok(())
}

fn validate_expected_revision(value: u64) -> Result<()> {
    ensure!(value > 0, "expected_revision must be positive");
    Ok(())
}

fn checked_next(value: u64, field: &str) -> Result<u64> {
    value
        .checked_add(1)
        .with_context(|| format!("{field} exhausted"))
}

fn validate_tags(tags: &[String]) -> Result<()> {
    ensure!(tags.len() <= MAX_TAGS, "too many UI spec tags");
    let mut unique = BTreeSet::new();
    for tag in tags {
        ensure!(
            !tag.is_empty() && tag.len() <= MAX_TAG_BYTES,
            "invalid UI spec tag"
        );
        ensure!(!tag.chars().any(char::is_control), "invalid UI spec tag");
        ensure!(unique.insert(tag), "duplicate UI spec tag");
    }
    Ok(())
}

fn validate_spec_bounds(spec: &Value) -> Result<()> {
    check_json(
        spec,
        JsonLimits {
            max_depth: 64,
            max_nodes: 65_536,
            max_bytes: MAX_SPEC_BYTES,
        },
    )?;
    let root = spec
        .get("root")
        .and_then(Value::as_str)
        .context("UI spec has no root")?;
    validate_id("UI spec root", root)?;
    let elements = spec
        .get("elements")
        .and_then(Value::as_object)
        .context("UI spec has no elements object")?;
    ensure!(!elements.is_empty(), "UI spec elements are empty");
    ensure!(
        elements.len() <= MAX_ELEMENTS,
        "UI spec has too many elements"
    );
    ensure!(
        elements.contains_key(root),
        "UI spec root element is absent"
    );
    Ok(())
}

fn now_seconds() -> Result<i64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs()
        .try_into()
        .context("system clock exceeds supported range")
}

#[cfg(test)]
#[path = "ui_workspace_production_tests.rs"]
mod production_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use op_cozo_store::CozoGraphShuttle;

    const CATALOG_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const NEW_CATALOG_HASH: &str =
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const PROMPT_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    struct FixtureValidator;

    struct NewCatalogValidator;

    impl UiSpecValidator for FixtureValidator {
        fn catalog_hash(&self) -> &str {
            CATALOG_HASH
        }

        fn validate_spec(&self, spec: &Value) -> Result<()> {
            let elements = spec["elements"]
                .as_object()
                .context("fixture elements missing")?;
            for element in elements.values() {
                ensure!(element["type"] == "text", "unknown fixture component");
                ensure!(element["props"]["text"].is_string(), "text prop missing");
            }
            Ok(())
        }
    }

    impl UiSpecValidator for NewCatalogValidator {
        fn catalog_hash(&self) -> &str {
            NEW_CATALOG_HASH
        }

        fn validate_spec(&self, _spec: &Value) -> Result<()> {
            anyhow::bail!("old spec must not be revalidated as a new-catalog render")
        }
    }

    fn access(principal: &str, session: &str) -> UiAccessContext {
        UiAccessContext {
            principal_id: principal.to_string(),
            session_id: session.to_string(),
            session_genesis: None,
        }
    }

    #[tokio::test]
    async fn taxonomy_is_durable_projected_and_genesis_scoped() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let owner = UiAccessContext {
            principal_id: "principal".into(),
            session_id: "session".into(),
            session_genesis: Some("genesis-a".into()),
        };
        let commit = service
            .dispatch_mutation(
                &owner,
                "register_tag",
                &serde_json::json!({
                    "expected_taxonomy_revision":0,
                    "tag":{
                        "id":"deploy",
                        "label":"Deploy",
                        "description":"Release operations",
                        "keywords":["deploy","release"],
                        "perspectives":["operational"]
                    }
                }),
                None,
            )
            .await
            .unwrap();
        let state = commit.plugin_state().unwrap();
        let recovered = UiWorkspaceRecovery::from_plugin_state(&state).unwrap();
        let classified = service
            .dispatch_committed_read(
                "classify",
                &serde_json::json!({"text":"release then deploy","limit":8}),
                Some(recovered),
            )
            .unwrap();
        assert_eq!(classified["classification"], "deploy");
        assert_eq!(classified["taxonomy_revision"], 1);

        let other_term = UiAccessContext {
            principal_id: owner.principal_id.clone(),
            session_id: owner.session_id.clone(),
            session_genesis: Some("genesis-b".into()),
        };
        assert!(service.snapshot(&other_term).await.unwrap().is_none());
    }

    fn spec(text: &str) -> Value {
        serde_json::json!({
            "root":"root",
            "elements":{
                "root":{"type":"text","props":{"text":text},"children":[]}
            }
        })
    }

    fn service(store: CozoGraphShuttle) -> UiWorkspaceMethods {
        service_from_documents(Arc::new(RuntimeDocuments::new(store)), false)
    }

    fn service_from_documents(
        documents: Arc<RuntimeDocuments>,
        new_catalog: bool,
    ) -> UiWorkspaceMethods {
        let validator: Arc<dyn UiSpecValidator> = if new_catalog {
            Arc::new(NewCatalogValidator)
        } else {
            Arc::new(FixtureValidator)
        };
        UiWorkspaceMethods::new(documents, validator).unwrap()
    }

    async fn completed_spec(service: &UiWorkspaceMethods, owner: &UiAccessContext) {
        service
            .create_job(owner, "job-1", PROMPT_DIGEST)
            .await
            .unwrap();
        service.mark_job_running(owner, "job-1", 1).await.unwrap();
        service
            .complete_job(
                owner,
                "job-1",
                2,
                "spec-1",
                spec("before"),
                vec!["one".into()],
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn complete_is_atomic_and_projection_omits_raw_owner_and_prompt() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let owner = access("principal-a", "session-a");
        completed_spec(&service, &owner).await;
        let snapshot = service.snapshot(&owner).await.unwrap().unwrap();
        assert_eq!(snapshot.jobs["job-1"].status, UiJobStatus::Completed);
        assert_eq!(snapshot.specs["spec-1"].spec, spec("before"));
        let encoded = serde_json::to_string(&snapshot).unwrap();
        assert!(!encoded.contains("principal-a"));
        assert!(!encoded.contains("session-a"));
        assert_eq!(snapshot.jobs["job-1"].prompt_digest, PROMPT_DIGEST);
        assert!(!encoded.contains("prompt_text"));
    }

    #[tokio::test]
    async fn ownership_is_exact_for_principal_and_session() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        completed_spec(&service, &access("principal-a", "session-a")).await;
        assert!(service
            .snapshot(&access("principal-b", "session-a"))
            .await
            .unwrap()
            .is_none());
        assert!(service
            .snapshot(&access("principal-a", "session-b"))
            .await
            .unwrap()
            .is_none());
    }

    #[test]
    fn owner_hash_is_length_prefixed_and_domain_separated() {
        let left = Owner::new(&access("ab", "c")).unwrap();
        let right = Owner::new(&access("a", "bc")).unwrap();
        assert_ne!(left.owner_key, right.owner_key);
        assert!(left.document_id.starts_with(DOCUMENT_PREFIX));
    }

    #[tokio::test]
    async fn input_bounds_fail_before_creating_a_document() {
        let documents = Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let service =
            UiWorkspaceMethods::new(documents.clone(), Arc::new(FixtureValidator)).unwrap();
        let owner = access("principal", "session");
        assert!(service
            .create_job(&owner, &"x".repeat(MAX_ID_BYTES + 1), PROMPT_DIGEST)
            .await
            .is_err());
        let key = Owner::new(&owner).unwrap().document_id;
        assert!(documents.read(&key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn stale_revision_and_invalid_catalog_spec_leave_state_unchanged() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let owner = access("principal", "session");
        completed_spec(&service, &owner).await;
        assert!(service
            .update_spec_properties(
                &owner,
                "spec-1",
                7,
                "root",
                serde_json::json!({"text":"stale"}),
            )
            .await
            .is_err());
        assert!(service
            .update_spec_properties(
                &owner,
                "spec-1",
                1,
                "root",
                serde_json::json!({"wrong":true}),
            )
            .await
            .is_err());
        let stored = service.get_spec(&owner, "spec-1").await.unwrap().unwrap();
        assert_eq!(stored.revision, 1);
        assert_eq!(stored.spec, spec("before"));
    }

    #[tokio::test]
    async fn committed_workspace_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cozo");
        {
            let service = service(CozoGraphShuttle::new_persistent(path.clone()).unwrap());
            completed_spec(&service, &access("principal", "session")).await;
        }
        let reopened = service(CozoGraphShuttle::new_persistent(path).unwrap());
        let stored = reopened
            .get_spec(&access("principal", "session"), "spec-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stored.spec, spec("before"));
        assert_eq!(stored.catalog_hash, CATALOG_HASH);
    }

    #[tokio::test]
    async fn durable_recovery_returns_exact_document_revision_and_retained_projection() {
        let documents = Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let service = service_from_documents(documents, false);
        let owner = access("principal", "session");
        let created = service
            .create_job(&owner, "job-1", PROMPT_DIGEST)
            .await
            .unwrap();
        let recovery = service.recover_snapshot(&owner).await.unwrap().unwrap();
        assert_eq!(recovery.document_revision, created.document_revision);
        assert_eq!(recovery.projection, created.projection);
        assert_eq!(recovery.projection.catalog_hash, CATALOG_HASH);
    }

    #[tokio::test]
    async fn successful_completion_receipt_cannot_be_rewritten_as_publication_failure() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let owner = access("principal", "session");
        service
            .create_job(&owner, "job-1", PROMPT_DIGEST)
            .await
            .unwrap();
        service.mark_job_running(&owner, "job-1", 1).await.unwrap();
        let completed = service
            .complete_job(&owner, "job-1", 2, "spec-1", spec("exact"), vec![])
            .await
            .unwrap();
        let completed_job = match &completed.receipt {
            UiMutationReceipt::SpecCompleted { job, spec } => {
                assert_eq!(spec.spec, self::spec("exact"));
                job.clone()
            }
            receipt => panic!("unexpected receipt: {receipt:?}"),
        };
        assert!(service
            .fail_job(
                &owner,
                "job-1",
                completed_job.revision,
                UiJobFailureCode::PublicationFailed,
            )
            .await
            .is_err());
        let recovered = service.recover_snapshot(&owner).await.unwrap().unwrap();
        assert_eq!(recovered.document_revision, completed.document_revision);
        assert_eq!(recovered.projection, completed.projection);
        assert_eq!(
            recovered.projection.jobs["job-1"].status,
            UiJobStatus::Completed
        );
    }

    #[tokio::test]
    async fn catalog_change_allows_truthful_failure_and_retained_deletion_without_rehash() {
        let documents = Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let old = service_from_documents(documents.clone(), false);
        let owner = access("principal", "session");
        old.create_job(&owner, "interrupted", PROMPT_DIGEST)
            .await
            .unwrap();
        completed_spec(&old, &owner).await;

        let current = service_from_documents(documents, true);
        let retained = current.get_spec(&owner, "spec-1").await.unwrap().unwrap();
        assert_eq!(retained.catalog_hash, CATALOG_HASH);
        assert!(current
            .update_spec_properties(
                &owner,
                "spec-1",
                1,
                "root",
                serde_json::json!({"text":"must-not-render"}),
            )
            .await
            .is_err());

        let failed = current
            .fail_job(&owner, "interrupted", 1, UiJobFailureCode::CatalogChanged)
            .await
            .unwrap();
        assert_eq!(failed.projection.catalog_hash, CATALOG_HASH);
        assert_eq!(
            failed.projection.jobs["interrupted"].error_code,
            Some(UiJobFailureCode::CatalogChanged)
        );
        assert!(current
            .create_job(&owner, "new-work", PROMPT_DIGEST)
            .await
            .is_err());

        let deleted = current.delete_spec(&owner, "spec-1", 1).await.unwrap();
        assert_eq!(deleted.projection.catalog_hash, CATALOG_HASH);
        assert!(!deleted.projection.specs.contains_key("spec-1"));
        assert_eq!(
            current
                .recover_snapshot(&owner)
                .await
                .unwrap()
                .unwrap()
                .projection
                .catalog_hash,
            CATALOG_HASH
        );
    }

    #[tokio::test]
    async fn catalog_changed_failure_requires_an_actual_contract_change() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let owner = access("principal", "session");
        service
            .create_job(&owner, "job-1", PROMPT_DIGEST)
            .await
            .unwrap();
        assert!(service
            .fail_job(&owner, "job-1", 1, UiJobFailureCode::CatalogChanged)
            .await
            .unwrap_err()
            .to_string()
            .contains("has not changed"));
    }

    #[tokio::test]
    async fn interrupted_nonterminal_job_is_marked_without_reconstructing_prompt() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let owner = access("principal", "session");
        service
            .create_job(&owner, "job-1", PROMPT_DIGEST)
            .await
            .unwrap();
        service.mark_job_running(&owner, "job-1", 1).await.unwrap();
        let commit = service
            .fail_job(&owner, "job-1", 2, UiJobFailureCode::Interrupted)
            .await
            .unwrap();
        assert_eq!(commit.projection.jobs["job-1"].status, UiJobStatus::Failed);
        assert_eq!(
            commit.projection.jobs["job-1"].error_code,
            Some(UiJobFailureCode::Interrupted)
        );
        assert_eq!(commit.projection.jobs["job-1"].prompt_digest, PROMPT_DIGEST);
    }

    #[tokio::test]
    async fn aggregate_read_limit_rejects_before_workspace_decode() {
        let documents = Arc::new(RuntimeDocuments::new(
            CozoGraphShuttle::new_in_memory().unwrap(),
        ));
        let service = service_from_documents(documents.clone(), false);
        let owner = access("principal", "session");
        let document_id = Owner::new(&owner).unwrap().document_id;
        documents
            .update(&document_id, |_| {
                Ok(serde_json::json!({"private_padding":"x".repeat(MAX_DOCUMENT_BYTES)}))
            })
            .await
            .unwrap();

        let ordinary = service.snapshot(&owner).await.unwrap_err();
        assert!(ordinary.to_string().contains("aggregate read/write bound"));
        assert!(!ordinary.to_string().contains("invalid stored UI workspace"));
        let durable = service.recover_snapshot(&owner).await.unwrap_err();
        assert!(durable.to_string().contains("aggregate read/write bound"));
        assert!(!durable.to_string().contains("private_padding"));
    }

    #[tokio::test]
    async fn failed_validation_does_not_complete_job_or_insert_spec() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let owner = access("principal", "session");
        service
            .create_job(&owner, "job-1", PROMPT_DIGEST)
            .await
            .unwrap();
        service.mark_job_running(&owner, "job-1", 1).await.unwrap();
        let invalid = serde_json::json!({
            "root":"root",
            "elements":{"root":{"type":"invented","props":{},"children":[]}}
        });
        assert!(service
            .complete_job(&owner, "job-1", 2, "spec-1", invalid, vec![])
            .await
            .is_err());
        let snapshot = service.snapshot(&owner).await.unwrap().unwrap();
        assert_eq!(snapshot.jobs["job-1"].status, UiJobStatus::Running);
        assert!(!snapshot.specs.contains_key("spec-1"));
    }

    #[tokio::test]
    async fn deletion_is_revision_checked_and_clears_job_reference() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let owner = access("principal", "session");
        completed_spec(&service, &owner).await;
        assert!(service.delete_spec(&owner, "spec-1", 2).await.is_err());
        let commit = service.delete_spec(&owner, "spec-1", 1).await.unwrap();
        assert!(!commit.projection.specs.contains_key("spec-1"));
        assert_eq!(commit.projection.jobs["job-1"].result_spec_id, None);
        assert_eq!(commit.projection.jobs["job-1"].revision, 4);
    }

    #[tokio::test]
    async fn terminal_job_retention_reuses_capacity_and_keeps_specs() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let owner = access("principal", "session");
        service
            .create_job(&owner, "job-00", PROMPT_DIGEST)
            .await
            .unwrap();
        service.mark_job_running(&owner, "job-00", 1).await.unwrap();
        service
            .complete_job(&owner, "job-00", 2, "spec-00", spec("retained"), vec![])
            .await
            .unwrap();

        for index in 1..MAX_JOBS {
            let job_id = format!("job-{index:02}");
            service
                .create_job(&owner, &job_id, PROMPT_DIGEST)
                .await
                .unwrap();
            service
                .fail_job(&owner, &job_id, 1, UiJobFailureCode::ProviderUnavailable)
                .await
                .unwrap();
        }

        let commit = service
            .create_job(&owner, "job-32", PROMPT_DIGEST)
            .await
            .unwrap();
        match &commit.receipt {
            UiMutationReceipt::JobCreated {
                job,
                pruned_job_ids,
            } => {
                assert_eq!(job.id, "job-32");
                assert_eq!(pruned_job_ids.len(), 1);
                assert_eq!(pruned_job_ids[0], "job-00");
            }
            receipt => panic!("unexpected receipt: {receipt:?}"),
        }
        assert_eq!(commit.projection.jobs.len(), MAX_JOBS);
        assert!(!commit.projection.jobs.contains_key("job-00"));
        assert!(commit.projection.jobs.contains_key("job-32"));
        assert_eq!(commit.projection.specs["spec-00"].spec, spec("retained"));
    }

    #[tokio::test]
    async fn active_jobs_are_never_pruned_at_capacity() {
        let service = service(CozoGraphShuttle::new_in_memory().unwrap());
        let owner = access("principal", "session");
        for index in 0..MAX_JOBS {
            service
                .create_job(&owner, &format!("active-{index:02}"), PROMPT_DIGEST)
                .await
                .unwrap();
        }

        let error = service
            .create_job(&owner, "overflow", PROMPT_DIGEST)
            .await
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("no terminal job eligible for retention"));
        let snapshot = service.snapshot(&owner).await.unwrap().unwrap();
        assert_eq!(snapshot.jobs.len(), MAX_JOBS);
        assert!(!snapshot.jobs.contains_key("overflow"));
    }
}
