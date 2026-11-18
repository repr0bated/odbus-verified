//! OSCAL subid taxonomy helpers.
//!
//! Every artifact in the system carries a dual identifier: a stable `uuid` and a
//! human-readable `subid` operational taxonomy key. The format is mandated by
//! AGENTS.md §4a:
//!
//! `<category>.<component-type>.<subject>.<verb>[.<facet>][@vN]`
//!
//! The taxonomy itself (categories, component types, aliases, the grammar) is
//! owned by the OSCAL subid registry plugin and defined once in the
//! `op-oscal-subid` crate. Everything here is a re-export under the names this
//! crate has always used.

pub use op_oscal_subid::{
    COMPONENT_TYPE_ALIASES as CATEGORY_ALIASES, INTERNAL_COMPONENT_TYPES as INTERNAL_CATEGORIES,
    OSCAL_COMPONENT_TYPES, SUBID_CATEGORIES, canonical_category, component_types,
    is_component_type, subid_regex,
};

/// Validate a subid against the canonical taxonomy.
///
/// Delegates to [`op_oscal_subid::validate_subid`]; only the error text is
/// kept in this module's historical form.
pub fn validate_subid(subid: &str) -> Result<(), String> {
    op_oscal_subid::validate_subid(subid)
        .map_err(|_| format!("subid does not match required pattern: {subid}"))
}

/// Return the metadata fields that are required for a given subid category.
///
/// Per AGENTS.md §4a:
/// - `mut` requires `actor_id` and `capability_id`
/// - `evt` requires `event_id` or `event_hash`
/// - `src` requires `source_system` and `source_locator`
/// - all other categories have no additional required metadata fields
pub fn category_required_fields(category: &str) -> &'static [&'static str] {
    match category {
        "mut" => &["actor_id", "capability_id"],
        "evt" => &["event_id", "event_hash"],
        "src" => &["source_system", "source_locator"],
        _ => &[],
    }
}

/// Ensure a `PluginSchema` carries the metadata fields required by its subid categories.
///
/// This is a runtime/schema-build helper: if the schema declares any `mut.*`, `evt.*`, or
/// `src.*` subids, the corresponding accountability fields are added to `fields` when not
/// already present. Fields are added as optional strings so they do not change required-field
/// validation for existing plugin state.
pub fn ensure_category_metadata_fields(schema: &mut op_state_store::PluginSchema) {
    use std::collections::HashSet;

    let categories: HashSet<&str> = schema
        .subids
        .values()
        .filter_map(|subid| subid.split('.').next())
        .collect();

    let string_field = |description: &str| op_state_store::FieldSchema {
        field_type: op_state_store::FieldType::String,
        required: false,
        description: description.to_string(),
        default: None,
        example: None,
        constraints: vec![],
        read_only: false,
        read_only_when: None,
    };

    if categories.contains("mut") {
        schema
            .fields
            .entry("actor_id".to_string())
            .or_insert_with(|| string_field("Actor identifier for mutation accountability"));
        schema
            .fields
            .entry("capability_id".to_string())
            .or_insert_with(|| string_field("Capability identifier authorizing the mutation"));
    }

    if categories.contains("evt") {
        if !schema.fields.contains_key("event_id") && !schema.fields.contains_key("event_hash") {
            schema
                .fields
                .entry("event_id".to_string())
                .or_insert_with(|| string_field("Event identifier for audit-chain provenance"));
        }
    }

    if categories.contains("src") {
        schema
            .fields
            .entry("source_system".to_string())
            .or_insert_with(|| string_field("Source system for authoritative data ingress"));
        schema
            .fields
            .entry("source_locator".to_string())
            .or_insert_with(|| string_field("Source locator for authoritative data ingress"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_subids_pass() {
        assert!(validate_subid("src.network.ovsdb.monitor@v1").is_ok());
        assert!(validate_subid("prj.service.projected-object.publish@v1").is_ok());
        assert!(validate_subid("sch.standard.plugin-schema.resolve@v1").is_ok());
        assert!(validate_subid("mut.service.state-sync.apply-patch@v1").is_ok());
        assert!(validate_subid("obs.service.plugin-projection.query").is_ok());
        assert!(validate_subid("evt.service.audit-chain.emit@v2").is_ok());
        assert!(validate_subid("exp.service.plugin-projection.render").is_ok());
    }

    #[test]
    fn invalid_subids_fail() {
        assert!(validate_subid("bad.software.foo.bar").is_err());
        assert!(validate_subid("mut.software.foo").is_err());
        assert!(validate_subid("mut.invalid.foo.bar").is_err());
        assert!(validate_subid("mut.software.foo.bar@").is_err());
        assert!(validate_subid("mut.software.foo.bar@v0").is_err());
    }

    #[test]
    fn category_required_fields_match_agents_md() {
        assert_eq!(
            category_required_fields("mut"),
            &["actor_id", "capability_id"]
        );
        assert_eq!(category_required_fields("evt"), &["event_id", "event_hash"]);
        assert_eq!(
            category_required_fields("src"),
            &["source_system", "source_locator"]
        );
        assert!(category_required_fields("prj").is_empty());
        assert!(category_required_fields("sch").is_empty());
        assert!(category_required_fields("obs").is_empty());
        assert!(category_required_fields("exp").is_empty());
    }
}

#[cfg(test)]
mod category_alias_tests {
    use super::{CATEGORY_ALIASES, OSCAL_COMPONENT_TYPES, canonical_category, validate_subid};

    #[test]
    fn an_aliased_category_is_accepted_in_a_subid() {
        // A UI surface can say what it is without inventing an invalid subid.
        assert!(validate_subid("exp.ui.plugin.web-ui.render@v1").is_ok());
    }

    #[test]
    fn aliases_resolve_to_their_oscal_category() {
        assert_eq!(canonical_category("ui"), "software");
    }

    #[test]
    fn non_aliased_categories_pass_through_untouched() {
        assert_eq!(canonical_category("network"), "network");
        assert_eq!(canonical_category("storage"), "storage");
    }

    #[test]
    fn every_alias_resolves_to_a_real_oscal_category() {
        // Guards the alias table: a typo on the right-hand side would silently
        // produce non-conformant OSCAL output.
        for (alias, oscal) in CATEGORY_ALIASES {
            assert!(
                OSCAL_COMPONENT_TYPES.contains(oscal),
                "alias {alias} resolves to {oscal}, which is not an OSCAL category",
            );
        }
    }
}
