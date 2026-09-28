//! Encrypted storage for WireGuard keys using Btrfs subvolumes
//!
//! This module provides secure storage for WireGuard private keys and session data
//! using encrypted Btrfs subvolumes with native encryption (experimental) or LUKS.

use chacha20poly1305::{AeadInPlace, ChaCha20Poly1305, Key, KeyInit, Nonce};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use tokio::fs as async_fs;
use tracing::{debug, info};
use zeroize::{Zeroize, ZeroizeOnDrop};

use anyhow::Result;

/// Configuration for encrypted storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedStorageConfig {
    /// Base path for encrypted storage
    pub base_path: PathBuf,
    /// Subvolume name for WireGuard keys
    pub subvolume_name: String,
    /// Use native Btrfs encryption (experimental)
    pub use_native_encryption: bool,
    /// LUKS device name (if not using native encryption)
    pub luks_device_name: Option<String>,
    /// Key derivation parameters
    pub kdf_params: KdfParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KdfParams {
    pub memory_cost: u32,
    pub time_cost: u32,
    pub parallelism: u32,
    pub salt_length: usize,
}

/// Encrypted key storage manager
pub struct EncryptedKeyStorage {
    config: EncryptedStorageConfig,
    storage_path: PathBuf,
    is_initialized: bool,
    master_key: Option<MasterKey>,
}

/// Master key for encryption/decryption
#[derive(Zeroize, ZeroizeOnDrop)]
struct MasterKey {
    key: [u8; 32],
    salt: [u8; 32],
    nonce_counter: u64,
}

/// Encrypted key entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedKeyEntry {
    pub key_id: String,
    pub encrypted_data: Vec<u8>,
    pub nonce: [u8; 12],
    pub created_at: u64,
    pub key_type: KeyType,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyType {
    WireGuardPrivate,
    WireGuardPsk,
    SessionKey,
    MasterKey,
}

impl Default for EncryptedStorageConfig {
    fn default() -> Self {
        Self {
            base_path: PathBuf::from("/var/lib/op-dbus/encrypted"),
            subvolume_name: "wireguard-keys".to_string(),
            use_native_encryption: true, // Use experimental Btrfs encryption
            luks_device_name: Some("opdbus_wg_keys".to_string()),
            kdf_params: KdfParams {
                memory_cost: 65536, // 64 MB
                time_cost: 3,
                parallelism: 4,
                salt_length: 32,
            },
        }
    }
}

impl EncryptedKeyStorage {
    /// Helper to get storage path as string with proper error handling
    fn storage_path_str(&self) -> anyhow::Result<&str> {
        self.storage_path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Storage path contains invalid UTF-8"))
    }

    /// Create new encrypted key storage
    #[tracing::instrument]
    pub async fn new(config: EncryptedStorageConfig) -> Result<Self> {
        info!(
            "Initializing encrypted key storage at {:?}",
            config.base_path
        );

        let storage_path = config.base_path.join(&config.subvolume_name);

        let mut storage = Self {
            config,
            storage_path,
            is_initialized: false,
            master_key: None,
        };

        // Initialize storage
        storage.initialize().await?;

        Ok(storage)
    }

    /// Initialize encrypted storage with Btrfs subvolume
    async fn initialize(&mut self) -> Result<()> {
        info!("Setting up encrypted Btrfs subvolume for WireGuard keys");

        // Ensure base directory exists
        async_fs::create_dir_all(&self.config.base_path).await?;

        if self.config.use_native_encryption {
            self.setup_native_btrfs_encryption().await?;
        } else {
            self.setup_luks_encryption().await?;
        }

        // Load or generate master key
        self.load_or_generate_master_key().await?;

        self.is_initialized = true;
        info!("Encrypted key storage initialized successfully");
        Ok(())
    }

