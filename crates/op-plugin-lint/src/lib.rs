//! Static audit of plugin `.rs` sources against `PLUGIN-RENDER-CONTRACT.md`.
//!
//! External discovery via `--introspect`: prefer Repomix XML (universal source
//! pack); CLI `--help` walk is secondary.

mod audit;
mod binary;
mod emit;
mod gadget;
mod gaps;
mod introspect;
mod repomix;
mod report;
mod subid;

pub use audit::{audit_source, audit_source_with_coverage, CoverageInputs};
pub use binary::{
    introspect_binary, surface_as_instance_json, surface_to_json, BinaryIntrospectOpts,
    BinarySurface,
};
pub use emit::{
    complete_to_json, complete_to_markdown, emit_complete_plugin, CompletePluginDocument,
};
#[cfg(feature = "inspector-gadget")]
pub use gadget::introspect_with_live_gadget;
pub use gadget::{
    declared_field_paths, declared_field_paths_multi, diff_coverage, introspect_json_paths,
    introspect_json_text, paths_from_sealed_schema, CoverageDiff,
};
pub use gaps::{gaps_from_surface_json, gaps_from_surface_json_for_plugin, IntrospectGaps};
pub use introspect::{
    resolve_introspect_target, resolve_introspect_target_for_plugin,
    resolve_introspect_target_with, IntrospectOpts,
};
pub use repomix::{introspect_repomix, read_and_introspect as read_repomix, RepomixSurface};
pub use report::{Finding, Report, Severity};
