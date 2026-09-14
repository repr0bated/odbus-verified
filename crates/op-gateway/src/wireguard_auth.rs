//! WireGuard authentication and session management integration
//!
//! This module integrates WireGuard identity management with the OP-DBUS service system,
//! providing high-performance JSON-RPC authentication with D-Bus compatibility.
//!
//! ## ⚠️ CRITICAL SECURITY MODEL - READ SECURITY-MODEL.md ⚠️
//!
//! - WG PSK is STATIC (identity, not rotated per-login)
//! - Session keys rotate per-login using SERVER NONCE (not timestamp)
//! - See `SECURITY-MODEL.md` in this crate for full details

use serde::{Deserialize, Serialize};

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};
use tokio::time::interval;
use tracing::{debug, info, warn};

use argon2::Argon2;
use base64::{engine::general_purpose, Engine as _};
use blake2::{Blake2s256, Digest};
use ring::rand::{SecureRandom, SystemRandom};

use crate::encrypted_storage::{EncryptedKeyStorage, EncryptedStorageConfig, KeyType};
use anyhow::Result;

#[derive(Clone)]
pub struct WireGuardDatabase {
    sessions: Arc<RwLock<HashMap<String, WireGuardSession>>>,
}

impl WireGuardDatabase {
    pub async fn new() -> Result<Self> {
        Ok(Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn migrate(&self) -> Result<()> {
        Ok(())
    }

    pub async fn store_wireguard_session(&self, session: &WireGuardSession) -> Result<()> {
        self.sessions
            .write()
            .await
            .insert(session.session_id.clone(), session.clone());
        Ok(())
    }

    pub async fn update_wireguard_session(&self, session: &WireGuardSession) -> Result<()> {
        self.store_wireguard_session(session).await?;
        Ok(())
    }

    pub async fn update_session_last_used(&self, session_id: &str, last_used: u64) -> Result<()> {
        if let Some(session) = self.sessions.write().await.get_mut(session_id) {
            session.last_used = last_used;
        }
        Ok(())
    }

    pub async fn load_wireguard_sessions(&self) -> Result<Vec<WireGuardSession>> {
        Ok(self.sessions.read().await.values().cloned().collect())
    }

    pub async fn remove_wireguard_session(&self, session_id: &str) -> Result<()> {
        self.sessions.write().await.remove(session_id);
        Ok(())
    }
}

/// WireGuard session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireGuardSession {
    pub session_id: String,
    pub peer_pubkey: String,
    pub psk: String,
    pub created_at: u64,
    pub expires_at: u64,
    pub is_active: bool,
    pub last_used: u64,
    pub client_ip: Option<String>,
    pub client_version: Option<String>,
    pub auth_method: String,
    pub key_rotation_count: u32,
    pub flags: HashMap<String, String>,
}

/// WireGuard authentication statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireGuardStats {
    pub total_sessions: u64,
    pub active_sessions: u64,
    pub keys_rotated: u64,
    pub auth_failures: u64,
    pub uptime_seconds: u64,
    pub memory_usage: u64,
    pub cpu_usage: f64,
    pub request_rate: f64,
    pub error_rate: f64,
    pub cache_hits: u64,
}

/// High-performance WireGuard authentication manager
pub struct WireGuardAuthManager {
    /// Cryptographic engine for key operations
    crypto_engine: Arc<SimdCryptoEngine>,
    /// Encrypted key storage
    key_storage: Arc<tokio::sync::Mutex<EncryptedKeyStorage>>,
    /// Active sessions cache
    sessions: Arc<RwLock<HashMap<String, WireGuardSession>>>,
    /// Peer public key to session mapping
    peer_sessions: Arc<RwLock<HashMap<String, String>>>,
    /// Database for WireGuard sessions (separate from services database)
    database: WireGuardDatabase,
    /// Statistics tracking
    stats: Arc<Mutex<WireGuardStats>>,
    /// Service start time
    start_time: Instant,
    /// Session cleanup interval
    cleanup_interval: Duration,
}

