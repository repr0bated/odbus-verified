//! BTRFS-based deployment image manager with symlink deduplication
//!
//! Creates deployment "images" as folders where:
//! - Each folder is a BTRFS snapshot for streaming
//! - Files that exist in previous images are symlinked (deduplication)
//! - New files are copied normally
//! - Images can be streamed for deployment

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::fs as async_fs;
use tokio::process::Command;

/// Deployment image metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageMetadata {
    pub name: String,
    pub path: PathBuf,
    pub created: i64,
    pub files: Vec<FileEntry>,
    pub total_size: u64,
    pub unique_size: u64,    // Size of files unique to this image
    pub symlinked_size: u64, // Size of files symlinked from previous images
}

/// File entry in an image
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: PathBuf,
    pub is_symlink: bool,
    pub symlink_target: Option<PathBuf>, // If symlink, where it points
    pub size: u64,
    pub hash: Option<String>, // SHA256 hash for deduplication
}

/// Image manager for BTRFS-based deployment images
pub struct ImageManager {
    base_path: PathBuf,
    images_dir: PathBuf,
    snapshots_dir: PathBuf,
}

impl ImageManager {
    /// Create new image manager
    pub fn new(base_path: impl AsRef<Path>) -> Self {
        let base = base_path.as_ref().to_path_buf();
        Self {
            images_dir: base.join("images"),
            snapshots_dir: base.join("snapshots"),
            base_path: base,
        }
    }

    /// Initialize the deployment directory structure
    pub async fn init(&self) -> Result<()> {
        async_fs::create_dir_all(&self.images_dir).await?;
        async_fs::create_dir_all(&self.snapshots_dir).await?;

        // Check if we're on BTRFS
        if self.is_btrfs(&self.base_path).await? {
            log::info!("BTRFS filesystem detected - snapshots enabled");
        } else {
            log::warn!("Not on BTRFS - snapshots will be disabled");
        }

        Ok(())
    }

    /// Check if path is on a BTRFS filesystem
    async fn is_btrfs(&self, path: &Path) -> Result<bool> {
        let output = Command::new("findmnt")
            .args(["-n", "-o", "FSTYPE", "-T"])
            .arg(path)
            .output()
            .await
            .context("Failed to check filesystem type")?;

        if output.status.success() {
            let fstype = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(fstype == "btrfs")
        } else {
            Ok(false)
        }
    }

    /// Create a new deployment image
    ///
    /// # Arguments
    /// * `image_name` - Name of the image (e.g., "PROXMOX-DBUS_STAGE")
    /// * `files` - List of files to add to the image
    pub async fn create_image(
        &self,
        image_name: &str,
        files: Vec<PathBuf>,
    ) -> Result<ImageMetadata> {
        log::info!("Creating deployment image: {}", image_name);

        // Get list of existing images (sorted by creation time)
        let existing_images = self.list_images().await?;

        // Create image directory
        let image_path = self.images_dir.join(image_name);
        async_fs::create_dir_all(&image_path).await?;

        let mut image_metadata = ImageMetadata {
            name: image_name.to_string(),
            path: image_path.clone(),
            created: chrono::Utc::now().timestamp(),
            files: Vec::new(),
            total_size: 0,
            unique_size: 0,
            symlinked_size: 0,
        };

        // Process each file
        for file_path in files {
            let file_name = file_path
                .file_name()
                .and_then(|n| n.to_str())
                .context("Invalid file name")?;

            let dest_path = image_path.join(file_name);

            // Check if this file exists in any previous image
            if let Some(previous_file) = self
                .find_file_in_previous_images(file_name, &existing_images)
                .await?
            {
                // File exists in previous image - create symlink
                log::debug!("Symlinking {} from previous image", file_name);

                // Calculate relative path from dest to source
                let relative_target =
                    self.calculate_relative_path(dest_path.parent().unwrap(), &previous_file)?;

                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&relative_target, &dest_path)
                        .context(format!("Failed to create symlink: {}", dest_path.display()))?;
                }

                #[cfg(not(unix))]
                {
                    // On non-Unix, just copy the file
                    async_fs::copy(&previous_file, &dest_path)
                        .await
                        .context(format!("Failed to copy file: {}", dest_path.display()))?;
                }

                let file_size = async_fs::metadata(&previous_file).await?.len();

                image_metadata.files.push(FileEntry {
                    path: dest_path.clone(),
                    is_symlink: true,
                    symlink_target: Some(previous_file),
                    size: file_size,
                    hash: None, // Symlinks don't need hash
                });

                image_metadata.symlinked_size += file_size;
            } else {
                // New file - copy it
                log::debug!("Copying new file: {}", file_name);

                async_fs::copy(&file_path, &dest_path)
                    .await
                    .context(format!("Failed to copy file: {}", file_path.display()))?;

                // Calculate hash for deduplication
                let hash = self.calculate_file_hash(&dest_path).await?;
                let file_size = async_fs::metadata(&dest_path).await?.len();

                image_metadata.files.push(FileEntry {
                    path: dest_path.clone(),
                    is_symlink: false,
                    symlink_target: None,
                    size: file_size,
                    hash: Some(hash),
                });

                image_metadata.unique_size += file_size;
            }

