//! Rtnetlink tools - native network interface and route management
//!
//! These tools provide direct access to Linux network configuration via rtnetlink,
//! avoiding CLI tools like `ip`, `ifconfig`, etc.

use crate::Tool;
use crate::ToolRegistry;
use anyhow::Result;
use async_trait::async_trait;
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;
use tracing::info;

/// Tool to list all network interfaces
pub struct RtnetlinkListInterfacesTool;

#[async_trait]
impl Tool for RtnetlinkListInterfacesTool {
    fn name(&self) -> &str {
        "list_network_interfaces"
    }

    fn description(&self) -> &str {
        "List all network interfaces with their details (name, MAC, MTU, state, addresses) using native rtnetlink. Equivalent to 'ip addr show' but without CLI."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "filter_state": {
                    "type": "string",
                    "description": "Optional: filter by state ('up' or 'down')",
                    "enum": ["up", "down"]
                }
            },
            "required": []
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "rtnetlink".to_string(),
            "network".to_string(),
            "interfaces".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        info!("Listing network interfaces via rtnetlink");

        let filter_state = input.get("filter_state").and_then(|v| v.as_str());

        let mut interfaces = op_network::rtnetlink::list_interfaces()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list interfaces via rtnetlink: {}", e))?;

        // Apply filters
        if let Some(state) = filter_state {
            interfaces.retain(|iface| iface.state == state);
        }

        let count = interfaces.len();
        Ok(json!({
            "protocol": "rtnetlink",
            "count": count,
            "interfaces": interfaces
        }))
    }
}

/// Tool to get the default route
pub struct RtnetlinkGetDefaultRouteTool;

#[async_trait]
impl Tool for RtnetlinkGetDefaultRouteTool {
    fn name(&self) -> &str {
        "rtnetlink_get_default_route"
    }

    fn description(&self) -> &str {
        "Get the default IPv4 route (gateway and interface) using native rtnetlink. Equivalent to 'ip route show default' but without CLI."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "rtnetlink".to_string(),
            "network".to_string(),
            "route".to_string(),
        ]
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        info!("Getting default route via rtnetlink");

        match op_network::rtnetlink::get_default_route().await {
            Ok(Some(route)) => Ok(json!({
                "protocol": "rtnetlink",
                "found": true,
                "route": route
            })),
            Ok(None) => Ok(json!({
                "protocol": "rtnetlink",
                "found": false,
                "message": "No default route configured"
            })),
            Err(e) => Err(anyhow::anyhow!("Failed to get default route: {}", e)),
        }
    }
}

/// Tool to add an IP address to an interface
pub struct RtnetlinkAddAddressTool;

#[async_trait]
impl Tool for RtnetlinkAddAddressTool {
    fn name(&self) -> &str {
        "rtnetlink_add_address"
    }

    fn description(&self) -> &str {
        "Add an IPv4 address to a network interface using native rtnetlink. Equivalent to 'ip addr add' but without CLI."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "interface": {
                    "type": "string",
                    "description": "Interface name (e.g., 'eth0', 'ens1')"
                },
                "address": {
                    "type": "string",
                    "description": "IPv4 address to add (e.g., '10.0.0.1')"
                },
                "prefix_len": {
                    "type": "integer",
                    "description": "Prefix length / CIDR (e.g., 24 for /24, 32 for single host)"
                }
            },
            "required": ["interface", "address", "prefix_len"]
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "rtnetlink".to_string(),
            "network".to_string(),
            "address".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        // Accept both "interface" and "iface" for compatibility
        let interface = input
            .get("interface")
            .or_else(|| input.get("iface"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: interface"))?;
        let address = input
            .get("address")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: address"))?;
        // Accept both "prefix_len" and "prefix" for compatibility
        let prefix_len = input
            .get("prefix_len")
            .or_else(|| input.get("prefix"))
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: prefix_len"))?
            as u8;

        info!(
            "Adding address {}/{} to {} via rtnetlink",
            address, prefix_len, interface
        );

        match op_network::rtnetlink::add_ipv4_address(interface, address, prefix_len).await {
            Ok(()) => Ok(json!({
                "protocol": "rtnetlink",
                "success": true,
                "interface": interface,
                "address": address,
                "prefix_len": prefix_len,
                "message": format!("Added {}/{} to {}", address, prefix_len, interface)
            })),
            Err(e) => Err(anyhow::anyhow!("Failed to add address: {}", e)),
        }
    }
}

/// Tool to bring an interface up
pub struct RtnetlinkLinkUpTool;

#[async_trait]
impl Tool for RtnetlinkLinkUpTool {
    fn name(&self) -> &str {
        "rtnetlink_link_up"
    }

