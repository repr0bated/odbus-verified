//! Explicit stopped-instance root upgrade. Never deletes the old root or user data.
use crate::{btrfs, seed::{SeedContainer, SeedLocation, inspect_seed, read_backup, write_new_file}};
use anyhow::{Context, Result, ensure};
use std::os::unix::fs::MetadataExt;
use std::os::fd::AsRawFd as _;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct RootUpgradeReceipt {
    pub name: String,
    pub seed_uuid: String,
    pub volume_uuid: String,
    pub volume_treeid: u64,
    pub old_volume_uuid: String,
    pub rollback_name: String,
}

fn rollback_name(name: &str, operation_digest: &str) -> Result<String> {
    ensure!(!name.is_empty() && name.len() <= 63 && !name.starts_with('.')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
        && operation_digest.len() == 64
        && operation_digest.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
        "unsafe root upgrade name or operation digest");
    Ok(format!(".{name}.upgrade-{operation_digest}"))
}

/// The caller owns the durable exchange intent and must prove the SAME native
/// UUID stopped immediately before entering. Replays reconcile the original
/// exact old/new subvolume pair; they never allocate another snapshot or swap
/// back. The old root remains at the operation-keyed rollback name forever.
pub fn upgrade_from_seed(location: &SeedLocation, name: &str, operation_digest: &str,
    expected_old_uuid: uuid::Uuid,
) -> Result<RootUpgradeReceipt> {
    ensure!(location.expected_seed_uuid.is_some() && location.expected_pool_fsid.is_some(),
        "root upgrade requires exact seed and pool authority pins");
    let rollback = rollback_name(name, operation_digest)?;
    let seed_uuid = inspect_seed(location)?;
    let state = btrfs::open_absolute_dir(&location.incus_dir)?;
    let pools = btrfs::open_dir(&state, b"storage-pools")?.context("Incus storage-pools absent")?;
    let pool = btrfs::open_dir(&pools, location.pool.as_bytes())?.context("pool absent")?;
    let mount = btrfs::mount_id(&pool)?;
    let fsid = btrfs::fsid(&pool)?;
    ensure!(location.expected_pool_fsid.is_some_and(|expected| *expected.as_bytes() == fsid), "pool FSID changed");
    let containers = btrfs::open_dir(&pool, b"containers")?.context("containers absent")?;
    ensure!(btrfs::mount_id(&containers)? == mount, "containers directory covered by mount");
    let current = btrfs::open_volume(&containers, name.as_bytes(), location.expected_container_uid)?.context("old root absent")?;
    let before = btrfs::subvolume_info(&current)?;
    ensure!(btrfs::mount_id(&current)? == mount && btrfs::fsid(&current)? == fsid
        && current.metadata()?.ino() == btrfs::SUBVOL_ROOT_INO && before.flags & btrfs::SUBVOL_RDONLY == 0,
        "root is not the existing writable native pool volume");
    let mut staged = btrfs::open_volume(&containers, rollback.as_bytes(), location.expected_container_uid)?;
    if staged.is_none() {
        ensure!(before.uuid == *expected_old_uuid.as_bytes(), "old root UUID changed before first snapshot");
        let parent = btrfs::open_dir(&pool, location.seed_dir.as_bytes())?.context("seed parent absent")?;
        let seed = btrfs::open_dir(&parent, location.seed_name.as_bytes())?.context("seed absent")?;
        let info = btrfs::subvolume_info(&seed)?;
        ensure!(info.uuid == *uuid::Uuid::parse_str(&seed_uuid)?.as_bytes()
            && info.flags & btrfs::SUBVOL_RDONLY != 0 && btrfs::mount_id(&seed)? == mount,
            "seed changed after proof");
        btrfs::snapshot(&seed, &containers, rollback.as_bytes())?;
        staged = btrfs::open_dir(&containers, rollback.as_bytes())?;
    }
    let staged = staged.context("staged root absent after snapshot")?;
    let stage = btrfs::subvolume_info(&staged)?;
    ensure!(btrfs::mount_id(&staged)? == mount && btrfs::fsid(&staged)? == fsid,
        "upgrade staging escaped native pool");
    if before.uuid == *expected_old_uuid.as_bytes() {
        ensure!(stage.parent_uuid == *uuid::Uuid::parse_str(&seed_uuid)?.as_bytes()
            && stage.uuid != before.uuid && stage.flags & btrfs::SUBVOL_RDONLY == 0,
            "upgrade staging is not the original Golden snapshot");
        let (backup, mode) = read_backup(&current)?;
        if let Some(backup) = backup {
            let (already, _) = read_backup(&staged)?;
            if let Some(already) = already { ensure!(already == backup, "staged native metadata differs"); }
            else { write_new_file(&staged, b"backup.yaml", &backup, mode)?; }
        }
        let owner = current.metadata()?;
        // Only the PRIVATE new wrapper receives Incus's existing protected
        // directory metadata. rootfs stays uid0 for the established idmapped
        // mount; the shared read-only seed is never changed.
        // SAFETY: held private snapshot FD; exact old wrapper uid/gid/mode.
        ensure!(unsafe { libc::fchown(staged.as_raw_fd(), owner.uid(), owner.gid()) } == 0
            && unsafe { libc::fchmod(staged.as_raw_fd(), owner.mode() & 0o7777) } == 0,
            "private replacement wrapper metadata could not be preserved");
        btrfs::syncfs(&staged)?;
        btrfs::exchange(&containers, rollback.as_bytes(), name.as_bytes())?;
        btrfs::syncfs(&containers)?;
    } else {
        ensure!(before.parent_uuid == *uuid::Uuid::parse_str(&seed_uuid)?.as_bytes()
            && stage.uuid == *expected_old_uuid.as_bytes(),
            "unknown root exchange outcome: preserve both roots and reconcile original intent");
    }
    inspect_upgrade(location, name, operation_digest, expected_old_uuid)
}

