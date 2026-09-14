//! D-Bus side of the Assistant gateway. Exposes a generic `call(method,
//! payload_json) -> response_json` surface so the gRPC side can dispatch
//! Assistant operations through D-Bus when available.
//!
//! Authentication is delegated to the bus policy files (`/usr/share/dbus-1/
//! system.d/ai.assistant.v1.conf`).

use crate::client::AssistantClient;
use crate::transport::{DEFAULT_DBUS_NAME, DEFAULT_DBUS_PATH};
use std::sync::Arc;
use zbus::object_server::SignalEmitter;

pub struct AssistantDbusService {
    client: Arc<AssistantClient>,
}

impl AssistantDbusService {
    pub fn new(client: Arc<AssistantClient>) -> Self {
        Self { client }
    }
}

#[zbus::interface(name = "ai.assistant.v1")]
impl AssistantDbusService {
    /// Generic JSON-RPC style passthrough. Returns the JSON-encoded response
    /// from the Assistant gateway.
    async fn call(&self, method: String, payload_json: String) -> zbus::fdo::Result<String> {
        let params: serde_json::Value = serde_json::from_str(&payload_json)
            .map_err(|e| zbus::fdo::Error::InvalidArgs(format!("invalid json: {}", e)))?;
        let result = self
            .client
            .call(&method, params)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        Ok(result.to_string())
    }

    /// Signal fired whenever an Assistant run emits an event. The gRPC side
    /// can subscribe to this signal to relay events to gRPC streaming clients.
    #[zbus(signal)]
    pub async fn run_event(
        emitter: &SignalEmitter<'_>,
        run_id: String,
        event_json: String,
    ) -> zbus::Result<()>;
}

/// Publish the D-Bus interface on the session bus. Returns the held connection
/// so callers can keep it alive.
pub async fn serve(client: Arc<AssistantClient>) -> zbus::Result<zbus::Connection> {
    let name = std::env::var("OP_ASSISTANT_DBUS_NAME").unwrap_or_else(|_| DEFAULT_DBUS_NAME.into());
    let path = std::env::var("OP_ASSISTANT_DBUS_PATH").unwrap_or_else(|_| DEFAULT_DBUS_PATH.into());

    let svc = AssistantDbusService::new(client);
    let conn = zbus::connection::Builder::session()?
        .name(name.as_str())?
        .serve_at(path.as_str(), svc)?
        .build()
        .await?;
    Ok(conn)
}
