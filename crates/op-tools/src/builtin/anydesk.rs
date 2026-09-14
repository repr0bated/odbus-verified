//! AnyDesk remote desktop tools.
//!
//! These tools provide management and monitoring capabilities for AnyDesk,
//! including getting the AnyDesk ID, checking service status, and controlling
//! the AnyDesk service.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::fs;
use std::path::Path;
use std::process::Command;
use zbus::Connection;

use crate::Tool;

/// Register AnyDesk tools with the tool registry
pub async fn register_anydesk_tools(registry: &crate::ToolRegistry) -> Result<()> {
    registry
        .register_tool(std::sync::Arc::new(AnyDeskGetIdTool))
        .await?;
    registry
        .register_tool(std::sync::Arc::new(AnyDeskGetStatusTool))
        .await?;
    registry
        .register_tool(std::sync::Arc::new(AnyDeskServiceControlTool))
        .await?;
    registry
        .register_tool(std::sync::Arc::new(AnyDeskGetConnectionsTool))
        .await?;
    registry
        .register_tool(std::sync::Arc::new(AnyDeskCheckX11DisplayTool))
        .await?;
    registry
        .register_tool(std::sync::Arc::new(AnyDeskDiagnoseX11AccessTool))
        .await?;

    Ok(())
}

/// Tool to get the AnyDesk ID
struct AnyDeskGetIdTool;

#[async_trait]
impl Tool for AnyDeskGetIdTool {
    fn name(&self) -> &str {
        "anydesk_get_id"
    }

    fn description(&self) -> &str {
        "Get the AnyDesk ID for remote connections"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        // Try to get AnyDesk ID from various sources
        // First check if AnyDesk is running and can provide the ID
        match get_anydesk_id().await {
            Ok(id) => Ok(json!({
                "success": true,
                "anydesk_id": id
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": format!("Could not retrieve AnyDesk ID: {}", e)
            })),
        }
    }

    fn category(&self) -> &str {
        "remote"
    }

    fn namespace(&self) -> &str {
        "anydesk"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "remote".to_string(),
            "desktop".to_string(),
            "anydesk".to_string(),
        ]
    }
}

/// Tool to get AnyDesk service status
struct AnyDeskGetStatusTool;

#[async_trait]
impl Tool for AnyDeskGetStatusTool {
    fn name(&self) -> &str {
        "anydesk_get_status"
    }

    fn description(&self) -> &str {
        "Get the current status of the AnyDesk service"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        match get_anydesk_status().await {
            Ok(status) => Ok(json!({
                "success": true,
                "status": status
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": format!("Could not get AnyDesk status: {}", e)
            })),
        }
    }

    fn category(&self) -> &str {
        "remote"
    }

    fn namespace(&self) -> &str {
        "anydesk"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "remote".to_string(),
            "desktop".to_string(),
            "anydesk".to_string(),
            "status".to_string(),
        ]
    }
}

/// Tool to control AnyDesk service (start/stop/restart)
struct AnyDeskServiceControlTool;

#[async_trait]
impl Tool for AnyDeskServiceControlTool {
    fn name(&self) -> &str {
        "anydesk_service_control"
    }

    fn description(&self) -> &str {
        "Control the AnyDesk service (start, stop, restart)"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["start", "stop", "restart"],
                    "description": "Action to perform on the AnyDesk service"
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing required parameter: action"))?;

        match control_anydesk_service(action).await {
            Ok(result) => Ok(json!({
                "success": true,
                "action": action,
                "result": result
            })),
            Err(e) => Ok(json!({
                "success": false,
                "action": action,
                "error": format!("Failed to {} AnyDesk service: {}", action, e)
            })),
        }
    }

    fn category(&self) -> &str {
        "remote"
    }

    fn namespace(&self) -> &str {
        "anydesk"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "remote".to_string(),
            "desktop".to_string(),
            "anydesk".to_string(),
            "control".to_string(),
        ]
    }
}

