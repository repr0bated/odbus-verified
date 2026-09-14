//! runit-systemctl D-Bus Daemon
//!
//! Provides a D-Bus interface that maps systemctl commands to **runit**
//! operations for Artix Linux. This host boots runit as PID 1 and is controlled
//! with `sv`; s6 is not installed.
//!
//! D-Bus object path: /org/opdbus/v1/plugins/runit/systemctl
//! Interface: org.opdbus.v1.Runit.Systemctl
//!
//! ## Mapping Reference
//!
//! | systemctl command | runit equivalent |
//! |-------------------|------------------|
//! | start <svc>       | `sv up <svc>` |
//! | stop <svc>        | `sv down <svc>` |
//! | restart <svc>     | `sv restart <svc>` (native, not stop + start) |
//! | reload <svc>      | `sv hup <svc>` |
//! | enable <svc>      | symlink into /etc/runit/runsvdir/default + remove `down` |
//! | disable <svc>     | `sv down` + remove the symlinks |
//! | status <svc>      | `sv status <svc>` |
//! | list-units        | list /run/runit/service |
//! | daemon-reload     | no-op — runit has no compiled service database |
//!
//! ## Architecture
//!
//! Per AGENTS.md D-Bus-first rules:
//! - All service operations go through D-Bus methods
//! - No direct subprocess spawning from clients
//! - Schema-driven interface with helpful error messages

use anyhow::{Context, Result};
use tracing::info;
use zbus::connection::Connection;

mod dbus;
use dbus::RunitSystemctlService;

const RUNIT_OBJECT_PATH: &str = "/org/opdbus/v1/plugins/runit/systemctl";
const RUNIT_BUS_NAME: &str = "org.opdbus.v1.Runit.Systemctl";

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("Starting op-runit-systemctl daemon");

    // Create the D-Bus service
    let service = RunitSystemctlService::new();

    // Build D-Bus connection
    let conn = Connection::system()
        .await
        .context("Failed to connect to system D-Bus")?;

    info!("Connected to system D-Bus");

    conn.object_server()
        .at(RUNIT_OBJECT_PATH, service)
        .await
        .context("Failed to register D-Bus object")?;

    conn.request_name(RUNIT_BUS_NAME)
        .await
        .context("Failed to request bus name")?;

    info!("D-Bus service registered at {RUNIT_OBJECT_PATH}");
    info!("Interface: {RUNIT_BUS_NAME}");
    info!("Daemon ready - press Ctrl+C to stop");

    // Confirm the runit supervisor is actually running. `sv` needs a live
    // `runsvdir` to talk to; without one every call would fail with a confusing
    // per-service error instead of one clear startup warning.
    match tokio::process::Command::new("pgrep")
        .arg("-x")
        .arg(op_core::runit::RUNSVDIR_PROC)
        .output()
        .await
    {
        Ok(out) if out.status.success() => info!("runsvdir detected - service operational"),
        _ => tracing::warn!(
            "runsvdir not detected. Service will return helpful error messages. \
             Ensure runit is supervising {} and `sv` is in PATH.",
            op_core::runit::SERVICE_DIR
        ),
    }

    // Keep the connection alive
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }
}
