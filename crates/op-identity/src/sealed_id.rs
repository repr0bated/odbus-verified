//! SID1 — the sealed, server-authored session identity carried by local clients.
//!
//! The MutationEngine mints this sealed ID only after it has minted (or recovered)
//! the session genesis.  The complete immutable session identity is encoded in
//! one bounded binary envelope and stored inline in the session sled's
//! `sealed_id` as `sid1:<unpadded-base64url>`.  A SID1-aware client forwards the
//! encoded payload unchanged; the receiver verifies the seal and exact-matches
//! every claim against the authoritative sled before constructing request
//! identity.
//!
//! SID1 is deliberately not an OPBLOB01 plugin-schema object and is never a
//! Snowball footprint.  Its SHA-256 trailer is an integrity seal for these
//! bytes only; it is not an identity, grant key, vector payload, or chain term.

use base64::Engine as _;
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 4] = b"SID1";
const SEAL_LEN: usize = 32;
const MAX_WIRE_LEN: usize = 16 * 1024;
const MAX_STRING_LEN: usize = 4096;

/// Inline sled representation of an SID1 envelope.
pub const INLINE_PREFIX: &str = "sid1:";
/// Direct Streamable HTTP header emitted by SID1-aware clients.
pub const HTTP_HEADER_NAME: &str = "x-opdbus-sealed-id-bin";

/// Immutable identity claims sealed by the MutationEngine at session arrival.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedId {
    /// Stable principal used for grants and audit actor metadata.
    pub principal_id: String,
    /// Principal namespace.  Version 1 uses `wireguard-principal`.
    pub principal_kind: String,
    pub session_id: String,
    pub wireguard_pubkey: String,
    /// Immutable session arrival anchor.  Never used as a grant key.
    pub session_genesis: String,
    pub trace_id: String,
    pub schema_version: u32,
    pub issued_at: i64,
    /// Zero means the sled owns a lifelong account term.
    pub expires_at: i64,
    pub arrival_timestamp: i64,
    pub chain_head_at_arrival: String,
    pub catalog_hash_at_arrival: String,
    pub head_timestamp_at_arrival: i64,
    /// Transport scope of the one identity envelope.
    pub transport_scope: String,
}

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum SealedIdError {
    #[error("malformed SID1 sealed identity")]
    Malformed,
    #[error("SID1 sealed identity exceeds the size limit")]
    TooLarge,
    #[error("SID1 sealed identity integrity seal mismatch")]
    SealMismatch,
    #[error("identity sled does not contain an inline SID1 sealed identity")]
    MissingInlineSealedId,
    #[error("identity sled SID1 sealed identity is not canonical base64url")]
    NonCanonicalBase64,
}

impl SealedId {
    /// Deterministic SID1 bytes: fixed-order bounded fields followed by the
    /// SHA-256 integrity seal over `MAGIC || body`.
    pub fn seal(&self) -> Result<Vec<u8>, SealedIdError> {
        let mut out = Vec::with_capacity(1024);
        out.extend_from_slice(MAGIC);
        push_string(&mut out, &self.principal_id)?;
        push_string(&mut out, &self.principal_kind)?;
        push_string(&mut out, &self.session_id)?;
        push_string(&mut out, &self.wireguard_pubkey)?;
        push_string(&mut out, &self.session_genesis)?;
        push_string(&mut out, &self.trace_id)?;
        out.extend_from_slice(&self.schema_version.to_le_bytes());
        out.extend_from_slice(&self.issued_at.to_le_bytes());
        out.extend_from_slice(&self.expires_at.to_le_bytes());
        out.extend_from_slice(&self.arrival_timestamp.to_le_bytes());
        push_string(&mut out, &self.chain_head_at_arrival)?;
        push_string(&mut out, &self.catalog_hash_at_arrival)?;
        out.extend_from_slice(&self.head_timestamp_at_arrival.to_le_bytes());
        push_string(&mut out, &self.transport_scope)?;
        if out.len().saturating_add(SEAL_LEN) > MAX_WIRE_LEN {
            return Err(SealedIdError::TooLarge);
        }
        let seal = Sha256::digest(&out);
        out.extend_from_slice(&seal);
        Ok(out)
    }

