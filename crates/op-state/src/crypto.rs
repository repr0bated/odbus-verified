#![allow(dead_code)]
#![allow(deprecated)]
use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit, OsRng},
};
use anyhow::{Context, Result, bail};
use argon2::Argon2;
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use std::path::Path;

const NONCE_SIZE: usize = 12;
const KEY_SIZE: usize = 32;
const PASSWORD_SALT_SIZE: usize = 16;

/// Encrypted state file structure
#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptedState {
    /// Base64 encoded nonce
    pub nonce: String,
    /// Base64 encoded salt (for password-derived keys)
    pub salt: Option<String>,
    /// Base64 encoded encrypted data
    pub ciphertext: String,
    /// Encryption version for future compatibility
    pub version: u8,
}

/// State encryption manager
pub struct StateEncryption {
    key: Key<Aes256Gcm>,
    password_salt: Option<String>,
}

impl StateEncryption {
    /// Create a new encryption manager with a random key
    pub fn new() -> Result<Self> {
        let key = Aes256Gcm::generate_key(&mut OsRng);
        Ok(Self {
            key,
            password_salt: None,
        })
    }

    /// Create encryption manager from a password
    pub fn from_password(password: &str) -> Result<Self> {
        let mut salt = [0u8; PASSWORD_SALT_SIZE];
        use rand::RngCore;
        OsRng.fill_bytes(&mut salt);
        Self::from_password_salt_bytes(password, &salt)
    }

    /// Recreate a password-derived encryption manager from a persisted salt.
    pub fn from_password_and_salt(password: &str, encoded_salt: &str) -> Result<Self> {
        let salt = BASE64
            .decode(encoded_salt)
            .context("Failed to decode password salt")?;
        if salt.len() != PASSWORD_SALT_SIZE {
            bail!(
                "Invalid password salt size: expected {} bytes, got {}",
                PASSWORD_SALT_SIZE,
                salt.len()
            );
        }
        Self::from_password_salt_bytes(password, &salt)
    }

    /// Recreate a password-derived encryption manager for an encrypted payload.
    pub fn from_password_for(password: &str, encrypted: &EncryptedState) -> Result<Self> {
        let salt = encrypted
            .salt
            .as_deref()
            .context("Encrypted state does not contain a password salt")?;
        Self::from_password_and_salt(password, salt)
    }

    fn from_password_salt_bytes(password: &str, salt: &[u8]) -> Result<Self> {
        if password.is_empty() {
            bail!("Password must not be empty");
        }
        let argon2 = Argon2::default();

        let mut key_bytes = [0u8; KEY_SIZE];
        argon2
            .hash_password_into(password.as_bytes(), salt, &mut key_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to derive key: {}", e))?;

        let key = *Key::<Aes256Gcm>::from_slice(&key_bytes);
        Ok(Self {
            key,
            password_salt: Some(BASE64.encode(salt)),
        })
    }

    /// Create encryption manager from existing key
    pub fn from_key(key_bytes: &[u8]) -> Result<Self> {
        if key_bytes.len() != KEY_SIZE {
            bail!(
                "Invalid key size: expected {} bytes, got {}",
                KEY_SIZE,
                key_bytes.len()
            );
        }

        let key = *Key::<Aes256Gcm>::from_slice(key_bytes);
        Ok(Self {
            key,
            password_salt: None,
        })
    }

    /// Load or generate key from file
    pub fn from_key_file(path: &Path) -> Result<Self> {
        if path.exists() {
            // Load existing key
            let key_data = std::fs::read(path).context("Failed to read key file")?;

            if key_data.len() != KEY_SIZE {
                bail!(
                    "Invalid key file: expected {} bytes, got {}",
                    KEY_SIZE,
                    key_data.len()
                );
            }

            Self::from_key(&key_data)
        } else {
            // Generate new key and save it
            let encryption = Self::new()?;

            // Ensure parent directory exists
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).context("Failed to create key directory")?;
            }

            // Save key with restricted permissions
            std::fs::write(path, encryption.key.as_slice()).context("Failed to write key file")?;

            // Set permissions to 600 (owner read/write only)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(path)?.permissions();
                perms.set_mode(0o600);
                std::fs::set_permissions(path, perms)
                    .context("Failed to set key file permissions")?;
            }

            Ok(encryption)
        }
    }

    /// Encrypt data
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedState> {
        let cipher = Aes256Gcm::new(&self.key);

        // Generate random nonce
        let mut nonce_bytes = [0u8; NONCE_SIZE];
        use rand::RngCore;
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt
        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

        Ok(EncryptedState {
            nonce: BASE64.encode(nonce_bytes),
            salt: self.password_salt.clone(),
            ciphertext: BASE64.encode(ciphertext),
            version: 1,
        })
    }

    /// Decrypt data
    pub fn decrypt(&self, encrypted: &EncryptedState) -> Result<Vec<u8>> {
        if encrypted.version != 1 {
            bail!("Unsupported encryption version: {}", encrypted.version);
        }

        let nonce_bytes = BASE64
            .decode(&encrypted.nonce)
            .context("Failed to decode nonce")?;

        if nonce_bytes.len() != NONCE_SIZE {
            bail!("Invalid nonce size");
        }

        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = BASE64
            .decode(&encrypted.ciphertext)
            .context("Failed to decode ciphertext")?;

        let cipher = Aes256Gcm::new(&self.key);
        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

        Ok(plaintext)
    }

    /// Encrypt JSON data
    pub fn encrypt_json<T: serde::Serialize>(&self, data: &T) -> Result<EncryptedState> {
        let json = simd_json::to_vec(data).context("Failed to serialize data")?;
        self.encrypt(&json)
    }

    pub fn decrypt_json<T: serde::de::DeserializeOwned>(
        &self,
        encrypted: &EncryptedState,
    ) -> Result<T> {
        let plaintext = self.decrypt(encrypted)?;
        let mut plaintext_mut = plaintext;
        simd_json::from_slice(&mut plaintext_mut).context("Failed to deserialize decrypted data")
    }
}

