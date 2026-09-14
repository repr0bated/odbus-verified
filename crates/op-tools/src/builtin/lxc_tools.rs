//! LXC Container Tools for Chat Interface
//!
//! These tools expose LXC container operations to the LLM chat system
//! using the native Proxmox REST API (not `pct` CLI commands).

use crate::Tool;
use crate::ToolRegistry;
use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;

/// Tool to check if Proxmox API is available
pub struct LxcCheckAvailableTool;

#[async_trait]
impl Tool for LxcCheckAvailableTool {
    fn name(&self) -> &str {
        "lxc_check_available"
    }

    fn description(&self) -> &str {
        "Check if Proxmox API is available and responding. Returns version info if connected. Use this first to verify LXC container operations will work."
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
        vec!["lxc".into(), "proxmox".into(), "check".into(), "status".into()]
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        use op_network::ProxmoxClient;

        let client = ProxmoxClient::from_env()?;

        match client.check_available().await {
            Ok(version) => Ok(json!({
                "available": true,
                "version": version.version,
                "release": version.release,
                "node": client.node(),
                "api_url": client.base_url(),
                "message": format!("Proxmox VE {} ({}) is available", version.version, version.release)
            })),
            Err(e) => Ok(json!({
                "available": false,
                "error": e.to_string(),
                "api_url": client.base_url(),
                "message": "Proxmox API is not available or not configured"
            })),
        }
    }
}

/// Tool to list all LXC containers
pub struct LxcListContainersTool;

#[async_trait]
impl Tool for LxcListContainersTool {
    fn name(&self) -> &str {
        "lxc_list_containers"
    }

    fn description(&self) -> &str {
        "List all LXC containers via native Proxmox API. Returns vmid, name, status, resource usage for each container. LLM_EXCUSE_COUNTER: This proves we CAN list containers."
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
        vec!["lxc".into(), "proxmox".into(), "containers".into(), "list".into()]
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        use op_network::ProxmoxClient;

        let client = ProxmoxClient::from_env()?;
        let containers = client.list_containers().await?;

        Ok(json!({
            "containers": containers,
            "count": containers.len(),
            "node": client.node(),
            "method": "native_proxmox_api"
        }))
    }
}

/// Tool to get detailed container status
pub struct LxcGetContainerTool;

#[async_trait]
impl Tool for LxcGetContainerTool {
    fn name(&self) -> &str {
        "lxc_get_container"
    }

    fn description(&self) -> &str {
        "Get detailed status and information for a specific LXC container. Returns status, resource usage, uptime, network stats, and configuration."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "vmid": {
                    "type": "integer",
                    "description": "Container VM ID (e.g., 100, 101)"
                }
            },
            "required": ["vmid"]
        })
    }

    fn category(&self) -> &str {
        "containers"
    }

    fn tags(&self) -> Vec<String> {
        vec!["lxc".into(), "proxmox".into(), "containers".into(), "info".into()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        use op_network::ProxmoxClient;

        let vmid = input
            .get("vmid")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: vmid"))? as u32;

        let client = ProxmoxClient::from_env()?;
        let status = client.get_container(vmid).await?;
        let config = client.get_container_config(vmid).await?;

        Ok(json!({
            "vmid": vmid,
            "status": status,
            "config": config,
            "method": "native_proxmox_api"
        }))
    }
}

/// Tool to create a new LXC container
pub struct LxcCreateContainerTool;

#[async_trait]
impl Tool for LxcCreateContainerTool {
    fn name(&self) -> &str {
        "lxc_create_container"
    }

