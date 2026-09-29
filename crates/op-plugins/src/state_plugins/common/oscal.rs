//! OSCAL subid taxonomy helpers.
//!
//! Every artifact in the system carries a dual identifier: a stable `uuid` and a
//! human-readable `subid` operational taxonomy key. The format is mandated by
//! AGENTS.md §4a:
//!
//! `<category>.<component-type>.<subject>.<verb>[.<facet>][@vN]`

use regex::Regex;

/// The seven subid categories. First segment of every subid.
pub const SUBID_CATEGORIES: &[&str] = &["src", "prj", "sch", "mut", "obs", "evt", "exp"];

/// Component types drawn from the OSCAL standard vocabulary.
///
/// This list mirrors OSCAL. Do not extend it to fit a local need — adding a
/// type here is a claim about the standard. Put local types in
/// [`INTERNAL_COMPONENT_TYPES`] instead.
pub const OSCAL_COMPONENT_TYPES: &[&str] = &[
    "this-system",
    "system",
    "interconnection",
    "software",
    "hardware",
    "service",
    "policy",
    "physical",
    "process-procedure",
    "plan",
    "guidance",
    "standard",
    "validation",
    "network",
];

/// Categories local to this system, deliberately kept separate from the OSCAL
/// list so the standard vocabulary stays recognisable as the standard. Extend
/// this one freely — that is what it is for.
///
/// The internal/OSCAL split is an authoring concern, not a presentation one:
/// at the render boundary these alias to plain `category`, so a UI never has to
/// know which list a value came from.
///
/// Each entry below is already in use by a plugin that declares or derives it.
pub const INTERNAL_CATEGORIES: &[&str] = &[
    "agent",     // persona
    "container", // oci — `mut.container.oci.run@v1` and friends
    "data",      // cozo
    "privacy",   // privacy — `mut.privacy.data.mask@v1` and friends
    "security",  // fail2ban
    "storage",   // btrfs
];

/// Category aliases: a readable name a surface may declare, paired with the
/// OSCAL category it actually means.
///
/// `ui` is the worked example. OSCAL has no vocabulary for "this is a user
/// interface" — the nearest honest answer is `software` — but `software` says
/// nothing useful when a dashboard is grouping surfaces for a human. So a
/// plugin declares `ui`, the subid stays valid because the alias is accepted,
/// and anything emitting OSCAL resolves it back to `software` via
/// [`canonical_category`]. The readable name and the conformant one stay in
/// sync because neither is written down twice.
pub const CATEGORY_ALIASES: &[(&str, &str)] = &[
    ("ui", "software"),
    // Declared by `large_language_model`. A model integration is a service in
    // OSCAL terms; `llm` is what it is called everywhere else in this system.
    ("llm", "service"),
    // Declared by the agent plugins. Distinct from the internal `agent`
    // category, which `persona` uses for its own subids.
    ("agents", "software"),
];

/// Resolve a possibly-aliased category to the OSCAL category it means.
///
/// Non-aliased values pass through untouched, so this is safe to call on any
/// category — including internal ones, which have no OSCAL equivalent and are
/// deliberately returned as-is rather than forced into the standard vocabulary.
pub fn canonical_category(category: &str) -> &str {
    CATEGORY_ALIASES
        .iter()
        .find(|(alias, _)| *alias == category)
        .map(|(_, oscal)| *oscal)
        .unwrap_or(category)
}

/// Every accepted value for a subid's second segment: OSCAL, internal, and
/// aliases. Presented to consumers as simply the `category`.
pub fn component_types() -> impl Iterator<Item = &'static str> {
    OSCAL_COMPONENT_TYPES
        .iter()
        .chain(INTERNAL_CATEGORIES.iter())
        .copied()
        .chain(CATEGORY_ALIASES.iter().map(|(alias, _)| *alias))
}

/// Whether `candidate` is an accepted category from either vocabulary.
pub fn is_component_type(candidate: &str) -> bool {
    component_types().any(|t| t == candidate)
}

/// Regex validating an OSCAL subid.
///
/// Shared between the schemars adapter, the canonical subid registry schema,
/// and any runtime validation that wants a single source of truth for the
/// taxonomy format. Built from [`SUBID_CATEGORIES`], [`OSCAL_COMPONENT_TYPES`]
/// and [`INTERNAL_CATEGORIES`] so a vocabulary change cannot leave the regex
/// behind — adding a category is a one-line edit to one list.
pub fn subid_regex() -> &'static str {
    static PATTERN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        let segment = r"[a-z0-9]+(?:-[a-z0-9]+)*";
        let types: Vec<&str> = component_types().collect();
        format!(
            r"^({})\.({})\.{segment}\.{segment}(?:\.{segment}){{0,2}}(?:@v[1-9][0-9]*)?$",
            SUBID_CATEGORIES.join("|"),
            types.join("|"),
        )
    })
}

/// Validate a subid string against the canonical regex.
pub fn validate_subid(subid: &str) -> Result<(), String> {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(subid_regex()).expect("subid_regex is valid"));
    if re.is_match(subid) {
        Ok(())
    } else {
        Err(format!("subid does not match required pattern: {subid}"))
    }
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
    use super::{canonical_category, validate_subid, CATEGORY_ALIASES, OSCAL_COMPONENT_TYPES};

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
