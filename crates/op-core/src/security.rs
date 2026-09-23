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

        // Automatic VPN address ranges are never proof of peer identity, even
        // when legacy auto-detection flags are set in a stored configuration.
        // Only an explicit trusted range above may select TrustedMesh.
        // 3. Standard private networks (RFC 1918)
        if Self::is_private_network(ip) {
            return Self::PrivateNetwork;
        }

        Self::Public
    }

    fn is_localhost(ip: &str) -> bool {
        ip == "localhost"
            || ip
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback())
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

    /// Retained for configuration compatibility; never grants TrustedMesh.
    #[serde(default)]
    pub auto_tailscale: bool,

    /// Retained for configuration compatibility; never grants TrustedMesh.
    #[serde(default)]
    pub auto_zerotier: bool,

    /// Retained for configuration compatibility; never grants TrustedMesh.
    #[serde(default)]
    pub auto_wireguard: bool,
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
            auto_tailscale: false,
            auto_zerotier: false,
            auto_wireguard: false,
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

    /// Check if an IP is in trusted networks
    pub fn is_trusted(&self, ip: &str) -> bool {
        let Ok(address) = ip.parse::<std::net::IpAddr>() else {
            return false;
        };
        // Check exact IPs
        if self
            .trusted_ips
            .iter()
            .filter_map(|trusted| trusted.parse::<std::net::IpAddr>().ok())
            .any(|trusted| trusted == address)
        {
            return true;
        }

        // Check prefixes
        for prefix in &self.trusted_prefixes {
            if ip.starts_with(prefix) {
                return true;
            }
        }

        // Parse CIDRs exactly; string-prefix matching can admit addresses
        // outside the configured network.
        for cidr in &self.trusted_cidrs {
            if cidr
                .parse::<ipnet::IpNet>()
                .is_ok_and(|network| network.contains(&address))
            {
                return true;
            }
        }

        false
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

        // Address heuristics do not grant trusted-mesh authority by default.
        assert_eq!(AccessZone::from_ip("100.64.1.1"), AccessZone::Public);
        assert_eq!(
            AccessZone::from_ip("10.101.0.5"),
            AccessZone::PrivateNetwork
        );
        assert_eq!(
            AccessZone::from_ip("10.147.20.1"),
            AccessZone::PrivateNetwork
        );
        assert_eq!(AccessZone::from_ip("10.0.0.5"), AccessZone::PrivateNetwork);
        assert_eq!(
            AccessZone::from_ip("10.66.66.1"),
            AccessZone::PrivateNetwork
        );

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

    #[test]
    fn trusted_mesh_requires_explicit_configuration() {
        let configured = NetworkConfig::new().trust_cidr("100.64.0.0/10");
        assert_eq!(
            AccessZone::from_ip_with_config("100.100.50.1", &configured),
            AccessZone::TrustedMesh
        );
        assert!(!configured.is_trusted("100.128.0.1"));

        let legacy = NetworkConfig {
            auto_wireguard: true,
            auto_tailscale: true,
            auto_zerotier: true,
            ..NetworkConfig::default()
        };
        for ip in [
            "10.0.0.5",
            "10.200.0.17",
            "10.66.66.1",
            "10.101.0.5",
            "10.147.1.1",
        ] {
            let zone = AccessZone::from_ip_with_config(ip, &legacy);
            assert_eq!(zone, AccessZone::PrivateNetwork, "{ip}");
            assert!(!zone.can_access(SecurityLevel::Restricted), "{ip}");
        }
        for ip in ["100.100.50.1", "fd00::1"] {
            let zone = AccessZone::from_ip_with_config(ip, &legacy);
            assert_ne!(zone, AccessZone::TrustedMesh, "{ip}");
            assert!(!zone.can_access(SecurityLevel::Restricted), "{ip}");
        }
    }
}
