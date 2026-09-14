//! Local, non-custodial identity vault.
//!
//! Maps a WireGuard pubkey (the user's machine identity) to a sparse user
//! record. The vault is stored in `/etc/ghostbridge/user-vault.json` with
//! 0600 permissions. It is the only server-side record we keep: no private
//! keys, no recovery seeds, no OAuth refresh tokens — just the pubkey and
//! enough metadata to recognize a re-derived identity.
//!
//! Per AGENTS.md: no SQLite, no Btrfs mutation loops, D-Bus-first control plane.
//! This file is read/written by the `session` module only; external tools go
//! through the SessionManager D-Bus projection.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use tracing::info;

pub const DEFAULT_VAULT_PATH: &str = "/etc/ghostbridge/user-vault.json";

/// Sparse user record stored in the vault.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRecord {
    pub pubkey: String,
    pub user_email: String,
    pub allowed_ip: String,
    pub created_at: DateTime<Utc>,
    pub provisioned_at: DateTime<Utc>,
}

/// On-disk layout for the vault file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct VaultFile {
    version: u32,
    users: HashMap<String, UserRecord>,
}

/// Identity vault: pubkey → user record.
#[derive(Debug, Clone)]
pub struct IdentityVault {
    path: PathBuf,
    users: HashMap<String, UserRecord>,
}

impl IdentityVault {
    /// Load the vault from the default system path.
    pub fn load() -> Result<Self> {
        Self::load_from(DEFAULT_VAULT_PATH)
    }

    /// Load the vault from an explicit path, or start empty if it does not exist.
    pub fn load_from<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Ok(Self::new(path));
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read identity vault: {}", path.display()))?;
        let file: VaultFile =
            serde_json::from_str(&raw).with_context(|| "failed to parse identity vault as JSON")?;
        Ok(Self {
            path,
            users: file.users,
        })
    }

    /// Create an empty vault at the given path.
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            users: HashMap::new(),
        }
    }

    /// Store a user record, overwriting any existing record for the pubkey.
    pub fn store(&mut self, record: UserRecord) -> Result<()> {
        self.users.insert(record.pubkey.clone(), record);
        self.flush()
    }

    /// Get a user record by pubkey.
    pub fn get(&self, pubkey: &str) -> Option<&UserRecord> {
        self.users.get(pubkey)
    }

    /// Remove a user record.
    pub fn remove(&mut self, pubkey: &str) -> Result<()> {
        self.users.remove(pubkey);
        self.flush()
    }

    /// Persist the vault to disk atomically with owner-only permissions.
    fn flush(&self) -> Result<()> {
        let file = VaultFile {
            version: 1,
            users: self.users.clone(),
        };
        let json = serde_json::to_string_pretty(&file).context("serialize identity vault")?;

        let tmp = self.path.with_extension("tmp");
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .with_context(|| format!("open temp vault file: {}", tmp.display()))?;
        f.write_all(json.as_bytes())
            .with_context(|| format!("write temp vault file: {}", tmp.display()))?;
        f.sync_data()
            .with_context(|| format!("sync temp vault file: {}", tmp.display()))?;
        drop(f);
        fs::rename(&tmp, &self.path)
            .with_context(|| format!("rename temp vault to: {}", self.path.display()))?;
        info!("Persisted identity vault with {} records", self.users.len());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn round_trip_user_record() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.json");
        let record = UserRecord {
            pubkey: "pubkey1".to_string(),
            user_email: "user@example.com".to_string(),
            allowed_ip: "10.0.0.1".to_string(),
            created_at: Utc::now(),
            provisioned_at: Utc::now(),
        };
        let mut vault = IdentityVault::load_from(&path).unwrap();
        vault.store(record).unwrap();

        let loaded = IdentityVault::load_from(&path).unwrap();
        assert_eq!(
            loaded.get("pubkey1").unwrap().user_email,
            "user@example.com"
        );
    }
}
