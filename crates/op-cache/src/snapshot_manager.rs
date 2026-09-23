//! BTRFS snapshot management with automatic rotation
//!
//! Manages cache snapshots with configurable retention policy

use anyhow::{Context, Result};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use tokio::process::Command;

fn is_nested_btrfs_subvolume(fs_type: u64, inode: u64) -> bool {
    fs_type == libc::BTRFS_SUPER_MAGIC as u64 && inode == 256
}

#[derive(Debug, Clone)]
pub struct SnapshotConfig {
    /// Directory containing cache rotation peers.
    pub snapshot_dir: PathBuf,

    /// Maximum cache rotation snapshots (1–2; source is the third cache subvolume).
    pub max_snapshots: usize,

    /// Snapshot name prefix (default: "cache")
    pub prefix: String,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            snapshot_dir: PathBuf::from("/var/lib/op-dbus"),
            max_snapshots: 2,
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
    /// Create new snapshot manager
    pub fn new(source_subvol: PathBuf, config: SnapshotConfig) -> Self {
        Self {
            config,
            source_subvol,
            rotation: tokio::sync::Mutex::new(()),
        }
    }

    /// Refuse a legacy layout that cannot be represented by one complete
    /// source snapshot and two sibling rotation peers. Migration must be an
    /// explicit operator action; never delete historical cache data here.
    pub async fn validate_layout(&self) -> Result<()> {
        let parent = self.source_subvol.parent().ok_or_else(|| {
            anyhow::anyhow!(
                "cache subvolume has no parent: {}",
                self.source_subvol.display()
            )
        })?;
        let legacy = parent.join("@cache-snapshots");
        match tokio::fs::symlink_metadata(&legacy).await {
            Ok(metadata) => {
                anyhow::ensure!(
                    metadata.is_dir() && !metadata.file_type().is_symlink(),
                    "legacy snapshot path {} needs manual migration",
                    legacy.display()
                );
                let mut entries = tokio::fs::read_dir(&legacy).await?;
                if entries.next_entry().await?.is_some() {
                    anyhow::bail!(
                        "legacy snapshots in {} need manual migration before cache rotation",
                        legacy.display()
                    );
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        for category in ["embeddings", "blocks", "queries", "diffs", "kv"] {
            let path = self.source_subvol.join(category);
            let metadata = match tokio::fs::symlink_metadata(&path).await {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            anyhow::ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "cache category {} is not an ordinary directory; migrate it before snapshotting",
                path.display()
            );
            let file = std::fs::File::open(&path)?;
            let mut stat = std::mem::MaybeUninit::<libc::statfs>::uninit();
            if unsafe { libc::fstatfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let stat = unsafe { stat.assume_init() };
            anyhow::ensure!(
                !is_nested_btrfs_subvolume(stat.f_type as u64, metadata.ino()),
                "legacy nested cache subvolume {} needs manual migration before snapshotting",
                path.display()
            );
        }
        Ok(())
    }

    /// Create snapshot with automatic rotation
    pub async fn create_snapshot(&self) -> Result<PathBuf> {
        anyhow::ensure!(
            (1..=2).contains(&self.config.max_snapshots),
            "Cache rotation supports one or two snapshots"
        );
        let _rotation = self.rotation.lock().await;
        self.validate_layout().await?;
        // Create snapshot directory if it doesn't exist
        tokio::fs::create_dir_all(&self.config.snapshot_dir).await?;

        let existing = self.list_snapshots().await?;
        anyhow::ensure!(
            existing.len() <= self.config.max_snapshots,
            "existing cache snapshot count {} exceeds configured limit {}; migrate excess snapshots manually",
            existing.len(),
            self.config.max_snapshots
        );
        let snapshot_counter = self.next_snapshot_counter().await?;
        let snapshot_name = format!("{}-{:06}", self.config.prefix, snapshot_counter);
        let snapshot_path = self.config.snapshot_dir.join(&snapshot_name);

        log::info!("Creating BTRFS snapshot: {}", snapshot_name);

        // Create readonly snapshot
        let output = Command::new("btrfs")
            .args(["subvolume", "snapshot", "-r"])
            .arg(&self.source_subvol)
            .arg(&snapshot_path)
            .output()
            .await
            .context("Failed to execute btrfs snapshot command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to create snapshot: {}", stderr);
        }

        log::info!("Created snapshot: {}", snapshot_path.display());

        // Preserve existing recovery points if creation fails. With two
        // retained snapshots this briefly permits three cache snapshots
        // (five total subvolumes including source and golden), then prunes to
        // two cache snapshots (four total).
        self.rotate_snapshots(self.config.max_snapshots).await?;

        Ok(snapshot_path)
    }

    /// List all snapshots for this cache
    pub async fn list_snapshots(&self) -> Result<Vec<SnapshotInfo>> {
        let mut snapshots = Vec::new();

        if !self.config.snapshot_dir.exists() {
            return Ok(snapshots);
        }

        let mut entries = tokio::fs::read_dir(&self.config.snapshot_dir).await?;

        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Filter by prefix
            let prefix = format!("{}-", self.config.prefix);
            if !name_str.starts_with(&prefix) {
                continue;
            }

            let path = entry.path();
            let metadata = tokio::fs::metadata(&path).await?;
            let created = metadata.created().or_else(|_| metadata.modified()).ok();

            let counter = name_str
                .strip_prefix(&prefix)
                .and_then(|suffix| suffix.parse::<u64>().ok());

            snapshots.push(SnapshotInfo {
                name: name_str.to_string(),
                path: path.clone(),
                created,
                counter,
            });
        }

        // Sort by counter (oldest first), fall back to created time
        snapshots.sort_by(|a, b| match (a.counter, b.counter) {
            (Some(a_counter), Some(b_counter)) => a_counter.cmp(&b_counter),
            _ => a.created.cmp(&b.created),
        });

        Ok(snapshots)
    }

    /// Rotate snapshots, keeping only max_snapshots
    async fn rotate_snapshots(&self, limit: usize) -> Result<()> {
        let snapshots = self.list_snapshots().await?;

        if snapshots.len() <= limit {
            log::debug!("Snapshot count {} within limit {}", snapshots.len(), limit);
            return Ok(());
        }

        // Calculate how many to delete
        let to_delete = snapshots.len() - limit;

        log::info!(
            "Rotating snapshots: {} total, keeping {}, deleting {}",
            snapshots.len(),
            limit,
            to_delete
        );

        // Delete oldest snapshots
        for snapshot in snapshots.iter().take(to_delete) {
            log::info!("Deleting old snapshot: {}", snapshot.name);
            self.delete_snapshot(&snapshot.path).await?;
        }

        Ok(())
    }

    async fn next_snapshot_counter(&self) -> Result<u64> {
        let snapshots = self.list_snapshots().await?;
        let mut max_counter = 0u64;

        for snapshot in snapshots {
            if let Some(counter) = snapshot.counter {
                if counter > max_counter {
                    max_counter = counter;
                }
            }
        }

        Ok(max_counter + 1)
    }

    /// Delete a specific snapshot
    pub async fn delete_snapshot(&self, snapshot_path: &Path) -> Result<()> {
        log::debug!("Deleting snapshot: {}", snapshot_path.display());

        let output = Command::new("btrfs")
            .args(["subvolume", "delete"])
            .arg(snapshot_path)
            .output()
            .await
            .context("Failed to execute btrfs delete command")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to delete snapshot: {}", stderr);
        }

        Ok(())
    }

    /// Delete all snapshots
    pub async fn delete_all_snapshots(&self) -> Result<usize> {
        let snapshots = self.list_snapshots().await?;
        let count = snapshots.len();

        for snapshot in snapshots {
            self.delete_snapshot(&snapshot.path).await?;
        }

        log::info!("Deleted {} snapshots", count);
        Ok(count)
    }

    /// Get oldest snapshot
    #[allow(dead_code)]
    pub async fn oldest_snapshot(&self) -> Result<Option<SnapshotInfo>> {
        let snapshots = self.list_snapshots().await?;
        Ok(snapshots.into_iter().next())
    }

    /// Get newest snapshot
    #[allow(dead_code)]
    pub async fn newest_snapshot(&self) -> Result<Option<SnapshotInfo>> {
        let snapshots = self.list_snapshots().await?;
        Ok(snapshots.into_iter().last())
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
    fn test_snapshot_config_defaults() {
        let config = SnapshotConfig::default();
        assert_eq!(config.max_snapshots, 2);
        assert_eq!(config.prefix, "SNP-cache");
    }

    #[tokio::test]
    async fn rejects_unbounded_or_empty_rotation_before_filesystem_work() {
        for count in [0, 3, 24] {
            let manager = SnapshotManager::new(
                PathBuf::from("/unused"),
                SnapshotConfig {
                    max_snapshots: count,
                    ..Default::default()
                },
            );
            assert!(manager
                .create_snapshot()
                .await
                .unwrap_err()
                .to_string()
                .contains("one or two snapshots"));
        }
    }

    #[tokio::test]
    async fn legacy_snapshots_are_refused_without_deletion_and_ordinary_dirs_pass() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("cache");
        let manager = SnapshotManager::new(
            source.clone(),
            SnapshotConfig {
                snapshot_dir: root.path().to_path_buf(),
                ..Default::default()
            },
        );
        tokio::fs::create_dir_all(source.join("embeddings"))
            .await
            .unwrap();
        manager.validate_layout().await.unwrap();
        let legacy = root.path().join("@cache-snapshots");
        tokio::fs::create_dir(&legacy).await.unwrap();
        tokio::fs::write(legacy.join("SNP-cache-000001"), b"old snapshot")
            .await
            .unwrap();
        assert!(manager
            .validate_layout()
            .await
            .unwrap_err()
            .to_string()
            .contains("manual migration"));
        assert!(legacy.join("SNP-cache-000001").exists());
    }

    #[test]
    fn nested_subvolume_identity_requires_btrfs_and_root_inode() {
        assert!(is_nested_btrfs_subvolume(
            libc::BTRFS_SUPER_MAGIC as u64,
            256
        ));
        assert!(!is_nested_btrfs_subvolume(0, 256));
        assert!(!is_nested_btrfs_subvolume(
            libc::BTRFS_SUPER_MAGIC as u64,
            257
        ));
    }

    #[tokio::test]
    async fn excess_rotation_is_refused_without_deleting_recovery_points() {
        let root = tempfile::tempdir().unwrap();
        for counter in 1..=3 {
            tokio::fs::create_dir(root.path().join(format!("SNP-cache-{counter:06}")))
                .await
                .unwrap();
        }
        let manager = SnapshotManager::new(
            root.path().join("cache"),
            SnapshotConfig {
                snapshot_dir: root.path().to_path_buf(),
                max_snapshots: 2,
                ..Default::default()
            },
        );
        assert!(manager
            .create_snapshot()
            .await
            .unwrap_err()
            .to_string()
            .contains("migrate excess snapshots manually"));
        assert_eq!(manager.list_snapshots().await.unwrap().len(), 3);
    }
}
