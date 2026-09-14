//! OVS Tools - OVSDB JSON-RPC based

use anyhow::Result;
use async_trait::async_trait;
use op_network::ovsdb::OvsdbClient;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;

use crate::tool::Tool;
use crate::ToolRegistry;

pub struct OvsTool {
    name: String,
    description: String,
}

impl OvsTool {
    pub fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
        }
    }
}

#[async_trait]
impl Tool for OvsTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        match self.name.as_str() {
            "ovs_list_bridges" => json!({
                "type": "object",
                "properties": {}
            }),
            "ovs_create_bridge" => json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Bridge name"}
                },
                "required": ["name"]
            }),
            "ovs_delete_bridge" => json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Bridge name"}
                },
                "required": ["name"]
            }),
            "ovs_add_port" => json!({
                "type": "object",
                "properties": {
                    "bridge": {"type": "string", "description": "Bridge name"},
                    "port": {"type": "string", "description": "Port name"}
                },
                "required": ["bridge", "port"]
            }),
            "ovs_list_ports" => json!({
                "type": "object",
                "properties": {
                    "bridge": {"type": "string", "description": "Bridge name"}
                },
                "required": ["bridge"]
            }),
            _ => json!({"type": "object", "properties": {}})
        }
    }

    async fn execute(&self, args: Value) -> Result<Value> {
        let client = OvsdbClient::new();
        
        match self.name.as_str() {
            "ovs_list_bridges" => {
                match client.list_bridges().await {
                    Ok(bridges) => Ok(json!({"bridges": bridges})),
                    Err(e) => Err(anyhow::anyhow!("Failed to list bridges: {}", e))
                }
            }
            "ovs_create_bridge" => {
                let name = args.get("name").and_then(|n| n.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing bridge name"))?;
                match client.create_bridge(name).await {
                    Ok(_) => Ok(json!({"created": name})),
                    Err(e) => Err(anyhow::anyhow!("Failed to create bridge: {}", e))
                }
            }
            "ovs_delete_bridge" => {
                let name = args.get("name").and_then(|n| n.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing bridge name"))?;
                match client.delete_bridge(name).await {
                    Ok(_) => Ok(json!({"deleted": name})),
                    Err(e) => Err(anyhow::anyhow!("Failed to delete bridge: {}", e))
                }
            }
            "ovs_list_ports" => {
                let bridge = args.get("bridge").and_then(|b| b.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing bridge name"))?;
                match client.list_bridge_ports(bridge).await {
                    Ok(ports) => Ok(json!({"bridge": bridge, "ports": ports})),
                    Err(e) => Err(anyhow::anyhow!("Failed to list ports: {}", e))
                }
            }
            "ovs_add_port" => {
                let bridge = args.get("bridge").and_then(|b| b.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing bridge name"))?;
                let port = args.get("port").and_then(|p| p.as_str())
                    .ok_or_else(|| anyhow::anyhow!("Missing port name"))?;
                match client.add_port(bridge, port).await {
                    Ok(_) => Ok(json!({"bridge": bridge, "port_added": port})),
                    Err(e) => Err(anyhow::anyhow!("Failed to add port: {}", e))
                }
            }
            _ => Ok(json!({"error": "Not implemented"}))
        }
    }
}

/// Register OVS tools with the registry
pub async fn register_ovs_tools(registry: &ToolRegistry) -> Result<()> {
    let tools = vec![
        OvsTool::new("ovs_list_bridges", "List all OVS bridges"),
        OvsTool::new("ovs_create_bridge", "Create a new OVS bridge"),
        OvsTool::new("ovs_delete_bridge", "Delete an OVS bridge"),
        OvsTool::new("ovs_list_ports", "List ports on an OVS bridge"),
        OvsTool::new("ovs_add_port", "Add a port to an OVS bridge"),
    ];

    for tool in tools {
        registry.register_tool(Arc::new(tool)).await?;
    }

    Ok(())
}
