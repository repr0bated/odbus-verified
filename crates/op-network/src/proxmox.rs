//! Native Proxmox API Client
//!
//! Provides native REST API access to Proxmox VE for LXC container management.
//! This replaces shelling out to `pct` commands with direct API calls.
//!
//! ## Authentication
//!
//! The client supports API token authentication. Create a token file at
//! `/etc/op-dbus/pve-token` or set `PVE_TOKEN_FILE` environment variable:
//!
//! ```text
//! PVE_API_USER=root@pam
//! PVE_API_TOKEN_ID=op-dbus
//! PVE_API_TOKEN_SECRET=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
//! PVE_API_NODE=proxmox
//! ```

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Proxmox API client for LXC container management
pub struct ProxmoxClient {
    client: Client,
    base_url: String,
    node: String,
    token: Option<ProxmoxToken>,
}

/// API token for Proxmox authentication
#[derive(Clone, Debug)]
pub struct ProxmoxToken {
    /// User identifier (e.g., "root@pam")
    pub user: String,
    /// Token ID (e.g., "op-dbus")
    pub token_id: String,
    /// Token secret value
    pub secret: String,
}

impl ProxmoxToken {
    /// Format the authorization header value
    pub fn to_auth_header(&self) -> String {
        format!(
            "PVEAPIToken={}!{}={}",
            self.user, self.token_id, self.secret
        )
    }
}

/// LXC container information from Proxmox API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LxcContainer {
    /// Container VM ID
    pub vmid: u32,
    /// Container name/hostname
    #[serde(default)]
    pub name: Option<String>,
    /// Container status (running, stopped, etc.)
    pub status: String,
    /// CPU usage (if available)
    #[serde(default)]
    pub cpu: Option<f64>,
    /// Memory usage in bytes (if available)
    #[serde(default)]
    pub mem: Option<u64>,
    /// Maximum memory in bytes (if available)
    #[serde(default)]
    pub maxmem: Option<u64>,
    /// Disk usage in bytes (if available)
    #[serde(default)]
    pub disk: Option<u64>,
    /// Maximum disk in bytes (if available)
    #[serde(default)]
    pub maxdisk: Option<u64>,
    /// Uptime in seconds (if available)
    #[serde(default)]
    pub uptime: Option<u64>,
    /// Additional fields
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Request to create a new LXC container
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreateContainerRequest {
    /// Container VM ID
    pub vmid: u32,
    /// OS template (e.g., "local:vztmpl/debian-13-standard_13.1-2_amd64.tar.zst")
    pub ostemplate: String,
    /// Hostname
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    /// Memory in MB
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<u32>,
    /// Swap in MB
    #[serde(skip_serializing_if = "Option::is_none")]
    pub swap: Option<u32>,
    /// Number of CPU cores
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cores: Option<u32>,
    /// Root filesystem specification (e.g., "local-btrfs:8")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rootfs: Option<String>,
    /// Network configuration (e.g., "name=eth0,bridge=vmbr0")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub net0: Option<String>,
    /// Run as unprivileged container
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unprivileged: Option<bool>,
    /// Container features (e.g., "nesting=1")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<String>,
    /// Start container after creation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<bool>,
    /// Start on boot
    #[serde(skip_serializing_if = "Option::is_none")]
    pub onboot: Option<bool>,
    /// Protect container from deletion
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protection: Option<bool>,
    /// DNS server
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nameserver: Option<String>,
    /// DNS search domain
    #[serde(skip_serializing_if = "Option::is_none")]
    pub searchdomain: Option<String>,
    /// Password for root user
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// SSH public keys
    #[serde(rename = "ssh-public-keys", skip_serializing_if = "Option::is_none")]
    pub ssh_public_keys: Option<String>,
    /// Storage backend (e.g., "local-btrfs")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage: Option<String>,
}

