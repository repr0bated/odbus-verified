//! D-Bus first transport layer for the Assistant compatibility client.
//!
//! A failed D-Bus control call must not fall back to direct HTTP JSON-RPC,
//! where the legacy schema-tag headers are not an identity credential.

use crate::error::{AssistantError, Result};
use crate::incus::{DEFAULT_WG_XRAY_ENDPOINT, ENV_RPC_ENDPOINT};
use serde_json::Value;
use std::time::Duration;

pub const DEFAULT_DBUS_NAME: &str = "ai.assistant.v1";
pub const DEFAULT_DBUS_PATH: &str = "/ai/assistant";
/// Default RPC endpoint targets the on-host operation.v1 gRPC server served
/// by op-grpc-bridge on the `:8090` door. The retired `10.200.0.x:50051`
/// listeners are gone; `10.200.0.1:50051` lived inside the wg-xray container and is
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

    /// Dispatch a high-level Assistant call only through its configured D-Bus
    /// target. The retired direct RPC path cannot carry canonical admission.
    pub async fn call(&self, method: &str, body: Value) -> Result<Value> {
        match self.primary {
            TransportKind::DBus => self.dbus_call(method, &body).await,
            TransportKind::Rpc => Err(AssistantError::Transport(
                "direct Assistant HTTP RPC transport is disabled".into(),
            )),
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
    async fn direct_rpc_mode_fails_before_sending_control_call() {
        let cfg = TransportConfig {
            force_kind: Some(TransportKind::Rpc),
            rpc_endpoint: "http://127.0.0.1:9".into(),
            ..Default::default()
        };
        let t = Transport::new(cfg).await.expect("init");
        assert_eq!(t.primary_kind(), TransportKind::Rpc);
        let error = t
            .call("cron.create", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(error, AssistantError::Transport(_)));
    }
}
