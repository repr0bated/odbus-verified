//! Subid taxonomy check. The taxonomy is owned by the OSCAL subid registry
//! plugin and defined once in `op-oscal-subid` (no zbus, so this crate still
//! builds without the workspace patch).

/// Validate a subid against the canonical taxonomy.
///
/// Delegates to [`op_oscal_subid::validate_subid`]; only the error text is
/// kept in this crate's historical form.
pub fn validate_subid(subid: &str) -> Result<(), String> {
    op_oscal_subid::validate_subid(subid)
        .map_err(|_| format!("subid does not match required pattern: {subid}"))
}

/// Known `x-*` extension keys the render contract recognizes.
pub const KNOWN_X_KEYS: &[&str] = &["x-oscal-subid", "x-oscal-category", "x-immutable-paths"];
