//! Core security types and access control logic
//!
//! Provides IP-based access zones and security levels used across the system.

use serde::{Deserialize, Serialize};

/// Security level for resources/tools
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SecurityLevel {
    /// Safe, read-only operations - any IP
    #[default]
    Public,
    /// Normal operations - any IP
    Standard,
    /// System modifications - localhost or private network
    Elevated,
    /// Dangerous commands - localhost only
    Restricted,
}

impl SecurityLevel {
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "public" => Self::Public,
            "standard" => Self::Standard,
            "elevated" => Self::Elevated,
            "restricted" => Self::Restricted,
            _ => Self::Standard,
        }
    }
}

/// IP-based access control zones
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccessZone {
    /// 127.0.0.1, ::1 - full access to everything
    Localhost,
    /// Trusted VPN/mesh networks (Netmaker, Tailscale, etc.) - full access
    TrustedMesh,
    /// 192.168.x.x, 10.x.x.x, 172.16-31.x.x - elevated access
    PrivateNetwork,
    /// Public IPs - restricted to safe tools only
    #[default]
    Public,
}

impl AccessZone {
    /// Detect access zone from IP address string
    pub fn from_ip(ip: &str) -> Self {
        Self::from_ip_with_config(ip, &NetworkConfig::default())
    }

    /// Detect access zone with custom network configuration
    pub fn from_ip_with_config(ip: &str, config: &NetworkConfig) -> Self {
        let ip = ip.trim();

        // 1. Localhost - always full access
        if Self::is_localhost(ip) {
            return Self::Localhost;
        }

        // 2. Check custom trusted networks first (from config or env)
        if config.is_trusted(ip) {
            return Self::TrustedMesh;
        }

        // 3. Known VPN/mesh networks - auto-detect common ranges
        if Self::is_mesh_network(ip) {
            return Self::TrustedMesh;
        }

        // 4. Standard private networks (RFC 1918)
        if Self::is_private_network(ip) {
            return Self::PrivateNetwork;
        }

        Self::Public
    }

    fn is_localhost(ip: &str) -> bool {
        ip == "127.0.0.1" || ip == "::1" || ip == "localhost" || ip.starts_with("127.")
    }

    /// Check for known VPN/mesh network ranges
    fn is_mesh_network(ip: &str) -> bool {
        // Netmaker default ranges (commonly 10.x.x.x but checking specific patterns)
        // Netmaker often uses: 10.101.0.0/16, 10.102.0.0/16, etc.
        if ip.starts_with("10.101.") || ip.starts_with("10.102.") || ip.starts_with("10.103.") {
            return true;
        }

        // Tailscale CGNAT range: 100.64.0.0/10 (100.64.x.x - 100.127.x.x)
        if let Some(first) = ip.split('.').next() {
            if first == "100" {
                if let Some(second) = ip.split('.').nth(1) {
                    if let Ok(n) = second.parse::<u8>() {
                        if (64..=127).contains(&n) {
                            return true;
                        }
                    }
                }
            }
        }

        // ZeroTier default range: often 10.147.x.x, 10.244.x.x
        if ip.starts_with("10.147.") || ip.starts_with("10.244.") {
            return true;
        }

        // WireGuard common ranges: often 10.0.0.x, 10.200.x.x, 10.66.66.x
        if ip.starts_with("10.0.0.") || ip.starts_with("10.200.") || ip.starts_with("10.66.66.") {
            return true;
        }

        // Nebula default: often 10.42.x.x
        if ip.starts_with("10.42.") {
            return true;
        }

        // IPv6 ULA for mesh (fd00::/8)
        if ip.starts_with("fd") {
            return true;
        }

        false
    }

    fn is_private_network(ip: &str) -> bool {
        // RFC 1918 private ranges
        if ip.starts_with("192.168.") || ip.starts_with("10.") {
            return true;
        }

        // 172.16.0.0 - 172.31.255.255
        if let Some(rest) = ip.strip_prefix("172.") {
            if let Some(second_octet) = rest.split('.').next() {
                if let Ok(n) = second_octet.parse::<u8>() {
                    if (16..=31).contains(&n) {
                        return true;
                    }
                }
            }
        }

        // IPv6 link-local
        if ip.starts_with("fe80") {
            return true;
        }

        false
    }

