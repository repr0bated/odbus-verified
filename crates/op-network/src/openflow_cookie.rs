//! Flow identity for the `openflow` plugin: the cookie is derived, never stored.
//!
//! Cookie = `0x33444348_xxxxxxxx`. The high 32 bits are the fixed 3tched
//! prefix; the low 32 bits are the first 32 bits of SHA-256 over the
//! canonical `(bridge, table, priority, match)`. The same flow always gets
//! the same cookie, so the switch stays the only record. Two different
//! identities that share low bits are a collision and are refused.

use anyhow::{Result, bail};
use rovs_openflow::Match;
use sha2::{Digest, Sha256};

/// High 32 bits of every plugin-assigned cookie.
pub const COOKIE_PREFIX: u64 = 0x3344_4348_0000_0000;
/// Mask selecting the prefix half of a cookie.
pub const COOKIE_PREFIX_MASK: u64 = 0xffff_ffff_0000_0000;
/// Domain tag, so the digest is not reusable for any other purpose.
const DOMAIN: &[u8] = b"3tched.openflow.cookie.v1\0";
/// Bridge names are at most 15 bytes (IFNAMSIZ - 1).
pub const MAX_BRIDGE_LEN: usize = 15;

/// The fields that identify a flow on a bridge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowIdentity {
    pub bridge: String,
    pub table: u8,
    pub priority: u16,
    /// The match as rovs encodes it (canonical OXM order).
    pub match_bytes: Vec<u8>,
}

impl FlowIdentity {
    pub fn new(bridge: &str, table: u8, priority: u16, m: &Match) -> Result<Self> {
        if bridge.is_empty() || bridge.len() > MAX_BRIDGE_LEN {
            bail!(
                "bridge name must be 1..={MAX_BRIDGE_LEN} bytes, got {}",
                bridge.len()
            );
        }
        Ok(Self {
            bridge: bridge.to_owned(),
            table,
            priority,
            match_bytes: m.encode(),
        })
    }

    /// The derived cookie.
    pub fn cookie(&self) -> u64 {
        let mut h = Sha256::new();
        h.update(DOMAIN);
        // Length-prefixed bridge, so ("ab", ..) and ("a", "b..") never collide.
        h.update([self.bridge.len() as u8]);
        h.update(self.bridge.as_bytes());
        h.update([self.table]);
        h.update(self.priority.to_be_bytes());
        h.update(&self.match_bytes);
        let d = h.finalize();
        COOKIE_PREFIX | u64::from(u32::from_be_bytes([d[0], d[1], d[2], d[3]]))
    }

    /// Whether `existing` (same cookie on the switch) is this same flow.
    /// `Ok(true)` it is, `Ok(false)` the cookie is free; a different flow
    /// holding the cookie is a collision error.
    pub fn check_cookie_holder(&self, holder: Option<&FlowIdentity>) -> Result<bool> {
        match holder {
            None => Ok(false),
            Some(h) if h == self => Ok(true),
            Some(h) => bail!(
                "cookie collision: {:#x} is held by a different flow (bridge {}, table {}, priority {})",
                self.cookie(),
                h.bridge,
                h.table,
                h.priority
            ),
        }
    }
}

/// True when `cookie` was assigned by this plugin (carries the prefix).
pub fn is_plugin_cookie(cookie: u64) -> bool {
    cookie & COOKIE_PREFIX_MASK == COOKIE_PREFIX
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(bridge: &str, table: u8, prio: u16, m: &Match) -> FlowIdentity {
        FlowIdentity::new(bridge, table, prio, m).unwrap()
    }

    #[test]
    fn empty_match_encoding_is_the_pinned_layout() {
        assert_eq!(Match::new().encode(), [0, 1, 0, 4, 0, 0, 0, 0]);
    }

    #[test]
    fn cookie_matches_independent_sha256_vectors() {
        // sha256 of DOMAIN || len(bridge) || bridge || table || prio_be || match,
        // computed with `sha256sum` outside this code.
        assert_eq!(
            id("ovsbr0", 0, 10, &Match::new()).cookie(),
            0x3344_4348_1aa9_a527
        );
        assert_eq!(
            id("ovsbr0", 0, 0, &Match::new()).cookie(),
            0x3344_4348_bf93_851b
        );
    }

    #[test]
    fn same_identity_same_cookie() {
        let m = Match::new().in_port(1);
        assert_eq!(
            id("ovsbr0", 0, 10, &m).cookie(),
            id("ovsbr0", 0, 10, &m).cookie()
        );
    }

    #[test]
    fn each_identity_field_changes_the_cookie() {
        let m = Match::new().in_port(1);
        let base = id("ovsbr0", 0, 10, &m).cookie();
        assert_ne!(base, id("ovsbr1", 0, 10, &m).cookie());
        assert_ne!(base, id("ovsbr0", 1, 10, &m).cookie());
        assert_ne!(base, id("ovsbr0", 0, 11, &m).cookie());
        assert_ne!(base, id("ovsbr0", 0, 10, &Match::new().in_port(2)).cookie());
    }

    #[test]
    fn cookie_is_never_zero_and_carries_the_prefix() {
        let c = id("ovsbr0", 0, 0, &Match::new()).cookie();
        assert_ne!(c, 0);
        assert!(is_plugin_cookie(c));
    }

    #[test]
    fn foreign_cookies_are_not_plugin_cookies() {
        assert!(!is_plugin_cookie(0));
        assert!(!is_plugin_cookie(0x1234_5678_1aa9_a527));
    }

    #[test]
    fn free_cookie_and_same_flow_pass_and_a_different_flow_collides() {
        let a = id("ovsbr0", 0, 10, &Match::new().in_port(1));
        let b = id("ovsbr0", 0, 10, &Match::new().in_port(2));
        assert!(!a.check_cookie_holder(None).unwrap());
        assert!(a.check_cookie_holder(Some(&a)).unwrap());
        let err = a.check_cookie_holder(Some(&b)).unwrap_err();
        assert!(err.to_string().contains("collision"), "{err}");
    }

    #[test]
    fn bridge_name_bounds() {
        assert!(FlowIdentity::new("", 0, 0, &Match::new()).is_err());
        assert!(FlowIdentity::new("0123456789abcdef", 0, 0, &Match::new()).is_err());
        assert!(FlowIdentity::new("0123456789abcde", 0, 0, &Match::new()).is_ok());
    }
}