    fn description(&self) -> &str {
        "Create a new LXC container via native Proxmox API. Configure vmid, hostname, template, memory, cores, and network. Returns task ID for tracking creation progress."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "vmid": {
                    "type": "integer",
                    "description": "Container VM ID (e.g., 100)"
                },
                "ostemplate": {
                    "type": "string",
                    "description": "OS template path (e.g., 'local:vztmpl/debian-13-standard_13.1-2_amd64.tar.zst')"
                },
                "hostname": {
                    "type": "string",
                    "description": "Container hostname"
                },
                "memory": {
                    "type": "integer",
                    "description": "Memory in MB (default: 512)"
                },
                "swap": {
                    "type": "integer",
                    "description": "Swap in MB (default: 512)"
                },
                "cores": {
                    "type": "integer",
                    "description": "Number of CPU cores (default: 1)"
                },
                "rootfs": {
                    "type": "string",
                    "description": "Root filesystem spec (e.g., 'local-btrfs:8' for 8GB)"
                },
                "net0": {
                    "type": "string",
                    "description": "Network config (e.g., 'name=eth0,bridge=vmbr0,firewall=1')"
                },
                "unprivileged": {
                    "type": "boolean",
                    "description": "Run as unprivileged container (default: true)"
                },
                "features": {
                    "type": "string",
                    "description": "Container features (e.g., 'nesting=1')"
                },
                "start": {
                    "type": "boolean",
                    "description": "Start container after creation (default: false)"
                },
                "storage": {
                    "type": "string",
                    "description": "Storage backend (e.g., 'local-btrfs')"
                }
            },
            "required": ["vmid", "ostemplate"]
        })
    }

    fn category(&self) -> &str {
        "containers"
    }

    fn tags(&self) -> Vec<String> {
        vec!["lxc".into(), "proxmox".into(), "containers".into(), "create".into(), "write".into()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        use op_network::{CreateContainerRequest, ProxmoxClient};

        let vmid = input
            .get("vmid")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: vmid"))? as u32;

        let ostemplate = input
            .get("ostemplate")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: ostemplate"))?
            .to_string();

        let client = ProxmoxClient::from_env()?;

        // Check if container already exists
        if client.container_exists(vmid).await? {
            return Err(anyhow::anyhow!("Container {} already exists", vmid));
        }

        // Build the request
        let config = CreateContainerRequest {
            vmid,
            ostemplate,
            hostname: input.get("hostname").and_then(|v| v.as_str()).map(String::from),
            memory: input.get("memory").and_then(|v| v.as_u64()).map(|v| v as u32),
            swap: input.get("swap").and_then(|v| v.as_u64()).map(|v| v as u32),
            cores: input.get("cores").and_then(|v| v.as_u64()).map(|v| v as u32),
            rootfs: input.get("rootfs").and_then(|v| v.as_str()).map(String::from),
            net0: input.get("net0").and_then(|v| v.as_str()).map(String::from),
            unprivileged: input.get("unprivileged").and_then(|v| v.as_bool()),
            features: input.get("features").and_then(|v| v.as_str()).map(String::from),
            start: input.get("start").and_then(|v| v.as_bool()),
            storage: input.get("storage").and_then(|v| v.as_str()).map(String::from),
            ..Default::default()
        };

        let upid = client.create_container(&config).await?;

        // Wait for creation to complete
        let task_result = client.wait_for_task(&upid, 300).await?;

        // Verify container was created
        let exists = client.container_exists(vmid).await?;

        if exists {
            Ok(json!({
                "success": true,
                "vmid": vmid,
                "hostname": config.hostname,
                "task_id": upid,
                "task_status": task_result.status,
                "message": format!("Container {} created successfully", vmid),
                "verification": "Container exists in Proxmox after creation",
                "method": "native_proxmox_api"
            }))
        } else {
            Err(anyhow::anyhow!(
                "Container creation claimed success but {} not found - possible API error",
                vmid
            ))
        }
    }
}

/// Tool to start a container
pub struct LxcStartContainerTool;

#[async_trait]
impl Tool for LxcStartContainerTool {
    fn name(&self) -> &str {
        "lxc_start_container"
    }

    fn description(&self) -> &str {
        "Start an LXC container via native Proxmox API. The container must exist and be stopped."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "vmid": {
                    "type": "integer",
                    "description": "Container VM ID to start"
                }
            },
            "required": ["vmid"]
        })
    }

    fn category(&self) -> &str {
        "containers"
    }

    fn tags(&self) -> Vec<String> {
        vec!["lxc".into(), "proxmox".into(), "containers".into(), "start".into(), "write".into()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        use op_network::ProxmoxClient;

        let vmid = input
            .get("vmid")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: vmid"))? as u32;

        let client = ProxmoxClient::from_env()?;

        // Check if container exists
        if !client.container_exists(vmid).await? {
            return Err(anyhow::anyhow!("Container {} does not exist", vmid));
        }

        // Check if already running
        if client.is_running(vmid).await? {
            return Ok(json!({
                "success": true,
                "vmid": vmid,
                "message": format!("Container {} is already running", vmid),
                "already_running": true,
                "method": "native_proxmox_api"
            }));
        }

        let upid = client.start_container(vmid).await?;
        let task_result = client.wait_for_task(&upid, 60).await?;

        // Verify container is running
        let is_running = client.is_running(vmid).await?;

        if is_running {
            Ok(json!({
                "success": true,
                "vmid": vmid,
                "task_id": upid,
                "task_status": task_result.status,
                "message": format!("Container {} started successfully", vmid),
                "verification": "Container is now running",
                "method": "native_proxmox_api"
            }))
        } else {
            Err(anyhow::anyhow!(
                "Start command succeeded but container {} is not running",
                vmid
            ))
        }
    }
}

