//! Caller identity from OIA1 assertions (Requirement 12).
//!
//! The wire codec and the strict Ed25519 check come from `op-identity`
//! (`oracle_assertion`), not from the bridge. Only public material is read:
//! decoy verifying keys, the WireGuard-key → principal map, and the mount map.

use crate::fusecall::MountSpec;
use anyhow::{Context, Result};
use base64::Engine;
use ed25519_dalek::VerifyingKey;
use op_identity::oracle_assertion::{MAX_LIFETIME_SECS, SignedAssertion, verify_signature};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

/// Allowed clock skew for `issued_at`, seconds.
const SKEW: i64 = 30;
pub const ASSERTION_HEADER: &str = "x-oracle-identity-assertion-bin";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub principal_id: String,
    pub human_pubkey: String,
    pub nonce: [u8; 16],
    pub expires_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    #[error("malformed assertion")]
    Malformed,
    #[error("ambiguous credentials")]
    Ambiguous,
    #[error("unknown signing key")]
    UnknownKey,
    #[error("bad signature")]
    BadSignature,
    #[error("assertion outside its validity window")]
    Expired,
    #[error("no registered principal")]
    Unregistered,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct MountMap {
    #[serde(default)]
    pub default: Option<MountSpec>,
    #[serde(default)]
    pub principals: HashMap<String, MountSpec>,
}

impl MountMap {
    /// The mount for a principal's non-public calls (Requirement 12.4).
    pub fn for_principal(&self, principal: &str) -> Option<&MountSpec> {
        self.principals.get(principal)
    }
}

#[derive(Default)]
pub struct Verifier {
    keys: HashMap<String, VerifyingKey>,
    principals: HashMap<String, String>,
}

fn decode_b64(text: &str) -> Option<Vec<u8>> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    let text = text.trim();
    [STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD]
        .iter()
        .find_map(|engine| engine.decode(text).ok())
}

/// Pick the assertion bytes from the two accepted headers. Both present is
/// ambiguous; neither is anonymous (`Ok(None)`).
pub fn extract_assertion(
    header: Option<&str>,
    authorization: Option<&str>,
) -> Result<Option<Vec<u8>>, AuthError> {
    let bearer = authorization.map(|value| {
        value
            .strip_prefix("Bearer ")
            .or_else(|| value.strip_prefix("bearer "))
            .ok_or(AuthError::Malformed)
    });
    match (header, bearer) {
        (Some(_), Some(_)) => Err(AuthError::Ambiguous),
        (Some(h), None) => decode_b64(h).map(Some).ok_or(AuthError::Malformed),
        (None, Some(b)) => decode_b64(b?).map(Some).ok_or(AuthError::Malformed),
        (None, None) => Ok(None),
    }
}

impl Verifier {
    pub fn new(keys: HashMap<String, VerifyingKey>, principals: HashMap<String, String>) -> Self {
        Self { keys, principals }
    }

    /// Load `oia1-keys.json` and `principals.json`; absent files mean nobody
    /// can authenticate (everyone is anonymous → public only).
    pub fn load(keys_file: &Path, principals_file: &Path) -> Result<Self> {
        let mut keys = HashMap::new();
        if keys_file.exists() {
            let raw: HashMap<String, String> =
                serde_json::from_value(crate::catalog::read_json(keys_file)?)
                    .context("oia1-keys.json shape")?;
            for (id, b64) in raw {
                let bytes: [u8; 32] = decode_b64(&b64)
                    .and_then(|b| b.try_into().ok())
                    .with_context(|| format!("verifying key {id} is not 32 base64 bytes"))?;
                keys.insert(
                    id.clone(),
                    VerifyingKey::from_bytes(&bytes)
                        .with_context(|| format!("verifying key {id}"))?,
                );
            }
        }
        let principals = if principals_file.exists() {
            serde_json::from_value(crate::catalog::read_json(principals_file)?)
                .context("principals.json shape")?
        } else {
            HashMap::new()
        };
        Ok(Self { keys, principals })
    }

    pub fn verify(&self, wire: &[u8], now: i64) -> Result<Identity, AuthError> {
        let signed = SignedAssertion::from_wire(wire).map_err(|_| AuthError::Malformed)?;
        let assertion = &signed.assertion;
        let key = self
            .keys
            .get(&assertion.decoy_key_id)
            .ok_or(AuthError::UnknownKey)?;
        verify_signature(assertion, &signed.signature, key).map_err(|_| AuthError::BadSignature)?;
        let lifetime = assertion.expires_at - assertion.issued_at;
        if assertion.issued_at > now + SKEW
            || now >= assertion.expires_at
            || lifetime <= 0
            || lifetime > MAX_LIFETIME_SECS as i64
        {
            return Err(AuthError::Expired);
        }
        let principal_id = self
            .principals
            .get(&assertion.human_pubkey)
            .cloned()
            .ok_or(AuthError::Unregistered)?;
        Ok(Identity {
            principal_id,
            human_pubkey: assertion.human_pubkey.clone(),
            nonce: assertion.nonce,
            expires_at: assertion.expires_at,
        })
    }
}

/// Binds each assertion nonce to the first session that used it
/// (Requirement 12.3).
#[derive(Default)]
pub struct NonceBindings {
    inner: Mutex<HashMap<[u8; 16], (String, i64)>>,
}

