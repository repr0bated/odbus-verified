//! Runit service-management tools, exposed to agents over the tool registry.
//!
//! This host boots runit as PID 1 and is controlled with `sv`; s6 is not
//! installed. Each tool prefers the `op-runit-systemctl` D-Bus service so the
//! change lands on the audited control plane, and falls back to invoking `sv`
//! directly when that daemon is not reachable.
//!
//! The five tools mirror the verbs runit actually has. `sv restart` is native,
//! so restart is one operation rather than a stop/start pair.
//!
//! OSCAL subid: `mut.service.runit.service-control@v1`

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use zbus::{proxy, Connection};

use crate::{Tool, ToolRegistry};

/// The daemon's real bus name and object path (see `op-s6-systemctl/src/main.rs`).
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
    async fn list_units(&self) -> zbus::Result<String>;
}

async fn get_proxy() -> Result<RunitSystemctlProxy<'static>> {
    let conn = Connection::system()
        .await
        .context("connect to system D-Bus for RunitSystemctl")?;
    RunitSystemctlProxy::new(&conn)
        .await
        .context("create RunitSystemctl D-Bus proxy")
}

/// Fallback: invoke `sv` directly. Requires root, same as the daemon path.
async fn sv(args: &[&str]) -> Result<std::process::Output> {
    tokio::process::Command::new(op_core::runit::SV_BIN)
        .args(args)
        .output()
        .await
        .with_context(|| format!("failed to run sv {}", args.join(" ")))
}

/// One-line schema shared by every tool that takes a service name.
fn service_schema(verb: &str) -> Value {
    json!({
        "type": "object",
        "properties": {
            "service": {
                "type": "string",
                "description": format!("runit service name to {verb} (as under /etc/runit/sv)")
            }
        },
        "required": ["service"]
    })
}

fn service_arg(input: &Value) -> Result<String> {
    input
        .get("service")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Missing required parameter: service"))
}

/// The three mutating verbs share one shape: try D-Bus, else `sv <verb>`.
async fn control(verb: &str, service: &str) -> Result<Value> {
    let past_tense = match verb {
        "start" => "started",
        "stop" => "stopped",
        _ => "restarted",
    };

    if let Ok(proxy) = get_proxy().await {
        let call = match verb {
            "start" => proxy.start(service).await,
            "stop" => proxy.stop(service).await,
            _ => proxy.restart(service).await,
        };
        if let Ok((ok, message)) = call {
            if ok {
                return Ok(json!({
                    past_tense: true,
                    "service": service,
                    "manager": "runit",
                    "via": "dbus"
                }));
            }
            return Err(anyhow!("sv {verb} {service} failed: {message}"));
        }
    }

    let out = sv(&[verb, service]).await?;
    if out.status.success() {
        return Ok(json!({
            past_tense: true,
            "service": service,
            "manager": "runit",
            "via": "sv"
        }));
    }
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(anyhow!("sv {verb} {service} failed: {stderr}"))
}

pub struct SvStartServiceTool;
pub struct SvStopServiceTool;
pub struct SvRestartServiceTool;
pub struct SvStatusTool;
pub struct SvListServicesTool;

#[async_trait]
impl Tool for SvStartServiceTool {
    fn name(&self) -> &str {
        "sv_start_service"
    }
    fn description(&self) -> &str {
        "Start a runit service (sv up). Use for host services under /etc/runit/sv."
    }
    fn input_schema(&self) -> Value {
        service_schema("start")
    }
    async fn execute(&self, input: Value) -> Result<Value> {
        control("start", &service_arg(&input)?).await
    }
    fn category(&self) -> &str {
        "runit"
    }
}

#[async_trait]
impl Tool for SvStopServiceTool {
    fn name(&self) -> &str {
        "sv_stop_service"
    }
    fn description(&self) -> &str {
        "Stop a runit service (sv down)."
    }
    fn input_schema(&self) -> Value {
        service_schema("stop")
    }
    async fn execute(&self, input: Value) -> Result<Value> {
        control("stop", &service_arg(&input)?).await
    }
    fn category(&self) -> &str {
        "runit"
    }
}