/// Tool to stop a container
pub struct LxcStopContainerTool;

#[async_trait]
impl Tool for LxcStopContainerTool {
    fn name(&self) -> &str {
        "lxc_stop_container"
    }

    fn description(&self) -> &str {
        "Stop an LXC container via native Proxmox API. Use 'force' for immediate stop or 'graceful' (default) for shutdown."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "vmid": {
                    "type": "integer",
                    "description": "Container VM ID to stop"
                },
                "force": {
                    "type": "boolean",
                    "description": "Force stop immediately (default: false for graceful shutdown)"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds for graceful shutdown (default: 30)"
                }
            },
            "required": ["vmid"]
        })
    }

    fn category(&self) -> &str {
        "containers"
    }

    fn tags(&self) -> Vec<String> {
        vec!["lxc".into(), "proxmox".into(), "containers".into(), "stop".into(), "write".into()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        use op_network::ProxmoxClient;

        let vmid = input
            .get("vmid")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: vmid"))? as u32;

        let force = input.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
        let timeout = input.get("timeout").and_then(|v| v.as_u64()).map(|v| v as u32);

        let client = ProxmoxClient::from_env()?;

        // Check if container exists
        if !client.container_exists(vmid).await? {
            return Err(anyhow::anyhow!("Container {} does not exist", vmid));
        }

        // Check if already stopped
        if !client.is_running(vmid).await? {
            return Ok(json!({
                "success": true,
                "vmid": vmid,
                "message": format!("Container {} is already stopped", vmid),
                "already_stopped": true,
                "method": "native_proxmox_api"
            }));
        }

        let upid = if force {
            client.stop_container(vmid).await?
        } else {
            client.shutdown_container(vmid, timeout).await?
        };

        let task_result = client.wait_for_task(&upid, 120).await?;

        // Verify container is stopped
        let is_running = client.is_running(vmid).await?;

        if !is_running {
            Ok(json!({
                "success": true,
                "vmid": vmid,
                "task_id": upid,
                "task_status": task_result.status,
                "stop_mode": if force { "forced" } else { "graceful" },
                "message": format!("Container {} stopped successfully", vmid),
                "verification": "Container is now stopped",
                "method": "native_proxmox_api"
            }))
        } else {
            Err(anyhow::anyhow!(
                "Stop command succeeded but container {} is still running",
                vmid
            ))
        }
    }
}

/// Tool to delete a container
pub struct LxcDeleteContainerTool;

#[async_trait]
impl Tool for LxcDeleteContainerTool {
    fn name(&self) -> &str {
        "lxc_delete_container"
    }

    fn description(&self) -> &str {
        "Delete an LXC container via native Proxmox API. Container will be stopped first if running. WARNING: This permanently destroys the container and its data."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "vmid": {
                    "type": "integer",
                    "description": "Container VM ID to delete"
                },
                "force": {
                    "type": "boolean",
                    "description": "Force delete even if running (default: false)"
                },
                "purge": {
                    "type": "boolean",
                    "description": "Also purge firewall rules and backup jobs (default: true)"
                }
            },
            "required": ["vmid"]
        })
    }

    fn category(&self) -> &str {
        "containers"
    }

    fn tags(&self) -> Vec<String> {
        vec!["lxc".into(), "proxmox".into(), "containers".into(), "delete".into(), "write".into()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        use op_network::ProxmoxClient;

        let vmid = input
            .get("vmid")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: vmid"))? as u32;

        let force = input.get("force").and_then(|v| v.as_bool()).unwrap_or(false);

        let client = ProxmoxClient::from_env()?;

        // Check if container exists
        if !client.container_exists(vmid).await? {
            return Ok(json!({
                "success": true,
                "vmid": vmid,
                "message": format!("Container {} does not exist (already deleted?)", vmid),
                "already_deleted": true,
                "method": "native_proxmox_api"
            }));
        }

        // Stop if running (unless force delete)
        if client.is_running(vmid).await? {
            if force {
                tracing::info!("Force stopping container {} before deletion", vmid);
                let _ = client.stop_container_sync(vmid, 30).await;
            } else {
                return Err(anyhow::anyhow!(
                    "Container {} is running. Stop it first or use force=true",
                    vmid
                ));
            }
        }

        let upid = if force {
            client.force_delete_container(vmid).await?
        } else {
            client.delete_container(vmid).await?
        };

        let task_result = client.wait_for_task(&upid, 120).await?;

        // Verify container is deleted
        let exists = client.container_exists(vmid).await?;

        if !exists {
            Ok(json!({
                "success": true,
                "vmid": vmid,
                "task_id": upid,
                "task_status": task_result.status,
                "message": format!("Container {} deleted successfully", vmid),
                "verification": "Container no longer exists",
                "method": "native_proxmox_api"
            }))
        } else {
            Err(anyhow::anyhow!(
                "Delete command succeeded but container {} still exists",
                vmid
            ))
        }
    }
}

