//! Seed-derived WireGuard identity + local, non-custodial recovery.
//!
//! The user *is* their WireGuard key. To make identity recoverable without any
//! server-side custody, the keypair is derived deterministically from a 256-bit
//! recovery seed (rendered as a BIP39 mnemonic the user backs up OFF the machine).
//! Re-deriving from the same seed yields the same pubkey, which the server
//! recognizes via `user_exists` — no PII, no custodian.
//!
//! Custody is LOCAL-ONLY: the seed is written under the user's config dir with
//! `0600` (owner-only) permissions. The server never sees the seed or private key.
//! If the machine is lost and no off-machine mnemonic backup exists, the identity
//! is unrecoverable by design — there is no server lever to pull.

use anyhow::{anyhow, Context, Result};
use bip39::Mnemonic;
use rand::rngs::OsRng;
use rand::RngCore;

use crate::registration::WireGuardKeyPair;

/// Domain-separation context for the Blake3 KDF.
const KDF_CONTEXT: &str = "op-identity wireguard-identity v1";

/// Generate a fresh 256-bit recovery seed.
pub fn generate_seed() -> [u8; 32] {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    seed
}

/// Render a 256-bit seed as a 24-word BIP39 mnemonic (the recovery phrase).
pub fn seed_to_mnemonic(seed: &[u8; 32]) -> Result<String> {
    let m = Mnemonic::from_entropy(seed).context("seed -> mnemonic")?;
    Ok(m.to_string())
}

/// Parse a BIP39 mnemonic back into its 256-bit seed (recovery).
pub fn mnemonic_to_seed(mnemonic: &str) -> Result<[u8; 32]> {
    let m =
        Mnemonic::parse(mnemonic.trim()).map_err(|e| anyhow!("invalid recovery phrase: {e}"))?;
    let (entropy, len) = m.to_entropy_array();
    if len != 32 {
        return Err(anyhow!(
            "recovery phrase must encode 256 bits (24 words), got {len} bytes"
        ));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&entropy[..32]);
    Ok(out)
}

/// Deterministically derive the WireGuard keypair from a recovery seed.
/// The same seed always yields the same pubkey (= the stable identity).
pub fn derive_keypair(seed: &[u8; 32]) -> WireGuardKeyPair {
    use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
    use x25519_dalek::{PublicKey, StaticSecret};

    let key_material = blake3::derive_key(KDF_CONTEXT, seed);
    let secret = StaticSecret::from(key_material);
    let public = PublicKey::from(&secret);
    WireGuardKeyPair {
        private_key: BASE64.encode(secret.as_bytes()),
        public_key: BASE64.encode(public.as_bytes()),
    }
}

/// Recover the keypair from a backed-up mnemonic.
pub fn recover_keypair(mnemonic: &str) -> Result<WireGuardKeyPair> {
    Ok(derive_keypair(&mnemonic_to_seed(mnemonic)?))
}

/// Local-only seed store path: `<config_dir>/op-identity/seed.mnemonic`.
fn seed_path() -> Result<std::path::PathBuf> {
    let dir = dirs::config_dir()
        .ok_or_else(|| anyhow!("no user config dir"))?
        .join("op-identity");
    std::fs::create_dir_all(&dir).context("create op-identity config dir")?;
    Ok(dir.join("seed.mnemonic"))
}

/// Write the mnemonic to the local store with owner-only (0600) permissions.
/// The server never holds this; only the local user can read it.
pub fn store_mnemonic_local(mnemonic: &str) -> Result<std::path::PathBuf> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let path = seed_path()?;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    f.write_all(mnemonic.trim().as_bytes())?;
    f.write_all(b"\n")?;
    // Re-assert perms in case the file pre-existed with looser bits.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok(path)
}

/// Load the locally stored mnemonic, if present.
pub fn load_mnemonic_local() -> Result<String> {
    let path = seed_path()?;
    let s = std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    Ok(s.trim().to_string())
}

/// First-run provisioning: mint a seed, store it locally (0600), derive the keypair.
/// Returns the keypair plus the mnemonic to surface to the user ONCE for off-machine backup.
pub fn provision_local_identity() -> Result<(WireGuardKeyPair, String)> {
    let seed = generate_seed();
    let mnemonic = seed_to_mnemonic(&seed)?;
    store_mnemonic_local(&mnemonic)?;
    let keypair = derive_keypair(&seed);
    Ok((keypair, mnemonic))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derivation_is_deterministic() {
        let seed = [7u8; 32];
        let a = derive_keypair(&seed);
        let b = derive_keypair(&seed);
        assert_eq!(a.public_key, b.public_key);
        assert_eq!(a.private_key, b.private_key);
    }

    #[test]
    fn different_seeds_differ() {
        let a = derive_keypair(&[1u8; 32]);
        let b = derive_keypair(&[2u8; 32]);
        assert_ne!(a.public_key, b.public_key);
    }

    #[test]
    fn mnemonic_round_trip_recovers_same_key() {
        let seed = generate_seed();
        let original = derive_keypair(&seed);
        let mnemonic = seed_to_mnemonic(&seed).unwrap();
        let recovered = recover_keypair(&mnemonic).unwrap();
        assert_eq!(original.public_key, recovered.public_key);
        assert_eq!(original.private_key, recovered.private_key);
    }

    #[test]
    fn mnemonic_is_24_words() {
        let m = seed_to_mnemonic(&[0u8; 32]).unwrap();
        assert_eq!(m.split_whitespace().count(), 24);
    }
}