    /// Setup native Btrfs encryption (experimental)
    async fn setup_native_btrfs_encryption(&self) -> Result<()> {
        info!(
            "Setting up native Btrfs encryption for subvolume: {}",
            self.config.subvolume_name
        );

        // A directory or unencrypted subvolume cannot be treated as encrypted
        // merely because it exists. There is no reliable native-encryption
        // attestation for this legacy backend on reopen.
        if self.storage_path.exists() {
            anyhow::bail!(
                "Cannot verify native encryption of existing key storage {}; use a verified LUKS mapper",
                self.storage_path.display()
            );
        }

        // Create encrypted subvolume using btrfs command
        // Note: This requires kernel support for Btrfs encryption
        let path_str = self.storage_path_str()?;
        let output = Command::new("btrfs")
            .args([
                "subvolume",
                "create",
                "-e", // Enable encryption (experimental)
                path_str,
            ])
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to execute btrfs command: {}", e))?;

        anyhow::ensure!(
            output.status.success(),
            "Native Btrfs encryption is unavailable; refusing plaintext key storage: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
        info!("Created encrypted Btrfs subvolume: {:?}", self.storage_path);

        // Set restrictive permissions
        self.set_secure_permissions().await?;

        Ok(())
    }

    /// Setup LUKS encryption as fallback
    async fn setup_luks_encryption(&self) -> Result<()> {
        let device_name = self
            .config
            .luks_device_name
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("LUKS device name required"))?;

        anyhow::ensure!(
            !device_name.is_empty()
                && device_name.len() <= 128
                && !matches!(device_name.as_str(), "." | "..")
                && device_name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')),
            "LUKS mapper name must be one safe path component"
        );

        info!("Setting up LUKS encryption for device: {}", device_name);

        // Check if LUKS device already exists
        let luks_path = format!("/dev/mapper/{}", device_name);
        anyhow::ensure!(
            Path::new(&luks_path).exists(),
            "LUKS mapper {} is absent; provision and open it before key storage starts",
            luks_path
        );
        debug!("LUKS device exists: {}", luks_path);

        if !self.storage_path.exists() {
            self.mount_luks_device(&luks_path).await?;
        }
        self.verify_luks_mount(device_name, &luks_path).await?;
        Ok(())
    }

