//! Open vSwitch Tools
//!
//! AGENTS.md §4: D-Bus first. D-Bus always. D-Bus only.
//! Bridge/port queries go through the `rovs_commands` D-Bus plugin; flow
//! operations connect directly via `op_network::OpenFlowClient` (no daemon).

use crate::tool_registry::{Tool, ToolRegistry};
use anyhow::Result;
use async_trait::async_trait;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;

pub async fn register_all(registry: &ToolRegistry) -> Result<usize> {
    registry.register(Arc::new(OvsListBridgesTool)).await?;
    registry.register(Arc::new(OvsShowBridgeTool)).await?;
    registry.register(Arc::new(OvsListPortsTool)).await?;
    registry.register(Arc::new(OvsDumpFlowsTool)).await?;
    registry.register(Arc::new(OvsAddBridgeTool)).await?;
    registry.register(Arc::new(OvsDelBridgeTool)).await?;
    registry.register(Arc::new(OvsAddPortTool)).await?;
    registry.register(Arc::new(OvsDelFlowsTool)).await?;
    Ok(9)
}

macro_rules! ovs_tool {
    ($name:ident, $tool_name:expr, $desc:expr, $schema:expr, $exec:expr) => {
        pub struct $name;

        #[async_trait]
        impl Tool for $name {
            fn name(&self) -> &str { $tool_name }
            fn description(&self) -> &str { $desc }
            fn category(&self) -> &str { "ovs" }
            fn tags(&self) -> Vec<String> { vec!["ovs".into(), "network".into()] }
            fn input_schema(&self) -> Value { $schema }
            async fn execute(&self, input: Value) -> Result<Value> { $exec(input).await }
        }
    };
}

/// Get OVSDB client (direct rovs transport).
async fn ovsdb_client() -> Result<op_network::ovsdb::OvsdbClient> {
    Ok(op_network::ovsdb::OvsdbClient::new())
}

ovs_tool!(OvsListBridgesTool, "ovs_list_bridges", "List all OVS bridges via native OVSDB client.",
    json!({"type": "object", "properties": {}}),
    |_input: Value| async {
        let client = ovsdb_client().await?;
        let bridges = client.list_bridges().await
            .map_err(|e| anyhow::anyhow!("D-Bus list_bridges failed: {}", e))?;
        Ok(json!({"success": true, "bridges": bridges, "count": bridges.len(), "method": "dbus_native"}))
    }
);

ovs_tool!(OvsShowBridgeTool, "ovs_show_bridge", "Show OVS bridge details via native OVSDB client.",
    json!({"type": "object", "properties": {"bridge": {"type": "string"}}, "required": ["bridge"]}),
    |input: Value| async move {
        let bridge = input.get("bridge").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Missing bridge"))?;
        let client = ovsdb_client().await?;
        let ports = client.list_bridge_ports(bridge).await
            .map_err(|e| anyhow::anyhow!("D-Bus list_bridge_ports failed: {}", e))?;
        Ok(json!({"success": true, "bridge": bridge, "ports": ports, "method": "dbus_native"}))
    }
);

ovs_tool!(OvsListPortsTool, "ovs_list_ports", "List ports on an OVS bridge via native OVSDB client.",
    json!({"type": "object", "properties": {"bridge": {"type": "string"}}, "required": ["bridge"]}),
    |input: Value| async move {
        let bridge = input.get("bridge").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Missing bridge"))?;
        let client = ovsdb_client().await?;
        let ports = client.list_bridge_ports(bridge).await
            .map_err(|e| anyhow::anyhow!("D-Bus list_bridge_ports failed: {}", e))?;
        Ok(json!({"success": true, "bridge": bridge, "ports": ports, "count": ports.len(), "method": "dbus_native"}))
    }
);

