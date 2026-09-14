//! Systemd D-Bus Tools

use crate::tool_registry::{Tool, ToolRegistry};
use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;

pub async fn register_all(registry: &ToolRegistry) -> Result<usize> {
    registry.register(Arc::new(SystemdUnitStatusTool)).await?;
    registry.register(Arc::new(SystemdListUnitsTool)).await?;
    registry.register(Arc::new(SystemdStartUnitTool)).await?;
    registry.register(Arc::new(SystemdStopUnitTool)).await?;
    registry.register(Arc::new(SystemdRestartUnitTool)).await?;
    registry.register(Arc::new(SystemdEnableUnitTool)).await?;
    registry.register(Arc::new(SystemdDisableUnitTool)).await?;
    registry.register(Arc::new(SystemdReloadDaemonTool)).await?;
    Ok(8)
}

async fn get_systemd_proxy() -> Result<zbus::Proxy<'static>> {
    let connection = zbus::Connection::system().await?;
    zbus::proxy::Builder::new(&connection)
        .destination("org.freedesktop.systemd1")?
        .path("/org/freedesktop/systemd1")?
        .interface("org.freedesktop.systemd1.Manager")?
        .build()
        .await
        .map_err(|e| anyhow::anyhow!("D-Bus error: {}", e))
}

pub struct SystemdUnitStatusTool;

#[async_trait]
impl Tool for SystemdUnitStatusTool {
    fn name(&self) -> &str { "systemd_unit_status" }
    fn description(&self) -> &str { "Get the status of a systemd unit." }
    fn category(&self) -> &str { "systemd" }
    fn tags(&self) -> Vec<String> { vec!["systemd".into(), "dbus".into(), "status".into()] }
    
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"unit": {"type": "string"}}, "required": ["unit"]})
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let unit = input.get("unit").and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing unit"))?;
        
        let connection = zbus::Connection::system().await?;
        let proxy = get_systemd_proxy().await?;
        
        let unit_path: zbus::zvariant::OwnedObjectPath = proxy.call("GetUnit", &(unit,)).await
            .map_err(|e| anyhow::anyhow!("Failed to get unit: {}", e))?;
        
        let unit_proxy = zbus::proxy::Builder::new(&connection)
            .destination("org.freedesktop.systemd1")?
            .path(unit_path.as_str())?
            .interface("org.freedesktop.systemd1.Unit")?
            .build().await?;
        
        let active: String = unit_proxy.get_property("ActiveState").await.unwrap_or_else(|_| "unknown".into());
        let sub: String = unit_proxy.get_property("SubState").await.unwrap_or_else(|_| "unknown".into());
        let load: String = unit_proxy.get_property("LoadState").await.unwrap_or_else(|_| "unknown".into());
        let desc: String = unit_proxy.get_property("Description").await.unwrap_or_else(|_| "No description".into());
        
        Ok(json!({"success": true, "unit": unit, "active_state": active, "sub_state": sub, "load_state": load, "description": desc}))
    }
}

pub struct SystemdListUnitsTool;

#[async_trait]
impl Tool for SystemdListUnitsTool {
    fn name(&self) -> &str { "systemd_list_units" }
    fn description(&self) -> &str { "List systemd units with optional filtering." }
    fn category(&self) -> &str { "systemd" }
    fn tags(&self) -> Vec<String> { vec!["systemd".into(), "dbus".into(), "list".into()] }
    
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {
            "unit_type": {"type": "string", "default": "service"},
            "state": {"type": "string", "default": "all"},
            "limit": {"type": "integer", "default": 50}
        }})
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let unit_type = input.get("unit_type").and_then(|v| v.as_str()).unwrap_or("service");
        let state_filter = input.get("state").and_then(|v| v.as_str()).unwrap_or("all");
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
        
        let proxy = get_systemd_proxy().await?;
        let units: Vec<(String, String, String, String, String, String, zbus::zvariant::OwnedObjectPath, u32, String, zbus::zvariant::OwnedObjectPath)> = 
            proxy.call("ListUnits", &()).await?;
        
        let filtered: Vec<Value> = units.into_iter()
            .filter(|(name, _, _, active, _, _, _, _, _, _)| {
                let type_ok = unit_type == "all" || name.ends_with(&format!(".{}", unit_type));
                let state_ok = state_filter == "all" || active == state_filter;
                type_ok && state_ok
            })
            .take(limit)
            .map(|(name, desc, load, active, sub, _, _, _, _, _)| json!({
                "name": name, "description": desc, "load_state": load, "active_state": active, "sub_state": sub
            }))
            .collect();
        
        Ok(json!({"success": true, "units": filtered, "count": filtered.len()}))
    }
}

