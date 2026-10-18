//! Btrfs cache snapshot sets with retention.
//!
//! Cache categories are separate Btrfs subvolumes. A snapshot or send of the
//! cache root alone does not contain their data. A committed set therefore has
//! one read-only snapshot for the root and each category.

use super::managed_process::{self, COMMAND_TIMEOUT};
use anyhow::{Context, Result};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, LazyLock,
    atomic::{AtomicBool, Ordering},
};
use tokio::process::Command;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

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

#[derive(Clone)]
pub struct SnapshotManager {
    config: SnapshotConfig,
    source_subvol: PathBuf,
    rotation: Arc<tokio::sync::Mutex<()>>,
}

impl SnapshotManager {
    pub fn new(source_subvol: PathBuf, config: SnapshotConfig) -> Self {
        Self {
            config,
            source_subvol,
            rotation: Arc::new(tokio::sync::Mutex::new(())),
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
        let output = managed_process::output(
            Command::new("btrfs")
                .args(["property", "get", "-ts"])
                .arg(path)
                .arg("ro"),
            COMMAND_TIMEOUT,
        )
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
            self.set_counter(name).is_some() || self.is_transport_name(name),
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
            marker_metadata.is_file()
                && !marker_metadata.file_type().is_symlink()
                && marker_metadata.len() == COMPLETE_MARKER.len() as u64,
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

        self.populate_set(&set_path).await?;
        self.rotate_snapshots().await?;
        Ok(set_path)
    }

    fn is_transport_name(&self, name: &str) -> bool {
        name.strip_prefix(&format!("{}-transfer-", self.config.prefix))
            .is_some_and(|suffix| uuid::Uuid::parse_str(suffix).is_ok())
    }

    /// Temporary transport sets have unique names and never enter manual
    /// snapshot rotation. Creation remains owned after caller cancellation;
    /// its result is dropped and cleaned if the caller has gone away.
    pub(crate) async fn create_transport_snapshot(&self) -> Result<TemporarySet> {
        self.validate_prefix()?;
        let permit = transfer_permit().await?;
        let manager = self.clone();
        tokio::spawn(async move {
            manager.validate_layout().await?;
            std::fs::create_dir_all(&manager.config.snapshot_dir)?;
            let path = manager.config.snapshot_dir.join(format!(
                "{}-transfer-{}",
                manager.config.prefix,
                uuid::Uuid::new_v4()
            ));
            let set = TemporarySet::create(path, permit)?;
            if let Err(error) = manager.populate_set(set.path()).await {
                if let Err(cleanup) = set.cleanup().await {
                    return Err(
                        error.context(format!("temporary snapshot cleanup failed: {cleanup:#}"))
                    );
                }
                return Err(error);
            }
            Ok(set)
        })
        .await
        .context("snapshot creation task failed")?
    }

    async fn populate_set(&self, set_path: &Path) -> Result<()> {
        for member in CACHE_MEMBERS {
            let source = self.source_for(member);
            let target = set_path.join(member);
            let output = managed_process::output(
                Command::new("btrfs")
                    .args(["subvolume", "snapshot", "-r"])
                    .arg(&source)
                    .arg(&target),
                COMMAND_TIMEOUT,
            )
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

        Self::verify_members(set_path).await?;
        let marker = set_path.join(".complete");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)?;
        use std::io::Write as _;
        file.write_all(COMPLETE_MARKER.as_bytes())?;
        file.sync_all()?;
        std::fs::File::open(set_path)?.sync_all()?;
        self.validate_complete_set(set_path).await?;
        Ok(())
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
            let writable = managed_process::output(
                Command::new("btrfs")
                    .args(["property", "set", "-ts"])
                    .arg(&path)
                    .args(["ro", "false"]),
                COMMAND_TIMEOUT,
            )
            .await
            .context("Failed to update Btrfs snapshot property")?;
            anyhow::ensure!(
                writable.status.success(),
                "failed to make cache snapshot member writable for deletion {}: {}",
                path.display(),
                String::from_utf8_lossy(&writable.stderr)
            );
            let output = managed_process::output(
                Command::new("btrfs")
                    .args(["subvolume", "delete"])
                    .arg(&path),
                COMMAND_TIMEOUT,
            )
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

// Admission happens before creating snapshots or spawning cleanup owners.
static TRANSFERS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(2)));

static CLEANUP_FAILED: AtomicBool = AtomicBool::new(false);

pub(crate) async fn transfer_permit() -> Result<OwnedSemaphorePermit> {
    let permit = TRANSFERS.clone().acquire_owned().await?;
    anyhow::ensure!(
        !CLEANUP_FAILED.load(Ordering::Acquire),
        "temporary cache cleanup previously failed; inspect logged paths before restarting transport"
    );
    Ok(permit)
}

/// Owns only a directory created by this operation. A committed receive can
/// relinquish ownership; source snapshots and partial receives are removed.
pub(crate) struct TemporarySet {
    path: Option<PathBuf>,
    permit: Option<OwnedSemaphorePermit>,
}

impl TemporarySet {
    pub(crate) fn create(path: PathBuf, permit: OwnedSemaphorePermit) -> Result<Self> {
        // Synchronous create closes the cancellation gap between mkdir and
        // registering ownership; existing destinations are never adopted.
        std::fs::create_dir(&path)?;
        Ok(Self {
            path: Some(path),
            permit: Some(permit),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        self.path.as_deref().expect("live temporary set")
    }

    pub(crate) fn preserve(mut self) {
        self.path.take();
    }

    pub(crate) async fn cleanup(mut self) -> Result<()> {
        let result = cleanup_owned_set(self.path()).await;
        if result.is_ok() {
            self.path.take();
        }
        result
    }
}

impl Drop for TemporarySet {
    fn drop(&mut self) {
        let Some(path) = self.path.take() else {
            return;
        };
        let permit = self.permit.take();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _permit = permit;
                // A cancelled send/receive child was signalled before this
                // guard drops. Retry briefly if its kernel work is draining.
                for attempt in 0..3 {
                    match cleanup_owned_set(&path).await {
                        Ok(()) => return,
                        Err(error) if attempt == 2 => { CLEANUP_FAILED.store(true, Ordering::Release); tracing::error!(path = %path.display(), %error, "temporary cache set cleanup failed"); },
                        Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
                    }
                }
            });
        } else {
            CLEANUP_FAILED.store(true, Ordering::Release);
            tracing::error!(path = %path.display(), "runtime stopped before temporary cache set cleanup; recovery required");
        }
    }
}

