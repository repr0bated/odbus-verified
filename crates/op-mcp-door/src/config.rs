//! Door configuration, resolved from the environment.
//!
//! Every value comes from one variable (one file per variable under
//! `/etc/op-dbus/env`, loaded by `chpst -e`). There is deliberately no
//! variable, flag or option that disables TLS (Requirement 1.5).

use anyhow::{Context, Result, bail};
use std::net::SocketAddr;
use std::path::PathBuf;

pub const DEFAULT_LISTEN: &str = "127.0.0.1:8091";
pub const DEFAULT_TLS_CERT: &str = "/var/lib/op-dbus/door-tls/door.crt";
pub const DEFAULT_TLS_KEY: &str = "/var/lib/op-dbus/door-tls/door.key";
pub const DEFAULT_CONFIG_DIR: &str = "/etc/opdbus/mcp-door";
pub const DEFAULT_STATE_DIR: &str = "/dev/shm/opdbus/mcp-door";
pub const DEFAULT_GRANTS_DOCUMENT: &str = "/dev/shm/opdbus/capability-grants.json";
pub const DEFAULT_TOOLSETS_PATH: &str = "/etc/opdbus/mcp-toolsets.json";
pub const DEFAULT_PUBLIC_RATE: u32 = 60;
pub const DEFAULT_MAX_BODY: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub listen: SocketAddr,
    pub tls_cert: PathBuf,
    pub tls_key: PathBuf,
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
    pub grants_document: PathBuf,
    pub toolsets_path: PathBuf,
    pub allowed_origins: Vec<String>,
    pub public_rate_per_minute: u32,
    pub max_body_bytes: usize,
    /// Collection override for the coding gate.
    pub coding_collections: Option<Vec<String>>,
    pub footprint_cozo_path: PathBuf,
    pub semantic_cozo_path: PathBuf,
}