/// Container status response from Proxmox API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStatus {
    /// Container status (running, stopped, etc.)
    pub status: String,
    /// VM ID
    pub vmid: u32,
    /// Container name
    #[serde(default)]
    pub name: Option<String>,
    /// CPU usage
    #[serde(default)]
    pub cpu: Option<f64>,
    /// Memory usage in bytes
    #[serde(default)]
    pub mem: Option<u64>,
    /// Maximum memory in bytes
    #[serde(default)]
    pub maxmem: Option<u64>,
    /// Disk read bytes
    #[serde(default)]
    pub diskread: Option<u64>,
    /// Disk write bytes
    #[serde(default)]
    pub diskwrite: Option<u64>,
    /// Network in bytes
    #[serde(default)]
    pub netin: Option<u64>,
    /// Network out bytes
    #[serde(default)]
    pub netout: Option<u64>,
    /// Uptime in seconds
    #[serde(default)]
    pub uptime: Option<u64>,
    /// PID of main process
    #[serde(default)]
    pub pid: Option<u32>,
    /// HA state
    #[serde(default)]
    pub ha: Option<HashMap<String, serde_json::Value>>,
    /// Additional fields
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Proxmox API response wrapper
#[derive(Debug, Clone, Deserialize)]
pub struct ProxmoxResponse<T> {
    pub data: T,
}

/// Task status response
#[derive(Debug, Clone, Deserialize)]
pub struct TaskStatus {
    pub status: String,
    #[serde(default)]
    pub exitstatus: Option<String>,
    #[serde(default)]
    pub node: Option<String>,
    #[serde(rename = "type", default)]
    pub task_type: Option<String>,
    #[serde(default)]
    pub upid: Option<String>,
}

/// Proxmox version info
#[derive(Debug, Clone, Deserialize)]
pub struct ProxmoxVersion {
    pub version: String,
    pub release: String,
    #[serde(default)]
    pub repoid: Option<String>,
}

impl ProxmoxClient {
    /// Create a new client with default settings
    pub fn new() -> Self {
        Self::with_config("https://localhost:8006", "localhost", None)
    }

