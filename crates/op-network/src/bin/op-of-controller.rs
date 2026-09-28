//! OpenFlow 1.5 mTLS controller for ovsbr0
//!
//! Listens on OF_CONTROLLER_LISTEN (default 10.200.0.1:6653) for OVS to
//! connect, then installs bidirectional flows between the configured port pairs.
//!
//! Environment variables:
//!   OF_CONTROLLER_LISTEN   listen address (default: 10.200.0.1:6653)
//!   OF_FLOW_PAIRS          comma-separated port pairs, e.g. "grpc-bridge:ovsbr0-sock"
//!                          defaults to "grpc-bridge:ovsbr0-sock"
//!   OF_FLOW_PRIORITY       flow priority (default: 100)
//!   OF_CONTROLLER_TLS_CA   PEM CA for authorized OVS client certificates
//!   OF_CONTROLLER_TLS_CERT PEM controller server certificate
//!   OF_CONTROLLER_TLS_KEY  PEM controller server private key
//!
//! Also exposes `org.opdbus.v1.plugins.openflow` at
//! `/org/opdbus/v1/plugins/openflow` on the system bus. Mutations are denied
//! until this bus can verify the bridge caller's principal and grant.

use std::net::SocketAddr;

use anyhow::{Context, Result};
use op_network::{OpenFlowController, OpenFlowControllerHandle, get_datapath_health};
use tracing::info;
use tracing_subscriber::EnvFilter;
use zbus::interface;

struct OpenFlowDbusService {
    handle: OpenFlowControllerHandle,
}

/// The bridge admits exact `openflow.write` grants, but these native methods
/// currently receive only a system-bus sender. A bus name, UID, PID, or
/// caller-supplied argument cannot recover the verified principal/session on
/// the bridge's separate identity-gated session bus. Deny before any OVS sink
/// until an authenticated bridge-to-owner attribution protocol exists.
fn deny_unattributed_mutation() -> zbus::fdo::Error {
    zbus::fdo::Error::AccessDenied(
        "OpenFlow mutation requires a verified principal and openflow.write grant".into(),
    )
}

#[interface(name = "org.opdbus.v1.plugins.openflow")]
impl OpenFlowDbusService {
    /// Install an absent entry or replace/remove an exact live entry. Empty
    /// expected_json means absent; native readback is required before success.
    async fn compare_exchange_flow(
        &self,
        _bridge: String,
        _flow_json: String,
        _expected_json: String,
        _delete: bool,
    ) -> zbus::fdo::Result<String> {
        Err(deny_unattributed_mutation())
    }