impl Config {
    /// Resolve from the process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Resolve from any key lookup (tests pass a map).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let value = |key: &str| {
            get(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let collections = |key: &str| -> Result<Option<Vec<String>>> {
            let Some(raw) = value(key) else {
                return Ok(None);
            };
            let names: Vec<String> = raw
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            if names.is_empty() {
                return Ok(None);
            }
            if let Some(bad) = names
                .iter()
                .find(|n| !crate::readiness::valid_collection(n))
            {
                bail!("{key} has an invalid collection name: {bad}");
            }
            Ok(Some(names))
        };
        let listen = value("OP_MCP_DOOR_LISTEN").unwrap_or_else(|| DEFAULT_LISTEN.to_owned());
        let listen: SocketAddr = listen
            .parse()
            .with_context(|| format!("OP_MCP_DOOR_LISTEN is not a socket address: {listen}"))?;
        let tls_cert = value("OP_MCP_DOOR_TLS_CERT_FILE")
            .or_else(|| value("THREETCHED_TLS_CERT_FILE"))
            .unwrap_or_else(|| DEFAULT_TLS_CERT.to_owned());
        let tls_key = value("OP_MCP_DOOR_TLS_KEY_FILE")
            .or_else(|| value("THREETCHED_TLS_KEY_FILE"))
            .unwrap_or_else(|| DEFAULT_TLS_KEY.to_owned());
        let public_rate_per_minute = match value("OP_MCP_DOOR_PUBLIC_RATE") {
            Some(rate) => rate
                .parse()
                .with_context(|| format!("OP_MCP_DOOR_PUBLIC_RATE is not a number: {rate}"))?,
            None => DEFAULT_PUBLIC_RATE,
        };
        let max_body_bytes = match value("OP_MCP_DOOR_MAX_BODY") {
            Some(size) => size
                .parse()
                .with_context(|| format!("OP_MCP_DOOR_MAX_BODY is not a number: {size}"))?,
            None => DEFAULT_MAX_BODY,
        };
        if max_body_bytes == 0 {
            bail!("OP_MCP_DOOR_MAX_BODY must be positive");
        }
        let allowed_origins = value("OP_MCP_DOOR_ALLOWED_ORIGINS")
            .map(|list| {
                list.split(',')
                    .map(str::trim)
                    .filter(|o| !o.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            listen,
            tls_cert: tls_cert.into(),
            tls_key: tls_key.into(),
            config_dir: value("OP_MCP_DOOR_CONFIG_DIR")
                .unwrap_or_else(|| DEFAULT_CONFIG_DIR.to_owned())
                .into(),
            state_dir: value("OP_MCP_DOOR_STATE_DIR")
                .unwrap_or_else(|| DEFAULT_STATE_DIR.to_owned())
                .into(),
            grants_document: value("OP_MCP_DOOR_GRANTS_DOCUMENT")
                .unwrap_or_else(|| DEFAULT_GRANTS_DOCUMENT.to_owned())
                .into(),
            toolsets_path: value("OP_MCP_TOOLSETS_PATH")
                .unwrap_or_else(|| DEFAULT_TOOLSETS_PATH.to_owned())
                .into(),
            allowed_origins,
            public_rate_per_minute,
            max_body_bytes,
            coding_collections: collections("OP_MCP_CODING_COLLECTIONS")?,
            semantic_cozo_path: value("OPDBUS_SEMANTIC_COZO_PATH")
                .unwrap_or_else(|| "/var/lib/opdbus/semantic-index".into())
                .into(),
            footprint_cozo_path: value("OPDBUS_FOOTPRINT_COZO_PATH")
                .unwrap_or_else(|| "/var/lib/opdbus/snowball-footprint-index".into())
                .into(),
        })
    }

    pub fn mounts_file(&self) -> PathBuf {
        self.config_dir.join("mounts.json")
    }
    pub fn principals_file(&self) -> PathBuf {
        self.config_dir.join("principals.json")
    }
    pub fn oia1_keys_file(&self) -> PathBuf {
        self.config_dir.join("oia1-keys.json")
    }
    pub fn grants_dir(&self) -> PathBuf {
        self.state_dir.join("grants")
    }
    pub fn tickets_dir(&self) -> PathBuf {
        self.state_dir.join("tickets")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key| map.get(key).cloned()
    }

    #[test]
    fn defaults_use_a_separate_port_and_the_door_certificate() {
        let config = Config::from_lookup(lookup(&[])).unwrap();
        assert_eq!(config.listen.to_string(), DEFAULT_LISTEN);
        assert_ne!(config.listen.port(), 8090);
        assert_eq!(config.tls_cert, PathBuf::from(DEFAULT_TLS_CERT));
        assert_eq!(config.tls_key, PathBuf::from(DEFAULT_TLS_KEY));
        assert!(config.allowed_origins.is_empty());
    }

    #[test]
    fn door_tls_convention_is_the_fallback_and_explicit_paths_win() {
        let config = Config::from_lookup(lookup(&[
            ("THREETCHED_TLS_CERT_FILE", "/door/c.pem"),
            ("THREETCHED_TLS_KEY_FILE", "/door/k.pem"),
        ]))
        .unwrap();
        assert_eq!(config.tls_cert, PathBuf::from("/door/c.pem"));
        assert_eq!(config.tls_key, PathBuf::from("/door/k.pem"));
        let config = Config::from_lookup(lookup(&[
            ("THREETCHED_TLS_CERT_FILE", "/door/c.pem"),
            ("OP_MCP_DOOR_TLS_CERT_FILE", "/mcp/c.pem"),
        ]))
        .unwrap();
        assert_eq!(config.tls_cert, PathBuf::from("/mcp/c.pem"));
    }

    #[test]
    fn no_variable_disables_tls() {
        // Whatever plaintext-looking switches are set, the config still names
        // a certificate and key: there is no field that turns TLS off.
        let config = Config::from_lookup(lookup(&[
            ("OP_MCP_DOOR_INSECURE", "1"),
            ("OP_MCP_DOOR_PLAINTEXT", "1"),
            ("OP_MCP_DOOR_TLS", "off"),
        ]))
        .unwrap();
        assert_eq!(config.tls_cert, PathBuf::from(DEFAULT_TLS_CERT));
    }

    #[test]
    fn bad_values_are_rejected() {
        assert!(Config::from_lookup(lookup(&[("OP_MCP_DOOR_LISTEN", "nope")])).is_err());
        assert!(Config::from_lookup(lookup(&[("OP_MCP_DOOR_PUBLIC_RATE", "x")])).is_err());
        assert!(Config::from_lookup(lookup(&[("OP_MCP_DOOR_MAX_BODY", "0")])).is_err());
        let config = Config::from_lookup(lookup(&[(
            "OP_MCP_DOOR_ALLOWED_ORIGINS",
            "https://a, ,https://b",
        )]))
        .unwrap();
        assert_eq!(config.allowed_origins, vec!["https://a", "https://b"]);
        assert!(Config::from_lookup(lookup(&[("OP_MCP_CODING_COLLECTIONS", "ok,../x")])).is_err());
    }

    #[test]
    fn vector_readiness_defaults_and_overrides() {
        let config = Config::from_lookup(lookup(&[])).unwrap();
        assert!(config.coding_collections.is_none());
        let config = Config::from_lookup(lookup(&[
            ("OP_MCP_CODING_COLLECTIONS", "a, b ,"),
            ("OPDBUS_FOOTPRINT_COZO_PATH", "/test/cozo"),
        ]))
        .unwrap();
        assert_eq!(
            config.coding_collections,
            Some(vec!["a".to_owned(), "b".to_owned()])
        );
        assert_eq!(config.footprint_cozo_path, PathBuf::from("/test/cozo"));
    }
}
