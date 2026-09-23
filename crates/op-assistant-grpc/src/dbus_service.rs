//! D-Bus side of the Assistant gateway. Exposes a generic `call(method,
//! payload_json) -> response_json` surface so the gRPC side can dispatch
//! Assistant operations through D-Bus when available. The standalone server
//! no longer mounts this generic passthrough: a session-bus peer has no
//! verified principal or per-operation grant at this interface.

use crate::client::AssistantClient;
use crate::transport::{DEFAULT_DBUS_NAME, DEFAULT_DBUS_PATH};
use std::sync::Arc;
use zbus::object_server::SignalEmitter;

pub struct AssistantDbusService;

impl AssistantDbusService {
    pub fn new(_client: Arc<AssistantClient>) -> Self {
        Self
    }
}

#[zbus::interface(name = "ai.assistant.v1")]
impl AssistantDbusService {
    /// Generic JSON-RPC style passthrough. Returns the JSON-encoded response
    /// from the Assistant gateway.
    async fn call(&self, _method: String, _payload_json: String) -> zbus::fdo::Result<String> {
        Err(zbus::fdo::Error::AccessDenied(
            "standalone Assistant D-Bus passthrough is disabled".into(),
        ))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn generic_call_is_denied_before_payload_or_backend_dispatch() {
        let service = AssistantDbusService;
        let error = service
            .call("cron.create".into(), "{".into())
            .await
            .unwrap_err();
        assert!(matches!(error, zbus::fdo::Error::AccessDenied(_)));
    }
}
