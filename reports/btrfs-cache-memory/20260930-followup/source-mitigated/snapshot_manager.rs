//! Btrfs cache snapshot sets with retention.
//!
//! Cache categories are separate Btrfs subvolumes. A snapshot or send of the
//! cache root alone does not contain their data. A committed set therefore has
//! one read-only snapshot for the root and each category.

use anyhow::{Context, Result};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use tokio::process::Command;

pub const CACHE_MEMBERS: [&str; 6] = ["root", "embeddings", "blocks", "queries", "diffs", "kv"];
pub(crate) const COMPLETE_MARKER: &str =
    "cache-snapshot-set-v1\nroot\nembeddings\nblocks\nqueries\ndiffs\nkv\n";

#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    /// Directory containing both historical root-only snapshots and new sets.
    pub snapshot_dir: PathBuf,
    /// Number of complete sets to keep. Historical root-only snapshots are not rotated.
    pub max_snapshots: usize,
    pub prefix: String,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            snapshot_dir: PathBuf::from("/var/lib/op-dbus/@cache-snapshots"),
            max_snapshots: 24,
            prefix: "SNP-cache".to_string(),
        }
    }
}

pub struct SnapshotManager {
    config: SnapshotConfig,
    source_subvol: PathBuf,
    rotation: tokio::sync::Mutex<()>,
}

impl SnapshotManager {
    pub fn new(source_subvol: PathBuf, config: SnapshotConfig) -> Self {
        Self {
            config,
            source_subvol,
            rotation: tokio::sync::Mutex::new(()),
        }
    }

    fn set_prefix(&self) -> String {
        format!("{}-set-", self.config.prefix)
    }

    fn validate_prefix(&self) -> Result<()> {
        anyhow::ensure!(
            !self.config.prefix.is_empty()
                && self
                    .config
                    .prefix
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')),
            "invalid cache snapshot prefix"
        );
        Ok(())
    }

    fn source_for(&self, member: &str) -> PathBuf {
        if member == "root" {
            self.source_subvol.clone()
        } else {
            self.source_subvol.join(member)
        }
    }

    fn set_counter(&self, name: &str) -> Option<u64> {
        name.strip_prefix(&self.set_prefix())
            .filter(|suffix| suffix.len() >= 6 && suffix.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|suffix| suffix.parse().ok())
    }

