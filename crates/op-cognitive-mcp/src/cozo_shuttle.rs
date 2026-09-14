//! Re-exports the shared CozoDB shuttle from the `op-cozo-store` crate.
//!
//! Schema, queries, and helpers all live in `op-cozo-store::lib`.

pub use op_cozo_store::{named_rows_to_json, CozoGraphShuttle, PolicyVerdict};