async fn cleanup_owned_set(path: &Path) -> Result<()> {
    if !path.try_exists()? {
        return Ok(());
    }
    let metadata = std::fs::symlink_metadata(path)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "invalid temporary set"
    );
    // Never recursively remove arbitrary files or unexpected nested objects.
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let name = entry.file_name();
        anyhow::ensure!(
            name == ".complete" || CACHE_MEMBERS.iter().any(|member| name == *member),
            "unexpected object in temporary cache set: {}",
            entry.path().display()
        );
        anyhow::ensure!(
            !entry.file_type()?.is_symlink(),
            "symlink in temporary cache set"
        );
    }
    let marker = path.join(".complete");
    if marker.try_exists()? {
        std::fs::remove_file(marker)?;
    }
    for member in CACHE_MEMBERS.iter().rev() {
        let member = path.join(member);
        if !member.try_exists()? {
            continue;
        }
        SnapshotManager::ensure_subvolume(&member, false).await?;
        let output = managed_process::output(
            Command::new("btrfs")
                .args(["subvolume", "delete"])
                .arg(&member),
            COMMAND_TIMEOUT,
        )
        .await?;
        if !output.status.success() {
            // Owning unprivileged users may need a writable source snapshot.
            // Received UUID protection is respected; never force-clear it.
            let writable = managed_process::output(
                Command::new("btrfs")
                    .args(["property", "set", "-ts"])
                    .arg(&member)
                    .args(["ro", "false"]),
                COMMAND_TIMEOUT,
            )
            .await?;
            anyhow::ensure!(
                writable.status.success(),
                "cannot prepare temporary snapshot deletion: {}",
                String::from_utf8_lossy(&writable.stderr)
            );
            let retry = managed_process::output(
                Command::new("btrfs")
                    .args(["subvolume", "delete"])
                    .arg(&member),
                COMMAND_TIMEOUT,
            )
            .await?;
            anyhow::ensure!(
                retry.status.success(),
                "cannot delete temporary snapshot: {}",
                String::from_utf8_lossy(&retry.stderr)
            );
        }
    }
    std::fs::remove_dir(path)?;
    Ok(())
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

    #[tokio::test]
    async fn cleanup_refuses_unowned_contents() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("unrelated"), "keep").unwrap();
        assert!(cleanup_owned_set(root.path()).await.is_err());
        assert_eq!(
            std::fs::read_to_string(root.path().join("unrelated")).unwrap(),
            "keep"
        );
    }

    #[tokio::test]
    async fn transport_sets_are_excluded_from_manual_rotation() {
        let root = tempfile::tempdir().unwrap();
        let manager = SnapshotManager::new(
            root.path().join("cache"),
            SnapshotConfig {
                snapshot_dir: root.path().into(),
                ..Default::default()
            },
        );
        let name = format!("SNP-cache-transfer-{}", uuid::Uuid::new_v4());
        std::fs::create_dir(root.path().join(&name)).unwrap();
        assert!(manager.is_transport_name(&name));
        assert!(!manager.is_transport_name("SNP-cache-transfer-../escape"));
        assert_eq!(manager.next_snapshot_counter().await.unwrap(), 1);
        assert!(manager.list_snapshots().await.unwrap().is_empty());
    }

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