    /// Decode and verify one exact SID1 envelope.
    pub fn open(wire: &[u8]) -> Result<Self, SealedIdError> {
        if wire.len() > MAX_WIRE_LEN {
            return Err(SealedIdError::TooLarge);
        }
        if wire.len() < MAGIC.len() + SEAL_LEN || &wire[..MAGIC.len()] != MAGIC {
            return Err(SealedIdError::Malformed);
        }
        let body_end = wire.len() - SEAL_LEN;
        let expected = Sha256::digest(&wire[..body_end]);
        if expected.as_slice() != &wire[body_end..] {
            return Err(SealedIdError::SealMismatch);
        }

        let mut reader = Reader::new(&wire[MAGIC.len()..body_end]);
        let sealed = Self {
            principal_id: reader.read_string()?,
            principal_kind: reader.read_string()?,
            session_id: reader.read_string()?,
            wireguard_pubkey: reader.read_string()?,
            session_genesis: reader.read_string()?,
            trace_id: reader.read_string()?,
            schema_version: reader.read_u32()?,
            issued_at: reader.read_i64()?,
            expires_at: reader.read_i64()?,
            arrival_timestamp: reader.read_i64()?,
            chain_head_at_arrival: reader.read_string()?,
            catalog_hash_at_arrival: reader.read_string()?,
            head_timestamp_at_arrival: reader.read_i64()?,
            transport_scope: reader.read_string()?,
        };
        if !reader.is_consumed()
            || sealed.principal_id.is_empty()
            || sealed.session_id.is_empty()
            || sealed.wireguard_pubkey.is_empty()
            || sealed.session_genesis.is_empty()
            || sealed.trace_id.is_empty()
        {
            return Err(SealedIdError::Malformed);
        }
        Ok(sealed)
    }

    /// Embed the sealed bytes directly into the sled's existing `sealed_id`.
    pub fn to_inline_ref(&self) -> Result<String, SealedIdError> {
        Ok(format!(
            "{INLINE_PREFIX}{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(self.seal()?)
        ))
    }

    /// Read an inline sled value, requiring its base64url spelling to be
    /// canonical so the bridge can exact-match what the client forwarded.
    pub fn from_inline_ref(value: &str) -> Result<Self, SealedIdError> {
        let encoded = value
            .strip_prefix(INLINE_PREFIX)
            .ok_or(SealedIdError::MissingInlineSealedId)?;
        let wire = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded.as_bytes())
            .map_err(|_| SealedIdError::NonCanonicalBase64)?;
        if base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&wire) != encoded {
            return Err(SealedIdError::NonCanonicalBase64);
        }
        Self::open(&wire)
    }
}

fn push_string(out: &mut Vec<u8>, value: &str) -> Result<(), SealedIdError> {
    if value.len() > MAX_STRING_LEN {
        return Err(SealedIdError::TooLarge);
    }
    let len = u32::try_from(value.len()).map_err(|_| SealedIdError::TooLarge)?;
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], SealedIdError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or(SealedIdError::Malformed)?;
        if end > self.bytes.len() {
            return Err(SealedIdError::Malformed);
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn read_u32(&mut self) -> Result<u32, SealedIdError> {
        Ok(u32::from_le_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| SealedIdError::Malformed)?,
        ))
    }

    fn read_i64(&mut self) -> Result<i64, SealedIdError> {
        Ok(i64::from_le_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| SealedIdError::Malformed)?,
        ))
    }

    fn read_string(&mut self) -> Result<String, SealedIdError> {
        let length = self.read_u32()? as usize;
        if length > MAX_STRING_LEN {
            return Err(SealedIdError::TooLarge);
        }
        String::from_utf8(self.take(length)?.to_vec()).map_err(|_| SealedIdError::Malformed)
    }

    fn is_consumed(&self) -> bool {
        self.position == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SealedId {
        SealedId {
            principal_id: "principal".into(),
            principal_kind: "wireguard-principal".into(),
            session_id: "session".into(),
            wireguard_pubkey: "pubkey".into(),
            session_genesis: "ab".repeat(32),
            trace_id: "cd".repeat(16),
            schema_version: 3,
            issued_at: 11,
            expires_at: 0,
            arrival_timestamp: 11,
            chain_head_at_arrival: "ef".repeat(32),
            catalog_hash_at_arrival: "12".repeat(32),
            head_timestamp_at_arrival: 10,
            transport_scope: "dbus,grpc,mcp".into(),
        }
    }

    #[test]
    fn sid1_round_trips_deterministically() {
        let sealed = sample();
        let first = sealed.seal().unwrap();
        assert_eq!(first, sealed.seal().unwrap());
        assert_eq!(SealedId::open(&first).unwrap(), sealed);
        let inline = sealed.to_inline_ref().unwrap();
        assert_eq!(SealedId::from_inline_ref(&inline).unwrap(), sealed);
    }

    #[test]
    fn sid1_rejects_tampering_and_trailing_bytes() {
        let mut wire = sample().seal().unwrap();
        wire[20] ^= 1;
        assert_eq!(SealedId::open(&wire), Err(SealedIdError::SealMismatch));

        let mut trailing = sample().seal().unwrap();
        trailing.push(0);
        assert_eq!(SealedId::open(&trailing), Err(SealedIdError::SealMismatch));
    }

    #[test]
    fn inline_ref_rejects_plugin_blob_and_padded_base64() {
        assert_eq!(
            SealedId::from_inline_ref("OPBLOB01"),
            Err(SealedIdError::MissingInlineSealedId)
        );
        let padded = format!("{}=", sample().to_inline_ref().unwrap());
        assert_eq!(
            SealedId::from_inline_ref(&padded),
            Err(SealedIdError::NonCanonicalBase64)
        );
    }
}