impl WireGuardAuthManager {
    /// Create new WireGuard authentication manager
    pub async fn new() -> Result<Self> {
        info!("Initializing WireGuard authentication manager");

        // Initialize separate WireGuard database
        let database = WireGuardDatabase::new().await?;
        database.migrate().await?;

        // Initialize encrypted key storage
        let storage_config = EncryptedStorageConfig::default();
        let key_storage = Arc::new(tokio::sync::Mutex::new(
            EncryptedKeyStorage::new(storage_config).await?,
        ));

        // Initialize crypto engine
        let crypto_engine = Arc::new(SimdCryptoEngine::new().await?);

        // Initialize statistics
        let stats = Arc::new(Mutex::new(WireGuardStats {
            total_sessions: 0,
            active_sessions: 0,
            keys_rotated: 0,
            auth_failures: 0,
            uptime_seconds: 0,
            memory_usage: 0,
            cpu_usage: 0.0,
            request_rate: 0.0,
            error_rate: 0.0,
            cache_hits: 0,
        }));

        let manager = Self {
            crypto_engine,
            key_storage,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            peer_sessions: Arc::new(RwLock::new(HashMap::new())),
            database,
            stats,
            start_time: Instant::now(),
            cleanup_interval: Duration::from_secs(300), // 5 minutes
        };

        // Load existing sessions from database
        manager.load_sessions_from_database().await?;

        // Start background tasks
        manager.start_background_tasks().await;

        info!("WireGuard authentication manager initialized");
        Ok(manager)
    }

    /// Create a new WireGuard session
    #[tracing::instrument(skip(self))]
    pub async fn create_session(
        &self,
        peer_pubkey: &str,
        client_info: Option<ClientInfo>,
    ) -> Result<WireGuardSession> {
        debug!("Creating WireGuard session for peer: {}", peer_pubkey);

        // Validate peer public key format
        if !Self::is_valid_pubkey(peer_pubkey) {
            return Err(anyhow::anyhow!(
                "Invalid peer public key format: {}",
                peer_pubkey
            ));
        }

        // Check if session already exists for this peer
        {
            let peer_sessions = self.peer_sessions.read().await;
            if let Some(existing_session_id) = peer_sessions.get(peer_pubkey) {
                let sessions = self.sessions.read().await;
                if let Some(session) = sessions.get(existing_session_id) {
                    if session.is_active && session.expires_at > Self::current_timestamp() {
                        debug!(
                            "Returning existing active session for peer: {}",
                            peer_pubkey
                        );
                        return Ok(session.clone());
                    }
                }
            }
        }

        // The public wristband must be unguessable. Keep the WireGuard-derived
        // claim ticket only as non-bearer metadata for correlation.
        let session_id = Self::generate_wristband_id()?;
        let claim_ticket = self.derive_wireguard_claim_ticket(peer_pubkey).await?;
        let psk = self.derive_psk(peer_pubkey).await?;

        let now = Self::current_timestamp();
        let expires_at = now + 3600; // 1 hour default
        let mut flags = HashMap::new();
        flags.insert("wireguard_claim_ticket".to_string(), claim_ticket);
        flags.insert("wristband_issuer".to_string(), "op-gateway".to_string());
        flags.insert(
            "wristband_entropy".to_string(),
            "256-bit-random".to_string(),
        );

        let session = WireGuardSession {
            session_id: session_id.clone(),
            peer_pubkey: peer_pubkey.to_string(),
            psk,
            created_at: now,
            expires_at,
            is_active: true,
            last_used: now,
            client_ip: client_info.as_ref().and_then(|c| c.ip.clone()),
            client_version: client_info.as_ref().and_then(|c| c.version.clone()),
            auth_method: "wireguard".to_string(),
            key_rotation_count: 0,
            flags,
        };

        // Store in database
        self.database.store_wireguard_session(&session).await?;

        // Update caches
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(session_id.clone(), session.clone());
        }
        {
            let mut peer_sessions = self.peer_sessions.write().await;
            peer_sessions.insert(peer_pubkey.to_string(), session_id);
        }

        // Update statistics
        {
            let mut stats = self.stats.lock().await;
            stats.total_sessions += 1;
            stats.active_sessions += 1;
        }