    fn description(&self) -> &str {
        "Bring a network interface up using native rtnetlink. Equivalent to 'ip link set up' but without CLI."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "interface": {
                    "type": "string",
                    "description": "Interface name to bring up (e.g., 'eth0', 'ens1')"
                }
            },
            "required": ["interface"]
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "rtnetlink".to_string(),
            "network".to_string(),
            "link".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let interface = input
            .get("interface")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: interface"))?;

        info!("Bringing interface {} up via rtnetlink", interface);

        match op_network::rtnetlink::link_up(interface).await {
            Ok(()) => Ok(json!({
                "protocol": "rtnetlink",
                "success": true,
                "interface": interface,
                "state": "up",
                "message": format!("Interface {} is now up", interface)
            })),
            Err(e) => Err(anyhow::anyhow!("Failed to bring interface up: {}", e)),
        }
    }
}

/// Tool to bring an interface down
pub struct RtnetlinkLinkDownTool;

#[async_trait]
impl Tool for RtnetlinkLinkDownTool {
    fn name(&self) -> &str {
        "rtnetlink_link_down"
    }

    fn description(&self) -> &str {
        "Bring a network interface down using native rtnetlink. Equivalent to 'ip link set down' but without CLI."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "interface": {
                    "type": "string",
                    "description": "Interface name to bring down (e.g., 'eth0', 'ens1')"
                }
            },
            "required": ["interface"]
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "rtnetlink".to_string(),
            "network".to_string(),
            "link".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let interface = input
            .get("interface")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: interface"))?;

        info!("Bringing interface {} down via rtnetlink", interface);

        match op_network::rtnetlink::link_down(interface).await {
            Ok(()) => Ok(json!({
                "protocol": "rtnetlink",
                "success": true,
                "interface": interface,
                "state": "down",
                "message": format!("Interface {} is now down", interface)
            })),
            Err(e) => Err(anyhow::anyhow!("Failed to bring interface down: {}", e)),
        }
    }
}

/// Tool to set MAC address on an interface
pub struct RtnetlinkSetMacAddressTool;

#[async_trait]
impl Tool for RtnetlinkSetMacAddressTool {
    fn name(&self) -> &str {
        "rtnetlink_set_mac_address"
    }

    fn description(&self) -> &str {
        "Set the MAC address on a network interface using native rtnetlink. Equivalent to 'ip link set dev <iface> address <mac>' but without CLI."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "interface": {
                    "type": "string",
                    "description": "Interface name (e.g., 'ovsbr0-int')"
                },
                "mac_address": {
                    "type": "string",
                    "description": "MAC address in colon-separated hex (e.g., 'fa:16:3e:f1:71:d2')"
                }
            },
            "required": ["interface", "mac_address"]
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "rtnetlink".to_string(),
            "network".to_string(),
            "mac".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let interface = input
            .get("interface")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: interface"))?;
        let mac = input
            .get("mac_address")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: mac_address"))?;

        info!("Setting MAC {} on {} via rtnetlink", mac, interface);

        match op_network::rtnetlink::set_mac_address(interface, mac).await {
            Ok(()) => Ok(json!({
                "protocol": "rtnetlink",
                "success": true,
                "interface": interface,
                "mac_address": mac,
                "message": format!("Set MAC {} on {}", mac, interface)
            })),
            Err(e) => Err(anyhow::anyhow!("Failed to set MAC address: {}", e)),
        }
    }
}

/// Tool to add a default route
pub struct RtnetlinkAddDefaultRouteTool;

#[async_trait]
impl Tool for RtnetlinkAddDefaultRouteTool {
    fn name(&self) -> &str {
        "rtnetlink_add_default_route"
    }

