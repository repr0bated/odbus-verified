//! Reviewed schema-tag metadata and owner-scoped policy contracts.
//!
//! Persistence and verified-owner injection are provided by MutationEngine;
//! this module declares the sealed API and pure domain invariants only.

use anyhow::{Result, bail};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema, SideEffect};
use simd_json::OwnedValue as Value;
use std::sync::Arc;

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
use super::schema_tag_registry_methods::*;

const NAME: &str = "schema_tag_registry";
const READ_CAP: &str = "cap.governance.schema-tag-registry.read@v1";
const REGISTER_TAG_CAP: &str = "cap.governance.schema-tag-registry.tag.register@v1";
const SET_MARKER_CAP: &str = "cap.governance.schema-tag-registry.marker.set@v1";
const AUTHOR_POLICY_CAP: &str = "cap.governance.schema-tag-registry.policy.author@v1";
const UPDATE_POLICY_CAP: &str = "cap.governance.schema-tag-registry.policy.update@v1";
const REVOKE_POLICY_CAP: &str = "cap.governance.schema-tag-registry.policy.revoke@v1";

pub struct SchemaTagRegistryPlugin;

#[async_trait]
impl StatePlugin for SchemaTagRegistryPlugin {
    fn name(&self) -> &str {
        NAME
    }
    fn version(&self) -> &str {
        "1.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(schema_tag_registry_schema())
    }
    async fn calculate_diff(&self, _: &Value, _: &Value) -> Result<StateDiff> {
        bail!("schema_tag_registry state changes use admitted methods")
    }
    async fn apply_state(&self, _: &StateDiff) -> Result<ApplyResult> {
        bail!("schema_tag_registry state changes use admitted methods")
    }
    async fn verify_state(&self, _: &Value) -> Result<bool> {
        Ok(false)
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        bail!("schema_tag_registry has no generic checkpoint surface")
    }
    async fn rollback(&self, _: &Checkpoint) -> Result<()> {
        bail!("schema_tag_registry has no generic rollback surface")
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

pub fn schema_tag_registry_schema() -> PluginSchema {
    let state_contract = serde_json::to_value(schemars::schema_for!(RegistryState))
        .expect("schema tag registry state schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        NAME,
        "1.0.0",
        "Owner-scoped reviewed schema-tag semantics, canonical mapping references, and revisioned policy rules",
        &state_contract,
    );
    schema.category = "governance".into();
    schema.capabilities.insert(
        READ_CAP.into(),
        CapabilityDecl {
            id: READ_CAP.into(),
            description: "Read owner-scoped tag and policy records".into(),
        },
    );
    for (id, description) in [
        (
            REGISTER_TAG_CAP,
            "Register reviewed tag semantics and associations",
        ),
        (
            SET_MARKER_CAP,
            "Set a protected-method policy marker after sealed-catalog resolution",
        ),
        (AUTHOR_POLICY_CAP, "Author an owner-scoped policy"),
        (UPDATE_POLICY_CAP, "Revise an owner-scoped policy"),
        (REVOKE_POLICY_CAP, "Revoke an owner-scoped policy"),
    ] {
        schema.capabilities.insert(
            id.into(),
            CapabilityDecl {
                id: id.into(),
                description: description.into(),
            },
        );
    }
    add_method::<ListTagsInput, ListTagsOutput>(
        &mut schema,
        "list_tags",
        SideEffect::Read,
        READ_CAP,
        "obs.software.schema-tag-registry.list-tags@v1",
    );
    add_method::<TagIdInput, GetTagOutput>(
        &mut schema,
        "get_tag",
        SideEffect::Read,
        READ_CAP,
        "obs.software.schema-tag-registry.get-tag@v1",
    );
    add_method::<RegisterTagInput, RegisterTagOutput>(
        &mut schema,
        "register_tag_semantics",
        SideEffect::Mutation,
        REGISTER_TAG_CAP,
        "mut.software.schema-tag-registry.register-tag-semantics@v1",
    );
    add_method::<ProtectedMethodMarkerKeyInput, GetProtectedMethodMarkerOutput>(
        &mut schema,
        "get_protected_method_marker",
        SideEffect::Read,
        READ_CAP,
        "obs.software.schema-tag-registry.get-protected-marker@v1",
    );
    add_method::<SetProtectedMethodMarkerInput, ProtectedMethodMarkerMutationOutput>(
        &mut schema,
        "set_protected_method_marker",
        SideEffect::Mutation,
        SET_MARKER_CAP,
        "mut.software.schema-tag-registry.set-protected-marker@v1",
    );
    add_method::<ListPoliciesInput, ListPoliciesOutput>(
        &mut schema,
        "list_policies",
        SideEffect::Read,
        READ_CAP,
        "obs.software.schema-tag-registry.list-policies@v1",
    );
    add_method::<PolicyIdInput, GetPolicyOutput>(
        &mut schema,
        "get_policy",
        SideEffect::Read,
        READ_CAP,
        "obs.software.schema-tag-registry.get-policy@v1",
    );
    add_method::<AuthorPolicyInput, PolicyMutationOutput>(
        &mut schema,
        "author_policy",
        SideEffect::Mutation,
        AUTHOR_POLICY_CAP,
        "mut.software.schema-tag-registry.author-policy@v1",
    );
    add_method::<UpdatePolicyInput, PolicyMutationOutput>(
        &mut schema,
        "update_policy",
        SideEffect::Mutation,
        UPDATE_POLICY_CAP,
        "mut.software.schema-tag-registry.update-policy@v1",
    );
    add_method::<RevokePolicyInput, PolicyMutationOutput>(
        &mut schema,
        "revoke_policy",
        SideEffect::Mutation,
        REVOKE_POLICY_CAP,
        "mut.software.schema-tag-registry.revoke-policy@v1",
    );
    schema
}

inventory::submit! {
    crate::default_registry::PluginReg::new(NAME, |_ctx| Arc::new(SchemaTagRegistryPlugin))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::schema_tag_registry_methods::{
        CanonicalMappingRef, CanonicalMappingRequest, PolicyRule, TagDisposition,
    };

    fn mapping(resolved: bool) -> CanonicalMappingRef {
        CanonicalMappingRef {
            mapping_id: "mapping-1".into(),
            subid: "mut.network.openflow.flow.add@v2".into(),
            generation: "gen-a".into(),
            source: "OSCAL mapping dataset".into(),
            source_ref: "qdrant://compliance_general/item-1".into(),
            source_sha256: "a".repeat(64),
            direction: "maps_to".into(),
            resolved,
        }
    }

    fn mapping_request() -> CanonicalMappingRequest {
        CanonicalMappingRequest {
            mapping_id: "mapping-1".into(),
            subid: "mut.network.openflow.flow.add@v2".into(),
            direction: "maps_to".into(),
        }
    }

    #[test]
    fn tag_schema_is_closed_and_policy_methods_have_capability_closure() {
        let schema = schema_tag_registry_schema();
        assert!(!schema.methods.is_empty());
        assert!(schema.validate_capability_closure().ok());
        assert!(schema.fields.contains_key("revision"));
        assert!(schema.fields.contains_key("tags"));
        assert!(schema.fields.contains_key("policies"));
        assert!(schema.fields.contains_key("protected_methods"));
        assert!(
            schema.public_state.paths.is_empty(),
            "registry policy state must remain private"
        );
    }

    #[test]
    fn mapping_is_provenance_reference_not_copied_taxonomy() {
        let mut policy = PolicyRecord {
            owner_id: "owner-a".into(),
            policy_id: "deny-flow".into(),
            revision: 1,
            active: true,
            revocation_reason: None,
            rule: PolicyRule::TagConstraint {
                tag_id: 7,
                disposition: TagDisposition::LocalOnly,
            },
            canonical_mappings: vec![mapping(false)],
        };
        assert!(policy.validate().is_ok());
        assert_eq!(policy.canonical_mappings[0].generation, "gen-a");
        assert!(!policy.canonical_mappings[0].resolved);
        // Resolution failure is retained as evidence; the gate, not this DTO,
        // turns it into hold/unresolved.
        policy.canonical_mappings[0].resolved = true;
        assert!(policy.validate().is_ok());
    }

    #[test]
    fn policy_mutations_are_owner_scoped_revisioned_and_retain_revocation_reason() {
        let mut records = Vec::new();
        let created = author_policy(
            &mut records,
            "owner-a",
            AuthorPolicyInput {
                policy_id: "tag-policy".into(),
                expected_revision: 0,
                rule: PolicyRule::TagConstraint {
                    tag_id: 7,
                    disposition: TagDisposition::LocalOnly,
                },
                canonical_mappings: vec![mapping_request()],
            },
            vec![mapping(true)],
        )
        .unwrap();
        assert_eq!(created.revision, 1);
        let update = UpdatePolicyInput {
            policy_id: "tag-policy".into(),
            expected_revision: 1,
            rule: PolicyRule::TagConstraint {
                tag_id: 7,
                disposition: TagDisposition::RequiresResidualValidation,
            },
            canonical_mappings: vec![mapping_request()],
        };
        assert!(
            apply_policy_update(&mut records, "owner-b", update.clone(), vec![mapping(true)])
                .is_err()
        );
        let updated =
            apply_policy_update(&mut records, "owner-a", update, vec![mapping(true)]).unwrap();
        assert_eq!(updated.revision, 2);
        let revoked = revoke_policy(
            &mut records,
            "owner-a",
            "tag-policy",
            2,
            "superseded by review",
        )
        .unwrap();
        assert!(!revoked.active);
        assert_eq!(revoked.revision, 3);
        assert_eq!(
            revoked.revocation_reason.as_deref(),
            Some("superseded by review")
        );
        assert!(revoke_policy(&mut records, "owner-a", "tag-policy", 3, "again").is_err());
    }

    #[test]
    fn tag_registration_uses_expected_revision_and_server_authored_revision() {
        let mut tags = Vec::new();
        let proposed = RegisterTagInput {
            tag_id: 7,
            name: "local-only".into(),
            description: "must remain local".into(),
            default_disposition: Some(TagDisposition::LocalOnly),
            associations: vec![],
            expected_revision: 0,
            openflow_binding: None,
        };
        let created = apply_tag_registration(&mut tags, proposed.clone(), vec![], None).unwrap();
        assert_eq!(created.revision, 1);
        assert!(apply_tag_registration(&mut tags, proposed.clone(), vec![], None).is_err());
        let updated = apply_tag_registration(
            &mut tags,
            RegisterTagInput {
                expected_revision: 1,
                ..proposed
            },
            vec![],
            None,
        )
        .unwrap();
        assert_eq!(updated.revision, 2);
    }

    #[test]
    fn marker_mutation_pins_sealed_method_and_revisions() {
        let mut markers = Vec::new();
        let input = SetProtectedMethodMarkerInput {
            marker: ProtectedMethodMarkerRequest {
                plugin_id: "openflow".into(),
                method_key: "add_flow".into(),
                schema_hash: "schema-a".into(),
                requires_policy: true,
            },
            expected_revision: 0,
        };
        let marker = apply_protected_method_marker(
            &mut markers,
            input.clone(),
            "openflow",
            "add_flow",
            "schema-a",
        )
        .unwrap();
        assert_eq!(marker.revision, 1);
        assert!(
            apply_protected_method_marker(&mut markers, input, "openflow", "add_flow", "schema-b")
                .is_err()
        );
        let update = SetProtectedMethodMarkerInput {
            marker: ProtectedMethodMarkerRequest {
                plugin_id: "openflow".into(),
                method_key: "add_flow".into(),
                schema_hash: "schema-a".into(),
                requires_policy: false,
            },
            expected_revision: 1,
        };
        assert_eq!(
            apply_protected_method_marker(&mut markers, update, "openflow", "add_flow", "schema-a")
                .unwrap()
                .revision,
            2
        );
    }
}
