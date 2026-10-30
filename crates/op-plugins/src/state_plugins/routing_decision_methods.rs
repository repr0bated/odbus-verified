//! Typed proposal, gate, and owner-scoped decision-record contracts.

use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use super::schema_tag_registry_methods::{
    CanonicalMappingRef, MAX_ID_BYTES, MAX_PAGE_SIZE, MAX_TEXT_BYTES, OpenFlowBindingRef,
    PolicyRecord, PolicyRule, PolicySnapshot, ResolvedTagRef, SchemaTagAssociation, TagDisposition,
    validate_text,
};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CandidateOperation {
    pub plugin_id: String,
    pub method_key: String,
    pub schema_hash: String,
}

impl CandidateOperation {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.plugin_id, "candidate plugin_id", MAX_ID_BYTES)?;
        validate_text(&self.method_key, "candidate method_key", MAX_ID_BYTES)?;
        validate_text(&self.schema_hash, "candidate schema_hash", MAX_ID_BYTES)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClassificationProposal {
    pub tag: Option<String>,
    /// Caller-asserted provenance; not backend-verified origin evidence.
    pub asserted_origin: ClassificationOrigin,
    pub taxonomy_revision: Option<u64>,
    pub classifier_version: Option<String>,
    pub uncertainty: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationOrigin {
    Declaration,
    UserSelection,
    KeywordMatch,
    ModelProposal,
}

impl ClassificationProposal {
    pub fn validate(&self) -> Result<()> {
        if let Some(tag) = &self.tag {
            validate_text(tag, "classification tag", MAX_ID_BYTES)?;
        }
        if let Some(version) = &self.classifier_version {
            validate_text(version, "classifier_version", MAX_ID_BYTES)?;
        }
        if let Some(value) = self.uncertainty {
            ensure!(
                value.is_finite() && (0.0..=1.0).contains(&value),
                "uncertainty must be finite and between zero and one"
            );
        }
        ensure!(
            self.tag.is_some(),
            "classification proposal must contain a tag label"
        );
        Ok(())
    }
}

/// Backend-authenticated identity. Never accepted in a public proposal DTO.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VerifiedCaller {
    pub principal_id: String,
    pub session_id: String,
    pub owner_id: String,
    pub container: Option<String>,
    pub project_scope: Option<String>,
    pub capabilities: BTreeSet<String>,
}

