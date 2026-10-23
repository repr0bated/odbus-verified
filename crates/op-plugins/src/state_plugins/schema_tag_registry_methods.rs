//! Typed, owner-scoped schema-tag and routing-policy contracts.

use anyhow::{Result, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_ID_BYTES: usize = 192;
pub const MAX_TEXT_BYTES: usize = 512;
pub const MAX_PAGE_SIZE: u16 = 100;

pub fn validate_text(value: &str, field: &str, max: usize) -> Result<()> {
    ensure!(!value.is_empty(), "{field} is empty");
    ensure!(value.len() <= max, "{field} exceeds {max} bytes");
    ensure!(
        !value.chars().any(char::is_control),
        "{field} contains control characters"
    );
    Ok(())
}

/// Reference to a network binding owned by the sealed `openflow` PluginSchema.
/// It identifies the contract and revision, but deliberately carries no FlowEntry
/// and selects no VLAN/register encoding.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OpenFlowBindingRef {
    pub binding_id: String,
    pub revision: u64,
    pub plugin_id: String,
    pub method_key: String,
    pub schema_hash: String,
    /// Numeric registry tag identity; interpretation belongs to OpenFlow.
    pub network_tag_id: u32,
}

impl OpenFlowBindingRef {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.binding_id, "binding_id", MAX_ID_BYTES)?;
        ensure!(self.revision > 0, "binding revision must be positive");
        ensure!(
            self.plugin_id == "openflow",
            "network binding must reference openflow"
        );
        ensure!(
            matches!(
                self.method_key.as_str(),
                "add_flow" | "modify_flow" | "delete_flow"
            ),
            "unsupported OpenFlow binding method"
        );
        validate_text(&self.schema_hash, "schema_hash", MAX_ID_BYTES)?;
        ensure!(self.network_tag_id > 0, "network_tag_id must be positive");
        Ok(())
    }
}

/// Public binding request. The admitted resolver supplies the contract
/// revision, plugin method, and schema hash before persistence.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OpenFlowBindingRequest {
    pub binding_id: String,
    pub network_tag_id: u32,
}

impl OpenFlowBindingRequest {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.binding_id, "binding_id", MAX_ID_BYTES)?;
        ensure!(self.network_tag_id > 0, "network_tag_id must be positive");
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SchemaTagAssociation {
    pub plugin_id: String,
    pub method_key: String,
    pub schema_hash: String,
    pub tag_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SchemaTagAssociationRequest {
    pub plugin_id: String,
    pub method_key: String,
}

impl SchemaTagAssociationRequest {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.plugin_id, "association plugin_id", MAX_ID_BYTES)?;
        validate_text(&self.method_key, "association method_key", MAX_ID_BYTES)
    }
}

impl SchemaTagAssociation {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.plugin_id, "association plugin_id", MAX_ID_BYTES)?;
        validate_text(&self.method_key, "association method_key", MAX_ID_BYTES)?;
        validate_text(&self.schema_hash, "association schema_hash", MAX_ID_BYTES)?;
        ensure!(self.tag_id > 0, "association tag_id must be positive");
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TagDescriptor {
    pub tag_id: u32,
    pub name: String,
    pub description: String,
    pub default_disposition: Option<TagDisposition>,
    pub revision: u64,
    pub associations: Vec<SchemaTagAssociation>,
    pub openflow_binding: Option<OpenFlowBindingRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ResolvedTagRef {
    pub tag_id: u32,
    pub revision: u64,
    pub name: String,
}

impl ResolvedTagRef {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.tag_id > 0 && self.revision > 0,
            "resolved tag id/revision must be positive"
        );
        validate_text(&self.name, "resolved tag name", MAX_ID_BYTES)
    }
}

