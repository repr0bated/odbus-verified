//! List identity genesis records to find seed FSID.

use op_cozo_store::CozoGraphShuttle;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("list-genesis: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let db = std::env::var("IDENTITY_SLED_COZO_DB_PATH")
        .unwrap_or_else(|_| "/var/lib/op-dbus/identity-cozo".to_string());

    eprintln!("Opening Cozo database: {}", db);
    let store = CozoGraphShuttle::new_persistent(PathBuf::from(&db))?;

    eprintln!("\n=== Identity Genesis Records ===\n");
    let genesis_records = store.list_identity_genesis()?;

    for record in &genesis_records {
        eprintln!("Session ID: {}", record.session_id);
        eprintln!("Chain head at arrival: {}", record.chain_head_at_arrival);
        eprintln!(
            "Catalog hash at arrival: {}",
            record.catalog_hash_at_arrival
        );
        eprintln!("Schema content hash: {}", record.schema_content_hash);
        eprintln!();
    }

    eprintln!("\n=== Identity Sled Sessions ===\n");
    let sled_records = store.list_identity_sessions()?;

    for record in &sled_records {
        eprintln!("Session ID: {}", record.session_id);
        eprintln!("Btrfs device JSON:\n{}\n", record.btrfs_device_json);
    }

    Ok(())
}
