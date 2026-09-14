//! Oracle identity assertions (OIA1).
//!
//! An `OracleIdentityAssertion` is a short-lived, Ed25519-signed identity
//! token issued by the external Oracle decoy (the sole WireGuard termination
//! point) after a human authenticates. It rides as gRPC metadata inside the
//! existing TLS channel and is validated solely by `op-grpc-bridge`.
//!
//! ## Canonical OIA1 wire format
//!
//! ```text
//! to_wire()      = b"OIA1" || assertion_bytes || signature[64]
//! signing_bytes()= b"OIA1" || assertion_bytes
//! assertion_bytes =
//!     u32le(len) || human_pubkey (UTF-8)
//!     || issued_at  (i64 little-endian, unix seconds)
//!     || expires_at (i64 little-endian, unix seconds)
//!     || nonce[16]
//!     || ip_family (1 byte: 4 | 6) || ip_addr (4 or 16 bytes)
//!     || u32le(len) || decoy_key_id (UTF-8)
//! ```
//!
//! The field order is fixed, strings are length-prefixed, and no serde/JSON
//! framing appears anywhere in the signing path. The versioned magic is part
//! of the signed payload so a signature is bound to the OIA1 layout.
//!
//! Decode and verify are separate stages: [`SignedAssertion::from_wire`] is
//! framing-only and never verifies the signature; [`verify_signature`] checks
//! a signature over the canonical signing bytes.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand::RngCore;

/// Versioned envelope magic for the OIA1 format.
const WIRE_MAGIC: &[u8; 4] = b"OIA1";

/// Hard cap on assertion lifetime, in seconds. An issuer configured with a
/// larger `max_lifetime` is still clamped to this cap.
pub const MAX_LIFETIME_SECS: u64 = 900;

const NONCE_LEN: usize = 16;
const SIGNATURE_LEN: usize = 64;

/// Errors produced by the assertion codec, issuer, and verifier.
#[derive(thiserror::Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssertionError {
    /// The envelope is not a well-formed OIA1 frame (bad magic, truncation,
    /// trailing bytes, corrupt interior fields).
    #[error("malformed OIA1 envelope")]
    Malformed,
    /// The signature does not verify over the canonical signing bytes.
    #[error("signature does not verify over the canonical signing bytes")]
    BadSignature,
    /// The requested ttl exceeds the issuer's (capped) maximum lifetime.
    #[error("assertion lifetime exceeds the maximum allowed")]
    LifetimeTooLong,
    /// The requested ttl is zero; lifetimes must be positive.
    #[error("assertion lifetime must be positive")]
    NonPositiveLifetime,
}

/// A short-lived identity assertion issued by the Oracle decoy.
///
/// Exactly these six fields, in this order, length-prefixed, make up the
/// canonical signing/wire encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleIdentityAssertion {
    /// Base64 WireGuard pubkey of the human.
    pub human_pubkey: String,
    /// Issuance time, unix seconds.
    pub issued_at: i64,
    /// Expiry time, unix seconds.
    pub expires_at: i64,
    /// Random per-assertion nonce; the replay-cache key.
    pub nonce: [u8; NONCE_LEN],
    /// The human's NetMaker inner IP.
    pub netmaker_inner_ip: IpAddr,
    /// Identifies which decoy signing key produced the signature.
    pub decoy_key_id: String,
}

impl OracleIdentityAssertion {
    /// Canonical signing bytes: `b"OIA1" || assertion_bytes`. Deterministic;
    /// no serde anywhere in this path.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(128);
        out.extend_from_slice(WIRE_MAGIC);
        self.encode_body(&mut out);
        out
    }

    /// Append the six fields in the contractual fixed order.
    fn encode_body(&self, out: &mut Vec<u8>) {
        push_length_prefixed(out, &self.human_pubkey);
        out.extend_from_slice(&self.issued_at.to_le_bytes());
        out.extend_from_slice(&self.expires_at.to_le_bytes());
        out.extend_from_slice(&self.nonce);
        match self.netmaker_inner_ip {
            IpAddr::V4(v4) => {
                out.push(4);
                out.extend_from_slice(&v4.octets());
            }
            IpAddr::V6(v6) => {
                out.push(6);
                out.extend_from_slice(&v6.octets());
            }
        }
        push_length_prefixed(out, &self.decoy_key_id);
    }
}