impl NonceBindings {
    /// True if `session` may use this nonce.
    pub fn bind(&self, nonce: [u8; 16], session: &str, expires_at: i64, now: i64) -> bool {
        let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|_, (_, expiry)| *expiry > now);
        match map.get(&nonce) {
            Some((bound, _)) => bound == session,
            None => {
                map.insert(nonce, (session.to_owned(), expires_at));
                true
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod test_keys {
    use ed25519_dalek::{Signer, SigningKey};
    use op_identity::oracle_assertion::{OracleIdentityAssertion, SignedAssertion};
    use std::net::{IpAddr, Ipv4Addr};

    pub const PUBKEY: &str = "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=";

    pub fn signing() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    pub fn wire(key_id: &str, issued: i64, expires: i64, nonce: u8) -> Vec<u8> {
        let assertion = OracleIdentityAssertion {
            human_pubkey: PUBKEY.into(),
            issued_at: issued,
            expires_at: expires,
            nonce: [nonce; 16],
            tunnel_inner_ip: IpAddr::V4(Ipv4Addr::new(10, 200, 0, 7)),
            decoy_key_id: key_id.into(),
        };
        let signature = signing().sign(&assertion.signing_bytes()).to_bytes();
        SignedAssertion {
            assertion,
            signature,
        }
        .to_wire()
    }
}

#[cfg(test)]
mod tests {
    use super::test_keys::*;
    use super::*;

    fn verifier() -> Verifier {
        let mut keys = HashMap::new();
        keys.insert("k1".to_owned(), signing().verifying_key());
        let mut principals = HashMap::new();
        principals.insert(PUBKEY.to_owned(), "p-1".to_owned());
        Verifier::new(keys, principals)
    }

    #[test]
    fn valid_assertion_resolves_the_principal() {
        let identity = verifier().verify(&wire("k1", 1000, 1600, 1), 1100).unwrap();
        assert_eq!(identity.principal_id, "p-1");
        assert_eq!(identity.nonce, [1; 16]);
    }

    #[test]
    fn every_bad_assertion_is_refused() {
        let v = verifier();
        assert_eq!(v.verify(b"junk", 1100), Err(AuthError::Malformed));
        assert_eq!(
            v.verify(&wire("k2", 1000, 1600, 1), 1100),
            Err(AuthError::UnknownKey)
        );
        let mut tampered = wire("k1", 1000, 1600, 1);
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert_eq!(v.verify(&tampered, 1100), Err(AuthError::BadSignature));
        assert_eq!(
            v.verify(&wire("k1", 1000, 1600, 1), 1600),
            Err(AuthError::Expired)
        );
        assert_eq!(
            v.verify(&wire("k1", 2000, 2600, 1), 1100),
            Err(AuthError::Expired)
        );
        assert_eq!(
            v.verify(&wire("k1", 1000, 3000, 1), 1100),
            Err(AuthError::Expired),
            "lifetime over 900 s"
        );
        let empty = Verifier::new(
            HashMap::from([("k1".to_owned(), signing().verifying_key())]),
            HashMap::new(),
        );
        assert_eq!(
            empty.verify(&wire("k1", 1000, 1600, 1), 1100),
            Err(AuthError::Unregistered)
        );
    }

    #[test]
    fn header_extraction() {
        use base64::engine::general_purpose::STANDARD;
        let bytes = wire("k1", 1000, 1600, 1);
        let b64 = STANDARD.encode(&bytes);
        assert_eq!(
            extract_assertion(Some(&b64), None).unwrap(),
            Some(bytes.clone())
        );
        assert_eq!(
            extract_assertion(None, Some(&format!("Bearer {b64}"))).unwrap(),
            Some(bytes)
        );
        assert_eq!(extract_assertion(None, None).unwrap(), None);
        assert_eq!(
            extract_assertion(Some(&b64), Some(&format!("Bearer {b64}"))),
            Err(AuthError::Ambiguous)
        );
        assert_eq!(
            extract_assertion(None, Some("Basic x")),
            Err(AuthError::Malformed)
        );
        assert_eq!(
            extract_assertion(Some("!!!"), None),
            Err(AuthError::Malformed)
        );
    }

    #[test]
    fn nonce_binds_to_one_session() {
        let bindings = NonceBindings::default();
        assert!(bindings.bind([1; 16], "s1", 200, 100));
        assert!(bindings.bind([1; 16], "s1", 200, 150));
        assert!(!bindings.bind([1; 16], "s2", 200, 150));
        // After expiry the record is pruned (the assertion itself is then
        // rejected by `verify`).
        assert!(bindings.bind([1; 16], "s2", 400, 250));
    }

    #[test]
    fn config_files_load() {
        use base64::engine::general_purpose::STANDARD;
        let dir = tempfile::tempdir().unwrap();
        let keys = dir.path().join("k.json");
        let principals = dir.path().join("p.json");
        std::fs::write(
            &keys,
            serde_json::json!({"k1": STANDARD.encode(signing().verifying_key().to_bytes())})
                .to_string(),
        )
        .unwrap();
        std::fs::write(&principals, serde_json::json!({PUBKEY: "p-1"}).to_string()).unwrap();
        let v = Verifier::load(&keys, &principals).unwrap();
        assert!(v.verify(&wire("k1", 1000, 1600, 1), 1100).is_ok());
        let none = Verifier::load(&dir.path().join("absent"), &dir.path().join("absent2")).unwrap();
        assert_eq!(
            none.verify(&wire("k1", 1000, 1600, 1), 1100),
            Err(AuthError::UnknownKey)
        );
        std::fs::write(&keys, r#"{"k1":"c2hvcnQ="}"#).unwrap();
        assert!(Verifier::load(&keys, &principals).is_err());
    }
}
