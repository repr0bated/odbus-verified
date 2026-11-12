//! Decoy-issued identity (IID1).
//!
//! WireGuard terminates at the decoy, so the decoy is the one place that
//! observes which key a peer arrived with. It mints the genesis and SID1 for
//! that key and delivers them signed with its Ed25519 decoy key. The identity
//! container verifies the envelope and commits the SID1 as its `sealed_id`;
//! from then on the container owns the identity.
//!
//! No hardware value is an input. The genesis inputs are the WireGuard key,
//! a 32-byte issuance nonce in the chain-head position, the issue time, the
//! decoy's sealed-catalog hash and the arrival time.
//!
//! ```text
//! to_wire()       = b"IID1" || body || signature[64]
//! signing_bytes() = b"IID1" || body
//! body            = u32le(len) || sid1_wire || u32le(len) || decoy_key_id
//! ```
//!
//! OSCAL subid: `mut.service.identity.decoy-issue@v1`
use std::collections::HashMap;

use base64::Engine as _;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::RngCore;

use crate::sealed_id::{NextDnsContext, SealedId, SealedIdError};

const MAGIC: &[u8; 4] = b"IID1";
const SIGNATURE_LEN: usize = 64;
const NONCE_LEN: usize = 32;
const MAX_WIRE_LEN: usize = 16 * 1024;
/// SID1 principal namespace; the only value `mcp_frontend` accepts.
pub const PRINCIPAL_KIND: &str = "wireguard-principal";
/// Transport scope sealed into every decoy-issued SID1.
pub const TRANSPORT_SCOPE: &str = "dbus,grpc,mcp,mqtt";
/// Identity sled record format sealed into SID1 (`identity_sled::RECORD_FORMAT`).
pub const SCHEMA_VERSION: u32 = 3;

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum IssuedIdentityError {
    #[error("malformed IID1 envelope")]
    Malformed,
    #[error("IID1 envelope exceeds the size limit")]
    TooLarge,
    #[error("IID1 decoy key is not trusted")]
    UntrustedKey,
    #[error("IID1 signature does not verify")]
    BadSignature,
    #[error("WireGuard public key must be canonical base64 of 32 bytes")]
    InvalidPublicKey,
    #[error("catalog hash must be 64 hex characters")]
    InvalidCatalogHash,
    #[error("issued identity does not match the expected WireGuard key")]
    KeyMismatch,
    #[error("issued identity ids are not derived from its WireGuard key")]
    DerivationMismatch,
    #[error("issued identity genesis does not match its inputs")]
    GenesisMismatch,
    #[error("sealed identity: {0}")]
    Sealed(#[from] SealedIdError),
}

/// Inputs the decoy supplies for one issuance.
pub struct IssueRequest<'a> {
    pub wireguard_pubkey: &'a str,
    pub catalog_hash_hex: &'a str,
    pub nextdns: Option<NextDnsContext>,
    pub schema_version: u32,
    /// Unix seconds; the caller's clock.
    pub now: i64,
}

fn decode_pubkey(value: &str) -> Result<[u8; 32], IssuedIdentityError> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| IssuedIdentityError::InvalidPublicKey)?;
    let key: [u8; 32] = raw
        .try_into()
        .map_err(|_| IssuedIdentityError::InvalidPublicKey)?;
    if base64::engine::general_purpose::STANDARD.encode(key) != value {
        return Err(IssuedIdentityError::InvalidPublicKey);
    }
    Ok(key)
}

fn decode_catalog_hash(value: &str) -> Result<[u8; 32], IssuedIdentityError> {
    hex::decode(value)
        .ok()
        .and_then(|raw| <[u8; 32]>::try_from(raw).ok())
        .ok_or(IssuedIdentityError::InvalidCatalogHash)
}

fn push(out: &mut Vec<u8>, bytes: &[u8]) {
    let len = u32::try_from(bytes.len()).expect("IID1 field fits in u32");
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(bytes);
}

/// A decoy-signed SID1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedIdentity {
    pub sealed: SealedId,
    pub decoy_key_id: String,
    pub signature: [u8; SIGNATURE_LEN],
}

impl IssuedIdentity {
    fn signing_bytes(sid1: &[u8], key_id: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(sid1.len() + key_id.len() + 12);
        out.extend_from_slice(MAGIC);
        push(&mut out, sid1);
        push(&mut out, key_id.as_bytes());
        out
    }

