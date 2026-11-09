//! Tool_Registry with dual-view derivation (spec task 6).
//! Property-test tasks 6.3–6.7 are deferred — the owner directed no
//! testing or verification until after deployment.

pub mod tool_registry;

pub use tool_registry::{
    DeriveInput, DeriveOutput, DiscoveredMethod, ViewDiscovery, derive, rederive_view, tool_names,
};