    async fn subvolume_is_readonly(path: &Path) -> Result<bool> {
        let output = Command::new("btrfs")
            .args(["property", "get", "-ts"])
            .arg(path)
            .arg("ro")
            .output()
            .await
            .context("Failed to inspect Btrfs readonly property")?;
        anyhow::ensure!(
            output.status.success(),
            "cannot inspect Btrfs subvolume {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        let value = String::from_utf8_lossy(&output.stdout);
        let value = value.trim();
        anyhow::ensure!(
            matches!(value, "ro=true" | "ro=false"),
            "invalid Btrfs readonly property for {}",
            path.display()
        );
        Ok(value == "ro=true")
    }

    async fn ensure_subvolume(path: &Path, readonly: bool) -> Result<()> {
        let metadata = tokio::fs::symlink_metadata(path).await?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "cache subvolume path is not a directory: {}",
            path.display()
        );
        let file = std::fs::File::open(path)?;
        let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
        if unsafe { libc::fstatfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        anyhow::ensure!(
            unsafe { stat.assume_init() }.f_type as u64 == libc::BTRFS_SUPER_MAGIC as u64
                && metadata.ino() == 256,
            "expected Btrfs subvolume: {}",
            path.display()
        );
        // `btrfs subvolume show` needs privileged B-tree search on some
        // kernels. The typed ro property works for an owning unprivileged
        // process and rejects ordinary directories on Btrfs.
        let is_readonly = Self::subvolume_is_readonly(path).await?;
        if readonly {
            anyhow::ensure!(
                is_readonly,
                "snapshot member is writable: {}",
                path.display()
            );
        }
        Ok(())
    }

    /// Validate a source layout for complete snapshot sets. Runtime cache
    /// opening does not call this: cache reads/writes work even if snapshots
    /// cannot be made on the current filesystem or by the current principal.
    pub async fn validate_layout(&self) -> Result<()> {
        match tokio::fs::symlink_metadata(&self.config.snapshot_dir).await {
            Ok(metadata) => anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "snapshot path is not a directory: {}",
                self.config.snapshot_dir.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        for member in CACHE_MEMBERS {
            Self::ensure_subvolume(&self.source_for(member), false).await?;
        }
        Ok(())
    }

    pub async fn verify_members(path: &Path) -> Result<()> {
        for member in CACHE_MEMBERS {
            Self::ensure_subvolume(&path.join(member), true).await?;
        }
        Ok(())
    }

    async fn validate_complete_set(&self, path: &Path) -> Result<()> {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        anyhow::ensure!(
            self.set_counter(name).is_some(),
            "invalid cache snapshot set name"
        );
        let metadata = tokio::fs::symlink_metadata(path).await?;
        anyhow::ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "invalid set directory"
        );
        let marker = path.join(".complete");
        let marker_metadata = tokio::fs::symlink_metadata(&marker).await?;
        anyhow::ensure!(
            marker_metadata.is_file() && !marker_metadata.file_type().is_symlink(),
            "invalid set completion marker"
        );
        anyhow::ensure!(
            tokio::fs::read_to_string(&marker).await? == COMPLETE_MARKER,
            "invalid set completion marker: {}",
            marker.display()
        );
        Self::verify_members(path).await
    }

    /// All six read-only members of a committed set, in stable send order.
    pub async fn set_members(&self, path: &Path) -> Result<Vec<PathBuf>> {
        self.validate_complete_set(path).await?;
        Ok(CACHE_MEMBERS
            .iter()
            .map(|member| path.join(member))
            .collect())
    }

    /// Create and commit a complete cache snapshot set. All six members are
    /// covered, but their snapshots are taken sequentially: callers must not
    /// treat this as one atomic instant across concurrent cache writers. The
    /// Snowball timing trail is authoritative; cache contents are rebuildable.
    /// A failed partial set is left uncommitted for inspection and is never
    /// listed, sent, or rotated.
    pub async fn create_snapshot(&self) -> Result<PathBuf> {
        anyhow::ensure!(
            self.config.max_snapshots > 0,
            "Cache rotation requires positive retention"
        );
        self.validate_prefix()?;
        let _rotation = self.rotation.lock().await;
        self.validate_layout().await?;
        tokio::fs::create_dir_all(&self.config.snapshot_dir).await?;
        let counter = self.next_snapshot_counter().await?;
        let set_path =
            self.config
                .snapshot_dir
                .join(format!("{}{:06}", self.set_prefix(), counter));
        tokio::fs::create_dir(&set_path).await?;

        for member in CACHE_MEMBERS {
            let source = self.source_for(member);
            let target = set_path.join(member);
            let output = Command::new("btrfs")
                .args(["subvolume", "snapshot", "-r"])
                .arg(&source)
                .arg(&target)
                .output()
                .await
                .context("Failed to execute Btrfs snapshot command")?;
            anyhow::ensure!(
                output.status.success(),
                "failed to snapshot {}: {}",
                source.display(),
                String::from_utf8_lossy(&output.stderr)
            );
            Self::ensure_subvolume(&target, true).await?;
        }

        Self::verify_members(&set_path).await?;
        let marker = set_path.join(".complete");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)?;
        use std::io::Write as _;
        file.write_all(COMPLETE_MARKER.as_bytes())?;
        file.sync_all()?;
        std::fs::File::open(&set_path)?.sync_all()?;
        self.validate_complete_set(&set_path).await?;
        self.rotate_snapshots().await?;
        Ok(set_path)
    }

    pub async fn list_snapshots(&self) -> Result<Vec<SnapshotInfo>> {
        let mut snapshots = Vec::new();
        if !self.config.snapshot_dir.exists() {
            return Ok(snapshots);
        }
        let mut entries = tokio::fs::read_dir(&self.config.snapshot_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let Some(counter) = self.set_counter(&name_str) else {
                continue; // Preserve historical root-only snapshots.
            };
            let path = entry.path();
            if !path.join(".complete").exists() {
                continue; // Preserve failed/incomplete sets for operator review.
            }
            self.validate_complete_set(&path).await?;
            let metadata = tokio::fs::metadata(&path).await?;
            let created = metadata.created().or_else(|_| metadata.modified()).ok();
            snapshots.push(SnapshotInfo {
                name: name_str.into_owned(),
                path,
                created,
                counter: Some(counter),
            });
        }
        snapshots.sort_by_key(|snapshot| snapshot.counter);
        Ok(snapshots)
    }

    async fn next_snapshot_counter(&self) -> Result<u64> {
        if !self.config.snapshot_dir.exists() {
            return Ok(1);
        }
        let mut max_counter = 0;
        let mut entries = tokio::fs::read_dir(&self.config.snapshot_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if let Some(counter) = self.set_counter(&entry.file_name().to_string_lossy()) {
                max_counter = max_counter.max(counter);
            }
        }
        max_counter
            .checked_add(1)
            .context("cache snapshot counter exhausted")
    }

