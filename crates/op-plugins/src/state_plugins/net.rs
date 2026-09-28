// Net state plugin - authoritative OVS state management via D-Bus
// Handles: interfaces, bridges, IPs, basic connectivity via plugin schema
// Integrates with systemd-networkd as subordinate service for L3 configuration
use op_snowball::PluginFootprint;

// Use D-Bus introspection instead of CLI commands
use anyhow::{Context, Result};
use async_trait::async_trait;
use log;
use op_state::{ApplyResult, Checkpoint, PluginCapabilities, StateAction, StateDiff, StatePlugin};
use op_state_store::{CapabilityDecl, PluginSchema};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json;
use simd_json::{prelude::*, OwnedValue as Value};
use std::collections::HashMap;
// use std::net::Ipv4Addr; // not needed currently

/// Network interface management via rtnetlink.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.net.schema@v1"))]
#[schemars(extend("x-oscal-category" = "network"))]
pub struct NetworkConfig {
    /// List of network interfaces.
    #[serde(default)]
    #[schemars(
        description = "List of network interfaces",
        extend("x-oscal-subid" = "exp.service.net.interfaces.render@v1")
    )]
    pub interfaces: Vec<InterfaceConfig>,
}

/// Interface configuration with immutable identity and tunable config.
/// Pattern matches LXC plugin: immutable core + tunable properties.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InterfaceConfig {
    /// Interface name (e.g., "ovsbr0", "mesh").
    #[schemars(
        description = "Interface name",
        example = &"eth0",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "exp.service.net.interface.name.declare@v1")
    )]
    pub name: String,

    /// Interface type (e.g., "ovs-bridge", "ethernet").
    #[serde(rename = "type")]
    #[schemars(
        description = "Interface type",
        example = &"ethernet",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "exp.service.net.interface.type.declare@v1")
    )]
    pub if_type: InterfaceType,

    /// L2 driver to use (e.g., "openvswitch", "linux-bridge").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "L2 driver to use",
        extend("x-oscal-subid" = "exp.service.net.interface.driver.declare@v1")
    )]
    pub driver: Option<String>,

    /// All tunable configuration in a single object.
    #[serde(flatten)]
    pub tunable: TunableConfig,
}

/// Tunable configuration - can be changed, each change tracked in snowball.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
pub struct TunableConfig {
    /// Ports attached to this interface.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Ports attached to this interface",
        extend("x-oscal-subid" = "exp.service.net.interface.ports.declare@v1")
    )]
    pub ports: Option<Vec<String>>,

    /// L3 driver for IP configuration (e.g., "rtnetlink", "ovs-rpc", "systemd-networkd").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "L3 driver for IP configuration",
        extend("x-oscal-subid" = "exp.service.net.interface.l3-driver.declare@v1")
    )]
    pub l3_driver: Option<String>,

    /// IPv4 configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "IPv4 configuration",
        extend("x-oscal-subid" = "exp.service.net.interface.ipv4.configure@v1")
    )]
    pub ipv4: Option<Ipv4Config>,

    /// IPv6 configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "IPv6 configuration",
        extend("x-oscal-subid" = "exp.service.net.interface.ipv6.configure@v1")
    )]
    pub ipv6: Option<Ipv6Config>,

    /// SDN controller (for OpenFlow).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "SDN controller (for OpenFlow)",
        extend("x-oscal-subid" = "exp.service.net.interface.controller.declare@v1")
    )]
    pub controller: Option<String>,

    /// Dynamic properties - introspection captures ALL hardware properties here.
    /// Examples: mtu, mac_addresses (array), speed, duplex, txqueuelen, etc.
    ///
    /// APPEND-ONLY: Field names are permanent once added (by introspection or user).
    /// Values are mutable (ledger tracks all changes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        with = "serde_json::Value",
        description = "Dynamic properties captured by introspection",
        extend("x-oscal-subid" = "exp.service.net.interface.properties.collect@v1")
    )]
    pub properties: Option<HashMap<String, Value>>,

    /// Property schema - tracks which fields exist (append-only set).
    /// Used for validation: new fields can be added, existing fields cannot be removed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Property schema - tracks which fields exist",
        extend("x-oscal-subid" = "exp.service.net.interface.property-schema.declare@v1")
    )]
    pub property_schema: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum InterfaceType {
    Ethernet,
    OvsBridge,
    OvsPort,
    Bridge,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
pub struct Ipv4Config {
    /// Whether IPv4 is enabled.
    #[serde(default)]
    #[schemars(
        description = "Whether IPv4 is enabled",
        extend("x-oscal-subid" = "exp.service.net.ipv4.enabled.declare@v1")
    )]
    pub enabled: bool,
    /// Whether DHCP is enabled for IPv4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Whether DHCP is enabled for IPv4",
        extend("x-oscal-subid" = "exp.service.net.ipv4.dhcp.declare@v1")
    )]
    pub dhcp: Option<bool>,
    /// Static IPv4 addresses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Static IPv4 addresses",
        extend("x-oscal-subid" = "exp.service.net.ipv4.address.declare@v1")
    )]
    pub address: Option<Vec<AddressConfig>>,
    /// IPv4 default gateway.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "IPv4 default gateway",
        example = &"192.168.1.1",
        extend("x-oscal-subid" = "exp.service.net.ipv4.gateway.declare@v1")
    )]
    pub gateway: Option<String>,
    /// IPv4 DNS servers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "IPv4 DNS servers",
        example = &["1.1.1.1", "8.8.8.8"],
        extend("x-oscal-subid" = "exp.service.net.ipv4.dns.declare@v1")
    )]
    pub dns: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
pub struct Ipv6Config {
    /// Whether IPv6 is enabled.
    #[serde(default)]
    #[schemars(
        description = "Whether IPv6 is enabled",
        extend("x-oscal-subid" = "exp.service.net.ipv6.enabled.declare@v1")
    )]
    pub enabled: bool,
    /// Whether DHCP is enabled for IPv6.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Whether DHCP is enabled for IPv6",
        extend("x-oscal-subid" = "exp.service.net.ipv6.dhcp.declare@v1")
    )]
    pub dhcp: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddressConfig {
    /// IP address.
    #[schemars(
        description = "IP address",
        example = &"192.168.1.100",
        extend("x-oscal-subid" = "exp.service.net.address.ip.declare@v1")
    )]
    pub ip: String,
    /// CIDR prefix length.
    #[schemars(
        description = "CIDR prefix length",
        example = 24,
        extend("x-oscal-subid" = "exp.service.net.address.prefix.declare@v1")
    )]
    pub prefix: u8,
}

