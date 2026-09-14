//! D-Bus Tools - Native D-Bus Protocol Implementation
//!
//! These tools use zbus to communicate directly with D-Bus services.
//! They DO NOT use systemctl, nmcli, or any CLI commands.

use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tracing::info;
use zbus::Connection;

use crate::{Tool, ToolRegistry};

// ============================================================================
// SYSTEMD RESTART UNIT TOOL
// ============================================================================

pub struct DbusSystemdRestartTool;

#[async_trait]
impl Tool for DbusSystemdRestartTool {
    fn name(&self) -> &str {
        "dbus_systemd_restart_unit"
    }

    fn description(&self) -> &str {
        "Restart a systemd unit via D-Bus (not systemctl)"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "unit": {
                    "type": "string",
                    "description": "Service name (e.g., gbr-xray, op-web)"
                },
                "mode": {
                    "type": "string",
                    "description": "Job mode (replace, fail, isolate, etc.)",
                    "default": "replace"
                }
            },
            "required": ["unit"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let unit = input
            .get("unit")
            .and_then(|n| n.as_str())
            .map(|n| n.to_string())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: unit"))?;

        let mode = input.get("mode").and_then(|m| m.as_str()).unwrap_or("replace");

        info!("Restarting unit '{}' via D-Bus", unit);

        let job_path = restart_unit_dbus(&unit, mode).await?;
        Ok(json!({
            "restarted": true,
            "unit": unit,
            "job_path": job_path,
            "protocol": "D-Bus"
        }))
    }

    fn category(&self) -> &str {
        "systemd"
    }
}

async fn restart_unit_dbus(unit: &str, mode: &str) -> Result<String> {
    let connection = Connection::system().await?;

    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    ).await?;

    let job_path: zbus::zvariant::OwnedObjectPath = proxy
        .call("RestartUnit", &(unit, mode))
        .await?;

    Ok(job_path.to_string())
}

// ============================================================================
// SYSTEMD START UNIT TOOL
// ============================================================================

pub struct DbusSystemdStartTool;

#[async_trait]
impl Tool for DbusSystemdStartTool {
    fn name(&self) -> &str {
        "dbus_systemd_start_unit"
    }

    fn description(&self) -> &str {
        "Start a systemd unit via D-Bus (not systemctl)"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "unit": {
                    "type": "string",
                    "description": "Service name (e.g., gbr-xray, op-web)"
                },
                "mode": {
                    "type": "string",
                    "description": "Job mode (replace, fail, isolate, etc.)",
                    "default": "replace"
                }
            },
            "required": ["unit"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let unit = input
            .get("unit")
            .and_then(|n| n.as_str())
            .map(|n| n.to_string())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: unit"))?;

        let mode = input.get("mode").and_then(|m| m.as_str()).unwrap_or("replace");

        info!("Starting unit '{}' via D-Bus", unit);

        let job_path = start_unit_dbus(&unit, mode).await?;
        Ok(json!({
            "started": true,
            "unit": unit,
            "job_path": job_path,
            "protocol": "D-Bus"
        }))
    }

    fn category(&self) -> &str {
        "systemd"
    }
}

async fn start_unit_dbus(unit: &str, mode: &str) -> Result<String> {
    let connection = Connection::system().await?;

    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    ).await?;

    let job_path: zbus::zvariant::OwnedObjectPath = proxy
        .call("StartUnit", &(unit, mode))
        .await?;

    Ok(job_path.to_string())
}

// ============================================================================
// SYSTEMD STOP UNIT TOOL
// ============================================================================

pub struct DbusSystemdStopTool;

#[async_trait]
impl Tool for DbusSystemdStopTool {
    fn name(&self) -> &str {
        "dbus_systemd_stop_unit"
    }

    fn description(&self) -> &str {
        "Stop a systemd unit via D-Bus (not systemctl)"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "unit": {
                    "type": "string",
                    "description": "Service name (e.g., gbr-xray, op-web)"
                },
                "mode": {
                    "type": "string",
                    "description": "Job mode (replace, fail, isolate, etc.)",
                    "default": "replace"
                }
            },
            "required": ["unit"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let unit = input
            .get("unit")
            .and_then(|n| n.as_str())
            .map(|n| n.to_string())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: unit"))?;

        let mode = input.get("mode").and_then(|m| m.as_str()).unwrap_or("replace");

        info!("Stopping unit '{}' via D-Bus", unit);

        let job_path = stop_unit_dbus(&unit, mode).await?;
        Ok(json!({
            "stopped": true,
            "unit": unit,
            "job_path": job_path,
            "protocol": "D-Bus"
        }))
    }

    fn category(&self) -> &str {
        "systemd"
    }
}

async fn stop_unit_dbus(unit: &str, mode: &str) -> Result<String> {
    let connection = Connection::system().await?;

    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    ).await?;

    let job_path: zbus::zvariant::OwnedObjectPath = proxy
        .call("StopUnit", &(unit, mode))
        .await?;

    Ok(job_path.to_string())
}

