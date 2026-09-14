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

/// OSCAL component-type vocabulary accepted in position 2.
pub const COMPONENT_TYPES: &[&str] = &[
    "service",
    "software",
    "network",
    "hardware",
    "data",
    "policy",
    "process",
    "plan",
    "guidance",
    "standard",
    "validation",
    "system",
    "interconnection",
];

pub fn validate_subid(subid: &str) -> Result<(), String> {
    let (base, version) = match subid.split_once('@') {
        Some((b, v)) => (b, Some(v)),
        None => (subid, None),
    };

    if let Some(v) = version {
        let ok = v.len() >= 2 && v.starts_with('v') && v[1..].chars().all(|c| c.is_ascii_digit());
        if !ok {
            return Err(format!("{subid}: version must be @vN, got @{v}"));
        }
    }

    let segments: Vec<&str> = base.split('.').collect();
    if segments.len() < 4 || segments.len() > 5 {
        return Err(format!(
            "{subid}: expected category.component-type.subject.verb[.facet], got {} segments",
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
            "{subid}: component-type {:?} not in OSCAL vocabulary",
            segments[1]
        ));
    }
    for seg in &segments[2..] {
        let ok = !seg.is_empty()
            && seg
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            && !seg.starts_with('-')
            && !seg.ends_with('-');
        if !ok {
            return Err(format!(
                "{subid}: segment {seg:?} must be lowercase hyphenated [a-z0-9-]"
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
        assert!(validate_subid("sch.network.plugin.wireguard.schema@v1").is_ok()); // 5-segment facet form
        assert!(validate_subid("bad-category.software.foo.bar").is_err());
        assert!(validate_subid("mut.bad-type.foo.bar").is_err());
        assert!(validate_subid("mut.software.foo").is_err()); // too few segments
        assert!(validate_subid("mut.software.foo.bar@x2").is_err()); // bad version
        assert!(validate_subid("mut.software.Foo.bar").is_err()); // uppercase subject
    }
}