/// Net state plugin implementation - authoritative OVS state via D-Bus
pub struct NetStatePlugin {
    #[allow(dead_code)]
    snowball_sender: Option<tokio::sync::mpsc::UnboundedSender<PluginFootprint>>,
}

#[allow(dead_code)]
impl NetStatePlugin {
    pub fn new() -> Self {
        Self {
            snowball_sender: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_snowball_sender(
        snowball_sender: tokio::sync::mpsc::UnboundedSender<PluginFootprint>,
    ) -> Self {
        Self {
            snowball_sender: Some(snowball_sender),
        }
    }

    /// Validate interface configuration
    pub fn validate_interface_config(&self, _config: &InterfaceConfig) -> Result<()> {
        // TODO: Implement validation logic
        Ok(())
    }

    /// Check if OVS is available via D-Bus daemon
    pub async fn check_ovs_available(&self) -> Result<bool> {
        let client = op_network::rovs_proxy::OvsdbDbusClient::new();
        match client.list_dbs().await {
            Ok(_) => Ok(true),
            Err(_) => {
                log::info!("OVSDB not reachable via D-Bus daemon - skipping OVS operations");
                Ok(false)
            }
        }
    }

    /// Query current network state via D-Bus (OVS bridges only)
    pub async fn query_current_state_dbus(&self) -> Result<NetworkConfig> {
        let mut network_interfaces = Vec::new();

        // Query OVS bridges via D-Bus
        let ovs_bridges = self.query_ovs_bridges().await?;
        network_interfaces.extend(ovs_bridges);

        Ok(NetworkConfig {
            interfaces: network_interfaces,
        })
    }

    /// Parse IPv4 configuration from ip addr show output
    fn parse_ipv4_config(output: &str) -> Option<Ipv4Config> {
        let mut ipv4_config = Ipv4Config {
            enabled: false,
            dhcp: None,
            address: Some(Vec::new()),
            gateway: None,
            dns: Some(Vec::new()),
        };

        let mut found_ipv4 = false;

        for line in output.lines() {
            let line = line.trim();

            // Look for inet lines (IPv4 addresses)
            if line.starts_with("inet ") {
                found_ipv4 = true;
                ipv4_config.enabled = true;

                // Parse inet 192.168.1.100/24 brd 192.168.1.255 scope global dynamic ens1
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let addr_part = parts[1]; // e.g., "192.168.1.100/24"
                    if let Some((ip, prefix)) = Self::parse_cidr(addr_part) {
                        if let Some(ref mut addresses) = ipv4_config.address {
                            addresses.push(AddressConfig {
                                ip,
                                prefix: prefix as u8,
                            });
                        }
                    }
                }
            }
        }

        if found_ipv4 {
            Some(ipv4_config)
        } else {
            None
        }
    }

    /// Parse CIDR notation like "192.168.1.100/24" into (ip, prefix)
    fn parse_cidr(cidr: &str) -> Option<(String, u32)> {
        let parts: Vec<&str> = cidr.split('/').collect();
        if parts.len() == 2 {
            if let Ok(prefix) = parts[1].parse::<u32>() {
                return Some((parts[0].to_string(), prefix));
            }
        }
        None
    }

    /// Query OVS bridges via D-Bus daemon
    pub async fn query_ovs_bridges(&self) -> Result<Vec<InterfaceConfig>> {
        let client = op_network::rovs_proxy::OvsdbDbusClient::new();

        if client.list_dbs().await.is_err() {
            log::info!("OVSDB not reachable via D-Bus daemon - skipping OVS operations");
            return Ok(Vec::new());
        }

        let mut bridges = Vec::new();

        let bridge_names = match client.list_bridges().await {
            Ok(names) => names,
            Err(_) => {
                log::info!("Failed to list OVS bridges via D-Bus daemon");
                return Ok(Vec::new());
            }
        };

        for bridge_name in bridge_names {
            // Get bridge information via JSON-RPC
            let bridge_info_json = match client.get_bridge_info(&bridge_name).await {
                Ok(info) => info,
                Err(_) => {
                    log::debug!("Failed to get info for bridge: {}", bridge_name);
                    continue;
                }
            };

            // Parse JSON string to HashMap
            let mut bridge_info: HashMap<String, Value> =
                match serde_json::from_str::<HashMap<String, Value>>(&bridge_info_json) {
                    Ok(info) => info,
                    Err(_) => {
                        log::debug!("Failed to parse bridge info JSON for: {}", bridge_name);
                        continue;
                    }
                };

            // Enrich with routing info (via rtnetlink) for this bridge
            if let Ok(routes) = op_network::rtnetlink::list_routes_for_interface(&bridge_name).await
            {
                bridge_info.insert(
                    "routing".to_string(),
                    simd_json::json!({
                        "ipv4_routes": routes
                    }),
                );
            }

            // Get ports for this bridge via JSON-RPC
            let ports = match client.list_bridge_ports(&bridge_name).await {
                Ok(ports) => Some(ports),
                Err(_) => {
                    log::debug!("Failed to get ports for bridge: {}", bridge_name);
                    None
                }
            };

            // Derive simple role tags for ports (best-effort heuristics)
            if let Some(ref port_list) = ports {
                let mut tags: HashMap<String, String> = HashMap::new();
                for p in port_list {
                    let role = if p == "wgcf" {
                        "warp"
                    } else if p.starts_with("wg") {
                        "wireguard"
                    } else if p.starts_with("vi") {
                        // vi{VMID}
                        "container"
                    } else if p.starts_with("eth") || p.starts_with("en") {
                        "uplink"
                    } else if p == &bridge_name {
                        "internal"
                    } else {
                        "unknown"
                    };
                    tags.insert(p.clone(), role.to_string());
                }
                bridge_info.insert(
                    "port_tags".to_string(),
                    simd_json::serde::to_owned_value(tags).unwrap_or(Value::null()),
                );
            }

            bridges.push(InterfaceConfig {
                name: bridge_name,
                if_type: InterfaceType::OvsBridge,
                driver: Some("openvswitch".to_string()),
                tunable: TunableConfig {
                    ports,
                    l3_driver: None, // Bridges typically don't need L3 config
                    ipv4: None,      // OVS bridges don't have IP config directly
                    ipv6: None,
                    controller: None,
                    properties: Some(bridge_info),
                    property_schema: Some(vec!["ovsdb".to_string()]),
                },
            });
        }

        Ok(bridges)
    }

    /// Apply OVS bridge configuration via D-Bus daemon and rtnetlink
    pub async fn apply_ovs_config(&self, config: &InterfaceConfig) -> Result<()> {
        let client = op_network::rovs_proxy::OvsdbDbusClient::new();
        log::info!("Starting apply_ovs_config for {}", config.name);

        // Ensure bridge exists via D-Bus daemon
        if !client
            .bridge_exists(&config.name)
            .await
            .context("Failed to check bridge existence via D-Bus")?
        {
            client
                .create_bridge(&config.name)
                .await
                .context("Failed to create OVS bridge via D-Bus daemon")?;
            log::info!("Created OVS bridge via D-Bus daemon: {}", config.name);
        }

        // Add ports to bridge if specified via D-Bus daemon
        // Skip WireGuard interfaces - their lifecycle is owned by the decoy tunnel
        if let Some(ref ports) = config.tunable.ports {
            let current_ports = client
                .list_bridge_ports(&config.name)
                .await
                .context("Failed to list ports via D-Bus daemon")?;

            for port in ports {
                // Skip WireGuard interfaces - the tunnel owner manages them
                if port.starts_with("wg") {
                    log::info!("Skipping WireGuard interface {} (tunnel-owned)", port);
                    continue;
                }

                if !current_ports.contains(port) {
                    client.add_port(&config.name, port).await.context(format!(
                        "Failed to add port {} to bridge {} via JSON-RPC",
                        port, config.name
                    ))?;
                    log::info!("Added port {} to bridge {} via JSON-RPC", port, config.name);
                }
            }
        }

        // Update /etc/network/interfaces with bridge and IP configuration
        self.update_interfaces_file(&config.name, None, &config.tunable.ipv4)
            .await?;

        // Bring bridge up via rtnetlink (native netlink)
        if let Err(e) = op_network::rtnetlink::link_up(&config.name).await {
            log::warn!("Failed to bring bridge up via netlink: {}", e);
        }

        // Configure IPv4 if specified via rtnetlink (native netlink)
        if let Some(ref ipv4) = config.tunable.ipv4 {
            if ipv4.enabled {
                if let Some(ref addresses) = ipv4.address {
                    for addr in addresses {
                        match op_network::rtnetlink::add_ipv4_address(
                            &config.name,
                            &addr.ip,
                            addr.prefix,
                        )
                        .await
                        {
                            Ok(_) => {
                                log::info!(
                                    "Added IP {}/{} to {} via rtnetlink",
                                    addr.ip,
                                    addr.prefix,
                                    config.name
                                );
                            }
                            Err(e) => {
                                log::warn!(
                                    "Failed to add IP {} (may already exist): {}",
                                    addr.ip,
                                    e
                                );
                            }
                        }
                    }
                }

                // Configure gateway if specified via rtnetlink (native netlink)
                if let Some(ref gateway) = ipv4.gateway {
                    // Replace only the main-table default route atomically.
                    match op_network::rtnetlink::replace_default_route(&config.name, gateway).await
                    {
                        Ok(_) => {
                            log::info!(
                                "Added default route via {} on {} via rtnetlink",
                                gateway,
                                config.name
                            );
                        }
                        Err(e) => {
                            log::warn!("Failed to add default route: {}", e);
                        }
                    }
                }
            }
        }

        log::info!("Finished apply_ovs_config for {}", config.name);
        Ok(())
    }

    /// Apply OVS internal port configuration
    pub async fn apply_ovs_port_config(&self, config: &InterfaceConfig) -> Result<()> {
        log::info!("Starting apply_ovs_port_config for {}", config.name);

        // Internal ports are created as part of their parent bridge
        // This function handles IP configuration only

        // Determine L3 driver (default to rtnetlink)
        let l3_driver = config.tunable.l3_driver.as_deref().unwrap_or("rtnetlink");

        if l3_driver == "rtnetlink" {
            // Bring interface up via native rtnetlink
            if let Err(e) = op_network::rtnetlink::link_up(&config.name).await {
                log::warn!("Failed to bring port up: {}", e);
            }

            // Configure IPv4 if specified via rtnetlink
            if let Some(ref ipv4) = config.tunable.ipv4 {
                if ipv4.enabled {
                    if let Some(ref addresses) = ipv4.address {
                        for addr in addresses {
                            match op_network::rtnetlink::add_ipv4_address(
                                &config.name,
                                &addr.ip,
                                addr.prefix,
                            )
                            .await
                            {
                                Ok(_) => {
                                    log::info!(
                                        "Added IP {}/{} to {} via rtnetlink",
                                        addr.ip,
                                        addr.prefix,
                                        config.name
                                    );
                                }
                                Err(e) => {
                                    log::warn!(
                                        "Failed to add IP {} (may already exist): {}",
                                        addr.ip,
                                        e
                                    );
                                }
                            }
                        }
                    }

                    // Configure gateway if specified
                    if let Some(ref gateway) = ipv4.gateway {
                        match op_network::rtnetlink::replace_default_route(&config.name, gateway)
                            .await
                        {
                            Ok(_) => {
                                log::info!("Added default route via {} via rtnetlink", gateway);
                            }
                            Err(e) => {
                                log::warn!("Failed to add default route: {}", e);
                            }
                        }
                    }
                }
            }

            // Update /etc/network/interfaces for persistence
            self.update_interfaces_file(&config.name, None, &config.tunable.ipv4)
                .await?;
        } else {
            log::warn!("Unsupported L3 driver '{}' for {}", l3_driver, config.name);
        }

        log::info!("Finished apply_ovs_port_config for {}", config.name);
        Ok(())
    }

    /// Delete OVS bridge via D-Bus daemon
    pub async fn delete_ovs_bridge(&self, name: &str) -> Result<()> {
        let client = op_network::rovs_proxy::OvsdbDbusClient::new();

        client
            .delete_bridge(name)
            .await
            .context("Failed to delete OVS bridge via D-Bus daemon")?;

        Ok(())
    }

    /// Update /etc/network/interfaces with bridge configuration
    async fn update_interfaces_file(
        &self,
        bridge: &str,
        uplink: Option<&str>,
        ipv4: &Option<Ipv4Config>,
    ) -> Result<()> {
        let interfaces_path = std::path::Path::new("/etc/network/interfaces");
        let tag = "op-dbus-managed";
        let begin_marker = format!("# BEGIN {}\n", tag);
        let end_marker = format!("# END {}\n", tag);

        // Build the managed block
        let mut block = String::new();
        block.push_str(&begin_marker);
        block.push_str(&format!("# Managed by {}. Do not edit manually.\n\n", tag));

        // OVS Bridge with IP configuration
        // Use allow-ovs instead of auto to prevent ifupdown hang
        block.push_str(&format!("allow-ovs {}\n", bridge));
        block.push_str(&format!("iface {} inet ", bridge));

        if let Some(ipv4_cfg) = ipv4 {
            if ipv4_cfg.enabled {
                if ipv4_cfg.dhcp == Some(true) {
                    block.push_str("dhcp\n");
                } else if let Some(ref addresses) = ipv4_cfg.address {
                    if let Some(addr) = addresses.first() {
                        block.push_str("static\n");
                        block.push_str(&format!("    address {}\n", addr.ip));
                        block.push_str(&format!(
                            "    netmask {}\n",
                            Self::prefix_to_netmask(addr.prefix)
                        ));

                        if let Some(ref gateway) = ipv4_cfg.gateway {
                            block.push_str(&format!("    gateway {}\n", gateway));
                        }
                    } else {
                        block.push_str("manual\n");
                    }
                } else {
                    block.push_str("manual\n");
                }
            } else {
                block.push_str("manual\n");
            }
        } else {
            block.push_str("manual\n");
        }

        block.push_str("    ovs_type OVSBridge\n");

        // Add uplink to ovs_ports if specified
        if let Some(uplink_iface) = uplink {
            block.push_str(&format!("    ovs_ports {}\n", uplink_iface));
        }
        block.push('\n');

        // Physical uplink (if specified)
        if let Some(uplink_iface) = uplink {
            block.push_str(&format!("allow-{} {}\n", bridge, uplink_iface));
            block.push_str(&format!("iface {} inet manual\n", uplink_iface));
            block.push_str(&format!("    ovs_bridge {}\n", bridge));
            block.push_str("    ovs_type OVSPort\n");
            block.push('\n');
        }

        block.push_str(&end_marker);

        // Read current file content
        let content = tokio::fs::read_to_string(interfaces_path)
            .await
            .unwrap_or_else(|_| String::from("# network interfaces file\n"));

        // Replace or append the managed block
        let new_content = Self::replace_block(&content, &begin_marker, &end_marker, &block);

        // Write back if changed
        if new_content != content {
            tokio::fs::write(interfaces_path, new_content)
                .await
                .context("Failed to write /etc/network/interfaces")?;
            log::info!("Updated /etc/network/interfaces");
        }

        Ok(())
    }

    /// Convert CIDR prefix to netmask string
    fn prefix_to_netmask(prefix: u8) -> String {
        let mask: u32 = !0u32 << (32 - prefix);
        format!(
            "{}.{}.{}.{}",
            (mask >> 24) & 0xFF,
            (mask >> 16) & 0xFF,
            (mask >> 8) & 0xFF,
            mask & 0xFF
        )
    }

    /// Replace a marked block in text content
    fn replace_block(
        content: &str,
        begin_marker: &str,
        end_marker: &str,
        new_block: &str,
    ) -> String {
        if let Some(start) = content.find(begin_marker) {
            if let Some(end) = content[start..].find(end_marker) {
                let end_idx = start + end + end_marker.len();
                let mut result = String::with_capacity(content.len() + new_block.len());
                result.push_str(&content[..start]);
                result.push_str(new_block);
                result.push_str(&content[end_idx..]);
                return result;
            }
        }

        // Block not found, append it
        let mut result = String::with_capacity(content.len() + new_block.len() + 1);
        result.push_str(content);
        if !content.ends_with('\n') {
            result.push('\n');
        }
        result.push_str(new_block);
        result
    }
}

impl Default for NetStatePlugin {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl StatePlugin for NetStatePlugin {
    fn name(&self) -> &str {
        "net"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(net_schema())
    }

