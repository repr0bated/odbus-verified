//! OSCAL subid registry taxonomy — the single source of truth.
//!
//! This crate is owned by the OSCAL subid registry plugin
//! (`op_plugins::state_plugins::oscal_subid_registry`), which re-exports
//! [`OscalSubidCategory`] as its own. It lives in its own crate only because
//! `op-plugins` depends on `op-blob` and `op-plugin-lint`, so those crates
//! cannot depend on the plugin directly without a dependency cycle.
//!
//! Every other copy of the taxonomy (`op-plugins` `common::oscal`,
//! `op_blob::subid`, `op-plugin-lint`'s subid check) re-exports from here.
//! Change the vocabulary here and nowhere else.
//!
//! Every artifact carries a dual identifier: a stable `uuid` and a
//! human-readable `subid` operational taxonomy key (AGENTS.md §4a):
//!
//! ```text
//! <category>.<component-type>.<subject>.<verb>[.<facet>][@vN]
//! ```
//!
//! The paired immutable `uuid` and compliance mappings live in metadata, never
//! in the subid string.

#![forbid(unsafe_code)]

use std::fmt;
use std::str::FromStr;

// ── Categories (first segment) ───────────────────────────────────────────────

/// Operational category. Determines which additional fields are required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "schema",
    derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)
)]
#[cfg_attr(feature = "schema", serde(rename_all = "kebab-case"))]
#[cfg_attr(
    feature = "schema",
    schemars(extend("x-oscal-subid" = "sch.standard.oscal-subid-registry.category@v1"))
)]
pub enum OscalSubidCategory {
    Src,
    Prj,
    Sch,
    Mut,
    Obs,
    Evt,
    Exp,
}

impl OscalSubidCategory {
    /// Every category, in canonical order.
    pub const ALL: [OscalSubidCategory; 7] = [
        Self::Src,
        Self::Prj,
        Self::Sch,
        Self::Mut,
        Self::Obs,
        Self::Evt,
        Self::Exp,
    ];

    /// The subid segment for this category (also its serde value).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Src => "src",
            Self::Prj => "prj",
            Self::Sch => "sch",
            Self::Mut => "mut",
            Self::Obs => "obs",
            Self::Evt => "evt",
            Self::Exp => "exp",
        }
    }

    /// Parse a subid category segment.
    pub fn parse(segment: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == segment)
    }
}

impl fmt::Display for OscalSubidCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when a string is not one of the seven subid categories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownSubidCategory(pub String);

impl fmt::Display for UnknownSubidCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "category {:?} not one of {SUBID_CATEGORIES:?}",
            self.0
        )
    }
}

impl std::error::Error for UnknownSubidCategory {}

impl FromStr for OscalSubidCategory {
    type Err = UnknownSubidCategory;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| UnknownSubidCategory(s.to_string()))
    }
}

const SUBID_CATEGORY_ARRAY: [&str; OscalSubidCategory::ALL.len()] = {
    let mut out = [""; OscalSubidCategory::ALL.len()];
    let mut i = 0;
    while i < out.len() {
        out[i] = OscalSubidCategory::ALL[i].as_str();
        i += 1;
    }
    out
};

/// The seven subid categories as strings. First segment of every subid.
/// Derived from [`OscalSubidCategory::ALL`].
pub const SUBID_CATEGORIES: &[&str] = &SUBID_CATEGORY_ARRAY;