/// Tool to get current AnyDesk connections
struct AnyDeskGetConnectionsTool;

/// Tool to check X11 display environment for AnyDesk
struct AnyDeskCheckX11DisplayTool;

/// Tool to diagnose X11 access issues for AnyDesk
struct AnyDeskDiagnoseX11AccessTool;

#[async_trait]
impl Tool for AnyDeskGetConnectionsTool {
    fn name(&self) -> &str {
        "anydesk_get_connections"
    }

    fn description(&self) -> &str {
        "Get information about current AnyDesk remote connections"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        match get_anydesk_connections() {
            Ok(connections) => Ok(json!({
                "success": true,
                "connections": connections
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": format!("Could not get AnyDesk connections: {}", e)
            })),
        }
    }

    fn category(&self) -> &str {
        "remote"
    }

    fn namespace(&self) -> &str {
        "anydesk"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "remote".to_string(),
            "desktop".to_string(),
            "anydesk".to_string(),
            "connections".to_string(),
        ]
    }
}

#[async_trait]
impl Tool for AnyDeskCheckX11DisplayTool {
    fn name(&self) -> &str {
        "anydesk_check_x11_display"
    }

    fn description(&self) -> &str {
        "Check X11 display environment and configuration for AnyDesk screen sharing"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        match check_x11_display_environment().await {
            Ok(result) => Ok(json!({
                "success": true,
                "x11_environment": result
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": format!("Failed to check X11 display environment: {}", e)
            })),
        }
    }

    fn category(&self) -> &str {
        "remote"
    }

    fn namespace(&self) -> &str {
        "anydesk"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "remote".to_string(),
            "desktop".to_string(),
            "anydesk".to_string(),
            "x11".to_string(),
            "display".to_string(),
        ]
    }
}

#[async_trait]
impl Tool for AnyDeskDiagnoseX11AccessTool {
    fn name(&self) -> &str {
        "anydesk_diagnose_x11_access"
    }

    fn description(&self) -> &str {
        "Diagnose X11 access issues and provide fixes for AnyDesk screen sharing"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        match diagnose_x11_access_issues().await {
            Ok(result) => Ok(json!({
                "success": true,
                "diagnosis": result
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": format!("Failed to diagnose X11 access: {}", e)
            })),
        }
    }

    fn category(&self) -> &str {
        "remote"
    }

    fn namespace(&self) -> &str {
        "anydesk"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "remote".to_string(),
            "desktop".to_string(),
            "anydesk".to_string(),
            "x11".to_string(),
            "diagnostics".to_string(),
        ]
    }
}

// ============================================================================
// SYSTEMD D-BUS HELPERS (no systemctl bypasses)
// ============================================================================

async fn get_systemd_unit_pid_dbus(unit: &str) -> Result<u32> {
    let connection = Connection::system().await?;
    let manager_proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    )
    .await?;

    let unit_path: zbus::zvariant::OwnedObjectPath = manager_proxy
        .call("GetUnit", &(unit,))
        .await
        .map_err(|_| anyhow!("Unit {} not found", unit))?;

    let unit_proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        unit_path.as_str(),
        "org.freedesktop.systemd1.Unit",
    )
    .await?;

    let main_pid: u32 = unit_proxy.get_property("MainPID").await?;
    Ok(main_pid)
}

async fn get_systemd_unit_active_state_dbus(unit: &str) -> Result<String> {
    let connection = Connection::system().await?;
    let manager_proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    )
    .await?;

    let unit_path: zbus::zvariant::OwnedObjectPath = manager_proxy
        .call("GetUnit", &(unit,))
        .await
        .map_err(|_| anyhow!("Unit {} not found", unit))?;

    let unit_proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        unit_path.as_str(),
        "org.freedesktop.systemd1.Unit",
    )
    .await?;

    let active_state: String = unit_proxy.get_property("ActiveState").await?;
    Ok(active_state)
}

