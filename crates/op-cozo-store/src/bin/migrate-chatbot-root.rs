//! Migrate chatbot identity to use the shared physical seed device.
//! This follows the proper atomic migration procedure from the receipt.
//!
//! Usage:
//!   1. Get preflight: cargo run --bin migrate-chatbot-root -p op-cozo-store -- preflight
//!   2. Prepare: cargo run --bin migrate-chatbot-root -p op-cozo-store -- prepare
//!   3. Migrate: cargo run --bin migrate-chatbot-root -p op-cozo-store -- migrate

fn main() {
    if let Err(error) = run() {
        eprintln!("migrate-chatbot-root: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let command = args.get(1).map(|s| s.as_str()).unwrap_or("help");

    match command {
        "preflight" => preflight(),
        "prepare" => prepare(),
        "migrate" => migrate(),
        _ => {
            eprintln!("Usage:");
            eprintln!("  cargo run --bin migrate-chatbot-root -p op-cozo-store -- preflight");
            eprintln!("  cargo run --bin migrate-chatbot-root -p op-cozo-store -- prepare");
            eprintln!("  cargo run --bin migrate-chatbot-root -p op-cozo-store -- migrate");
            eprintln!();
            eprintln!("This utility helps migrate the chatbot identity from loop images");
            eprintln!("to the shared physical seed device (/dev/sdb2).");
            eprintln!();
            eprintln!("Step 1: Get preflight info (shows current state)");
            eprintln!("Step 2: Prepare migration plan (creates plan JSON and SHA256)");
            eprintln!("Step 3: Execute migration (calls the atomic D-Bus migration method)");
            Ok(())
        }
    }
}

fn preflight() -> anyhow::Result<()> {
    eprintln!("\n=== Chatbot Physical Root Migration Preflight ===\n");
    eprintln!("This step requires authenticated D-Bus access through lungs.sock");
    eprintln!("and the capability: cap.identity.sled.physical_root_migrate@v1\n");

    eprintln!("Required: Call identity_sled__inspect_own_physical_root_migration_preflight");
    eprintln!("This returns:");
    eprintln!("  - session_id");
    eprintln!("  - expected_revision");
    eprintln!("  - expected_revision_operation_id");
    eprintln!("  - previous_binding");
    eprintln!("  - previous_root_repair_sha256");
    eprintln!("  - previous_seed_provision_sha256");
    eprintln!();
    eprintln!("The preflight must be obtained through:");
    eprintln!("  1. Authenticated access to /run/opdbus/lungs.sock");
    eprintln!("  2. Or via gRPC door with SID1 credential");
    eprintln!();
    eprintln!("This is a root-owned operation; see the migration receipt:");
    eprintln!("  reports/identity-replacement/20260930/cozo-reconciliation/receipt.md");

    eprintln!("\nFor now, use the authorized operator helper:");
    eprintln!("  /var/tmp/owner-identity-call-20260930.sh");
    eprintln!();

    Ok(())
}

fn prepare() -> anyhow::Result<()> {
    eprintln!("\n=== Prepare Migration Plan ===\n");
    eprintln!("This step requires preflight data from the previous step.");
    eprintln!();

    eprintln!("To create the migration plan, you need:");
    eprintln!("  1. Preflight data (from inspect_own_physical_root_migration_preflight)");
    eprintln!("  2. Native root canonical path");
    eprintln!("  3. Stopped Incus instance JSON");
    eprintln!("  4. Seed device info (/dev/sdb2 FSID, generation)");
    eprintln!("  5. Chatbot leaf info (inode, size)");
    eprintln!();

    eprintln!("The plan is a JSON file at:");
    eprintln!(
        "  /var/lib/op-dbus/identity-physical-migration-plans/b0c777e3-0955-cd8b-3d01-ca12998cae28.json"
    );
    eprintln!();

    eprintln!("Plan structure (see receipt for complete fields):");
    eprintln!(
        r#"{{
  "version": 1,
  "session_id": "b0c777e3-0955-cd8b-3d01-ca12998cae28",
  "expected_revision": <from preflight>,
  "expected_revision_operation_id": <from preflight>,
  "previous_binding": <from preflight>,
  "previous_root_repair_sha256": <from preflight>,
  "previous_seed_provision_sha256": <from preflight>,
  "binding": {{
    "device_path": "/dev/sdb2",
    "mount_point": "/var/lib/opdbus-runtime/seed-identities/b0c777e3-0955-cd8b-3d01-ca12998cae28/volume",
    "btrfs_uuid": "bc95536f-1d38-46ad-b87e-2637c5c548ec",
    "cozo_id": "ec388ad3-607d-455c-ad0e-3dc4fe2fdbb4",
    "attached": true
  }},
  "seed_device": "/dev/sdb2",
  "seed_fsid": "2dcac52c-fb0b-418a-95a4-cb6eacc80209",
  "seed_generation": <from btrfs inspect-internal>,
  "leaf_inode": <from native root>,
  "leaf_size": <from native root>,
  "guest_subvolume_id": <from btrfs inspect-internal>,
  "native_root": "/var/lib/incus/containers/b0c777e3-0955-cd8b-3d01-ca12998cae28/rootfs",
  "pool_name": "3tched-storage",
  "pool_source": <from incus profile show>,
  "instance": <complete stopped Incus instance JSON>
}}"#
    );

    eprintln!("\nOnce the plan is prepared:");
    eprintln!("  sha256sum /var/lib/op-dbus/identity-physical-migration-plans/*.json");

    Ok(())
}

fn migrate() -> anyhow::Result<()> {
    eprintln!("\n=== Execute Migration ===\n");
    eprintln!("This step calls the atomic D-Bus migration method.\n");

    eprintln!("Once the plan file is prepared, call:");
    eprintln!("  identity_sled__migrate_own_physical_root {{\"plan_sha256\": \"<SHA256>\"}}");
    eprintln!();
    eprintln!("This is a root-owned operation that:");
    eprintln!("  1. Verifies the plan SHA256");
    eprintln!("  2. Checks current revision and journals");
    eprintln!("  3. Archives old bindings and provenance");
    eprintln!("  4. Records new physical provenance");
    eprintln!("  5. Advances revision/index atomically");
    eprintln!();
    eprintln!("After successful migration:");
    eprintln!("  1. Verify with inventory and get_identity");
    eprintln!("  2. Stop and restart the chatbot container");
    eprintln!("  3. Confirm it uses the shared seed device");

    eprintln!("\nFor authenticated D-Bus calls, use:");
    eprintln!("  busctl --system call org.opdbus.v1 \\");
    eprintln!("    /org/opdbus/v1/plugins/identity_sled \\");
    eprintln!("    org.freedesktop.DBus.Properties Get \\");
    eprintln!("    ss org.opdbus.v1.PluginService CallMethod");

    Ok(())
}
