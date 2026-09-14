//! Registration helpers for signup flows.
//!
//! Centralize WireGuard key generation and magic-link token creation so
//! web/API layers can reuse a single identity implementation.

use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use rand::distributions::Alphanumeric;
use rand::rngs::OsRng;
use rand::Rng;
use x25519_dalek::{PublicKey, StaticSecret};

/// WireGuard keypair used for user identity and VPN config.
#[derive(Debug, Clone)]
pub struct WireGuardKeyPair {
    pub private_key: String,
    pub public_key: String,
}

/// Generate a new WireGuard keypair.
pub fn generate_wireguard_keypair() -> WireGuardKeyPair {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);

    WireGuardKeyPair {
        private_key: BASE64.encode(secret.as_bytes()),
        public_key: BASE64.encode(public.as_bytes()),
    }
}

/// Generate a random token suitable for magic-link flows.
pub fn generate_magic_link_token(len: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_wireguard_keypair() {
        let keypair = generate_wireguard_keypair();
        assert_eq!(keypair.private_key.len(), 44);
        assert_eq!(keypair.public_key.len(), 44);
    }

    #[test]
    fn generates_magic_token_with_requested_length() {
        let token = generate_magic_link_token(32);
        assert_eq!(token.len(), 32);
    }
}
