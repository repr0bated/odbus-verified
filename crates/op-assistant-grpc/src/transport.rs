//! D-Bus first transport layer with HTTP/JSON-RPC fallback.
//!
//! `Transport::new` attempts to acquire a session D-Bus connection. When the
//! connection succeeds the primary kind is D-Bus; otherwise the fallback HTTP
//! transport is used. Both kinds remain initialised so callers can request a
//! specific transport explicitly.

use crate::error::{AssistantError, Result};
use crate::incus::{
    SchemaTags, DEFAULT_WG_XRAY_ENDPOINT, ENV_RPC_ENDPOINT, HEADER_FOOTPRINT, HEADER_GENESIS,
    HEADER_TRACE_ID,
};
use serde_json::Value;
use std::time::Duration;

pub const DEFAULT_DBUS_NAME: &str = "ai.assistant.v1";
pub const DEFAULT_DBUS_PATH: &str = "/ai/assistant";
/// Default RPC endpoint targets the on-host operation.v1 gRPC server served
/// by `op-dbus` at `10.200.0.2:50051` (the `grpc-uplink` veth IP). The
/// deprecated `10.200.0.1:50051` lived inside the wg-xray container and is
/// dead.
pub const DEFAULT_RPC_ENDPOINT: &str = DEFAULT_WG_XRAY_ENDPOINT;
pub const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone)]
pub struct TransportConfig {
    pub rpc_endpoint: String,
    pub dbus_name: String,
    pub dbus_path: String,
    pub http_timeout_secs: u64,
    pub force_kind: Option<TransportKind>,
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self {
            rpc_endpoint: std::env::var(ENV_RPC_ENDPOINT)
                .unwrap_or_else(|_| DEFAULT_RPC_ENDPOINT.to_string()),
            dbus_name: std::env::var("OP_ASSISTANT_DBUS_NAME")
                .unwrap_or_else(|_| DEFAULT_DBUS_NAME.to_string()),
            dbus_path: std::env::var("OP_ASSISTANT_DBUS_PATH")
                .unwrap_or_else(|_| DEFAULT_DBUS_PATH.to_string()),
            http_timeout_secs: std::env::var("OP_ASSISTANT_HTTP_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(DEFAULT_HTTP_TIMEOUT_SECS),
            force_kind: std::env::var("OP_ASSISTANT_TRANSPORT").ok().and_then(|v| {
                match v.to_lowercase().as_str() {
                    "dbus" => Some(TransportKind::DBus),
                    "rpc" | "http" => Some(TransportKind::Rpc),
                    _ => None,
                }
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    DBus,
    Rpc,
}

pub struct Transport {
    cfg: TransportConfig,
    primary: TransportKind,
    dbus: Option<zbus::Connection>,
    http: reqwest::Client,
}

impl Transport {
    pub async fn new(cfg: TransportConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(cfg.http_timeout_secs))
            .build()
            .map_err(AssistantError::Http)?;

        let dbus = match cfg.force_kind {
            Some(TransportKind::Rpc) => None,
            _ => zbus::Connection::session().await.ok(),
        };

        let primary = match (cfg.force_kind, dbus.is_some()) {
            (Some(TransportKind::DBus), false) => {
                return Err(AssistantError::Transport(
                    "OP_ASSISTANT_TRANSPORT=dbus but no session bus available".into(),
                ));
            }
            (Some(kind), _) => kind,
            (None, true) => TransportKind::DBus,
            (None, false) => TransportKind::Rpc,
        };

        Ok(Self {
            cfg,
            primary,
            dbus,
            http,
        })
    }

    pub fn primary_kind(&self) -> TransportKind {
        self.primary
    }

    pub fn config(&self) -> &TransportConfig {
        &self.cfg
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn dbus(&self) -> Option<&zbus::Connection> {
        self.dbus.as_ref()
    }

    /// Dispatch a high-level Assistant call. The transport will try the
    /// primary route and transparently fall back to HTTP if the D-Bus call
    /// fails.
    pub async fn call(&self, method: &str, body: Value) -> Result<Value> {
        match self.primary {
            TransportKind::DBus => match self.dbus_call(method, &body).await {
                Ok(v) => Ok(v),
                Err(err) => {
                    tracing::warn!(?err, %method, "D-Bus call failed, falling back to RPC");
                    self.rpc_call(method, body).await
                }
            },
            TransportKind::Rpc => self.rpc_call(method, body).await,
        }
    }

    async fn dbus_call(&self, method: &str, body: &Value) -> Result<Value> {
        let conn = self
            .dbus
            .as_ref()
            .ok_or_else(|| AssistantError::Transport("dbus not initialised".into()))?;

        let payload = serde_json::to_string(body)?;
        let reply = conn
            .call_method(
                Some(self.cfg.dbus_name.as_str()),
                self.cfg.dbus_path.as_str(),
                Some(self.cfg.dbus_name.as_str()),
                method,
                &(payload,),
            )
            .await?;

        let response: String = reply.body().deserialize()?;
        let value: Value = serde_json::from_str(&response)?;
        Ok(value)
    }

    async fn rpc_call(&self, method: &str, body: Value) -> Result<Value> {
        let url = format!(
            "{}/rpc/{}",
            self.cfg.rpc_endpoint.trim_end_matches('/'),
            method
        );
        let tags = SchemaTags::load();
        let mut req = self.http.post(&url).json(&body);
        if tags.is_valid() {
            req = req
                .header(HEADER_GENESIS, tags.footprint_hex.clone())
                .header(HEADER_FOOTPRINT, tags.footprint_hex)
                .header(HEADER_TRACE_ID, tags.trace_id);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(crate::error::map_http_status(status.as_u16(), text).into());
        }
        let value = resp.json::<Value>().await?;
        Ok(value)
    }
}

impl From<tonic::Status> for AssistantError {
    fn from(s: tonic::Status) -> Self {
        AssistantError::Internal(format!("grpc: {}", s.message()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_use_env_overrides() {
        // Just ensure default builder does not panic.
        let cfg = TransportConfig::default();
        assert!(!cfg.rpc_endpoint.is_empty());
        assert!(!cfg.dbus_name.is_empty());
        assert!(!cfg.dbus_path.is_empty());
    }

    #[tokio::test]
    async fn transport_initialises_with_rpc_fallback() {
        let cfg = TransportConfig {
            force_kind: Some(TransportKind::Rpc),
            ..Default::default()
        };
        let t = Transport::new(cfg).await.expect("init");
        assert_eq!(t.primary_kind(), TransportKind::Rpc);
    }
}
