//! Whole-record credential gate. Uses the identity-chat guard, adding evidence
//! digest provenance; it never hashes registered credentials for detection.
use op_chat_interface::guard::{GuardVerdict, SecretGuard};
use op_chat_interface::types::CredentialClass;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
pub enum RecordError {
    #[error("invalid cockpit evidence: {0}")]
    Invalid(&'static str),
    #[error("cockpit evidence withheld: {0}")]
    Withheld(&'static str),
    #[error("cockpit evidence serialization failed")]
    Encoding,
    #[error("cockpit evidence I/O failed")]
    Io(#[from] std::io::Error),
}

/// Only this constructor can produce a persistable digest. Deserializing a
/// digest does not establish its provenance; its source must be checked again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceDigest {
    algorithm: DigestAlgorithm,
    value: String,
    source_reference: String,
    #[serde(skip)]
    checked_non_secret_source: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DigestAlgorithm { Sha256 }

impl EvidenceDigest {
    pub fn from_source(guard: &RecordGuard<'_>, source_reference: &str, source: &[u8]) -> Result<Self, RecordError> {
        guard.check(source_reference.as_bytes())?;
        guard.check(source)?;
        if source_reference.trim().is_empty() { return Err(RecordError::Invalid("digest source absent")); }
        Ok(Self {
            algorithm: DigestAlgorithm::Sha256,
            value: hex::encode(Sha256::digest(source)),
            source_reference: source_reference.to_owned(),
            checked_non_secret_source: true,
        })
    }

    pub fn reestablish_source(&mut self, guard: &RecordGuard<'_>, source: &[u8]) -> Result<(), RecordError> {
        self.checked_non_secret_source = false;
        let expected = Self::from_source(guard, &self.source_reference, source)?;
        if self.value != expected.value || self.algorithm != expected.algorithm {
            return Err(RecordError::Invalid("digest source changed"));
        }
        self.checked_non_secret_source = true;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), RecordError> {
        if !self.checked_non_secret_source || self.source_reference.trim().is_empty()
            || self.value.len() != 64 || !self.value.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(RecordError::Invalid("non-secret digest provenance unestablished"));
        }
        Ok(())
    }
}

pub struct RecordGuard<'a> { shared: &'a SecretGuard }
impl<'a> RecordGuard<'a> {
    pub fn new(shared: &'a SecretGuard) -> Self { Self { shared } }

    pub fn check(&self, bytes: &[u8]) -> Result<(), RecordError> {
        let started = Instant::now();
        match self.shared.check_item(bytes) {
            GuardVerdict::Clean => {}
            GuardVerdict::Blocked { cls } => return Err(RecordError::Withheld(class_name(cls))),
            GuardVerdict::Error { .. } => return Err(RecordError::Withheld("guard-error")),
        }
        // PEM and common credential-bearing key/value text are not limited to
        // JSON documents. Detection discloses class only, never the match.
        let text = String::from_utf8_lossy(bytes);
        for line in text.lines() {
            if line.contains("-----BEGIN ") && line.contains("PRIVATE KEY-----") {
                return Err(RecordError::Withheld("identity-key"));
            }
            let trimmed = line.trim();
            if trimmed.starts_with("Authorization: Bearer ") || trimmed.starts_with("authorization: Bearer ") {
                return Err(RecordError::Withheld("session-credential"));
            }
        }
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) { inspect_json(&value)?; }
        if started.elapsed() > Duration::from_millis(2000) { return Err(RecordError::Withheld("guard-timeout")); }
        Ok(())
    }

    /// The caller explicitly supplies the credential class. Redaction never
    /// returns any part, window, fingerprint, or digest of the unsafe error.
    pub fn redact_error(&self, bytes: &[u8], declared_class: Option<CredentialClass>) -> Result<String, RecordError> {
        if let Some(class) = declared_class { return Ok(format!("[redacted:{}]", class_name(class))); }
        match self.check(bytes) {
            Ok(()) => String::from_utf8(bytes.to_vec()).map_err(|_| RecordError::Encoding),
            Err(RecordError::Withheld(class)) => Ok(format!("[redacted:{class}]")),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn serialize(&self, record: &super::records::EvidenceRecord) -> Result<Zeroizing<Vec<u8>>, RecordError> {
        record.validate()?;
        let bytes = Zeroizing::new(serde_json::to_vec(record).map_err(|_| RecordError::Encoding)?);
        self.check(&bytes)?;
        Ok(bytes)
    }
}

fn class_name(class: CredentialClass) -> &'static str {
    match class {
        CredentialClass::Sid1 => "sid1",
        CredentialClass::Oia1 => "oia1",
        CredentialClass::ProviderCredential => "provider-credential",
        CredentialClass::IdentityKey => "identity-key",
        CredentialClass::W3Key => "w3-key",
        CredentialClass::DeclaredSecretField => "declared-secret-field",
    }
}

fn inspect_json(value: &serde_json::Value) -> Result<(), RecordError> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let normalized = key.to_ascii_lowercase().replace('-', "_");
                if matches!(normalized.as_str(), "password" | "token" | "access_token" | "refresh_token"
                    | "api_key" | "private_key" | "secret" | "client_secret" | "credential_material"
                    | "credential_digest" | "secret_digest" | "credential_hash" | "secret_hash"
                    | "sid1" | "oia1" | "sealed_id" | "authorization") {
                    return Err(RecordError::Withheld("declared-secret-field"));
                }
                inspect_json(value)?;
            }
        }
        serde_json::Value::Array(items) => for item in items { inspect_json(item)?; },
        _ => {}
    }
    Ok(())
}