/// Tool to clone a container
pub struct LxcCloneContainerTool;

#[async_trait]
impl Tool for LxcCloneContainerTool {
    fn name(&self) -> &str {
        "lxc_clone_container"
    }

    fn description(&self) -> &str {
        "Clone an existing LXC container to create a new one. Supports linked clones (fast, shared storage) or full clones (independent copy)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source_vmid": {
                    "type": "integer",
                    "description": "Source container VM ID to clone from"
                },
                "target_vmid": {
                    "type": "integer",
                    "description": "Target VM ID for the new container"
                },
                "hostname": {
                    "type": "string",
                    "description": "Hostname for the cloned container"
                },
                "full_clone": {
                    "type": "boolean",
                    "description": "Create a full independent clone (default: false for linked clone)"
                }
            },
            "required": ["source_vmid", "target_vmid"]
        })
    }

    fn category(&self) -> &str {
        "containers"
    }

    fn tags(&self) -> Vec<String> {
        vec!["lxc".into(), "proxmox".into(), "containers".into(), "clone".into(), "write".into()]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        use op_network::ProxmoxClient;

        let source_vmid = input
            .get("source_vmid")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: source_vmid"))? as u32;

        let target_vmid = input
            .get("target_vmid")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing required argument: target_vmid"))? as u32;

        let hostname = input.get("hostname").and_then(|v| v.as_str());
        let full_clone = input.get("full_clone").and_then(|v| v.as_bool()).unwrap_or(false);

        let client = ProxmoxClient::from_env()?;

        // Check source exists
        if !client.container_exists(source_vmid).await? {
            return Err(anyhow::anyhow!("Source container {} does not exist", source_vmid));
        }

        // Check target doesn't exist
        if client.container_exists(target_vmid).await? {
            return Err(anyhow::anyhow!("Target container {} already exists", target_vmid));
        }

        let upid = client
            .clone_container(source_vmid, target_vmid, hostname, full_clone)
            .await?;

        let task_result = client.wait_for_task(&upid, 600).await?;

        // Verify clone was created
        let exists = client.container_exists(target_vmid).await?;

        if exists {
            Ok(json!({
                "success": true,
                "source_vmid": source_vmid,
                "target_vmid": target_vmid,
                "hostname": hostname,
                "clone_type": if full_clone { "full" } else { "linked" },
                "task_id": upid,
                "task_status": task_result.status,
                "message": format!("Container {} cloned to {} successfully", source_vmid, target_vmid),
                "verification": "Cloned container exists",
                "method": "native_proxmox_api"
            }))
        } else {
            Err(anyhow::anyhow!(
                "Clone command succeeded but container {} not found",
                target_vmid
            ))
        }
    }
}

/// Register all LXC tools with the registry
pub async fn register_lxc_tools(registry: &ToolRegistry) -> Result<()> {
    registry.register_tool(Arc::new(LxcCheckAvailableTool)).await?;
    registry.register_tool(Arc::new(LxcListContainersTool)).await?;
    registry.register_tool(Arc::new(LxcGetContainerTool)).await?;
    registry.register_tool(Arc::new(LxcCreateContainerTool)).await?;
    registry.register_tool(Arc::new(LxcStartContainerTool)).await?;
    registry.register_tool(Arc::new(LxcStopContainerTool)).await?;
    registry.register_tool(Arc::new(LxcDeleteContainerTool)).await?;
    registry.register_tool(Arc::new(LxcCloneContainerTool)).await?;
    Ok(())
}

/// Create all LXC tools as a vector
pub fn create_lxc_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(LxcCheckAvailableTool),
        Arc::new(LxcListContainersTool),
        Arc::new(LxcGetContainerTool),
        Arc::new(LxcCreateContainerTool),
        Arc::new(LxcStartContainerTool),
        Arc::new(LxcStopContainerTool),
        Arc::new(LxcDeleteContainerTool),
        Arc::new(LxcCloneContainerTool),
    ]
}