macro_rules! systemd_action_tool {
    ($name:ident, $tool_name:expr, $desc:expr, $method:expr, $action:expr) => {
        pub struct $name;
        
        #[async_trait]
        impl Tool for $name {
            fn name(&self) -> &str { $tool_name }
            fn description(&self) -> &str { $desc }
            fn category(&self) -> &str { "systemd" }
            fn tags(&self) -> Vec<String> { vec!["systemd".into(), "dbus".into()] }
            
            fn input_schema(&self) -> Value {
                json!({"type": "object", "properties": {"unit": {"type": "string"}}, "required": ["unit"]})
            }

            async fn execute(&self, input: Value) -> Result<Value> {
                let unit = input.get("unit").and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing unit"))?;
                
                let proxy = get_systemd_proxy().await?;
                let job: zbus::zvariant::OwnedObjectPath = proxy.call($method, &(unit, "replace")).await
                    .map_err(|e| anyhow::anyhow!("Failed to {} unit: {}", $action, e))?;
                
                Ok(json!({"success": true, "unit": unit, "action": $action, "job_path": job.as_str()}))
            }
        }
    };
}

systemd_action_tool!(SystemdStartUnitTool, "systemd_start_unit", "Start a systemd unit.", "StartUnit", "started");
systemd_action_tool!(SystemdStopUnitTool, "systemd_stop_unit", "Stop a systemd unit.", "StopUnit", "stopped");
systemd_action_tool!(SystemdRestartUnitTool, "systemd_restart_unit", "Restart a systemd unit.", "RestartUnit", "restarted");

pub struct SystemdEnableUnitTool;

#[async_trait]
impl Tool for SystemdEnableUnitTool {
    fn name(&self) -> &str { "systemd_enable_unit" }
    fn description(&self) -> &str { "Enable a systemd unit." }
    fn category(&self) -> &str { "systemd" }
    fn tags(&self) -> Vec<String> { vec!["systemd".into()] }
    fn input_schema(&self) -> Value { json!({"type": "object", "properties": {"unit": {"type": "string"}}, "required": ["unit"]}) }

    async fn execute(&self, input: Value) -> Result<Value> {
        let unit = input.get("unit").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Missing unit"))?;
        let proxy = get_systemd_proxy().await?;
        let _: (bool, Vec<(String, String, String)>) = proxy.call("EnableUnitFiles", &(vec![unit], false, true)).await?;
        Ok(json!({"success": true, "unit": unit, "action": "enabled"}))
    }
}

pub struct SystemdDisableUnitTool;

#[async_trait]
impl Tool for SystemdDisableUnitTool {
    fn name(&self) -> &str { "systemd_disable_unit" }
    fn description(&self) -> &str { "Disable a systemd unit." }
    fn category(&self) -> &str { "systemd" }
    fn tags(&self) -> Vec<String> { vec!["systemd".into()] }
    fn input_schema(&self) -> Value { json!({"type": "object", "properties": {"unit": {"type": "string"}}, "required": ["unit"]}) }

    async fn execute(&self, input: Value) -> Result<Value> {
        let unit = input.get("unit").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Missing unit"))?;
        let proxy = get_systemd_proxy().await?;
        let _: Vec<(String, String, String)> = proxy.call("DisableUnitFiles", &(vec![unit], false)).await?;
        Ok(json!({"success": true, "unit": unit, "action": "disabled"}))
    }
}

pub struct SystemdReloadDaemonTool;

#[async_trait]
impl Tool for SystemdReloadDaemonTool {
    fn name(&self) -> &str { "systemd_reload_daemon" }
    fn description(&self) -> &str { "Reload systemd daemon configuration." }
    fn category(&self) -> &str { "systemd" }
    fn tags(&self) -> Vec<String> { vec!["systemd".into()] }
    fn input_schema(&self) -> Value { json!({"type": "object", "properties": {}}) }

    async fn execute(&self, _input: Value) -> Result<Value> {
        let proxy = get_systemd_proxy().await?;
        let _: () = proxy.call("Reload", &()).await?;
        Ok(json!({"success": true, "action": "daemon-reload"}))
    }
}
