//! Incus Container Management Tools
//!
//! These tools expose Incus instance operations (containers and VMs) to the
//! LLM chat system using the Incus REST API over Unix socket.
//! NO CLI COMMANDS (incus) are used.

use crate::Tool;
use crate::ToolRegistry;
use anyhow::Result;
use async_trait::async_trait;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

const INCUS_SOCKET: &str = "/var/lib/incus/unix.socket";

// ---------------------------------------------------------------------------
// Helper: raw HTTP/1.1 over Unix socket to Incus REST API
// ---------------------------------------------------------------------------

async fn incus_rest_request(method: &str, path: &str, body: Option<Value>) -> Result<Value> {
    let stream = UnixStream::connect(INCUS_SOCKET)
        .await
        .map_err(|e| anyhow::anyhow!("Cannot connect to Incus socket {}: {}", INCUS_SOCKET, e))?;

    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let body_str = body
        .map(|b| simd_json::to_string(&b).unwrap_or_default())
        .unwrap_or_default();

    let request = format!(
        "{} {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        method,
        path,
        body_str.len(),
        body_str
    );

    writer
        .write_all(request.as_bytes())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to write to Incus socket: {}", e))?;

    // Read status line
    let mut status_line = String::new();
    reader
        .read_line(&mut status_line)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read Incus response: {}", e))?;

    // Parse status code from something like "HTTP/1.1 200 OK\r\n"
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(0);

    // Read headers until empty line
    let mut content_length = None;
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read Incus headers: {}", e))?;
        if line.trim().is_empty() {
            break;
        }
        let lower = line.to_lowercase();
        if lower.starts_with("content-length:") {
            content_length = lower
                .split(':')
                .nth(1)
                .and_then(|s| s.trim().parse::<usize>().ok());
        }
        // Handle chunked transfer (Incus shouldn't use it for small responses,
        // but if it does we'll read until connection closes — not ideal but rare)
    }

    // Read body
    let mut body_buf = Vec::new();
    if let Some(len) = content_length {
        body_buf.resize(len, 0);
        reader
            .read_exact(&mut body_buf)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read Incus body: {}", e))?;
    } else {
        // Fallback: read all remaining bytes
        let mut remainder = Vec::new();
        reader
            .read_to_end(&mut remainder)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to read Incus body: {}", e))?;
        body_buf = remainder;
    }

    let body_str = String::from_utf8_lossy(&body_buf);
    let mut json_bytes = body_str.into_bytes();
    let response: Value = simd_json::from_slice(&mut json_bytes)
        .map_err(|e| anyhow::anyhow!("Failed to parse Incus JSON: {}", e))?;

    if status_code >= 400 {
        let err_msg = response
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown Incus error")
            .to_string();
        return Err(anyhow::anyhow!("Incus API error ({}): {}", status_code, err_msg));
    }

    Ok(response)
}

// ---------------------------------------------------------------------------
// Helper: extract metadata or error from Incus sync response
// ---------------------------------------------------------------------------

fn incus_sync_metadata(response: Value) -> Result<Value> {
    let err = response
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !err.is_empty() {
        return Err(anyhow::anyhow!("Incus error: {}", err));
    }
    Ok(response.get("metadata").cloned().unwrap_or(Value::Null))
}

// ---------------------------------------------------------------------------
// 1. IncusCheckAvailableTool
// ---------------------------------------------------------------------------

/// Tool to check if incusd is running and available
pub struct IncusCheckAvailableTool;

#[async_trait]
impl Tool for IncusCheckAvailableTool {
    fn name(&self) -> &str {
        "incus_check_available"
    }

    fn description(&self) -> &str {
        "Check if incusd is running and available. Returns version info if connected. Use this first to verify Incus operations will work."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn category(&self) -> &str {
        "containers"
    }

    fn tags(&self) -> Vec<String> {
        vec!["incus".into(), "check".into(), "status".into()]
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        match incus_rest_request("GET", "/1.0", None).await {
            Ok(response) => {
                let metadata = incus_sync_metadata(response)?;
                let version = metadata
                    .get("environment")
                    .and_then(|e| e.get("server_version"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                Ok(json!({
                    "available": true,
                    "version": version,
                    "message": format!("Incus {} is available", version)
                }))
            }
            Err(e) => Ok(json!({
                "available": false,
                "error": e.to_string(),
                "message": "Incus is not available or incusd is not running"
            })),
        }
    }
}

// ---------------------------------------------------------------------------
// 2. IncusListInstancesTool
// ---------------------------------------------------------------------------

/// Tool to list all Incus instances (containers and VMs) with status
pub struct IncusListInstancesTool;

#[async_trait]
impl Tool for IncusListInstancesTool {
    fn name(&self) -> &str {
        "incus_list_instances"
    }

    fn description(&self) -> &str {
        "List all Incus instances (containers and VMs) with status. Optionally filter by type ('container' or 'virtual-machine')."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "type": {
                    "type": "string",
                    "description": "Filter by instance type: 'container' or 'virtual-machine'",
                    "enum": ["container", "virtual-machine"]
                }
            },
            "required": []
        })
    }

