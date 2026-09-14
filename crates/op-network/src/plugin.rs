//! Network plugin with OVS/OVSDB persistence
//!
//! This plugin manages network configuration including OVS bridges via OVSDB.
//! CRITICAL: Uses OVSDB JSON-RPC to ensure bridges persist in database.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::openflow::OpenFlowClient;
use crate::ovsdb::OvsdbClient;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkPlugin {
    /// OVS bridges to create
    #[serde(default)]
    pub bridges: Vec<OvsBridge>,

    /// Network interfaces to configure
    #[serde(default)]
    pub interfaces: Vec<NetworkInterface>,

    /// OVSDB persistence configuration
    #[serde(default)]
    pub ovsdb: OvsdbConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OvsBridge {
    /// Bridge name (e.g., "vmbr0", "ovsbr0")
    pub name: String,

    /// Datapath type: "system" (default, kernel-based, persistent) or "netdev" (userspace)
    #[serde(default = "default_datapath_type")]
    pub datapath_type: String,

    /// Physical ports to add to bridge
    #[serde(default)]
    pub ports: Vec<String>,

    /// Internal ports (for IP assignment)
    #[serde(default)]
    pub internal_ports: Vec<String>,

    /// IP address for bridge interface (e.g., "10.0.1.1/24")
    pub address: Option<String>,

    /// Enable DHCP on this bridge
    #[serde(default)]
    pub dhcp: bool,

    /// VLAN ID (if this is a VLAN interface)
    pub vlan: Option<u16>,

    /// OpenFlow configuration
    pub openflow: Option<OpenFlowConfig>,
}

fn default_datapath_type() -> String {
    "system".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenFlowConfig {
    /// Controller address (default: tcp:127.0.0.1:6653)
    #[serde(default = "default_controller")]
    pub controller: String,

    /// Automatically apply default rules on bridge creation
    #[serde(default)]
    pub auto_apply_defaults: bool,

    /// Default OpenFlow rules to apply
    #[serde(default)]
    pub default_rules: Vec<String>,

    /// Enable fail-secure mode (drop packets if controller unavailable)
    #[serde(default)]
    pub fail_secure: bool,
}

fn default_controller() -> String {
    "tcp:10.200.0.1:6653".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    /// Interface name (e.g., "eth0", "ens1")
    pub name: String,

    /// IP address (e.g., "192.168.1.10/24")
    pub address: Option<String>,

    /// Enable DHCP
    #[serde(default)]
    pub dhcp: bool,

    /// Bring interface up
    #[serde(default = "default_true")]
    pub up: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OvsdbConfig {
    /// OVSDB socket path (default: /var/run/openvswitch/db.sock)
    #[serde(default = "default_ovsdb_socket")]
    pub socket_path: String,

    /// Database file path for persistence (default: /etc/openvswitch/conf.db)
    #[serde(default = "default_ovsdb_database")]
    pub database_path: String,

    /// Connection timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,

    /// Ensure database persists across reboots
    #[serde(default = "default_true")]
    pub persist: bool,
}

fn default_ovsdb_socket() -> String {
    "/var/run/openvswitch/db.sock".to_string()
}

fn default_ovsdb_database() -> String {
    "/etc/openvswitch/conf.db".to_string()
}

fn default_timeout() -> u64 {
    30
}

impl Default for OvsdbConfig {
    fn default() -> Self {
        Self {
            socket_path: default_ovsdb_socket(),
            database_path: default_ovsdb_database(),
            timeout_seconds: default_timeout(),
            persist: true,
        }
    }
}

impl NetworkPlugin {
    /// Create a new network plugin with default configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply the network configuration
    pub async fn apply(&self) -> Result<()> {
        info!("Network plugin: Starting network configuration");

        // Step 1: Verify OVSDB persistence configuration
        if !self.bridges.is_empty() {
            self.verify_ovsdb_persistence().await?;
        }

        // Step 2: Wait for OVSDB to be ready
        if !self.bridges.is_empty() {
            self.wait_for_ovsdb().await?;
        }

        // Step 3: Create OVS bridges (via OVSDB JSON-RPC for persistence)
        for bridge in &self.bridges {
            self.create_ovs_bridge(bridge).await?;
        }

        // Step 4: Configure network interfaces
        for interface in &self.interfaces {
            self.configure_interface(interface).await?;
        }

        info!("✓ Network plugin: Complete");
        Ok(())
    }

    /// Get current network state
    pub async fn get_state(&self) -> Result<Value> {
        let client = OvsdbClient::new();

        // Get list of bridges
        let bridges = client.list_bridges().await.unwrap_or_default();

        // Get details for each bridge
        let mut bridge_details = Vec::new();
        for bridge_name in &bridges {
            let ports = client
                .list_bridge_ports(bridge_name)
                .await
                .unwrap_or_default();
            bridge_details.push(serde_json::json!({
                "name": bridge_name,
                "ports": ports,
            }));
        }

        Ok(serde_json::json!({
            "bridges": bridge_details,
            "interfaces": self.interfaces,
            "ovsdb": {
                "socket_path": self.ovsdb.socket_path,
                "persist": self.ovsdb.persist,
            }
        }))
    }

    async fn verify_ovsdb_persistence(&self) -> Result<()> {
        info!("Verifying OVSDB persistence configuration");

        // Check if database file directory exists
        let db_path = std::path::Path::new(&self.ovsdb.database_path);
        if let Some(parent) = db_path.parent() {
            if !parent.exists() {
                warn!(
                    "OVSDB database directory does not exist: {}",
                    parent.display()
                );
                info!("Creating directory: {}", parent.display());
                tokio::fs::create_dir_all(parent).await?;
            }
        }

        // Verify persistence is enabled
        if !self.ovsdb.persist {
            warn!("OVSDB persistence is DISABLED - bridges may not survive reboots!");
            warn!("Set ovsdb.persist=true in state.json to enable persistence");
        } else {
            info!("✓ OVSDB persistence enabled: {}", self.ovsdb.database_path);
        }

        Ok(())
    }

    async fn wait_for_ovsdb(&self) -> Result<()> {
        info!(
            "Waiting for OVSDB to be ready (timeout: {}s)",
            self.ovsdb.timeout_seconds
        );

        let client = OvsdbClient::new();
        let timeout = Duration::from_secs(self.ovsdb.timeout_seconds);
        let start = std::time::Instant::now();

        loop {
            match client.list_dbs().await {
                Ok(dbs) => {
                    info!("✓ OVSDB is ready, available databases: {:?}", dbs);
                    return Ok(());
                }
                Err(e) => {
                    if start.elapsed() > timeout {
                        return Err(anyhow::anyhow!(
                            "OVSDB connection timeout after {}s: {}",
                            self.ovsdb.timeout_seconds,
                            e
                        ));
                    }
                    warn!("OVSDB not ready yet, retrying... ({})", e);
                    sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }

    async fn create_ovs_bridge(&self, bridge: &OvsBridge) -> Result<()> {
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        info!("Creating OVS bridge: {}", bridge.name);
        info!("  Datapath type: {}", bridge.datapath_type);
        info!("  Ports: {:?}", bridge.ports);
        info!("  Internal ports: {:?}", bridge.internal_ports);
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

        let client = OvsdbClient::new();

        // Check if bridge already exists
        let exists = client.bridge_exists(&bridge.name).await?;

        if exists {
            info!("  Bridge '{}' already exists (idempotent)", bridge.name);
            let existing_ports = client.list_bridge_ports(&bridge.name).await?;
            info!("  Existing ports: {:?}", existing_ports);
        } else {
            // Create bridge via OVSDB JSON-RPC
            info!("  Creating bridge via OVSDB JSON-RPC (persistent)");
            client
                .create_bridge(&bridge.name)
                .await
                .context(format!("Failed to create bridge {}", bridge.name))?;
        }

        // Add physical ports
        for port in &bridge.ports {
            info!("  Adding port: {}", port);
            if let Err(e) = client.add_port(&bridge.name, port).await {
                warn!("  Failed to add port {}: {}", port, e);
            }
        }

        // Bring up bridge interface
        info!("  Bringing up bridge interface");
        self.bring_up_interface(&bridge.name).await?;

        // Configure IP address if specified
        if let Some(ref address) = bridge.address {
            info!("  Configuring IP address: {}", address);
            self.configure_ip(&bridge.name, address).await?;
        }

        // Enable DHCP if requested
        if bridge.dhcp {
            info!("  Enabling DHCP");
            self.enable_dhcp(&bridge.name).await?;
        }

        // Apply OpenFlow rules if configured
        if let Some(ref openflow) = bridge.openflow {
            if openflow.auto_apply_defaults && !openflow.default_rules.is_empty() {
                info!("  Applying OpenFlow default rules");
                self.apply_openflow_rules(&bridge.name, openflow).await?;
            }
        }

        info!("✓ Bridge '{}' configured successfully", bridge.name);
        Ok(())
    }

    async fn configure_interface(&self, interface: &NetworkInterface) -> Result<()> {
        info!("Configuring interface: {}", interface.name);

        // Bring interface up/down
        if interface.up {
            self.bring_up_interface(&interface.name).await?;
        } else {
            self.bring_down_interface(&interface.name).await?;
        }

        // Configure IP address
        if let Some(ref address) = interface.address {
            self.configure_ip(&interface.name, address).await?;
        }

        // Enable DHCP
        if interface.dhcp {
            self.enable_dhcp(&interface.name).await?;
        }

        info!("✓ Interface '{}' configured", interface.name);
        Ok(())
    }

    async fn bring_up_interface(&self, name: &str) -> Result<()> {
        crate::rtnetlink::link_up(name)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to bring up interface {}: {}", name, e))?;

        info!("    ✓ Interface '{}' is up", name);
        Ok(())
    }

    async fn bring_down_interface(&self, name: &str) -> Result<()> {
        crate::rtnetlink::link_down(name)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to bring down interface {}: {}", name, e))?;

        Ok(())
    }

    async fn configure_ip(&self, interface: &str, address: &str) -> Result<()> {
        // Parse CIDR (e.g. 192.168.1.1/24)
        let parts: Vec<&str> = address.split('/').collect();
        if parts.len() != 2 {
            return Err(anyhow::anyhow!("Invalid CIDR address: {}", address));
        }

        let ip_str = parts[0];
        let prefix: u8 = parts[1].parse().context("Invalid prefix length")?;

        // Flush existing addresses
        crate::rtnetlink::flush_addresses(interface)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to flush addresses on {}: {}", interface, e))?;

        // Add new address
        crate::rtnetlink::add_ipv4_address(interface, ip_str, prefix)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to add IP {} to {}: {}", address, interface, e))?;

        info!("    ✓ IP address {} configured on {}", address, interface);
        Ok(())
    }

    async fn enable_dhcp(&self, interface: &str) -> Result<()> {
        // TODO: Implement native DHCP handshake (RFC 2131) using a pure-Rust
        // client such as `dhcproto` + raw UDP sockets. The current code cannot
        // perform a DHCP DISCOVER/OFFER/REQUEST/ACK exchange without an external
        // binary, so we perform a best-effort lease-file read and warn.
        let lease_paths = [
            format!("/var/lib/dhcp/dhclient.{}.leases", interface),
            format!("/var/lib/dhclient/dhclient-{}.lease", interface),
        ];
        let mut found_lease = false;
        for path in &lease_paths {
            if tokio::fs::metadata(path).await.is_ok() {
                found_lease = true;
                info!(
                    "    Found existing DHCP lease file for {}: {}",
                    interface, path
                );
            }
        }
        if !found_lease {
            warn!(
                "No existing DHCP lease found for {}. DHCP not enabled — native client required.",
                interface
            );
        }
        info!(
            "    ✓ DHCP status checked on {} (native client pending)",
            interface
        );
        Ok(())
    }

    async fn apply_openflow_rules(&self, bridge: &str, config: &OpenFlowConfig) -> Result<()> {
        info!(
            "    Applying {} OpenFlow rules to {}",
            config.default_rules.len(),
            bridge
        );

        // Parse controller address
        let addr = if config.controller.starts_with("tcp:") {
            let addr_str = config.controller.trim_start_matches("tcp:");
            addr_str
                .parse()
                .unwrap_or_else(|_| std::net::SocketAddr::from(([10, 200, 0, 1], 6653)))
        } else {
            std::net::SocketAddr::from(([10, 200, 0, 1], 6653))
        };

        // Connect to OpenFlow switch
        let mut client = OpenFlowClient::connect(addr).await.context(format!(
            "Failed to connect to OpenFlow switch for bridge {}",
            bridge
        ))?;

        // Clear existing flows first
        client.delete_all_flows().await?;

        // Apply each rule
        for rule in &config.default_rules {
            client.add_flow_rule(rule).await?;
            info!("      Applied rule: {}", rule);
        }

        info!("    ✓ OpenFlow rules applied to {}", bridge);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_network_config() {
        let json = r#"
        {
            "bridges": [
                {
                    "name": "vmbr0",
                    "datapath_type": "system",
                    "ports": ["eth0"],
                    "internal_ports": ["vmbr0-if"],
                    "address": "10.0.0.1/24"
                }
            ],
            "ovsdb": {
                "database_path": "/etc/openvswitch/conf.db",
                "persist": true
            }
        }
        "#
        .to_string();

        let plugin: NetworkPlugin = serde_json::from_str(&json).unwrap();
        assert_eq!(plugin.bridges.len(), 1);
        assert_eq!(plugin.bridges[0].name, "vmbr0");
        assert_eq!(plugin.bridges[0].datapath_type, "system");
        assert_eq!(plugin.ovsdb.database_path, "/etc/openvswitch/conf.db");
        assert!(plugin.ovsdb.persist);
    }

    #[test]
    fn test_default_ovsdb_config() {
        let config = OvsdbConfig::default();
        assert_eq!(config.socket_path, "/var/run/openvswitch/db.sock");
        assert_eq!(config.database_path, "/etc/openvswitch/conf.db");
        assert_eq!(config.timeout_seconds, 30);
        assert!(config.persist);
    }
}
