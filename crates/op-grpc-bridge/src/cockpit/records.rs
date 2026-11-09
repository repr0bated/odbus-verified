//! Cockpit evidence documents (spec task 1.1). These are evidence schemas, not
//! alternate plugin schemas or claims that any runtime contract is available.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::safety::{EvidenceDigest, RecordError};

pub const COCKPIT: &str = "3tchedCockpitBot";
pub const MAIN_SESSION: &str = "b0c777e3-0955-cd8b-3d01-ca12998cae28";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewKind { ThreeTched, Threetched }
impl ViewKind {
    pub fn mount_path(self) -> &'static str {
        match self { Self::ThreeTched => "/run/mounts/3tchedFS", Self::Threetched => "/run/mounts/threetched" }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogPin { pub hash: String, pub generation: u64 }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordHeader {
    pub id: String,
    pub recorded_at: DateTime<Utc>,
    pub agent: String,
    pub task_ids: Vec<String>,
    pub source_references: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "classification", rename_all = "snake_case", deny_unknown_fields)]
pub enum Finding<T> {
    ObservedRuntime { value: T, evidence: Vec<String> },
    SourceDefinitionOnly { value: T, evidence: Vec<String> },
    NotFound { searched: Vec<String>, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceLocation { pub path: String, pub first_line: u32, pub last_line: u32 }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSymbol { pub location: SourceLocation, pub signature: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceInterface {
    pub interface: String,
    pub repository: String,
    pub commit: String,
    pub symbol: SourceSymbol,
    pub file_digest: EvidenceDigest,
    pub callers: Vec<SourceSymbol>,
    /// False means the definition exists but is not linked into the active graph.
    pub linked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceTransition { pub caller: SourceSymbol, pub callee: SourceSymbol, pub evidence: Vec<String> }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum TraceEnd {
    ReturnedThroughSameHandle { evidence: Vec<String> },
    Unresolved { last_proven_symbol: SourceSymbol, missing_transition: String, dependent_tasks: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationTrace { pub view: ViewKind, pub transitions: Vec<TraceTransition>, pub end: TraceEnd }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineRecord {
    pub header: RecordHeader,
    pub interfaces: Vec<Finding<SourceInterface>>,
    pub operation_traces: Vec<OperationTrace>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestKernelIdentity { pub principal_id: String, pub session_id: String, pub mount_instance_id: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountedViewObservation {
    pub view: ViewKind,
    pub mount_path: String,
    pub kernel_mount_source: Finding<String>,
    pub owning_process: Finding<String>,
    pub catalog_pin: Finding<CatalogPin>,
    pub guest_kernel_identity: Finding<GuestKernelIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreservationObservation {
    pub cockpit_name: Finding<String>,
    pub session: Finding<String>,
    pub native_uuid: Finding<String>,
    pub principal_id: Finding<String>,
    pub grants_identity: Finding<String>,
    /// Stable public locator only; never a credential or its fingerprint.
    pub credential_identity: Finding<String>,
    pub seed: Finding<String>,
    pub leaf: Finding<String>,
    pub btrfs_members: Finding<Vec<String>>,
    pub subvolumes: Finding<Vec<String>>,
    pub host_channels: Finding<Vec<String>>,
    pub listeners: Finding<Vec<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceClass {
    MountedProjection,
    CommittedShmProjection,
    GuestKernelIdentity,
    TrackerAttempt,
    TrackerTerminal,
    Receipt,
    AuthoritativeState,
    LifecycleSnapshot,
    HostBoundary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeFinding {
    pub id: String,
    pub class: EvidenceClass,
    pub view: Option<ViewKind>,
    pub observation: Finding<String>,
}

/// Evidence schemas describe the required contract; collectors may leave it
/// absent. They do not synthesize fields missing from the actual wire receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attribution {
    pub operation_id: String,
    pub caller_identity: String,
    pub caller_session: String,
    pub plugin: String,
    pub method: String,
    pub subid: String,
    pub catalog_pin: CatalogPin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackerRecordReference {
    pub attribution: Attribution,
    pub event_id: u64,
    pub event_hash: String,
    pub previous_record_hash: String,
    pub evidence_reference: String,
}

/// A finite supporting read has its own attempt, terminal and receipt; it
/// deliberately contains no nested authoritative-state read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportingReadReferences {
    pub view: ViewKind,
    pub attempt: TrackerRecordReference,
    pub terminal: TrackerRecordReference,
    pub receipt_attribution: Attribution,
    pub outcome: String,
    pub receipt_outcome: String,
    pub receipt_attempt_event_id: u64,
    pub receipt_attempt_event_hash: String,
    pub receipt_terminal_event_id: u64,
    pub receipt_terminal_event_hash: String,
    pub receipt_reference: String,
    pub hash_verification_reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionReferences {
    pub guest_identity: GuestKernelIdentity,
    pub attempt: TrackerRecordReference,
    pub terminal: TrackerRecordReference,
    pub terminal_outcome: String,
    pub receipt_attribution: Attribution,
    pub receipt_outcome: String,
    pub receipt_attempt_event_id: u64,
    pub receipt_attempt_event_hash: String,
    pub receipt_terminal_event_id: u64,
    pub receipt_terminal_event_hash: String,
    pub receipt_reference: String,
    pub authoritative_state_reference: String,
    pub post_terminal_state_read: SupportingReadReferences,
    pub hash_verification_reference: String,
    pub state_comparison_reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCallObservation {
    pub attribution: Attribution,
    pub view: ViewKind,
    pub evidence: Vec<String>,
    pub completion: Option<CompletionReferences>,
    pub missing_or_mismatched: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeRecord {
    pub header: RecordHeader,
    pub preservation: PreservationObservation,
    pub views: Vec<Finding<MountedViewObservation>>,
    pub findings: Vec<RuntimeFinding>,
    pub tool_calls: Vec<ToolCallObservation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockerKind { MissingPrerequisite, Denial, UnknownOutcome, AttributionMismatch, InvariantFailure, SourceTraceGap }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockerRecord {
    pub header: RecordHeader,
    pub kind: BlockerKind,
    pub operation: String,
    pub operation_id: Option<String>,
    /// Discovery may establish no caller. Absence is never filled from config.
    pub caller_identity: Option<String>,
    pub caller_session: Option<String>,
    pub plugin: Option<String>,
    pub method: Option<String>,
    pub subid: Option<String>,
    pub catalog_pin: Option<CatalogPin>,
    pub missing_capability_or_policy: String,
    pub redacted_error: String,
    pub dependent_tasks: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InventoryClassification {
    LegacyChatRemove,
    CtlPlaneChatbotKeepAndDecouple,
    ThreetchedServiceKeep,
    IdentityGrantStorageLifecycleKeep,
    UnclearOwnerDecisionRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryItem {
    pub source: SourceInterface,
    pub classification: InventoryClassification,
    pub rationale: String,
    pub jev_evidence: Vec<String>,
    pub naming_defects: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyChatInventory { pub header: RecordHeader, pub items: Vec<InventoryItem> }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerAuthorization {
    pub header: RecordHeader,
    pub authorization_id: String,
    pub action: String,
    pub target_identity: String,
    pub target_cockpit: String,
    pub affected_files: Vec<String>,
    pub affected_services: Vec<String>,
    pub operation_files: Vec<String>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    /// Reference to the owner's actual instruction. This document cannot mint authority.
    pub owner_request: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceStage { NotStarted, Proposed, Complete, Blocked, Unconfirmed }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageClaim {
    pub stage: EvidenceStage,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandEvidence {
    pub command: String,
    pub target: Option<String>,
    pub exit_status: i32,
    pub owner_request: String,
    pub receipt: String,
}

/// Each stage has its own evidence; no stage is inferred from another.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentStatus {
    pub component: String,
    pub task_ids: Vec<String>,
    pub proposed: StageClaim,
    pub implemented: StageClaim,
    pub built: StageClaim,
    pub tested: StageClaim,
    pub deployed: StageClaim,
    pub source_files: Vec<String>,
    pub build: Option<CommandEvidence>,
    pub test: Option<CommandEvidence>,
    pub deployment_evidence: Vec<RuntimeFinding>,
    pub completed_calls: Vec<ToolCallObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenBlocker { pub record_id: String, pub dependent_tasks: Vec<String> }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StatusReport {
    pub header: RecordHeader,
    pub components: Vec<ComponentStatus>,
    pub open_blockers: Vec<OpenBlocker>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "kebab-case", deny_unknown_fields)]
pub enum EvidenceRecord {
    #[serde(rename = "cockpit.baseline/1")] Baseline(BaselineRecord),
    #[serde(rename = "cockpit.runtime/1")] Runtime(RuntimeRecord),
    #[serde(rename = "cockpit.blocker/1")] Blocker(BlockerRecord),
    #[serde(rename = "cockpit.legacy-chat-inventory/1")] LegacyChatInventory(LegacyChatInventory),
    #[serde(rename = "cockpit.owner-authorization/1")] OwnerAuthorization(OwnerAuthorization),
    #[serde(rename = "cockpit.status/1")] Status(StatusReport),
    #[serde(rename = "cockpit.storage-layout/1")] StorageLayout(StorageLayoutRecord),
    #[serde(rename = "cockpit.image-provisioning/1")] ImageProvisioning(ImageProvisioningRecord),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartitionDef {
    pub label: String,
    pub format: String,
    pub flags: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageImage {
    pub name: String,
    pub partition_layout: Vec<PartitionDef>,
    pub size_bytes: u64,
    pub sparse: bool,
    pub device_add: bool, // true = used with btrfs device add (no loop)
    pub mount_path: String,
    pub subvolume: Option<String>, // if image contains only one subvol (e.g. "@")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StorageLayoutRecord {
    pub header: RecordHeader,
    pub images: Vec<StorageImage>,
    pub sled_description: String,
    pub image_directory: String,
    pub no_loop_devices: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageFileState {
    pub name: String,
    pub exists: bool,
    pub size_bytes: u64,
    pub permissions: String,
    pub can_read: bool,
    pub can_write: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageProvisioningRecord {
    pub header: RecordHeader,
    pub expected_layout: StorageLayoutRecord,
    pub actual_files: Vec<ImageFileState>,
    pub all_present: bool,
    pub naming_correct: bool, // all files match expected naming pattern
    pub validation_errors: Vec<String>,
}

fn nonempty(value: &str) -> Result<(), RecordError> {
    if value.trim().is_empty() { Err(RecordError::Invalid("required value absent")) } else { Ok(()) }
}
fn references(values: &[String]) -> Result<(), RecordError> {
    if values.is_empty() { return Err(RecordError::Invalid("evidence references absent")); }
    for value in values { nonempty(value)?; }
    Ok(())
}
fn header(value: &RecordHeader) -> Result<(), RecordError> {
    nonempty(&value.id)?; nonempty(&value.agent)?;
    references(&value.task_ids)?; references(&value.source_references)
}
fn source(value: &SourceInterface) -> Result<(), RecordError> {
    if value.commit.len() != 40 || !value.commit.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
        return Err(RecordError::Invalid("source commit is not a full lowercase SHA-1 pin"));
    }
    nonempty(&value.interface)?; nonempty(&value.repository)?;
    symbol(&value.symbol)?; value.file_digest.validate()?;
    for caller in &value.callers { symbol(caller)?; }
    Ok(())
}
fn symbol(value: &SourceSymbol) -> Result<(), RecordError> {
    nonempty(&value.signature)?; nonempty(&value.location.path)?;
    if value.location.first_line == 0 || value.location.last_line < value.location.first_line {
        return Err(RecordError::Invalid("source line range is invalid"));
    }
    Ok(())
}
fn finding<T>(value: &Finding<T>, validate: impl FnOnce(&T) -> Result<(), RecordError>) -> Result<(), RecordError> {
    match value {
        Finding::ObservedRuntime { value, evidence } | Finding::SourceDefinitionOnly { value, evidence } => {
            references(evidence)?; validate(value)
        }
        Finding::NotFound { searched, reason } => { references(searched)?; nonempty(reason) }
    }
}
fn runtime_finding(value: &RuntimeFinding) -> Result<(), RecordError> {
    nonempty(&value.id)?; finding(&value.observation, |v| nonempty(v))
}

fn attribution(value: &Attribution) -> Result<(), RecordError> {
    for field in [&value.operation_id, &value.caller_identity, &value.caller_session,
        &value.plugin, &value.method, &value.subid, &value.catalog_pin.hash] { nonempty(field)?; }
    Ok(())
}

impl ToolCallObservation {
    pub fn validate(&self) -> Result<(), RecordError> {
        attribution(&self.attribution)?;
        references(&self.evidence)?;
        let Some(proof) = &self.completion else { return references(&self.missing_or_mismatched); };
        if !self.missing_or_mismatched.is_empty()
            || proof.attempt.attribution != self.attribution
            || proof.terminal.attribution != self.attribution
            || proof.receipt_attribution != self.attribution
            || proof.guest_identity.principal_id != self.attribution.caller_identity
            || proof.guest_identity.session_id != self.attribution.caller_session
            || proof.attempt.event_id == 0 || proof.terminal.event_id <= proof.attempt.event_id
            || proof.receipt_attempt_event_id != proof.attempt.event_id
            || proof.receipt_attempt_event_hash != proof.attempt.event_hash
            || proof.receipt_terminal_event_id != proof.terminal.event_id
            || proof.receipt_terminal_event_hash != proof.terminal.event_hash
            || proof.receipt_outcome != proof.terminal_outcome {
            return Err(RecordError::Invalid("completion attribution or event references mismatch"));
        }
        let read = &proof.post_terminal_state_read;
        attribution(&read.attempt.attribution)?;
        if read.view != self.view
            || read.attempt.attribution.operation_id == self.attribution.operation_id
            || read.attempt.attribution.caller_identity != self.attribution.caller_identity
            || read.attempt.attribution.caller_session != self.attribution.caller_session
            || read.attempt.attribution.catalog_pin != self.attribution.catalog_pin
            || read.attempt.event_id <= proof.terminal.event_id
            || read.terminal.event_id <= read.attempt.event_id
            || read.terminal.attribution != read.attempt.attribution
            || read.receipt_attribution != read.attempt.attribution
            || read.outcome != read.receipt_outcome
            || read.receipt_attempt_event_id != read.attempt.event_id
            || read.receipt_attempt_event_hash != read.attempt.event_hash
            || read.receipt_terminal_event_id != read.terminal.event_id
            || read.receipt_terminal_event_hash != read.terminal.event_hash {
            return Err(RecordError::Invalid("post-terminal supporting read attribution mismatch"));
        }
        for field in [&proof.guest_identity.mount_instance_id,
            &proof.attempt.event_hash, &proof.attempt.previous_record_hash, &proof.attempt.evidence_reference,
            &proof.terminal.event_hash, &proof.terminal.previous_record_hash, &proof.terminal.evidence_reference,
            &proof.receipt_reference, &proof.authoritative_state_reference,
            &read.attempt.event_hash, &read.attempt.previous_record_hash, &read.attempt.evidence_reference,
            &read.terminal.event_hash, &read.terminal.previous_record_hash, &read.terminal.evidence_reference,
            &read.receipt_reference, &read.hash_verification_reference, &read.outcome,
            &proof.hash_verification_reference, &proof.state_comparison_reference, &proof.terminal_outcome] { nonempty(field)?; }
        Ok(())
    }
}

impl ComponentStatus {
    pub fn validate(&self) -> Result<(), RecordError> {
        nonempty(&self.component)?; references(&self.task_ids)?;
        for claim in [&self.proposed, &self.implemented, &self.built, &self.tested, &self.deployed] {
            if claim.stage != EvidenceStage::NotStarted { references(&claim.evidence)?; }
        }
        if self.implemented.stage == EvidenceStage::Complete { references(&self.source_files)?; }
        for (claim, command, build) in [(&self.built, &self.build, true), (&self.tested, &self.test, false)] {
            if claim.stage == EvidenceStage::Complete {
                let command = command.as_ref().ok_or(RecordError::Invalid("command evidence absent"))?;
                nonempty(&command.command)?; nonempty(&command.owner_request)?; nonempty(&command.receipt)?;
                if command.exit_status != 0 || (build && command.target.as_deref() != Some("x86_64-unknown-linux-gnu")) {
                    return Err(RecordError::Invalid("successful native command evidence absent"));
                }
            }
        }
        for item in &self.deployment_evidence { runtime_finding(item)?; }
        for call in &self.completed_calls { call.validate()?; }
        if self.deployed.stage == EvidenceStage::Complete {
            if self.completed_calls.is_empty() || self.completed_calls.iter().any(|call| call.completion.is_none()) {
                return Err(RecordError::Invalid("correlated deployment completion evidence absent"));
            }
            let observed: BTreeSet<_> = self.deployment_evidence.iter()
                .filter(|e| matches!(e.observation, Finding::ObservedRuntime { .. }))
                .map(|e| e.class).collect();
            for required in [EvidenceClass::GuestKernelIdentity, EvidenceClass::TrackerAttempt,
                EvidenceClass::TrackerTerminal, EvidenceClass::Receipt, EvidenceClass::AuthoritativeState] {
                if !observed.contains(&required) { return Err(RecordError::Invalid("deployment evidence incomplete")); }
            }
        }
        Ok(())
    }

    /// Missing deployment observations downgrade only deployment, never source,
    /// build, or test. Completion attestation belongs to the later executor.
    pub fn calculate(mut self) -> Self {
        if self.implemented.stage == EvidenceStage::Complete
            && (self.source_files.is_empty() || self.task_ids.is_empty()) {
            self.implemented.stage = EvidenceStage::Unconfirmed;
        }
        for (claim, command, native) in [(&mut self.built, &self.build, true), (&mut self.tested, &self.test, false)] {
            if claim.stage == EvidenceStage::Complete && !command.as_ref().is_some_and(|command|
                command.exit_status == 0 && !command.command.trim().is_empty()
                && !command.owner_request.trim().is_empty() && !command.receipt.trim().is_empty()
                && (!native || command.target.as_deref() == Some("x86_64-unknown-linux-gnu"))) {
                claim.stage = EvidenceStage::Unconfirmed;
            }
        }
        if self.deployed.stage == EvidenceStage::Complete &&
            (self.completed_calls.is_empty() || self.completed_calls.iter().any(|call|
                call.completion.is_none() || call.validate().is_err())) {
            self.deployed.stage = EvidenceStage::Unconfirmed;
        }
        self
    }
}

impl EvidenceRecord {
    pub fn validate(&self) -> Result<(), RecordError> {
        match self {
            Self::Baseline(record) => {
                header(&record.header)?;
                for item in &record.interfaces { finding(item, source)?; }
                let mut views = BTreeSet::new();
                for trace in &record.operation_traces {
                    if !views.insert(trace.view) { return Err(RecordError::Invalid("duplicate view trace")); }
                    for pair in trace.transitions.windows(2) {
                        if pair[0].callee != pair[1].caller { return Err(RecordError::Invalid("disconnected source trace")); }
                    }
                    for transition in &trace.transitions { symbol(&transition.caller)?; symbol(&transition.callee)?; references(&transition.evidence)?; }
                    match &trace.end {
                        TraceEnd::ReturnedThroughSameHandle { evidence } => references(evidence)?,
                        TraceEnd::Unresolved { last_proven_symbol, missing_transition, dependent_tasks } => {
                            symbol(last_proven_symbol)?; nonempty(missing_transition)?; references(dependent_tasks)?;
                            if trace.transitions.last().is_some_and(|step| step.callee != *last_proven_symbol) {
                                return Err(RecordError::Invalid("trace continues beyond its last proven symbol"));
                            }
                        }
                    }
                }
            }
            Self::Runtime(record) => {
                header(&record.header)?;
                let preservation = &record.preservation;
                for field in [&preservation.cockpit_name, &preservation.session, &preservation.native_uuid,
                    &preservation.principal_id, &preservation.grants_identity, &preservation.credential_identity,
                    &preservation.seed, &preservation.leaf] { finding(field, |value| nonempty(value))?; }
                for field in [&preservation.btrfs_members, &preservation.subvolumes, &preservation.host_channels,
                    &preservation.listeners] {
                    finding(field, |values| { for value in values { nonempty(value)?; } Ok(()) })?;
                }
                for view in &record.views {
                    finding(view, |view| {
                        if view.mount_path != view.view.mount_path() { return Err(RecordError::Invalid("view mount path mismatch")); }
                        finding(&view.kernel_mount_source, |v| nonempty(v))?;
                        finding(&view.owning_process, |v| nonempty(v))?;
                        finding(&view.catalog_pin, |v| nonempty(&v.hash))?;
                        finding(&view.guest_kernel_identity, |v| { nonempty(&v.principal_id)?; nonempty(&v.session_id)?; nonempty(&v.mount_instance_id) })
                    })?;
                }
                for item in &record.findings { runtime_finding(item)?; }
                for call in &record.tool_calls { call.validate()?; }
            }
            Self::Blocker(record) => {
                header(&record.header)?; nonempty(&record.operation)?;
                nonempty(&record.missing_capability_or_policy)?; nonempty(&record.redacted_error)?;
                references(&record.dependent_tasks)?;
                if record.kind == BlockerKind::Denial {
                    for field in [&record.operation_id, &record.caller_identity, &record.caller_session,
                        &record.plugin, &record.method, &record.subid] {
                        nonempty(field.as_deref().ok_or(RecordError::Invalid("denial attribution absent"))?)?;
                    }
                    nonempty(&record.catalog_pin.as_ref().ok_or(RecordError::Invalid("denial catalog pin absent"))?.hash)?;
                }
            }
            Self::LegacyChatInventory(record) => {
                header(&record.header)?;
                for item in &record.items { source(&item.source)?; nonempty(&item.rationale)?; }
            }
            Self::OwnerAuthorization(record) => {
                header(&record.header)?;
                nonempty(&record.authorization_id)?; nonempty(&record.action)?;
                nonempty(&record.target_identity)?; nonempty(&record.owner_request)?;
                if record.target_cockpit != COCKPIT || record.expires_at.is_some_and(|at| at <= record.issued_at) {
                    return Err(RecordError::Invalid("authorization target or validity interval invalid"));
                }
                if record.affected_files.is_empty() && record.affected_services.is_empty() && record.operation_files.is_empty() {
                    return Err(RecordError::Invalid("authorization scope absent"));
                }
            }
            Self::Status(record) => {
                header(&record.header)?;
                for item in &record.components { item.validate()?; }
                for item in &record.open_blockers { nonempty(&item.record_id)?; references(&item.dependent_tasks)?; }
            }
        }
        Ok(())
    }
}