async fn control_systemd_unit_dbus(unit: &str, action: &str) -> Result<String> {
    let connection = Connection::system().await?;
    let manager_proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    )
    .await?;

    let method = match action {
        "start" => "StartUnit",
        "stop" => "StopUnit",
        "restart" => "RestartUnit",
        _ => return Err(anyhow!("Invalid action: {}", action)),
    };

    let job_path: zbus::zvariant::OwnedObjectPath =
        manager_proxy.call(method, &(unit, "replace")).await?;

    Ok(format!(
        "AnyDesk service {} via D-Bus (job: {})",
        action, job_path
    ))
}

async fn get_systemd_unit_environment_dbus(unit: &str) -> Result<Vec<String>> {
    let connection = Connection::system().await?;
    let manager_proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    )
    .await?;

    let unit_path: zbus::zvariant::OwnedObjectPath = manager_proxy
        .call("GetUnit", &(unit,))
        .await
        .map_err(|_| anyhow!("Unit {} not found", unit))?;

    let unit_proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.systemd1",
        unit_path.as_str(),
        "org.freedesktop.systemd1.Unit",
    )
    .await?;

    let env: Vec<String> = unit_proxy.get_property("Environment").await?;
    Ok(env)
}

// ============================================================================
// /proc HELPERS (no pgrep / netstat bypasses)
// ============================================================================