/// Helper functions for state files  
pub mod state_file {
    use super::*;
    use crate::DesiredState as State;

    /// Save state with encryption
    pub fn save_encrypted(state: &State, path: &Path, encryption: &StateEncryption) -> Result<()> {
        let encrypted = encryption.encrypt_json(state)?;

        let json = simd_json::to_string_pretty(&encrypted)
            .context("Failed to serialize encrypted state")?;

        // Write to temporary file first
        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, json).context("Failed to write encrypted state")?;

        // Atomic rename
        std::fs::rename(&tmp_path, path).context("Failed to rename state file")?;

        Ok(())
    }

    /// Load state with decryption
    pub fn load_encrypted(path: &Path, encryption: &StateEncryption) -> Result<State> {
        let mut contents = std::fs::read_to_string(path).context("Failed to read state file")?;

        let encrypted: EncryptedState = unsafe { simd_json::from_str(&mut contents) }
            .context("Failed to parse encrypted state")?;

        encryption.decrypt_json(&encrypted)
    }

    /// Load state encrypted with a password-derived key. The salt is read from
    /// the envelope so the key can be recreated after a process restart.
    pub fn load_encrypted_with_password(path: &Path, password: &str) -> Result<State> {
        let mut contents = std::fs::read_to_string(path).context("Failed to read state file")?;

        let encrypted: EncryptedState = unsafe { simd_json::from_str(&mut contents) }
            .context("Failed to parse encrypted state")?;
        let encryption = StateEncryption::from_password_for(password, &encrypted)?;

        encryption.decrypt_json(&encrypted)
    }

    /// Check if a state file is encrypted
    pub fn is_encrypted(path: &Path) -> Result<bool> {
        let contents = std::fs::read_to_string(path).context("Failed to read state file")?;

        // Try to parse as encrypted state
        let mut c1 = contents.clone();
        if unsafe { simd_json::from_str::<EncryptedState>(&mut c1) }.is_ok() {
            return Ok(true);
        }

        // Try to parse as plain state
        let mut c2 = contents;
        if unsafe { simd_json::from_str::<State>(&mut c2) }.is_ok() {
            return Ok(false);
        }

        bail!("Unknown state file format");
    }

    /// Migrate unencrypted state to encrypted
    pub fn migrate_to_encrypted(path: &Path, encryption: &StateEncryption) -> Result<()> {
        if is_encrypted(path)? {
            return Ok(()); // Already encrypted
        }

        // Read plain state
        let mut contents = std::fs::read_to_string(path).context("Failed to read state file")?;

        let state: State =
            unsafe { simd_json::from_str(&mut contents) }.context("Failed to parse state")?;

        // Backup original
        let backup_path = path.with_extension("bak");
        std::fs::copy(path, &backup_path).context("Failed to create backup")?;

        // Save encrypted
        save_encrypted(&state, path, encryption)?;

        println!("Successfully migrated state to encrypted format");
        println!("Backup saved to: {:?}", backup_path);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_roundtrip() {
        let encryption = StateEncryption::new().unwrap();

        let plaintext = b"Hello, World!";
        let encrypted = encryption.encrypt(plaintext).unwrap();
        let decrypted = encryption.decrypt(&encrypted).unwrap();

        assert_eq!(plaintext.to_vec(), decrypted);
    }

    #[test]
    fn test_json_encryption() {
        use simd_json::json;

        let encryption = StateEncryption::new().unwrap();

        let data = json!({
            "key": "value",
            "number": 42,
            "nested": {
                "array": [1, 2, 3]
            }
        });

        let encrypted = encryption.encrypt_json(&data).unwrap();
        let decrypted: simd_json::OwnedValue = encryption.decrypt_json(&encrypted).unwrap();

        assert_eq!(data, decrypted);
    }

    #[test]
    fn test_password_derivation() {
        let password = "test_password_123";
        let encryption1 = StateEncryption::from_password(password).unwrap();

        let plaintext = b"Secret data";
        let encrypted = encryption1.encrypt(plaintext).unwrap();

        assert!(encrypted.salt.is_some());

        // A fresh process can derive the same key from the persisted salt.
        let encryption2 = StateEncryption::from_password_for(password, &encrypted).unwrap();
        let result = encryption2.decrypt(&encrypted).unwrap();
        assert_eq!(plaintext.to_vec(), result);

        let wrong_password = StateEncryption::from_password_for("wrong", &encrypted).unwrap();
        assert!(wrong_password.decrypt(&encrypted).is_err());
        assert!(StateEncryption::from_password("").is_err());
    }
}
