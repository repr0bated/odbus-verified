//! Inspect the identity namespace owned by the existing host mount service.
//!
//! The bridge owns the lungs server, not a second FUSE daemon. Missing mounts,
//! wrong identity selectors and stale views are errors, never a reason to
//! capture a global view or silently fall back to an unauthenticated mount.

use std::fs::OpenOptions;
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde_json::Value;

/// Target owned by the host's strict mount.fuse.3tchedFS adapter.
pub const FUSED_MOUNTPOINT: &str = "/run/mounts/3tchedFS";
const SESSION_FILE: &str = "/etc/op-dbus/human-lungs-session";
const OWNER_FILE: &str = "/etc/op-dbus/human-lungs-call-owner-uid";
const MAX_VIEW_BYTES: u64 = 16 * 1024 * 1024;

fn protected_selector(path: &str) -> Result<String> {
    let file = OpenOptions::new().read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(path)
        .with_context(|| format!("open host mount selector {path}"))?;
    let metadata = file.metadata()?;
    ensure!(metadata.is_file() && metadata.uid() == 0 && metadata.gid() == 0
        && matches!(metadata.mode() & 0o7777, 0o400 | 0o600),
        "host mount selector is not protected: {path}");
    let mut value = String::new();
    file.take(257).read_to_string(&mut value)?;
    ensure!(!value.is_empty() && value.len() <= 256, "invalid host mount selector: {path}");
    Ok(value.trim().to_owned())
}

fn read_json(path: &Path, limit: u64) -> Result<Value> {
    let file = OpenOptions::new().read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(path)
        .with_context(|| format!("read mounted namespace {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "mounted namespace document exceeds byte bound");
    serde_json::from_slice(&bytes).context("invalid mounted namespace document")
}

fn mounted_id(mountinfo: &str) -> Result<&str> {
    let mut selected = None;
    for line in mountinfo.lines() {
        let Some((mount, filesystem)) = line.split_once(" - ") else { continue; };
        let mut fields = mount.split_whitespace();
        let id = fields.next().context("mountinfo record has no ID")?;
        // Following the mount ID: parent, major:minor, root, mountpoint, options.
        let target = fields.nth(3);
        if target != Some(FUSED_MOUNTPOINT) { continue; }
        let options = fields.next().unwrap_or_default();
        let mut filesystem = filesystem.split_whitespace();
        ensure!(filesystem.next() == Some("fuse.3tchedFS")
            && filesystem.next() == Some("3tchedFS")
            && options.split(',').any(|option| option == "rw"),
            "host namespace is not the service-owned writable 3tchedFS mount");
        ensure!(selected.replace(id).is_none(), "host namespace has stacked mounts");
    }
    selected.context("identity namespace is not mounted; use the existing host mount owner")
}

fn validate_namespace(metadata: &Value, view: &Value, catalog: &Value, session: &str, uid: u32) -> Result<()> {
    ensure!(metadata["format"] == "3tchedfs/1"
        && metadata["mount_identity"]["kind"] == "host_session"
        && metadata["mount_identity"]["session"] == session
        && metadata["local_call_owner_uid"].as_u64() == Some(u64::from(uid))
        && metadata.get("workspace_id").is_some_and(Value::is_null),
        "mounted namespace differs from the configured host identity or caller UID");
    let hash = catalog["catalog_hash"].as_str().context("sealed catalog hash is absent")?;
    ensure!(hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "sealed catalog hash is invalid");
    let generation = catalog["generation"].as_u64().context("sealed catalog generation is absent")?;
    ensure!(view["source_catalog_hash"] == hash
        && view["source_catalog_generation"].as_u64() == Some(generation)
        && metadata["source_catalog_hash"] == hash
        && metadata["source_catalog_generation"].as_u64() == Some(generation)
        && view["view_id"].as_str().is_some()
        && metadata["view_id"] == view["view_id"],
        "mounted namespace is not pinned to the current sealed catalog");
    let objects = view["objects"].as_object().context("mounted view objects are absent")?;
    let plugins = catalog["plugins"].as_object().context("sealed plugin inventory is absent")?;
    let expected = |id: &str| id != "threetched" && id != "tched_router";
    ensure!(objects.contains_key("tched_fs")
        && objects.len() == plugins.keys().filter(|id| expected(id)).count()
        && objects.keys().all(|id| expected(id) && plugins.contains_key(id)),
        "mounted namespace differs from the host owner's sealed plugin selection");
    Ok(())
}

/// Return the existing host identity's mount after checking its ownership,
/// identity selectors and current sealed catalog view. This does not claim that
/// a method grant exists or that a backend operation has succeeded.
///
/// The strict host mount adapter owns creation, capture, caller-UID assignment
/// and teardown. Guest mount services own their separate container namespaces.
/// This inspection is optional to bridge startup: requiring a mount before
/// starting lungs would create a circular service dependency.
pub fn existing_identity_mount() -> Result<PathBuf> {
    let before = std::fs::read_to_string("/proc/self/mountinfo")?;
    let mount_id = mounted_id(&before)?;
    let session = protected_selector(SESSION_FILE)?;
    ensure!(uuid::Uuid::parse_str(&session).is_ok_and(|id| id.to_string() == session),
        "host mount session selector is not canonical");
    let uid: u32 = protected_selector(OWNER_FILE)?.parse().context("host mount caller UID is invalid")?;
    let mountpoint = PathBuf::from(FUSED_MOUNTPOINT);
    let metadata = read_json(&mountpoint.join(".3tchedfs.json"), 64 * 1024)?;
    let view = read_json(&mountpoint.join("view.json"), MAX_VIEW_BYTES)?;
    let catalog_dir = std::env::var_os("OP_BLOB_CATALOG_DIR").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(crate::source::DEFAULT_CATALOG_DIR));
    let catalog = read_json(&catalog_dir.join(".manifest.json"), MAX_VIEW_BYTES)?;
    validate_namespace(&metadata, &view, &catalog, &session, uid)?;
    let after = std::fs::read_to_string("/proc/self/mountinfo")?;
    ensure!(mounted_id(&after)? == mount_id, "host namespace mount changed during inspection");
    Ok(mountpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_presence_requires_exact_filesystem_source_and_writable_owner() {
        let record = "42 1 0:1 / /run/mounts/3tchedFS rw,nosuid - fuse.3tchedFS 3tchedFS rw\n";
        assert_eq!(mounted_id(record).unwrap(), "42");
        assert!(mounted_id("").is_err());
        assert!(mounted_id(&record.replace("fuse.3tchedFS", "tmpfs")).is_err());
        assert!(mounted_id(&record.replace("3tchedFS rw\n", "other rw\n")).is_err());
        assert!(mounted_id(&record.replace("rw,nosuid", "ro,nosuid")).is_err());
        assert!(mounted_id(&format!("{record}{record}")).is_err());
    }
}