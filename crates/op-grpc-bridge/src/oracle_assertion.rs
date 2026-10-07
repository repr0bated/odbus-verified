//! Oracle identity assertion validation for op-grpc-bridge.
//!
//! Pipeline order (contractual): parse → trusted decoy key → signature →
//! expiry → non-mutating replay lookup → configured transport binding →
//! HumanPrincipal/session resolution → durable nonce consumption.

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use base64::Engine as _;
use ed25519_dalek::VerifyingKey;
use op_identity::oracle_assertion::{MAX_LIFETIME_SECS, SignedAssertion, verify_signature};
use op_identity::session::derive_principal_id;
use thiserror::Error;

pub const DEFAULT_DECOY_TRUST_STORE: &str = "/etc/opdbus/decoy-trust.json";
pub const DEFAULT_AUTH_REPLAY_DB_PATH: &str = "/var/lib/op-dbus/auth-replay.db";
pub const AUTH_REPLAY_DB_PATH_ENV: &str = "OP_AUTH_REPLAY_DB_PATH";
pub const SOURCE_BINDING_ENV: &str = "OP_ORACLE_ASSERTION_SOURCE_BINDING";
pub const CLOCK_LEEWAY_SECS: i64 = 30;
/// Public operator assignment. A single canonical WG key, never a client target.
pub const BROWSER_PEER_KEY_ENV: &str = "OP_BROWSER_CONTROL_PLANE_PEER_KEY";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrowserIdentityBinding {
    peer_key: String,
    pub(crate) principal_id: String,
    pub(crate) session_id: String,
}

#[derive(Debug, Clone, Default)]
enum BrowserBindingPolicy {
    #[default]
    Disabled,
    Assigned(BrowserIdentityBinding),
    Invalid,
}

impl BrowserBindingPolicy {
    fn parse(peer: Option<&str>, principal: Option<&str>, session: Option<&str>) -> Self {
        let Some(peer) = peer else {
            return Self::Disabled;
        };
        let valid_key = base64::engine::general_purpose::STANDARD
            .decode(peer)
            .is_ok_and(|bytes| {
                bytes.len() == 32 && base64::engine::general_purpose::STANDARD.encode(bytes) == peer
            });
        let canonical_uuid = |value: Option<&str>| {
            value.is_some_and(|value| {
                uuid::Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.to_string() == value)
            })
        };
        if !valid_key || !canonical_uuid(principal) || !canonical_uuid(session) {
            return Self::Invalid;
        }
        Self::Assigned(BrowserIdentityBinding {
            peer_key: peer.to_owned(),
            principal_id: principal.expect("validated principal").to_owned(),
            session_id: session.expect("validated session").to_owned(),
        })
    }

    fn from_env() -> Self {
        let peer = match std::env::var(BROWSER_PEER_KEY_ENV) {
            Ok(value) => value,
            Err(std::env::VarError::NotPresent) => return Self::Disabled,
            Err(_) => return Self::Invalid,
        };
        for name in [
            BROWSER_PEER_KEY_ENV,
            "OP_CONTROL_PLANE_CHATBOT_PRINCIPAL_ID",
            "OP_CONTROL_PLANE_CHATBOT_SESSION_ID",
        ] {
            if std::env::vars_os().filter(|(key, _)| key == name).count() > 1 {
                return Self::Invalid;
            }
        }
        Self::parse(
            Some(&peer),
            std::env::var("OP_CONTROL_PLANE_CHATBOT_PRINCIPAL_ID")
                .ok()
                .as_deref(),
            std::env::var("OP_CONTROL_PLANE_CHATBOT_SESSION_ID")
                .ok()
                .as_deref(),
        )
    }
}

/// Receiver-created transport evidence. Fields are private so request values
/// cannot construct admission evidence. No credential bytes enter the audit.
#[derive(Debug, Clone)]
pub(crate) struct BoundOracleProof {
    binding: BrowserIdentityBinding,
    assertion_digest: String,
    issuer_key_id: String,
    signed_inner_ip: String,
    expires_at: i64,
    target_credential_digest: Option<String>,
}

impl BoundOracleProof {
    #[cfg(test)]
    pub(crate) fn expire_for_test(&mut self) {
        self.expires_at = 1;
    }
    pub(crate) fn binding(&self) -> &BrowserIdentityBinding {
        &self.binding
    }

    pub(crate) fn bind_credential(&mut self, digest: String) {
        self.target_credential_digest = Some(digest);
    }

    pub(crate) fn credential_matches(&self, digest: &str) -> bool {
        self.target_credential_digest.as_deref() == Some(digest)
    }

    pub(crate) fn is_current(&self, now: i64) -> bool {
        now < self.expires_at
    }

    pub(crate) fn peer_key(&self) -> &str {
        &self.binding.peer_key
    }

    pub(crate) fn audit_value(&self) -> serde_json::Value {
        let binding = serde_json::json!({"peer_key": self.binding.peer_key,
            "principal_id": self.binding.principal_id, "session_id": self.binding.session_id});
        serde_json::json!({"kind": "bound-oracle", "peer_key": self.binding.peer_key,
            "peer_principal_id": derive_principal_id(&self.binding.peer_key),
            "issuer_key_id": self.issuer_key_id, "signed_inner_ip": self.signed_inner_ip,
            "expires_at": self.expires_at, "assertion_digest": self.assertion_digest,
            "binding_digest": blake3::hash(binding.to_string().as_bytes()).to_hex().to_string(),
            "selected_principal_id": self.binding.principal_id,
            "selected_session_id": self.binding.session_id})
    }
}

/// How a decoy-signed assertion is bound to its transport.
///
/// `TrustedDecoySignature` is the live fabric binding: privacy transports replace the
/// human inner source address, so the request is bound to the decoy signature,
/// lifetime, one-time nonce, and registered human key — not TCP-source equality.
/// `ExactPeerIp` is the retired Netmaker overlay check; it is opt-in only via
/// `OP_ORACLE_ASSERTION_SOURCE_BINDING=exact-peer-ip`. A missing or unknown
/// env value must not revive that path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssertionSourceBinding {
    ExactPeerIp,
    TrustedDecoySignature,
}

impl AssertionSourceBinding {
    pub(crate) fn parse(value: Result<String, std::env::VarError>) -> Self {
        match value.as_deref() {
            Ok("exact-peer-ip") => Self::ExactPeerIp,
            Ok("trusted-decoy-signature") | Err(_) => Self::TrustedDecoySignature,
            Ok(other) => {
                tracing::error!(
                    env = SOURCE_BINDING_ENV,
                    value = other,
                    "unknown oracle assertion source binding; using trusted-decoy-signature"
                );
                Self::TrustedDecoySignature
            }
        }
    }

    fn from_env() -> Self {
        Self::parse(std::env::var(SOURCE_BINDING_ENV))
    }
}

/// Validated human identity carried in request extensions after assertion
/// validation succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanPrincipalIdentity {
    pub principal_id: String,
    pub human_pubkey: String,
    /// Stable, per-arrival session identifier. Populated only after the
    /// verified human session has been anchored.
    pub session_id: String,
    pub expires_at: i64,
    /// Snowball genesis anchor for this session. This proves arrival; it is
    /// not an authorization identity and is never used as a grant-store key.
    pub session_genesis: String,
}

/// Receiver-created origin of a remote MCP caller. The issuer only signs an
/// inner address after resolving an exact WireGuard AllowedIP (/32 or /128).
/// These claims become evidence only after signature, lifetime, transport and
/// principal validation; neither HTTP Origin nor client metadata can create it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct VerifiedWireGuardOrigin {
    principal_id: String,
    peer_key: String,
    issuer_key_id: String,
    signed_inner_ip: IpAddr,
    assertion_digest: String,
    expires_at: i64,
}

impl VerifiedWireGuardOrigin {
    pub(crate) fn principal_id(&self) -> &str {
        &self.principal_id
    }
    pub(crate) fn peer_key(&self) -> &str {
        &self.peer_key
    }
    pub(crate) fn issuer_key_id(&self) -> &str {
        &self.issuer_key_id
    }
    pub(crate) fn signed_inner_ip(&self) -> IpAddr {
        self.signed_inner_ip
    }
    pub(crate) fn assertion_digest(&self) -> &str {
        &self.assertion_digest
    }
    pub(crate) fn expires_at(&self) -> i64 {
        self.expires_at
    }

    pub(crate) fn allowed_ip_scope(&self) -> String {
        format!(
            "{}/{}",
            self.signed_inner_ip,
            if self.signed_inner_ip.is_ipv4() {
                32
            } else {
                128
            }
        )
    }

    /// A stable logical origin, never a destination or an authorization key.
    /// Grants remain keyed by the registered principal. The full signed tuple
    /// is retained by the external-session owner for scope validation.
    pub(crate) fn origin(&self) -> String {
        op_cozo_store::ExternalMcpSessionRecord::canonical_origin(
            &self.principal_id,
            &self.peer_key,
            &self.issuer_key_id,
            &self.allowed_ip_scope(),
        )
    }
}

/// Fail-closed rejection reasons for the assertion pipeline.
#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum AssertionRejection {
    #[error("Malformed")]
    Malformed,
    #[error("UnknownDecoyKey")]
    UnknownDecoyKey,
    #[error("BadSignature")]
    BadSignature,
    #[error("NotYetValid")]
    NotYetValid,
    #[error("Expired")]
    Expired,
    #[error("LifetimeTooLong")]
    LifetimeTooLong,
    #[error("Replay")]
    Replay,
    #[error("MissingConnectInfo")]
    MissingConnectInfo,
    #[error("SourceIpMismatch {{ expected: {expected}, actual: {actual} }}")]
    SourceIpMismatch { expected: IpAddr, actual: IpAddr },
    #[error("UnknownPrincipal")]
    UnknownPrincipal,
    #[error("RevokedPrincipal")]
    RevokedPrincipal,
    #[error("RegistryUnavailable")]
    RegistryUnavailable,
    #[error("ReplayStoreUnavailable")]
    ReplayStoreUnavailable,
    #[error("InvalidBrowserIdentityBinding")]
    InvalidBrowserIdentityBinding,
}

impl AssertionRejection {
    /// Authentication failures are `Unauthenticated`; persistence failures are
    /// `Unavailable` so callers can distinguish invalid credentials from a
    /// fail-closed infrastructure condition.
    pub fn into_unauthenticated_status(self) -> tonic::Status {
        let code = if self == Self::ReplayStoreUnavailable {
            tonic::Code::Unavailable
        } else {
            tonic::Code::Unauthenticated
        };
        tonic::Status::new(code, self.to_string())
    }
}

impl From<AssertionRejection> for tonic::Status {
    fn from(value: AssertionRejection) -> Self {
        value.into_unauthenticated_status()
    }
}

/// Trusted decoy verifying keys loaded once from JSON at construction time.
#[derive(Debug, Clone)]
pub struct DecoyTrustStore {
    keys: HashMap<String, VerifyingKey>,
}