/// Read-only receipt reconciliation used after exchange, startup and replay.
pub fn inspect_upgrade(location: &SeedLocation, name: &str, operation_digest: &str,
    expected_old_uuid: uuid::Uuid,
) -> Result<RootUpgradeReceipt> {
    let rollback_name = rollback_name(name, operation_digest)?;
    let SeedContainer {seed_uuid, volume_uuid, volume_treeid, ..} = crate::inspect_container(location, name)?;
    let state = btrfs::open_absolute_dir(&location.incus_dir)?;
    let pools = btrfs::open_dir(&state, b"storage-pools")?.context("Incus storage-pools absent")?;
    let pool = btrfs::open_dir(&pools, location.pool.as_bytes())?.context("pool absent")?;
    let containers = btrfs::open_dir(&pool, b"containers")?.context("containers absent")?;
    let old = btrfs::open_volume(&containers, rollback_name.as_bytes(), location.expected_container_uid)?.context("preserved old root absent")?;
    let old_info = btrfs::subvolume_info(&old)?;
    ensure!(old_info.uuid == *expected_old_uuid.as_bytes() && old_info.flags & btrfs::SUBVOL_RDONLY == 0
        && btrfs::fsid(&old)? == btrfs::fsid(&pool)? && btrfs::mount_id(&old)? == btrfs::mount_id(&pool)?,
        "preserved old root UUID, writable data or native pool changed");
    Ok(RootUpgradeReceipt {name:name.into(), seed_uuid, volume_uuid, volume_treeid,
        old_volume_uuid:expected_old_uuid.to_string(), rollback_name})
}

#[cfg(test)]
mod tests {
    #[test]
    fn rollback_is_deterministic_and_cannot_target_other_paths() {
        let digest = "a".repeat(64);
        assert_eq!(super::rollback_name("Jeremy", &digest).unwrap(), format!(".Jeremy.upgrade-{digest}"));
        assert!(super::rollback_name("../Jeremy", &digest).is_err());
        assert!(super::rollback_name("Jeremy", "unknown").is_err());
        assert!(super::rollback_name(".Jeremy", &digest).is_err());
    }
}