    async fn verify_flow(
        &self,
        bridge: String,
        flow_json: String,
        absent: bool,
    ) -> zbus::fdo::Result<bool> {
        self.handle
            .verify_flow(bridge, flow_json, absent)
            .await
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))
    }
    /// Install a schema-driven flow (JSON-encoded `FlowEntry`).
    async fn send_flow(&self, _flow_json: String) -> zbus::fdo::Result<String> {
        Err(deny_unattributed_mutation())
    }

    /// Delete a schema-driven flow (JSON-encoded `FlowEntry`).
    async fn delete_flow(&self, _flow_json: String) -> zbus::fdo::Result<String> {
        Err(deny_unattributed_mutation())
    }

    /// Dump flows this controller has pushed (in-memory tracking, not a live
    /// re-query of the switch's flow table).
    async fn dump_flows(&self) -> Vec<String> {
        self.handle.dump_flows()
    }

    /// Ensure priority=0 actions=NORMAL is present on `bridge`.
    async fn ensure_fallback_normal(&self, _bridge: String) -> zbus::fdo::Result<String> {
        Err(deny_unattributed_mutation())
    }

    /// Set Bridge fail_mode (`standalone` or `secure`).
    async fn set_fail_mode(&self, _bridge: String, _mode: String) -> zbus::fdo::Result<String> {
        Err(deny_unattributed_mutation())
    }

    /// Remove controllers from `bridge`.
    async fn del_controller(&self, _bridge: String) -> zbus::fdo::Result<String> {
        Err(deny_unattributed_mutation())
    }

    /// Set controller after ensuring NORMAL fallback. Endpoint e.g. ssl:10.200.0.1:6653.
    async fn set_controller(
        &self,
        _bridge: String,
        _endpoint: String,
    ) -> zbus::fdo::Result<String> {
        Err(deny_unattributed_mutation())
    }

    /// JSON datapath health (fail_mode, controllers, fallback_normal).
    async fn get_datapath_health(&self, bridge: String) -> zbus::fdo::Result<String> {
        let h = get_datapath_health(&bridge)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(format!("{e:#}")))?;
        serde_json::to_string(&h).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    /// Safe attach: standalone + NORMAL + set-controller + verify; rollback on failure.
    async fn attach_controller_safe(
        &self,
        _bridge: String,
        _endpoint: String,
    ) -> zbus::fdo::Result<String> {
        Err(deny_unattributed_mutation())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("op_network=info".parse()?))
        .init();

    let listen: SocketAddr = std::env::var("OF_CONTROLLER_LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:6653".to_string())
        .parse()
        .expect("OF_CONTROLLER_LISTEN must be a valid socket address");

    let pairs_env =
        std::env::var("OF_FLOW_PAIRS").unwrap_or_else(|_| "grpc-bridge:ovsbr0-sock".to_string());

    let priority: u16 = std::env::var("OF_FLOW_PRIORITY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100);

    let mut controller = OpenFlowController::new(listen);

    for pair in pairs_env.split(',') {
        let parts: Vec<&str> = pair.trim().splitn(2, ':').collect();
        if parts.len() != 2 {
            tracing::warn!("Ignoring malformed flow pair: {:?}", pair);
            continue;
        }
        info!(
            "Flow pair: {} ↔ {} (priority {})",
            parts[0], parts[1], priority
        );
        controller = controller.add_port_pair(parts[0], parts[1], priority);
    }

    // Durable static flows: rich FlowEntry JSON reinstalled on every OVS
    // reconnect (survives controller restarts). File is a JSON array.
    let static_flows_path = std::env::var("OF_STATIC_FLOWS_FILE")
        .unwrap_or_else(|_| "/etc/op-dbus/openflow-static-flows.json".to_string());
    match std::fs::read_to_string(&static_flows_path) {
        Ok(contents) => match serde_json::from_str::<Vec<serde_json::Value>>(&contents) {
            Ok(entries) => {
                for e in &entries {
                    controller = controller.add_static_flow(&e.to_string());
                }
                info!(
                    "Loaded {} static flow(s) from {}",
                    entries.len(),
                    static_flows_path
                );
            }
            Err(e) => tracing::warn!(
                "Failed to parse static flows {}: {:#}",
                static_flows_path,
                e
            ),
        },
        Err(_) => info!("No static flows file at {} (skipping)", static_flows_path),
    }

    let dbus_handle = controller.handle();
    let service = OpenFlowDbusService {
        handle: dbus_handle,
    };
    let _dbus_conn = zbus::connection::Builder::system()?
        .name("org.opdbus.v1.plugins.openflow")?
        .serve_at("/org/opdbus/v1/plugins/openflow", service)?
        .build()
        .await
        .context("registering org.opdbus.v1.plugins.openflow on the system bus")?;
    info!("org.opdbus.v1.plugins.openflow registered on the system bus");

    controller.run().await
}

#[cfg(test)]
mod mutation_admission_tests {
    use super::*;

    #[tokio::test]
    async fn unattributed_dbus_mutations_are_denied_before_sinks() {
        let controller = OpenFlowController::new("127.0.0.1:0".parse().unwrap());
        let service = OpenFlowDbusService {
            handle: controller.handle(),
        };
        let outcomes = [
            service
                .compare_exchange_flow("fixture0".into(), "{}".into(), String::new(), false)
                .await
                .map(|_| ()),
            service.send_flow("{}".into()).await.map(|_| ()),
            service.delete_flow("{}".into()).await.map(|_| ()),
            service
                .ensure_fallback_normal("fixture0".into())
                .await
                .map(|_| ()),
            service
                .set_fail_mode("fixture0".into(), "secure".into())
                .await
                .map(|_| ()),
            service.del_controller("fixture0".into()).await.map(|_| ()),
            service
                .set_controller("fixture0".into(), "tcp:127.0.0.1:6653".into())
                .await
                .map(|_| ()),
            service
                .attach_controller_safe("fixture0".into(), "tcp:127.0.0.1:6653".into())
                .await
                .map(|_| ()),
        ];
        for outcome in outcomes {
            assert!(matches!(outcome, Err(zbus::fdo::Error::AccessDenied(_))));
        }
        assert!(service.dump_flows().await.is_empty(), "flow owner changed");
    }
}