#[derive(serde::Deserialize)]
struct TrustStoreJson {
    decoy_keys: HashMap<String, String>,
}

impl DecoyTrustStore {
    /// Load from `OP_DECOY_TRUST_STORE` or [`DEFAULT_DECOY_TRUST_STORE`].
    /// Missing/unreadable/invalid ⇒ empty store (fail-closed).
    pub fn load() -> Self {
        let path = std::env::var("OP_DECOY_TRUST_STORE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_DECOY_TRUST_STORE));
        Self::load_from_path(&path)
    }

    /// Load from an explicit path (tests).
    pub fn load_from_path(path: &Path) -> Self {
        match std::fs::read(path) {
            Ok(bytes) => Self::parse_bytes(&bytes),
            Err(_) => Self::empty(),
        }
    }

    /// Parse trust-store bytes. Any corruption yields an empty store.
    pub fn parse_bytes(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self::empty();
        }
        let raw = match std::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => return Self::empty(),
        };
        if decoy_keys_object_has_duplicate_keys(raw) {
            return Self::empty();
        }
        let parsed: TrustStoreJson = match serde_json::from_slice(bytes) {
            Ok(value) => value,
            Err(_) => return Self::empty(),
        };
        Self::from_decoy_keys_map(parsed.decoy_keys)
    }

    /// Construct directly from an in-memory map (tests).
    pub fn from_decoy_keys(keys: HashMap<String, VerifyingKey>) -> Self {
        Self { keys }
    }

    fn from_decoy_keys_map(entries: HashMap<String, String>) -> Self {
        let mut keys = HashMap::new();
        for (key_id, b64) in entries {
            let decoded = match base64::engine::general_purpose::STANDARD.decode(b64.as_bytes()) {
                Ok(bytes) => bytes,
                Err(_) => return Self::empty(),
            };
            if decoded.len() != 32 {
                return Self::empty();
            }
            let arr: [u8; 32] = match decoded.try_into() {
                Ok(arr) => arr,
                Err(_) => return Self::empty(),
            };
            let verifying_key = match VerifyingKey::from_bytes(&arr) {
                Ok(key) => key,
                Err(_) => return Self::empty(),
            };
            if keys.insert(key_id, verifying_key).is_some() {
                return Self::empty();
            }
        }
        Self { keys }
    }

    fn empty() -> Self {
        Self {
            keys: HashMap::new(),
        }
    }

    pub fn key_count(&self) -> usize {
        self.keys.len()
    }

    pub fn contains_key(&self, key_id: &str) -> bool {
        self.keys.contains_key(key_id)
    }

    fn verifying_key(&self, key_id: &str) -> Option<&VerifyingKey> {
        self.keys.get(key_id)
    }
}

#[derive(Debug, Error)]
#[error("{0}")]
pub struct ReplayStoreError(String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ReplayKey {
    issuer_key_id: String,
    nonce: [u8; 16],
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct DurableReplayRecord {
    envelope_kind: String,
    issuer_key_id: String,
    nonce_hex: String,
    expires_at: i64,
}

#[derive(Debug)]
struct DurableReplayState {
    file: File,
    seen: HashMap<ReplayKey, i64>,
    failure: Option<String>,
}

#[derive(Debug)]
enum ReplayBackend {
    Memory(Mutex<HashMap<ReplayKey, i64>>),
    Durable(Mutex<DurableReplayState>),
    Unavailable(String),
}

/// Replay state shared by every ingress attached to one validator.
///
/// Production uses the append-only durable backend. Each accepted record is
/// flushed before validation succeeds; a partial/corrupt journal or any write
/// failure makes the backend unavailable and all protected requests fail closed.
#[derive(Debug)]
pub struct AssertionReplayCache {
    backend: ReplayBackend,
    leeway_secs: i64,
}

impl AssertionReplayCache {
    /// In-memory backend for focused tests and explicitly injected validators.
    pub fn new(leeway_secs: i64) -> Self {
        Self {
            backend: ReplayBackend::Memory(Mutex::new(HashMap::new())),
            leeway_secs,
        }
    }

    pub fn persistent(path: &Path, leeway_secs: i64) -> Result<Self, ReplayStoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                ReplayStoreError(format!(
                    "create replay-store parent {}: {error}",
                    parent.display()
                ))
            })?;
        }

        #[cfg(unix)]
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let mut options = OpenOptions::new();
        options.create(true).read(true).append(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(path).map_err(|error| {
            ReplayStoreError(format!("open replay store {}: {error}", path.display()))
        })?;
        #[cfg(unix)]
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(
            |error| {
                ReplayStoreError(format!(
                    "secure replay store permissions {}: {error}",
                    path.display()
                ))
            },
        )?;

        let reader_file = file.try_clone().map_err(|error| {
            ReplayStoreError(format!("clone replay store {}: {error}", path.display()))
        })?;
        let mut seen: HashMap<ReplayKey, i64> = HashMap::new();
        for (index, line) in BufReader::new(reader_file).lines().enumerate() {
            let line = line.map_err(|error| {
                ReplayStoreError(format!(
                    "read replay store {} line {}: {error}",
                    path.display(),
                    index + 1
                ))
            })?;
            if line.trim().is_empty() {
                continue;
            }
            let record: DurableReplayRecord = serde_json::from_str(&line).map_err(|error| {
                ReplayStoreError(format!(
                    "parse replay store {} line {}: {error}",
                    path.display(),
                    index + 1
                ))
            })?;
            if record.envelope_kind != "OIA1" || record.issuer_key_id.is_empty() {
                return Err(ReplayStoreError(format!(
                    "invalid replay-store domain at {} line {}",
                    path.display(),
                    index + 1
                )));
            }
            let nonce = hex::decode(&record.nonce_hex)
                .ok()
                .and_then(|bytes| <[u8; 16]>::try_from(bytes).ok())
                .ok_or_else(|| {
                    ReplayStoreError(format!(
                        "invalid replay nonce at {} line {}",
                        path.display(),
                        index + 1
                    ))
                })?;
            let key = ReplayKey {
                issuer_key_id: record.issuer_key_id,
                nonce,
            };
            seen.entry(key)
                .and_modify(|expires| *expires = (*expires).max(record.expires_at))
                .or_insert(record.expires_at);
        }
        file.seek(SeekFrom::End(0)).map_err(|error| {
            ReplayStoreError(format!("seek replay store {}: {error}", path.display()))
        })?;

        Ok(Self {
            backend: ReplayBackend::Durable(Mutex::new(DurableReplayState {
                file,
                seen,
                failure: None,
            })),
            leeway_secs,
        })
    }

    fn unavailable(error: impl Into<String>, leeway_secs: i64) -> Self {
        Self {
            backend: ReplayBackend::Unavailable(error.into()),
            leeway_secs,
        }
    }

    fn replay_key(issuer_key_id: &str, nonce: [u8; 16]) -> ReplayKey {
        ReplayKey {
            issuer_key_id: issuer_key_id.to_string(),
            nonce,
        }
    }

    fn retain_unexpired(seen: &mut HashMap<ReplayKey, i64>, leeway_secs: i64, now: i64) {
        seen.retain(|_, entry_expires| *entry_expires + leeway_secs >= now);
    }

    /// Non-mutating existence lookup. Expired entries do not count as replay;
    /// durable cleanup is intentionally deferred so lookup cannot mutate disk.
    pub fn contains(
        &self,
        issuer_key_id: &str,
        nonce: [u8; 16],
        now: i64,
    ) -> Result<bool, ReplayStoreError> {
        let key = Self::replay_key(issuer_key_id, nonce);
        let active = |seen: &HashMap<ReplayKey, i64>| {
            seen.get(&key)
                .is_some_and(|expires| *expires + self.leeway_secs >= now)
        };
        match &self.backend {
            ReplayBackend::Memory(seen) => {
                let seen = seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                Ok(active(&seen))
            }
            ReplayBackend::Durable(state) => {
                let state = state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(error) = &state.failure {
                    return Err(ReplayStoreError(error.clone()));
                }
                Ok(active(&state.seen))
            }
            ReplayBackend::Unavailable(error) => Err(ReplayStoreError(error.clone())),
        }
    }

    /// Atomically check and consume one issuer-scoped nonce. Returns `false`
    /// for replay and an error for any persistence failure.
    pub fn check_and_insert_for_issuer(
        &self,
        issuer_key_id: &str,
        nonce: [u8; 16],
        expires_at: i64,
        now: i64,
    ) -> Result<bool, ReplayStoreError> {
        let key = Self::replay_key(issuer_key_id, nonce);
        match &self.backend {
            ReplayBackend::Memory(seen) => {
                let mut seen = seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                Self::retain_unexpired(&mut seen, self.leeway_secs, now);
                if seen.contains_key(&key) {
                    return Ok(false);
                }
                seen.insert(key, expires_at);
                Ok(true)
            }
            ReplayBackend::Durable(state) => {
                let mut state = state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if let Some(error) = &state.failure {
                    return Err(ReplayStoreError(error.clone()));
                }
                Self::retain_unexpired(&mut state.seen, self.leeway_secs, now);
                if state.seen.contains_key(&key) {
                    return Ok(false);
                }
                let record = DurableReplayRecord {
                    envelope_kind: "OIA1".to_string(),
                    issuer_key_id: issuer_key_id.to_string(),
                    nonce_hex: hex::encode(nonce),
                    expires_at,
                };
                let result = (|| -> Result<(), ReplayStoreError> {
                    serde_json::to_writer(&mut state.file, &record).map_err(|error| {
                        ReplayStoreError(format!("append replay record: {error}"))
                    })?;
                    state.file.write_all(b"\n").map_err(|error| {
                        ReplayStoreError(format!("terminate replay record: {error}"))
                    })?;
                    state.file.sync_data().map_err(|error| {
                        ReplayStoreError(format!("sync replay record: {error}"))
                    })?;
                    Ok(())
                })();
                if let Err(error) = result {
                    state.failure = Some(error.to_string());
                    return Err(error);
                }
                state.seen.insert(key, expires_at);
                Ok(true)
            }
            ReplayBackend::Unavailable(error) => Err(ReplayStoreError(error.clone())),
        }
    }

    /// Compatibility helper for low-level cache tests.
    pub fn check_and_insert(
        &self,
        nonce: [u8; 16],
        expires_at: i64,
        now: i64,
    ) -> Result<bool, ReplayStoreError> {
        self.check_and_insert_for_issuer("test-issuer", nonce, expires_at, now)
    }

    pub fn entry_count(&self) -> usize {
        match &self.backend {
            ReplayBackend::Memory(seen) => seen
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            ReplayBackend::Durable(state) => state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .seen
                .len(),
            ReplayBackend::Unavailable(_) => 0,
        }
    }
}