    /// Mint and sign a new identity for `request.wireguard_pubkey`.
    pub fn issue(
        signing_key: &SigningKey,
        key_id: &str,
        request: IssueRequest<'_>,
    ) -> Result<Self, IssuedIdentityError> {
        let key = decode_pubkey(request.wireguard_pubkey)?;
        let catalog_hash = decode_catalog_hash(request.catalog_hash_hex)?;
        let mut nonce = [0u8; NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let mut trace = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut trace);
        let genesis = crate::session_genesis::mint_genesis(
            &key,
            &nonce,
            request.now,
            &catalog_hash,
            request.now,
        );
        let sealed = SealedId {
            principal_id: crate::session::derive_principal_id(request.wireguard_pubkey),
            principal_kind: PRINCIPAL_KIND.to_string(),
            session_id: crate::session::derive_session_id(request.wireguard_pubkey),
            wireguard_pubkey: request.wireguard_pubkey.to_string(),
            session_genesis: hex::encode(genesis),
            trace_id: hex::encode(trace),
            schema_version: request.schema_version,
            issued_at: request.now,
            expires_at: 0,
            arrival_timestamp: request.now,
            chain_head_at_arrival: hex::encode(nonce),
            catalog_hash_at_arrival: request.catalog_hash_hex.to_ascii_lowercase(),
            head_timestamp_at_arrival: request.now,
            transport_scope: TRANSPORT_SCOPE.to_string(),
            nextdns: request.nextdns,
        };
        let sid1 = sealed.seal()?;
        let signature = signing_key
            .sign(&Self::signing_bytes(&sid1, key_id))
            .to_bytes();
        Ok(Self {
            sealed,
            decoy_key_id: key_id.to_string(),
            signature,
        })
    }

    pub fn to_wire(&self) -> Result<Vec<u8>, IssuedIdentityError> {
        let mut out = Self::signing_bytes(&self.sealed.seal()?, &self.decoy_key_id);
        out.extend_from_slice(&self.signature);
        Ok(out)
    }

    /// Decode, verify the decoy signature against `trusted`, require the
    /// identity to belong to `expected_pubkey`, and recheck that the ids and
    /// the genesis follow from the sealed inputs.
    pub fn verify(
        wire: &[u8],
        trusted: &HashMap<String, VerifyingKey>,
        expected_pubkey: &str,
    ) -> Result<Self, IssuedIdentityError> {
        if wire.len() > MAX_WIRE_LEN {
            return Err(IssuedIdentityError::TooLarge);
        }
        if wire.len() < MAGIC.len() + 8 + SIGNATURE_LEN || &wire[..MAGIC.len()] != MAGIC {
            return Err(IssuedIdentityError::Malformed);
        }
        let body_end = wire.len() - SIGNATURE_LEN;
        let mut at = MAGIC.len();
        let take = |at: &mut usize| -> Result<&[u8], IssuedIdentityError> {
            let len_end = at
                .checked_add(4)
                .filter(|end| *end <= body_end)
                .ok_or(IssuedIdentityError::Malformed)?;
            let len = u32::from_le_bytes(wire[*at..len_end].try_into().expect("4 bytes")) as usize;
            let end = len_end
                .checked_add(len)
                .filter(|end| *end <= body_end)
                .ok_or(IssuedIdentityError::Malformed)?;
            *at = end;
            Ok(&wire[len_end..end])
        };
        let sid1 = take(&mut at)?;
        let key_id = std::str::from_utf8(take(&mut at)?)
            .map_err(|_| IssuedIdentityError::Malformed)?
            .to_string();
        if at != body_end {
            return Err(IssuedIdentityError::Malformed);
        }
        let verifying_key = trusted
            .get(&key_id)
            .ok_or(IssuedIdentityError::UntrustedKey)?;
        let signature: [u8; SIGNATURE_LEN] = wire[body_end..].try_into().expect("signature length");
        verifying_key
            .verify_strict(&wire[..body_end], &Signature::from_bytes(&signature))
            .map_err(|_| IssuedIdentityError::BadSignature)?;

        let sealed = SealedId::open(sid1)?;
        if sealed.wireguard_pubkey != expected_pubkey {
            return Err(IssuedIdentityError::KeyMismatch);
        }
        let key = decode_pubkey(&sealed.wireguard_pubkey)?;
        if sealed.session_id != crate::session::derive_session_id(&sealed.wireguard_pubkey)
            || sealed.principal_id != crate::session::derive_principal_id(&sealed.wireguard_pubkey)
            || sealed.principal_kind != PRINCIPAL_KIND
        {
            return Err(IssuedIdentityError::DerivationMismatch);
        }
        let nonce = hex::decode(&sealed.chain_head_at_arrival)
            .map_err(|_| IssuedIdentityError::GenesisMismatch)?;
        let catalog_hash = decode_catalog_hash(&sealed.catalog_hash_at_arrival)
            .map_err(|_| IssuedIdentityError::GenesisMismatch)?;
        let genesis = crate::session_genesis::mint_genesis(
            &key,
            &nonce,
            sealed.head_timestamp_at_arrival,
            &catalog_hash,
            sealed.arrival_timestamp,
        );
        if hex::encode(genesis) != sealed.session_genesis {
            return Err(IssuedIdentityError::GenesisMismatch);
        }
        Ok(Self {
            sealed,
            decoy_key_id: key_id,
            signature,
        })
    }
}

