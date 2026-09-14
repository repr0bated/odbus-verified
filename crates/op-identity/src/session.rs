//! Session management using WireGuard pubkey as identity.
//!
//! Sessions are created when a WireGuard peer connects and
//! destroyed on disconnect or timeout.

use anyhow::Context;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::gcloud_auth::GCloudAuth;
use crate::identity_vault::{IdentityVault, UserRecord};
use crate::wireguard::WireGuardIdentity;

const SESSION_TIMEOUT_SECS: i64 = 3600; // 1 hour

/// Domain-separation context for deriving a deterministic session id from a
/// WireGuard pubkey. The same pubkey always yields the same session id, so
/// re-authentication after a timeout resumes the same logical session without
/// server-side custody of the private key.
const SESSION_ID_KDF_CONTEXT: &str = "op-identity session-id v1";

/// Server-side proof stored in Cozo; the raw PSK is never persisted.
pub fn session_proof(session_id: &str) -> String {
    blake3::hash(session_id.as_bytes()).to_hex().to_string()
}

pub fn verify_session_proof(session_id: &str, stored_proof: &str) -> bool {
    session_proof(session_id) == stored_proof
}

/// Derive a stable session id from a WireGuard pubkey using Blake3 KDF.
///
/// Output is formatted as a UUID string so existing consumers (headers,
/// trace IDs, accountability page) do not need to change. The derivation is
/// deterministic and local-only: no network, no DB, no randomness.
pub fn derive_session_id(pubkey: &str) -> String {
    let key_material = blake3::derive_key(SESSION_ID_KDF_CONTEXT, pubkey.as_bytes());
    let bytes: [u8; 16] = key_material[..16]
        .try_into()
        .expect("16 bytes from 32-byte KDF output");
    Uuid::from_bytes(bytes).to_string()
}

/// Domain-separation context for deriving a deterministic human principal id
/// from a WireGuard pubkey. DISTINCT from `SESSION_ID_KDF_CONTEXT`, so a
/// human principal id can never collide with a container session id.
const PRINCIPAL_ID_KDF_CONTEXT: &str = "op-identity human-principal v1";

/// Derive a stable human principal id from a WireGuard pubkey using Blake3
/// KDF.
///
/// Same recipe as [`derive_session_id`] (first 16 bytes of the KDF output as
/// a raw UUID, no version/variant munging) but under the distinct
/// `PRINCIPAL_ID_KDF_CONTEXT`, separating the two namespaces by construction.
/// Deterministic and local-only: no network, no DB, no randomness.
pub fn derive_principal_id(pubkey: &str) -> String {
    let key_material = blake3::derive_key(PRINCIPAL_ID_KDF_CONTEXT, pubkey.as_bytes());
    let bytes: [u8; 16] = key_material[..16]
        .try_into()
        .expect("16 bytes from 32-byte KDF output");
    Uuid::from_bytes(bytes).to_string()
}

/// Represents an active session
#[derive(Debug, Clone)]
pub struct Session {
    pub session_id: String,
    pub pubkey: String,
    pub user_email: Option<String>,
    pub oauth_token: Option<String>,
    pub token_expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

/// Manages sessions and their lifecycle
#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<DashMap<String, Session>>,
    identity_vault: Arc<Mutex<IdentityVault>>,
    gcloud_auth: GCloudAuth,
    wireguard: WireGuardIdentity,
    current_session_id: Arc<Mutex<Option<String>>>,
}

impl SessionManager {
    pub fn new() -> anyhow::Result<Self> {
        Self::with_vault_path(crate::identity_vault::DEFAULT_VAULT_PATH)
    }

    /// Backward-compatible constructor for explicit interface-based identity wiring.
    ///
    /// Deprecated: WG-interface-based identity is deprecated in favor of explicit
    /// pubkey injection (WG_PUBKEY / signed metadata).
    pub fn with_wireguard_interface(interface: &str) -> anyhow::Result<Self> {
        Self::with_vault_and_interface(crate::identity_vault::DEFAULT_VAULT_PATH, interface)
    }

