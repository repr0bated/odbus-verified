//! Offline verification/import of a typed, namespace-owned system graph.
//! Never point this example at a live service's database. Live imports use
//! cozo.replace_system_graph through authenticated D-Bus/3tchedFS.
//!
//! Convert the source export with system_graph_inventory --source-graph first.
//! Usage: load_crate_graph SNAPSHOT.json [OFFLINE_ROCKSDB_DIR EXPECTED_REVISION]
//! Persistent replacement requires an explicit revision and cannot silently
//! retain stale edges or clear another graph owner's rows.

use std::{io::Read, path::PathBuf};

use op_cozo_store::{CozoGraphShuttle, GraphSnapshot, MAX_SYSTEM_GRAPH_BYTES};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !matches!(args.len(), 1 | 3) {
        return Err(
            "usage: load_crate_graph SNAPSHOT.json [OFFLINE_ROCKSDB_DIR EXPECTED_REVISION]".into(),
        );
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&args[0])?
        .take(MAX_SYSTEM_GRAPH_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_SYSTEM_GRAPH_BYTES {
        return Err("system graph snapshot exceeds byte limit".into());
    }
    let snapshot: GraphSnapshot = serde_json::from_slice(&bytes)?;
    let (store, expected) = if args.len() == 3 {
        (
            CozoGraphShuttle::new_persistent(PathBuf::from(&args[1]))?,
            args[2].parse()?,
        )
    } else {
        (CozoGraphShuttle::new_in_memory()?, 0)
    };
    let receipt = store.replace_system_graph(&snapshot, expected)?;
    println!("{}", serde_json::to_string_pretty(&receipt)?);
    Ok(())
}