#[derive(Debug)]
pub(crate) struct PendingAssertion {
    identity: HumanPrincipalIdentity,
    verified_origin: VerifiedWireGuardOrigin,
    issuer_key_id: String,
    nonce: [u8; 16],
    expires_at: i64,
    now: i64,
    bound_oracle: Option<BoundOracleProof>,
}

impl PendingAssertion {
    pub(crate) fn verified_origin(&self) -> &VerifiedWireGuardOrigin {
        &self.verified_origin
    }

    pub(crate) fn identity(&self) -> &HumanPrincipalIdentity {
        &self.identity
    }

    pub(crate) fn identity_mut(&mut self) -> &mut HumanPrincipalIdentity {
        &mut self.identity
    }

    pub(crate) fn bound_oracle(&self) -> Option<&BoundOracleProof> {
        self.bound_oracle.as_ref()
    }
}

/// Validates oracle identity assertions end-to-end.
#[derive(Debug)]
pub struct AssertionValidator {
    trust_store: DecoyTrustStore,
    replay_cache: AssertionReplayCache,
    leeway_secs: i64,
    max_lifetime_secs: i64,
    source_binding: AssertionSourceBinding,
    browser_binding: BrowserBindingPolicy,
}

impl AssertionValidator {
    #[cfg(test)]
    pub(crate) fn with_browser_binding(
        mut self,
        peer: &str,
        principal: &str,
        session: &str,
    ) -> Self {
        self.browser_binding =
            BrowserBindingPolicy::parse(Some(peer), Some(principal), Some(session));
        self
    }
    /// Test and explicit-construction helper. Source binding is
    /// `ExactPeerIp` so IP-equality unit tests do not all have to opt in.
    /// Production always uses [`Self::from_env`], which defaults to
    /// `trusted-decoy-signature`.
    pub fn new(trust_store: DecoyTrustStore) -> Self {
        Self {
            trust_store,
            replay_cache: AssertionReplayCache::new(CLOCK_LEEWAY_SECS),
            leeway_secs: CLOCK_LEEWAY_SECS,
            max_lifetime_secs: MAX_LIFETIME_SECS as i64,
            source_binding: AssertionSourceBinding::ExactPeerIp,
            browser_binding: BrowserBindingPolicy::Disabled,
        }
    }

    /// Construct the production validator, reading the transport-binding
    /// selector. Missing or unknown `OP_ORACLE_ASSERTION_SOURCE_BINDING` uses
    /// `trusted-decoy-signature` (the live fabric binding). `exact-peer-ip`
    /// remains available as an explicit legacy opt-in.
    pub fn from_env(trust_store: DecoyTrustStore) -> Self {
        let path = std::env::var(AUTH_REPLAY_DB_PATH_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(DEFAULT_AUTH_REPLAY_DB_PATH));
        let replay_cache = match AssertionReplayCache::persistent(&path, CLOCK_LEEWAY_SECS) {
            Ok(cache) => cache,
            Err(error) => {
                tracing::error!(path = %path.display(), %error, "durable auth replay store unavailable");
                AssertionReplayCache::unavailable(error.to_string(), CLOCK_LEEWAY_SECS)
            }
        };
        let mut validator = Self::new(trust_store)
            .with_replay_cache(replay_cache)
            .with_source_binding(AssertionSourceBinding::from_env());
        validator.browser_binding = BrowserBindingPolicy::from_env();
        validator
    }

    pub fn with_source_binding(mut self, source_binding: AssertionSourceBinding) -> Self {
        self.source_binding = source_binding;
        self
    }

    pub fn with_replay_cache(mut self, replay_cache: AssertionReplayCache) -> Self {
        self.replay_cache = replay_cache;
        self
    }

    pub fn trust_store(&self) -> &DecoyTrustStore {
        &self.trust_store
    }

    pub fn replay_cache(&self) -> &AssertionReplayCache {
        &self.replay_cache
    }

    /// Contractual pipeline with clock injection (`now` = unix seconds).
    pub fn validate(
        &self,
        wire: &[u8],
        source: Option<SocketAddr>,
        now: i64,
    ) -> Result<HumanPrincipalIdentity, AssertionRejection> {
        self.validate_with_bootstrap(wire, source, now, false)
    }

    /// Like [`validate`], but allows synthesizing identity for an unregistered
    /// pubkey when `registration_bootstrap` is true (declared
    /// `human_principal.write` on `register_key`).
    pub fn validate_with_bootstrap(
        &self,
        wire: &[u8],
        source: Option<SocketAddr>,
        now: i64,
        registration_bootstrap: bool,
    ) -> Result<HumanPrincipalIdentity, AssertionRejection> {
        let pending =
            self.validate_pending_with_bootstrap(wire, source, now, registration_bootstrap)?;
        self.consume_pending(pending)
    }

    pub(crate) fn validate_pending_with_bootstrap(
        &self,
        wire: &[u8],
        source: Option<SocketAddr>,
        now: i64,
        registration_bootstrap: bool,
    ) -> Result<PendingAssertion, AssertionRejection> {
        self.validate_pending(wire, source, now, registration_bootstrap, true)
    }

    /// External MCP represents the verified WireGuard principal itself.
    /// Browser-to-chatbot assignments cannot supply a different identity or
    /// bootstrap an unregistered key on this transport.
    pub(crate) fn validate_pending_for_mcp(
        &self,
        wire: &[u8],
        source: Option<SocketAddr>,
        now: i64,
    ) -> Result<PendingAssertion, AssertionRejection> {
        self.validate_pending(wire, source, now, false, false)
    }

    fn validate_pending(
        &self,
        wire: &[u8],
        source: Option<SocketAddr>,
        now: i64,
        registration_bootstrap: bool,
        allow_browser_binding: bool,
    ) -> Result<PendingAssertion, AssertionRejection> {
        let signed = match SignedAssertion::from_wire(wire) {
            Ok(value) => value,
            Err(_) => return Err(AssertionRejection::Malformed),
        };
        if signed.assertion.expires_at <= signed.assertion.issued_at {
            return Err(AssertionRejection::Malformed);
        }

        let verifying_key = match self
            .trust_store
            .verifying_key(&signed.assertion.decoy_key_id)
        {
            Some(key) => key,
            None => return Err(AssertionRejection::UnknownDecoyKey),
        };

        if verify_signature(&signed.assertion, &signed.signature, verifying_key).is_err() {
            return Err(AssertionRejection::BadSignature);
        }

        let assertion = &signed.assertion;
        if assertion.issued_at > now + self.leeway_secs {
            return Err(AssertionRejection::NotYetValid);
        }
        if now > assertion.expires_at + self.leeway_secs {
            return Err(AssertionRejection::Expired);
        }
        if assertion.expires_at - assertion.issued_at > self.max_lifetime_secs {
            return Err(AssertionRejection::LifetimeTooLong);
        }

        if self
            .replay_cache
            .contains(&assertion.decoy_key_id, assertion.nonce, now)
            .map_err(|_| AssertionRejection::ReplayStoreUnavailable)?
        {
            return Err(AssertionRejection::Replay);
        }

        if self.source_binding == AssertionSourceBinding::ExactPeerIp {
            let peer = match source {
                Some(addr) => addr,
                None => return Err(AssertionRejection::MissingConnectInfo),
            };
            if peer.ip() != assertion.tunnel_inner_ip {
                return Err(AssertionRejection::SourceIpMismatch {
                    expected: assertion.tunnel_inner_ip,
                    actual: peer.ip(),
                });
            }
        }

        let assigned = match &self.browser_binding {
            _ if !allow_browser_binding => None,
            BrowserBindingPolicy::Invalid => {
                return Err(AssertionRejection::InvalidBrowserIdentityBinding);
            }
            BrowserBindingPolicy::Assigned(binding)
                if binding.peer_key == assertion.human_pubkey =>
            {
                Some(binding)
            }
            _ => None,
        };
        let identity = self.resolve_principal(
            &assertion.human_pubkey,
            assertion.expires_at,
            registration_bootstrap || assigned.is_some(),
        )?;
        let verified_origin = VerifiedWireGuardOrigin {
            principal_id: identity.principal_id.clone(),
            peer_key: assertion.human_pubkey.clone(),
            issuer_key_id: assertion.decoy_key_id.clone(),
            signed_inner_ip: assertion.tunnel_inner_ip,
            assertion_digest: blake3::hash(wire).to_hex().to_string(),
            expires_at: assertion.expires_at,
        };
        let bound_oracle = assigned.map(|binding| BoundOracleProof {
            binding: binding.clone(),
            assertion_digest: blake3::hash(wire).to_hex().to_string(),
            issuer_key_id: assertion.decoy_key_id.clone(),
            signed_inner_ip: assertion.tunnel_inner_ip.to_string(),
            expires_at: assertion.expires_at,
            target_credential_digest: None,
        });
        Ok(PendingAssertion {
            identity,
            verified_origin,
            issuer_key_id: assertion.decoy_key_id.clone(),
            nonce: assertion.nonce,
            expires_at: assertion.expires_at,
            now,
            bound_oracle,
        })
    }

    pub(crate) fn consume_pending(
        &self,
        pending: PendingAssertion,
    ) -> Result<HumanPrincipalIdentity, AssertionRejection> {
        if !self
            .replay_cache
            .check_and_insert_for_issuer(
                &pending.issuer_key_id,
                pending.nonce,
                pending.expires_at,
                pending.now,
            )
            .map_err(|_| AssertionRejection::ReplayStoreUnavailable)?
        {
            return Err(AssertionRejection::Replay);
        }
        Ok(pending.identity)
    }

    fn resolve_principal(
        &self,
        human_pubkey: &str,
        expires_at: i64,
        registration_bootstrap: bool,
    ) -> Result<HumanPrincipalIdentity, AssertionRejection> {
        let record = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(
                crate::human_principal_dispatch::resolve_key_for_assertion(human_pubkey),
            )
        })
        .map_err(|_| AssertionRejection::RegistryUnavailable)?;

        let Some(record) = record else {
            if registration_bootstrap {
                return Ok(HumanPrincipalIdentity {
                    principal_id: derive_principal_id(human_pubkey),
                    human_pubkey: human_pubkey.to_string(),
                    session_id: String::new(),
                    expires_at,
                    session_genesis: String::new(),
                });
            }
            return Err(AssertionRejection::UnknownPrincipal);
        };
        if record.revoked_at != 0 {
            return Err(AssertionRejection::RevokedPrincipal);
        }

        Ok(HumanPrincipalIdentity {
            principal_id: record.principal_id,
            human_pubkey: human_pubkey.to_string(),
            session_id: String::new(),
            expires_at,
            session_genesis: String::new(),
        })
    }
}

fn decoy_keys_object_has_duplicate_keys(raw: &str) -> bool {
    let Some(marker) = raw.find("\"decoy_keys\"") else {
        return false;
    };
    let tail = &raw[marker..];
    let Some(open) = tail.find('{') else {
        return false;
    };
    let keys = extract_json_object_string_keys(&tail[open..]);
    let mut seen = HashSet::new();
    keys.iter().any(|key| !seen.insert(key.clone()))
}

