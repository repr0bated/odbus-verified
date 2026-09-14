//! WireGuard Configuration Generation
//!
//! Generates WireGuard client configurations and QR codes for the privacy router.

use anyhow::Result;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use image::Luma;
use op_identity::{generate_wireguard_keypair, WireGuardKeyPair};
use qrcode::QrCode;

/// WireGuard keypair
pub type WgKeyPair = WireGuardKeyPair;

/// Generate a new WireGuard keypair
pub fn generate_keypair() -> WgKeyPair {
    generate_wireguard_keypair()
}

/// WireGuard server configuration for generating client configs
#[derive(Debug, Clone)]
pub struct WgServerConfig {
    pub public_key: String,
    pub endpoint: String,
    pub allowed_ips: String,
    pub dns: String,
}

impl Default for WgServerConfig {
    fn default() -> Self {
        Self::from_env_or_system()
    }
}

impl WgServerConfig {
    pub fn from_env_or_system() -> Self {
        let interface = std::env::var("WG_INTERFACE").unwrap_or_else(|_| "wg0".to_string());

        let public_key = std::env::var("WG_SERVER_PUBKEY")
            .or_else(|_| std::env::var("WG_SERVER_PUBLIC_KEY"))
            .or_else(|_| std::env::var("WIREGUARD_PUBLIC_KEY"))
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| detect_interface_public_key(&interface))
            .unwrap_or_default();

        let endpoint = std::env::var("WG_SERVER_ENDPOINT")
            .or_else(|_| std::env::var("VPN_ENDPOINT"))
            .unwrap_or_else(|_| "148.113.204.83:51820".to_string());

        let allowed_ips = std::env::var("WG_ALLOWED_IPS")
            .or_else(|_| std::env::var("VPN_ALLOWED_IPS"))
            .unwrap_or_else(|_| "0.0.0.0/0, ::/0".to_string());

        let dns = std::env::var("WG_DNS")
            .or_else(|_| std::env::var("VPN_DNS"))
            .unwrap_or_else(|_| "10.200.0.1".to_string());

        Self {
            public_key,
            endpoint,
            allowed_ips,
            dns,
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.public_key.trim().is_empty()
    }
}

fn detect_interface_public_key(interface: &str) -> Option<String> {
    let path = format!("/sys/class/net/{}/wireguard/public_key", interface);
    let key = std::fs::read_to_string(&path).ok()?.trim().to_string();
    if key.is_empty() {
        None
    } else {
        Some(key)
    }
}

/// Generate a WireGuard client configuration
pub fn generate_client_config(
    client_private_key: &str,
    client_address: &str,
    server: &WgServerConfig,
) -> String {
    format!(
        r#"[Interface]
PrivateKey = {}
Address = {}
DNS = {}

[Peer]
PublicKey = {}
AllowedIPs = {}
Endpoint = {}
PersistentKeepalive = 25
"#,
        client_private_key,
        client_address,
        server.dns,
        server.public_key,
        server.allowed_ips,
        server.endpoint
    )
}

/// Generate a QR code image as base64 PNG
pub fn generate_qr_code(config: &str) -> Result<String> {
    let code = QrCode::new(config.as_bytes())?;
    let image = code.render::<Luma<u8>>().build();

    // Encode as PNG using the image crate's write interface
    let mut png_bytes = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut png_bytes);
    image.write_to(&mut cursor, image::ImageFormat::Png)?;

    // Return as data URL
    Ok(format!(
        "data:image/png;base64,{}",
        BASE64.encode(&png_bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let keypair = generate_keypair();
        assert!(!keypair.private_key.is_empty());
        assert!(!keypair.public_key.is_empty());
        // Base64 encoded 32-byte key = 44 chars
        assert_eq!(keypair.private_key.len(), 44);
        assert_eq!(keypair.public_key.len(), 44);
    }

    #[test]
    fn test_config_generation() {
        let config = generate_client_config(
            "test_private_key",
            "10.100.0.2/32",
            &WgServerConfig {
                public_key: "server_pub_key".to_string(),
                endpoint: "vpn.example.com:51820".to_string(),
                ..Default::default()
            },
        );
        assert!(config.contains("PrivateKey = test_private_key"));
        assert!(config.contains("Address = 10.100.0.2/32"));
        assert!(config.contains("PublicKey = server_pub_key"));
    }
}