    fn is_available(&self) -> bool {
        // Check if OVSDB socket is available
        std::path::Path::new("/var/run/openvswitch/db.sock").exists()
    }

    fn unavailable_reason(&self) -> String {
        "OpenVSwitch OVSDB socket not found at /var/run/openvswitch/db.sock - install with: apt install openvswitch-switch".to_string()
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        let current_config: NetworkConfig = simd_json::serde::from_owned_value(current.clone())?;
        let desired_config: NetworkConfig = simd_json::serde::from_owned_value(desired.clone())?;

        let mut actions = Vec::new();

        // Build maps for quick lookup - avoid cloning strings unnecessarily
        let current_map: HashMap<&String, &InterfaceConfig> = current_config
            .interfaces
            .iter()
            .map(|i| (&i.name, i))
            .collect();

        let desired_map: HashMap<&String, &InterfaceConfig> = desired_config
            .interfaces
            .iter()
            .map(|i| (&i.name, i))
            .collect();

        // Find interfaces to create or modify
        for (name, desired_iface) in &desired_map {
            if let Some(current_iface) = current_map.get(name) {
                // Check if modification needed
                if simd_json::serde::to_owned_value(current_iface)?
                    != simd_json::serde::to_owned_value(desired_iface)?
                {
                    actions.push(StateAction::Modify {
                        resource: (*name).clone(),
                        changes: simd_json::serde::to_owned_value(desired_iface)?,
                    });
                }
            } else {
                actions.push(StateAction::Create {
                    resource: (*name).clone(),
                    config: simd_json::serde::to_owned_value(desired_iface)?,
                });
            }
        }

        // Find interfaces to delete
        for name in current_map.keys() {
            if !desired_map.contains_key(name) {
                actions.push(StateAction::Delete {
                    resource: (*name).clone(),
                });
            }
        }

        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata: op_state::DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: format!("{:x}", md5::compute(simd_json::to_string(current)?)),
                desired_hash: format!("{:x}", md5::compute(simd_json::to_string(desired)?)),
            },
        })
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let mut changes_applied = Vec::new();
        let mut errors = Vec::new();

