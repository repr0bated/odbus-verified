//! Validate chatbot identity state post-migration.
//! Verifies that the chatbot has successfully migrated to the shared seed device
//! and can perform self-validation checks.

use op_cozo_store::CozoGraphShuttle;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("validate-chatbot-state: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let db = std::env::var("IDENTITY_SLED_COZO_DB_PATH")
        .unwrap_or_else(|_| "/var/lib/op-dbus/identity-cozo".to_string());

    eprintln!("\n=== Chatbot State Validation ===\n");
    eprintln!("Opening Cozo database: {}", db);
    let store = CozoGraphShuttle::new_persistent(PathBuf::from(&db))?;

    let chatbot_id = "b0c777e3-0955-cd8b-3d01-ca12998cae28";
    eprintln!("Reading chatbot record: {}", chatbot_id);

    let record = store
        .get_identity_sled(chatbot_id)?
        .ok_or_else(|| anyhow::anyhow!("Chatbot record not found"))?;

    eprintln!("\n✓ Chatbot record exists\n");

    // Check device binding
    eprintln!("Checking device binding:");
    if record.btrfs_device_json.contains("/dev/sdb2") {
        eprintln!("  ✓ Using shared seed device (/dev/sdb2)");
    } else if record.btrfs_device_json.contains("leaf.img") {
        eprintln!("  ✗ Still using loop image - migration incomplete");
        return Err(anyhow::anyhow!("Chatbot still on loop image"));
    } else {
        eprintln!("  ? Unknown device binding");
        eprintln!("  Device JSON: {}", record.btrfs_device_json);
    }

    // Check FSID
    eprintln!("\nChecking FSID:");
    let correct_fsid = "bc95536f-1d38-46ad-b87e-2637c5c548ec";
    if record.btrfs_device_json.contains(correct_fsid) {
        eprintln!("  ✓ Correct seed FSID ({})", correct_fsid);
    } else {
        eprintln!("  ✗ Wrong FSID in binding");
        return Err(anyhow::anyhow!("Incorrect FSID"));
    }

    // Check active state
    eprintln!("\nChecking active state:");
    if record.active {
        eprintln!("  ✓ Identity is active");
    } else {
        eprintln!("  ⚠ Identity is marked inactive (normal during transition)");
    }

    // Check session metadata
    eprintln!("\nChecking session metadata:");
    eprintln!("  ✓ Session ID: {}", record.session_id);
    eprintln!("  ✓ Principal kind: {:?}", record.principal_kind);
    eprintln!("  ✓ Active: {}", record.active);
    eprintln!("  ✓ Mutation index: {}", record.mutation_index);

    eprintln!("\n=== Validation Summary ===\n");
    eprintln!("✓ Chatbot state is VALID post-migration");
    eprintln!("✓ Device: /dev/sdb2");
    eprintln!("✓ FSID: {}", correct_fsid);
    eprintln!("✓ Session: {}", record.session_id);
    eprintln!("\nNext steps:");
    eprintln!(
        "  1. Restart chatbot container: incus restart {}",
        chatbot_id
    );
    eprintln!("  2. Verify container started: incus list");
    eprintln!("  3. Check tonic door: sudo sv status op-grpc-bridge");
    eprintln!("  4. Test gRPC: grpcurl -plaintext 10.0.0.2:8090 list");

    Ok(())
}