    fn category(&self) -> &str {
        "containers"
    }

    fn tags(&self) -> Vec<String> {
        vec!["incus".into(), "containers".into(), "list".into()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let type_filter = input.get("type").and_then(|v| v.as_str());

        let response = incus_rest_request("GET", "/1.0/instances?recursion=1", None).await?;
        let metadata = incus_sync_metadata(response)?;

        let instances: Vec<Value> = if let Some(arr) = metadata.as_array() {
            arr.clone()
        } else {
            Vec::new()
        };

        // Apply type filter if provided
        let filtered: Vec<&Value> = if let Some(filter) = type_filter {
            instances
                .iter()
                .filter(|inst| {
                    inst.get("type")
                        .and_then(|t| t.as_str())
                        .map(|t| t == filter)
                        .unwrap_or(false)
                })
                .collect()
        } else {
            instances.iter().collect()
        };

        let count = filtered.len();
        Ok(json!({
            "instances": filtered,
            "count": count,
            "filter": type_filter
        }))
    }
}

// ---------------------------------------------------------------------------
// 3. IncusGetInstanceTool
// ---------------------------------------------------------------------------

/// Tool to get detailed info about a specific Incus instance
pub struct IncusGetInstanceTool;

#[async_trait]
impl Tool for IncusGetInstanceTool {
    fn name(&self) -> &str {
        "incus_get_instance"
    }

    fn description(&self) -> &str {
        "Get detailed info about a specific Incus instance including its full expanded configuration."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Instance name"
                }
            },
            "required": ["name"]
        })
    }

    fn category(&self) -> &str {
        "containers"
    }

    fn tags(&self) -> Vec<String> {
        vec!["incus".into(), "containers".into(), "info".into()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: name"))?;

        let path = format!("/1.0/instances/{}", name);
        let response = incus_rest_request("GET", &path, None).await?;
        let metadata = incus_sync_metadata(response)?;

        Ok(json!({
            "name": name,
            "config": metadata,
            "message": format!("Configuration for instance '{}'", name)
        }))
    }
}

// ---------------------------------------------------------------------------
// 4. IncusLaunchInstanceTool
// ---------------------------------------------------------------------------

/// Tool to launch a new Incus instance from an image (creates and starts it)
pub struct IncusLaunchInstanceTool;

#[async_trait]
impl Tool for IncusLaunchInstanceTool {
    fn name(&self) -> &str {
        "incus_launch_instance"
    }

    fn description(&self) -> &str {
        "Launch a new Incus instance from an image (creates and starts it). For example, 'images:debian/13' or 'images:ubuntu/24.04'."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "image": {
                    "type": "string",
                    "description": "Image to launch from (e.g. 'images:debian/13', 'images:ubuntu/24.04')"
                },
                "name": {
                    "type": "string",
                    "description": "Name for the new instance"
                },
                "type": {
                    "type": "string",
                    "description": "Instance type: 'container' (default) or 'virtual-machine'",
                    "enum": ["container", "virtual-machine"]
                },
                "profile": {
                    "type": "string",
                    "description": "Profile to apply to the instance"
                }
            },
            "required": ["image", "name"]
        })
    }

    fn category(&self) -> &str {
        "containers"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "incus".into(),
            "containers".into(),
            "create".into(),
            "write".into(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let image = input
            .get("image")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: image"))?;

        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: name"))?;

        let instance_type = input.get("type").and_then(|v| v.as_str());
        let profile = input.get("profile").and_then(|v| v.as_str());

        let mut body = json!({
            "name": name,
            "source": {
                "type": "image",
                "alias": image,
                "mode": "pull",
                "protocol": "simplestreams",
                "server": "https://images.linuxcontainers.org"
            }
        });

        if let Some(t) = instance_type {
            if let Some(obj) = body.as_object_mut() {
                obj.insert("type".to_string(), json!(t));
            }
        }
        if let Some(p) = profile {
            if let Some(obj) = body.as_object_mut() {
                obj.insert("profiles".to_string(), json!([p]));
            }
        }

        let response = incus_rest_request("POST", "/1.0/instances", Some(body)).await?;
        let metadata = incus_sync_metadata(response)?;

        Ok(json!({
            "success": true,
            "name": name,
            "image": image,
            "type": instance_type.unwrap_or("container"),
            "profile": profile,
            "metadata": metadata,
            "message": format!("Instance '{}' launched successfully from {}", name, image)
        }))
    }
}

