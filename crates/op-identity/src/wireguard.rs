//! WireGuard identity detection and peer management.

use std::process::Command;
use tracing::debug;

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

    /// Get the local WireGuard public key (this machine's identity)
    pub fn get_local_pubkey(&self) -> anyhow::Result<String> {
        // Try environment variable first
        if let Ok(pubkey) = std::env::var("WG_PUBKEY") {
            debug!("Using WG_PUBKEY from environment");
            return Ok(pubkey);
        }

        let interface = self
            .interface
            .as_ref()
            .filter(|v| !v.is_empty())
            .map(String::as_str)
            .ok_or_else(|| anyhow::anyhow!("WG_PUBKEY unset and WG interface not configured"))?;

        // Fall back to live interface state (wg show <iface> public-key).
        let output = Command::new("wg")
            .args(["show", interface, "public-key"])
            .output()?;
        if output.status.success() {
            let pubkey = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !pubkey.is_empty() {
                debug!(iface = %interface, "Using WireGuard pubkey from wg show");
                return Ok(pubkey);
            }
        }

        anyhow::bail!(
            "WG_PUBKEY unset and `wg show {} public-key` failed; cannot resolve local identity",
            interface
        );
    }

    /// True when `pubkey` is a peer on this interface with a recent handshake.
    pub fn is_recent_peer(&self, pubkey: &str, max_age_secs: u64) -> anyhow::Result<bool> {
        let interface = self
            .interface
            .as_ref()
            .filter(|v| !v.is_empty())
            .map(String::as_str)
            .ok_or_else(|| anyhow::anyhow!("WG interface not configured"))?;

        let output = Command::new("wg")
            .args(["show", interface, "latest-handshakes"])
            .output()?;
        if !output.status.success() {
            return Ok(false);
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        for line in stdout.lines() {
            let mut parts = line.split('\t');
            let Some(peer) = parts.next() else { continue };
            let Some(ts) = parts.next() else { continue };
            if peer != pubkey {
                continue;
            }
            let timestamp: u64 = ts.parse().unwrap_or(0);
            return Ok(timestamp > 0 && now.saturating_sub(timestamp) <= max_age_secs);
        }
        Ok(false)
    }

    /// Get peer's pubkey from their IP address
    pub fn get_pubkey_for_ip(&self, peer_ip: &str) -> anyhow::Result<Option<String>> {
        let interface = self
            .interface
            .as_ref()
            .filter(|v| !v.is_empty())
            .map(String::as_str)
            .ok_or_else(|| anyhow::anyhow!("WG interface not configured"))?;

        let output = Command::new("wg")
            .args(["show", interface, "allowed-ips"])
            .output()?;

        if !output.status.success() {
            return Ok(None);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Format: pubkey\tallowed_ip1, allowed_ip2
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                let pubkey = parts[0];
                let ips = parts[1];

                if ips.contains(peer_ip) {
                    return Ok(Some(pubkey.to_string()));
                }
            }
        }

        Ok(None)
    }

    /// Get all connected peers with their latest handshake times
    pub fn get_connected_peers(&self) -> anyhow::Result<Vec<PeerInfo>> {
        let interface = self
            .interface
            .as_ref()
            .filter(|v| !v.is_empty())
            .map(String::as_str)
            .ok_or_else(|| anyhow::anyhow!("WG interface not configured"))?;

        let output = Command::new("wg")
            .args(["show", interface, "latest-handshakes"])
            .output()?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut peers = Vec::new();

        // Format: pubkey\ttimestamp
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                let pubkey = parts[0].to_string();
                let timestamp: u64 = parts[1].parse().unwrap_or(0);

                // Only include peers with recent handshakes (within 3 minutes)
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                if timestamp > 0 && now - timestamp < 180 {
                    peers.push(PeerInfo {
                        pubkey,
                        last_handshake: timestamp,
                        allowed_ips: self.get_allowed_ips_for_peer(parts[0]).unwrap_or_default(),
                    });
                }
            }
        }

        Ok(peers)
    }

    /// Get the primary IPv4 address of the WireGuard interface.
    ///
    /// Uses `ip -4 addr show <iface>` and parses the first `inet` address.
    /// Returns `None` if the interface has no IPv4 address or the command fails.
    pub fn get_local_ip(&self) -> Option<String> {
        let interface = self.interface.as_deref()?;

        let output = Command::new("ip")
            .args(["-4", "addr", "show", interface])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("inet ") {
                // rest is e.g. "100.90.37.254/32 scope global netmaker"
                let ip = rest.split('/').next()?.trim();
                if !ip.is_empty() {
                    return Some(ip.to_string());
                }
            }
        }
        None
    }

    /// Get allowed IPs for a specific peer
    fn get_allowed_ips_for_peer(&self, pubkey: &str) -> anyhow::Result<Vec<String>> {
        let interface = self
            .interface
            .as_ref()
            .filter(|v| !v.is_empty())
            .map(String::as_str)
            .ok_or_else(|| anyhow::anyhow!("WG interface not configured"))?;

        let output = Command::new("wg")
            .args(["show", interface, "allowed-ips"])
            .output()?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);

        for line in stdout.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 && parts[0] == pubkey {
                return Ok(parts[1].split(',').map(|s| s.trim().to_string()).collect());
            }
        }

        Ok(Vec::new())
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
