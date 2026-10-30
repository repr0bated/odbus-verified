//! Update chatbot identity_sled record with correct seed FSID.
//! Usage: IDENTITY_SLED_COZO_DB_PATH=/var/lib/op-dbus/identity-cozo cargo run --bin update-chatbot-fsid

use op_cozo_store::CozoGraphShuttle;
use serde_json::{Value, json};
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("update-chatbot-fsid: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let db = std::env::var("IDENTITY_SLED_COZO_DB_PATH")
        .unwrap_or_else(|_| "/var/lib/op-dbus/identity-cozo".to_string());

    let chatbot_id = "b0c777e3-0955-cd8b-3d01-ca12998cae28";
    let new_fsid = "bc95536f-1d38-46ad-b87e-2637c5c548ec";
    let new_device = "/dev/sdb2";

    eprintln!("Opening Cozo database: {}", db);
    let store = CozoGraphShuttle::new_persistent(PathBuf::from(&db))?;

    eprintln!("Reading chatbot record: {}", chatbot_id);
    let mut record = match store.get_identity_sled(chatbot_id)? {
        Some(r) => r,
        None => {
            return Err(anyhow::anyhow!("Chatbot record not found: {}", chatbot_id));
        }
    };

    eprintln!(
        "Current btrfs_device_json ({} chars): {}",
        record.btrfs_device_json.len(),
        &record.btrfs_device_json[..100.min(record.btrfs_device_json.len())]
    );

    // Parse and update the btrfs_device_json
    let mut device_data: Value = serde_json::from_str(&record.btrfs_device_json)?;

    eprintln!("Device data before update: {:#}", device_data);

    // Update the btrfs_uuid and device fields
    if let Some(obj) = device_data.as_object_mut() {
        obj.insert("btrfs_uuid".to_string(), json!(new_fsid));
        obj.insert("device_path".to_string(), json!(new_device));
    }

    eprintln!("Device data after update: {:#}", device_data);

    record.btrfs_device_json = device_data.to_string();
    record.active = false; // Mark inactive to trigger reattach

    eprintln!("Writing updated record...");
    store.put_identity_sled(&record)?;

    eprintln!("Verification: reading back the record");
    let updated = store
        .get_identity_sled(chatbot_id)?
        .ok_or_else(|| anyhow::anyhow!("Record disappeared after update"))?;

    eprintln!("Updated btrfs_device_json: {}", updated.btrfs_device_json);

    if updated.btrfs_device_json.contains(new_fsid) {
        eprintln!("✓ Successfully updated chatbot FSID to: {}", new_fsid);
        eprintln!("✓ Device path set to: {}", new_device);
        eprintln!("✓ Active flag set to: false (will reattach on next mount)");
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "Update verification failed: new FSID not found in record"
        ))
    }
}
