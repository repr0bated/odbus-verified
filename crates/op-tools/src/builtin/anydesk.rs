//! AnyDesk tools use the runit D-Bus owner for service state and control.
//! The AnyDesk ID has no admitted projection on this host.

use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use simd_json::prelude::*;
use simd_json::{OwnedValue as Value, json};
use std::sync::Arc;
use zbus::{Connection, proxy};

use crate::{Tool, ToolRegistry};

#[proxy(
    interface = "org.opdbus.v1.Runit.Systemctl",
    default_service = "org.opdbus.v1.Runit.Systemctl",
    default_path = "/org/opdbus/v1/plugins/runit/systemctl"
)]
trait RunitSystemctl {
    async fn start(&self, unit: &str) -> zbus::Result<(bool, String)>;
    async fn stop(&self, unit: &str) -> zbus::Result<(bool, String)>;
    async fn restart(&self, unit: &str) -> zbus::Result<(bool, String)>;
    async fn status(&self, unit: &str) -> zbus::Result<String>;
}

const ANYDESK_UNIT: &str = "anydesk";

async fn runit_proxy() -> Result<RunitSystemctlProxy<'static>> {
    let connection = Connection::system()
        .await
        .context("connect to system D-Bus")?;
    RunitSystemctlProxy::new(&connection)
        .await
        .context("connect to runit D-Bus owner")
}

pub async fn register_anydesk_tools(registry: &ToolRegistry) -> Result<()> {
    registry.register_tool(Arc::new(AnyDeskGetIdTool)).await?;
    registry
        .register_tool(Arc::new(AnyDeskGetStatusTool))
        .await?;
    registry
        .register_tool(Arc::new(AnyDeskServiceControlTool))
        .await?;
    Ok(())
}

struct AnyDeskGetIdTool;
struct AnyDeskGetStatusTool;
struct AnyDeskServiceControlTool;

#[async_trait]
impl Tool for AnyDeskGetIdTool {
    fn name(&self) -> &str {
        "anydesk_get_id"
    }
    fn description(&self) -> &str {
        "Get the AnyDesk ID when an admitted projection is available"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{}})
    }
    async fn execute(&self, _input: Value) -> Result<Value> {
        Err(anyhow!("AnyDesk ID has no admitted D-Bus projection"))
    }
    fn category(&self) -> &str {
        "remote"
    }
    fn namespace(&self) -> &str {
        "anydesk"
    }
}

#[async_trait]
impl Tool for AnyDeskGetStatusTool {
    fn name(&self) -> &str {
        "anydesk_get_status"
    }
    fn description(&self) -> &str {
        "Get AnyDesk supervision status from the runit D-Bus owner"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{}})
    }
    async fn execute(&self, _input: Value) -> Result<Value> {
        let status = runit_proxy()
            .await?
            .status(ANYDESK_UNIT)
            .await
            .context("RunitSystemctl.status")?;
        Ok(json!({"status":status,"manager":"runit","via":"dbus"}))
    }
    fn category(&self) -> &str {
        "remote"
    }
    fn namespace(&self) -> &str {
        "anydesk"
    }
}

#[async_trait]
impl Tool for AnyDeskServiceControlTool {
    fn name(&self) -> &str {
        "anydesk_service_control"
    }
    fn description(&self) -> &str {
        "Start, stop, or restart AnyDesk through the runit D-Bus owner"
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"action":{"type":"string","enum":["start","stop","restart"]}},"required":["action"]})
    }
    async fn execute(&self, input: Value) -> Result<Value> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required parameter: action"))?;
        let proxy = runit_proxy().await?;
        let (ok, message) = match action {
            "start" => proxy.start(ANYDESK_UNIT).await,
            "stop" => proxy.stop(ANYDESK_UNIT).await,
            "restart" => proxy.restart(ANYDESK_UNIT).await,
            _ => return Err(anyhow!("Invalid action: {action}")),
        }
        .context("RunitSystemctl service control")?;
        if !ok {
            return Err(anyhow!("AnyDesk {action} failed: {message}"));
        }
        Ok(json!({"success":true,"action":action,"result":message,"manager":"runit","via":"dbus"}))
    }
    fn category(&self) -> &str {
        "remote"
    }
    fn namespace(&self) -> &str {
        "anydesk"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_owner_backed_anydesk_tools_are_registered() {
        assert_eq!(AnyDeskGetStatusTool.name(), "anydesk_get_status");
        assert_eq!(AnyDeskServiceControlTool.name(), "anydesk_service_control");
        assert_eq!(ANYDESK_UNIT, "anydesk");
    }
}
