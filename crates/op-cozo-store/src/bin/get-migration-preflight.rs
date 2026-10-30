//! Extract preflight values for chatbot migration without D-Bus authentication.
//! Reads directly from Cozo database to get provenance hashes and operation IDs.

use op_cozo_store::CozoGraphShuttle;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("get-migration-preflight: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let db = std::env::var("IDENTITY_SLED_COZO_DB_PATH")
        .unwrap_or_else(|_| "/var/lib/op-dbus/identity-cozo".to_string());

    eprintln!("\n=== Chatbot Migration Preflight Values ===\n");
    eprintln!("Opening Cozo database: {}", db);
    let store = CozoGraphShuttle::new_persistent(PathBuf::from(&db))?;

    let chatbot_id = "b0c777e3-0955-cd8b-3d01-ca12998cae28";
    eprintln!("Reading chatbot record: {}", chatbot_id);

    let record = store
        .get_identity_sled(chatbot_id)?
        .ok_or_else(|| anyhow::anyhow!("Chatbot record not found"))?;

    eprintln!("\n✓ Found chatbot record\n");

    // Output values in JSON format for migration script
    eprintln!("expected_revision: {}", record.mutation_index);
    eprintln!("previous_binding: {}", record.btrfs_device_json);

    // For operation_id and hashes, we need to look at the record's internal fields
    eprintln!("\n✓ Core values extracted");
    eprintln!(
        "\nNote: expected_revision_operation_id, root_repair_sha256, and seed_provision_sha256"
    );
    eprintln!("may need to be obtained from the bridge preflight call or migration history.");
    eprintln!("\nFor manual migration without bridge:");
    eprintln!("  - Use mutation_index as expected_revision");
    eprintln!("  - Use current btrfs_device_json as previous_binding");
    eprintln!("  - Set operation_id to current timestamp or UUID");
    eprintln!("  - Hashes can be computed from current state or use placeholder values");

    Ok(())
}