    async fn rotate_snapshots(&self) -> Result<()> {
        let snapshots = self.list_snapshots().await?;
        let to_delete = snapshots.len().saturating_sub(self.config.max_snapshots);
        for snapshot in snapshots.iter().take(to_delete) {
            self.delete_snapshot(&snapshot.path).await?;
        }
        Ok(())
    }

    /// Delete only a verified complete set created by the current format.
    pub async fn delete_snapshot(&self, snapshot_path: &Path) -> Result<()> {
        anyhow::ensure!(
            snapshot_path.parent() == Some(self.config.snapshot_dir.as_path()),
            "snapshot set is outside the configured directory"
        );
        self.validate_complete_set(snapshot_path).await?;
        // Uncommit first so interrupted deletion can never be listed or sent.
        tokio::fs::remove_file(snapshot_path.join(".complete")).await?;
        std::fs::File::open(snapshot_path)?.sync_all()?;
        for member in CACHE_MEMBERS.iter().rev() {
            let path = snapshot_path.join(member);
            // An unprivileged owner can delete its Btrfs snapshot only after
            // clearing the read-only property. It is no longer a committed
            // recovery point, and all six members were verified above.
            let writable = Command::new("btrfs")
                .args(["property", "set", "-ts"])
                .arg(&path)
                .args(["ro", "false"])
                .output()
                .await
                .context("Failed to update Btrfs snapshot property")?;
            anyhow::ensure!(
                writable.status.success(),
                "failed to make cache snapshot member writable for deletion {}: {}",
                path.display(),
                String::from_utf8_lossy(&writable.stderr)
            );
            let output = Command::new("btrfs")
                .args(["subvolume", "delete"])
                .arg(&path)
                .output()
                .await
                .context("Failed to execute Btrfs subvolume delete")?;
            anyhow::ensure!(
                output.status.success(),
                "failed to delete cache snapshot member {}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        tokio::fs::remove_dir(snapshot_path).await?;
        Ok(())
    }

    pub async fn delete_all_snapshots(&self) -> Result<usize> {
        let snapshots = self.list_snapshots().await?;
        let count = snapshots.len();
        for snapshot in snapshots {
            self.delete_snapshot(&snapshot.path).await?;
        }
        Ok(count)
    }

    #[allow(dead_code)]
    pub async fn oldest_snapshot(&self) -> Result<Option<SnapshotInfo>> {
        Ok(self.list_snapshots().await?.into_iter().next())
    }

    #[allow(dead_code)]
    pub async fn newest_snapshot(&self) -> Result<Option<SnapshotInfo>> {
        Ok(self.list_snapshots().await?.into_iter().last())
    }
}

#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    pub name: String,
    pub path: PathBuf,
    #[allow(dead_code)]
    pub created: Option<std::time::SystemTime>,
    pub counter: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_retention_and_set_naming() {
        let config = SnapshotConfig::default();
        assert_eq!(config.max_snapshots, 24);
        let manager = SnapshotManager::new(PathBuf::from("/unused"), config);
        assert_eq!(manager.set_counter("SNP-cache-set-000042"), Some(42));
        assert_eq!(manager.set_counter("SNP-cache-000042"), None);
        assert_eq!(manager.set_counter("SNP-cache-set-42"), None);
    }

    #[tokio::test]
    async fn legacy_and_incomplete_sets_are_preserved_and_never_listed() {
        let root = tempfile::tempdir().unwrap();
        tokio::fs::create_dir(root.path().join("SNP-cache-000001"))
            .await
            .unwrap();
        tokio::fs::create_dir(root.path().join("SNP-cache-set-000002"))
            .await
            .unwrap();
        let manager = SnapshotManager::new(
            root.path().join("cache"),
            SnapshotConfig {
                snapshot_dir: root.path().to_path_buf(),
                ..Default::default()
            },
        );
        assert!(manager.list_snapshots().await.unwrap().is_empty());
        assert_eq!(manager.next_snapshot_counter().await.unwrap(), 3);
        assert!(root.path().join("SNP-cache-000001").exists());
        assert!(root.path().join("SNP-cache-set-000002").exists());
    }

    #[tokio::test]
    async fn zero_retention_fails_before_filesystem_work() {
        let manager = SnapshotManager::new(
            PathBuf::from("/unused"),
            SnapshotConfig {
                max_snapshots: 0,
                ..Default::default()
            },
        );
        assert!(
            manager
                .create_snapshot()
                .await
                .unwrap_err()
                .to_string()
                .contains("positive retention")
        );
    }
}