    /// Check if this zone can access a security level
    pub fn can_access(&self, level: SecurityLevel) -> bool {
        match (self, level) {
            // Localhost can access everything
            (Self::Localhost, _) => true,

            // Trusted mesh (Netmaker, Tailscale, etc.) - full access like localhost
            (Self::TrustedMesh, _) => true,

            // Private network: public, standard, elevated (not restricted)
            (Self::PrivateNetwork, SecurityLevel::Public) => true,
            (Self::PrivateNetwork, SecurityLevel::Standard) => true,
            (Self::PrivateNetwork, SecurityLevel::Elevated) => true,
            (Self::PrivateNetwork, SecurityLevel::Restricted) => false,

            // Public: only public and standard
            (Self::Public, SecurityLevel::Public) => true,
            (Self::Public, SecurityLevel::Standard) => true,
            (Self::Public, _) => false,
        }
    }

    /// Get human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            Self::Localhost => "localhost (full access)",
            Self::TrustedMesh => "trusted mesh/VPN (full access)",
            Self::PrivateNetwork => "private network (elevated access)",
            Self::Public => "public network (limited access)",
        }
    }
}

/// Network configuration for trusted ranges
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Custom trusted CIDR ranges (e.g., "10.50.0.0/16")
    #[serde(default)]
    pub trusted_cidrs: Vec<String>,

    /// Custom trusted IP prefixes (e.g., "10.50.")
    #[serde(default)]
    pub trusted_prefixes: Vec<String>,

    /// Exact trusted IPs
    #[serde(default)]
    pub trusted_ips: Vec<String>,

    /// Auto-detect Netmaker networks
    #[serde(default = "default_true")]
    pub auto_netmaker: bool,

    /// Auto-detect Tailscale networks  
    #[serde(default = "default_true")]
    pub auto_tailscale: bool,

    /// Auto-detect ZeroTier networks
    #[serde(default = "default_true")]
    pub auto_zerotier: bool,

    /// Auto-detect WireGuard common ranges
    #[serde(default = "default_true")]
    pub auto_wireguard: bool,
}

fn default_true() -> bool {
    true
}

impl Default for NetworkConfig {
    fn default() -> Self {
        // Also check environment variable for additional trusted networks
        let env_trusted: Vec<String> = std::env::var("OP_TRUSTED_NETWORKS")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| s.trim().to_string())
            .collect();

        Self {
            trusted_cidrs: env_trusted,
            trusted_prefixes: vec![],
            trusted_ips: vec![],
            auto_netmaker: true,
            auto_tailscale: true,
            auto_zerotier: true,
            auto_wireguard: true,
        }
    }
}

impl NetworkConfig {
    /// Create new network config
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a trusted CIDR range (e.g., "10.50.0.0/16")
    pub fn trust_cidr(mut self, cidr: &str) -> Self {
        self.trusted_cidrs.push(cidr.to_string());
        self
    }

    /// Add a trusted prefix (e.g., "10.50." for 10.50.x.x)
    pub fn trust_prefix(mut self, prefix: &str) -> Self {
        self.trusted_prefixes.push(prefix.to_string());
        self
    }

    /// Add a trusted IP
    pub fn trust_ip(mut self, ip: &str) -> Self {
        self.trusted_ips.push(ip.to_string());
        self
    }

    /// Add your Netmaker network range
    pub fn trust_netmaker(mut self, cidr: &str) -> Self {
        self.trusted_cidrs.push(cidr.to_string());
        self
    }

