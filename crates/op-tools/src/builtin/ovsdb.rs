//! OVSDB Tools — D-Bus based tools for Open vSwitch
//!
//! Uses the OVSDB JSON-RPC D-Bus passthrough (org.opdbus.rovs.jsonrpc).
//! No direct Unix-socket connections or CLI tools (ovs-vsctl, ovs-ofctl).

use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;

use crate::tool::{BoxedTool, Tool};

/// Get an OVSDB client (via D-Bus proxy)
async fn ovsdb_client() -> Result<op_network::ovsdb::OvsdbClient> {
    Ok(op_network::ovsdb::OvsdbClient::new())
}

// =============================================================================
// TOOL IMPLEMENTATIONS
// =============================================================================

/// Tool: Create OVS Bridge
pub struct OvsCreateBridgeTool;

#[async_trait]
impl Tool for OvsCreateBridgeTool {
    fn name(&self) -> &str {
        "ovs_create_bridge"
    }

    fn description(&self) -> &str {
        "Create an Open vSwitch bridge via D-Bus daemon (OVSDB)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the bridge to create (e.g., 'ovsbr0')"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?;

        let client = ovsdb_client().await?;
        client.create_bridge(name).await
            .map_err(|e| anyhow::anyhow!("D-Bus create_bridge failed: {}", e))?;

        Ok(json!({
            "success": true,
            "operation": "create_bridge",
            "bridge": name,
            "method": "dbus_native"
        }))
    }

    fn category(&self) -> &str {
        "network"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "bridge".to_string(), "ovsdb".to_string()]
    }
}

/// Tool: Delete OVS Bridge
pub struct OvsDeleteBridgeTool;

#[async_trait]
impl Tool for OvsDeleteBridgeTool {
    fn name(&self) -> &str {
        "ovs_delete_bridge"
    }

    fn description(&self) -> &str {
        "Delete an Open vSwitch bridge via D-Bus daemon (OVSDB)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the bridge to delete"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?;

        let client = ovsdb_client().await?;
        client.delete_bridge(name).await
            .map_err(|e| anyhow::anyhow!("D-Bus delete_bridge failed: {}", e))?;

        Ok(json!({
            "success": true,
            "operation": "delete_bridge",
            "bridge": name,
            "method": "dbus_native"
        }))
    }

    fn category(&self) -> &str {
        "network"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "bridge".to_string(), "ovsdb".to_string()]
    }
}

/// Tool: List OVS Bridges
pub struct OvsListBridgesTool;

#[async_trait]
impl Tool for OvsListBridgesTool {
    fn name(&self) -> &str {
        "ovs_list_bridges"
    }

    fn description(&self) -> &str {
        "List all Open vSwitch bridges via D-Bus daemon (OVSDB)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        let client = ovsdb_client().await?;
        let bridges = client.list_bridges().await
            .map_err(|e| anyhow::anyhow!("D-Bus list_bridges failed: {}", e))?;

        Ok(json!({
            "success": true,
            "operation": "list_bridges",
            "bridges": bridges,
            "count": bridges.len(),
            "method": "dbus_native"
        }))
    }

    fn category(&self) -> &str {
        "network"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "bridge".to_string(), "ovsdb".to_string()]
    }
}

/// Tool: Add Port to Bridge
pub struct OvsAddPortTool;

#[async_trait]
impl Tool for OvsAddPortTool {
    fn name(&self) -> &str {
        "ovs_add_port"
    }

    fn description(&self) -> &str {
        "Add a port to an Open vSwitch bridge via D-Bus daemon (OVSDB)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "bridge": {
                    "type": "string",
                    "description": "Name of the bridge"
                },
                "port": {
                    "type": "string",
                    "description": "Name of the port to add"
                }
            },
            "required": ["bridge", "port"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge = input
            .get("bridge")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: bridge"))?;

        let port = input
            .get("port")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: port"))?;

        let client = ovsdb_client().await?;
        client.add_port(bridge, port).await
            .map_err(|e| anyhow::anyhow!("D-Bus add_port failed: {}", e))?;

        Ok(json!({
            "success": true,
            "operation": "add_port",
            "bridge": bridge,
            "port": port,
            "method": "dbus_native"
        }))
    }

    fn category(&self) -> &str {
        "network"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "port".to_string(), "ovsdb".to_string()]
    }
}

/// Tool: Delete Port from Bridge
pub struct OvsDeletePortTool;