        info!(
            "Created WireGuard session {} for peer {}",
            session.session_id, peer_pubkey
        );
        Ok(session)
    }

    /// Validate a WireGuard session
    #[tracing::instrument(skip(self))]
    pub async fn validate_session(&self, session_id: &str) -> Result<bool> {
        debug!("Validating WireGuard session: {}", session_id);

        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(session_id) {
            let now = Self::current_timestamp();
            let is_valid = session.is_active && session.expires_at > now;

            if is_valid {
                // Update last used timestamp (we'll do this in a separate task to avoid blocking)
                tokio::spawn({
                    let database = self.database.clone();
                    let session_id = session_id.to_string();
                    async move {
                        if let Err(e) = database.update_session_last_used(&session_id, now).await {
                            warn!("Failed to update session last used: {}", e);
                        }
                    }
                });
            }

            Ok(is_valid)
        } else {
            Ok(false)
        }
    }

    /// Get session information
    #[tracing::instrument(skip(self))]
    pub async fn get_session(&self, session_id: &str) -> Result<Option<WireGuardSession>> {
        let sessions = self.sessions.read().await;
        Ok(sessions.get(session_id).cloned())
    }

    /// List active sessions
    #[tracing::instrument(skip(self))]
    pub async fn list_sessions(
        &self,
        filter: Option<SessionFilter>,
    ) -> Result<Vec<WireGuardSession>> {
        let sessions = self.sessions.read().await;
        let mut result: Vec<WireGuardSession> = sessions.values().cloned().collect();

        // Apply filters
        if let Some(filter) = filter {
            result.retain(|session| {
                if let Some(active_only) = filter.active_only {
                    if active_only && !session.is_active {
                        return false;
                    }
                }
                if let Some(peer_pubkey) = &filter.peer_pubkey {
                    if &session.peer_pubkey != peer_pubkey {
                        return false;
                    }
                }
                true
            });
        }

        // Sort by creation time (newest first)
        result.sort_by_key(|b| std::cmp::Reverse(b.created_at));

        Ok(result)
    }

    /// Rotate session key for a peer (NOT the WireGuard PSK)
    /// WireGuard PSK remains stable to avoid desync issues
    #[tracing::instrument(skip(self))]
    pub async fn rotate_session_key(&self, peer_pubkey: &str, force: bool) -> Result<String> {
        info!(
            "Rotating session key for peer: {} (force: {})",
            peer_pubkey, force
        );

        // Get existing session
        let session_id = {
            let peer_sessions = self.peer_sessions.read().await;
            peer_sessions.get(peer_pubkey).cloned()
        };

        let session_id = match session_id {
            Some(id) => id,
            None => {
                if force {
                    // Create new session if forced
                    let session = self.create_session(peer_pubkey, None).await?;
                    return Ok(session.session_id);
                } else {
                    return Err(anyhow::anyhow!("No active session found for peer"));
                }
            }
        };

        // Generate new session key (not PSK!)
        let new_session_key = self.derive_session_key(peer_pubkey).await?;

        // Update session
        {
            let mut sessions = self.sessions.write().await;
            if let Some(session) = sessions.get_mut(&session_id) {
                // Store new session key in metadata, keep PSK stable
                session
                    .flags
                    .insert("current_session_key".to_string(), new_session_key.clone());
                session.key_rotation_count += 1;
                session.last_used = Self::current_timestamp();

                // Update in database
                if let Err(e) = self.database.update_wireguard_session(session).await {
                    warn!("Failed to update session in database: {}", e);
                }
            }
        }

        // Update statistics
        {
            let mut stats = self.stats.lock().await;
            stats.keys_rotated += 1;
        }

        info!("Session key rotated successfully for peer: {}", peer_pubkey);
        Ok(new_session_key)
    }

    /// Get authentication statistics
    #[tracing::instrument(skip(self))]
    pub async fn get_stats(&self) -> Result<WireGuardStats> {
        let mut stats = self.stats.lock().await;

        // Update uptime
        stats.uptime_seconds = self.start_time.elapsed().as_secs();

        // Update active sessions count
        let sessions = self.sessions.read().await;
        let now = Self::current_timestamp();
        stats.active_sessions = sessions
            .values()
            .filter(|s| s.is_active && s.expires_at > now)
            .count() as u64;

        Ok(stats.clone())
    }

    /// Generate the bearer wristband ID with high-entropy randomness.
    fn generate_wristband_id() -> anyhow::Result<String> {
        let mut token = [0u8; 32];
        SystemRandom::new()
            .fill(&mut token)
            .map_err(|_| anyhow::anyhow!("Failed to generate WireGuard wristband ID"))?;
        Ok(hex::encode(token))
    }

    /// Derive a non-bearer claim ticket using SIMD-accelerated BLAKE2s.
    async fn derive_wireguard_claim_ticket(&self, peer_pubkey: &str) -> anyhow::Result<String> {
        let input = format!("WG-SESSION-{}-{}", peer_pubkey, Self::current_timestamp());
        let session_ids = self
            .crypto_engine
            .generate_session_ids_batch(&[input.as_bytes()]);

        if let Some(session_id) = session_ids.first() {
            Ok(hex::encode(session_id))
        } else {
            Err(anyhow::anyhow!("Failed to generate session ID"))
        }
    }

    /// Derive stable PSK (no timestamp, no rotation)
    /// WireGuard PSK should remain stable to avoid connection issues
    async fn derive_psk(&self, peer_pubkey: &str) -> anyhow::Result<String> {
        // Convert peer pubkey to bytes
        let peer_key_bytes = match hex::decode(peer_pubkey) {
            Ok(bytes) => {
                if bytes.len() == 32 {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(&bytes);
                    key
                } else {
                    return Err(anyhow::anyhow!(
                        "Invalid peer public key length: {}",
                        bytes.len()
                    ));
                }
            }
            Err(_) => {
                return Err(anyhow::anyhow!(
                    "Invalid peer public key format: {}",
                    peer_pubkey
                ))
            }
        };

        // Check if we have a stored PSK for this peer (stable, no rotation)
        let psk_key_id = format!("psk_{}", peer_pubkey);
        let mut key_storage = self.key_storage.lock().await;

        if let Ok(stored_psk) = key_storage.retrieve_key(&psk_key_id).await {
            if stored_psk.len() == 32 {
                return Ok(general_purpose::STANDARD.encode(&stored_psk));
            }
        }

        // Generate stable PSK (no timestamp input)
        let psks = self.crypto_engine.derive_stable_psk(&peer_key_bytes);

        if let Some(psk) = psks.first() {
            // Store the PSK in encrypted storage
            if let Err(e) = key_storage
                .store_key(&psk_key_id, psk, KeyType::WireGuardPsk)
                .await
            {
                warn!("Failed to store PSK in encrypted storage: {}", e);
            }

            Ok(general_purpose::STANDARD.encode(psk))
        } else {
            Err(anyhow::anyhow!("Failed to derive PSK"))
        }
    }

    /// Derive session key using server nonce (not timestamp)
    /// This is what gets rotated per-login, not the WireGuard PSK
    async fn derive_session_key(&self, peer_pubkey: &str) -> anyhow::Result<String> {
        // Generate server nonce for this session
        let server_nonce = self.generate_server_nonce().await?;

        // Convert peer pubkey to bytes
        let peer_key_bytes = match hex::decode(peer_pubkey) {
            Ok(bytes) => {
                if bytes.len() == 32 {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(&bytes);
                    key
                } else {
                    return Err(anyhow::anyhow!(
                        "Invalid peer public key length: {}",
                        bytes.len()
                    ));
                }
            }
            Err(_) => {
                return Err(anyhow::anyhow!(
                    "Invalid peer public key format: {}",
                    peer_pubkey
                ))
            }
        };

        // Derive session key using nonce (not timestamp)
        let session_keys = self
            .crypto_engine
            .derive_session_keys(&[peer_key_bytes], &[server_nonce]);

        if let Some((session_key, _)) = session_keys.first() {
            Ok(general_purpose::STANDARD.encode(session_key))
        } else {
            Err(anyhow::anyhow!("Failed to derive session key"))
        }
    }

    /// Generate server nonce for session key derivation
    async fn generate_server_nonce(&self) -> anyhow::Result<[u8; 32]> {
        let mut nonce = [0u8; 32];
        ring::rand::SecureRandom::fill(&SystemRandom::new(), &mut nonce)
            .map_err(|_| anyhow::anyhow!("Failed to generate server nonce"))?;
        Ok(nonce)
    }

    /// Store WireGuard private key in encrypted storage
    #[tracing::instrument(skip(self))]
    pub async fn store_private_key(&self, key_id: &str, private_key: &[u8; 32]) -> Result<()> {
        let mut key_storage = self.key_storage.lock().await;
        key_storage
            .store_key(key_id, private_key, KeyType::WireGuardPrivate)
            .await?;
        info!("Stored WireGuard private key: {}", key_id);
        Ok(())
    }

    /// Retrieve WireGuard private key from encrypted storage
    #[tracing::instrument(skip(self))]
    pub async fn retrieve_private_key(&self, key_id: &str) -> anyhow::Result<[u8; 32]> {
        let key_storage = self.key_storage.lock().await;
        let key_data = key_storage.retrieve_key(key_id).await?;

        if key_data.len() != 32 {
            return Err(anyhow::anyhow!(
                "Invalid private key length: {}",
                key_data.len()
            ));
        }

        let mut private_key = [0u8; 32];
        private_key.copy_from_slice(&key_data);
        Ok(private_key)
    }

    /// Get encrypted storage statistics
    #[tracing::instrument(skip(self))]
    pub async fn get_storage_stats(
        &self,
    ) -> anyhow::Result<crate::encrypted_storage::StorageStats> {
        let key_storage = self.key_storage.lock().await;
        key_storage.get_stats().await
    }

    /// Load or generate master key
    #[allow(dead_code)]
    async fn load_or_generate_master_key() -> anyhow::Result<Arc<[u8; 32]>> {
        // Try to load from environment or file
        if let Ok(key_hex) = std::env::var("WG_AUTH_MASTER_KEY") {
            if let Ok(key_bytes) = hex::decode(&key_hex) {
                if key_bytes.len() == 32 {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(&key_bytes);
                    return Ok(Arc::new(key));
                }
            }
        }

        // Generate new key
        let rng = SystemRandom::new();
        let mut key = [0u8; 32];
        ring::rand::SecureRandom::fill(&rng, &mut key)
            .map_err(|_| anyhow::anyhow!("Failed to generate master key"))?;

        warn!("Generated new master key - consider persisting it for production use");
        Ok(Arc::new(key))
    }

    /// Load existing sessions from database
    async fn load_sessions_from_database(&self) -> anyhow::Result<()> {
        debug!("Loading WireGuard sessions from database");

        let sessions = self.database.load_wireguard_sessions().await?;
        let now = Self::current_timestamp();

        let mut active_count = 0;
        {
            let mut session_cache = self.sessions.write().await;
            let mut peer_cache = self.peer_sessions.write().await;

            for session in sessions {
                // Only load active, non-expired sessions
                if session.is_active && session.expires_at > now {
                    peer_cache.insert(session.peer_pubkey.clone(), session.session_id.clone());
                    session_cache.insert(session.session_id.clone(), session);
                    active_count += 1;
                }
            }
        }

        info!(
            "Loaded {} active WireGuard sessions from database",
            active_count
        );
        Ok(())
    }

    /// Start background maintenance tasks
    async fn start_background_tasks(&self) {
        let sessions = self.sessions.clone();
        let peer_sessions = self.peer_sessions.clone();
        let database = self.database.clone();
        let stats = self.stats.clone();
        let cleanup_interval = self.cleanup_interval;

        // Session cleanup task
        tokio::spawn(async move {
            let mut interval = interval(cleanup_interval);

            loop {
                interval.tick().await;

                let now = Self::current_timestamp();
                let mut expired_sessions = Vec::new();

                // Find expired sessions
                {
                    let sessions_read = sessions.read().await;
                    for (session_id, session) in sessions_read.iter() {
                        if !session.is_active || session.expires_at <= now {
                            expired_sessions
                                .push((session_id.clone(), session.peer_pubkey.clone()));
                        }
                    }
                }

                // Remove expired sessions
                if !expired_sessions.is_empty() {
                    let mut sessions_write = sessions.write().await;
                    let mut peer_sessions_write = peer_sessions.write().await;

                    for (session_id, peer_pubkey) in expired_sessions {
                        sessions_write.remove(&session_id);
                        peer_sessions_write.remove(&peer_pubkey);

                        // Remove from database
                        if let Err(e) = database.remove_wireguard_session(&session_id).await {
                            warn!("Failed to remove expired session from database: {}", e);
                        }
                    }

                    // Update stats
                    let mut stats_lock = stats.lock().await;
                    let active_sessions = sessions_write
                        .values()
                        .filter(|s| s.is_active && s.expires_at > now)
                        .count() as u64;
                    stats_lock.active_sessions = active_sessions;
                }
            }
        });
    }

    /// Validate peer public key format
    fn is_valid_pubkey(pubkey: &str) -> bool {
        if pubkey.len() != 64 {
            return false;
        }
        hex::decode(pubkey).is_ok()
    }

    /// Get current Unix timestamp
    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

