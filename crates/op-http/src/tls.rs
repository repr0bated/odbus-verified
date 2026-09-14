//! TLS Configuration and Certificate Management
//!
//! Centralized TLS handling for all HTTP services.
//! Supports auto-detection of certificates from common locations.
//! Updated to include Cloudflare Origin certificate detection.

use crate::{Result, ServerError};
use rustls::ServerConfig as RustlsServerConfig;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use tokio_rustls::TlsAcceptor;
use tracing::{info, warn};

/// TLS mode configuration
#[derive(Clone, Debug, Default)]
pub enum TlsMode {
    /// No TLS, HTTP only
    #[default]
    Disabled,
    /// TLS enabled with explicit certificate paths
    Enabled { cert_path: String, key_path: String },
    /// Auto-detect certificates from common locations
    Auto,
}

/// TLS configuration
#[derive(Clone, Debug)]
pub struct TlsConfig {
    pub mode: TlsMode,
    pub cert_path: Option<String>,
    pub key_path: Option<String>,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            mode: TlsMode::Disabled,
            cert_path: None,
            key_path: None,
        }
    }
}

impl TlsConfig {
    /// Create a new TLS config with auto-detection
    pub fn auto() -> Self {
        Self {
            mode: TlsMode::Auto,
            cert_path: None,
            key_path: None,
        }
    }

    /// Create a new TLS config with explicit paths
    pub fn with_certs(cert_path: impl Into<String>, key_path: impl Into<String>) -> Self {
        let cert = cert_path.into();
        let key = key_path.into();
        Self {
            mode: TlsMode::Enabled {
                cert_path: cert.clone(),
                key_path: key.clone(),
            },
            cert_path: Some(cert),
            key_path: Some(key),
        }
    }

    /// Create a disabled TLS config (HTTP only)
    pub fn disabled() -> Self {
        Self::default()
    }

    /// Check if TLS is enabled
    pub fn is_enabled(&self) -> bool {
        !matches!(self.mode, TlsMode::Disabled)
    }

    /// Build a TLS acceptor from this config
    pub fn build_acceptor(&self) -> Result<Option<TlsAcceptor>> {
        match &self.mode {
            TlsMode::Disabled => Ok(None),
            TlsMode::Enabled {
                cert_path,
                key_path,
            } => {
                let acceptor = create_tls_acceptor(cert_path, key_path)?;
                Ok(Some(acceptor))
            }
            TlsMode::Auto => {
                if let Some((cert_path, key_path)) = detect_certificates()? {
                    info!("Auto-detected TLS certificates:");
                    info!("  cert: {}", cert_path);
                    info!("  key:  {}", key_path);
                    let acceptor = create_tls_acceptor(&cert_path, &key_path)?;
                    Ok(Some(acceptor))
                } else {
                    warn!("No TLS certificates found, falling back to HTTP");
                    Ok(None)
                }
            }
        }
    }
}

/// Create a TLS acceptor from certificate files
fn create_tls_acceptor(cert_path: &str, key_path: &str) -> Result<TlsAcceptor> {
    let cert_file = File::open(cert_path)
        .map_err(|e| ServerError::CertificateError(format!("Failed to open cert file: {}", e)))?;
    let key_file = File::open(key_path)
        .map_err(|e| ServerError::CertificateError(format!("Failed to open key file: {}", e)))?;

    let mut cert_reader = BufReader::new(cert_file);
    let mut key_reader = BufReader::new(key_file);

    let certs: Vec<_> = rustls_pemfile::certs(&mut cert_reader)
        .filter_map(|r| r.ok())
        .collect();

    let key = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|e| ServerError::CertificateError(format!("Failed to read private key: {}", e)))?
        .ok_or_else(|| ServerError::CertificateError("No private key found".to_string()))?;

    if certs.is_empty() {
        return Err(ServerError::CertificateError(
            "No certificates found".to_string(),
        ));
    }

    let tls_config = RustlsServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| ServerError::TlsError(format!("TLS config error: {}", e)))?;

    Ok(TlsAcceptor::from(Arc::new(tls_config)))
}