// ── Component types (second segment) ─────────────────────────────────────────

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
pub const INTERNAL_COMPONENT_TYPES: &[&str] = &[
    "network",   // local; OSCAL has no network component type. `src.network.xray.serve@v1`
    "agent",     // persona
    "container", // oci — `mut.container.oci.run@v1` and friends
    "data",      // cozo
    "privacy",   // privacy — `mut.privacy.data.mask@v1` and friends
    "security",  // fail2ban
    "storage",   // btrfs
    "lxd",       // Muse LXD plugin — `obs.lxd.instances.get_instance@v1` and friends
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
pub const COMPONENT_TYPE_ALIASES: &[(&str, &str)] = &[
    ("ui", "software"),
    // Declared by `large_language_model`. A model integration is a service in
    // OSCAL terms; `llm` is what it is called everywhere else in this system.
    ("llm", "service"),
    // Declared by the agent plugins. Distinct from the internal `agent`
    // category, which `persona` uses for its own subids.
    ("agents", "software"),
];

const COMPONENT_TYPE_COUNT: usize =
    OSCAL_COMPONENT_TYPES.len() + INTERNAL_COMPONENT_TYPES.len() + COMPONENT_TYPE_ALIASES.len();

const COMPONENT_TYPE_ARRAY: [&str; COMPONENT_TYPE_COUNT] = {
    let mut out = [""; COMPONENT_TYPE_COUNT];
    let mut i = 0;
    let mut j = 0;
    while j < OSCAL_COMPONENT_TYPES.len() {
        out[i] = OSCAL_COMPONENT_TYPES[j];
        i += 1;
        j += 1;
    }
    j = 0;
    while j < INTERNAL_COMPONENT_TYPES.len() {
        out[i] = INTERNAL_COMPONENT_TYPES[j];
        i += 1;
        j += 1;
    }
    j = 0;
    while j < COMPONENT_TYPE_ALIASES.len() {
        out[i] = COMPONENT_TYPE_ALIASES[j].0;
        i += 1;
        j += 1;
    }
    out
};

/// Every value accepted in position 2 (OSCAL, then internal, then aliases),
/// as one flat list. Derived from the three lists above.
pub const COMPONENT_TYPES: &[&str] = &COMPONENT_TYPE_ARRAY;

/// Resolve a possibly-aliased category to the OSCAL category it means.
///
/// Non-aliased values pass through untouched, so this is safe to call on any
/// category — including internal ones, which have no OSCAL equivalent and are
/// deliberately returned as-is rather than forced into the standard vocabulary.
pub fn canonical_category(category: &str) -> &str {
    COMPONENT_TYPE_ALIASES
        .iter()
        .find(|(alias, _)| *alias == category)
        .map(|(_, oscal)| *oscal)
        .unwrap_or(category)
}

/// Every accepted value for a subid's second segment: OSCAL, internal, and
/// aliases. Presented to consumers as simply the `category`.
pub fn component_types() -> impl Iterator<Item = &'static str> {
    COMPONENT_TYPES.iter().copied()
}

/// Whether `candidate` is an accepted category from either vocabulary.
pub fn is_component_type(candidate: &str) -> bool {
    COMPONENT_TYPES.contains(&candidate)
}

// ── Validation ───────────────────────────────────────────────────────────────

/// Regex (as a pattern string) validating an OSCAL subid.
///
/// Built from [`SUBID_CATEGORIES`] and [`COMPONENT_TYPES`] so a vocabulary
/// change cannot leave it behind. Accepts exactly the same strings as
/// [`validate_subid`] (checked by this crate's tests).
pub fn subid_regex() -> &'static str {
    static PATTERN: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PATTERN.get_or_init(|| {
        // Hyphen is preferred; underscore is accepted for Muse-generated method verbs
        // (e.g. fuse `forget_multi`, lxd `create_instance`).
        let segment = r"[a-z0-9]+(?:[-_][a-z0-9]+)*";
        format!(
            r"^({})\.({})\.{segment}\.{segment}(?:\.{segment}){{0,2}}(?:@v[1-9][0-9]*)?$",
            SUBID_CATEGORIES.join("|"),
            COMPONENT_TYPES.join("|"),
        )
    })
}

/// One subject/verb/facet segment: `[a-z0-9]+([-_][a-z0-9]+)*`.
fn valid_segment(segment: &str) -> bool {
    // Prefer hyphen; allow underscore for Muse-generated method verbs.
    segment.split(['-', '_']).all(|part| {
        !part.is_empty()
            && part
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    })
}

/// Validate a subid against the taxonomy. The one validator; every crate's
/// `validate_subid` delegates here. Errors say which part is wrong.
pub fn validate_subid(subid: &str) -> Result<(), String> {
    let expected = "<category>.<component-type>.<subject>.<verb>[.<facet>][@vN]";
    let (base, version) = match subid.split_once('@') {
        Some((base, version)) => (base, Some(version)),
        None => (subid, None),
    };
    if let Some(version) = version {
        let ok = version.len() >= 2
            && version.starts_with('v')
            && version.as_bytes()[1].is_ascii_digit()
            && version.as_bytes()[1] != b'0'
            && version[2..].chars().all(|c| c.is_ascii_digit());
        if !ok {
            return Err(format!("{subid}: version must be @vN, got @{version}"));
        }
    }
    let segments: Vec<&str> = base.split('.').collect();
    if !(4..=6).contains(&segments.len()) {
        return Err(format!(
            "{subid}: expected {expected}, got {} segments",
            segments.len()
        ));
    }
    if OscalSubidCategory::parse(segments[0]).is_none() {
        return Err(format!(
            "{subid}: category {:?} not one of {SUBID_CATEGORIES:?}",
            segments[0]
        ));
    }
    if !is_component_type(segments[1]) {
        return Err(format!(
            "{subid}: component-type {:?} is not in the taxonomy",
            segments[1]
        ));
    }
    for segment in &segments[2..] {
        if !valid_segment(segment) {
            return Err(format!(
                "{subid}: segment {segment:?} must be lowercase hyphenated [a-z0-9-]"
            ));
        }
    }
    Ok(())
}