impl TagDescriptor {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.tag_id > 0, "tag_id must be positive");
        validate_text(&self.name, "tag name", MAX_ID_BYTES)?;
        validate_text(&self.description, "tag description", MAX_TEXT_BYTES)?;
        ensure!(self.revision > 0, "tag revision must be positive");
        ensure!(
            self.associations.len() <= 256,
            "too many schema-tag associations"
        );
        let mut seen_associations = std::collections::BTreeSet::new();
        for association in &self.associations {
            association.validate()?;
            ensure!(
                association.tag_id == self.tag_id,
                "association tag_id mismatch"
            );
            ensure!(
                seen_associations.insert((
                    association.plugin_id.as_str(),
                    association.method_key.as_str(),
                    association.schema_hash.as_str()
                )),
                "duplicate schema-tag association"
            );
        }
        if let Some(binding) = &self.openflow_binding {
            binding.validate()?;
            ensure!(
                binding.network_tag_id == self.tag_id,
                "OpenFlow binding tag_id mismatch"
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProtectedMethodMarker {
    pub plugin_id: String,
    pub method_key: String,
    pub schema_hash: String,
    pub requires_policy: bool,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProtectedMethodMarkerRequest {
    pub plugin_id: String,
    pub method_key: String,
    pub schema_hash: String,
    pub requires_policy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetProtectedMethodMarkerInput {
    pub marker: ProtectedMethodMarkerRequest,
    pub expected_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProtectedMethodMarkerKeyInput {
    pub plugin_id: String,
    pub method_key: String,
    pub schema_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetProtectedMethodMarkerOutput {
    pub marker: Option<ProtectedMethodMarker>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProtectedMethodMarkerMutationOutput {
    pub marker: ProtectedMethodMarker,
}

/// Provenance pointer into the maintained canonical OSCAL/Qdrant mappings.
/// This is a reference, never a copied mapping row or a second graph authority.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanonicalMappingRef {
    pub mapping_id: String,
    pub subid: String,
    pub generation: String,
    pub source: String,
    pub source_ref: String,
    pub source_sha256: String,
    pub direction: String,
    pub resolved: bool,
}

/// Untrusted policy-author request. The admitted resolver must fetch this ID
/// and construct `CanonicalMappingRef`; clients cannot assert provenance or
/// resolution status.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CanonicalMappingRequest {
    pub mapping_id: String,
    pub subid: String,
    pub direction: String,
}

impl CanonicalMappingRequest {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.mapping_id, "mapping_id", MAX_ID_BYTES)?;
        validate_text(&self.subid, "mapping subid", MAX_ID_BYTES)?;
        ensure!(
            matches!(
                self.direction.as_str(),
                "maps_to"
                    | "covered_by"
                    | "merged_into"
                    | "renumbered_to"
                    | "split_into"
                    | "reverse:maps_to"
                    | "reverse:covered_by"
            ),
            "unsupported canonical mapping direction"
        );
        Ok(())
    }
}

impl CanonicalMappingRef {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.mapping_id, "mapping_id", MAX_ID_BYTES)?;
        validate_text(&self.subid, "mapping subid", MAX_ID_BYTES)?;
        validate_text(&self.generation, "mapping generation", MAX_ID_BYTES)?;
        validate_text(&self.source, "mapping source", MAX_ID_BYTES)?;
        validate_text(&self.source_ref, "mapping source_ref", MAX_ID_BYTES)?;
        ensure!(
            self.source_sha256.len() == 64
                && self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit()),
            "mapping source_sha256 must be 64 hexadecimal characters"
        );
        ensure!(
            matches!(
                self.direction.as_str(),
                "maps_to"
                    | "covered_by"
                    | "merged_into"
                    | "renumbered_to"
                    | "split_into"
                    | "reverse:maps_to"
                    | "reverse:covered_by"
            ),
            "unsupported canonical mapping direction"
        );
        Ok(())
    }
}

impl ProtectedMethodMarker {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.plugin_id, "marker plugin_id", MAX_ID_BYTES)?;
        validate_text(&self.method_key, "marker method_key", MAX_ID_BYTES)?;
        validate_text(&self.schema_hash, "marker schema_hash", MAX_ID_BYTES)?;
        ensure!(self.revision > 0, "marker revision must be positive");
        Ok(())
    }
}

