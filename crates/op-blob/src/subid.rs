//! OSCAL subid validation per `subid-taxonomy.md`:
//!
//! ```text
//! <category>.<component-type>.<subject>.<verb>[.<facet>][@vN]
//! ```
//!
//! `subid` is the stable human-readable operational taxonomy key; the paired
//! immutable `uuid` and compliance mappings live in metadata, never in the
//! subid string.
//!
//! The taxonomy is owned by the OSCAL subid registry plugin and defined once
//! in `op-oscal-subid`; this module re-exports it under its historical names.

pub use op_oscal_subid::{
    COMPONENT_TYPE_ALIASES, COMPONENT_TYPES, INTERNAL_COMPONENT_TYPES, OSCAL_COMPONENT_TYPES,
    SUBID_CATEGORIES as CATEGORIES, validate_subid,
};

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
