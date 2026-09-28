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
mod rust_emit;
mod subid;

pub use audit::{CoverageInputs, audit_source, audit_source_with_coverage};
pub use binary::{
    BinaryIntrospectOpts, BinarySurface, introspect_binary, surface_as_instance_json,
    surface_to_json,
};
pub use emit::{
    CompletePluginDocument, complete_to_json, complete_to_markdown, emit_complete_plugin,
};
#[cfg(feature = "inspector-gadget")]
pub use gadget::introspect_with_live_gadget;
pub use gadget::{
    CoverageDiff, declared_field_paths, declared_field_paths_multi, diff_coverage,
    introspect_json_paths, introspect_json_text, paths_from_sealed_schema,
};
pub use gaps::{IntrospectGaps, gaps_from_surface_json, gaps_from_surface_json_for_plugin};
pub use introspect::{
    IntrospectOpts, resolve_introspect_target, resolve_introspect_target_for_plugin,
    resolve_introspect_target_with,
};
pub use repomix::{RepomixSurface, introspect_repomix, read_and_introspect as read_repomix};
pub use report::{Finding, Report, Severity};
pub use rust_emit::emit_inspector_rust;
