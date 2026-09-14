//! Session genesis — the single author of the session identity anchor.
//!
//! The genesis is a blake3 hash minted exactly once per session, at arrival
//! (the first authenticated mutation). It is never recomputed for an existing
//! session. The output is stored immutably in the session record and verified
//! with a single equality on every subsequent request.
//!
//! OSCAL subid: `mut.service.session-genesis.mint@v1`

use blake3::Hasher;

/// Mint a session genesis — the immutable identity anchor.
///
/// Called exactly once per session, at arrival (first authenticated mutation).
/// The output is stored and never recomputed.
///
/// All inputs are raw bytes or integers — no encoding ambiguity. Callers must
/// decode base64 pubkeys and hex hashes before calling.
///
/// # Arguments
///
/// * `wg_pubkey` — WireGuard public key, decoded from base64 to 32 bytes.
/// * `chain_head_hash` — Exact native chain-head digest, decoded from hex.
///   The current EventChain uses 16 bytes; existing 32-byte callers retain
///   their original output. The identity producer validates the native width
///   before calling, and stores the original hash alongside the minted anchor.
/// * `head_timestamp` — Unix seconds of the chain head event.
/// * `catalog_hash` — Schema catalog hash, decoded from hex to 32 bytes
///   (`schema_catalog_hash()`).
/// * `arrival_timestamp` — `Utc::now().timestamp()` at mint time.
///
/// # Returns
///
/// A 32-byte blake3 digest. Callers hex-encode it for storage.
///
/// # Properties
///
/// - Pure function, no I/O, no side effects.
/// - Deterministic: same inputs → same output.
/// - Different `arrival_timestamp` → different output (uniqueness term).
pub fn mint_genesis(
    wg_pubkey: &[u8; 32],
    chain_head_hash: &[u8],
    head_timestamp: i64,
    catalog_hash: &[u8; 32],
    arrival_timestamp: i64,
) -> [u8; 32] {
    let mut hasher = Hasher::new();
    hasher.update(wg_pubkey);
    hasher.update(chain_head_hash);
    hasher.update(&head_timestamp.to_le_bytes());
    hasher.update(catalog_hash);
    hasher.update(&arrival_timestamp.to_le_bytes());
    *hasher.finalize().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_sixteen_byte_chain_heads_are_bound_without_zero_padding_or_substitution() {
        let key = [0x42; 32];
        let catalog = [0x77; 32];
        let head = [0x55; 16];
        let native = mint_genesis(&key, &head, 10, &catalog, 11);
        let mut changed = head;
        changed[15] ^= 1;
        assert_ne!(native, mint_genesis(&key, &changed, 10, &catalog, 11));
        assert_ne!(native, mint_genesis(&key, &[0; 32], 10, &catalog, 11));
        let mut padded = [0; 32];
        padded[..16].copy_from_slice(&head);
        assert_ne!(native, mint_genesis(&key, &padded, 10, &catalog, 11));
    }

    /// VAL-GENESIS-001: Same inputs produce identical output.
    #[test]
    fn mint_genesis_deterministic() {
        let pubkey = [0x42; 32];
        let chain_head = [0x55; 32];
        let catalog = [0x77; 32];
        let head_ts = 1_700_000_000_i64;
        let arrival_ts = 1_700_000_100_i64;

        let first = mint_genesis(&pubkey, &chain_head, head_ts, &catalog, arrival_ts);
        let second = mint_genesis(&pubkey, &chain_head, head_ts, &catalog, arrival_ts);
        assert_eq!(
            first, second,
            "identical inputs must produce identical genesis"
        );
    }

    /// VAL-GENESIS-002: Different arrival_timestamp produces different genesis.
    #[test]
    fn mint_genesis_uniqueness() {
        let pubkey = [0x42; 32];
        let chain_head = [0x55; 32];
        let catalog = [0x77; 32];
        let head_ts = 1_700_000_000_i64;

        let g1 = mint_genesis(&pubkey, &chain_head, head_ts, &catalog, 1_700_000_100);
        let g2 = mint_genesis(&pubkey, &chain_head, head_ts, &catalog, 1_700_000_200);
        assert_ne!(
            g1, g2,
            "different arrival timestamps must produce different genesis"
        );
    }

    /// VAL-GENESIS-003: All-zeros input does not panic and returns a valid hash.
    #[test]
    fn mint_genesis_all_bytes() {
        let zeros = [0u8; 32];
        let genesis = mint_genesis(&zeros, &zeros, 0, &zeros, 0);
        // Must be a valid 32-byte hash (not necessarily non-zero, but must not panic).
        assert_eq!(genesis.len(), 32);
    }

    /// VAL-GENESIS-004: Different pubkey produces different genesis.
    #[test]
    fn mint_genesis_different_pubkey() {
        let pubkey_a = [0x42; 32];
        let pubkey_b = [0x43; 32];
        let chain_head = [0x55; 32];
        let catalog = [0x77; 32];
        let head_ts = 1_700_000_000_i64;
        let arrival_ts = 1_700_000_100_i64;

        let g_a = mint_genesis(&pubkey_a, &chain_head, head_ts, &catalog, arrival_ts);
        let g_b = mint_genesis(&pubkey_b, &chain_head, head_ts, &catalog, arrival_ts);
        assert_ne!(g_a, g_b, "different pubkeys must produce different genesis");
    }
}