fn find_pids_by_name(name: &str) -> Vec<String> {
    let mut pids = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.filter_map(|e| e.ok()) {
            let file_name = entry.file_name();
            if file_name.to_string_lossy().parse::<u32>().is_ok() {
                let cmdline_path = entry.path().join("cmdline");
                if let Ok(cmdline) = std::fs::read_to_string(&cmdline_path) {
                    if cmdline.contains(name) {
                        pids.push(file_name.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    pids
}

fn read_proc_net_tcp() -> Result<String> {
    std::fs::read_to_string("/proc/net/tcp")
        .map_err(|e| anyhow!("Failed to read /proc/net/tcp: {}", e))
}

// ============================================================================
// X11 HELPERS (no xdpyinfo / xauth bypasses)
// ============================================================================

fn check_x11_socket(display: &str) -> bool {
    let socket_path = if display.starts_with(':') {
        format!("/tmp/.X11-unix/X{}", display.trim_start_matches(':'))
    } else {
        return false;
    };
    Path::new(&socket_path).exists()
}

fn read_xauth_cookie(_display: &str) -> bool {
    let xauthority = std::env::var("XAUTHORITY").unwrap_or_else(|_| {
        dirs::home_dir()
            .map(|h| h.join(".Xauthority").to_string_lossy().to_string())
            .unwrap_or_default()
    });
    if Path::new(&xauthority).exists() {
        // We don't parse the xauth file; just verify it exists and is non-empty.
        if let Ok(meta) = std::fs::metadata(&xauthority) {
            return meta.len() > 0;
        }
    }
    false
}

/// Helper function to get AnyDesk ID
async fn get_anydesk_id() -> Result<String> {
    // Try to get ID from AnyDesk configuration or command
    // First check if we can run anydesk command to get ID

    // Check for AnyDesk ID in various locations
    let config_paths = vec![
        "/etc/anydesk/anydesk.conf",
        "/home/jeremy/.anydesk/anydesk.conf",
        "/home/jeremy/.anydesk/user.conf",
    ];

    for path in config_paths {
        if Path::new(path).exists() {
            if let Ok(content) = fs::read_to_string(path) {
                // Look for ID in config file
                for line in content.lines() {
                    if line.contains("ad.anynet.id") || line.contains("id=") {
                        // Parse the ID from the line
                        if let Some(id_part) = line.split('=').nth(1) {
                            let id = id_part.trim().trim_matches('"');
                            if !id.is_empty() && id != "0" {
                                return Ok(id.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // Try to run anydesk command if available
    match Command::new("anydesk").arg("--get-id").output() {
        Ok(output) if output.status.success() => {
            let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !id.is_empty() {
                return Ok(id);
            }
        }
        _ => {}
    }

    // Fallback: check systemd service MainPID via D-Bus
    if let Ok(pid_num) = get_systemd_unit_pid_dbus("anydesk.service").await {
        if pid_num > 0 {
            return Ok(format!("running_pid_{}", pid_num));
        }
    }

    Err(anyhow!(
        "Could not determine AnyDesk ID. AnyDesk may not be properly configured or running."
    ))
}

/// Helper function to get AnyDesk service status
async fn get_anydesk_status() -> Result<Value> {
    let mut status = json!({
        "service_running": false,
        "version": null,
        "connections": []
    });

    // Check systemd service status via D-Bus
    if let Ok(active_state) = get_systemd_unit_active_state_dbus("anydesk.service").await {
        status["service_running"] = json!(active_state == "active");
        status["active_state"] = json!(active_state);
    }

    // Check if anydesk process is running via /proc
    let pids = find_pids_by_name("anydesk");
    if !pids.is_empty() {
        status["process_pids"] = json!(pids);
    }

    // Try to get version
    match Command::new("anydesk").arg("--version").output() {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            status["version"] = json!(version);
        }
        _ => {}
    }

    Ok(status)
}

/// Helper function to control AnyDesk service
async fn control_anydesk_service(action: &str) -> Result<String> {
    control_systemd_unit_dbus("anydesk.service", action).await
}

/// Helper function to get AnyDesk connections
fn get_anydesk_connections() -> Result<Vec<Value>> {
    // AnyDesk doesn't provide a direct way to list connections
    // This is a placeholder for future implementation
    // In a real implementation, this might parse logs or use AnyDesk's API

    let connections = Vec::new();

    // Check for any active connections by looking at /proc/net/tcp
    // instead of spawning netstat. AnyDesk typically uses ports 7070, 6568.
    if let Ok(tcp_output) = read_proc_net_tcp() {
        let anydesk_ports = ["7070", "6568", "1B9E", "19A8"]; // decimal + hex
        for line in tcp_output.lines().skip(1) {
            // /proc/net/tcp columns: sl local_address rem_address st tx_queue:rx_queue ...
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let local_addr = parts[1];
                // Port is the second half of local_address (hex after colon)
                if let Some(colon_pos) = local_addr.rfind(':') {
                    let port_hex = &local_addr[colon_pos + 1..];
                    if anydesk_ports.contains(&port_hex) {
                        // Potential AnyDesk listening port found
                        // Full parsing would convert hex IP:port to readable form
                    }
                }
            }
        }
    }

    Ok(connections)
}

/// Helper function to check X11 display environment
async fn check_x11_display_environment() -> Result<Value> {
    let mut result = json!({
        "display_available": false,
        "display_variable": null,
        "xauthority_available": false,
        "xauthority_path": null,
        "anydesk_service_environment": {},
        "x11_server_running": false,
        "x11_auth_configured": false
    });

    // Check DISPLAY environment variable
    if let Ok(display) = std::env::var("DISPLAY") {
        result["display_variable"] = json!(display);
        // Check X11 Unix socket directly instead of spawning xdpyinfo
        if check_x11_socket(&display) {
            result["x11_server_running"] = json!(true);
            result["display_available"] = json!(true);
        }
    }

    // Check XAUTHORITY environment variable and file
    let xauthority = std::env::var("XAUTHORITY").unwrap_or_else(|_| {
        dirs::home_dir()
            .map(|h| h.join(".Xauthority").to_string_lossy().to_string())
            .unwrap_or_default()
    });
    result["xauthority_path"] = json!(xauthority);
    result["xauthority_available"] = json!(Path::new(&xauthority).exists());

    // Check AnyDesk service environment via D-Bus
    if let Ok(env) = get_systemd_unit_environment_dbus("anydesk.service").await {
        let env_map: std::collections::HashMap<String, String> = env
            .iter()
            .filter_map(|kv| {
                kv.split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
            })
            .collect();
        result["anydesk_service_environment"] = json!(env_map);
    }

    // Check X11 authentication via file existence instead of xauth
    if let Ok(display) = std::env::var("DISPLAY") {
        if read_xauth_cookie(&display) {
            result["x11_auth_configured"] = json!(true);
        }
    }

    Ok(result)
}

/// Helper function to diagnose X11 access issues
async fn diagnose_x11_access_issues() -> Result<Value> {
    let mut issues = Vec::new();
    let mut recommendations = Vec::new();
    let mut fix_commands = Vec::new();

    // Check if AnyDesk service is running via D-Bus
    match get_systemd_unit_active_state_dbus("anydesk.service").await {
        Ok(active_state) => {
            if active_state != "active" {
                issues.push("AnyDesk service is not running".to_string());
                recommendations.push(
                    "Start AnyDesk service via D-Bus: dbus_systemd_start_unit anydesk.service"
                        .to_string(),
                );
                fix_commands.push("dbus_systemd_start_unit anydesk.service".to_string());
            }
        }
        _ => {
            issues.push("Cannot determine AnyDesk service status".to_string());
        }
    }

    // Check DISPLAY / XAUTHORITY environment for AnyDesk service via D-Bus
    match get_systemd_unit_environment_dbus("anydesk.service").await {
        Ok(env) => {
            let has_display = env.iter().any(|e| e.starts_with("DISPLAY="));
            let has_xauthority = env.iter().any(|e| e.starts_with("XAUTHORITY="));

            if !has_display {
                issues.push("AnyDesk service missing DISPLAY environment variable".to_string());
                recommendations.push("Add DISPLAY=:99 to AnyDesk service environment".to_string());
                fix_commands.push(
                    "dbus_systemd_set_unit_environment anydesk.service DISPLAY=:99".to_string(),
                );
            }

            if !has_xauthority {
                issues.push("AnyDesk service missing XAUTHORITY environment variable".to_string());
                recommendations.push(
                    "Add XAUTHORITY=/root/.Xauthority to AnyDesk service environment".to_string(),
                );
                fix_commands.push("dbus_systemd_set_unit_environment anydesk.service XAUTHORITY=/root/.Xauthority".to_string());
            }
        }
        _ => {
            issues.push("Cannot check AnyDesk service environment".to_string());
        }
    }

    // Check X11 server accessibility via Unix socket instead of xdpyinfo
    if let Ok(display) = std::env::var("DISPLAY") {
        if !check_x11_socket(&display) {
            issues.push(format!("Cannot access X11 display {}", display));
            recommendations
                .push("Ensure Xvfb or X server is running on the specified display".to_string());
        }
    } else {
        issues.push("DISPLAY environment variable not set".to_string());
        recommendations.push("Set DISPLAY=:99 for headless X11 server".to_string());
    }

    // Check X11 authentication via file instead of xauth
    if let Ok(display) = std::env::var("DISPLAY") {
        if !read_xauth_cookie(&display) {
            issues.push(format!(
                "No X11 authentication configured for display {}",
                display
            ));
            recommendations.push(
                "Generate X11 authentication cookie with: xauth generate :99 . trusted".to_string(),
            );
            fix_commands.push("xauth generate :99 . trusted".to_string());
        }
    } else {
        issues.push("Cannot check X11 authentication (DISPLAY not set)".to_string());
    }

    // Check if Xauthority file exists for root
    if !Path::new("/root/.Xauthority").exists() {
        issues.push("Xauthority file missing for root user".to_string());
        recommendations.push(
            "Copy user Xauthority to root: cp /home/user/.Xauthority /root/.Xauthority".to_string(),
        );
        fix_commands.push("cp /home/jeremy/.Xauthority /root/.Xauthority && chown root:root /root/.Xauthority && chmod 600 /root/.Xauthority".to_string());
    }

    let diagnosis = json!({
        "issues": issues,
        "recommendations": recommendations,
        "can_fix_automatically": !fix_commands.is_empty(),
        "fix_commands": fix_commands
    });

    Ok(diagnosis)
}