// ---------------------------------------------------------------------------
// 5. IncusStartInstanceTool
// ---------------------------------------------------------------------------

/// Tool to start a stopped Incus instance
pub struct IncusStartInstanceTool;

#[async_trait]
impl Tool for IncusStartInstanceTool {
    fn name(&self) -> &str {
        "incus_start_instance"
    }

    fn description(&self) -> &str {
        "Start a stopped Incus instance."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Instance name to start"
                }
            },
            "required": ["name"]
        })
    }

    fn category(&self) -> &str {
        "containers"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "incus".into(),
            "containers".into(),
            "start".into(),
            "write".into(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: name"))?;

        let path = format!("/1.0/instances/{}/state", name);
        let body = json!({"action": "start"});
        let response = incus_rest_request("PUT", &path, Some(body)).await?;
        let metadata = incus_sync_metadata(response)?;

        Ok(json!({
            "success": true,
            "name": name,
            "metadata": metadata,
            "message": format!("Instance '{}' started successfully", name)
        }))
    }
}

// ---------------------------------------------------------------------------
// 6. IncusStopInstanceTool
// ---------------------------------------------------------------------------

/// Tool to stop a running Incus instance
pub struct IncusStopInstanceTool;

#[async_trait]
impl Tool for IncusStopInstanceTool {
    fn name(&self) -> &str {
        "incus_stop_instance"
    }

    fn description(&self) -> &str {
        "Stop a running Incus instance. Use force=true for immediate stop."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Instance name to stop"
                },
                "force": {
                    "type": "boolean",
                    "description": "Force stop immediately (default: false for graceful shutdown)"
                }
            },
            "required": ["name"]
        })
    }

    fn category(&self) -> &str {
        "containers"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "incus".into(),
            "containers".into(),
            "stop".into(),
            "write".into(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: name"))?;

        let force = input
            .get("force")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let path = format!("/1.0/instances/{}/state", name);
        let body = if force {
            json!({"action": "stop", "force": true})
        } else {
            json!({"action": "stop"})
        };
        let response = incus_rest_request("PUT", &path, Some(body)).await?;
        let metadata = incus_sync_metadata(response)?;

        Ok(json!({
            "success": true,
            "name": name,
            "force": force,
            "metadata": metadata,
            "message": format!("Instance '{}' stopped successfully", name)
        }))
    }
}

// ---------------------------------------------------------------------------
// 7. IncusDeleteInstanceTool
// ---------------------------------------------------------------------------

/// Tool to delete an Incus instance permanently
pub struct IncusDeleteInstanceTool;

#[async_trait]
impl Tool for IncusDeleteInstanceTool {
    fn name(&self) -> &str {
        "incus_delete_instance"
    }

    fn description(&self) -> &str {
        "Delete an Incus instance permanently. WARNING: Destroys the instance and its data. Use force=true to delete a running instance."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Instance name to delete"
                },
                "force": {
                    "type": "boolean",
                    "description": "Force delete even if running (default: false)"
                }
            },
            "required": ["name"]
        })
    }

    fn category(&self) -> &str {
        "containers"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "incus".into(),
            "containers".into(),
            "delete".into(),
            "write".into(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: name"))?;

        let force = input
            .get("force")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let path = if force {
            format!("/1.0/instances/{}?force=true", name)
        } else {
            format!("/1.0/instances/{}", name)
        };
        let response = incus_rest_request("DELETE", &path, None).await?;
        let metadata = incus_sync_metadata(response)?;

        Ok(json!({
            "success": true,
            "name": name,
            "force": force,
            "metadata": metadata,
            "message": format!("Instance '{}' deleted successfully", name)
        }))
    }
}

// ---------------------------------------------------------------------------
// 8. IncusExecTool
// ---------------------------------------------------------------------------

/// Tool to execute a command inside an Incus instance
pub struct IncusExecTool;

#[async_trait]
impl Tool for IncusExecTool {
    fn name(&self) -> &str {
        "incus_exec"
    }