    /// Require the exact mountpoint to use the configured mapper. Comparing
    /// block-device identity accepts /dev/mapper aliases and /dev/dm-N alike.
    async fn verify_luks_mount(&self, mapper_name: &str, mapper: &str) -> Result<()> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt};

        let directory = async_fs::symlink_metadata(&self.storage_path).await?;
        anyhow::ensure!(
            directory.is_dir() && !directory.file_type().is_symlink(),
            "LUKS storage path is not a real directory"
        );
        let mapper_device = std::fs::metadata(mapper)?;
        anyhow::ensure!(
            mapper_device.file_type().is_block_device(),
            "LUKS mapper is not a block device"
        );

        // A mapper name or matching block device alone is not proof of
        // encryption: dm-linear and plain dm-crypt devices also live here.
        let crypt = Command::new("cryptsetup")
            .arg("status")
            .arg(mapper_name)
            .output()?;
        anyhow::ensure!(
            crypt.status.success() && Self::status_is_luks(&crypt.stdout),
            "Configured mapper {} is not an active verified LUKS device: {}",
            mapper,
            String::from_utf8_lossy(&crypt.stderr).trim()
        );

        let output = Command::new("findmnt")
            .args(["-n", "-e", "-v", "-M"])
            .arg(&self.storage_path)
            .args(["-o", "SOURCE"])
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "LUKS storage path {} is not a mountpoint: {}",
            self.storage_path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        let source = String::from_utf8(output.stdout)?;
        let source_device = std::fs::metadata(source.trim())?;
        anyhow::ensure!(
            source_device.file_type().is_block_device()
                && source_device.rdev() == mapper_device.rdev(),
            "LUKS storage mount is not backed by configured mapper {}",
            mapper
        );
        Ok(())
    }

    fn status_is_luks(status: &[u8]) -> bool {
        let Ok(status) = std::str::from_utf8(status) else {
            return false;
        };
        status.lines().any(|line| {
            let value = line.trim().strip_prefix("type:").map(str::trim);
            matches!(value, Some("LUKS1" | "LUKS2"))
        })
    }

    /// Mount LUKS device
    async fn mount_luks_device(&self, device_path: &str) -> Result<()> {
        async_fs::create_dir_all(&self.storage_path).await?;

        let path_str = self.storage_path_str()?;
        let output = Command::new("mount")
            .args([device_path, path_str])
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to mount LUKS device: {}", e))?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "Failed to mount LUKS device: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        info!("Mounted LUKS device at {:?}", self.storage_path);
        Ok(())
    }

    /// Set secure permissions on storage directory
    async fn set_secure_permissions(&self) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        // Set permissions to 700 (owner read/write/execute only)
        let mut perms = async_fs::metadata(&self.storage_path).await?.permissions();
        perms.set_mode(0o700);
        async_fs::set_permissions(&self.storage_path, perms).await?;

        debug!("Set secure permissions on {:?}", self.storage_path);
        Ok(())
    }

    /// Load or generate master key
    async fn load_or_generate_master_key(&mut self) -> Result<()> {
        let master_key_path = self.storage_path.join("master.key");

        if master_key_path.exists() {
            debug!("Loading existing master key");
            self.load_master_key(&master_key_path).await?;
        } else {
            info!("Generating new master key");
            self.generate_master_key(&master_key_path).await?;
        }

        Ok(())
    }

    /// Load master key from file
    async fn load_master_key(&mut self, path: &Path) -> Result<()> {
        let encrypted_data = async_fs::read(path).await?;

        // For now, use a simple key derivation
        // In production, this would use proper key derivation with user passphrase
        let mut key = [0u8; 32];
        let mut salt = [0u8; 32];

        if encrypted_data.len() >= 64 {
            key.copy_from_slice(&encrypted_data[0..32]);
            salt.copy_from_slice(&encrypted_data[32..64]);
        } else {
            return Err(anyhow::anyhow!("Invalid master key file"));
        }

        self.master_key = Some(MasterKey {
            key,
            salt,
            nonce_counter: 0,
        });

        debug!("Master key loaded successfully");
        Ok(())
    }

    /// Generate new master key
    async fn generate_master_key(&mut self, path: &Path) -> Result<()> {
        let rng = SystemRandom::new();

        let mut key = [0u8; 32];
        let mut salt = [0u8; 32];

        rng.fill(&mut key)
            .map_err(|_| anyhow::anyhow!("Failed to generate key"))?;
        rng.fill(&mut salt)
            .map_err(|_| anyhow::anyhow!("Failed to generate salt"))?;

        // Store encrypted key (in production, encrypt with user passphrase)
        let mut key_data = Vec::with_capacity(64);
        key_data.extend_from_slice(&key);
        key_data.extend_from_slice(&salt);

        async_fs::write(path, &key_data).await?;

        // Set restrictive permissions on key file
        use std::os::unix::fs::PermissionsExt;
        let mut perms = async_fs::metadata(path).await?.permissions();
        perms.set_mode(0o600);
        async_fs::set_permissions(path, perms).await?;

        self.master_key = Some(MasterKey {
            key,
            salt,
            nonce_counter: 0,
        });

        info!("Generated and stored new master key");
        Ok(())
    }

    /// Store encrypted key
    #[tracing::instrument(skip(self))]
    pub async fn store_key(
        &mut self,
        key_id: &str,
        key_data: &[u8],
        key_type: KeyType,
    ) -> anyhow::Result<()> {
        if !self.is_initialized {
            return Err(anyhow::anyhow!("Storage not initialized"));
        }

        let master_key = self
            .master_key
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Master key not available"))?;

        // Generate nonce
        let mut nonce = [0u8; 12];
        let nonce_counter = master_key.nonce_counter;
        nonce[4..12].copy_from_slice(&nonce_counter.to_le_bytes());
        master_key.nonce_counter += 1;

        // Encrypt the key data
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&master_key.key));
        let mut encrypted_data = key_data.to_vec();
        encrypted_data.reserve(16); // Reserve space for authentication tag

        cipher
            .encrypt_in_place(Nonce::from_slice(&nonce), b"", &mut encrypted_data)
            .map_err(|_| anyhow::anyhow!("Encryption failed"))?;

        let entry = EncryptedKeyEntry {
            key_id: key_id.to_string(),
            encrypted_data,
            nonce,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            key_type,
            metadata: std::collections::HashMap::new(),
        };

        // Store to file
        let key_file_path = self.storage_path.join(format!("{}.key", key_id));
        let entry_json = simd_json::to_string(&entry)?;
        async_fs::write(&key_file_path, entry_json).await?;

        // Set secure permissions
        use std::os::unix::fs::PermissionsExt;
        let mut perms = async_fs::metadata(&key_file_path).await?.permissions();
        perms.set_mode(0o600);
        async_fs::set_permissions(&key_file_path, perms).await?;

        debug!("Stored encrypted key: {}", key_id);
        Ok(())
    }

    /// Retrieve and decrypt key
    #[tracing::instrument(skip(self))]
    pub async fn retrieve_key(&self, key_id: &str) -> anyhow::Result<Vec<u8>> {
        if !self.is_initialized {
            return Err(anyhow::anyhow!("Storage not initialized"));
        }

        let master_key = self
            .master_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Master key not available"))?;

        let key_file_path = self.storage_path.join(format!("{}.key", key_id));

        if !key_file_path.exists() {
            return Err(anyhow::anyhow!("Key not found: {}", key_id));
        }

        let entry_json = async_fs::read_to_string(&key_file_path).await?;
        let mut entry_bytes = entry_json.into_bytes();
        let entry: EncryptedKeyEntry = simd_json::from_slice(&mut entry_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize key entry: {}", e))?;

        // Decrypt the key data
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&master_key.key));
        let mut decrypted_data = entry.encrypted_data.clone();

        cipher
            .decrypt_in_place(Nonce::from_slice(&entry.nonce), b"", &mut decrypted_data)
            .map_err(|_| anyhow::anyhow!("Decryption failed"))?;

        debug!("Retrieved and decrypted key: {}", key_id);
        Ok(decrypted_data)
    }

    /// List all stored keys
    #[tracing::instrument(skip(self))]
    pub async fn list_keys(&self) -> anyhow::Result<Vec<String>> {
        if !self.is_initialized {
            return Err(anyhow::anyhow!("Storage not initialized"));
        }

        let mut keys = Vec::new();
        let mut entries = async_fs::read_dir(&self.storage_path).await?;

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if let Some(extension) = path.extension()
                && extension == "key"
                && let Some(stem) = path.file_stem()
                && let Some(key_id) = stem.to_str()
            {
                keys.push(key_id.to_string());
            }
        }

        Ok(keys)
    }

    /// Delete a key
    #[tracing::instrument(skip(self))]
    pub async fn delete_key(&self, key_id: &str) -> anyhow::Result<()> {
        if !self.is_initialized {
            return Err(anyhow::anyhow!("Storage not initialized"));
        }

        let key_file_path = self.storage_path.join(format!("{}.key", key_id));

        if key_file_path.exists() {
            async_fs::remove_file(&key_file_path).await?;
            debug!("Deleted key: {}", key_id);
        }

        Ok(())
    }

    /// Get storage statistics
    #[tracing::instrument(skip(self))]
    pub async fn get_stats(&self) -> anyhow::Result<StorageStats> {
        if !self.is_initialized {
            return Err(anyhow::anyhow!("Storage not initialized"));
        }

        let keys = self.list_keys().await?;
        let _metadata = async_fs::metadata(&self.storage_path).await?;

        // Get filesystem info
        let fs_info = self.get_filesystem_info().await?;

        Ok(StorageStats {
            total_keys: keys.len(),
            storage_path: self.storage_path.clone(),
            is_encrypted: self.config.use_native_encryption
                || self.config.luks_device_name.is_some(),
            encryption_type: if self.config.use_native_encryption {
                "btrfs-native".to_string()
            } else {
                "luks".to_string()
            },
            filesystem_type: fs_info.filesystem_type,
            total_space: fs_info.total_space,
            available_space: fs_info.available_space,
            used_space: fs_info.used_space,
        })
    }

    /// Get filesystem information
    async fn get_filesystem_info(&self) -> anyhow::Result<FilesystemInfo> {
        anyhow::bail!("Storage capacity is unavailable through this legacy gateway; request filesystem_usage from the sealed btrfs plugin through the canonical door")
    }
}