    pub fn with_vault_path<P: AsRef<std::path::Path>>(path: P) -> anyhow::Result<Self> {
        // Keeping this overload for compatibility, but do not assume a WG interface.
        // WG interface defaults are deprecated; identity should be injected via pubkey.
        Self::with_vault_and_interface(path, "")
    }

    pub fn with_vault_and_interface<P: AsRef<std::path::Path>>(
        path: P,
        interface: &str,
    ) -> anyhow::Result<Self> {
        // SQL is obsolete in the 3tched architecture.
        // We use in-memory DashMaps to prevent Btrfs mutation loops; the vault
        // is the only persistent record and is stored under /etc/ghostbridge.
        let path = path.as_ref().to_path_buf();
        let vault = IdentityVault::load_from(&path).unwrap_or_else(|e| {
            tracing::warn!("failed to load identity vault, starting empty: {e}");
            IdentityVault::new(&path)
        });
        Ok(Self {
            sessions: Arc::new(DashMap::new()),
            identity_vault: Arc::new(Mutex::new(vault)),
            gcloud_auth: GCloudAuth::new(),
            wireguard: WireGuardIdentity::with_interface(interface),
            current_session_id: Arc::new(Mutex::new(None)),
        })
    }

    /// Get the GCloud auth provider
    pub fn gcloud_auth(&self) -> &GCloudAuth {
        &self.gcloud_auth
    }

    /// Get the WireGuard identity provider
    pub fn wireguard(&self) -> &WireGuardIdentity {
        &self.wireguard
    }

    /// Create or retrieve session based on WireGuard identity
    pub async fn get_or_create_session_from_wireguard(&self) -> anyhow::Result<Session> {
        let pubkey = self.wireguard.get_local_pubkey()?;
        self.get_or_create_session(&pubkey).await
    }

    /// Get or create a session for a given pubkey
    pub async fn get_or_create_session(&self, pubkey: &str) -> anyhow::Result<Session> {
        let now = Utc::now();

        // Check for existing valid session
        if let Some(mut session_ref) = self.sessions.iter_mut().find(|r| {
            r.pubkey == pubkey && (now - r.last_seen_at).num_seconds() < SESSION_TIMEOUT_SECS
        }) {
            debug!("Found existing session: {}", session_ref.session_id);
            session_ref.last_seen_at = now;
            let session = session_ref.clone();

            *self.current_session_id.lock().await = Some(session.session_id.clone());
            return Ok(session);
        }

        // Create new session from the derived key for this pubkey.
        // The same pubkey always yields the same session id, so re-provisioning
        // the same machine resumes the same logical session without server-side
        // custody of the private key.
        let session_id = derive_session_id(pubkey);
        info!(
            "Creating new derived session: {} for pubkey: {}",
            session_id, pubkey
        );

        // Try to get user email from the persistent identity vault.
        let user_email = self
            .identity_vault
            .lock()
            .await
            .get(pubkey)
            .map(|u| u.user_email.clone());

        // Try to get OAuth token
        let (oauth_token, token_expires_at) = match self.gcloud_auth.get_token().await {
            Ok((token, expires)) => (Some(token), Some(expires)),
            Err(e) => {
                warn!("Could not get OAuth token: {}", e);
                (None, None)
            }
        };

        let session = Session {
            session_id: session_id.clone(),
            pubkey: pubkey.to_string(),
            user_email,
            oauth_token,
            token_expires_at,
            created_at: now,
            last_seen_at: now,
        };

        self.sessions.insert(session_id.clone(), session.clone());

        // Store current session ID
        *self.current_session_id.lock().await = Some(session_id);

        Ok(session)
    }

    /// Get the current session ID
    pub async fn current_session_id(&self) -> Option<String> {
        self.current_session_id.lock().await.as_ref().cloned()
    }

