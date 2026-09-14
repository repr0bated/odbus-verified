//! Dump the full 3tched Router PluginSchema (state fields + typed methods).
//!
//! ```bash
//! cargo run -p op-plugins --example dump_tched_router_schema -- /tmp/tched_router.full.schema.json
//! ```

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let out = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/tched_router.full.schema.json"));

    let schema = op_plugins::state_plugins::tched_router::tched_router_plugin_schema();
    let json = match serde_json::to_string_pretty(&schema) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("serialize schema: {e}");
            return ExitCode::from(1);
        }
    };

    if let Err(e) = fs::write(&out, format!("{json}\n")) {
        eprintln!("write {}: {e}", out.display());
        return ExitCode::from(1);
    }

    eprintln!(
        "wrote {} (fields={} methods={})",
        out.display(),
        schema.fields.len(),
        schema.methods.len()
    );
    ExitCode::SUCCESS
}