/// SIMD-optimized cryptographic engine
pub struct SimdCryptoEngine {
    _rng: SystemRandom,
}

impl SimdCryptoEngine {
    /// Create new SIMD crypto engine
    pub async fn new() -> Result<Self> {
        Ok(Self {
            _rng: SystemRandom::new(),
        })
    }

    /// Generate session IDs using SIMD-accelerated BLAKE2s
    pub fn generate_session_ids_batch(&self, inputs: &[&[u8]]) -> Vec<[u8; 16]> {
        let mut results = Vec::with_capacity(inputs.len());

        for input in inputs {
            let mut hasher = Blake2s256::new();
            hasher.update(input);
            let hash = hasher.finalize();
            results.push(hash[..16].try_into().unwrap());
        }

        results
    }

    /// Derive stable PSKs (no timestamp input to avoid lockout)
    pub fn derive_stable_psk(&self, peer_key: &[u8; 32]) -> Vec<[u8; 32]> {
        let mut results = Vec::with_capacity(1);

        // Use the WG public key as the unique salt, and the stable string as input
        let salt = peer_key;

        let input = b"WG-STABLE-PSK-2024";

        let argon2 = Argon2::default();
        let mut psk = [0u8; 32];
        if argon2.hash_password_into(input, salt, &mut psk).is_ok() {
            results.push(psk);
        }

        results
    }