impl ProtectedMethodMarkerRequest {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.plugin_id, "marker plugin_id", MAX_ID_BYTES)?;
        validate_text(&self.method_key, "marker method_key", MAX_ID_BYTES)?;
        validate_text(&self.schema_hash, "marker schema_hash", MAX_ID_BYTES)
    }
}

pub fn apply_protected_method_marker(
    markers: &mut Vec<ProtectedMethodMarker>,
    input: SetProtectedMethodMarkerInput,
    resolved_plugin_id: &str,
    resolved_method_key: &str,
    resolved_schema_hash: &str,
) -> Result<ProtectedMethodMarker> {
    input.marker.validate()?;
    ensure!(
        input.marker.plugin_id == resolved_plugin_id
            && input.marker.method_key == resolved_method_key
            && input.marker.schema_hash == resolved_schema_hash,
        "protected-method marker does not match sealed-catalog resolution"
    );
    let existing = markers.iter().position(|marker| {
        marker.plugin_id == input.marker.plugin_id && marker.method_key == input.marker.method_key
    });
    let revision = if let Some(index) = existing {
        ensure!(
            markers[index].revision == input.expected_revision,
            "stale expected_revision"
        );
        ensure!(
            input.expected_revision > 0,
            "existing marker update requires a positive expected_revision"
        );
        markers[index]
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("marker revision exhausted"))?
    } else {
        ensure!(
            input.expected_revision == 0,
            "new marker expected_revision must be zero"
        );
        1
    };
    let marker = ProtectedMethodMarker {
        plugin_id: input.marker.plugin_id,
        method_key: input.marker.method_key,
        schema_hash: input.marker.schema_hash,
        requires_policy: input.marker.requires_policy,
        revision,
    };
    marker.validate()?;
    if let Some(index) = existing {
        markers[index] = marker.clone();
    } else {
        markers.push(marker.clone());
    }
    Ok(marker)
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind", deny_unknown_fields)]
pub enum PolicyRule {
    DenyMethod {
        plugin_id: String,
        method_key: String,
        reason: String,
    },
    TagConstraint {
        tag_id: u32,
        disposition: TagDisposition,
    },
    EgressRule {
        destination_class: String,
        allowed_provider_routes: Vec<String>,
        required_policy_revision: u64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TagDisposition {
    LocalOnly,
    RequiresResidualValidation,
}

impl PolicyRule {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::DenyMethod {
                plugin_id,
                method_key,
                reason,
            } => {
                validate_text(plugin_id, "policy plugin_id", MAX_ID_BYTES)?;
                validate_text(method_key, "policy method_key", MAX_ID_BYTES)?;
                validate_text(reason, "policy reason", MAX_TEXT_BYTES)
            }
            Self::TagConstraint { tag_id, .. } => {
                ensure!(*tag_id > 0, "policy tag_id must be positive");
                Ok(())
            }
            Self::EgressRule {
                destination_class,
                allowed_provider_routes,
                required_policy_revision,
            } => {
                validate_text(destination_class, "destination_class", MAX_ID_BYTES)?;
                ensure!(
                    !allowed_provider_routes.is_empty() && allowed_provider_routes.len() <= 64,
                    "provider route count is out of bounds"
                );
                for route in allowed_provider_routes {
                    validate_text(route, "provider route", MAX_ID_BYTES)?;
                }
                ensure!(
                    *required_policy_revision > 0,
                    "required policy revision must be positive"
                );
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PolicyRecord {
    pub owner_id: String,
    pub policy_id: String,
    pub revision: u64,
    pub active: bool,
    pub revocation_reason: Option<String>,
    pub rule: PolicyRule,
    pub canonical_mappings: Vec<CanonicalMappingRef>,
}

impl PolicyRecord {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.owner_id, "owner_id", MAX_ID_BYTES)?;
        validate_text(&self.policy_id, "policy_id", MAX_ID_BYTES)?;
        ensure!(self.revision > 0, "policy revision must be positive");
        self.rule.validate()?;
        if let Some(reason) = &self.revocation_reason {
            validate_text(reason, "revocation_reason", MAX_TEXT_BYTES)?;
        }
        ensure!(
            self.active == self.revocation_reason.is_none(),
            "active/revocation_reason state is inconsistent"
        );
        ensure!(
            self.canonical_mappings.len() <= 64,
            "too many canonical mapping references"
        );
        for mapping in &self.canonical_mappings {
            mapping.validate()?;
        }
        ensure!(
            self.canonical_mappings.first().is_none_or(|first| self
                .canonical_mappings
                .iter()
                .all(|item| item.generation == first.generation)),
            "canonical mapping references mix Qdrant generations"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PolicySnapshot {
    pub owner_id: String,
    pub rule_revision: u64,
    pub marker: Option<ProtectedMethodMarker>,
    pub records: Vec<PolicyRecord>,
}

/// Exact private document committed by `schema_tag_registry` publication.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RegistryState {
    #[schemars(extend("readOnly" = true, "x-opdbus-private" = true))]
    pub revision: u64,
    #[schemars(extend("readOnly" = true, "x-opdbus-private" = true))]
    pub tags: Vec<TagDescriptor>,
    #[schemars(extend("readOnly" = true, "x-opdbus-private" = true))]
    pub policies: Vec<PolicyRecord>,
    #[schemars(extend("readOnly" = true, "x-opdbus-private" = true))]
    pub protected_methods: Vec<ProtectedMethodMarker>,
}

impl RegistryState {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.tags.len() <= 10_000, "registry exceeds tag bound");
        ensure!(
            self.policies.len() <= 10_000,
            "registry exceeds policy bound"
        );
        ensure!(
            self.protected_methods.len() <= 10_000,
            "registry exceeds protected-method bound"
        );
        let mut tag_ids = std::collections::BTreeSet::new();
        let mut tag_names = std::collections::BTreeSet::new();
        for tag in &self.tags {
            tag.validate()?;
            ensure!(tag_ids.insert(tag.tag_id), "duplicate tag_id");
            ensure!(tag_names.insert(tag.name.as_str()), "duplicate tag name");
        }
        let mut policy_ids = std::collections::BTreeSet::new();
        for policy in &self.policies {
            policy.validate()?;
            ensure!(
                policy_ids.insert((policy.owner_id.as_str(), policy.policy_id.as_str())),
                "duplicate owner/policy id"
            );
        }
        let mut marker_keys = std::collections::BTreeSet::new();
        for marker in &self.protected_methods {
            marker.validate()?;
            ensure!(
                marker_keys.insert((marker.plugin_id.as_str(), marker.method_key.as_str())),
                "duplicate protected-method marker"
            );
        }
        let max_revision = self
            .tags
            .iter()
            .map(|tag| tag.revision)
            .chain(self.policies.iter().map(|policy| policy.revision))
            .chain(self.protected_methods.iter().map(|marker| marker.revision))
            .max()
            .unwrap_or(0);
        ensure!(
            self.revision >= max_revision,
            "registry revision is behind a contained record"
        );
        Ok(())
    }

    pub fn validate_for_owner(&self, owner_id: &str) -> Result<()> {
        validate_text(owner_id, "owner_id", MAX_ID_BYTES)?;
        self.validate()?;
        ensure!(
            self.policies
                .iter()
                .all(|policy| policy.owner_id == owner_id),
            "registry state contains a policy for another owner"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListTagsInput {
    #[schemars(range(min = 1, max = 100))]
    pub limit: u16,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TagIdInput {
    pub tag_id: u32,
}

impl TagIdInput {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.tag_id > 0, "tag_id must be positive");
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListTagsOutput {
    pub tags: Vec<TagDescriptor>,
    pub next_cursor: Option<String>,
}

impl ListTagsInput {
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
pub struct GetTagOutput {
    pub tag: Option<TagDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterTagInput {
    pub tag_id: u32,
    pub name: String,
    pub description: String,
    pub default_disposition: Option<TagDisposition>,
    pub associations: Vec<SchemaTagAssociationRequest>,
    pub expected_revision: u64,
    pub openflow_binding: Option<OpenFlowBindingRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyIdInput {
    pub policy_id: String,
}

impl PolicyIdInput {
    pub fn validate(&self) -> Result<()> {
        validate_text(&self.policy_id, "policy_id", MAX_ID_BYTES)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListPoliciesInput {
    #[schemars(range(min = 1, max = 100))]
    pub limit: u16,
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListPoliciesOutput {
    pub policies: Vec<PolicyRecord>,
    pub next_cursor: Option<String>,
}

impl ListPoliciesInput {
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
pub struct GetPolicyOutput {
    pub policy: Option<PolicyRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthorPolicyInput {
    pub policy_id: String,
    pub expected_revision: u64,
    pub rule: PolicyRule,
    pub canonical_mappings: Vec<CanonicalMappingRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePolicyInput {
    pub policy_id: String,
    pub expected_revision: u64,
    pub rule: PolicyRule,
    pub canonical_mappings: Vec<CanonicalMappingRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RevokePolicyInput {
    pub policy_id: String,
    pub expected_revision: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyMutationOutput {
    pub policy: PolicyRecord,
    pub rule_revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterTagOutput {
    pub tag: TagDescriptor,
    pub registry_revision: u64,
}

pub fn apply_tag_registration(
    tags: &mut Vec<TagDescriptor>,
    input: RegisterTagInput,
    resolved_associations: Vec<SchemaTagAssociation>,
    resolved_binding: Option<OpenFlowBindingRef>,
) -> Result<TagDescriptor> {
    ensure!(input.tag_id > 0, "tag_id must be positive");
    ensure!(
        input.associations.len() <= 256 && resolved_associations.len() <= 256,
        "too many schema-tag associations"
    );
    for association in &input.associations {
        association.validate()?;
    }
    for association in &resolved_associations {
        association.validate()?;
        ensure!(
            association.tag_id == input.tag_id,
            "association tag_id mismatch"
        );
    }
    ensure!(
        input.associations.len() == resolved_associations.len(),
        "resolved association count does not match request"
    );
    for request in &input.associations {
        ensure!(
            resolved_associations
                .iter()
                .any(|association| association.plugin_id == request.plugin_id
                    && association.method_key == request.method_key),
            "resolved association does not match sealed schema request"
        );
    }
    ensure!(
        !tags.iter().any(|tag| tag.tag_id != input.tag_id && tag.name == input.name),
        "duplicate tag name"
    );
    let existing_revision = tags
        .iter()
        .find(|tag| tag.tag_id == input.tag_id)
        .map(|tag| tag.revision);
    if let Some(current_revision) = existing_revision {
        ensure!(
            current_revision == input.expected_revision,
            "stale expected_revision"
        );
        ensure!(
            input.expected_revision > 0,
            "existing tag update requires a positive expected_revision"
        );
    } else {
        ensure!(
            input.expected_revision == 0,
            "new tag expected_revision must be zero"
        );
    }
    match (&input.openflow_binding, &resolved_binding) {
        (None, None) => {}
        (Some(request), Some(binding)) => {
            request.validate()?;
            binding.validate()?;
            ensure!(
                request.binding_id == binding.binding_id
                    && request.network_tag_id == binding.network_tag_id
                    && binding.network_tag_id == input.tag_id,
                "resolved OpenFlow binding does not match request/tag"
            );
        }
        _ => anyhow::bail!("OpenFlow binding resolver result does not match request"),
    }
    let proposed = TagDescriptor {
        tag_id: input.tag_id,
        name: input.name,
        description: input.description,
        default_disposition: input.default_disposition,
        revision: existing_revision
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("tag revision exhausted"))?,
        associations: resolved_associations,
        openflow_binding: resolved_binding,
    };
    proposed.validate()?;
    if let Some(current) = tags.iter_mut().find(|tag| tag.tag_id == proposed.tag_id) {
        *current = proposed.clone();
    } else {
        tags.push(proposed.clone());
    }
    Ok(proposed)
}

/// Pure expected-revision update helper for the plugin's owner-scoped store.
pub fn apply_policy_update(
    records: &mut Vec<PolicyRecord>,
    owner_id: &str,
    input: UpdatePolicyInput,
    resolved_mappings: Vec<CanonicalMappingRef>,
) -> Result<PolicyRecord> {
    validate_text(owner_id, "owner_id", MAX_ID_BYTES)?;
    validate_text(&input.policy_id, "policy_id", MAX_ID_BYTES)?;
    validate_mapping_resolution(&input.canonical_mappings, &resolved_mappings)?;
    let current = records
        .iter_mut()
        .find(|record| record.owner_id == owner_id && record.policy_id == input.policy_id)
        .ok_or_else(|| anyhow::anyhow!("policy not found for owner"))?;
    ensure!(current.active, "revoked policy cannot be updated");
    ensure!(
        current.revision == input.expected_revision,
        "stale expected_revision"
    );
    input.rule.validate()?;
    let mut proposed = current.clone();
    proposed.revision = current
        .revision
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("policy revision exhausted"))?;
    proposed.rule = input.rule;
    proposed.canonical_mappings = resolved_mappings;
    proposed.validate()?;
    *current = proposed.clone();
    Ok(proposed)
}

pub fn author_policy(
    records: &mut Vec<PolicyRecord>,
    owner_id: &str,
    input: AuthorPolicyInput,
    resolved_mappings: Vec<CanonicalMappingRef>,
) -> Result<PolicyRecord> {
    validate_text(owner_id, "owner_id", MAX_ID_BYTES)?;
    validate_text(&input.policy_id, "policy_id", MAX_ID_BYTES)?;
    ensure!(
        input.expected_revision == 0,
        "new policy expected_revision must be zero"
    );
    ensure!(
        !records
            .iter()
            .any(|record| record.owner_id == owner_id && record.policy_id == input.policy_id),
        "policy already exists for owner"
    );
    input.rule.validate()?;
    validate_mapping_resolution(&input.canonical_mappings, &resolved_mappings)?;
    let record = PolicyRecord {
        owner_id: owner_id.to_owned(),
        policy_id: input.policy_id,
        revision: 1,
        active: true,
        revocation_reason: None,
        rule: input.rule,
        canonical_mappings: resolved_mappings,
    };
    record.validate()?;
    records.push(record.clone());
    Ok(record)
}

fn validate_mapping_resolution(
    requests: &[CanonicalMappingRequest],
    resolved: &[CanonicalMappingRef],
) -> Result<()> {
    ensure!(
        requests.len() <= 64 && resolved.len() <= 64,
        "too many canonical mapping references"
    );
    for request in requests {
        request.validate()?;
    }
    for mapping in resolved {
        mapping.validate()?;
    }
    ensure!(
        requests.len() == resolved.len(),
        "resolved Qdrant mapping count does not match policy request"
    );
    for request in requests {
        ensure!(
            resolved
                .iter()
                .any(|mapping| mapping.mapping_id == request.mapping_id
                    && mapping.subid == request.subid
                    && mapping.direction == request.direction),
            "resolved Qdrant mapping does not match requested reference"
        );
    }
    ensure!(
        resolved.first().is_none_or(|first| resolved
            .iter()
            .all(|item| item.generation == first.generation)),
        "resolved Qdrant mappings mix generations"
    );
    Ok(())
}

pub fn revoke_policy(
    records: &mut [PolicyRecord],
    owner_id: &str,
    policy_id: &str,
    expected_revision: u64,
    reason: &str,
) -> Result<PolicyRecord> {
    validate_text(owner_id, "owner_id", MAX_ID_BYTES)?;
    validate_text(policy_id, "policy_id", MAX_ID_BYTES)?;
    validate_text(reason, "revocation reason", MAX_TEXT_BYTES)?;
    let current = records
        .iter_mut()
        .find(|record| record.owner_id == owner_id && record.policy_id == policy_id)
        .ok_or_else(|| anyhow::anyhow!("policy not found for owner"))?;
    ensure!(current.active, "policy is already revoked");
    ensure!(
        current.revision == expected_revision,
        "stale expected_revision"
    );
    current.revision = current
        .revision
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("policy revision exhausted"))?;
    current.active = false;
    current.revocation_reason = Some(reason.to_owned());
    Ok(current.clone())
}