    /// Check if an IP is in trusted networks
    pub fn is_trusted(&self, ip: &str) -> bool {
        // Check exact IPs
        if self.trusted_ips.contains(&ip.to_string()) {
            return true;
        }

        // Check prefixes
        for prefix in &self.trusted_prefixes {
            if ip.starts_with(prefix) {
                return true;
            }
        }

        // Check CIDRs (simplified - just checks prefix for now)
        for cidr in &self.trusted_cidrs {
            if let Some(network) = cidr.split('/').next() {
                // Simple prefix match based on CIDR
                let prefix = Self::cidr_to_prefix(network, cidr);
                if ip.starts_with(&prefix) {
                    return true;
                }
            }
        }

        false
    }

    /// Convert CIDR to prefix for simple matching
    fn cidr_to_prefix(network: &str, cidr: &str) -> String {
        let mask: u8 = cidr
            .split('/')
            .nth(1)
            .and_then(|m| m.parse().ok())
            .unwrap_or(24);

        let octets: Vec<&str> = network.split('.').collect();

        match mask {
            0..=8 => octets
                .first()
                .map(|s| format!("{}.", s))
                .unwrap_or_default(),
            9..=16 => {
                if octets.len() >= 2 {
                    format!("{}.{}.", octets[0], octets[1])
                } else {
                    network.to_string()
                }
            }
            17..=24 => {
                if octets.len() >= 3 {
                    format!("{}.{}.{}.", octets[0], octets[1], octets[2])
                } else {
                    network.to_string()
                }
            }
            _ => network.to_string(),
        }
    }
}

/// Quick helper to create trusted network config
pub fn trust_networks(prefixes: &[&str]) -> NetworkConfig {
    let mut config = NetworkConfig::new();
    for prefix in prefixes {
        config = config.trust_prefix(prefix);
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ip_detection() {
        // Localhost
        assert_eq!(AccessZone::from_ip("127.0.0.1"), AccessZone::Localhost);
        assert_eq!(AccessZone::from_ip("::1"), AccessZone::Localhost);
        assert_eq!(AccessZone::from_ip("localhost"), AccessZone::Localhost);

        // Tailscale (100.64.0.0/10)
        assert_eq!(AccessZone::from_ip("100.64.1.1"), AccessZone::TrustedMesh);
        assert_eq!(AccessZone::from_ip("100.100.50.1"), AccessZone::TrustedMesh);
        assert_eq!(
            AccessZone::from_ip("100.127.255.255"),
            AccessZone::TrustedMesh
        );

        // Netmaker common ranges
        assert_eq!(AccessZone::from_ip("10.101.0.5"), AccessZone::TrustedMesh);
        assert_eq!(AccessZone::from_ip("10.102.1.1"), AccessZone::TrustedMesh);

        // ZeroTier
        assert_eq!(AccessZone::from_ip("10.147.20.1"), AccessZone::TrustedMesh);

        // WireGuard common
        assert_eq!(AccessZone::from_ip("10.0.0.5"), AccessZone::TrustedMesh);
        assert_eq!(AccessZone::from_ip("10.66.66.1"), AccessZone::TrustedMesh);

        // Private networks (non-mesh)
        assert_eq!(
            AccessZone::from_ip("192.168.1.100"),
            AccessZone::PrivateNetwork
        );
        assert_eq!(AccessZone::from_ip("10.1.0.1"), AccessZone::PrivateNetwork); // Generic 10.x
        assert_eq!(
            AccessZone::from_ip("172.16.0.1"),
            AccessZone::PrivateNetwork
        );
        assert_eq!(
            AccessZone::from_ip("172.31.255.255"),
            AccessZone::PrivateNetwork
        );

        // Public
        assert_eq!(AccessZone::from_ip("8.8.8.8"), AccessZone::Public);
        assert_eq!(AccessZone::from_ip("172.15.0.1"), AccessZone::Public); // Not in 172.16-31
        assert_eq!(AccessZone::from_ip("172.32.0.1"), AccessZone::Public);
        assert_eq!(AccessZone::from_ip("100.63.0.1"), AccessZone::Public); // Just below Tailscale range
        assert_eq!(AccessZone::from_ip("100.128.0.1"), AccessZone::Public); // Just above Tailscale range
    }
}