        for action in &diff.actions {
            match action {
                StateAction::Create { resource, config }
                | StateAction::Modify {
                    resource,
                    changes: config,
                } => {
                    let iface_config: InterfaceConfig =
                        simd_json::serde::from_owned_value(config.clone())?;

                    match self.apply_ovs_config(&iface_config).await {
                        Ok(_) => {
                            changes_applied.push(format!("Applied OVS config for: {}", resource));
                        }
                        Err(e) => {
                            errors.push(format!(
                                "Failed to apply OVS config for {}: {}",
                                resource, e
                            ));
                        }
                    }
                }
                StateAction::Delete { resource } => {
                    // Delete OVS bridge via D-Bus
                    if resource.starts_with("ovsbr") || resource.starts_with("br") {
                        match self.delete_ovs_bridge(resource).await {
                            Ok(_) => {
                                changes_applied.push(format!("Deleted OVS bridge: {}", resource));
                            }
                            Err(e) => {
                                errors.push(format!(
                                    "Failed to delete OVS bridge {}: {}",
                                    resource, e
                                ));
                            }
                        }
                    } else {
                        changes_applied.push(format!("Skipped non-OVS interface: {}", resource));
                    }
                }
                StateAction::NoOp { .. } => {}
            }
        }

        Ok(ApplyResult {
            success: errors.is_empty(),
            changes_applied,
            errors,
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        let current_state = simd_json::json!(null);

        Ok(Checkpoint {
            id: format!("network-{}", chrono::Utc::now().timestamp()),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: current_state,
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, checkpoint: &Checkpoint) -> Result<()> {
        let old_config: NetworkConfig =
            simd_json::serde::from_owned_value(checkpoint.state_snapshot.clone())?;

        // Restore old OVS configuration via D-Bus
        for iface in &old_config.interfaces {
            match iface.if_type {
                InterfaceType::OvsBridge => {
                    self.apply_ovs_config(iface).await?;
                }
                InterfaceType::OvsPort => {
                    self.apply_ovs_port_config(iface).await?;
                }
                _ => {}
            }
        }

        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: true,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: true, // D-Bus operations are atomic
        }
    }
}

// =============================================================================
// Method input types - single source of truth via schemars
// =============================================================================

/// apply_interface method input
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyInterfaceInput {
    /// The subset of interface state this method can apply natively.
    pub interface: ApplyInterfaceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyInterfaceConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub interface_type: ApplyInterfaceType,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<StaticIpv4ApplyConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ApplyInterfaceType {
    Ethernet,
    OvsBridge,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StaticIpv4ApplyConfig {
    #[serde(default)]
    pub addresses: Vec<StaticIpv4Address>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StaticIpv4Address {
    pub ip: String,
    pub prefix: u8,
}

/// delete_ovs_bridge method input
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeleteOvsBridgeInput {
    /// Bridge name to delete
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InterfaceApplyOutput {
    pub name: String,
    pub interface_type: ApplyInterfaceType,
    pub link_state: String,
    pub attached_ports: Vec<String>,
    pub ipv4_addresses: Vec<AddressConfig>,
    pub default_gateway: Option<String>,
    pub operations_completed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BridgeDeleteOutput {
    pub name: String,
    pub absent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObservedInterface {
    up: bool,
    kind: Option<String>,
    ipv4_addresses: Vec<(String, u8)>,
}

#[derive(Debug, Clone)]
struct InterfacePlan {
    name: String,
    interface_type: ApplyInterfaceType,
    ports: Vec<String>,
    addresses: Vec<AddressConfig>,
    gateway: Option<String>,
}

#[async_trait]
trait NetOperationBackend: Send + Sync {
    async fn inspect_interface(&self, name: &str) -> Result<Option<ObservedInterface>>;
    async fn ovs_bridge_exists(&self, name: &str) -> Result<bool>;
    async fn ovs_bridge_ports(&self, name: &str) -> Result<Vec<String>>;
    async fn ovs_port_owner(&self, port: &str) -> Result<Option<String>>;
    async fn create_ovs_bridge(&self, name: &str) -> Result<()>;
    async fn add_ovs_port(&self, bridge: &str, port: &str) -> Result<()>;
    async fn delete_ovs_bridge(&self, name: &str) -> Result<()>;
    async fn link_up(&self, name: &str) -> Result<()>;
    async fn add_ipv4_address(&self, name: &str, address: &str, prefix: u8) -> Result<()>;
    async fn replace_default_route(&self, name: &str, gateway: &str) -> Result<()>;
    async fn default_route_matches(&self, name: &str, gateway: &str) -> Result<bool>;
}

struct NativeNetBackend;

#[async_trait]
impl NetOperationBackend for NativeNetBackend {
    async fn inspect_interface(&self, name: &str) -> Result<Option<ObservedInterface>> {
        let interface = op_network::rtnetlink::list_interfaces()
            .await?
            .into_iter()
            .find(|interface| interface.name == name);
        Ok(interface.map(|interface| ObservedInterface {
            up: interface.state == "up",
            kind: interface.kind,
            ipv4_addresses: interface
                .addresses
                .into_iter()
                .filter(|address| address.family == "inet")
                .map(|address| (address.address, address.prefix_len))
                .collect(),
        }))
    }

    async fn ovs_bridge_exists(&self, name: &str) -> Result<bool> {
        op_network::rovs_proxy::OvsdbDbusClient::new()
            .bridge_exists(name)
            .await
    }

    async fn ovs_bridge_ports(&self, name: &str) -> Result<Vec<String>> {
        op_network::rovs_proxy::OvsdbDbusClient::new()
            .list_bridge_ports(name)
            .await
    }

    async fn ovs_port_owner(&self, port: &str) -> Result<Option<String>> {
        let client = op_network::rovs_proxy::OvsdbDbusClient::new();
        for bridge in client.list_bridges().await? {
            if client
                .list_bridge_ports(&bridge)
                .await?
                .iter()
                .any(|candidate| candidate == port)
            {
                return Ok(Some(bridge));
            }
        }
        Ok(None)
    }

    async fn create_ovs_bridge(&self, name: &str) -> Result<()> {
        op_network::rovs_proxy::OvsdbDbusClient::new()
            .create_bridge(name)
            .await
    }

    async fn add_ovs_port(&self, bridge: &str, port: &str) -> Result<()> {
        op_network::rovs_proxy::OvsdbDbusClient::new()
            .add_port(bridge, port)
            .await
    }

    async fn delete_ovs_bridge(&self, name: &str) -> Result<()> {
        op_network::rovs_proxy::OvsdbDbusClient::new()
            .delete_bridge(name)
            .await
    }

    async fn link_up(&self, name: &str) -> Result<()> {
        op_network::rtnetlink::link_up(name).await
    }

    async fn add_ipv4_address(&self, name: &str, address: &str, prefix: u8) -> Result<()> {
        op_network::rtnetlink::add_ipv4_address(name, address, prefix).await
    }

    async fn replace_default_route(&self, name: &str, gateway: &str) -> Result<()> {
        op_network::rtnetlink::replace_default_route(name, gateway).await
    }

    async fn default_route_matches(&self, name: &str, gateway: &str) -> Result<bool> {
        op_network::rtnetlink::default_route_matches(name, gateway).await
    }
}

fn validate_interface_name(name: &str) -> Result<()> {
    anyhow::ensure!(
        !name.is_empty() && name.len() <= 15,
        "invalid interface name"
    );
    anyhow::ensure!(
        name.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b)),
        "interface name contains unsafe characters"
    );
    Ok(())
}

fn plan_interface(config: &ApplyInterfaceConfig) -> Result<InterfacePlan> {
    validate_interface_name(&config.name)?;
    match &config.interface_type {
        ApplyInterfaceType::Ethernet => {
            anyhow::ensure!(
                config.ports.is_empty(),
                "ethernet does not own bridge ports"
            );
        }
        ApplyInterfaceType::OvsBridge => {}
    }

    let mut planned_addresses = Vec::new();
    let mut planned_gateway = None;
    if let Some(ipv4) = &config.ipv4 {
        for address in &ipv4.addresses {
            anyhow::ensure!(
                address.ip.parse::<std::net::Ipv4Addr>().is_ok(),
                "invalid IPv4 address"
            );
            anyhow::ensure!(address.prefix <= 32, "invalid IPv4 prefix");
            planned_addresses.push(AddressConfig {
                ip: address.ip.clone(),
                prefix: address.prefix,
            });
        }
        if let Some(requested_gateway) = &ipv4.gateway {
            anyhow::ensure!(
                requested_gateway.parse::<std::net::Ipv4Addr>().is_ok(),
                "invalid IPv4 gateway"
            );
            planned_gateway = Some(requested_gateway.clone());
        }
    }
    let mut unique_ports = std::collections::HashSet::new();
    for port in &config.ports {
        validate_interface_name(port)?;
        anyhow::ensure!(
            port != &config.name,
            "a bridge cannot attach itself as a port"
        );
        anyhow::ensure!(unique_ports.insert(port), "duplicate bridge port '{port}'");
    }
    Ok(InterfacePlan {
        name: config.name.clone(),
        interface_type: config.interface_type.clone(),
        ports: config.ports.clone(),
        addresses: planned_addresses,
        gateway: planned_gateway,
    })
}

async fn apply_interface_with_backend(
    backend: &dyn NetOperationBackend,
    config: &ApplyInterfaceConfig,
) -> Result<InterfaceApplyOutput> {
    // This pure planning pass validates every requested field before the first
    // backend query or mutation.
    let plan = plan_interface(config)?;
    let initial_interface = backend.inspect_interface(&plan.name).await?;
    let mut initial_ports = Vec::new();
    let bridge_exists = if matches!(&plan.interface_type, ApplyInterfaceType::OvsBridge) {
        let exists = backend.ovs_bridge_exists(&plan.name).await?;
        if exists {
            initial_ports = backend.ovs_bridge_ports(&plan.name).await?;
        }
        exists
    } else {
        false
    };
    if matches!(&plan.interface_type, ApplyInterfaceType::Ethernet) {
        let observed = initial_interface
            .as_ref()
            .context("ethernet interface does not exist")?;
        anyhow::ensure!(
            observed.kind.is_none(),
            "requested ethernet is a virtual interface of kind {:?}",
            observed.kind
        );
    }
    if matches!(&plan.interface_type, ApplyInterfaceType::OvsBridge) {
        for port in &plan.ports {
            backend
                .inspect_interface(port)
                .await?
                .with_context(|| format!("OVS port interface '{port}' does not exist"))?;
            if let Some(owner) = backend.ovs_port_owner(port).await? {
                anyhow::ensure!(
                    owner == plan.name,
                    "OVS port '{port}' is already owned by bridge '{owner}'"
                );
            }
        }
    }

    let mut completed = Vec::new();
    let result: Result<()> = async {
        if matches!(&plan.interface_type, ApplyInterfaceType::OvsBridge) {
            if !bridge_exists {
                backend.create_ovs_bridge(&plan.name).await?;
                completed.push("create_ovs_bridge".to_string());
            }
            for port in &plan.ports {
                if !initial_ports.contains(port) {
                    backend.add_ovs_port(&plan.name, port).await?;
                    completed.push(format!("add_ovs_port:{port}"));
                }
            }
        }

        if initial_interface
            .as_ref()
            .is_none_or(|interface| !interface.up)
        {
            backend.link_up(&plan.name).await?;
            completed.push("link_up".to_string());
        }
        let existing_addresses = initial_interface
            .as_ref()
            .map(|interface| interface.ipv4_addresses.as_slice())
            .unwrap_or_default();
        for address in &plan.addresses {
            if !existing_addresses.contains(&(address.ip.clone(), address.prefix)) {
                backend
                    .add_ipv4_address(&plan.name, &address.ip, address.prefix)
                    .await?;
                completed.push(format!("add_ipv4:{}/{}", address.ip, address.prefix));
            }
        }
        if let Some(gateway) = &plan.gateway {
            if !backend.default_route_matches(&plan.name, gateway).await? {
                backend.replace_default_route(&plan.name, gateway).await?;
                completed.push(format!("replace_default_route:{gateway}"));
            }
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        anyhow::bail!(
            "partial failure applying '{}': {}; completed steps: {:?}",
            plan.name,
            error,
            completed
        );
    }

    let observed = backend
        .inspect_interface(&plan.name)
        .await?
        .context("interface absent after apply")?;
    anyhow::ensure!(observed.up, "interface is not up after apply");
    for address in &plan.addresses {
        anyhow::ensure!(
            observed
                .ipv4_addresses
                .contains(&(address.ip.clone(), address.prefix)),
            "IPv4 address {}/{} is absent after apply",
            address.ip,
            address.prefix
        );
    }
    let mut attached_ports = Vec::new();
    if matches!(&plan.interface_type, ApplyInterfaceType::OvsBridge) {
        anyhow::ensure!(
            backend.ovs_bridge_exists(&plan.name).await?,
            "OVS bridge is absent after apply"
        );
        attached_ports = backend.ovs_bridge_ports(&plan.name).await?;
        for port in &plan.ports {
            anyhow::ensure!(
                attached_ports.contains(port),
                "OVS port {port} is absent after apply"
            );
        }
        attached_ports.sort();
    }
    let default_route_verified = match &plan.gateway {
        Some(gateway) => backend.default_route_matches(&plan.name, gateway).await?,
        None => false,
    };
    anyhow::ensure!(
        plan.gateway.is_none() || default_route_verified,
        "default route is absent after apply"
    );
    let default_gateway = if default_route_verified {
        plan.gateway.clone()
    } else {
        None
    };
    let mut ipv4_addresses = observed
        .ipv4_addresses
        .into_iter()
        .map(|(ip, prefix)| AddressConfig { ip, prefix })
        .collect::<Vec<_>>();
    ipv4_addresses.sort_by(|left, right| (&left.ip, left.prefix).cmp(&(&right.ip, right.prefix)));
    Ok(InterfaceApplyOutput {
        name: plan.name,
        interface_type: plan.interface_type,
        link_state: "up".to_string(),
        attached_ports,
        ipv4_addresses,
        default_gateway,
        operations_completed: completed,
    })
}

async fn delete_bridge_with_backend(
    backend: &dyn NetOperationBackend,
    name: &str,
) -> Result<BridgeDeleteOutput> {
    validate_interface_name(name)?;
    anyhow::ensure!(
        backend.ovs_bridge_exists(name).await?,
        "OVS bridge '{name}' does not exist"
    );
    backend.delete_ovs_bridge(name).await?;
    anyhow::ensure!(
        !backend.ovs_bridge_exists(name).await?,
        "OVS bridge still exists after deletion"
    );
    Ok(BridgeDeleteOutput {
        name: name.to_string(),
        absent: true,
    })
}

pub async fn dispatch_net_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    match method {
        "apply_interface" => {
            let input: ApplyInterfaceInput = serde_json::from_value(args.clone())
                .context("invalid apply_interface arguments")?;
            Ok(serde_json::to_value(
                apply_interface_with_backend(&NativeNetBackend, &input.interface).await?,
            )?)
        }
        "delete_ovs_bridge" => {
            let input: DeleteOvsBridgeInput = serde_json::from_value(args.clone())
                .context("invalid delete_ovs_bridge arguments")?;
            Ok(serde_json::to_value(
                delete_bridge_with_backend(&NativeNetBackend, &input.name).await?,
            )?)
        }
        _ => anyhow::bail!(
            "unknown net method '{}'; no canonical backend dispatch",
            method
        ),
    }
}

// impl Default for NetStatePlugin {
//     fn default() -> Self {
//         Self::new()
//     }
// }

pub(crate) fn net_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(NetworkConfig))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "net",
        "2.0.0",
        "Network interface management via rtnetlink",
        &root,
    );
    schema.methods.insert(
        "apply_interface".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ApplyInterfaceInput,
            InterfaceApplyOutput,
        >(
            "apply_interface",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.network.interface.apply@v1",
            "mut.network.interface.apply@v1",
        ),
    );
    schema.methods.insert(
        "delete_ovs_bridge".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            DeleteOvsBridgeInput,
            BridgeDeleteOutput,
        >(
            "delete_ovs_bridge",
            op_state_store::SideEffect::Mutation,
            false,
            "cap.network.ovs-bridge.delete@v1",
            "mut.network.ovs-bridge.delete@v1",
        ),
    );
    schema.capabilities.insert(
        "cap.network.interface.apply@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.interface.apply@v1".to_string(),
            description: "Grants: apply_interface.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.network.ovs-bridge.delete@v1".to_string(),
        CapabilityDecl {
            id: "cap.network.ovs-bridge.delete@v1".to_string(),
            description: "Grants: delete_ovs_bridge.".to_string(),
        },
    );

    // Publish the typed L2 topology only. Addresses, controllers, dynamic
    // properties, and property-schema contents remain internal.
    schema.public_state.paths = vec![
        "/interfaces/*/name".to_string(),
        "/interfaces/*/type".to_string(),
        "/interfaces/*/driver".to_string(),
        "/interfaces/*/ports".to_string(),
    ];

    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct MockState {
        interface: Option<ObservedInterface>,
        bridge_exists: bool,
        ports: Vec<String>,
        route: Option<String>,
        port_owner: Option<String>,
        operations: Vec<String>,
        fail_on: Option<String>,
    }

    #[derive(Clone, Default)]
    struct MockBackend(Arc<Mutex<MockState>>);

    impl MockBackend {
        fn fail_on(operation: &str) -> Self {
            let backend = Self::default();
            backend.0.lock().unwrap().fail_on = Some(operation.to_string());
            backend
        }

        fn operations(&self) -> Vec<String> {
            self.0.lock().unwrap().operations.clone()
        }

        fn record(state: &mut MockState, operation: String) -> Result<()> {
            state.operations.push(operation.clone());
            if state.fail_on.as_deref() == Some(operation.as_str()) {
                anyhow::bail!("fixture failure at {operation}");
            }
            Ok(())
        }
    }

    #[async_trait]
    impl NetOperationBackend for MockBackend {
        async fn inspect_interface(&self, name: &str) -> Result<Option<ObservedInterface>> {
            if name == "eth0" {
                return Ok(Some(ObservedInterface {
                    up: true,
                    kind: None,
                    ipv4_addresses: Vec::new(),
                }));
            }
            Ok(self.0.lock().unwrap().interface.clone())
        }

        async fn ovs_bridge_exists(&self, _name: &str) -> Result<bool> {
            Ok(self.0.lock().unwrap().bridge_exists)
        }

        async fn ovs_bridge_ports(&self, _name: &str) -> Result<Vec<String>> {
            Ok(self.0.lock().unwrap().ports.clone())
        }

        async fn ovs_port_owner(&self, _port: &str) -> Result<Option<String>> {
            Ok(self.0.lock().unwrap().port_owner.clone())
        }

        async fn create_ovs_bridge(&self, name: &str) -> Result<()> {
            let mut state = self.0.lock().unwrap();
            Self::record(&mut state, format!("create:{name}"))?;
            state.bridge_exists = true;
            state.interface = Some(ObservedInterface {
                up: false,
                kind: Some("OpenVSwitch".to_string()),
                ipv4_addresses: Vec::new(),
            });
            Ok(())
        }

        async fn add_ovs_port(&self, bridge: &str, port: &str) -> Result<()> {
            let mut state = self.0.lock().unwrap();
            Self::record(&mut state, format!("port:{bridge}:{port}"))?;
            state.ports.push(port.to_string());
            Ok(())
        }

        async fn delete_ovs_bridge(&self, name: &str) -> Result<()> {
            let mut state = self.0.lock().unwrap();
            Self::record(&mut state, format!("delete:{name}"))?;
            state.bridge_exists = false;
            state.interface = None;
            Ok(())
        }

        async fn link_up(&self, name: &str) -> Result<()> {
            let mut state = self.0.lock().unwrap();
            Self::record(&mut state, format!("up:{name}"))?;
            state
                .interface
                .as_mut()
                .context("fixture interface absent")?
                .up = true;
            Ok(())
        }

        async fn add_ipv4_address(&self, name: &str, address: &str, prefix: u8) -> Result<()> {
            let mut state = self.0.lock().unwrap();
            Self::record(&mut state, format!("address:{name}:{address}/{prefix}"))?;
            state
                .interface
                .as_mut()
                .context("fixture interface absent")?
                .ipv4_addresses
                .push((address.to_string(), prefix));
            Ok(())
        }

        async fn replace_default_route(&self, name: &str, gateway: &str) -> Result<()> {
            let mut state = self.0.lock().unwrap();
            Self::record(&mut state, format!("route:{name}:{gateway}"))?;
            state.route = Some(gateway.to_string());
            Ok(())
        }

        async fn default_route_matches(&self, _name: &str, gateway: &str) -> Result<bool> {
            Ok(self.0.lock().unwrap().route.as_deref() == Some(gateway))
        }
    }

    fn ovs_bridge_config() -> ApplyInterfaceConfig {
        ApplyInterfaceConfig {
            name: "switch0".to_string(),
            interface_type: ApplyInterfaceType::OvsBridge,
            ports: vec!["eth0".to_string()],
            ipv4: Some(StaticIpv4ApplyConfig {
                addresses: vec![StaticIpv4Address {
                    ip: "192.0.2.2".to_string(),
                    prefix: 24,
                }],
                gateway: Some("192.0.2.1".to_string()),
            }),
        }
    }

    /// Every `x-oscal-subid` annotation in the derived schema must be a valid
    /// OSCAL subid according to the canonical taxonomy.
    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(NetworkConfig)).unwrap();
        let mut subids = Vec::new();
        collect_subids(&raw, &mut subids);
        assert!(!subids.is_empty(), "expected at least one subid");
        for subid in subids {
            crate::state_plugins::common::oscal::validate_subid(&subid)
                .unwrap_or_else(|e| panic!("invalid subid {subid}: {e}"));
        }
    }

    fn collect_subids(value: &serde_json::Value, out: &mut Vec<String>) {
        if let serde_json::Value::Object(map) = value {
            if let Some(subid) = map.get("x-oscal-subid").and_then(|v| v.as_str()) {
                out.push(subid.to_string());
            }
            for v in map.values() {
                collect_subids(v, out);
            }
        } else if let serde_json::Value::Array(arr) = value {
            for v in arr {
                collect_subids(v, out);
            }
        }
    }

    #[test]
    fn preflight_rejects_every_unsupported_field_before_backend() {
        let backend = MockBackend::default();
        let error = serde_json::from_value::<ApplyInterfaceInput>(serde_json::json!({
            "interface": {
                "name": "switch0", "type": "ovs-bridge", "ports": [],
                "ipv4": {"addresses": [], "dns": ["1.1.1.1"]}
            }
        }))
        .unwrap_err();
        assert!(error.to_string().contains("unknown field"));
        assert!(backend.operations().is_empty());

        assert!(
            serde_json::from_value::<ApplyInterfaceInput>(serde_json::json!({
                "interface": {"name":"port0", "type":"ovs-port"}
            }))
            .is_err()
        );
        assert!(serde_json::from_value::<ApplyInterfaceInput>(serde_json::json!({
            "interface": {"name":"switch0", "type":"ovs-bridge", "controller":"tcp:127.0.0.1:6653"}
        }))
        .is_err());
    }

    #[tokio::test]
    async fn apply_executes_complete_plan_and_returns_observed_postconditions() {
        let backend = MockBackend::default();
        let output = apply_interface_with_backend(&backend, &ovs_bridge_config())
            .await
            .unwrap();
        assert_eq!(output.name, "switch0");
        assert_eq!(output.link_state, "up");
        assert_eq!(output.attached_ports, ["eth0"]);
        assert_eq!(output.default_gateway.as_deref(), Some("192.0.2.1"));
        assert!(output
            .ipv4_addresses
            .iter()
            .any(|address| address.ip == "192.0.2.2" && address.prefix == 24));
        assert_eq!(
            backend.operations(),
            [
                "create:switch0",
                "port:switch0:eth0",
                "up:switch0",
                "address:switch0:192.0.2.2/24",
                "route:switch0:192.0.2.1",
            ]
        );
    }

    #[tokio::test]
    async fn ethernet_requires_an_existing_physical_link() {
        let backend = MockBackend::default();
        let config = ApplyInterfaceConfig {
            name: "eth0".to_string(),
            interface_type: ApplyInterfaceType::Ethernet,
            ports: Vec::new(),
            ipv4: None,
        };
        let output = apply_interface_with_backend(&backend, &config)
            .await
            .unwrap();
        assert_eq!(output.interface_type, ApplyInterfaceType::Ethernet);
        assert_eq!(output.link_state, "up");
        assert!(backend.operations().is_empty());

        backend.0.lock().unwrap().interface = Some(ObservedInterface {
            up: true,
            kind: Some("Dummy".to_string()),
            ipv4_addresses: Vec::new(),
        });
        let virtual_config = ApplyInterfaceConfig {
            name: "virtual0".to_string(),
            interface_type: ApplyInterfaceType::Ethernet,
            ports: Vec::new(),
            ipv4: None,
        };
        assert!(apply_interface_with_backend(&backend, &virtual_config)
            .await
            .unwrap_err()
            .to_string()
            .contains("virtual interface"));
    }

    #[tokio::test]
    async fn partial_failure_reports_completed_native_operations() {
        let backend = MockBackend::fail_on("address:switch0:192.0.2.2/24");
        let error = apply_interface_with_backend(&backend, &ovs_bridge_config())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("partial failure"));
        assert!(error.to_string().contains("link_up"));
        assert_eq!(
            backend.operations(),
            [
                "create:switch0",
                "port:switch0:eth0",
                "up:switch0",
                "address:switch0:192.0.2.2/24",
            ]
        );
    }