    /// Update last_seen timestamp for current session
    pub async fn touch_session(&self) -> anyhow::Result<()> {
        let session_id = self.current_session_id.lock().await.as_ref().cloned();

        if let Some(id) = session_id {
            if let Some(mut session) = self.sessions.get_mut(&id) {
                session.last_seen_at = Utc::now();
            }
        }

        Ok(())
    }

    /// Get a valid OAuth token, refreshing if necessary
    pub async fn get_valid_token(&self) -> anyhow::Result<String> {
        let session_id = self.current_session_id.lock().await.as_ref().cloned();

        if let Some(id) = session_id {
            if let Some(session) = self.sessions.get(&id) {
                if let (Some(token), Some(expires_at)) =
                    (&session.oauth_token, session.token_expires_at)
                {
                    // Token valid for at least 5 more minutes
                    if expires_at > Utc::now() + chrono::Duration::minutes(5) {
                        return Ok(token.clone());
                    }
                }
            }
        }

        // Refresh token
        let (token, expires_at) = self.gcloud_auth.get_token().await?;

        // Update in session
        if let Some(id) = self.current_session_id.lock().await.as_ref().cloned() {
            if let Some(mut session) = self.sessions.get_mut(&id) {
                session.oauth_token = Some(token.clone());
                session.token_expires_at = Some(expires_at);
            }
        }

        Ok(token)
    }

    /// Register a WireGuard user mapping in the persistent identity vault.
    pub async fn register_wireguard_user(
        &self,
        pubkey: &str,
        user_email: &str,
        allowed_ip: &str,
    ) -> anyhow::Result<()> {
        let now = Utc::now();
        let record = UserRecord {
            pubkey: pubkey.to_string(),
            user_email: user_email.to_string(),
            allowed_ip: allowed_ip.to_string(),
            created_at: now,
            provisioned_at: now,
        };

        self.identity_vault
            .lock()
            .await
            .store(record)
            .context("persist user record to identity vault")?;

        info!("Registered WireGuard user: {} -> {}", pubkey, user_email);
        Ok(())
    }

