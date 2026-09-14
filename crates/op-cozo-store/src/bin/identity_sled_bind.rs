//! Bind provisioned identity containers into Cozo: instance JSON + btrfs
//! device metadata, then drop host-only ghost session rows.
//!
//! Must run while op-grpc-bridge does not hold `/var/lib/op-dbus/identity-cozo`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use op_cozo_store::CozoGraphShuttle;

fn main() {
    if let Err(error) = run() {
        eprintln!("identity-sled-bind: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let db = std::env::var("IDENTITY_SLED_COZO_DB_PATH")
        .unwrap_or_else(|_| "/var/lib/op-dbus/identity-cozo".to_string());
    let keep_raw = std::env::var("IDENTITY_SLED_KEEP_SESSIONS").unwrap_or_else(|_| {
        "bea37ecb-92be-197c-660f-09e806f1a34f,f036f8d8-aabb-c5f2-49c9-18dac19f41ea".to_string()
    });
    let keep: HashSet<String> = keep_raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let overlay_dir = PathBuf::from(
        std::env::var("IDENTITY_SLED_BIND_OVERLAY")
            .unwrap_or_else(|_| "/run/opdbus/identity-sled-bind".to_string()),
    );

    let store = CozoGraphShuttle::new_persistent(PathBuf::from(&db))?;
    let rows = store.list_identity_sessions()?;
    for row in &rows {
        if !keep.contains(&row.session_id) {
            eprintln!("dropping ghost session {}", row.session_id);
            store.delete_identity_sled(&row.session_id)?;
            continue;
        }
        let mut rec = row.clone();
        rec.instance_json = read_overlay(&overlay_dir, &row.session_id, "instance.json")?;
        rec.btrfs_device_json = read_overlay(&overlay_dir, &row.session_id, "btrfs_device.json")?;
        rec.active = false;
        store.put_identity_sled(&rec)?;
        eprintln!(
            "bound {} instance_len={} btrfs_len={}",
            rec.session_id,
            rec.instance_json.len(),
            rec.btrfs_device_json.len()
        );
    }
    eprintln!("remaining:");
    for row in store.list_identity_sessions()? {
        eprintln!(
            "  {} instance_len={} btrfs_len={} instance_head={:?}",
            row.session_id,
            row.instance_json.len(),
            row.btrfs_device_json.len(),
            row.instance_json.chars().take(48).collect::<String>()
        );
    }
    Ok(())
}

fn read_overlay(dir: &Path, session_id: &str, name: &str) -> anyhow::Result<String> {
    let path = dir.join(session_id).join(name);
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| anyhow::anyhow!("read {}: {e}", path.display()))?;
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|e| anyhow::anyhow!("parse {}: {e}", path.display()))?;
    Ok(serde_json::to_string(&value)?)
}
