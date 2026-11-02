//! Org chart as the sole grant authority.
//!
//! Humans edit org facts; [`compile`] derives capabilities. The compiler
//! takes exactly `(chart, profile)` and knows nothing of chart size, depth,
//! or classification spread — a deep hierarchy and a lone key differ only
//! in the chart file loaded.
#[cfg(test)]
pub mod acceptance;
pub mod compile;
pub mod model;

pub use compile::{
    compile, compile_pinned, verify_seal, CompileOutput, Grant, Skip,
    ChartError, SealError, MAX_DEPTH, MAX_POSITIONS_PER_LEVEL,
};
pub use model::{Member, OrgChart, Position, Role};