impl VerifiedCaller {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.principal_id, "principal_id", MAX_ID_BYTES)?;
        validate_text(&self.session_id, "session_id", MAX_ID_BYTES)?;
        validate_text(&self.owner_id, "owner_id", MAX_ID_BYTES)?;
        if let Some(value) = &self.container {
            validate_text(value, "container", MAX_ID_BYTES)?;
        }
        if let Some(value) = &self.project_scope {
            validate_text(value, "project_scope", MAX_ID_BYTES)?;
        }
        ensure!(
            self.capabilities.len() <= 256,
            "too many verified capabilities"
        );
        for capability in &self.capabilities {
            validate_text(capability, "verified capability", MAX_ID_BYTES)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResolvedOperationRef {
    pub plugin_id: String,
    pub method_key: String,
    pub transport_name: String,
    pub schema_hash: String,
    pub schema_version: String,
    pub catalog_hash: String,
    pub subid: String,
    pub required_capability: Option<String>,
}

impl ResolvedOperationRef {
    pub fn validate(&self) -> Result<()> {
        for (name, value) in [
            ("plugin_id", &self.plugin_id),
            ("method_key", &self.method_key),
            ("transport_name", &self.transport_name),
            ("schema_hash", &self.schema_hash),
            ("schema_version", &self.schema_version),
            ("catalog_hash", &self.catalog_hash),
            ("subid", &self.subid),
        ] {
            validate_text(value, name, MAX_ID_BYTES)?;
        }
        if let Some(capability) = &self.required_capability {
            validate_text(capability, "required_capability", MAX_ID_BYTES)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RequestedEffect {
    Local,
    Egress,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GateOutcome {
    AuthorizedLocal,
    AuthorizedEgress,
    Denied,
    Unresolved,
    Hold,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GateDiagnostic {
    pub code: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GateResult {
    pub outcome: GateOutcome,
    pub rule_revision: Option<u64>,
    pub diagnostic: Option<GateDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GateInput {
    pub caller: VerifiedCaller,
    /// Stable policy namespace selected by the backend, not derived from the
    /// caller's principal/session scope.
    pub policy_owner_id: String,
    pub operation: ResolvedOperationRef,
    pub classification: Option<ClassificationProposal>,
    /// Taxonomy revision read by the backend for this evaluation.
    pub current_taxonomy_revision: Option<u64>,
    /// Tag identity fetched from the reviewed registry, never from the caller's
    /// classification proposal.
    pub resolved_tag: Option<ResolvedTagRef>,
    /// Registry-resolved tag links for this exact sealed operation.
    pub tag_associations: Vec<SchemaTagAssociation>,
    /// `None` means the marker/policy source could not be read. It is not
    /// interpreted as an unprotected operation.
    pub policy_snapshot: Option<PolicySnapshot>,
    pub policy_read_succeeded: bool,
    pub marker_lookup_succeeded: bool,
    pub requested_effect: RequestedEffect,
    pub residual_validation_passed: bool,
    pub destination_class: Option<String>,
    pub provider_route: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReference {
    pub operation_id: String,
    pub event_id: String,
    pub event_hash: String,
    pub terminal_event_id: Option<String>,
    pub terminal_event_hash: Option<String>,
    pub observed_state_ref: Option<String>,
    pub outcome: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DecisionRecord {
    pub decision_id: String,
    pub verified_identity: VerifiedCaller,
    pub operation_ref: ResolvedOperationRef,
    pub classification_proposal: Option<ClassificationProposal>,
    /// True only when a backend independently verified the proposal's origin.
    /// The first slice has no such verifier and always stores false.
    pub classification_origin_verified: bool,
    pub resolved_tag: Option<ResolvedTagRef>,
    pub policy_owner_id: Option<String>,
    pub policy_revision: Option<u64>,
    pub canonical_mappings: Vec<CanonicalMappingRef>,
    pub gate_result: GateResult,
    pub openflow_binding: Option<OpenFlowBindingRef>,
    pub execution_ref: Option<ExecutionReference>,
    pub shadow_mode: bool,
}

/// The exact private document committed by `routing_decision` publication.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DecisionState {
    #[schemars(extend("readOnly" = true, "x-opdbus-private" = true))]
    pub revision: u64,
    #[schemars(extend("readOnly" = true, "x-opdbus-private" = true))]
    pub records: Vec<DecisionRecord>,
}

impl DecisionState {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.records.len() <= 10_000,
            "decision state exceeds retained-record bound"
        );
        ensure!(
            self.records.is_empty() || self.revision > 0,
            "nonempty decision state requires a positive revision"
        );
        let mut ids = BTreeSet::new();
        for record in &self.records {
            record.validate()?;
            ensure!(
                ids.insert(record.decision_id.as_str()),
                "duplicate decision_id"
            );
        }
        Ok(())
    }

    pub fn validate_for_owner(&self, owner_id: &str) -> Result<()> {
        validate_text(owner_id, "owner_id", MAX_ID_BYTES)?;
        self.validate()?;
        ensure!(
            self.records
                .iter()
                .all(|record| record.verified_identity.owner_id == owner_id),
            "decision state contains a record for another owner"
        );
        Ok(())
    }
}

impl DecisionRecord {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.decision_id, "decision_id", MAX_ID_BYTES)?;
        self.verified_identity.validate()?;
        self.operation_ref.validate()?;
        if let Some(proposal) = &self.classification_proposal {
            proposal.validate()?;
        }
        ensure!(
            !self.classification_origin_verified,
            "classification origin has no backend verifier in this slice"
        );
        if let Some(tag) = &self.resolved_tag {
            tag.validate()?;
        }
        if let Some(owner_id) = &self.policy_owner_id {
            validate_text(owner_id, "policy_owner_id", MAX_ID_BYTES)?;
        }
        ensure!(
            self.canonical_mappings.len() <= 64,
            "too many canonical mappings"
        );
        for mapping in &self.canonical_mappings {
            mapping.validate()?;
        }
        if let Some(binding) = &self.openflow_binding {
            binding.validate()?;
        }
        if let Some(diagnostic) = &self.gate_result.diagnostic {
            validate_text(&diagnostic.code, "gate diagnostic code", MAX_ID_BYTES)?;
            validate_text(&diagnostic.detail, "gate diagnostic detail", MAX_TEXT_BYTES)?;
        }
        if let Some(reference) = &self.execution_ref {
            validate_text(&reference.operation_id, "operation_id", MAX_ID_BYTES)?;
            validate_text(&reference.event_id, "event_id", MAX_ID_BYTES)?;
            validate_text(&reference.event_hash, "event_hash", MAX_ID_BYTES)?;
            if let Some(id) = &reference.terminal_event_id {
                validate_text(id, "terminal_event_id", MAX_ID_BYTES)?;
            }
            if let Some(hash) = &reference.terminal_event_hash {
                validate_text(hash, "terminal_event_hash", MAX_ID_BYTES)?;
            }
            if let Some(state_ref) = &reference.observed_state_ref {
                validate_text(state_ref, "observed_state_ref", MAX_ID_BYTES)?;
            }
            if let Some(outcome) = &reference.outcome {
                validate_text(outcome, "execution outcome", MAX_ID_BYTES)?;
            }
        }
        Ok(())
    }
}

/// Public proposal has no principal, operation_ref, policy, outcome, receipt,
/// or network mutation fields. Those are resolved/authored by the backend.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordDecisionInput {
    pub candidate: CandidateOperation,
    pub classification_proposal: Option<ClassificationProposal>,
    #[serde(default = "default_shadow_mode")]
    pub shadow_mode: bool,
}
fn default_shadow_mode() -> bool {
    true
}

impl RecordDecisionInput {
    pub fn validate(&self) -> Result<()> {
        self.candidate.validate()?;
        if let Some(classification) = &self.classification_proposal {
            classification.validate()?;
        }
        ensure!(
            self.shadow_mode,
            "only shadow-mode decision recording is implemented"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordDecisionOutput {
    pub decision: DecisionRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListDecisionsInput {
    #[schemars(range(min = 1, max = 100))]
    pub limit: u16,
    pub cursor: Option<String>,
}

impl ListDecisionsInput {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=MAX_PAGE_SIZE).contains(&self.limit),
            "limit is out of bounds"
        );
        if let Some(cursor) = &self.cursor {
            validate_text(cursor, "cursor", MAX_ID_BYTES)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecisionIdInput {
    pub decision_id: String,
}

impl DecisionIdInput {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.decision_id, "decision_id", MAX_ID_BYTES)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListDecisionsOutput {
    pub decisions: Vec<DecisionRecord>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDecisionOutput {
    pub decision: Option<DecisionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecheckInput {
    pub previous: GateInput,
    pub current: GateInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecheckOutput {
    pub result: GateResult,
}

/// Pure deterministic policy evaluation. Missing marker/policy evidence is a
/// hold; unresolved canonical references can never become an allow.
pub fn evaluate_gate(input: &GateInput) -> GateResult {
    let known_revision = input
        .policy_snapshot
        .as_ref()
        .map(|snapshot| snapshot.rule_revision)
        .filter(|revision| *revision > 0);
    let unresolved = |code: &str, detail: &str| GateResult {
        outcome: GateOutcome::Unresolved,
        rule_revision: known_revision,
        diagnostic: Some(GateDiagnostic {
            code: code.into(),
            detail: detail.into(),
        }),
    };
    let hold = |code: &str, detail: &str| GateResult {
        outcome: GateOutcome::Hold,
        rule_revision: known_revision,
        diagnostic: Some(GateDiagnostic {
            code: code.into(),
            detail: detail.into(),
        }),
    };
    if input.caller.validate().is_err()
        || input.operation.validate().is_err()
        || validate_text(&input.policy_owner_id, "policy_owner_id", MAX_ID_BYTES).is_err()
    {
        return hold(
            "invalid_context",
            "verified caller, policy owner, or resolved operation is malformed",
        );
    }
    if let Some(proposal) = &input.classification {
        if proposal.validate().is_err() {
            return unresolved(
                "invalid_classification",
                "classification proposal failed validation",
            );
        }
        let Some(current) = input
            .current_taxonomy_revision
            .filter(|revision| *revision > 0)
        else {
            return unresolved(
                "taxonomy_unavailable",
                "current taxonomy revision is unavailable",
            );
        };
        if proposal.taxonomy_revision != Some(current) {
            return unresolved(
                "taxonomy_revision_stale",
                "classification proposal does not pin the current taxonomy revision",
            );
        }
    }
    if !input.policy_read_succeeded {
        return hold(
            "policy_unavailable",
            "policy or protected-method marker could not be read",
        );
    }
    if !input.marker_lookup_succeeded {
        return hold(
            "marker_lookup_unavailable",
            "protected-method marker lookup did not complete",
        );
    }
    let Some(snapshot) = &input.policy_snapshot else {
        return hold("policy_unavailable", "policy snapshot is absent");
    };
    if snapshot.owner_id != input.policy_owner_id {
        return hold(
            "owner_mismatch",
            "policy snapshot is outside the configured policy-owner scope",
        );
    }
    if let Some(marker) = &snapshot.marker {
        if marker.validate().is_err()
            || marker.plugin_id != input.operation.plugin_id
            || marker.method_key != input.operation.method_key
            || marker.schema_hash != input.operation.schema_hash
        {
            return hold(
                "marker_mismatch",
                "protected-method marker does not match the resolved operation",
            );
        }
    }
    if let Some(required) = &input.operation.required_capability {
        if !input.caller.capabilities.contains(required) {
            return GateResult {
                outcome: GateOutcome::Denied,
                rule_revision: Some(snapshot.rule_revision),
                diagnostic: Some(GateDiagnostic {
                    code: "capability_denied".into(),
                    detail: "verified caller lacks the operation capability".into(),
                }),
            };
        }
    }
    if snapshot.rule_revision == 0 {
        return unresolved("policy_revision_unknown", "policy revision is missing");
    }
    if snapshot.records.len() > 256 {
        return hold(
            "policy_snapshot_oversized",
            "policy snapshot exceeds its bound",
        );
    }
    let mut seen_policy_ids = BTreeSet::new();
    for record in &snapshot.records {
        if record.validate().is_err() {
            return hold("policy_record_invalid", "policy record failed validation");
        }
        if record.owner_id != input.policy_owner_id {
            return hold(
                "owner_mismatch",
                "policy record is outside the configured policy-owner scope",
            );
        }
        if !seen_policy_ids.insert(record.policy_id.as_str()) {
            return hold(
                "policy_snapshot_duplicate",
                "policy snapshot contains duplicate policy identifiers",
            );
        }
    }
    for association in &input.tag_associations {
        if association.validate().is_err()
            || association.plugin_id != input.operation.plugin_id
            || association.method_key != input.operation.method_key
            || association.schema_hash != input.operation.schema_hash
        {
            return hold(
                "tag_association_mismatch",
                "tag association does not match the resolved sealed operation",
            );
        }
    }
    if let Some(tag) = &input.resolved_tag {
        if tag.validate().is_err() {
            return hold(
                "resolved_tag_invalid",
                "resolved tag failed registry validation",
            );
        }
        if !input
            .tag_associations
            .iter()
            .any(|association| association.tag_id == tag.tag_id)
        {
            return hold(
                "resolved_tag_unassociated",
                "resolved tag is not associated with the sealed operation",
            );
        }
        if input
            .classification
            .as_ref()
            .and_then(|proposal| proposal.tag.as_deref())
            .is_some_and(|proposed| proposed != tag.name)
        {
            return hold(
                "classification_resolution_mismatch",
                "resolved registry tag does not match the proposed label",
            );
        }
    }
    if input.classification.is_some() && input.resolved_tag.is_none() {
        return unresolved(
            "classification_tag_unresolved",
            "proposed classifier tag has no registry-resolved identity",
        );
    }
    // Explicit method denial wins before unresolved mappings on other rules.
    if snapshot.records.iter().any(|record| record.active && matches!(&record.rule,
        PolicyRule::DenyMethod { plugin_id, method_key, .. } if plugin_id == &input.operation.plugin_id && method_key == &input.operation.method_key)) {
        return GateResult { outcome: GateOutcome::Denied, rule_revision: Some(snapshot.rule_revision), diagnostic: Some(GateDiagnostic { code: "method_denied".into(), detail: "owner policy denies this operation".into() }) };
    }
    let mut applicable = Vec::new();
    for record in snapshot.records.iter().filter(|record| record.active) {
        let applies = match &record.rule {
            PolicyRule::DenyMethod { .. } => false,
            PolicyRule::TagConstraint { tag_id, .. } => {
                if !input
                    .tag_associations
                    .iter()
                    .any(|association| association.tag_id == *tag_id)
                {
                    false
                } else if input.classification.is_none() || input.resolved_tag.is_none() {
                    return hold(
                        "classification_unresolved",
                        "tag policy cannot be evaluated without a resolved numeric tag id",
                    );
                } else if input.resolved_tag.as_ref().map(|tag| tag.tag_id) != Some(*tag_id) {
                    false
                } else {
                    true
                }
            }
            PolicyRule::EgressRule { .. } => {
                input.requested_effect == RequestedEffect::Egress
                    && record
                        .canonical_mappings
                        .iter()
                        .any(|mapping| mapping.subid == input.operation.subid)
            }
        };
        if !applies {
            continue;
        }
        let matching_mappings = record
            .canonical_mappings
            .iter()
            .filter(|mapping| mapping.subid == input.operation.subid)
            .collect::<Vec<_>>();
        if matching_mappings.is_empty() {
            return unresolved(
                "canonical_mapping_missing",
                "applicable policy has no Qdrant mapping reference for the resolved method subid",
            );
        }
        if matching_mappings.iter().any(|mapping| !mapping.resolved) {
            return hold(
                "canonical_mapping_unresolved",
                "applicable policy references an unresolved canonical Qdrant mapping",
            );
        }
        applicable.push(record);
    }
    let mut mapping_generation: Option<&str> = None;
    for record in &applicable {
        for mapping in record
            .canonical_mappings
            .iter()
            .filter(|mapping| mapping.subid == input.operation.subid)
        {
            if mapping_generation
                .is_some_and(|generation| generation != mapping.generation.as_str())
            {
                return hold(
                    "canonical_mapping_generation_mismatch",
                    "applicable policies resolve through different Qdrant mapping generations",
                );
            }
            mapping_generation.get_or_insert(mapping.generation.as_str());
        }
    }
    if snapshot
        .marker
        .as_ref()
        .is_some_and(|marker| marker.requires_policy)
        && applicable.is_empty()
    {
        return unresolved(
            "applicable_policy_missing",
            "protected method has no applicable owner policy",
        );
    }
    let mut egress_permitted = false;
    for record in applicable {
        match &record.rule {
            PolicyRule::DenyMethod { .. } => {
                unreachable!("matching deny rules return before mapping checks")
            }
            PolicyRule::TagConstraint { disposition, .. } => {
                let violates = match disposition {
                    TagDisposition::LocalOnly => input.requested_effect == RequestedEffect::Egress,
                    TagDisposition::RequiresResidualValidation => {
                        input.requested_effect == RequestedEffect::Egress
                            && !input.residual_validation_passed
                    }
                };
                if violates {
                    return GateResult {
                        outcome: GateOutcome::Denied,
                        rule_revision: Some(snapshot.rule_revision),
                        diagnostic: Some(GateDiagnostic {
                            code: "tag_constraint".into(),
                            detail: "requested effect violates the registered tag constraint"
                                .into(),
                        }),
                    };
                }
            }
            PolicyRule::EgressRule {
                destination_class,
                allowed_provider_routes,
                required_policy_revision,
            } => {
                if input.destination_class.as_deref() != Some(destination_class.as_str())
                    || input
                        .provider_route
                        .as_ref()
                        .map_or(true, |route| !allowed_provider_routes.contains(route))
                    || *required_policy_revision != snapshot.rule_revision
                {
                    return GateResult {
                        outcome: GateOutcome::Denied,
                        rule_revision: Some(snapshot.rule_revision),
                        diagnostic: Some(GateDiagnostic {
                            code: "egress_policy_denied".into(),
                            detail: "destination, route, or policy revision is not permitted"
                                .into(),
                        }),
                    };
                }
                egress_permitted = true;
            }
        }
    }
    if input.requested_effect == RequestedEffect::Egress && !egress_permitted {
        return hold(
            "egress_policy_missing",
            "egress requires an applicable owner policy rule",
        );
    }
    GateResult {
        outcome: match input.requested_effect {
            RequestedEffect::Local => GateOutcome::AuthorizedLocal,
            RequestedEffect::Egress => GateOutcome::AuthorizedEgress,
        },
        rule_revision: Some(snapshot.rule_revision),
        diagnostic: None,
    }
}

/// Re-evaluate from current verified context; stale approvals are never reused.
pub fn recheck_before_dispatch(previous: &GateInput, current: &GateInput) -> GateResult {
    if previous.caller != current.caller || previous.operation != current.operation {
        return GateResult {
            outcome: GateOutcome::Hold,
            rule_revision: None,
            diagnostic: Some(GateDiagnostic {
                code: "context_changed".into(),
                detail: "caller or operation changed while queued".into(),
            }),
        };
    }
    let previous_revision = previous.policy_snapshot.as_ref().map(|snapshot| {
        (
            snapshot.owner_id.as_str(),
            snapshot.rule_revision,
            snapshot.marker.as_ref().map(|marker| marker.revision),
        )
    });
    let current_revision = current.policy_snapshot.as_ref().map(|snapshot| {
        (
            snapshot.owner_id.as_str(),
            snapshot.rule_revision,
            snapshot.marker.as_ref().map(|marker| marker.revision),
        )
    });
    if previous_revision != current_revision {
        return GateResult {
            outcome: GateOutcome::Hold,
            rule_revision: current
                .policy_snapshot
                .as_ref()
                .map(|snapshot| snapshot.rule_revision)
                .filter(|revision| *revision > 0),
            diagnostic: Some(GateDiagnostic {
                code: "policy_revision_changed".into(),
                detail: "policy or protected-method marker revision changed while queued".into(),
            }),
        };
    }
    evaluate_gate(current)
}