/// `true` when [`validate_subid`] accepts `subid`.
pub fn is_valid_subid(subid: &str) -> bool {
    validate_subid(subid).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_are_the_seven_in_order() {
        assert_eq!(
            SUBID_CATEGORIES,
            &["src", "prj", "sch", "mut", "obs", "evt", "exp"]
        );
        for c in OscalSubidCategory::ALL {
            assert_eq!(c.as_str().parse::<OscalSubidCategory>(), Ok(c));
            assert_eq!(c.to_string(), c.as_str());
        }
        assert!("qry".parse::<OscalSubidCategory>().is_err());
    }

    #[test]
    fn flat_component_types_are_oscal_then_internal_then_aliases() {
        let expected: Vec<&str> = OSCAL_COMPONENT_TYPES
            .iter()
            .chain(INTERNAL_COMPONENT_TYPES)
            .copied()
            .chain(COMPONENT_TYPE_ALIASES.iter().map(|(a, _)| *a))
            .collect();
        assert_eq!(COMPONENT_TYPES, expected.as_slice());
        assert_eq!(COMPONENT_TYPES.len(), 24);
    }

    #[test]
    fn every_alias_resolves_to_a_real_oscal_category() {
        for (alias, oscal) in COMPONENT_TYPE_ALIASES {
            assert!(OSCAL_COMPONENT_TYPES.contains(oscal), "{alias} -> {oscal}");
            assert_eq!(canonical_category(alias), *oscal);
        }
        assert_eq!(canonical_category("network"), "network");
    }

    #[test]
    fn validator_and_regex_agree() {
        let re = regex::Regex::new(subid_regex()).unwrap();
        let corpus = [
            "mut.service.state-sync.apply-patch@v1",
            "exp.software.cognitive-memory.render",
            "obs.service.plugin-projection.query",
            "sch.network.plugin.wireguard.schema@v1",
            "sch.process-procedure.router.user-container.schema@v1",
            "mut.this-system.host.describe@v1",
            "obs.lxd.instances.get_instance@v1",
            "exp.ui.plugin.web-ui.render@v1",
            "obs.llm.a.b.c.d@v12",
            "obs.agents.a.b",
            "src.network.xray-serve@v1",
            "mut.process.foo.bar",
            "bad-category.software.foo.bar",
            "qry.service.foo.bar",
            "mut.bad-type.foo.bar",
            "mut.software.foo",
            "mut.software.a.b.c.d.e",
            "mut.software.foo.bar@",
            "mut.software.foo.bar@v",
            "mut.software.foo.bar@v0",
            "mut.software.foo.bar@v01",
            "mut.software.foo.bar@x2",
            "mut.software.foo.bar@v1@v2",
            "mut.software.foo.bar@v1x",
            "mut.software.Foo.bar",
            "mut.software.foo-.bar",
            "mut.software.-foo.bar",
            "mut.software.foo--bar.baz",
            "mut.software.foo_-bar.baz",
            "mut.software.foo..bar",
            "mut.software.foo.bar.",
            "mut.software.foo.bar\n",
            "mut.software.föo.bar",
            "MUT.software.foo.bar",
            "",
            "@v1",
        ];
        for s in corpus {
            assert_eq!(re.is_match(s), is_valid_subid(s), "disagree on {s:?}");
        }
    }

    #[test]
    fn error_messages_name_the_bad_part() {
        assert!(validate_subid("qry.service.a.b").unwrap_err().contains("category \"qry\""));
        assert!(validate_subid("obs.nope.a.b").unwrap_err().contains("component-type"));
        assert!(validate_subid("obs.service.a.b@v0").unwrap_err().contains("@vN"));
    }
}