    #[tokio::test]
    async fn port_owned_by_another_bridge_fails_before_mutation() {
        let backend = MockBackend::default();
        backend.0.lock().unwrap().port_owner = Some("other-bridge".to_string());
        let error = apply_interface_with_backend(&backend, &ovs_bridge_config())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("already owned"));
        assert!(backend.operations().is_empty());
    }

    #[tokio::test]
    async fn bridge_delete_uses_ovsdb_membership_without_name_heuristics() {
        let backend = MockBackend::default();
        {
            let mut state = backend.0.lock().unwrap();
            state.bridge_exists = true;
            state.interface = Some(ObservedInterface {
                up: true,
                kind: Some("OpenVSwitch".to_string()),
                ipv4_addresses: Vec::new(),
            });
        }
        let output = delete_bridge_with_backend(&backend, "switch0")
            .await
            .unwrap();
        assert!(output.absent);
        assert_eq!(backend.operations(), ["delete:switch0"]);
    }

    #[test]
    fn invalid_ipv4_is_rejected_during_pure_preflight() {
        let mut config = ovs_bridge_config();
        config.ipv4.as_mut().unwrap().addresses = vec![StaticIpv4Address {
            ip: "not-an-ip".to_string(),
            prefix: 24,
        }];
        let error = plan_interface(&config).unwrap_err();
        assert!(error.to_string().contains("invalid IPv4 address"));
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("net", |_ctx| std::sync::Arc::new(NetStatePlugin::new()))
}