/// Storage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageStats {
    pub total_keys: usize,
    pub storage_path: PathBuf,
    pub is_encrypted: bool,
    pub encryption_type: String,
    pub filesystem_type: String,
    pub total_space: u64,
    pub available_space: u64,
    pub used_space: u64,
}

/// Filesystem information
#[derive(Debug, Clone)]
struct FilesystemInfo {
    pub filesystem_type: String,
    pub total_space: u64,
    pub used_space: u64,
    pub available_space: u64,
}

impl Default for FilesystemInfo {
    fn default() -> Self {
        Self {
            filesystem_type: "unknown".to_string(),
            total_space: 0,
            used_space: 0,
            available_space: 0,
        }
    }
}

impl Drop for EncryptedKeyStorage {
    fn drop(&mut self) {
        // Zeroize master key on drop
        if let Some(mut master_key) = self.master_key.take() {
            master_key.zeroize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_luks_cryptsetup_status_is_accepted() {
        assert!(EncryptedKeyStorage::status_is_luks(
            b"opdbus is active\n  type: LUKS2\n  cipher: aes-xts-plain64\n"
        ));
        assert!(EncryptedKeyStorage::status_is_luks(b"type: LUKS1\n"));
        assert!(!EncryptedKeyStorage::status_is_luks(b"type: PLAIN\n"));
        assert!(!EncryptedKeyStorage::status_is_luks(b"type: LUKS2x\n"));
    }

    #[tokio::test]
    async fn absent_luks_mapper_never_creates_plaintext_container_or_key() {
        let unique = format!(
            "opdbus-absent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let base = std::env::temp_dir().join(&unique);
        let config = EncryptedStorageConfig {
            base_path: base.clone(),
            subvolume_name: "wireguard-keys".to_string(),
            use_native_encryption: false,
            luks_device_name: Some(unique),
            ..EncryptedStorageConfig::default()
        };
        let error = EncryptedKeyStorage::new(config).await.err().unwrap();
        assert!(error.to_string().contains("is absent"));
        assert!(!base.join("wireguard_keys.img").exists());
        assert!(!base.join("wireguard-keys/master.key").exists());
        std::fs::remove_dir_all(base).unwrap();
    }

    #[tokio::test]
    async fn existing_plain_directory_cannot_claim_native_encryption() {
        let unique = format!(
            "opdbus-native-unverified-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let base = std::env::temp_dir().join(unique);
        let storage = base.join("wireguard-keys");
        std::fs::create_dir_all(&storage).unwrap();
        let config = EncryptedStorageConfig {
            base_path: base.clone(),
            ..EncryptedStorageConfig::default()
        };
        let error = EncryptedKeyStorage::new(config).await.err().unwrap();
        assert!(error
            .to_string()
            .contains("Cannot verify native encryption"));
        assert!(!storage.join("master.key").exists());
        std::fs::remove_dir_all(base).unwrap();
    }
}
