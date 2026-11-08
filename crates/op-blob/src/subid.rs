//! OSCAL subid validation per `subid-taxonomy.md`:
//!
//! ```text
//! <category>.<component-type>.<subject>.<verb>[.<facet>][@vN]
//! ```
//!
//! `subid` is the stable human-readable operational taxonomy key; the paired
//! immutable `uuid` and compliance mappings live in metadata, never in the
//! subid string.

pub const CATEGORIES: &[&str] = &["src", "prj", "sch", "mut", "obs", "evt", "exp"];

/// OSCAL component types. Keep this list identical to
/// `op-plugins` `OSCAL_COMPONENT_TYPES`. Local types go in
/// [`INTERNAL_COMPONENT_TYPES`].
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

/// Local component types. `network` is not an OSCAL noun; issued keys use it.
pub const INTERNAL_COMPONENT_TYPES: &[&str] = &[
    "network",
    "agent",
    "container",
    "data",
    "privacy",
    "security",
    "storage",
    "lxd",
];

/// Readable names accepted in position 2, each standing for an OSCAL type.
pub const COMPONENT_TYPE_ALIASES: &[(&str, &str)] = &[
    ("ui", "software"),
    ("llm", "service"),
    ("agents", "software"),
];

/// Every value accepted in position 2. Retained for callers that only need the flat set.
pub const COMPONENT_TYPES: &[&str] = &[
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
    "agent",
    "container",
    "data",
    "privacy",
    "security",
    "storage",
    "lxd",
    "ui",
    "llm",
    "agents",
];

fn hyphenated(segment: &str) -> bool {
    // Prefer hyphen; allow underscore for Muse-generated method verbs.
    let mut parts = segment.split(|c| c == '-' || c == '_');
    match parts.next() {
        Some(first) if !first.is_empty() && first.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) => {}
        _ => return false,
    }
    parts.all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
}

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
    if !CATEGORIES.contains(&segments[0]) {
        return Err(format!(
            "{subid}: category {:?} not one of {CATEGORIES:?}",
            segments[0]
        ));
    }
    if !COMPONENT_TYPES.contains(&segments[1]) {
        return Err(format!(
            "{subid}: component-type {:?} is not in the taxonomy",
            segments[1]
        ));
    }
    for segment in &segments[2..] {
        if !hyphenated(segment) {
            return Err(format!(
                "{subid}: segment {segment:?} must be lowercase hyphenated [a-z0-9-]"
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_and_rejects_invalid() {
        // Mirrors the acceptance set in op-plugins schemars_adapter tests.
        assert!(validate_subid("mut.service.state-sync.apply-patch@v1").is_ok());
        assert!(validate_subid("exp.software.cognitive-memory.render").is_ok());
        assert!(validate_subid("obs.service.plugin-projection.query").is_ok());
        assert!(validate_subid("sch.network.plugin.wireguard.schema@v1").is_ok());
        assert!(validate_subid("src.network.xray.serve@v1").is_ok());
        assert!(validate_subid("sch.process-procedure.router.user-container.schema@v1").is_ok());
        assert!(validate_subid("mut.this-system.host.describe@v1").is_ok());
        assert!(validate_subid("src.network.xray-serve@v1").is_err());
        assert!(validate_subid("mut.process.foo.bar").is_err());
        assert!(validate_subid("bad-category.software.foo.bar").is_err());
        assert!(validate_subid("mut.bad-type.foo.bar").is_err());
        assert!(validate_subid("mut.software.foo").is_err());
        assert!(validate_subid("mut.software.foo.bar@x2").is_err());
        assert!(validate_subid("mut.software.foo.bar@v0").is_err());
        assert!(validate_subid("mut.software.Foo.bar").is_err());
    }
}