#[async_trait]
impl Tool for OvsDeletePortTool {
    fn name(&self) -> &str {
        "ovs_delete_port"
    }

    fn description(&self) -> &str {
        "Delete a port from an Open vSwitch bridge via D-Bus daemon (OVSDB)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "bridge": {
                    "type": "string",
                    "description": "Name of the bridge"
                },
                "port": {
                    "type": "string",
                    "description": "Name of the port to delete"
                }
            },
            "required": ["bridge", "port"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge = input
            .get("bridge")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: bridge"))?;

        let port = input
            .get("port")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: port"))?;

        let client = ovsdb_client().await?;
        client.delete_port(bridge, port).await
            .map_err(|e| anyhow::anyhow!("D-Bus delete_port failed: {}", e))?;

        Ok(json!({
            "success": true,
            "operation": "delete_port",
            "bridge": bridge,
            "port": port,
            "method": "dbus_native"
        }))
    }

    fn category(&self) -> &str {
        "network"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "port".to_string(), "ovsdb".to_string()]
    }
}

/// Tool: List Ports on Bridge
pub struct OvsListPortsTool;

#[async_trait]
impl Tool for OvsListPortsTool {
    fn name(&self) -> &str {
        "ovs_list_ports"
    }

    fn description(&self) -> &str {
        "List all ports on an Open vSwitch bridge via D-Bus daemon (OVSDB)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "bridge": {
                    "type": "string",
                    "description": "Name of the bridge"
                }
            },
            "required": ["bridge"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let bridge = input
            .get("bridge")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: bridge"))?;

        let client = ovsdb_client().await?;
        let ports = client.list_bridge_ports(bridge).await
            .map_err(|e| anyhow::anyhow!("D-Bus list_bridge_ports failed: {}", e))?;

        Ok(json!({
            "success": true,
            "operation": "list_ports",
            "bridge": bridge,
            "ports": ports,
            "count": ports.len(),
            "method": "dbus_native"
        }))
    }

    fn category(&self) -> &str {
        "network"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "port".to_string(), "ovsdb".to_string()]
    }
}

/// Tool: Get Bridge Info
pub struct OvsGetBridgeTool;

#[async_trait]
impl Tool for OvsGetBridgeTool {
    fn name(&self) -> &str {
        "ovs_get_bridge"
    }

    fn description(&self) -> &str {
        "Get detailed information about an Open vSwitch bridge via D-Bus daemon (OVSDB)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the bridge"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing required parameter: name"))?;

        let client = ovsdb_client().await?;

        let info = client.get_bridge_info(name).await
            .map_err(|e| anyhow::anyhow!("D-Bus get_bridge_info failed: {}", e))?;

        let ports = client.list_bridge_ports(name).await
            .unwrap_or_default();

        Ok(json!({
            "success": true,
            "operation": "get_bridge",
            "bridge": name,
            "info": info,
            "ports": ports,
            "method": "dbus_native"
        }))
    }

    fn category(&self) -> &str {
        "network"
    }

    fn tags(&self) -> Vec<String> {
        vec!["ovs".to_string(), "bridge".to_string(), "ovsdb".to_string()]
    }
}

// =============================================================================
// TOOL REGISTRATION
// =============================================================================

/// Create all OVS tools (routed through D-Bus daemon)
pub fn create_ovs_tools() -> Vec<BoxedTool> {
    vec![
        Arc::new(OvsCreateBridgeTool),
        Arc::new(OvsDeleteBridgeTool),
        Arc::new(OvsListBridgesTool),
        Arc::new(OvsAddPortTool),
        Arc::new(OvsDeletePortTool),
        Arc::new(OvsListPortsTool),
        Arc::new(OvsGetBridgeTool),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_names() {
        let tools = create_ovs_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();

        assert!(names.contains(&"ovs_create_bridge"));
        assert!(names.contains(&"ovs_delete_bridge"));
        assert!(names.contains(&"ovs_list_bridges"));
        assert!(names.contains(&"ovs_add_port"));
        assert!(names.contains(&"ovs_delete_port"));
        assert!(names.contains(&"ovs_list_ports"));
        assert!(names.contains(&"ovs_get_bridge"));
    }

    #[test]
    fn test_tool_schemas() {
        let tool = OvsCreateBridgeTool;
        let schema = tool.input_schema();

        assert!(schema.get("properties").is_some());
        assert!(schema.get("properties").unwrap().get("name").is_some());
    }
}