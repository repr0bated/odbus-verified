//! WireGuard keypair as the object identity.
//!
//! The system identity is a WireGuard (X25519/Curve25519) keypair: the
//! base64 public key is the stable identity anchor embedded in every blob
//! manifest; the private key stays local (0600 file), never enters a blob,
//! and doubles as the node's transport credential — identity and reachability
//! are the same key, exactly like a WireGuard peer.
//!
//! The dual-identifier model from subid-taxonomy.md still holds: `subid` is
//! the human taxonomy key, the WG public key is the machine identity that
//! `uuid`-style references anchor to.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io;
use std::path::Path;

/// Public identity embedded in `BlobManifest` — safe to publish.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlobIdentity {
    /// WireGuard-format base64 X25519 public key (the identity).
    pub wg_public_key: String,
    /// sha256(public key bytes) first 16 hex chars — short stable id for
    /// filenames, logs, and subid registry `uuid` pairing.
    pub key_id: String,
    /// The session bound to this identity.
    pub session: SessionBinding,
}

/// Session semantics: the session is the keypair. It is derived
/// deterministically from the public key and persists for the life of the
/// account — no expiry, no per-connection rotation. Rotating the account's
/// WG keypair is the only thing that ends a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionBinding {
    /// sha256("opdbus-session:" + public key bytes), hex — stable for the
    /// account lifetime.
    pub session_id: String,
    pub lifespan: SessionLifespan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionLifespan {
    /// Persistent for the life of the account (the only supported policy).
    AccountPersistent,
}

/// Local keypair. Only `public_identity()` output may be serialized into
/// blobs; the secret is deliberately not `Serialize`.
pub struct WgKeypair {
    secret: x25519_dalek::StaticSecret,
}

impl WgKeypair {
    /// Generate a fresh identity keypair (clamped X25519 scalar, OS entropy) —
    /// byte-compatible with `wg genkey`.
    pub fn generate() -> Self {
        Self {
            secret: x25519_dalek::StaticSecret::random_from_rng(rand_core::OsRng),
        }
    }

    /// Load a WireGuard-format base64 private key (e.g. a `wg genkey` output
    /// or an existing interface key) so the node's existing WG identity IS the
    /// blob identity.
    pub fn from_base64(private_b64: &str) -> Result<Self, String> {
        let bytes = b64_decode(private_b64.trim())?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| "private key must be 32 bytes".to_string())?;
        Ok(Self {
            secret: x25519_dalek::StaticSecret::from(arr),
        })
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let s = std::fs::read_to_string(path)?;
        Self::from_base64(&s).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    /// Persist the private key, WireGuard-style (single base64 line, 0600).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, format!("{}\n", self.private_base64()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    pub fn private_base64(&self) -> String {
        b64_encode(self.secret.as_bytes())
    }

    pub fn public_base64(&self) -> String {
        let public = x25519_dalek::PublicKey::from(&self.secret);
        b64_encode(public.as_bytes())
    }

    /// The publishable identity block for blob manifests.
    pub fn public_identity(&self) -> BlobIdentity {
        let public = x25519_dalek::PublicKey::from(&self.secret);
        let mut h = Sha256::new();
        h.update(public.as_bytes());
        let digest = h.finalize();

        let mut s = Sha256::new();
        s.update(b"opdbus-session:");
        s.update(public.as_bytes());
        let session_digest = s.finalize();

        BlobIdentity {
            wg_public_key: b64_encode(public.as_bytes()),
            key_id: crate::schema::hex(&digest)[..16].to_string(),
            session: SessionBinding {
                session_id: crate::schema::hex(&session_digest),
                lifespan: SessionLifespan::AccountPersistent,
            },
        }
    }
}

// --- minimal std base64 (RFC 4648, with padding — WireGuard key format) ---

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            B64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

pub fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Result<u32, String> {
        match c {
            b'A'..=b'Z' => Ok((c - b'A') as u32),
            b'a'..=b'z' => Ok((c - b'a' + 26) as u32),
            b'0'..=b'9' => Ok((c - b'0' + 52) as u32),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(format!("invalid base64 byte {c:#x}")),
        }
    }
    let s = s.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.chunks(4) {
        if chunk.len() == 1 {
            return Err("truncated base64".into());
        }
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keypair_roundtrip_and_identity() {
        let kp = WgKeypair::generate();
        let priv_b64 = kp.private_base64();
        assert_eq!(priv_b64.len(), 44); // 32 bytes -> 44 base64 chars, WG format
        let restored = WgKeypair::from_base64(&priv_b64).unwrap();
        assert_eq!(kp.public_base64(), restored.public_base64());

        let id = kp.public_identity();
        assert_eq!(id.wg_public_key.len(), 44);
        assert_eq!(id.key_id.len(), 16);
        assert_eq!(b64_decode(&id.wg_public_key).unwrap().len(), 32);
        // Session is keypair-derived and account-persistent: same key, same
        // session id, forever.
        assert_eq!(id.session.lifespan, SessionLifespan::AccountPersistent);
        assert_eq!(id.session, restored.public_identity().session);
    }

    #[test]
    fn b64_matches_known_vector() {
        assert_eq!(b64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(b64_decode("Zm9vYg==").unwrap(), b"foob");
    }
}