fn extract_json_object_string_keys(object_with_brace: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escaped = false;
    let mut current = String::new();
    let mut capturing = false;

    for ch in object_with_brace.chars() {
        if in_string {
            if escaped {
                current.push(ch);
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => {
                    in_string = false;
                    if capturing && depth == 1 {
                        keys.push(std::mem::take(&mut current));
                        capturing = false;
                    }
                }
                _ => current.push(ch),
            }
            continue;
        }

        match ch {
            '{' => {
                depth += 1;
                if depth == 1 {
                    continue;
                }
            }
            '}' => {
                if depth == 1 {
                    break;
                }
                depth -= 1;
            }
            '"' if depth == 1 => {
                in_string = true;
                capturing = true;
                current.clear();
            }
            _ => {}
        }
    }
    keys
}

#[cfg(test)]
pub mod tests {
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
    use std::time::Duration;

    use base64::Engine as _;
    use ed25519_dalek::{Signer, SigningKey};
    use op_identity::oracle_assertion::{DecoyIssuer, OracleIdentityAssertion, SignedAssertion};
    use op_identity::session::{derive_principal_id, derive_session_id};
    use tonic::Code;

    use super::*;
    use crate::human_principal_dispatch::tests::{TempCozo, pk, register, revoke, temp_cozo};

    const TEST_KEY_BYTES: [u8; 32] = [7u8; 32];
    const SAMPLE_PUBKEY_LOCAL: &str = "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=";

    #[tokio::test(flavor = "multi_thread")]
    async fn external_mcp_origin_is_stable_on_renewal_and_separates_peer_and_allowed_ip() {
        let issuer = test_issuer();
        let peer = pk(111);
        let (_cozo, validator) = validator_with_registered(&issuer, &peer).await;
        let validator =
            validator.with_source_binding(AssertionSourceBinding::TrustedDecoySignature);
        register(&pk(112), "second-external-peer").await.unwrap();
        let now = 1_700_000_100;
        let origin_for = |key: &str, ip: IpAddr, nonce: u8| {
            let proof = signed_with_fields(&issuer, key, ip, now - 5, now + 300, [nonce; 16], None);
            validator
                .validate_pending_for_mcp(&proof.to_wire(), None, now)
                .unwrap()
                .verified_origin()
                .clone()
        };
        let first = origin_for(&peer, "10.0.0.5".parse().unwrap(), 1);
        let renewed = origin_for(&peer, "10.0.0.5".parse().unwrap(), 2);
        assert_eq!(first.origin(), renewed.origin());
        assert_ne!(first.assertion_digest(), renewed.assertion_digest());
        assert_eq!(first.allowed_ip_scope(), "10.0.0.5/32");
        assert_eq!(first.principal_id(), derive_principal_id(&peer));
        assert_ne!(
            first.origin(),
            origin_for(&pk(112), "10.0.0.5".parse().unwrap(), 3).origin()
        );
        assert_ne!(
            first.origin(),
            origin_for(&peer, "10.0.0.6".parse().unwrap(), 4).origin()
        );
        let ipv6 = origin_for(&peer, "fd00::5".parse().unwrap(), 5);
        assert_eq!(ipv6.allowed_ip_scope(), "fd00::5/128");
        assert_ne!(first.origin(), ipv6.origin());
        assert!(!first.origin().contains(&peer));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn external_mcp_never_uses_browser_assignment_to_bootstrap_or_change_principal() {
        let _cozo = temp_cozo();
        let issuer = test_issuer();
        let peer = pk(113);
        let target = pk(114);
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer))
            .with_browser_binding(
                &peer,
                &derive_principal_id(&target),
                &derive_session_id(&target),
            );
        let now = 1_700_000_100;
        let proof =
            signed_with_fields(&issuer, &peer, test_ip(), now - 5, now + 300, [6; 16], None);
        let wire = proof.to_wire();
        assert!(matches!(
            validator.validate_pending_for_mcp(&wire, Some(source_at(test_ip())), now),
            Err(AssertionRejection::UnknownPrincipal)
        ));
        register(&peer, "external-peer").await.unwrap();
        let pending = validator
            .validate_pending_for_mcp(&wire, Some(source_at(test_ip())), now)
            .unwrap();
        assert!(pending.bound_oracle().is_none());
        assert_eq!(pending.identity().principal_id, derive_principal_id(&peer));
        validator.consume_pending(pending).unwrap();
        assert!(matches!(
            validator.validate_pending_for_mcp(&wire, Some(source_at(test_ip())), now),
            Err(AssertionRejection::Replay)
        ));
        revoke(&peer).await.unwrap();
        let fresh =
            signed_with_fields(&issuer, &peer, test_ip(), now - 5, now + 300, [7; 16], None);
        assert!(matches!(
            validator.validate_pending_for_mcp(&fresh.to_wire(), Some(source_at(test_ip())), now),
            Err(AssertionRejection::RevokedPrincipal)
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn external_mcp_origin_requires_authentic_current_signed_scope() {
        let issuer = test_issuer();
        let peer = pk(115);
        let (_cozo, validator) = validator_with_registered(&issuer, &peer).await;
        let now = 1_700_000_100;
        let mut proof =
            signed_with_fields(&issuer, &peer, test_ip(), now - 5, now + 300, [8; 16], None);
        proof.assertion.tunnel_inner_ip = "10.0.0.9".parse().unwrap();
        assert!(matches!(
            validator.validate_pending_for_mcp(&proof.to_wire(), Some(source_at(test_ip())), now),
            Err(AssertionRejection::BadSignature)
        ));
        let expired = signed_with_fields(
            &issuer,
            &peer,
            test_ip(),
            now - 300,
            now - 100,
            [9; 16],
            None,
        );
        assert!(matches!(
            validator.validate_pending_for_mcp(&expired.to_wire(), Some(source_at(test_ip())), now),
            Err(AssertionRejection::Expired)
        ));
    }

    #[test]
    fn browser_binding_config_is_single_canonical_assignment_or_fail_closed() {
        let principal = "1262e982-14c1-44d4-0771-7ce74213211b";
        let session = "b0c777e3-0955-cd8b-3d01-ca12998cae28";
        assert!(matches!(
            BrowserBindingPolicy::parse(None, None, None),
            BrowserBindingPolicy::Disabled
        ));
        assert!(matches!(
            BrowserBindingPolicy::parse(Some(SAMPLE_PUBKEY_LOCAL), Some(principal), Some(session)),
            BrowserBindingPolicy::Assigned(_)
        ));
        for peer in [
            "".to_owned(),
            "bad".to_owned(),
            format!(" {SAMPLE_PUBKEY_LOCAL}"),
            format!("{SAMPLE_PUBKEY_LOCAL},{SAMPLE_PUBKEY_LOCAL}"),
        ] {
            assert!(matches!(
                BrowserBindingPolicy::parse(Some(&peer), Some(principal), Some(session)),
                BrowserBindingPolicy::Invalid
            ));
        }
        for (principal, session) in [
            (None, Some(session)),
            (Some(principal), None),
            (Some("00000000-0000-0000-0000-000000000000"), Some(session)),
            (Some(principal), Some("not-a-session")),
        ] {
            assert!(matches!(
                BrowserBindingPolicy::parse(Some(SAMPLE_PUBKEY_LOCAL), principal, session),
                BrowserBindingPolicy::Invalid
            ));
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn browser_binding_verifies_transport_before_exact_peer_selection_and_replay() {
        let _cozo = temp_cozo();
        let issuer = test_issuer();
        let peer = pk(101);
        let target_key = pk(102);
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer))
            .with_browser_binding(
                &peer,
                &derive_principal_id(&target_key),
                &derive_session_id(&target_key),
            );
        let now = 1_700_000_100;
        let signed = signed_with_fields(
            &issuer,
            &peer,
            test_ip(),
            now - 5,
            now + 300,
            [101; 16],
            None,
        );
        let pending = validator
            .validate_pending_with_bootstrap(
                &signed.to_wire(),
                Some(source_at(test_ip())),
                now,
                false,
            )
            .unwrap();
        let proof = pending.bound_oracle().unwrap();
        assert_eq!(proof.peer_key(), peer);
        assert_eq!(proof.binding().session_id, derive_session_id(&target_key));
        assert!(!proof.audit_value().to_string().contains("SID1"));
        validator.consume_pending(pending).unwrap();
        assert_eq!(
            validator.validate(&signed.to_wire(), Some(source_at(test_ip())), now),
            Err(AssertionRejection::Replay)
        );
        let foreign = signed_with_fields(
            &issuer,
            &pk(103),
            test_ip(),
            now - 5,
            now + 300,
            [103; 16],
            None,
        );
        assert_eq!(
            validator.validate(&foreign.to_wire(), Some(source_at(test_ip())), now),
            Err(AssertionRejection::UnknownPrincipal)
        );
        let mut forged = signed.clone();
        forged.signature[0] ^= 1;
        assert_eq!(
            validator.validate(&forged.to_wire(), Some(source_at(test_ip())), now),
            Err(AssertionRejection::BadSignature)
        );
        let expired = signed_with_fields(
            &issuer,
            &peer,
            test_ip(),
            now - 300,
            now - 100,
            [104; 16],
            None,
        );
        assert_eq!(
            validator.validate(&expired.to_wire(), Some(source_at(test_ip())), now),
            Err(AssertionRejection::Expired)
        );
        let fresh = signed_with_fields(
            &issuer,
            &peer,
            test_ip(),
            now - 5,
            now + 300,
            [105; 16],
            None,
        );
        assert!(matches!(
            validator.validate(
                &fresh.to_wire(),
                Some(source_at("127.0.0.2".parse().unwrap())),
                now
            ),
            Err(AssertionRejection::SourceIpMismatch { .. })
        ));
        register(&peer, "assigned-peer").await.unwrap();
        revoke(&peer).await.unwrap();
        assert_eq!(
            validator.validate(&fresh.to_wire(), Some(source_at(test_ip())), now),
            Err(AssertionRejection::RevokedPrincipal)
        );
        register(&pk(103), "ordinary-unbound-peer").await.unwrap();
        let ordinary = validator
            .validate_pending_with_bootstrap(
                &foreign.to_wire(),
                Some(source_at(test_ip())),
                now,
                false,
            )
            .unwrap();
        assert!(ordinary.bound_oracle().is_none());
        assert_eq!(
            ordinary.identity().principal_id,
            derive_principal_id(&pk(103))
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn browser_binding_cannot_turn_registry_failure_into_unregistered_admission() {
        let _cozo = temp_cozo();
        let blocker = tempfile::NamedTempFile::new().unwrap();
        unsafe {
            std::env::set_var(
                "OP_HUMAN_PRINCIPAL_COZO_DB_PATH",
                blocker.path().join("cozo"),
            )
        };
        let issuer = test_issuer();
        let peer = pk(106);
        let target = pk(107);
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer))
            .with_browser_binding(
                &peer,
                &derive_principal_id(&target),
                &derive_session_id(&target),
            );
        let now = 1_700_000_100;
        let signed = signed_with_fields(
            &issuer,
            &peer,
            test_ip(),
            now - 5,
            now + 300,
            [106; 16],
            None,
        );
        assert_eq!(
            validator.validate(&signed.to_wire(), Some(source_at(test_ip())), now),
            Err(AssertionRejection::RegistryUnavailable)
        );
    }

    pub(crate) fn test_signing_key() -> SigningKey {
        SigningKey::from_bytes(&TEST_KEY_BYTES)
    }

    pub(crate) fn test_issuer() -> DecoyIssuer {
        DecoyIssuer::new(test_signing_key(), "decoy-key-1", Duration::from_secs(900))
    }

    pub(crate) fn test_ip() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
    }

    pub(crate) fn trust_store_for_issuer(issuer: &DecoyIssuer) -> DecoyTrustStore {
        let mut keys = HashMap::new();
        keys.insert(issuer.key_id().to_string(), *issuer.verifying_key());
        DecoyTrustStore::from_decoy_keys(keys)
    }

    #[test]
    fn source_binding_defaults_to_trusted_decoy_not_netmaker_ip() {
        assert_eq!(
            AssertionSourceBinding::parse(Err(std::env::VarError::NotPresent)),
            AssertionSourceBinding::TrustedDecoySignature
        );
        assert_eq!(
            AssertionSourceBinding::parse(Ok("trusted-decoy-signature".into())),
            AssertionSourceBinding::TrustedDecoySignature
        );
        assert_eq!(
            AssertionSourceBinding::parse(Ok("exact-peer-ip".into())),
            AssertionSourceBinding::ExactPeerIp
        );
        assert_eq!(
            AssertionSourceBinding::parse(Ok("not-a-binding".into())),
            AssertionSourceBinding::TrustedDecoySignature
        );
    }

    fn write_trust_store(dir: &tempfile::TempDir, issuer: &DecoyIssuer) -> std::path::PathBuf {
        let b64 =
            base64::engine::general_purpose::STANDARD.encode(issuer.verifying_key().to_bytes());
        let json = format!(
            "{{\"decoy_keys\": {{\"{}\": \"{}\"}}}}",
            issuer.key_id(),
            b64
        );
        let path = dir.path().join("decoy-trust.json");
        std::fs::write(&path, json).expect("write trust store");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_DECOY_TRUST_STORE", &path) };
        path
    }

    pub(crate) fn signed_with_fields(
        issuer: &DecoyIssuer,
        human_pubkey: &str,
        inner_ip: IpAddr,
        issued_at: i64,
        expires_at: i64,
        nonce: [u8; 16],
        decoy_key_id: Option<&str>,
    ) -> SignedAssertion {
        let assertion = OracleIdentityAssertion {
            human_pubkey: human_pubkey.to_string(),
            issued_at,
            expires_at,
            nonce,
            tunnel_inner_ip: inner_ip,
            decoy_key_id: decoy_key_id.unwrap_or(issuer.key_id()).to_string(),
        };
        let signature = test_signing_key()
            .sign(&assertion.signing_bytes())
            .to_bytes();
        SignedAssertion {
            assertion,
            signature,
        }
    }

    fn signed_with_signing_key(
        signing_key: &SigningKey,
        mut signed: SignedAssertion,
    ) -> SignedAssertion {
        signed.signature = signing_key
            .sign(&signed.assertion.signing_bytes())
            .to_bytes();
        signed
    }

    pub(crate) async fn validator_with_registered(
        issuer: &DecoyIssuer,
        pubkey: &str,
    ) -> (TempCozo, AssertionValidator) {
        let cozo = temp_cozo();
        register(pubkey, "test").await.expect("register");
        (
            cozo,
            AssertionValidator::new(trust_store_for_issuer(issuer)),
        )
    }

    pub(crate) fn source_at(ip: IpAddr) -> SocketAddr {
        SocketAddr::new(ip, 12345)
    }

    /// VAL-BRIDGE-001
    #[tokio::test(flavor = "multi_thread")]
    async fn trust_store_loads_valid_json() {
        let _cozo = temp_cozo();
        let issuer = test_issuer();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_trust_store(&dir, &issuer);
        let store = DecoyTrustStore::load_from_path(&path);
        assert!(store.contains_key(issuer.key_id()));

        let mut signed = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0xA5; 16],
            None,
        );
        let wire = signed.to_wire();
        let validator = AssertionValidator::new(store);
        let err = validator.validate(&wire, Some(source_at(test_ip())), 1_700_000_100);
        assert_ne!(err, Err(AssertionRejection::UnknownDecoyKey));
        assert_eq!(err, Err(AssertionRejection::UnknownPrincipal));