#[async_trait]
impl Tool for SvRestartServiceTool {
    fn name(&self) -> &str {
        "sv_restart_service"
    }
    fn description(&self) -> &str {
        "Restart a runit service (sv restart). Native single operation — do not \
         chain stop then start."
    }
    fn input_schema(&self) -> Value {
        service_schema("restart")
    }
    async fn execute(&self, input: Value) -> Result<Value> {
        control("restart", &service_arg(&input)?).await
    }
    fn category(&self) -> &str {
        "runit"
    }
}

#[async_trait]
impl Tool for SvStatusTool {
    fn name(&self) -> &str {
        "sv_service_status"
    }
    fn description(&self) -> &str {
        "Report a runit service's supervision status (sv status)."
    }
    fn input_schema(&self) -> Value {
        service_schema("query")
    }
    async fn execute(&self, input: Value) -> Result<Value> {
        let service = service_arg(&input)?;

        if let Ok(proxy) = get_proxy().await {
            if let Ok(status) = proxy.status(&service).await {
                return Ok(json!({
                    "service": service,
                    "status": status,
                    "manager": "runit",
                    "via": "dbus"
                }));
            }
        }

        let out = sv(&["status", &service]).await?;
        let status = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() {
            // `sv status` prints e.g. "run: op-web: (pid 1234) 56s".
            let running = status.starts_with("run:");
            return Ok(json!({
                "service": service,
                "status": status,
                "running": running,
                "manager": "runit",
                "via": "sv"
            }));
        }
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(anyhow!("sv status {service} failed: {stderr}"))
    }
    fn category(&self) -> &str {
        "runit"
    }
}

#[async_trait]
impl Tool for SvListServicesTool {
    fn name(&self) -> &str {
        "sv_list_services"
    }
    fn description(&self) -> &str {
        "List runit services and whether each is enabled for boot."
    }
    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn execute(&self, _input: Value) -> Result<Value> {
        if let Ok(proxy) = get_proxy().await {
            if let Ok(units) = proxy.list_units().await {
                return Ok(json!({ "services": units, "manager": "runit", "via": "dbus" }));
            }
        }

        // Definitions are directories; enablement is a symlink in the runlevel.
        let mut services: Vec<Value> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(op_core::runit::SV_DIR) {
            for entry in entries.flatten() {
                if !entry.path().is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                let enabled = std::path::Path::new(&op_core::runit::enabled_path(&name)).exists();
                let supervised = std::path::Path::new(&op_core::runit::live_path(&name)).exists();
                services.push(json!({
                    "service": name,
                    "enabled": enabled,
                    "supervised": supervised
                }));
            }
        }
        Ok(json!({
            "services": services,
            "manager": "runit",
            "via": "filesystem",
            "definitions": op_core::runit::SV_DIR
        }))
    }
    fn category(&self) -> &str {
        "runit"
    }
}

pub async fn register_sv_tools(registry: &ToolRegistry) -> Result<()> {
    registry.register_tool(Arc::new(SvStartServiceTool)).await?;
    registry.register_tool(Arc::new(SvStopServiceTool)).await?;
    registry
        .register_tool(Arc::new(SvRestartServiceTool))
        .await?;
    registry.register_tool(Arc::new(SvStatusTool)).await?;
    registry.register_tool(Arc::new(SvListServicesTool)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_are_sv_not_s6() {
        let names = [
            SvStartServiceTool.name(),
            SvStopServiceTool.name(),
            SvRestartServiceTool.name(),
            SvStatusTool.name(),
            SvListServicesTool.name(),
        ];
        for name in names {
            assert!(name.starts_with("sv_"), "{name} is not an sv_* tool");
            assert!(!name.contains("s6"), "{name} still references s6");
        }
        // Restart is a distinct tool, not a documented stop+start dance.
        assert_eq!(SvRestartServiceTool.name(), "sv_restart_service");
    }
}