            image_metadata.total_size += image_metadata.files.last().unwrap().size;
        }

        // Save metadata
        let metadata_path = image_path.join(".image-metadata.json");
        let metadata_json = simd_json::to_string_pretty(&image_metadata)?;
        async_fs::write(&metadata_path, metadata_json).await?;

        // Create BTRFS snapshot for streaming
        if self.is_btrfs(&self.base_path).await? {
            self.create_image_snapshot(image_name).await?;
        }

        log::info!(
            "Created image: {} (unique: {} bytes, symlinked: {} bytes)",
            image_name,
            image_metadata.unique_size,
            image_metadata.symlinked_size
        );

        Ok(image_metadata)
    }

    /// Calculate relative path from base to target
    fn calculate_relative_path(&self, base: &Path, target: &Path) -> Result<PathBuf> {
        // Use pathdiff crate's diff_paths if available, otherwise manual calculation
        // For now, use manual calculation that works with non-canonicalized paths

        let base_components: Vec<_> = base.components().collect();
        let target_components: Vec<_> = target.components().collect();

        // Find common prefix length
        let mut common_len = 0;
        let min_len = base_components.len().min(target_components.len());
        for i in 0..min_len {
            if base_components[i] == target_components[i] {
                common_len = i + 1;
            } else {
                break;
            }
        }

        // Build relative path: go up from base, then down to target
        let mut relative = PathBuf::new();

        // Add ".." for each component in base beyond common prefix
        for _ in common_len..base_components.len() {
            relative.push("..");
        }

        // Add remaining components from target
        for comp in target_components.iter().skip(common_len) {
            relative.push(comp);
        }

        Ok(relative)
    }

    /// Find a file in previous images
    /// Returns the path to the actual file (following symlinks if needed)
    async fn find_file_in_previous_images(
        &self,
        file_name: &str,
        existing_images: &[ImageMetadata],
    ) -> Result<Option<PathBuf>> {
        // Search from most recent to oldest
        for image in existing_images.iter().rev() {
            let file_path = image.path.join(file_name);

            // Check if file exists (following symlinks)
            if async_fs::metadata(&file_path).await.is_ok() {
                // Check if it's a symlink
                let symlink_meta = async_fs::symlink_metadata(&file_path).await?;
                if symlink_meta.is_symlink() {
                    // Follow the symlink to find the original file
                    let target = async_fs::read_link(&file_path).await?;
                    let resolved = if target.is_absolute() {
                        target
                    } else {
                        file_path.parent().unwrap().join(&target)
                    };

                    // Check if the resolved path exists and is a real file
                    if async_fs::metadata(&resolved).await.is_ok() {
                        let resolved_meta = async_fs::symlink_metadata(&resolved).await?;
                        if !resolved_meta.is_symlink() {
                            return Ok(Some(resolved));
                        }
                    }
                } else {
                    // It's a real file
                    return Ok(Some(file_path));
                }
            }
        }
        Ok(None)
    }

    /// Calculate SHA256 hash of a file
    async fn calculate_file_hash(&self, file_path: &Path) -> Result<String> {
        let contents = async_fs::read(file_path).await?;
        let mut hasher = Sha256::new();
        hasher.update(&contents);
        let hash = hasher.finalize();
        Ok(format!("{:x}", hash))
    }

    /// Create BTRFS snapshot of an image for streaming
    async fn create_image_snapshot(&self, image_name: &str) -> Result<PathBuf> {
        let image_path = self.images_dir.join(image_name);
        let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
        let snapshot_name = format!("{}-{}", image_name, timestamp);
        let snapshot_path = self.snapshots_dir.join(&snapshot_name);

        log::info!("Creating BTRFS snapshot: {}", snapshot_name);

        let output = Command::new("btrfs")
            .args(["subvolume", "snapshot", "-r"])
            .arg(&image_path)
            .arg(&snapshot_path)
            .output()
            .await
            .context("Failed to create BTRFS snapshot")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("Failed to create snapshot: {}", stderr);
        }

        log::info!("Created snapshot: {}", snapshot_path.display());
        Ok(snapshot_path)
    }

    /// List all deployment images
    pub async fn list_images(&self) -> Result<Vec<ImageMetadata>> {
        let mut images = Vec::new();

        if !self.images_dir.exists() {
            return Ok(images);
        }

        let mut entries = async_fs::read_dir(&self.images_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_dir() {
                let metadata_path = path.join(".image-metadata.json");
                if metadata_path.exists() {
                    let content = async_fs::read_to_string(&metadata_path).await?;
                    if let Ok(metadata) = serde_json::from_str::<ImageMetadata>(&content) {
                        images.push(metadata);
                    }
                }
            }
        }

        // Sort by creation time (oldest first)
        images.sort_by_key(|img| img.created);

        Ok(images)
    }

    /// Get image metadata
    pub async fn get_image(&self, image_name: &str) -> Result<ImageMetadata> {
        let image_path = self.images_dir.join(image_name);
        let metadata_path = image_path.join(".image-metadata.json");

        let content = async_fs::read_to_string(&metadata_path).await?;
        let metadata: ImageMetadata = serde_json::from_str(&content)?;
        Ok(metadata)
    }

    /// Stream an image snapshot for deployment
    /// Returns the path to the snapshot that can be streamed
    pub async fn get_streamable_snapshot(&self, image_name: &str) -> Result<PathBuf> {
        // Find the most recent snapshot for this image
        let snapshot_prefix = image_name;
        let mut latest_snapshot: Option<PathBuf> = None;
        let mut latest_time: i64 = 0;

        if !self.snapshots_dir.exists() {
            anyhow::bail!("No snapshots directory found");
        }

        let mut entries = async_fs::read_dir(&self.snapshots_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

            if name.starts_with(snapshot_prefix) {
                if let Ok(metadata) = entry.metadata().await {
                    if let Ok(created) = metadata.created() {
                        let timestamp = created
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs() as i64;

                        if timestamp > latest_time {
                            latest_time = timestamp;
                            latest_snapshot = Some(path);
                        }
                    }
                }
            }
        }

        latest_snapshot.context(format!("No snapshot found for image: {}", image_name))
    }

    /// Delete an image and its snapshots
    pub async fn delete_image(&self, image_name: &str) -> Result<()> {
        let image_path = self.images_dir.join(image_name);

        // Delete snapshots first
        if self.snapshots_dir.exists() {
            let mut entries = async_fs::read_dir(&self.snapshots_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

                if name.starts_with(image_name) {
                    if self.is_btrfs(&path).await? {
                        let output = Command::new("btrfs")
                            .args(["subvolume", "delete"])
                            .arg(&path)
                            .output()
                            .await?;

                        if !output.status.success() {
                            log::warn!("Failed to delete snapshot: {}", path.display());
                        }
                    } else {
                        async_fs::remove_dir_all(&path).await?;
                    }
                }
            }
        }

        // Delete image directory
        if image_path.exists() {
            if self.is_btrfs(&self.base_path).await? {
                let output = Command::new("btrfs")
                    .args(["subvolume", "delete"])
                    .arg(&image_path)
                    .output()
                    .await?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    anyhow::bail!("Failed to delete image subvolume: {}", stderr);
                }
            } else {
                async_fs::remove_dir_all(&image_path).await?;
            }
        }

        log::info!("Deleted image: {}", image_name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_image_manager_init() {
        let temp_dir = TempDir::new().unwrap();
        let manager = ImageManager::new(temp_dir.path());
        manager.init().await.unwrap();

        assert!(temp_dir.path().join("images").exists());
        assert!(temp_dir.path().join("snapshots").exists());
    }
}
