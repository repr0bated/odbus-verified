//! Compatibility identity API for callers that inject a verified pubkey.
//! The host does not terminate the human WireGuard tunnel; live peer state is
//! owned by the decoy issuer and must not be inferred from host interfaces.

use tracing::debug;

fn injected_pubkey(value: Option<String>) -> anyhow::Result<String> {
    match value {
        Some(pubkey) if !pubkey.trim().is_empty() => Ok(pubkey),
        Some(_) => anyhow::bail!("WG_PUBKEY is empty"),
        None => anyhow::bail!(
            "WG_PUBKEY unset; host WireGuard interface discovery is not identity proof"
        ),
    }
}

/// WireGuard identity provider
#[derive(Debug, Clone)]
pub struct WireGuardIdentity {
    /// Interface name for optional WireGuard identity lookup.
    ///
    /// Deprecated: automatic interface discovery is deprecated; prefer setting
    /// WG_PUBKEY (or equivalent injected identity pubkey) directly.
    interface: Option<String>,
}

impl WireGuardIdentity {
    pub fn new() -> Self {
        let iface = std::env::var("WG_INTERFACE")
            .or_else(|_| std::env::var("FABRIC_WG_INTERFACE"))
            .ok();
        Self::with_interface(&iface.unwrap_or_default())
    }

    pub fn with_interface(interface: &str) -> Self {
        Self {
            interface: if interface.is_empty() {
                None
            } else {
                Some(interface.to_string())
            },
        }
    }

    /// Get the explicitly injected public key. Host interface discovery is
    /// not an identity proof in the decoy/WARP topology.
    pub fn get_local_pubkey(&self) -> anyhow::Result<String> {
        let pubkey = injected_pubkey(std::env::var("WG_PUBKEY").ok())?;
        debug!("Using WG_PUBKEY from environment");
        Ok(pubkey)
    }

    fn decoy_only_error(&self) -> anyhow::Error {
        anyhow::anyhow!(
            "live WireGuard peer discovery for {:?} belongs to the decoy issuer",
            self.interface
        )
    }

    /// Peer freshness must come from a decoy-signed assertion.
    pub fn is_recent_peer(&self, _pubkey: &str, _max_age_secs: u64) -> anyhow::Result<bool> {
        Err(self.decoy_only_error())
    }

    /// Exact peer mapping must come from the decoy-signed assertion.
    pub fn get_pubkey_for_ip(&self, _peer_ip: &str) -> anyhow::Result<Option<String>> {
        Err(self.decoy_only_error())
    }

    /// Connected peer inventory belongs to the decoy.
    pub fn get_connected_peers(&self) -> anyhow::Result<Vec<PeerInfo>> {
        Err(self.decoy_only_error())
    }

    /// The host has no WireGuard interface address for identity pickup.
    pub fn get_local_ip(&self) -> Option<String> {
        None
    }
}

impl Default for WireGuardIdentity {
    fn default() -> Self {
        Self::new()
    }
}

/// Information about a WireGuard peer
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub pubkey: String,
    pub last_handshake: u64,
    pub allowed_ips: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_identity_requires_explicit_injected_pubkey() {
        assert!(injected_pubkey(None).is_err());
        assert!(injected_pubkey(Some(" ".into())).is_err());
        assert_eq!(
            injected_pubkey(Some("verified-key".into())).unwrap(),
            "verified-key"
        );
        let legacy = WireGuardIdentity::with_interface("wg-opdbus");
        assert!(legacy.is_recent_peer("peer", 180).is_err());
        assert!(legacy.get_pubkey_for_ip("10.0.0.5").is_err());
        assert!(legacy.get_connected_peers().is_err());
        assert_eq!(legacy.get_local_ip(), None);
    }
}