    /// Derive session keys using server nonces (not timestamps)
    pub fn derive_session_keys(
        &self,
        peer_keys: &[[u8; 32]],
        server_nonces: &[[u8; 32]],
    ) -> Vec<([u8; 32], [u8; 16])> {
        let mut results = Vec::with_capacity(peer_keys.len());

        // Use different salt for session keys
        let salt = b"WG-SESSION-KEY-2024";

        for (peer_key, server_nonce) in peer_keys.iter().zip(server_nonces) {
            let mut input = Vec::with_capacity(71);
            input.extend_from_slice(b"WG-SESSION-");
            input.extend_from_slice(peer_key);
            input.extend_from_slice(server_nonce);

            let argon2 = Argon2::default();
            let mut session_key = [0u8; 32];
            if argon2
                .hash_password_into(&input, salt, &mut session_key)
                .is_ok()
            {
                // Derive session ID from session key
                let mut hasher = Blake2s256::new();
                hasher.update(session_key);
                let hash = hasher.finalize();
                let session_id: [u8; 16] = hash[..16].try_into().unwrap();

                results.push((session_key, session_id));
            }
        }

        results
    }
}

/// Client information for session creation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub ip: Option<String>,
    pub version: Option<String>,
    pub user_agent: Option<String>,
}

/// Session filter for listing operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFilter {
    pub active_only: Option<bool>,
    pub peer_pubkey: Option<String>,
    pub created_after: Option<u64>,
    pub created_before: Option<u64>,
}

impl Default for SessionFilter {
    fn default() -> Self {
        Self {
            active_only: Some(true),
            peer_pubkey: None,
            created_after: None,
            created_before: None,
        }
    }
}
