//! Owner-scoped decision records and pure deterministic gate contracts.
//!
//! The bridge supplies the verified caller, sealed-catalog resolution, policy
//! snapshot and canonical persistence; this plugin declares that API and does
//! not create an alternate dispatch or state-write path.

use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use simd_json::OwnedValue as Value;
use std::sync::Arc;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
use super::routing_decision_methods::*;
use super::schema_tag_registry_methods::PolicyRule;

const NAME: &str = "routing_decision";
const READ_CAP: &str = "cap.governance.routing-decision.read@v1";
const WRITE_CAP: &str = "cap.governance.routing-decision.record@v1";

pub struct RoutingDecisionPlugin;

#[async_trait]
impl StatePlugin for RoutingDecisionPlugin {
    fn name(&self) -> &str {
        NAME
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(routing_decision_schema())
    }
    async fn calculate_diff(&self, _: &Value, _: &Value) -> Result<StateDiff> {
        bail!("routing decisions use the private admitted publisher")
    }
    async fn apply_state(&self, _: &StateDiff) -> Result<ApplyResult> {
        bail!("routing decisions use the private admitted publisher")
    }
    async fn verify_state(&self, _: &Value) -> Result<bool> {
        Ok(false)
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        bail!("routing_decision has no generic checkpoint surface")
    }
    async fn rollback(&self, _: &Checkpoint) -> Result<()> {
        bail!("routing_decision has no generic rollback surface")
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

fn add_method<I: schemars::JsonSchema, O: schemars::JsonSchema>(
    schema: &mut PluginSchema,
    name: &str,
    effect: SideEffect,
    capability: &str,
    subid: &str,
) {
    let idempotent = effect == SideEffect::Read;
    schema.methods.insert(
        name.to_owned(),
        method_decl_from_schemars_with_output::<I, O>(name, effect, idempotent, capability, subid),
    );
}

pub fn routing_decision_schema() -> PluginSchema {
    let state_contract = serde_json::to_value(schemars::schema_for!(DecisionState))
        .expect("routing decision state schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        NAME,
        "1.0.0",
        "Verified-caller scoped shadow decisions and deterministic policy evaluation",
        &state_contract,
    );
    schema.category = "governance".into();
    schema.capabilities.insert(
        READ_CAP.into(),
        CapabilityDecl {
            id: READ_CAP.into(),
            description: "Read decision records within the verified owner scope".into(),
        },
    );
    schema.capabilities.insert(
        WRITE_CAP.into(),
        CapabilityDecl {
            id: WRITE_CAP.into(),
            description: "Record proposal-only shadow decisions within the verified owner scope"
                .into(),
        },
    );
    add_method::<ListDecisionsInput, ListDecisionsOutput>(
        &mut schema,
        "list_decisions",
        SideEffect::Read,
        READ_CAP,
        "obs.software.routing-decision.list@v1",
    );
    add_method::<DecisionIdInput, GetDecisionOutput>(
        &mut schema,
        "get_decision",
        SideEffect::Read,
        READ_CAP,
        "obs.software.routing-decision.get@v1",
    );
    add_method::<RecordDecisionInput, RecordDecisionOutput>(
        &mut schema,
        "record_shadow_decision",
        SideEffect::Mutation,
        WRITE_CAP,
        "mut.software.routing-decision.record-shadow@v1",
    );
    schema
}

/// Build the trusted decision envelope from server-resolved inputs. Candidate
/// strings, identity, policy, outcome and execution evidence are never trusted
/// from `RecordDecisionInput`.
pub fn build_shadow_record(
    decision_id: String,
    proposal: RecordDecisionInput,
    caller: VerifiedCaller,
    operation: ResolvedOperationRef,
    gate_input: GateInput,
    openflow_binding: Option<super::schema_tag_registry_methods::OpenFlowBindingRef>,
) -> Result<DecisionRecord> {
    proposal.validate()?;
    caller.validate()?;
    operation.validate()?;
    ensure!(
        proposal.candidate.plugin_id == operation.plugin_id,
        "candidate plugin does not match resolved operation"
    );
    ensure!(
        proposal.candidate.method_key == operation.method_key,
        "candidate method does not match resolved operation"
    );
    ensure!(
        proposal.candidate.schema_hash == operation.schema_hash,
        "candidate schema hash is stale"
    );
    ensure!(
        gate_input.caller == caller && gate_input.operation == operation,
        "gate context does not match trusted record context"
    );
    ensure!(
        gate_input.classification == proposal.classification_proposal,
        "gate classification does not match recorded proposal"
    );
    if let Some(binding) = &openflow_binding {
        binding.validate()?;
        ensure!(
            operation.plugin_id == binding.plugin_id
                && operation.method_key == binding.method_key
                && operation.schema_hash == binding.schema_hash,
            "OpenFlow binding does not match resolved operation"
        );
        if let Some(tag_id) = gate_input.resolved_tag.as_ref().map(|tag| tag.tag_id) {
            ensure!(
                tag_id == binding.network_tag_id,
                "OpenFlow binding does not match resolved network tag"
            );
        }
    }
    let mut gate_result = evaluate_gate(&gate_input);
    let mut canonical_mappings: Vec<super::schema_tag_registry_methods::CanonicalMappingRef> =
        if gate_result.outcome == GateOutcome::Denied {
            // A decisive capability/method/tag denial does not need to carry
            // mapping evidence from unrelated active policy rows.
            Vec::new()
        } else {
            gate_input
                .policy_snapshot
                .as_ref()
                .map(|snapshot| {
                    snapshot
                        .records
                        .iter()
                        .filter(|record| {
                            record.active
                                && match &record.rule {
                                    PolicyRule::DenyMethod {
                                        plugin_id,
                                        method_key,
                                        ..
                                    } => {
                                        plugin_id == &operation.plugin_id
                                            && method_key == &operation.method_key
                                    }
                                    PolicyRule::TagConstraint { tag_id, .. } => {
                                        gate_input.resolved_tag.as_ref().map(|tag| tag.tag_id)
                                            == Some(*tag_id)
                                    }
                                    PolicyRule::EgressRule { .. } => {
                                        gate_input.requested_effect == RequestedEffect::Egress
                                    }
                                }
                        })
                        .flat_map(|record| {
                            record
                                .canonical_mappings
                                .iter()
                                .filter(|mapping| mapping.subid == operation.subid)
                                .cloned()
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
    if canonical_mappings.len() > 64 {
        gate_result = GateResult {
            outcome: GateOutcome::Hold,
            rule_revision: gate_result.rule_revision,
            diagnostic: Some(GateDiagnostic {
                code: "canonical_mapping_capacity".into(),
                detail: "applicable mapping evidence exceeds the decision bound".into(),
            }),
        };
        canonical_mappings.clear();
    }
    let mapping_generation = canonical_mappings
        .first()
        .map(|mapping| mapping.generation.as_str());
    if gate_result.outcome != GateOutcome::Denied
        && mapping_generation.is_some_and(|generation| {
            canonical_mappings
                .iter()
                .any(|mapping| mapping.generation.as_str() != generation)
        })
    {
        gate_result = GateResult {
            outcome: GateOutcome::Hold,
            rule_revision: gate_result.rule_revision,
            diagnostic: Some(GateDiagnostic {
                code: "canonical_mapping_generation_mismatch".into(),
                detail: "decision evidence spans different Qdrant generations".into(),
            }),
        };
        canonical_mappings.clear();
    }
    let record = DecisionRecord {
        decision_id,
        verified_identity: caller,
        operation_ref: operation,
        classification_proposal: proposal.classification_proposal,
        classification_origin_verified: false,
        resolved_tag: gate_input.resolved_tag,
        policy_owner_id: gate_input
            .policy_snapshot
            .as_ref()
            .map(|snapshot| snapshot.owner_id.clone()),
        policy_revision: gate_result.rule_revision,
        canonical_mappings,
        gate_result,
        openflow_binding,
        execution_ref: None,
        shadow_mode: true,
    };
    record.validate()?;
    Ok(record)
}

inventory::submit! {
    crate::default_registry::PluginReg::new(NAME, |_ctx| Arc::new(RoutingDecisionPlugin))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::schema_tag_registry_methods::{
        CanonicalMappingRef, PolicyRecord, PolicyRule, ProtectedMethodMarker, ResolvedTagRef,
        SchemaTagAssociation, TagDisposition,
    };
    use std::collections::BTreeSet;

    fn base() -> GateInput {
        GateInput {
            caller: VerifiedCaller {
                principal_id: "principal-a".into(),
                session_id: "session-a".into(),
                owner_id: "owner-a".into(),
                container: None,
                project_scope: None,
                capabilities: BTreeSet::from(["cap.network.openflow.write@v2".into()]),
            },
            policy_owner_id: "registry-owner".into(),
            operation: ResolvedOperationRef {
                plugin_id: "openflow".into(),
                method_key: "add_flow".into(),
                transport_name: "AddFlow".into(),
                schema_hash: "schema-abc".into(),
                schema_version: "2.0.0".into(),
                catalog_hash: "catalog-abc".into(),
                subid: "mut.network.openflow.flow.add@v2".into(),
                required_capability: Some("cap.network.openflow.write@v2".into()),
            },
            classification: None,
            current_taxonomy_revision: Some(1),
            resolved_tag: None,
            tag_associations: vec![],
            policy_snapshot: Some(PolicySnapshot {
                owner_id: "registry-owner".into(),
                rule_revision: 1,
                marker: Some(ProtectedMethodMarker {
                    plugin_id: "openflow".into(),
                    method_key: "add_flow".into(),
                    schema_hash: "schema-abc".into(),
                    requires_policy: true,
                    revision: 1,
                }),
                records: Vec::new(),
            }),
            policy_read_succeeded: true,
            marker_lookup_succeeded: true,
            requested_effect: RequestedEffect::Local,
            residual_validation_passed: false,
            destination_class: None,
            provider_route: None,
        }
    }

    #[test]
    fn missing_or_unreadable_marker_holds() {
        let mut input = base();
        input.policy_read_succeeded = false;
        assert_eq!(evaluate_gate(&input).outcome, GateOutcome::Hold);
        input.policy_read_succeeded = true;
        input.policy_snapshot.as_mut().unwrap().marker = None;
        input.marker_lookup_succeeded = false;
        assert_eq!(evaluate_gate(&input).outcome, GateOutcome::Hold);
    }

    #[test]
    fn successful_lookup_confirming_no_marker_leaves_existing_admission_as_authority() {
        let mut input = base();
        input.policy_snapshot.as_mut().unwrap().marker = None;
        assert_eq!(evaluate_gate(&input).outcome, GateOutcome::AuthorizedLocal);
    }

    #[test]
    fn required_marker_without_applicable_policy_is_unresolved() {
        assert_eq!(evaluate_gate(&base()).outcome, GateOutcome::Unresolved);
    }

    #[test]
    fn denied_capability_and_method_deny_are_deterministic() {
        let mut input = base();
        input.caller.capabilities.clear();
        assert_eq!(evaluate_gate(&input).outcome, GateOutcome::Denied);
        input
            .caller
            .capabilities
            .insert("cap.network.openflow.write@v2".into());
        input.classification = Some(ClassificationProposal {
            tag: Some("local-only".into()),
            asserted_origin: ClassificationOrigin::UserSelection,
            taxonomy_revision: Some(1),
            classifier_version: None,
            uncertainty: None,
        });
        input.resolved_tag = Some(ResolvedTagRef {
            tag_id: 7,
            revision: 1,
            name: "local-only".into(),
        });
        input.tag_associations.push(SchemaTagAssociation {
            plugin_id: "openflow".into(),
            method_key: "add_flow".into(),
            schema_hash: "schema-abc".into(),
            tag_id: 7,
        });
        input
            .policy_snapshot
            .as_mut()
            .unwrap()
            .records
            .push(PolicyRecord {
                owner_id: "registry-owner".into(),
                policy_id: "unresolved-tag".into(),
                revision: 1,
                active: true,
                revocation_reason: None,
                rule: PolicyRule::TagConstraint {
                    tag_id: 7,
                    disposition: TagDisposition::LocalOnly,
                },
                canonical_mappings: vec![CanonicalMappingRef {
                    mapping_id: "mapping-unresolved".into(),
                    subid: "mut.network.openflow.flow.add@v2".into(),
                    generation: "qdrant-gen-old".into(),
                    source: "OSCAL canonical dataset".into(),
                    source_ref: "qdrant://oscal/unresolved".into(),
                    source_sha256: "b".repeat(64),
                    direction: "maps_to".into(),
                    resolved: false,
                }],
            });
        input
            .policy_snapshot
            .as_mut()
            .unwrap()
            .records
            .push(PolicyRecord {
                owner_id: "registry-owner".into(),
                policy_id: "deny-add".into(),
                revision: 1,
                active: true,
                revocation_reason: None,
                rule: PolicyRule::DenyMethod {
                    plugin_id: "openflow".into(),
                    method_key: "add_flow".into(),
                    reason: "review required".into(),
                },
                canonical_mappings: vec![CanonicalMappingRef {
                    mapping_id: "mapping-a".into(),
                    subid: "mut.network.openflow.flow.add@v2".into(),
                    generation: "qdrant-gen-a".into(),
                    source: "OSCAL canonical dataset".into(),
                    source_ref: "qdrant://oscal/item-a".into(),
                    source_sha256: "a".repeat(64),
                    direction: "maps_to".into(),
                    resolved: true,
                }],
            });
        assert_eq!(evaluate_gate(&input).outcome, GateOutcome::Denied);
    }

    #[test]
    fn unresolved_qdrant_reference_and_missing_egress_rule_fail_closed() {
        let mut input = base();
        input.classification = Some(ClassificationProposal {
            tag: Some("local-only".into()),
            asserted_origin: ClassificationOrigin::UserSelection,
            taxonomy_revision: Some(1),
            classifier_version: None,
            uncertainty: None,
        });
        input.resolved_tag = Some(ResolvedTagRef {
            tag_id: 7,
            revision: 1,
            name: "local-only".into(),
        });
        input.tag_associations.push(SchemaTagAssociation {
            plugin_id: "openflow".into(),
            method_key: "add_flow".into(),
            schema_hash: "schema-abc".into(),
            tag_id: 7,
        });
        input
            .policy_snapshot
            .as_mut()
            .unwrap()
            .records
            .push(PolicyRecord {
                owner_id: "registry-owner".into(),
                policy_id: "deny-add".into(),
                revision: 1,
                active: true,
                revocation_reason: None,
                rule: PolicyRule::TagConstraint {
                    tag_id: 7,
                    disposition: TagDisposition::LocalOnly,
                },
                canonical_mappings: vec![CanonicalMappingRef {
                    mapping_id: "mapping-a".into(),
                    subid: "mut.network.openflow.flow.add@v2".into(),
                    generation: "qdrant-gen-a".into(),
                    source: "OSCAL canonical dataset".into(),
                    source_ref: "qdrant://oscal/item-a".into(),
                    source_sha256: "a".repeat(64),
                    direction: "maps_to".into(),
                    resolved: false,
                }],
            });
        assert_eq!(evaluate_gate(&input).outcome, GateOutcome::Hold);

        let mut egress = base();
        egress.requested_effect = RequestedEffect::Egress;
        assert_eq!(evaluate_gate(&egress).outcome, GateOutcome::Unresolved);
    }

    #[test]
    fn mixed_generations_across_applicable_policies_hold_without_publishing_mapping_evidence() {
        let mut input = base();
        input.classification = Some(ClassificationProposal {
            tag: Some("local-only".into()),
            asserted_origin: ClassificationOrigin::UserSelection,
            taxonomy_revision: Some(1),
            classifier_version: None,
            uncertainty: None,
        });
        input.resolved_tag = Some(ResolvedTagRef {
            tag_id: 7,
            revision: 1,
            name: "local-only".into(),
        });
        input.tag_associations.push(SchemaTagAssociation {
            plugin_id: "openflow".into(),
            method_key: "add_flow".into(),
            schema_hash: "schema-abc".into(),
            tag_id: 7,
        });
        for (policy_id, generation) in [
            ("local-a", "qdrant-generation-a"),
            ("local-b", "qdrant-generation-b"),
        ] {
            input
                .policy_snapshot
                .as_mut()
                .unwrap()
                .records
                .push(PolicyRecord {
                    owner_id: "registry-owner".into(),
                    policy_id: policy_id.into(),
                    revision: 1,
                    active: true,
                    revocation_reason: None,
                    rule: PolicyRule::TagConstraint {
                        tag_id: 7,
                        disposition: TagDisposition::LocalOnly,
                    },
                    canonical_mappings: vec![CanonicalMappingRef {
                        mapping_id: format!("mapping-{policy_id}"),
                        subid: "mut.network.openflow.flow.add@v2".into(),
                        generation: generation.into(),
                        source: "OSCAL canonical dataset".into(),
                        source_ref: format!("qdrant://oscal/{policy_id}"),
                        source_sha256: "a".repeat(64),
                        direction: "maps_to".into(),
                        resolved: true,
                    }],
                });
        }
        let proposal = RecordDecisionInput {
            candidate: CandidateOperation {
                plugin_id: "openflow".into(),
                method_key: "add_flow".into(),
                schema_hash: "schema-abc".into(),
            },
            classification_proposal: input.classification.clone(),
            shadow_mode: true,
        };
        let record = build_shadow_record(
            "decision-mixed-generation".into(),
            proposal,
            input.caller.clone(),
            input.operation.clone(),
            input,
            None,
        )
        .unwrap();
        assert_eq!(record.gate_result.outcome, GateOutcome::Hold);
        assert_eq!(
            record.gate_result.diagnostic.as_ref().unwrap().code,
            "canonical_mapping_generation_mismatch"
        );
        assert!(record.canonical_mappings.is_empty());
    }

    #[test]
    fn egress_requires_explicit_route_bound_rule_and_revision() {
        let mut input = base();
        input.requested_effect = RequestedEffect::Egress;
        input.destination_class = Some("public-web".into());
        input.provider_route = Some("provider-a".into());
        input
            .policy_snapshot
            .as_mut()
            .unwrap()
            .records
            .push(PolicyRecord {
                owner_id: "registry-owner".into(),
                policy_id: "egress-web".into(),
                revision: 1,
                active: true,
                revocation_reason: None,
                rule: PolicyRule::EgressRule {
                    destination_class: "public-web".into(),
                    allowed_provider_routes: vec!["provider-a".into()],
                    required_policy_revision: 1,
                },
                canonical_mappings: vec![CanonicalMappingRef {
                    mapping_id: "mapping-a".into(),
                    subid: "mut.network.openflow.flow.add@v2".into(),
                    generation: "qdrant-gen-a".into(),
                    source: "OSCAL canonical dataset".into(),
                    source_ref: "qdrant://oscal/item-a".into(),
                    source_sha256: "a".repeat(64),
                    direction: "maps_to".into(),
                    resolved: true,
                }],
            });
        assert_eq!(evaluate_gate(&input).outcome, GateOutcome::AuthorizedEgress);
        input.provider_route = Some("provider-b".into());
        assert_eq!(evaluate_gate(&input).outcome, GateOutcome::Denied);
    }

    #[test]
    fn proposal_cannot_author_identity_or_gate_outcome() {
        let forged = serde_json::from_value::<RecordDecisionInput>(serde_json::json!({
            "candidate":{"plugin_id":"openflow","method_key":"add_flow","schema_hash":"schema-abc"},
            "classification_proposal":null,"shadow_mode":true,"verified_identity":{"principal_id":"forged"},"gate_outcome":"authorized_local"
        }));
        assert!(forged.is_err());
        let proposal: RecordDecisionInput = serde_json::from_value(serde_json::json!({
            "candidate":{"plugin_id":"openflow","method_key":"add_flow","schema_hash":"schema-abc"},
            "classification_proposal":{"tag":"asserted-label","asserted_origin":"model_proposal","taxonomy_revision":1,"classifier_version":"caller-claimed-model","uncertainty":0.1},"shadow_mode":true
        })).unwrap();
        let mut input = base();
        input
            .policy_snapshot
            .as_mut()
            .unwrap()
            .marker
            .as_mut()
            .unwrap()
            .requires_policy = false;
        input.classification = proposal.classification_proposal.clone();
        input.resolved_tag = Some(ResolvedTagRef {
            tag_id: 9,
            revision: 1,
            name: "asserted-label".into(),
        });
        input.tag_associations.push(SchemaTagAssociation {
            plugin_id: "openflow".into(),
            method_key: "add_flow".into(),
            schema_hash: "schema-abc".into(),
            tag_id: 9,
        });
        let record = build_shadow_record(
            "decision-a".into(),
            proposal,
            input.caller.clone(),
            input.operation.clone(),
            input,
            None,
        )
        .unwrap();
        assert_eq!(record.verified_identity.principal_id, "principal-a");
        assert_eq!(record.gate_result.outcome, GateOutcome::AuthorizedLocal);
        assert!(!record.classification_origin_verified);
        assert_eq!(
            record
                .classification_proposal
                .as_ref()
                .unwrap()
                .asserted_origin,
            ClassificationOrigin::ModelProposal
        );
    }

    #[test]
    fn caller_asserted_classification_origin_and_stale_taxonomy_never_become_verified_evidence() {
        let mut input = base();
        input.classification = Some(ClassificationProposal {
            tag: Some("local-only".into()),
            asserted_origin: ClassificationOrigin::ModelProposal,
            taxonomy_revision: Some(1),
            classifier_version: Some("caller-claimed-model".into()),
            uncertainty: Some(0.1),
        });
        input.current_taxonomy_revision = Some(2);
        let stale = evaluate_gate(&input);
        assert_eq!(stale.outcome, GateOutcome::Unresolved);
        assert_eq!(
            stale.diagnostic.as_ref().unwrap().code,
            "taxonomy_revision_stale"
        );
        input.current_taxonomy_revision = None;
        let unavailable = evaluate_gate(&input);
        assert_eq!(unavailable.outcome, GateOutcome::Unresolved);
        assert_eq!(
            unavailable.diagnostic.as_ref().unwrap().code,
            "taxonomy_unavailable"
        );
        input.current_taxonomy_revision = Some(1);
        let unresolved_tag = evaluate_gate(&input);
        assert_eq!(unresolved_tag.outcome, GateOutcome::Unresolved);
        assert_eq!(
            unresolved_tag.diagnostic.as_ref().unwrap().code,
            "classification_tag_unresolved"
        );
    }

    #[test]
    fn schema_exposes_only_typed_admitted_methods() {
        let schema = routing_decision_schema();
        assert!(schema.validate_capability_closure().ok());
        assert_eq!(
            schema
                .methods
                .get("record_shadow_decision")
                .unwrap()
                .side_effect,
            SideEffect::Mutation
        );
        assert!(schema.fields.contains_key("revision"));
        assert!(schema.fields.contains_key("records"));
        assert!(
            schema.public_state.paths.is_empty(),
            "private decision records must not enter shared state projection"
        );
    }

    #[test]
    fn recheck_rejects_policy_revision_change_after_queueing() {
        let previous = base();
        let mut current = previous.clone();
        current.policy_snapshot.as_mut().unwrap().rule_revision += 1;
        let result = recheck_before_dispatch(&previous, &current);
        assert_eq!(result.outcome, GateOutcome::Hold);
        assert_eq!(
            result.diagnostic.as_ref().unwrap().code,
            "policy_revision_changed"
        );
    }
}