fn push_length_prefixed(out: &mut Vec<u8>, s: &str) {
    let len = u32::try_from(s.len()).expect("string field fits in u32 length prefix");
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(s.as_bytes());
}

/// An assertion together with its Ed25519 signature over the canonical
/// signing bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedAssertion {
    pub assertion: OracleIdentityAssertion,
    pub signature: [u8; SIGNATURE_LEN],
}

impl SignedAssertion {
    /// OIA1 envelope: `b"OIA1" || assertion_bytes || signature[64]`.
    pub fn to_wire(&self) -> Vec<u8> {
        let mut out = self.assertion.signing_bytes();
        out.extend_from_slice(&self.signature);
        out
    }

    /// Framing-only decode. Parses the envelope and checks structural
    /// well-formedness (magic, lengths, UTF-8, IP family, exact 64-byte
    /// trailing signature, no trailing bytes); it NEVER verifies the
    /// signature. All decode failures are [`AssertionError::Malformed`]; this
    /// function never panics on adversarial input.
    pub fn from_wire(bytes: &[u8]) -> Result<Self, AssertionError> {
        let mut r = Reader::new(bytes);
        if r.take(WIRE_MAGIC.len())? != WIRE_MAGIC {
            return Err(AssertionError::Malformed);
        }
        let assertion = OracleIdentityAssertion {
            human_pubkey: r.read_string()?,
            issued_at: r.read_i64()?,
            expires_at: r.read_i64()?,
            nonce: r.read_array::<NONCE_LEN>()?,
            netmaker_inner_ip: r.read_ip()?,
            decoy_key_id: r.read_string()?,
        };
        let signature = r.read_array::<SIGNATURE_LEN>()?;
        if !r.is_consumed() {
            // Trailing bytes after the signature are rejected: the decoder
            // consumes exactly the envelope.
            return Err(AssertionError::Malformed);
        }
        Ok(SignedAssertion {
            assertion,
            signature,
        })
    }
}

/// Bounds-checked cursor over the wire bytes. Every read is checked; no
/// panics on malformed input.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], AssertionError> {
        let end = self.pos.checked_add(n).ok_or(AssertionError::Malformed)?;
        if end > self.buf.len() {
            return Err(AssertionError::Malformed);
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn is_consumed(&self) -> bool {
        self.pos == self.buf.len()
    }

    fn read_array<const N: usize>(&mut self) -> Result<[u8; N], AssertionError> {
        self.take(N)?
            .try_into()
            .map_err(|_| AssertionError::Malformed)
    }

    fn read_u32_le(&mut self) -> Result<u32, AssertionError> {
        Ok(u32::from_le_bytes(self.read_array::<4>()?))
    }

    fn read_i64(&mut self) -> Result<i64, AssertionError> {
        Ok(i64::from_le_bytes(self.read_array::<8>()?))
    }

    fn read_string(&mut self) -> Result<String, AssertionError> {
        let len = self.read_u32_le()? as usize;
        let raw = self.take(len)?;
        String::from_utf8(raw.to_vec()).map_err(|_| AssertionError::Malformed)
    }

    fn read_ip(&mut self) -> Result<IpAddr, AssertionError> {
        match self.take(1)?[0] {
            4 => Ok(IpAddr::V4(Ipv4Addr::from(self.read_array::<4>()?))),
            6 => Ok(IpAddr::V6(Ipv6Addr::from(self.read_array::<16>()?))),
            _ => Err(AssertionError::Malformed),
        }
    }
}