    fn description(&self) -> &str {
        "Add a default IPv4 route using native rtnetlink. Equivalent to 'ip route add default via <gateway> dev <iface>' but without CLI."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "interface": {
                    "type": "string",
                    "description": "Interface name for the route (e.g., 'ens3')"
                },
                "gateway": {
                    "type": "string",
                    "description": "Gateway IPv4 address (e.g., '148.113.204.1')"
                }
            },
            "required": ["interface", "gateway"]
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "rtnetlink".to_string(),
            "network".to_string(),
            "route".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let interface = input
            .get("interface")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: interface"))?;
        let gateway = input
            .get("gateway")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: gateway"))?;

        info!(
            "Adding default route via {} on {} via rtnetlink",
            gateway, interface
        );

        match op_network::rtnetlink::add_default_route(interface, gateway).await {
            Ok(()) => Ok(json!({
                "protocol": "rtnetlink",
                "success": true,
                "interface": interface,
                "gateway": gateway,
                "message": format!("Added default route via {} on {}", gateway, interface)
            })),
            Err(e) => Err(anyhow::anyhow!("Failed to add default route: {}", e)),
        }
    }
}

/// Tool to delete the default route
pub struct RtnetlinkDelDefaultRouteTool;

#[async_trait]
impl Tool for RtnetlinkDelDefaultRouteTool {
    fn name(&self) -> &str {
        "rtnetlink_del_default_route"
    }

    fn description(&self) -> &str {
        "Delete the default IPv4 route using native rtnetlink. Equivalent to 'ip route del default' but without CLI."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "rtnetlink".to_string(),
            "network".to_string(),
            "route".to_string(),
        ]
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        info!("Deleting default route via rtnetlink");

        match op_network::rtnetlink::del_default_route().await {
            Ok(()) => Ok(json!({
                "protocol": "rtnetlink",
                "success": true,
                "message": "Deleted default route"
            })),
            Err(e) => Err(anyhow::anyhow!("Failed to delete default route: {}", e)),
        }
    }
}

/// Tool to flush all addresses from an interface
pub struct RtnetlinkFlushAddressesTool;

#[async_trait]
impl Tool for RtnetlinkFlushAddressesTool {
    fn name(&self) -> &str {
        "rtnetlink_flush_addresses"
    }

    fn description(&self) -> &str {
        "Flush all IP addresses from a network interface using native rtnetlink. Equivalent to 'ip addr flush dev <iface>' but without CLI."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "interface": {
                    "type": "string",
                    "description": "Interface name to flush addresses from"
                }
            },
            "required": ["interface"]
        })
    }

    fn category(&self) -> &str {
        "networking"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "rtnetlink".to_string(),
            "network".to_string(),
            "address".to_string(),
        ]
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let interface = input
            .get("interface")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: interface"))?;

        info!("Flushing addresses on {} via rtnetlink", interface);

        match op_network::rtnetlink::flush_addresses(interface).await {
            Ok(()) => Ok(json!({
                "protocol": "rtnetlink",
                "success": true,
                "interface": interface,
                "message": format!("Flushed all addresses from {}", interface)
            })),
            Err(e) => Err(anyhow::anyhow!("Failed to flush addresses: {}", e)),
        }
    }
}

/// Register all rtnetlink tools
pub async fn register_rtnetlink_tools(registry: &ToolRegistry) -> Result<()> {
    registry
        .register_tool(Arc::new(RtnetlinkListInterfacesTool))
        .await?;
    registry
        .register_tool(Arc::new(RtnetlinkGetDefaultRouteTool))
        .await?;
    registry
        .register_tool(Arc::new(RtnetlinkAddAddressTool))
        .await?;
    registry
        .register_tool(Arc::new(RtnetlinkLinkUpTool))
        .await?;
    registry
        .register_tool(Arc::new(RtnetlinkLinkDownTool))
        .await?;
    registry
        .register_tool(Arc::new(RtnetlinkSetMacAddressTool))
        .await?;
    registry
        .register_tool(Arc::new(RtnetlinkAddDefaultRouteTool))
        .await?;
    registry
        .register_tool(Arc::new(RtnetlinkDelDefaultRouteTool))
        .await?;
    registry
        .register_tool(Arc::new(RtnetlinkFlushAddressesTool))
        .await?;
    info!("Registered 9 rtnetlink tools");
    Ok(())
}