// ============================================================================
// SYSTEMD GET UNIT STATUS TOOL
// ============================================================================

pub struct DbusSystemdStatusTool;

#[async_trait]
impl Tool for DbusSystemdStatusTool {
    fn name(&self) -> &str {
        "dbus_systemd_get_unit_status"
    }

    fn description(&self) -> &str {
        "Get systemd unit status via D-Bus (not systemctl)"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "unit": {
                    "type": "string",
                    "description": "Service name (e.g., gbr-xray, op-web)"
                }
            },
            "required": ["unit"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let unit = input
            .get("unit")
            .and_then(|n| n.as_str())
            .map(|n| n.to_string())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: unit"))?;

        info!("Getting status of unit '{}' via D-Bus", unit);

        get_unit_status_dbus(&unit).await
    }

    fn category(&self) -> &str {
        "systemd"
    }
}

async fn get_unit_status_dbus(unit: &str) -> Result<Value> {
    let connection = Connection::system().await?;

    // Get unit object path
    let manager_proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    ).await?;

    let unit_path: zbus::zvariant::OwnedObjectPath = manager_proxy
        .call("GetUnit", &(unit,))
        .await?;

    // Get unit properties
    let unit_proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        unit_path.as_str(),
        "org.freedesktop.systemd1.Unit",
    ).await?;

    let active_state: String = unit_proxy.get_property("ActiveState").await?;
    let sub_state: String = unit_proxy.get_property("SubState").await?;
    let load_state: String = unit_proxy.get_property("LoadState").await?;
    let description: String = unit_proxy.get_property("Description").await?;

    Ok(json!({
        "unit": unit,
        "active_state": active_state,
        "sub_state": sub_state,
        "load_state": load_state,
        "description": description,
        "protocol": "D-Bus"
    }))
}

// ============================================================================
// SYSTEMD LIST UNITS TOOL
// ============================================================================

pub struct DbusSystemdListUnitsTool;

#[async_trait]
impl Tool for DbusSystemdListUnitsTool {
    fn name(&self) -> &str {
        "dbus_systemd_list_units"
    }

    fn description(&self) -> &str {
        "List systemd units via D-Bus (not systemctl)"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "filter": {
                    "type": "string",
                    "description": "Filter pattern (e.g., '*.service')"
                },
                "active_only": {
                    "type": "boolean",
                    "description": "Only show active units",
                    "default": false
                }
            },
            "required": []
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let filter = input
            .get("filter")
            .and_then(|f| f.as_str())
            .map(|s| s.to_string());

        let active_only = input
            .get("active_only")
            .and_then(|a| a.as_bool())
            .unwrap_or(false);

        info!("Listing systemd units via D-Bus");

        let units = list_units_dbus(filter, active_only).await?;
        Ok(json!({
            "units": units,
            "count": units.len(),
            "protocol": "D-Bus"
        }))
    }

    fn category(&self) -> &str {
        "systemd"
    }
}

async fn list_units_dbus(filter: Option<String>, active_only: bool) -> Result<Vec<Value>> {
    let connection = Connection::system().await?;

    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    ).await?;

    // ListUnits returns array of (name, description, load_state, active_state, sub_state, following, unit_path, job_id, job_type, job_path)
    let units: Vec<(
        String, String, String, String, String, String,
        zbus::zvariant::OwnedObjectPath, u32, String, zbus::zvariant::OwnedObjectPath
    )> = proxy.call("ListUnits", &()).await?;

    let units: Vec<Value> = units
        .into_iter()
        .filter(|(name, _, _, active_state, _, _, _, _, _, _)| {
            let name_match = filter.as_ref().map(|f| {
                if f.contains('*') {
                    let pattern = f.replace('*', "");
                    name.contains(&pattern)
                } else {
                    name.contains(f)
                }
            }).unwrap_or(true);

            let active_match = if active_only {
                active_state == "active"
            } else {
                true
            };

            name_match && active_match
        })
        .map(|(name, description, load_state, active_state, sub_state, _, _, _, _, _)| {
            json!({
                "name": name,
                "description": description,
                "load_state": load_state,
                "active_state": active_state,
                "sub_state": sub_state
            })
        })
        .collect();

    Ok(units)
}

/// Register all D-Bus tools
pub async fn register_dbus_tools(registry: &ToolRegistry) -> Result<()> {
    registry.register_tool(Arc::new(DbusSystemdRestartTool)).await?;
    registry.register_tool(Arc::new(DbusSystemdStartTool)).await?;
    registry.register_tool(Arc::new(DbusSystemdStopTool)).await?;
    registry.register_tool(Arc::new(DbusSystemdStatusTool)).await?;
    registry.register_tool(Arc::new(DbusSystemdListUnitsTool)).await?;
    Ok(())
}