        signed.signature[0] ^= 0x01;
        let bad = signed.to_wire();
        assert_eq!(
            validator.validate(&bad, Some(source_at(test_ip())), 1_700_000_100),
            Err(AssertionRejection::BadSignature)
        );
    }

    /// VAL-BRIDGE-002
    #[tokio::test(flavor = "multi_thread")]
    async fn trust_store_missing_file_fails_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("missing.json");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("OP_DECOY_TRUST_STORE", &missing) };
        let store = DecoyTrustStore::load();
        assert_eq!(store.key_count(), 0);

        let issuer = test_issuer();
        let signed = issuer
            .issue(SAMPLE_PUBKEY_LOCAL, test_ip(), Duration::from_secs(300))
            .expect("issue");
        let validator = AssertionValidator::new(store);
        assert_eq!(
            validator.validate(
                &signed.to_wire(),
                Some(source_at(test_ip())),
                signed.assertion.issued_at + 1
            ),
            Err(AssertionRejection::UnknownDecoyKey)
        );
    }

    /// VAL-BRIDGE-003
    #[tokio::test(flavor = "multi_thread")]
    async fn trust_store_malformed_json_fails_closed() {
        let issuer = test_issuer();
        let good_b64 =
            base64::engine::general_purpose::STANDARD.encode(issuer.verifying_key().to_bytes());
        let dir = tempfile::tempdir().expect("tempdir");
        let wrong_len = format!(
            "{{\"decoy_keys\": {{\"decoy-key-1\": \"{}\"}}}}",
            base64::engine::general_purpose::STANDARD.encode([1u8; 16])
        );
        let duplicate = format!(
            "{{\"decoy_keys\": {{\"decoy-key-1\": \"{}\", \"decoy-key-1\": \"{}\"}}}}",
            good_b64, good_b64
        );
        let variants: Vec<(&str, Vec<u8>)> = vec![
            ("empty", b"".to_vec()),
            ("invalid json", b"{not json".to_vec()),
            ("wrong type array", br#"{"decoy_keys": []}"#.to_vec()),
            ("wrong type string", br#"{"decoy_keys": "x"}"#.to_vec()),
            ("wrong length key", wrong_len.into_bytes()),
            (
                "bad base64",
                br#"{"decoy_keys": {"decoy-key-1": "!!!"}}"#.to_vec(),
            ),
            ("duplicate key id", duplicate.into_bytes()),
        ];
        for (label, bytes) in &variants {
            let path = dir.path().join(format!("bad-{label}.json"));
            std::fs::write(&path, bytes).expect("write variant");
            let store = DecoyTrustStore::load_from_path(&path);
            assert_eq!(store.key_count(), 0, "variant {label} must load empty");
        }
    }

    /// VAL-BRIDGE-004
    #[tokio::test(flavor = "multi_thread")]
    async fn validate_accepts_fully_valid_assertion() {
        let issuer = test_issuer();
        let pubkey = pk(1);
        let (_cozo, validator) = validator_with_registered(&issuer, &pubkey).await;
        let signed = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x01; 16],
            None,
        );
        let identity = validator
            .validate(&signed.to_wire(), Some(source_at(test_ip())), 1_700_000_100)
            .expect("valid assertion");
        assert_eq!(identity.human_pubkey, pubkey);
        assert_eq!(identity.expires_at, signed.assertion.expires_at);
    }

    /// VAL-BRIDGE-005
    #[tokio::test(flavor = "multi_thread")]
    async fn validate_rejects_malformed_envelope() {
        let issuer = test_issuer();
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer));
        let valid = issuer
            .issue(SAMPLE_PUBKEY_LOCAL, test_ip(), Duration::from_secs(60))
            .unwrap()
            .to_wire();
        let mut bad_magic = valid.clone();
        bad_magic[0] = b'X';
        let mut truncated = valid.clone();
        truncated.truncate(valid.len() - 10);
        let mut trailing = valid.clone();
        trailing.push(0xFF);
        let mut bad_len = valid.clone();
        bad_len[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        for sample in [&[] as &[u8], &bad_magic, &truncated, &trailing, &bad_len] {
            assert_eq!(
                validator.validate(sample, Some(source_at(test_ip())), 1_700_000_000),
                Err(AssertionRejection::Malformed)
            );
        }
    }

    /// VAL-BRIDGE-006
    #[tokio::test(flavor = "multi_thread")]
    async fn validate_rejects_unknown_decoy_key() {
        let issuer = test_issuer();
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer));
        let signed = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x02; 16],
            Some("unknown-key"),
        );
        assert_eq!(
            validator.validate(&signed.to_wire(), Some(source_at(test_ip())), 1_700_000_100),
            Err(AssertionRejection::UnknownDecoyKey)
        );
    }

    /// VAL-BRIDGE-007
    #[tokio::test(flavor = "multi_thread")]
    async fn validate_rejects_bad_signature() {
        let issuer = test_issuer();
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer));
        let mut signed = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x03; 16],
            None,
        );
        signed.signature[0] ^= 0x01;
        assert_eq!(
            validator.validate(&signed.to_wire(), Some(source_at(test_ip())), 1_700_000_100),
            Err(AssertionRejection::BadSignature)
        );

        let other = SigningKey::from_bytes(&[9u8; 32]);
        let mut wrong_key = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x04; 16],
            None,
        );
        wrong_key.signature = other.sign(&wrong_key.assertion.signing_bytes()).to_bytes();
        assert_eq!(
            validator.validate(
                &wrong_key.to_wire(),
                Some(source_at(test_ip())),
                1_700_000_100
            ),
            Err(AssertionRejection::BadSignature)
        );
    }

    /// VAL-BRIDGE-008
    #[tokio::test(flavor = "multi_thread")]
    async fn validate_expiry_leeway_boundary() {
        let issuer = test_issuer();
        let pubkey = pk(2);
        let (_cozo, validator) = validator_with_registered(&issuer, &pubkey).await;
        let issued = 1_700_000_000i64;
        let expires = issued + 300;
        let signed = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            issued,
            expires,
            [0x05; 16],
            None,
        );
        let wire = signed.to_wire();
        assert_eq!(
            validator.validate(&wire, Some(source_at(test_ip())), expires + 31),
            Err(AssertionRejection::Expired)
        );
        validator
            .validate(&wire, Some(source_at(test_ip())), expires + 30)
            .expect("within leeway passes expiry");
    }

    /// VAL-BRIDGE-009
    #[tokio::test(flavor = "multi_thread")]
    async fn validate_rejects_future_issued_at_beyond_leeway() {
        let issuer = test_issuer();
        let pubkey = pk(3);
        let (_cozo, validator) = validator_with_registered(&issuer, &pubkey).await;
        let now = 1_700_000_000i64;
        let signed = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            now + 31,
            now + 331,
            [0x06; 16],
            None,
        );
        assert_eq!(
            validator.validate(&signed.to_wire(), Some(source_at(test_ip())), now),
            Err(AssertionRejection::NotYetValid)
        );
        validator
            .validate(
                &signed_with_fields(
                    &issuer,
                    &pubkey,
                    test_ip(),
                    now + 30,
                    now + 330,
                    [0x07; 16],
                    None,
                )
                .to_wire(),
                Some(source_at(test_ip())),
                now,
            )
            .expect("within future leeway passes expiry");
    }

    /// VAL-BRIDGE-010
    #[tokio::test(flavor = "multi_thread")]
    async fn validate_rejects_lifetime_over_900s() {
        let issuer = test_issuer();
        let pubkey = pk(4);
        let (_cozo, validator) = validator_with_registered(&issuer, &pubkey).await;
        let issued = 1_700_000_000i64;
        let over = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            issued,
            issued + 901,
            [0x08; 16],
            None,
        );
        assert_eq!(
            validator.validate(&over.to_wire(), Some(source_at(test_ip())), issued + 1),
            Err(AssertionRejection::LifetimeTooLong)
        );
        let exact = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            issued,
            issued + 900,
            [0x09; 16],
            None,
        );
        validator
            .validate(&exact.to_wire(), Some(source_at(test_ip())), issued + 1)
            .expect("exactly 900s accepted");
    }

    /// VAL-BRIDGE-011
    #[tokio::test(flavor = "multi_thread")]
    async fn validate_rejects_replayed_nonce() {
        let issuer = test_issuer();
        let pubkey = pk(5);
        let (_cozo, validator) = validator_with_registered(&issuer, &pubkey).await;
        let signed = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x0A; 16],
            None,
        );
        let wire = signed.to_wire();
        let now = 1_700_000_100;
        validator
            .validate(&wire, Some(source_at(test_ip())), now)
            .expect("first accepted");
        assert_eq!(
            validator.validate(&wire, Some(source_at(test_ip())), now),
            Err(AssertionRejection::Replay)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn durable_replay_survives_validator_reopen() {
        let issuer = test_issuer();
        let pubkey = pk(54);
        let _cozo = temp_cozo();
        register(&pubkey, "durable-replay")
            .await
            .expect("register principal");
        let dir = tempfile::tempdir().expect("replay dir");
        let path = dir.path().join("auth-replay.db");
        let signed = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x6A; 16],
            None,
        );
        let wire = signed.to_wire();
        let now = 1_700_000_100;

        {
            let replay = AssertionReplayCache::persistent(&path, CLOCK_LEEWAY_SECS)
                .expect("open durable replay store");
            let validator =
                AssertionValidator::new(trust_store_for_issuer(&issuer)).with_replay_cache(replay);
            validator
                .validate(&wire, Some(source_at(test_ip())), now)
                .expect("first process accepts nonce");
        }

        let replay = AssertionReplayCache::persistent(&path, CLOCK_LEEWAY_SECS)
            .expect("reopen durable replay store");
        let validator =
            AssertionValidator::new(trust_store_for_issuer(&issuer)).with_replay_cache(replay);
        assert_eq!(
            validator.validate(&wire, Some(source_at(test_ip())), now),
            Err(AssertionRejection::Replay)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path)
                    .expect("replay metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unavailable_replay_store_fails_closed() {
        let issuer = test_issuer();
        let blocker = tempfile::NamedTempFile::new().expect("blocking file");
        let bad_path = blocker.path().join("auth-replay.db");
        let error = AssertionReplayCache::persistent(&bad_path, CLOCK_LEEWAY_SECS)
            .expect_err("file parent cannot contain replay store");
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer)).with_replay_cache(
            AssertionReplayCache::unavailable(error.to_string(), CLOCK_LEEWAY_SECS),
        );
        let signed = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x6B; 16],
            None,
        );
        let rejection =
            validator.validate(&signed.to_wire(), Some(source_at(test_ip())), 1_700_000_100);
        assert_eq!(rejection, Err(AssertionRejection::ReplayStoreUnavailable));
        let status: tonic::Status = AssertionRejection::ReplayStoreUnavailable.into();
        assert_eq!(status.code(), Code::Unavailable);
    }

    /// VAL-BRIDGE-012
    #[tokio::test(flavor = "multi_thread")]
    async fn replay_cache_lazy_purge_no_background_task() {
        let cache = AssertionReplayCache::new(CLOCK_LEEWAY_SECS);
        let nonce = [0xBB; 16];
        let expires_at = 1_700_000_100i64;
        assert!(
            cache
                .check_and_insert(nonce, expires_at, 1_700_000_000)
                .expect("insert nonce")
        );
        assert_eq!(cache.entry_count(), 1);
        // TTL passes without cache access — entry remains (no background purge).
        assert_eq!(cache.entry_count(), 1);
        // The nonce remains active through the last second in which the
        // assertion itself is time-valid. Access one second later purges it.
        assert!(
            cache
                .check_and_insert(
                    [0xCC; 16],
                    expires_at + 1000,
                    expires_at + CLOCK_LEEWAY_SECS + 1
                )
                .expect("insert after purge")
        );
        assert_eq!(cache.entry_count(), 1, "expired nonce purged on access");
        assert!(
            cache
                .check_and_insert(nonce, expires_at + 1000, expires_at + CLOCK_LEEWAY_SECS + 1)
                .expect("reuse expired nonce")
        );
    }

    /// VAL-BRIDGE-013
    #[tokio::test(flavor = "multi_thread")]
    async fn validate_rejects_source_ip_mismatch() {
        let issuer = test_issuer();
        let pubkey = pk(6);
        let (_cozo, validator) = validator_with_registered(&issuer, &pubkey).await;
        let signed = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x0B; 16],
            None,
        );
        let now = 1_700_000_100;
        let wrong = source_at(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
        assert_eq!(
            validator.validate(&signed.to_wire(), Some(wrong), now),
            Err(AssertionRejection::SourceIpMismatch {
                expected: test_ip(),
                actual: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            })
        );
        validator
            .validate(&signed.to_wire(), Some(source_at(test_ip())), now)
            .expect("wrong source must not consume the nonce");
        let signed_mapped = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x1B; 16],
            None,
        );
        let mapped = source_at(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x7f00, 1)));
        assert_eq!(
            validator.validate(&signed_mapped.to_wire(), Some(mapped), now),
            Err(AssertionRejection::SourceIpMismatch {
                expected: test_ip(),
                actual: IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x7f00, 1)),
            })
        );
        let signed_port = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x2B; 16],
            None,
        );
        let different_port = SocketAddr::new(test_ip(), 9999);
        validator
            .validate(&signed_port.to_wire(), Some(different_port), now)
            .expect("port ignored");
    }

    /// VAL-BRIDGE-014
    #[tokio::test(flavor = "multi_thread")]
    async fn validate_rejects_missing_connect_info() {
        let issuer = test_issuer();
        let pubkey = pk(7);
        let (_cozo, validator) = validator_with_registered(&issuer, &pubkey).await;
        let signed = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x0C; 16],
            None,
        );
        assert_eq!(
            validator.validate(&signed.to_wire(), None, 1_700_000_100),
            Err(AssertionRejection::MissingConnectInfo)
        );
    }

    /// A NAT/proxy transport replaces the human inner address, so this mode
    /// binds the request to the trusted decoy signature instead of an
    /// impossible TCP source-address equality check.
    #[tokio::test(flavor = "multi_thread")]
    async fn trusted_decoy_binding_accepts_proxy_source_or_missing_connect_info() {
        let issuer = test_issuer();
        let pubkey = pk(27);
        let cozo = temp_cozo();
        register(&pubkey, "warp-user").await.expect("register");
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer))
            .with_source_binding(AssertionSourceBinding::TrustedDecoySignature);
        let now = 1_700_000_100;
        let proxied = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0xD1; 16],
            None,
        );
        validator
            .validate(
                &proxied.to_wire(),
                Some(source_at(IpAddr::V4(Ipv4Addr::new(104, 28, 196, 79)))),
                now,
            )
            .expect("trusted decoy signature survives WARP source NAT");

        let no_connect_info = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0xD2; 16],
            None,
        );
        validator
            .validate(&no_connect_info.to_wire(), None, now)
            .expect("proxy/UDS handoff may not expose TCP ConnectInfo");
        drop(cozo);
    }

    /// VAL-BRIDGE-015
    #[tokio::test(flavor = "multi_thread")]
    async fn validate_rejects_unknown_principal() {
        let _cozo = temp_cozo();
        let issuer = test_issuer();
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer));
        let signed = signed_with_fields(
            &issuer,
            &pk(8),
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x0D; 16],
            None,
        );
        assert_eq!(
            validator.validate(&signed.to_wire(), Some(source_at(test_ip())), 1_700_000_100),
            Err(AssertionRejection::UnknownPrincipal)
        );
    }

    /// VAL-BRIDGE-016
    #[tokio::test(flavor = "multi_thread")]
    async fn validate_rejects_revoked_principal() {
        let issuer = test_issuer();
        let pubkey = pk(9);
        let (_cozo, validator) = validator_with_registered(&issuer, &pubkey).await;
        let signed = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x0E; 16],
            None,
        );
        validator
            .validate(&signed.to_wire(), Some(source_at(test_ip())), 1_700_000_100)
            .expect("active passes");
        revoke(&pubkey).await.expect("revoke");
        let signed2 = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x0F; 16],
            None,
        );
        assert_eq!(
            validator.validate(
                &signed2.to_wire(),
                Some(source_at(test_ip())),
                1_700_000_100
            ),
            Err(AssertionRejection::RevokedPrincipal)
        );
    }

    /// VAL-BRIDGE-017
    #[tokio::test(flavor = "multi_thread")]
    async fn validate_rejects_when_registry_unavailable() {
        let _cozo = temp_cozo();
        let blocker = tempfile::NamedTempFile::new().expect("blocker");
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe {
            std::env::set_var(
                "OP_HUMAN_PRINCIPAL_COZO_DB_PATH",
                blocker.path().join("cozo"),
            )
        };
        let issuer = test_issuer();
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer));
        let signed = signed_with_fields(
            &issuer,
            &pk(10),
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x0F; 16],
            None,
        );
        assert_eq!(
            validator.validate(&signed.to_wire(), Some(source_at(test_ip())), 1_700_000_100),
            Err(AssertionRejection::RegistryUnavailable)
        );
    }

    /// VAL-BRIDGE-018
    #[tokio::test(flavor = "multi_thread")]
    async fn ordering_signature_before_expiry() {
        let issuer = test_issuer();
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer));
        let issued = 1_700_000_000i64;
        let mut expired_bad_sig = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            issued,
            issued + 300,
            [0x10; 16],
            None,
        );
        expired_bad_sig.signature[0] ^= 0x01;
        assert_eq!(
            validator.validate(
                &expired_bad_sig.to_wire(),
                Some(source_at(test_ip())),
                issued + 400
            ),
            Err(AssertionRejection::BadSignature)
        );
        let expired_ok_sig = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            issued,
            issued + 300,
            [0x11; 16],
            None,
        );
        assert_eq!(
            validator.validate(
                &expired_ok_sig.to_wire(),
                Some(source_at(test_ip())),
                issued + 400
            ),
            Err(AssertionRejection::Expired)
        );
    }

    /// VAL-BRIDGE-019
    #[tokio::test(flavor = "multi_thread")]
    async fn ordering_full_pipeline_multifault() {
        let issuer = test_issuer();
        let store = trust_store_for_issuer(&issuer);
        let cache = AssertionReplayCache::new(CLOCK_LEEWAY_SECS);
        let validator = AssertionValidator::new(store).with_replay_cache(cache);
        let issued = 1_700_000_000i64;

        // malformed + unknown key => Malformed (inverted lifetime in wire)
        let inverted = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            issued + 300,
            issued,
            [0x12; 16],
            Some("missing"),
        );
        assert_eq!(
            validator.validate(&inverted.to_wire(), Some(source_at(test_ip())), issued),
            Err(AssertionRejection::Malformed)
        );

        // unknown key + bad signature => UnknownDecoyKey
        let mut unknown_bad = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            issued,
            issued + 300,
            [0x13; 16],
            Some("missing"),
        );
        unknown_bad.signature[0] ^= 0x01;
        assert_eq!(
            validator.validate(
                &unknown_bad.to_wire(),
                Some(source_at(test_ip())),
                issued + 1
            ),
            Err(AssertionRejection::UnknownDecoyKey)
        );

        // expired + replay (pre-populated) => Expired
        let replay_nonce = [0x14; 16];
        let expired = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            issued,
            issued + 300,
            replay_nonce,
            None,
        );
        validator
            .replay_cache()
            .check_and_insert(replay_nonce, expired.assertion.expires_at, issued + 1)
            .expect("seed replay cache");
        assert_eq!(
            validator.validate(&expired.to_wire(), Some(source_at(test_ip())), issued + 400),
            Err(AssertionRejection::Expired)
        );

        // A binding failure does not consume the nonce. Repeated wrong-source
        // attempts remain binding failures, and the valid source can still use it.
        let _cozo = temp_cozo();
        register(&pk(11), "m").await.expect("register");
        let replay_first = signed_with_fields(
            &issuer,
            &pk(11),
            test_ip(),
            issued,
            issued + 300,
            [0x15; 16],
            None,
        );
        let replay_wire = replay_first.to_wire();
        validator
            .validate(
                &replay_wire,
                Some(source_at(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)))),
                issued + 1,
            )
            .expect_err("first fails IP");
        assert_eq!(
            validator.validate(
                &replay_wire,
                Some(source_at(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3)))),
                issued + 1
            ),
            Err(AssertionRejection::SourceIpMismatch {
                expected: test_ip(),
                actual: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3)),
            })
        );
        validator
            .validate(&replay_wire, Some(source_at(test_ip())), issued + 1)
            .expect("correct source consumes nonce after binding succeeds");

        // IP mismatch + unknown principal => SourceIpMismatch
        let ip_bad = signed_with_fields(
            &issuer,
            &pk(12),
            test_ip(),
            issued,
            issued + 300,
            [0x16; 16],
            None,
        );
        assert_eq!(
            validator.validate(
                &ip_bad.to_wire(),
                Some(source_at(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 4)))),
                issued + 1
            ),
            Err(AssertionRejection::SourceIpMismatch {
                expected: test_ip(),
                actual: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 4)),
            })
        );
    }

    /// VAL-BRIDGE-020
    #[tokio::test(flavor = "multi_thread")]
    async fn identity_fields_derived_with_correct_contexts() {
        let issuer = test_issuer();
        let pubkey = pk(13);
        let (_cozo, validator) = validator_with_registered(&issuer, &pubkey).await;
        let expires = 1_700_000_300i64;
        let signed = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            1_700_000_000,
            expires,
            [0x17; 16],
            None,
        );
        let identity = validator
            .validate(&signed.to_wire(), Some(source_at(test_ip())), 1_700_000_100)
            .expect("valid");
        assert_eq!(identity.expires_at, expires);
        assert_eq!(identity.principal_id, derive_principal_id(&pubkey));
        assert!(identity.session_id.is_empty());
        assert!(identity.session_genesis.is_empty());
        assert_ne!(identity.principal_id, derive_session_id(&pubkey));
    }

    // --- crate-root test implementations ---

    pub async fn rejection_variants_map_to_unauthenticated_tags_impl() {
        let cases: Vec<AssertionRejection> = vec![
            AssertionRejection::Malformed,
            AssertionRejection::UnknownDecoyKey,
            AssertionRejection::BadSignature,
            AssertionRejection::NotYetValid,
            AssertionRejection::Expired,
            AssertionRejection::LifetimeTooLong,
            AssertionRejection::Replay,
            AssertionRejection::MissingConnectInfo,
            AssertionRejection::SourceIpMismatch {
                expected: test_ip(),
                actual: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)),
            },
            AssertionRejection::UnknownPrincipal,
            AssertionRejection::RevokedPrincipal,
            AssertionRejection::RegistryUnavailable,
        ];
        for rejection in cases {
            let status: tonic::Status = rejection.clone().into();
            assert_eq!(status.code(), Code::Unauthenticated);
            assert!(status.message().contains(&rejection.to_string()));
            if let AssertionRejection::SourceIpMismatch { expected, actual } = rejection {
                assert!(status.message().contains(&expected.to_string()));
                assert!(status.message().contains(&actual.to_string()));
            }
        }
    }

    pub async fn replay_cache_keyed_by_nonce_not_wire_bytes_impl() {
        let issuer = test_issuer();
        let pubkey = pk(20);
        let (_cozo, validator) = validator_with_registered(&issuer, &pubkey).await;
        let nonce = [0x21; 16];
        let first = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            nonce,
            None,
        );
        validator
            .validate(&first.to_wire(), Some(source_at(test_ip())), 1_700_000_100)
            .expect("first");
        let second = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            1_700_000_000,
            1_700_000_400,
            nonce,
            None,
        );
        assert_eq!(
            validator.validate(&second.to_wire(), Some(source_at(test_ip())), 1_700_000_100),
            Err(AssertionRejection::Replay)
        );
    }

    pub async fn nonce_not_consumed_when_principal_check_fails_impl() {
        let _cozo = temp_cozo();
        let issuer = test_issuer();
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer));
        let pubkey = pk(21);
        let signed = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x22; 16],
            None,
        );
        let wire = signed.to_wire();
        assert_eq!(
            validator.validate(&wire, Some(source_at(test_ip())), 1_700_000_100),
            Err(AssertionRejection::UnknownPrincipal)
        );
        register(&pubkey, "late").await.expect("register");
        validator
            .validate(&wire, Some(source_at(test_ip())), 1_700_000_100)
            .expect("failed principal resolution must not consume nonce");
    }

    pub async fn corrupted_store_rejects_unknown_decoy_key_at_validate_impl() {
        let issuer = test_issuer();
        let good_b64 =
            base64::engine::general_purpose::STANDARD.encode(issuer.verifying_key().to_bytes());
        let dir = tempfile::tempdir().expect("tempdir");
        let variants: Vec<Vec<u8>> = vec![
            br#"{"decoy_keys": []}"#.to_vec(),
            br#"{"decoy_keys": "x"}"#.to_vec(),
            format!(
                "{{\"decoy_keys\": {{\"decoy-key-1\": \"{}\"}}}}",
                base64::engine::general_purpose::STANDARD.encode([1u8; 16])
            )
            .into_bytes(),
            br#"{"decoy_keys": {"decoy-key-1": "!!!"}}"#.to_vec(),
            format!(
                "{{\"decoy_keys\": {{\"decoy-key-1\": \"{}\", \"decoy-key-1\": \"{}\"}}}}",
                good_b64, good_b64
            )
            .into_bytes(),
        ];
        let signed = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x23; 16],
            None,
        );
        for (idx, bytes) in variants.into_iter().enumerate() {
            let path = dir.path().join(format!("corrupt-{idx}.json"));
            std::fs::write(&path, &bytes).expect("write");
            let validator = AssertionValidator::new(DecoyTrustStore::load_from_path(&path));
            assert_eq!(
                validator.validate(&signed.to_wire(), Some(source_at(test_ip())), 1_700_000_100),
                Err(AssertionRejection::UnknownDecoyKey),
                "variant {idx}"
            );
        }
    }

    pub async fn validate_rejects_inverted_lifetime_impl() {
        let issuer = test_issuer();
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer));
        for (issued, expires) in [(300i64, 0i64), (100i64, 100i64)] {
            let signed = signed_with_fields(
                &issuer,
                SAMPLE_PUBKEY_LOCAL,
                test_ip(),
                1_700_000_000 + issued,
                1_700_000_000 + expires,
                [0x24; 16],
                None,
            );
            assert_eq!(
                validator.validate(
                    &signed.to_wire(),
                    Some(source_at(test_ip())),
                    1_700_000_000 + 50
                ),
                Err(AssertionRejection::Malformed)
            );
        }
    }

    pub async fn leeway_equality_edges_are_exact_impl() {
        let issuer = test_issuer();
        let pubkey = pk(22);
        let (_cozo, validator) = validator_with_registered(&issuer, &pubkey).await;
        let issued = 1_700_000_000i64;
        let expires = issued + 300;
        let base = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            issued,
            expires,
            [0x25; 16],
            None,
        );
        let wire = base.to_wire();
        validator
            .validate(&wire, Some(source_at(test_ip())), expires + 30)
            .expect("expires_at+30 passes");
        assert_eq!(
            validator.validate(&wire, Some(source_at(test_ip())), expires + 31),
            Err(AssertionRejection::Expired)
        );
        let future = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            issued + 30,
            issued + 330,
            [0x26; 16],
            None,
        );
        validator
            .validate(&future.to_wire(), Some(source_at(test_ip())), issued)
            .expect("issued_at == now+30 passes");
        let future_bad = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            issued + 31,
            issued + 331,
            [0x27; 16],
            None,
        );
        assert_eq!(
            validator.validate(&future_bad.to_wire(), Some(source_at(test_ip())), issued),
            Err(AssertionRejection::NotYetValid)
        );
    }

    pub async fn inverted_lifetime_fires_at_parse_step_impl() {
        let issuer = test_issuer();
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer));
        let issued = 1_700_000_000i64;
        let inverted = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            issued + 300,
            issued,
            [0x28; 16],
            Some("missing"),
        );
        assert_eq!(
            validator.validate(&inverted.to_wire(), Some(source_at(test_ip())), issued),
            Err(AssertionRejection::Malformed)
        );
        let mut inverted_bad_sig = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            issued + 300,
            issued,
            [0x29; 16],
            None,
        );
        inverted_bad_sig.signature[0] ^= 0x01;
        assert_eq!(
            validator.validate(
                &inverted_bad_sig.to_wire(),
                Some(source_at(test_ip())),
                issued
            ),
            Err(AssertionRejection::Malformed)
        );
    }

    pub async fn empty_trust_store_rejects_all_impl() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("empty.json");
        std::fs::write(&path, br#"{"decoy_keys": {}}"#).expect("write");
        let store = DecoyTrustStore::load_from_path(&path);
        assert_eq!(store.key_count(), 0);
        let issuer = test_issuer();
        let signed = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x2A; 16],
            None,
        );
        let validator = AssertionValidator::new(store);
        assert_eq!(
            validator.validate(&signed.to_wire(), Some(source_at(test_ip())), 1_700_000_100),
            Err(AssertionRejection::UnknownDecoyKey)
        );
    }

    pub async fn replay_purge_edge_equals_acceptance_edge_impl() {
        let issuer = test_issuer();
        let pubkey = pk(23);
        let (_cozo, validator) = validator_with_registered(&issuer, &pubkey).await;
        let issued = 1_700_000_000i64;
        let expires = issued + 300;
        let signed = signed_with_fields(
            &issuer,
            &pubkey,
            test_ip(),
            issued,
            expires,
            [0x2B; 16],
            None,
        );
        let wire = signed.to_wire();
        validator
            .validate(&wire, Some(source_at(test_ip())), issued + 1)
            .expect("first use");
        let edge = expires + CLOCK_LEEWAY_SECS;
        let result = validator.validate(&wire, Some(source_at(test_ip())), edge);
        assert_eq!(result, Err(AssertionRejection::Replay));
    }

    pub async fn cross_principal_assertion_ip_swap_matrix_impl() {
        let issuer = test_issuer();
        let _cozo = temp_cozo();
        let pubkey_a = pk(30);
        let pubkey_b = pk(31);
        register(&pubkey_a, "a").await.expect("register a");
        register(&pubkey_b, "b").await.expect("register b");
        let ip_a = IpAddr::V4(Ipv4Addr::new(10, 200, 0, 10));
        let ip_b = IpAddr::V4(Ipv4Addr::new(10, 200, 0, 11));
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer));
        let assertion_a = signed_with_fields(
            &issuer,
            &pubkey_a,
            ip_a,
            1_700_000_000,
            1_700_000_300,
            [0x30; 16],
            None,
        );
        let assertion_b = signed_with_fields(
            &issuer,
            &pubkey_b,
            ip_b,
            1_700_000_000,
            1_700_000_300,
            [0x31; 16],
            None,
        );
        validator
            .validate(&assertion_a.to_wire(), Some(source_at(ip_a)), 1_700_000_100)
            .expect("A from A");
        validator
            .validate(&assertion_b.to_wire(), Some(source_at(ip_b)), 1_700_000_100)
            .expect("B from B");
        let assertion_a_swap = signed_with_fields(
            &issuer,
            &pubkey_a,
            ip_a,
            1_700_000_000,
            1_700_000_300,
            [0x32; 16],
            None,
        );
        assert_eq!(
            validator.validate(
                &assertion_a_swap.to_wire(),
                Some(source_at(ip_b)),
                1_700_000_100
            ),
            Err(AssertionRejection::SourceIpMismatch {
                expected: ip_a,
                actual: ip_b,
            })
        );
        let assertion_b_swap = signed_with_fields(
            &issuer,
            &pubkey_b,
            ip_b,
            1_700_000_000,
            1_700_000_300,
            [0x33; 16],
            None,
        );
        assert_eq!(
            validator.validate(
                &assertion_b_swap.to_wire(),
                Some(source_at(ip_a)),
                1_700_000_100
            ),
            Err(AssertionRejection::SourceIpMismatch {
                expected: ip_b,
                actual: ip_a,
            })
        );
    }

    pub async fn replay_cache_keyed_globally_by_nonce_impl() {
        let issuer = test_issuer();
        let _cozo = temp_cozo();
        let pubkey_a = pk(32);
        let pubkey_b = pk(33);
        register(&pubkey_a, "a").await.expect("register a");
        register(&pubkey_b, "b").await.expect("register b");
        let validator = AssertionValidator::new(trust_store_for_issuer(&issuer));
        let shared_nonce = [0x32; 16];
        let ip_a = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let ip_b = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));
        let a = signed_with_fields(
            &issuer,
            &pubkey_a,
            ip_a,
            1_700_000_000,
            1_700_000_300,
            shared_nonce,
            None,
        );
        validator
            .validate(&a.to_wire(), Some(source_at(ip_a)), 1_700_000_100)
            .expect("A first");
        let b = signed_with_fields(
            &issuer,
            &pubkey_b,
            ip_b,
            1_700_000_000,
            1_700_000_300,
            shared_nonce,
            None,
        );
        assert_eq!(
            validator.validate(&b.to_wire(), Some(source_at(ip_b)), 1_700_000_100),
            Err(AssertionRejection::Replay)
        );
        let a2 = signed_with_fields(
            &issuer,
            &pubkey_a,
            ip_a,
            1_700_000_000,
            1_700_000_300,
            [0x33; 16],
            None,
        );
        validator
            .validate(&a2.to_wire(), Some(source_at(ip_a)), 1_700_000_100)
            .expect("A fresh nonce");
    }

    pub async fn trust_store_rotation_is_load_once_impl() {
        let issuer = test_issuer();
        let other = SigningKey::from_bytes(&[8u8; 32]);
        let other_signing_key = SigningKey::from_bytes(&[8u8; 32]);
        let _cozo = temp_cozo();
        register(SAMPLE_PUBKEY_LOCAL, "rotation")
            .await
            .expect("register rotation principal");
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_trust_store(&dir, &issuer);
        let store_v1 = DecoyTrustStore::load_from_path(&path);
        let validator_v1 = AssertionValidator::new(store_v1);
        let signed_v1 = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x34; 16],
            None,
        );
        validator_v1
            .validate(
                &signed_v1.to_wire(),
                Some(source_at(test_ip())),
                1_700_000_100,
            )
            .expect("v1 accepts its original signing key");

        let other_b64 =
            base64::engine::general_purpose::STANDARD.encode(other.verifying_key().to_bytes());
        let issuer_other = DecoyIssuer::new(other, "other-key", Duration::from_secs(900));
        std::fs::write(
            &path,
            format!("{{\"decoy_keys\": {{\"other-key\": \"{}\"}}}}", other_b64),
        )
        .expect("rotate file");
        let signed_v1_after_rotation = signed_with_fields(
            &issuer,
            SAMPLE_PUBKEY_LOCAL,
            test_ip(),
            1_700_000_000,
            1_700_000_300,
            [0x36; 16],
            None,
        );
        validator_v1
            .validate(
                &signed_v1_after_rotation.to_wire(),
                Some(source_at(test_ip())),
                1_700_000_100,
            )
            .expect("existing validator unchanged after file rotation");

        let validator_v2 = AssertionValidator::new(DecoyTrustStore::load_from_path(&path));
        assert_eq!(
            validator_v1.validate(
                &signed_v1.to_wire(),
                Some(source_at(test_ip())),
                1_700_000_100
            ),
            Err(AssertionRejection::Replay)
        );
        assert_eq!(
            validator_v2.validate(
                &signed_v1.to_wire(),
                Some(source_at(test_ip())),
                1_700_000_100
            ),
            Err(AssertionRejection::UnknownDecoyKey)
        );
        let signed_other = signed_with_signing_key(
            &other_signing_key,
            signed_with_fields(
                &issuer_other,
                SAMPLE_PUBKEY_LOCAL,
                test_ip(),
                1_700_000_000,
                1_700_000_300,
                [0x35; 16],
                Some("other-key"),
            ),
        );
        assert_eq!(
            validator_v1.validate(
                &signed_other.to_wire(),
                Some(source_at(test_ip())),
                1_700_000_100
            ),
            Err(AssertionRejection::UnknownDecoyKey)
        );
        validator_v2
            .validate(
                &signed_other.to_wire(),
                Some(source_at(test_ip())),
                1_700_000_100,
            )
            .expect("rotated validator accepts its new signing key");
        let unknown_pubkey = pk(201);
        let signed_unknown = signed_with_signing_key(
            &other_signing_key,
            signed_with_fields(
                &issuer_other,
                &unknown_pubkey,
                test_ip(),
                1_700_000_000,
                1_700_000_300,
                [0x37; 16],
                Some("other-key"),
            ),
        );
        assert_eq!(
            validator_v2
                .validate(
                    &signed_unknown.to_wire(),
                    Some(source_at(test_ip())),
                    1_700_000_100,
                )
                .expect_err("unknown principal"),
            AssertionRejection::UnknownPrincipal
        );
    }

    pub async fn validator_state_is_per_serving_instance_impl() {
        let dir1 = tempfile::tempdir().expect("tempdir1");
        let dir2 = tempfile::tempdir().expect("tempdir2");
        let issuer1 = test_issuer();
        let issuer2 = DecoyIssuer::new(
            SigningKey::from_bytes(&[8u8; 32]),
            "decoy-key-2",
            Duration::from_secs(900),
        );
        write_trust_store(&dir1, &issuer1);
        let path2 = dir2.path().join("decoy-trust.json");
        let b64 =
            base64::engine::general_purpose::STANDARD.encode(issuer2.verifying_key().to_bytes());
        std::fs::write(
            &path2,
            format!(
                "{{\"decoy_keys\": {{\"{}\": \"{}\"}}}}",
                issuer2.key_id(),
                b64
            ),
        )
        .expect("write2");
        let path1 = dir1.path().join("decoy-trust.json");
        let v1 = AssertionValidator::new(DecoyTrustStore::load_from_path(&path1));
        let v2 = AssertionValidator::new(DecoyTrustStore::load_from_path(&path2));
        assert!(v1.trust_store().contains_key("decoy-key-1"));
        assert!(!v1.trust_store().contains_key("decoy-key-2"));
        assert!(v2.trust_store().contains_key("decoy-key-2"));
        assert!(!v2.trust_store().contains_key("decoy-key-1"));
    }
}
