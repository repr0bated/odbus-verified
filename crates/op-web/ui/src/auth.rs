//! ZeroClaw daemon authentication — simple blocking HTTP calls.
//!
//! Pair: POST /pair with X-Pairing-Code header → get bearer token.
//! Token persisted to ~/.config/zeroclaw-gui/token.json.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_GATEWAY: &str = "http://assistant.3tched.com:8090";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthState {
    pub gateway: String,
    pub token: Option<String>,
    pub device_name: String,
}

impl Default for AuthState {
    fn default() -> Self {
        Self {
            gateway: DEFAULT_GATEWAY.to_string(),
            token: None,
            device_name: "zeroclaw-gui".to_string(),
        }
    }
}

impl AuthState {
    fn token_path() -> PathBuf {
        dirs_next::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("zeroclaw-gui")
            .join("token.json")
    }

    pub fn load() -> Self {
        match std::fs::read_to_string(Self::token_path()) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let path = Self::token_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
    }

    pub fn bearer(&self) -> Option<String> {
        self.token.as_ref().map(|t| format!("Bearer {t}"))
    }

    /// Blocking: generate a new pairing code via admin endpoint.
    pub fn generate_code(&self) -> Result<String> {
        let url = format!("{}/admin/paircode/new", self.gateway);
        let resp = ehttp::fetch_blocking(&ehttp::Request::post(&url, Vec::new()))
            .map_err(|e| anyhow!("{e}"))?;
        if !resp.ok {
            return Err(anyhow!("HTTP {}: {}", resp.status, String::from_utf8_lossy(&resp.bytes)));
        }
        let body: serde_json::Value = serde_json::from_slice(&resp.bytes)?;
        body.get("pairing_code").and_then(|v| v.as_str()).map(String::from)
            .ok_or_else(|| anyhow!("no pairing_code in response"))
    }

    /// Blocking: exchange pairing code for bearer token.
    pub fn pair(&mut self, code: &str) -> Result<()> {
        let url = format!("{}/pair", self.gateway);
        let mut req = ehttp::Request::post(&url, serde_json::to_vec(&serde_json::json!({
            "device_name": self.device_name,
            "device_type": "desktop",
        })).unwrap());
        req.headers.insert("X-Pairing-Code".to_string(), code.to_string());
        req.headers.insert("Content-Type".to_string(), "application/json".to_string());
        let resp = ehttp::fetch_blocking(&req).map_err(|e| anyhow!("{e}"))?;
        if !resp.ok {
            return Err(anyhow!("HTTP {}: {}", resp.status, String::from_utf8_lossy(&resp.bytes)));
        }
        let body: serde_json::Value = serde_json::from_slice(&resp.bytes)?;
        self.token = body.get("token").and_then(|v| v.as_str()).map(String::from);
        self.save();
        Ok(())
    }
}