ovs_tool!(OvsDumpFlowsTool, "ovs_dump_flows", "Dump flows from an OVS bridge's OVSDB Flow table via native OVSDB client.",
    json!({"type": "object", "properties": {"bridge": {"type": "string"}}, "required": ["bridge"]}),
    |input: Value| async move {
        let bridge = input.get("bridge").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Missing bridge"))?;
        let client = ovsdb_client().await?;
        let dump = client.dump_db("Open_vSwitch").await
            .map_err(|e| anyhow::anyhow!("D-Bus dump_db failed: {}", e))?;
        // Extract flows from the dump (flows table in OVSDB)
        let flows: Vec<String> = dump.get("tables")
            .and_then(|t| t.get("Flow"))
            .and_then(|f| f.get("rows"))
            .and_then(|r| r.as_array())
            .map(|arr| arr.iter().map(|v| v.to_string()).collect())
            .unwrap_or_default();
        Ok(json!({"success": true, "bridge": bridge, "flows": flows, "count": flows.len(), "method": "dbus_native"}))
    }
);

ovs_tool!(OvsAddBridgeTool, "ovs_add_bridge", "Create an OVS bridge via native OVSDB client.",
    json!({"type": "object", "properties": {"bridge": {"type": "string"}}, "required": ["bridge"]}),
    |input: Value| async move {
        let bridge = input.get("bridge").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Missing bridge"))?;
        let client = ovsdb_client().await?;
        client.create_bridge(bridge).await
            .map_err(|e| anyhow::anyhow!("D-Bus create_bridge failed: {}", e))?;
        Ok(json!({"success": true, "bridge": bridge, "action": "created", "method": "dbus_native"}))
    }
);

ovs_tool!(OvsDelBridgeTool, "ovs_del_bridge", "Delete an OVS bridge via native OVSDB client.",
    json!({"type": "object", "properties": {"bridge": {"type": "string"}}, "required": ["bridge"]}),
    |input: Value| async move {
        let bridge = input.get("bridge").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Missing bridge"))?;
        let client = ovsdb_client().await?;
        client.delete_bridge(bridge).await
            .map_err(|e| anyhow::anyhow!("D-Bus delete_bridge failed: {}", e))?;
        Ok(json!({"success": true, "bridge": bridge, "action": "deleted", "method": "dbus_native"}))
    }
);

ovs_tool!(OvsAddPortTool, "ovs_add_port", "Add a port to an OVS bridge via OVSDB.",
    json!({"type": "object", "properties": {"bridge": {"type": "string"}, "port": {"type": "string"}}, "required": ["bridge", "port"]}),
    |input: Value| async move {
        let bridge = input.get("bridge").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Missing bridge"))?;
        let port = input.get("port").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Missing port"))?;
        let client = ovsdb_client().await?;
        client.add_port(bridge, port).await
            .map_err(|e| anyhow::anyhow!("OVSDB add_port failed: {}", e))?;
        Ok(json!({"success": true, "bridge": bridge, "port": port, "action": "added", "method": "ovsdb"}))
    }
);

ovs_tool!(OvsDelFlowsTool, "ovs_del_flows", "Delete flows from an OVS bridge via OpenFlow native protocol.",
    json!({"type": "object", "properties": {"bridge": {"type": "string"}, "match_str": {"type": "string"}}, "required": ["bridge"]}),
    |input: Value| async move {
        let bridge = input.get("bridge").and_then(|v| v.as_str()).ok_or_else(|| anyhow::anyhow!("Missing bridge"))?;
        let addr: std::net::SocketAddr = "127.0.0.1:6653".parse()
            .map_err(|e| anyhow::anyhow!("Invalid OpenFlow controller address: {}", e))?;
        let mut openflow_client = op_network::OpenFlowClient::connect(addr).await
            .map_err(|e| anyhow::anyhow!("OpenFlow connect failed: {}", e))?;
        if input.get("match_str").and_then(|v| v.as_str()).is_some() {
            // Match-specific deletion requires OpenFlow protocol support
            return Ok(json!({"success": false, "error": "Match-specific flow deletion not yet implemented - use delete_all_flows instead", "bridge": bridge}));
        }
        openflow_client.delete_all_flows().await
            .map_err(|e| anyhow::anyhow!("OpenFlow delete_all_flows failed: {}", e))?;
        Ok(json!({"success": true, "bridge": bridge, "action": "flows_deleted", "method": "openflow_native"}))
    }
);