    /// Get user email for a pubkey from the persistent identity vault.
    pub async fn get_user_for_pubkey(&self, pubkey: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .identity_vault
            .lock()
            .await
            .get(pubkey)
            .map(|u| u.user_email.clone()))
    }

    /// Clean up expired sessions
    pub async fn cleanup_expired_sessions(&self) -> anyhow::Result<usize> {
        let now = Utc::now();
        let mut deleted = 0;

        self.sessions.retain(|_, session| {
            if (now - session.last_seen_at).num_seconds() >= SESSION_TIMEOUT_SECS {
                deleted += 1;
                false
            } else {
                true
            }
        });

        if deleted > 0 {
            info!("Cleaned up {} expired sessions", deleted);
        }

        Ok(deleted)
    }

    /// Invalidate a specific session
    pub async fn invalidate_session(&self, session_id: &str) -> anyhow::Result<()> {
        self.sessions.remove(session_id);

        // Clear current session if it matches
        let mut current_guard = self.current_session_id.lock().await;
        if current_guard.as_deref() == Some(session_id) {
            *current_guard = None;
        }

        info!("Invalidated session: {}", session_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_manager() -> (SessionManager, TempDir) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.json");
        (SessionManager::with_vault_path(&path).unwrap(), dir)
    }

    #[test]
    fn derive_session_id_is_deterministic_and_uuid_like() {
        let id1 = derive_session_id("test-pubkey");
        let id2 = derive_session_id("test-pubkey");
        let id3 = derive_session_id("different-pubkey");
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        // Uuid::parse_str accepts both hyphenated and raw hex forms; our output is hyphenated.
        assert!(uuid::Uuid::parse_str(&id1).is_ok());
    }

    /// VAL-ASSERT-017: derive_principal_id is deterministic and UUID-shaped.
    #[test]
    fn derive_principal_id_is_deterministic_and_uuid_like() {
        let id1 = derive_principal_id("test-pubkey");
        let id2 = derive_principal_id("test-pubkey");
        let id3 = derive_principal_id("different-pubkey");
        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert!(uuid::Uuid::parse_str(&id1).is_ok());
        // Hyphenated UUID shape: 36 chars, 4 hyphens.
        assert_eq!(id1.len(), 36);
        assert_eq!(id1.chars().filter(|c| *c == '-').count(), 4);
    }

    /// VAL-ASSERT-018: derive_principal_id is context-separated from
    /// derive_session_id and matches an independent recomputation of the
    /// exact recipe.
    #[test]
    fn derive_principal_id_context_separated_from_session_id() {
        let pubkey = "dGVzdC1wdWJrZXk=";
        // A principal id can never collide with a container session id.
        assert_ne!(derive_principal_id(pubkey), derive_session_id(pubkey));

        // Independent recomputation: blake3 derive_key under the
        // human-principal context -> first 16 bytes -> raw Uuid::from_bytes
        // (no version/variant munging).
        let kdf = blake3::derive_key("op-identity human-principal v1", pubkey.as_bytes());
        let bytes: [u8; 16] = kdf[..16]
            .try_into()
            .expect("16 bytes from 32-byte KDF output");
        assert_eq!(
            derive_principal_id(pubkey),
            Uuid::from_bytes(bytes).to_string()
        );

        // The session id recipe uses the distinct context.
        let session_kdf = blake3::derive_key("op-identity session-id v1", pubkey.as_bytes());
        let session_bytes: [u8; 16] = session_kdf[..16]
            .try_into()
            .expect("16 bytes from 32-byte KDF output");
        assert_eq!(
            derive_session_id(pubkey),
            Uuid::from_bytes(session_bytes).to_string()
        );
    }

    #[test]
    fn session_proof_is_bound_to_the_canonical_session_id() {
        let first = derive_session_id("public-key-a");
        let again = derive_session_id("public-key-a");
        let other = derive_session_id("public-key-b");
        assert_eq!(first, again);
        assert_ne!(first, other);
        assert!(Uuid::parse_str(&first).is_ok());
        assert!(verify_session_proof(&first, &session_proof(&first)));
    }

    #[tokio::test]
    async fn test_session_creation() {
        let (manager, _dir) = test_manager();
        let session = manager.get_or_create_session("test-pubkey").await.unwrap();
        assert_eq!(session.pubkey, "test-pubkey");
        assert!(manager.current_session_id().await.is_some());
        // The session id should be the deterministic derivation, not a random UUID.
        assert_eq!(session.session_id, derive_session_id("test-pubkey"));
    }

    #[tokio::test]
    async fn test_session_touch() {
        let (manager, _dir) = test_manager();
        let session = manager.get_or_create_session("test-pubkey").await.unwrap();
        let last_seen = session.last_seen_at;

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        manager.touch_session().await.unwrap();

        let updated = manager.get_or_create_session("test-pubkey").await.unwrap();
        assert!(updated.last_seen_at > last_seen);
    }

    #[tokio::test]
    async fn test_wireguard_user_registration() {
        let (manager, _dir) = test_manager();
        manager
            .register_wireguard_user("pubkey1", "user@example.com", "10.0.0.1")
            .await
            .unwrap();

        let email = manager.get_user_for_pubkey("pubkey1").await.unwrap();
        assert_eq!(email, Some("user@example.com".to_string()));
    }

    #[tokio::test]
    async fn identity_vault_survives_reconstruction() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.json");
        {
            let manager = SessionManager::with_vault_path(&path).unwrap();
            manager
                .register_wireguard_user("pubkey2", "vault@example.com", "10.0.0.2")
                .await
                .unwrap();
        }
        let manager = SessionManager::with_vault_path(&path).unwrap();
        let email = manager.get_user_for_pubkey("pubkey2").await.unwrap();
        assert_eq!(email, Some("vault@example.com".to_string()));
    }
}