/// Issues signed assertions for the Oracle decoy. Local library stand-in for
/// the external decoy; also used by the E2E decoy simulator.
pub struct DecoyIssuer {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
    key_id: String,
    max_lifetime: Duration,
}

impl DecoyIssuer {
    /// Construct an issuer. `max_lifetime` is clamped to the hard cap of
    /// [`MAX_LIFETIME_SECS`] seconds, so the cap governs even when a larger
    /// value is configured.
    pub fn new(signing_key: SigningKey, key_id: impl Into<String>, max_lifetime: Duration) -> Self {
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
            key_id: key_id.into(),
            max_lifetime: max_lifetime.min(Duration::from_secs(MAX_LIFETIME_SECS)),
        }
    }

    /// The key id stamped into every issued assertion.
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// The Ed25519 verifying key counterpart of the signing key.
    pub fn verifying_key(&self) -> &VerifyingKey {
        &self.verifying_key
    }

    /// Issue a signed assertion.
    ///
    /// Total over string inputs: `human_pubkey` is carried verbatim (pubkey
    /// shape validation is the registry's job, not the issuer's). Rejects a
    /// zero ttl with [`AssertionError::NonPositiveLifetime`] and a ttl above
    /// the (capped) maximum lifetime with [`AssertionError::LifetimeTooLong`].
    /// Every call draws a fresh random nonce.
    pub fn issue(
        &self,
        human_pubkey: &str,
        inner_ip: IpAddr,
        ttl: Duration,
    ) -> Result<SignedAssertion, AssertionError> {
        if ttl.is_zero() {
            return Err(AssertionError::NonPositiveLifetime);
        }
        if ttl > self.max_lifetime {
            return Err(AssertionError::LifetimeTooLong);
        }
        let issued_at = chrono::Utc::now().timestamp();
        let expires_at = issued_at + ttl.as_secs() as i64;
        let mut nonce = [0u8; NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let assertion = OracleIdentityAssertion {
            human_pubkey: human_pubkey.to_string(),
            issued_at,
            expires_at,
            nonce,
            netmaker_inner_ip: inner_ip,
            decoy_key_id: self.key_id.clone(),
        };
        let signature = self.signing_key.sign(&assertion.signing_bytes()).to_bytes();
        Ok(SignedAssertion {
            assertion,
            signature,
        })
    }
}

/// Verify `signature` over the canonical signing bytes of `assertion`.
pub fn verify_signature(
    assertion: &OracleIdentityAssertion,
    signature: &[u8; SIGNATURE_LEN],
    key: &VerifyingKey,
) -> Result<(), AssertionError> {
    verify_signature_bytes(&assertion.signing_bytes(), signature, key)
}