    /// Create a client with custom configuration
    pub fn with_config(base_url: &str, node: &str, token: Option<ProxmoxToken>) -> Self {
        // Create client that accepts self-signed certificates (Proxmox default)
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            node: node.to_string(),
            token,
        }
    }

    /// Create a client from environment/config file
    pub fn from_env() -> Result<Self> {
        let token_file = std::env::var("PVE_TOKEN_FILE")
            .unwrap_or_else(|_| "/etc/op-dbus/pve-token".to_string());

        // Try to read token from file
        let (token, node) = if let Ok(content) = std::fs::read_to_string(&token_file) {
            let mut user = None;
            let mut token_id = None;
            let mut secret = None;
            let mut node = "localhost".to_string();

            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }

                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    let value = value.trim().trim_matches('"').trim_matches('\'');

                    match key {
                        "PVE_API_USER" => user = Some(value.to_string()),
                        "PVE_API_TOKEN_ID" => token_id = Some(value.to_string()),
                        "PVE_API_TOKEN_SECRET" => secret = Some(value.to_string()),
                        "PVE_API_NODE" => node = value.to_string(),
                        _ => {}
                    }
                }
            }

            let token = match (user, token_id, secret) {
                (Some(user), Some(token_id), Some(secret)) => Some(ProxmoxToken {
                    user,
                    token_id,
                    secret,
                }),
                _ => {
                    warn!("Incomplete token configuration in {}", token_file);
                    None
                }
            };

            (token, node)
        } else {
            debug!("Token file not found: {}", token_file);
            (None, "localhost".to_string())
        };

        // Check for base URL override
        let base_url =
            std::env::var("PVE_API_URL").unwrap_or_else(|_| "https://localhost:8006".to_string());

        Ok(Self::with_config(&base_url, &node, token))
    }

    /// Build the authorization header if token is configured
    fn auth_header(&self) -> Option<String> {
        self.token.as_ref().map(|t| t.to_auth_header())
    }

    /// Make a GET request to the Proxmox API
    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        debug!("GET {}", url);

        let mut req = self.client.get(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }

        let resp = req.send().await.context("Failed to send request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("API request failed: {} - {}", status, body));
        }

        let response: ProxmoxResponse<T> = resp.json().await.context("Failed to parse response")?;
        Ok(response.data)
    }

    /// Make a POST request to the Proxmox API
    async fn post<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<R> {
        let url = format!("{}{}", self.base_url, path);
        debug!("POST {}", url);

        let mut req = self.client.post(&url).form(body);
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }

        let resp = req.send().await.context("Failed to send request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("API request failed: {} - {}", status, body));
        }

        let response: ProxmoxResponse<R> = resp.json().await.context("Failed to parse response")?;
        Ok(response.data)
    }

    /// Make a DELETE request to the Proxmox API
    async fn delete<R: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<R> {
        let url = format!("{}{}", self.base_url, path);
        debug!("DELETE {}", url);

        let mut req = self.client.delete(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }

        let resp = req.send().await.context("Failed to send request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("API request failed: {} - {}", status, body));
        }

        let response: ProxmoxResponse<R> = resp.json().await.context("Failed to parse response")?;
        Ok(response.data)
    }

    // =========================================================================
    // Public API Methods
    // =========================================================================

    /// Check if Proxmox API is available
    pub async fn check_available(&self) -> Result<ProxmoxVersion> {
        self.get("/api2/json/version").await
    }

    /// List all LXC containers on the node
    pub async fn list_containers(&self) -> Result<Vec<LxcContainer>> {
        let path = format!("/api2/json/nodes/{}/lxc", self.node);
        self.get(&path).await
    }

    /// Get detailed status of a specific container
    pub async fn get_container(&self, vmid: u32) -> Result<ContainerStatus> {
        let path = format!("/api2/json/nodes/{}/lxc/{}/status/current", self.node, vmid);
        self.get(&path).await
    }

    /// Get container configuration
    pub async fn get_container_config(
        &self,
        vmid: u32,
    ) -> Result<HashMap<String, serde_json::Value>> {
        let path = format!("/api2/json/nodes/{}/lxc/{}/config", self.node, vmid);
        self.get(&path).await
    }

    /// Create a new LXC container
    ///
    /// Returns the task UPID for tracking the creation progress
    pub async fn create_container(&self, config: &CreateContainerRequest) -> Result<String> {
        let path = format!("/api2/json/nodes/{}/lxc", self.node);
        info!(
            "Creating container {} with hostname {:?}",
            config.vmid, config.hostname
        );
        self.post(&path, config).await
    }

    /// Start a container
    ///
    /// Returns the task UPID
    pub async fn start_container(&self, vmid: u32) -> Result<String> {
        let path = format!("/api2/json/nodes/{}/lxc/{}/status/start", self.node, vmid);
        info!("Starting container {}", vmid);
        self.post::<(), String>(&path, &()).await
    }

    /// Stop a container
    ///
    /// Returns the task UPID
    pub async fn stop_container(&self, vmid: u32) -> Result<String> {
        let path = format!("/api2/json/nodes/{}/lxc/{}/status/stop", self.node, vmid);
        info!("Stopping container {}", vmid);
        self.post::<(), String>(&path, &()).await
    }

    /// Shutdown a container gracefully
    ///
    /// Returns the task UPID
    pub async fn shutdown_container(&self, vmid: u32, timeout: Option<u32>) -> Result<String> {
        let path = format!(
            "/api2/json/nodes/{}/lxc/{}/status/shutdown",
            self.node, vmid
        );
        info!("Shutting down container {} (timeout: {:?})", vmid, timeout);

        #[derive(Serialize)]
        struct ShutdownParams {
            #[serde(skip_serializing_if = "Option::is_none")]
            timeout: Option<u32>,
        }

        self.post(&path, &ShutdownParams { timeout }).await
    }

    /// Delete a container
    ///
    /// The container must be stopped first.
    /// Returns the task UPID
    pub async fn delete_container(&self, vmid: u32) -> Result<String> {
        let path = format!("/api2/json/nodes/{}/lxc/{}", self.node, vmid);
        info!("Deleting container {}", vmid);
        self.delete(&path).await
    }

    /// Force stop and delete a container
    pub async fn force_delete_container(&self, vmid: u32) -> Result<String> {
        let path = format!(
            "/api2/json/nodes/{}/lxc/{}?force=1&purge=1",
            self.node, vmid
        );
        info!("Force deleting container {}", vmid);
        self.delete(&path).await
    }

    /// Get task status
    pub async fn get_task_status(&self, upid: &str) -> Result<TaskStatus> {
        let path = format!("/api2/json/nodes/{}/tasks/{}/status", self.node, upid);
        self.get(&path).await
    }

    /// Wait for a task to complete
    pub async fn wait_for_task(&self, upid: &str, timeout_secs: u64) -> Result<TaskStatus> {
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        loop {
            let status = self.get_task_status(upid).await?;

            if status.status == "stopped" {
                if let Some(ref exit) = status.exitstatus {
                    if exit != "OK" {
                        return Err(anyhow!("Task failed: {}", exit));
                    }
                }
                return Ok(status);
            }

            if start.elapsed() > timeout {
                return Err(anyhow!("Task timed out after {} seconds", timeout_secs));
            }

            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Check if container exists
    pub async fn container_exists(&self, vmid: u32) -> Result<bool> {
        match self.get_container(vmid).await {
            Ok(_) => Ok(true),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("500")
                    || msg.contains("does not exist")
                    || msg.contains("not found")
                {
                    Ok(false)
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Check if container is running
    pub async fn is_running(&self, vmid: u32) -> Result<bool> {
        let status = self.get_container(vmid).await?;
        Ok(status.status == "running")
    }

    /// Clone a container
    pub async fn clone_container(
        &self,
        source_vmid: u32,
        target_vmid: u32,
        hostname: Option<&str>,
        full_clone: bool,
    ) -> Result<String> {
        let path = format!("/api2/json/nodes/{}/lxc/{}/clone", self.node, source_vmid);
        info!(
            "Cloning container {} to {} (full: {})",
            source_vmid, target_vmid, full_clone
        );

        #[derive(Serialize)]
        struct CloneParams<'a> {
            newid: u32,
            #[serde(skip_serializing_if = "Option::is_none")]
            hostname: Option<&'a str>,
            full: bool,
        }

        self.post(
            &path,
            &CloneParams {
                newid: target_vmid,
                hostname,
                full: full_clone,
            },
        )
        .await
    }

    /// Create container and wait for completion
    pub async fn create_container_sync(
        &self,
        config: &CreateContainerRequest,
        timeout_secs: u64,
    ) -> Result<()> {
        let upid = self.create_container(config).await?;
        self.wait_for_task(&upid, timeout_secs).await?;
        Ok(())
    }

    /// Start container and wait for completion
    pub async fn start_container_sync(&self, vmid: u32, timeout_secs: u64) -> Result<()> {
        let upid = self.start_container(vmid).await?;
        self.wait_for_task(&upid, timeout_secs).await?;
        Ok(())
    }

    /// Stop container and wait for completion
    pub async fn stop_container_sync(&self, vmid: u32, timeout_secs: u64) -> Result<()> {
        let upid = self.stop_container(vmid).await?;
        self.wait_for_task(&upid, timeout_secs).await?;
        Ok(())
    }

    /// Delete container and wait for completion
    pub async fn delete_container_sync(&self, vmid: u32, timeout_secs: u64) -> Result<()> {
        let upid = self.delete_container(vmid).await?;
        self.wait_for_task(&upid, timeout_secs).await?;
        Ok(())
    }

    /// Get the node name
    pub fn node(&self) -> &str {
        &self.node
    }

    /// Get the base URL
    pub fn base_url(&self) -> &str {
        &self.base_url
    }
}

impl Default for ProxmoxClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_auth_header() {
        let token = ProxmoxToken {
            user: "root@pam".to_string(),
            token_id: "op-dbus".to_string(),
            secret: "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx".to_string(),
        };

        assert_eq!(
            token.to_auth_header(),
            "PVEAPIToken=root@pam!op-dbus=xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
        );
    }

    #[test]
    fn test_create_request_serialization() {
        let req = CreateContainerRequest {
            vmid: 100,
            ostemplate: "local:vztmpl/debian-13.tar.zst".to_string(),
            hostname: Some("test".to_string()),
            memory: Some(512),
            cores: Some(2),
            ..Default::default()
        };

        let json: serde_json::Value = serde_json::to_value(&req).unwrap();
        assert_eq!(json["vmid"], 100);
        assert_eq!(json["hostname"], "test");
        assert_eq!(json["memory"], 512);
        assert!(json.get("swap").is_none()); // Should be skipped when None
    }
}