/// Parse the decoy trust store JSON shared with `op-grpc-bridge`:
/// `{"decoy_keys": {"<key_id>": "<base64 Ed25519 verifying key>"}}`.
pub fn parse_trust_store(bytes: &[u8]) -> anyhow::Result<HashMap<String, VerifyingKey>> {
    #[derive(serde::Deserialize)]
    struct TrustStore {
        decoy_keys: HashMap<String, String>,
    }
    let parsed: TrustStore = serde_json::from_slice(bytes)?;
    let mut keys = HashMap::new();
    for (key_id, encoded) in parsed.decoy_keys {
        let raw = base64::engine::general_purpose::STANDARD.decode(encoded.as_bytes())?;
        let raw: [u8; 32] = raw
            .try_into()
            .map_err(|_| anyhow::anyhow!("decoy key {key_id} is not 32 bytes"))?;
        keys.insert(key_id, VerifyingKey::from_bytes(&raw)?);
    }
    anyhow::ensure!(!keys.is_empty(), "decoy trust store has no keys");
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUBKEY: &str = "wNhmhGDpdkScmSdcHnzhr/1vmTIL5wxLXcve/jwk9xE=";
    const CATALOG: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn signer() -> SigningKey {
        SigningKey::from_bytes(&[9u8; 32])
    }

    fn trust(key: &SigningKey, id: &str) -> HashMap<String, VerifyingKey> {
        HashMap::from([(id.to_string(), key.verifying_key())])
    }

    fn issue() -> IssuedIdentity {
        IssuedIdentity::issue(
            &signer(),
            "decoy-1",
            IssueRequest {
                wireguard_pubkey: PUBKEY,
                catalog_hash_hex: CATALOG,
                nextdns: Some(NextDnsContext {
                    profile_id: "689ec7".into(),
                    device_name: "Ghostbridge-Incus".into(),
                }),
                schema_version: 1,
                now: 1_800_000_000,
            },
        )
        .unwrap()
    }

    #[test]
    fn issued_identity_round_trips_and_derives_from_the_key() {
        let issued = issue();
        let wire = issued.to_wire().unwrap();
        let verified = IssuedIdentity::verify(&wire, &trust(&signer(), "decoy-1"), PUBKEY).unwrap();
        assert_eq!(verified, issued);
        assert_eq!(
            verified.sealed.session_id,
            crate::session::derive_session_id(PUBKEY)
        );
        assert_eq!(
            verified.sealed.principal_id,
            crate::session::derive_principal_id(PUBKEY)
        );
        assert_eq!(verified.sealed.nextdns.unwrap().profile_id, "689ec7");
    }

    #[test]
    fn each_issuance_has_a_distinct_genesis() {
        assert_ne!(
            issue().sealed.session_genesis,
            issue().sealed.session_genesis
        );
    }

    #[test]
    fn rejects_untrusted_or_wrong_signer() {
        let wire = issue().to_wire().unwrap();
        assert_eq!(
            IssuedIdentity::verify(&wire, &trust(&signer(), "other"), PUBKEY),
            Err(IssuedIdentityError::UntrustedKey)
        );
        let impostor = SigningKey::from_bytes(&[3u8; 32]);
        assert_eq!(
            IssuedIdentity::verify(&wire, &trust(&impostor, "decoy-1"), PUBKEY),
            Err(IssuedIdentityError::BadSignature)
        );
    }

    #[test]
    fn rejects_tampering_truncation_and_another_key() {
        let mut wire = issue().to_wire().unwrap();
        let trusted = trust(&signer(), "decoy-1");
        assert_eq!(
            IssuedIdentity::verify(
                &wire,
                &trusted,
                "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
            ),
            Err(IssuedIdentityError::KeyMismatch)
        );
        assert!(IssuedIdentity::verify(&wire[..wire.len() - 1], &trusted, PUBKEY).is_err());
        wire[10] ^= 1;
        assert!(IssuedIdentity::verify(&wire, &trusted, PUBKEY).is_err());
    }

    #[test]
    fn rejects_noncanonical_inputs() {
        let bad = |pubkey, catalog| {
            IssuedIdentity::issue(
                &signer(),
                "decoy-1",
                IssueRequest {
                    wireguard_pubkey: pubkey,
                    catalog_hash_hex: catalog,
                    nextdns: None,
                    schema_version: 1,
                    now: 1,
                },
            )
            .unwrap_err()
        };
        assert_eq!(
            bad("not-a-key", CATALOG),
            IssuedIdentityError::InvalidPublicKey
        );
        assert_eq!(bad(PUBKEY, "abcd"), IssuedIdentityError::InvalidCatalogHash);
    }

    #[test]
    fn trust_store_parses_the_bridge_format() {
        let key =
            base64::engine::general_purpose::STANDARD.encode(signer().verifying_key().to_bytes());
        let json = format!(r#"{{"decoy_keys":{{"decoy-1":"{key}"}}}}"#);
        let parsed = parse_trust_store(json.as_bytes()).unwrap();
        assert!(parsed.contains_key("decoy-1"));
        assert!(parse_trust_store(br#"{"decoy_keys":{}}"#).is_err());
    }
}