/// Verify an Ed25519 signature over raw message bytes. Strict verification:
/// non-canonical encodings and small-order points are rejected.
fn verify_signature_bytes(
    msg: &[u8],
    signature: &[u8; SIGNATURE_LEN],
    key: &VerifyingKey,
) -> Result<(), AssertionError> {
    let sig = Signature::from_bytes(signature);
    key.verify_strict(msg, &sig)
        .map_err(|_| AssertionError::BadSignature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::collections::HashSet;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::time::Duration;

    const TEST_KEY_BYTES: [u8; 32] = [7u8; 32];

    /// A realistic base64 WireGuard pubkey (32 bytes -> 44 chars, padded).
    const SAMPLE_PUBKEY: &str = "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=";

    fn test_signing_key() -> SigningKey {
        SigningKey::from_bytes(&TEST_KEY_BYTES)
    }

    fn test_issuer() -> DecoyIssuer {
        DecoyIssuer::new(test_signing_key(), "decoy-key-1", Duration::from_secs(900))
    }

    fn test_ip() -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 200, 0, 7))
    }

    fn sample_assertion() -> OracleIdentityAssertion {
        OracleIdentityAssertion {
            human_pubkey: SAMPLE_PUBKEY.to_string(),
            issued_at: 1_700_000_000,
            expires_at: 1_700_000_900,
            nonce: [0xA5; 16],
            netmaker_inner_ip: test_ip(),
            decoy_key_id: "decoy-key-1".to_string(),
        }
    }

    fn signed_sample() -> SignedAssertion {
        SignedAssertion {
            assertion: sample_assertion(),
            signature: [0x5A; 64],
        }
    }

    /// VAL-ASSERT-001: wire round-trip is identity (IPv4).
    #[test]
    fn wire_roundtrip_is_identity() {
        let s = signed_sample();
        let wire = s.to_wire();
        let decoded = SignedAssertion::from_wire(&wire).expect("valid envelope decodes");
        assert_eq!(decoded, s);
    }

    /// VAL-ASSERT-002: round-trip preserves IPv6 and boundary field values.
    #[test]
    fn wire_roundtrip_ipv6_and_boundaries() {
        // IPv6 branch: 1-byte family + 16-byte address.
        let v6 = SignedAssertion {
            assertion: OracleIdentityAssertion {
                netmaker_inner_ip: IpAddr::V6(Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 1)),
                ..sample_assertion()
            },
            ..signed_sample()
        };
        assert_eq!(SignedAssertion::from_wire(&v6.to_wire()).unwrap(), v6);

        // Extreme-but-valid i64 magnitudes (far pre-epoch and near i64::MAX).
        let extreme = SignedAssertion {
            assertion: OracleIdentityAssertion {
                issued_at: -6_211_000_000_000,
                expires_at: i64::MAX - 42,
                ..sample_assertion()
            },
            ..signed_sample()
        };
        assert_eq!(
            SignedAssertion::from_wire(&extreme.to_wire()).unwrap(),
            extreme
        );

        // Maximum-length base64 pubkey with padding + a long decoy_key_id.
        let long = SignedAssertion {
            assertion: OracleIdentityAssertion {
                human_pubkey: SAMPLE_PUBKEY.to_string(), // 44 chars, '=' padding
                decoy_key_id: "d".repeat(512),
                ..sample_assertion()
            },
            ..signed_sample()
        };
        assert_eq!(SignedAssertion::from_wire(&long.to_wire()).unwrap(), long);
    }

    /// VAL-ASSERT-003: canonical encoding matches the golden byte layout.
    #[test]
    fn canonical_encoding_golden_vector() {
        let assertion = OracleIdentityAssertion {
            human_pubkey: "pk".to_string(),
            issued_at: 0x0102_0304_0506_0708,
            expires_at: 0x1112_1314_1516_1718,
            nonce: [
                0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD,
                0xAE, 0xAF,
            ],
            netmaker_inner_ip: IpAddr::V4(Ipv4Addr::new(10, 200, 0, 7)),
            decoy_key_id: "dk1".to_string(),
        };
        let signed = SignedAssertion {
            assertion,
            signature: [0x5A; 64],
        };

        // Golden layout, written out byte-for-byte per the contract:
        //   b"OIA1" || u32le(len) || human_pubkey || issued_at(i64 LE)
        //   || expires_at(i64 LE) || nonce[16] || family(1) || addr(4/16)
        //   || u32le(len) || decoy_key_id || signature[64]
        let mut golden = Vec::new();
        golden.extend_from_slice(b"OIA1"); // versioned magic
        golden.extend_from_slice(&[0x02, 0x00, 0x00, 0x00, b'p', b'k']); // human_pubkey
        golden.extend_from_slice(&[0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]); // issued_at
        golden.extend_from_slice(&[0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11]); // expires_at
        golden.extend_from_slice(&[
            0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6, 0xA7, 0xA8, 0xA9, 0xAA, 0xAB, 0xAC, 0xAD,
            0xAE, 0xAF,
        ]); // nonce
        golden.extend_from_slice(&[0x04, 10, 200, 0, 7]); // IPv4 family + address
        golden.extend_from_slice(&[0x03, 0x00, 0x00, 0x00, b'd', b'k', b'1']); // decoy_key_id

        assert_eq!(signed.assertion.signing_bytes(), golden);

        let mut golden_wire = golden.clone();
        golden_wire.extend_from_slice(&[0x5A; 64]);
        assert_eq!(signed.to_wire(), golden_wire);
        // Total length: 4 + (4+2) + 8 + 8 + 16 + (1+4) + (4+3) + 64.
        assert_eq!(signed.to_wire().len(), 118);
        // No serde JSON framing anywhere in the signing bytes.
        assert!(!golden.iter().any(|b| *b == b'{' || *b == b'"'));
    }

    /// VAL-ASSERT-004: encoding is deterministic.
    #[test]
    fn encoding_is_deterministic() {
        let s = signed_sample();
        let signing = s.assertion.signing_bytes();
        let wire = s.to_wire();
        for _ in 0..10 {
            assert_eq!(s.assertion.signing_bytes(), signing);
            assert_eq!(s.to_wire(), wire);
        }
        // Two independently constructed assertions with identical field values.
        let a = sample_assertion();
        let b = sample_assertion();
        assert_eq!(a.signing_bytes(), b.signing_bytes());
    }

    /// VAL-ASSERT-005: length-prefixing prevents field-boundary ambiguity.
    #[test]
    fn length_prefixes_prevent_field_ambiguity() {
        let a = OracleIdentityAssertion {
            human_pubkey: "ab".to_string(),
            decoy_key_id: "c".to_string(),
            ..sample_assertion()
        };
        let b = OracleIdentityAssertion {
            human_pubkey: "a".to_string(),
            decoy_key_id: "bc".to_string(),
            ..sample_assertion()
        };
        // A concatenation-ambiguous encoding would collide here.
        assert_ne!(a.signing_bytes(), b.signing_bytes());
    }

    /// VAL-ASSERT-006: from_wire rejects wrong-version magic.
    #[test]
    fn from_wire_rejects_wrong_magic() {
        let wire = signed_sample().to_wire();
        for bad_magic in [
            b"OIA0".as_slice(),
            b"OIA2".as_slice(),
            b"XXXX".as_slice(),
            &[0xDE, 0xAD, 0xBE, 0xEF][..],
        ] {
            let mut mutated = wire.clone();
            mutated[..4].copy_from_slice(bad_magic);
            assert_eq!(
                SignedAssertion::from_wire(&mutated),
                Err(AssertionError::Malformed),
                "magic {bad_magic:?} must be rejected"
            );
        }
        // Truncated / empty magic.
        assert_eq!(
            SignedAssertion::from_wire(b""),
            Err(AssertionError::Malformed)
        );
        assert_eq!(
            SignedAssertion::from_wire(b"O"),
            Err(AssertionError::Malformed)
        );
        assert_eq!(
            SignedAssertion::from_wire(b"OIA"),
            Err(AssertionError::Malformed)
        );
        // Control: unmodified envelope still decodes.
        assert!(SignedAssertion::from_wire(&wire).is_ok());
    }

    /// VAL-ASSERT-007: from_wire rejects trailing bytes.
    #[test]
    fn from_wire_rejects_trailing_bytes() {
        let wire = signed_sample().to_wire();
        assert!(SignedAssertion::from_wire(&wire).is_ok()); // control
        for suffix in [&[0x00][..], &[0x00, 0x01, 0x02][..], &[0x5A; 64][..]] {
            let mut mutated = wire.clone();
            mutated.extend_from_slice(suffix);
            assert!(
                SignedAssertion::from_wire(&mutated).is_err(),
                "trailing suffix of {} byte(s) must be rejected",
                suffix.len()
            );
        }
    }

    /// VAL-ASSERT-008: from_wire rejects truncated and malformed envelopes.
    #[test]
    fn from_wire_rejects_truncated_and_malformed() {
        assert_eq!(
            SignedAssertion::from_wire(b""),
            Err(AssertionError::Malformed)
        );
        assert_eq!(
            SignedAssertion::from_wire(b"OI"),
            Err(AssertionError::Malformed)
        );

        let wire = signed_sample().to_wire();
        // Every proper prefix of a valid encoding is a truncation.
        for i in 0..wire.len() {
            assert!(
                SignedAssertion::from_wire(&wire[..i]).is_err(),
                "prefix of len {i} must be rejected"
            );
        }
        // Trailing signature segment not exactly 64 bytes (32-byte sig).
        let body_len = wire.len() - 64;
        let short_sig = [&wire[..body_len], &[0x5A; 32][..]].concat();
        assert_eq!(
            SignedAssertion::from_wire(&short_sig),
            Err(AssertionError::Malformed)
        );
        // Control.
        assert!(SignedAssertion::from_wire(&wire).is_ok());
    }

    /// VAL-ASSERT-009: issue then verify_signature round-trips.
    #[test]
    fn issue_sign_verify_roundtrip() {
        let issuer = test_issuer();
        let signed = issuer
            .issue(SAMPLE_PUBKEY, test_ip(), Duration::from_secs(300))
            .expect("issue at valid ttl");
        verify_signature(&signed.assertion, &signed.signature, issuer.verifying_key())
            .expect("freshly issued assertion verifies");
    }

    /// VAL-ASSERT-010: tampering with any assertion byte fails verification.
    #[test]
    fn tampered_assertion_bytes_fail_verification() {
        let issuer = test_issuer();
        let signed = issuer
            .issue(SAMPLE_PUBKEY, test_ip(), Duration::from_secs(300))
            .unwrap();
        // Control.
        verify_signature(&signed.assertion, &signed.signature, issuer.verifying_key()).unwrap();

        // Flip one byte at every position of the signed payload.
        let payload = signed.assertion.signing_bytes();
        for i in 0..payload.len() {
            let mut mutated = payload.clone();
            mutated[i] ^= 0x01;
            assert_eq!(
                verify_signature_bytes(&mutated, &signed.signature, issuer.verifying_key()),
                Err(AssertionError::BadSignature),
                "mutation at byte {i} must fail verification"
            );
        }

        // A valid Ed25519 signature (same issuer key) over a DIFFERENT encoding
        // of identical field values must not verify against the canonical one.
        let a = &signed.assertion;
        let alternate_encoding = format!(
            "{}|{}|{}|{}|{}|{}",
            a.human_pubkey,
            a.issued_at,
            a.expires_at,
            a.nonce
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(","),
            a.netmaker_inner_ip,
            a.decoy_key_id
        );
        let alt_sig: [u8; 64] = test_signing_key()
            .sign(alternate_encoding.as_bytes())
            .to_bytes();
        assert_eq!(
            verify_signature(&signed.assertion, &alt_sig, issuer.verifying_key()),
            Err(AssertionError::BadSignature)
        );
    }

    /// VAL-ASSERT-011: tampered signature or wrong key fails verification.
    #[test]
    fn tampered_signature_and_wrong_key_fail() {
        let issuer = test_issuer();
        let signed = issuer
            .issue(SAMPLE_PUBKEY, test_ip(), Duration::from_secs(300))
            .unwrap();
        // Control.
        verify_signature(&signed.assertion, &signed.signature, issuer.verifying_key()).unwrap();

        // Flip every byte of the 64-byte signature, assertion untouched.
        for i in 0..64 {
            let mut sig = signed.signature;
            sig[i] ^= 0x01;
            assert_eq!(
                verify_signature(&signed.assertion, &sig, issuer.verifying_key()),
                Err(AssertionError::BadSignature),
                "signature byte {i} mutation must fail"
            );
        }

        // A different Ed25519 verifying key than the issuer's.
        let other_key = SigningKey::from_bytes(&[9u8; 32]);
        assert_eq!(
            verify_signature(
                &signed.assertion,
                &signed.signature,
                &other_key.verifying_key()
            ),
            Err(AssertionError::BadSignature)
        );
    }

    /// VAL-ASSERT-012: issued assertions carry the requested fields.
    #[test]
    fn issue_populates_fields() {
        let issuer = test_issuer();
        let ip = test_ip();
        let before = chrono::Utc::now().timestamp();
        let signed = issuer
            .issue(SAMPLE_PUBKEY, ip, Duration::from_secs(300))
            .unwrap();
        let after = chrono::Utc::now().timestamp();
        let a = &signed.assertion;
        assert_eq!(a.human_pubkey, SAMPLE_PUBKEY);
        assert_eq!(a.netmaker_inner_ip, ip);
        assert_eq!(a.decoy_key_id, issuer.key_id());
        assert!(
            a.issued_at >= before - 1 && a.issued_at <= after + 1,
            "issued_at {} within tolerance of now [{before}, {after}]",
            a.issued_at
        );
        assert_eq!(a.expires_at - a.issued_at, 300);
    }

    /// VAL-ASSERT-013: TTL hard-cap boundary — 900 s accepted, 901 s rejected.
    #[test]
    fn ttl_hard_cap_boundary() {
        let issuer = test_issuer(); // max_lifetime = 900 s
        assert!(issuer
            .issue(SAMPLE_PUBKEY, test_ip(), Duration::from_secs(900))
            .is_ok());
        assert_eq!(
            issuer
                .issue(SAMPLE_PUBKEY, test_ip(), Duration::from_secs(901))
                .unwrap_err(),
            AssertionError::LifetimeTooLong
        );
        assert_eq!(
            issuer
                .issue(SAMPLE_PUBKEY, test_ip(), Duration::from_secs(3600))
                .unwrap_err(),
            AssertionError::LifetimeTooLong
        );
    }

    /// VAL-ASSERT-014: ttl above configured max_lifetime is rejected; the
    /// 900 s hard cap governs even for a larger configured max_lifetime.
    #[test]
    fn ttl_above_max_lifetime_rejected() {
        let tight = DecoyIssuer::new(test_signing_key(), "decoy-key-1", Duration::from_secs(60));
        assert!(tight
            .issue(SAMPLE_PUBKEY, test_ip(), Duration::from_secs(60))
            .is_ok());
        assert_eq!(
            tight
                .issue(SAMPLE_PUBKEY, test_ip(), Duration::from_secs(61))
                .unwrap_err(),
            AssertionError::LifetimeTooLong
        );

        // Configured above the hard cap: issuance above 900 s is still rejected.
        let loose = DecoyIssuer::new(test_signing_key(), "decoy-key-1", Duration::from_secs(3600));
        assert!(loose
            .issue(SAMPLE_PUBKEY, test_ip(), Duration::from_secs(900))
            .is_ok());
        assert_eq!(
            loose
                .issue(SAMPLE_PUBKEY, test_ip(), Duration::from_secs(901))
                .unwrap_err(),
            AssertionError::LifetimeTooLong
        );
    }

    /// VAL-ASSERT-015: zero TTL is rejected (distinct from LifetimeTooLong).
    #[test]
    fn zero_ttl_rejected() {
        let issuer = test_issuer();
        let err = issuer
            .issue(SAMPLE_PUBKEY, test_ip(), Duration::ZERO)
            .unwrap_err();
        assert_eq!(err, AssertionError::NonPositiveLifetime);
        assert_ne!(err, AssertionError::LifetimeTooLong);
    }

    /// VAL-ASSERT-016: issuance never reuses a nonce.
    #[test]
    fn issuance_nonces_are_unique() {
        let issuer = test_issuer();
        let mut nonces = HashSet::new();
        let mut signatures = HashSet::new();
        let mut wires = HashSet::new();
        for _ in 0..100 {
            let s = issuer
                .issue(SAMPLE_PUBKEY, test_ip(), Duration::from_secs(300))
                .unwrap();
            assert!(nonces.insert(s.assertion.nonce), "nonce reused");
            assert!(signatures.insert(s.signature), "signature reused");
            assert!(wires.insert(s.to_wire()), "wire bytes reused");
        }
    }

    /// VAL-ASSERT-019: from_wire rejects corrupt interior bytes — no panic.
    #[test]
    fn from_wire_rejects_corrupt_interior_no_panic() {
        let wire = signed_sample().to_wire();
        let pubkey_len = sample_assertion().human_pubkey.len();
        // Offset of the IP family byte: magic(4) + len(4) + pubkey + i64 + i64 + nonce(16).
        let family_offset = 4 + 4 + pubkey_len + 8 + 8 + 16;

        // (a) IP family byte outside {4, 6}.
        let mut bad_family = wire.clone();
        bad_family[family_offset] = 0x05;
        assert_eq!(
            SignedAssertion::from_wire(&bad_family),
            Err(AssertionError::Malformed)
        );

        // (b) u32 length prefix that overruns the buffer.
        let mut overrun = wire.clone();
        overrun[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(
            SignedAssertion::from_wire(&overrun),
            Err(AssertionError::Malformed)
        );

        // (b2) u32 length prefix that swallows the signature region.
        let body_len = wire.len() - 64;
        let swallow_len = (body_len - 8) as u32;
        let mut swallow = wire.clone();
        swallow[4..8].copy_from_slice(&swallow_len.to_le_bytes());
        assert_eq!(
            SignedAssertion::from_wire(&swallow),
            Err(AssertionError::Malformed)
        );

        // (c) invalid UTF-8 in a string field (first human_pubkey content byte).
        let mut bad_utf8 = wire.clone();
        bad_utf8[8] = 0xFF;
        assert_eq!(
            SignedAssertion::from_wire(&bad_utf8),
            Err(AssertionError::Malformed)
        );

        // Control: unmodified envelope still decodes.
        assert!(SignedAssertion::from_wire(&wire).is_ok());
    }

    /// VAL-ASSERT-020: decode does not verify — parse/verify stage separation.
    #[test]
    fn decode_does_not_verify_parse_and_verify_are_separate() {
        let mut signed = signed_sample();
        signed.signature = [0x00; 64]; // well-formed envelope, invalid signature
        let decoded = SignedAssertion::from_wire(&signed.to_wire())
            .expect("decoding performs no cryptographic verification");
        assert_eq!(decoded, signed);
        // A subsequent verify on the decoded value fails.
        let issuer = test_issuer();
        assert_eq!(
            verify_signature(
                &decoded.assertion,
                &decoded.signature,
                issuer.verifying_key()
            ),
            Err(AssertionError::BadSignature)
        );
    }

    /// VAL-ASSERT-021: zero-length string fields round-trip byte-exactly.
    #[test]
    fn empty_string_fields_roundtrip_byte_exactly() {
        for (pk, dk) in [
            ("", ""),
            ("", "decoy-key-1"),
            ("dGVzdA==", ""),
            ("dGVzdA==", "decoy-key-1"),
        ] {
            let s = SignedAssertion {
                assertion: OracleIdentityAssertion {
                    human_pubkey: pk.to_string(),
                    decoy_key_id: dk.to_string(),
                    ..sample_assertion()
                },
                ..signed_sample()
            };
            let decoded = SignedAssertion::from_wire(&s.to_wire())
                .expect("empty string fields decode without misalignment");
            assert_eq!(decoded, s, "pk={pk:?} dk={dk:?}");
        }
    }

    /// VAL-ASSERT-022: DecoyIssuer::issue is total over strings.
    #[test]
    fn issue_is_total_over_string_inputs() {
        let issuer = test_issuer();
        // base64 of 16 bytes: valid base64, wrong length (not 32 bytes).
        let wrong_len_b64 = "AAAAAAAAAAAAAAAAAAAAAA==";
        for bad in ["not!base64!!!", wrong_len_b64, ""] {
            let signed = issuer
                .issue(bad, test_ip(), Duration::from_secs(60))
                .expect("issue does not reject malformed pubkey strings");
            assert_eq!(signed.assertion.human_pubkey, bad);
        }
    }
}
