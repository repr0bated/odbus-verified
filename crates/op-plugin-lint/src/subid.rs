//! Standalone subid taxonomy check (mirrors `op-plugins` oscal helpers).
//! Kept local so this crate builds without the zbus workspace patch.

use regex::Regex;
use std::sync::OnceLock;

const CATEGORIES: &[&str] = &["src", "prj", "sch", "mut", "obs", "evt", "exp"];

const COMPONENT_TYPES: &[&str] = &[
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
    // internal
    "agent",
    "container",
    "data",
    "privacy",
    "security",
    "storage",
    // aliases
    "ui",
    "llm",
    "agents",
];

pub fn validate_subid(subid: &str) -> Result<(), String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        let segment = r"[a-z0-9]+(?:-[a-z0-9]+)*";
        let pat = format!(
            r"^({})\.({})\.{segment}\.{segment}(?:\.{segment}){{0,2}}(?:@v[1-9][0-9]*)?$",
            CATEGORIES.join("|"),
            COMPONENT_TYPES.join("|"),
        );
        Regex::new(&pat).expect("subid regex")
    });
    if re.is_match(subid) {
        Ok(())
    } else {
        Err(format!("subid does not match required pattern: {subid}"))
    }
}

/// Known `x-*` extension keys the render contract recognizes.
pub const KNOWN_X_KEYS: &[&str] = &["x-oscal-subid", "x-oscal-category", "x-immutable-paths"];