/// Auto-detect SSL certificates from common locations
pub fn detect_certificates() -> Result<Option<(String, String)>> {
    // Priority 1: Environment variables
    if let (Ok(cert), Ok(key)) = (
        std::env::var("SSL_CERT_PATH"),
        std::env::var("SSL_KEY_PATH"),
    ) {
        if Path::new(&cert).exists() && Path::new(&key).exists() {
            info!("Using certificates from environment variables");
            return Ok(Some((cert, key)));
        }
    }

    // Priority 2: Cloudflare Origin certificates (NEW)
    let cloudflare_paths = [
        // Standard Cloudflare Origin certificate locations
        (
            "/etc/ssl/cloudflare/origin.pem",
            "/etc/ssl/cloudflare/origin.key",
        ),
        (
            "/etc/ssl/cloudflare/cert.pem",
            "/etc/ssl/cloudflare/key.pem",
        ),
        ("/etc/cloudflare/origin.pem", "/etc/cloudflare/origin.key"),
        ("/etc/cloudflare/cert.pem", "/etc/cloudflare/key.pem"),
        // Domain-specific Cloudflare paths
        (
            "/etc/ssl/cloudflare/ghostbridge.tech/cert.pem",
            "/etc/ssl/cloudflare/ghostbridge.tech/key.pem",
        ),
        // User directory
        (
            "/home/jeremy/certs/cloudflare_origin.pem",
            "/home/jeremy/certs/cloudflare_origin.key",
        ),
    ];

    for (cert, key) in &cloudflare_paths {
        if Path::new(cert).exists() && Path::new(key).exists() {
            info!("Found Cloudflare Origin certificate");
            return Ok(Some((cert.to_string(), key.to_string())));
        }
    }

    // Priority 3: Nginx/custom certificates
    let nginx_certs = [
        (
            "/etc/nginx/ssl/ghostbridge.crt",
            "/etc/nginx/ssl/ghostbridge.key",
        ),
        ("/etc/nginx/ssl/proxmox.crt", "/etc/nginx/ssl/proxmox.key"),
        ("/etc/nginx/ssl/server.crt", "/etc/nginx/ssl/server.key"),
        (
            "/etc/nginx/ssl/cloudflare.crt",
            "/etc/nginx/ssl/cloudflare.key",
        ),
    ];

    for (cert, key) in &nginx_certs {
        if Path::new(cert).exists() && Path::new(key).exists() {
            info!("Found nginx SSL certificate");
            return Ok(Some((cert.to_string(), key.to_string())));
        }
    }

    // Priority 4: Let's Encrypt certificates
    let letsencrypt_domains = [
        "ghostbridge.tech",
        "proxmox.ghostbridge.tech",
        "op-web.ghostbridge.tech",
    ];

    let hostname = gethostname::gethostname().to_string_lossy().to_string();

    for domain in letsencrypt_domains
        .iter()
        .chain(std::iter::once(&hostname.as_str()))
    {
        let cert = format!("/etc/letsencrypt/live/{}/fullchain.pem", domain);
        let key = format!("/etc/letsencrypt/live/{}/privkey.pem", domain);
        if Path::new(&cert).exists() && Path::new(&key).exists() {
            info!("Found Let's Encrypt certificate for {}", domain);
            return Ok(Some((cert, key)));
        }
    }

    // Priority 5: Proxmox cluster certificates
    let pve_cert = format!("/etc/pve/nodes/{}/pve-ssl.pem", hostname);
    let pve_key = format!("/etc/pve/nodes/{}/pve-ssl.key", hostname);

    if Path::new(&pve_cert).exists() && Path::new(&pve_key).exists() {
        info!("Found Proxmox cluster certificate");
        return Ok(Some((pve_cert, pve_key)));
    }

    // Priority 6: System default certificates (self-signed)
    let system_certs = [
        (
            "/etc/ssl/certs/ssl-cert-snakeoil.pem",
            "/etc/ssl/private/ssl-cert-snakeoil.key",
        ),
        (
            "/etc/ssl/certs/localhost.pem",
            "/etc/ssl/private/localhost.key",
        ),
    ];

    for (cert, key) in &system_certs {
        if Path::new(cert).exists() && Path::new(key).exists() {
            warn!("Using system default (possibly self-signed) certificate");
            return Ok(Some((cert.to_string(), key.to_string())));
        }
    }

    Ok(None)
}

/// Validate that a certificate and key match
pub fn validate_cert_key_match(cert_path: &str, key_path: &str) -> Result<bool> {
    use std::process::Command;

    // Get certificate modulus
    let cert_output = Command::new("openssl")
        .args(["x509", "-in", cert_path, "-noout", "-modulus"])
        .output()
        .map_err(|e| ServerError::CertificateError(format!("Failed to run openssl: {}", e)))?;

    let key_output = Command::new("openssl")
        .args(["rsa", "-in", key_path, "-noout", "-modulus"])
        .output()
        .map_err(|e| ServerError::CertificateError(format!("Failed to run openssl: {}", e)))?;

    Ok(cert_output.stdout == key_output.stdout)
}

/// Get certificate expiry information
pub fn get_cert_expiry(cert_path: &str) -> Result<String> {
    use std::process::Command;

    let output = Command::new("openssl")
        .args(["x509", "-in", cert_path, "-noout", "-enddate"])
        .output()
        .map_err(|e| ServerError::CertificateError(format!("Failed to run openssl: {}", e)))?;

    let expiry = String::from_utf8_lossy(&output.stdout)
        .replace("notAfter=", "")
        .trim()
        .to_string();

    Ok(expiry)
}

/// Check if certificate is from Cloudflare
pub fn is_cloudflare_cert(cert_path: &str) -> Result<bool> {
    use std::process::Command;

    let output = Command::new("openssl")
        .args(["x509", "-in", cert_path, "-noout", "-issuer"])
        .output()
        .map_err(|e| ServerError::CertificateError(format!("Failed to run openssl: {}", e)))?;

    let issuer = String::from_utf8_lossy(&output.stdout).to_lowercase();
    Ok(issuer.contains("cloudflare"))
}