    fn description(&self) -> &str {
        "Execute a command inside an Incus instance. The command can be a single string (run via sh -c) or an array of strings (run directly)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Instance name to execute the command in"
                },
                "command": {
                    "description": "Command to execute. String (run via 'sh -c') or array of strings (run directly).",
                    "oneOf": [
                        { "type": "string" },
                        { "type": "array", "items": { "type": "string" } }
                    ]
                }
            },
            "required": ["name", "command"]
        })
    }

    fn category(&self) -> &str {
        "containers"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "incus".into(),
            "containers".into(),
            "exec".into(),
            "write".into(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: name"))?;

        let command_value = input
            .get("command")
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: command"))?;

        let (cmd, args): (String, Vec<String>) = if let Some(cmd_str) = command_value.as_str() {
            ("sh".into(), vec!["-c".into(), cmd_str.to_string()])
        } else if let Some(cmd_array) = command_value.as_array() {
            let parts: Vec<String> = cmd_array
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if parts.is_empty() {
                return Err(anyhow::anyhow!("command must not be empty"));
            }
            let head = parts[0].clone();
            let tail = parts.into_iter().skip(1).collect();
            (head, tail)
        } else {
            return Err(anyhow::anyhow!(
                "command must be a string or array of strings"
            ));
        };

        let body = json!({
            "command": [cmd],
            "args": args,
            "wait-for-websocket": false,
            "record-output": true,
            "interactive": false
        });

        let path = format!("/1.0/instances/{}/exec", name);
        let response = incus_rest_request("POST", &path, Some(body)).await?;
        let metadata = incus_sync_metadata(response)?;

        // Incus exec returns an operation ID; poll it for results.
        let op_id = metadata
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if op_id.is_empty() {
            return Ok(json!({
                "name": name,
                "message": format!("Command submitted in '{}' but no operation ID returned", name),
                "metadata": metadata
            }));
        }

        // Poll operation until it completes (max ~10s)
        let mut stdout = String::new();
        let mut stderr = String::new();
        let mut return_code = -1;

        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let op_path = format!("/1.0/operations/{}", op_id);
            let op_resp = incus_rest_request("GET", &op_path, None).await?;
            let op_meta = incus_sync_metadata(op_resp)?;

            let status = op_meta
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if status == "Success" {
                if let Some(ret) = op_meta.get("return") {
                    return_code = ret.as_i64().unwrap_or(-1) as i32;
                }
                if let Some(out) = op_meta.get("output") {
                    if let Some(one) = out.get("1") {
                        stdout = one
                            .get("data")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                    }
                    if let Some(two) = out.get("2") {
                        stderr = two
                            .get("data")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                    }
                }
                break;
            } else if status == "Failure" {
                return_code = 1;
                stderr = op_meta
                    .get("err")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Command failed")
                    .to_string();
                break;
            }
        }

        let success = return_code == 0;
        Ok(json!({
            "name": name,
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": return_code,
            "success": success,
            "message": if success {
                format!("Command executed successfully in '{}'", name)
            } else {
                format!("Command failed in '{}'", name)
            }
        }))
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register all Incus tools with the tool registry
pub async fn register_incus_tools(registry: &ToolRegistry) -> Result<()> {
    registry
        .register_tool(Arc::new(IncusCheckAvailableTool))
        .await?;
    registry
        .register_tool(Arc::new(IncusListInstancesTool))
        .await?;
    registry
        .register_tool(Arc::new(IncusGetInstanceTool))
        .await?;
    registry
        .register_tool(Arc::new(IncusLaunchInstanceTool))
        .await?;
    registry
        .register_tool(Arc::new(IncusStartInstanceTool))
        .await?;
    registry
        .register_tool(Arc::new(IncusStopInstanceTool))
        .await?;
    registry
        .register_tool(Arc::new(IncusDeleteInstanceTool))
        .await?;
    registry.register_tool(Arc::new(IncusExecTool)).await?;
    tracing::info!("Registered 8 Incus container tools (REST API over Unix socket)");
    Ok(())
}

/// Create all Incus tools as a vector
pub fn create_incus_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(IncusCheckAvailableTool),
        Arc::new(IncusListInstancesTool),
        Arc::new(IncusGetInstanceTool),
        Arc::new(IncusLaunchInstanceTool),
        Arc::new(IncusStartInstanceTool),
        Arc::new(IncusStopInstanceTool),
        Arc::new(IncusDeleteInstanceTool),
        Arc::new(IncusExecTool),
    ]
}
