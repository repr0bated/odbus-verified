//! Direct OVSDB JSON-RPC client - no wrappers, pure native protocol
//! Talks directly to /var/run/openvswitch/db.sock

use anyhow::{Context, Result};
use simd_json::{json, OwnedValue as Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Direct OVSDB JSON-RPC client
pub struct OvsdbDbusClient {
    socket_path: String,
}

impl OvsdbDbusClient {
    /// Connect to OVSDB unix socket
    pub fn new() -> Self {
        Self {
            socket_path: "/var/run/openvswitch/db.sock".to_string(),
        }
    }

    /// Send JSON-RPC request and get response
    async fn rpc_call(&self, method: &str, params: Value) -> Result<Value> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .context("Failed to connect to OVSDB socket")?;

        // Build JSON-RPC request
        let request = json!({
            "method": method,
            "params": params,
            "id": 0
        });

        // Send request
        let request_str = simd_json::to_string(&request)?;
        stream.write_all(request_str.as_bytes()).await?;
        stream.write_all(b"\n").await?;
        // Read response with timeout
        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            reader.read_line(&mut response_line),
        )
        .await
        .context("OVSDB response timeout")??;

        let response: Value = simd_json::from_str(&response_line)?;

        // Check for error
        if let Some(error) = response.get("error") {
            return Err(anyhow::anyhow!("OVSDB error: {}", error));
        }

        Ok(response["result"].clone())
    }

    /// List all databases
    pub async fn list_dbs(&self) -> Result<Vec<String>> {
        let result = self.rpc_call("list_dbs", json!([])).await?;
        Ok(simd_json::serde::from_owned_value(result)?)
    }

    /// Get schema for Open_vSwitch database
    #[allow(dead_code)]
    pub async fn get_schema(&self) -> Result<Value> {
        self.rpc_call("get_schema", json!(["Open_vSwitch"])).await
    }

    /// Dump entire Open_vSwitch database: table -> rows (JSON)
    #[allow(dead_code)]
    pub async fn dump_open_vswitch(&self) -> Result<Value> {
        // Discover tables from schema
        let schema = self.get_schema().await?;
        let tables = schema
            .get("tables")
            .and_then(|v| v.as_object())
            .ok_or_else(|| anyhow::anyhow!("Invalid OVSDB schema: missing tables"))?;

        // Build select ops for all tables
        let mut ops = Vec::new();
        let mut order = Vec::new();
        for (name, _def) in tables.iter() {
            ops.push(json!({
                "op": "select",
                "table": name,
                "where": []
            }));
            order.push(name.clone());
        }

        let result = self.transact(json!(ops)).await?;

        // Assemble into object
        let mut out = simd_json::value::owned::Object::new();
        for (i, name) in order.into_iter().enumerate() {
            let rows = result
                .get(i)
                .and_then(|r| r.get("rows"))
                .cloned()
                .unwrap_or_else(|| json!([]));
            out.insert(name, rows);
        }

        Ok(Value::Object(Box::new(out)))
    }

    /// Transact - execute OVSDB operations
    pub async fn transact(&self, operations: Value) -> Result<Value> {
        let mut params = vec![json!("Open_vSwitch")];
        if let Some(ops_array) = operations.as_array() {
            for op in ops_array {
                params.push(op.clone());
            }
        }
        self.rpc_call("transact", json!(params)).await
    }

    /// Create OVS bridge
    pub async fn create_bridge(&self, bridge_name: &str) -> Result<()> {
        // Generate UUIDs for bridge and port
        let bridge_uuid = format!("bridge-{}", bridge_name);
        let port_uuid = format!("port-{}", bridge_name);
        let iface_uuid = format!("iface-{}", bridge_name);

        let operations = json!([
            {
                "op": "insert",
                "table": "Bridge",
                "row": {
                    "name": bridge_name,
                    "ports": ["set", [["named-uuid", port_uuid]]]
                },
                "uuid-name": bridge_uuid
            },
            {
                "op": "insert",
                "table": "Port",
                "row": {
                    "name": bridge_name,
                    "interfaces": ["set", [["named-uuid", iface_uuid]]]
                },
                "uuid-name": port_uuid
            },
            {
                "op": "insert",
                "table": "Interface",
                "row": {
                    "name": bridge_name,
                    "type": "internal"
                },
                "uuid-name": iface_uuid
            },
            {
                "op": "mutate",
                "table": "Open_vSwitch",
                "where": [],
                "mutations": [
                    ["bridges", "insert", ["set", [["named-uuid", bridge_uuid]]]]
                ]
            }
        ]);

        self.transact(operations).await?;
        Ok(())
    }

    /// Add port to bridge
    pub async fn add_port(&self, bridge_name: &str, port_name: &str) -> Result<()> {
        // First, find the bridge UUID
        let bridge_uuid = self.find_bridge_uuid(bridge_name).await?;

        let port_uuid = format!("port-{}", port_name);
        let iface_uuid = format!("iface-{}", port_name);

        let operations = json!([
            {
                "op": "insert",
                "table": "Port",
                "row": {
                    "name": port_name,
                    "interfaces": ["set", [["named-uuid", iface_uuid]]]
                },
                "uuid-name": port_uuid
            },
            {
                "op": "insert",
                "table": "Interface",
                "row": {
                    "name": port_name
                },
                "uuid-name": iface_uuid
            },
            {
                "op": "mutate",
                "table": "Bridge",
                "where": [["_uuid", "==", ["uuid", &bridge_uuid]]],
                "mutations": [
                    ["ports", "insert", ["set", [["named-uuid", port_uuid]]]]
                ]
            }
        ]);

        self.transact(operations).await?;
        Ok(())
    }

    /// Delete bridge
    pub async fn delete_bridge(&self, bridge_name: &str) -> Result<()> {
        let bridge_uuid = self.find_bridge_uuid(bridge_name).await?;

        let operations = json!([
            {
                "op": "mutate",
                "table": "Open_vSwitch",
                "where": [],
                "mutations": [
                    ["bridges", "delete", ["uuid", &bridge_uuid]]
                ]
            },
            {
                "op": "delete",
                "table": "Bridge",
                "where": [["_uuid", "==", ["uuid", &bridge_uuid]]]
            }
        ]);

        self.transact(operations).await?;
        Ok(())
    }

    /// Check if bridge exists
    pub async fn bridge_exists(&self, bridge_name: &str) -> Result<bool> {
        match self.find_bridge_uuid(bridge_name).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Find bridge UUID by name
    async fn find_bridge_uuid(&self, bridge_name: &str) -> Result<String> {
        let operations = json!([{
            "op": "select",
            "table": "Bridge",
            "where": [["name", "==", bridge_name]],
            "columns": ["_uuid"]
        }]);

        let result = self.transact(operations).await?;

        if let Some(rows) = result[0]["rows"].as_array() {
            if let Some(first_row) = rows.first() {
                if let Some(uuid_array) = first_row["_uuid"].as_array() {
                    if uuid_array.len() == 2 && uuid_array[0] == "uuid" {
                        return Ok(uuid_array[1].as_str().unwrap().to_string());
                    }
                }
            }
        }

        Err(anyhow::anyhow!("Bridge '{}' not found", bridge_name))
    }

    /// List all bridges
    pub async fn list_bridges(&self) -> Result<Vec<String>> {
        let operations = json!([{
            "op": "select",
            "table": "Bridge",
            "where": [],
            "columns": ["name"]
        }]);

        let result = self.transact(operations).await?;

        let mut bridges = Vec::new();
        if let Some(rows) = result[0]["rows"].as_array() {
            for row in rows {
                if let Some(name) = row["name"].as_str() {
                    bridges.push(name.to_string());
                }
            }
        }

        Ok(bridges)
    }

    /// List ports on bridge
    pub async fn list_bridge_ports(&self, bridge_name: &str) -> Result<Vec<String>> {
        let bridge_uuid = self.find_bridge_uuid(bridge_name).await?;

        // Get the bridge with its ports
        let operations = json!([{
            "op": "select",
            "table": "Bridge",
            "where": [["_uuid", "==", ["uuid", &bridge_uuid]]],
            "columns": ["ports"]
        }]);

        let result = self.transact(operations).await?;

        let mut port_uuids = Vec::new();
        if let Some(rows) = result[0]["rows"].as_array() {
            if let Some(first_row) = rows.first() {
                if let Some(ports) = first_row["ports"].as_array() {
                    if ports.len() == 2 && ports[0] == "set" {
                        if let Some(port_set) = ports[1].as_array() {
                            for port in port_set {
                                if let Some(uuid_array) = port.as_array() {
                                    if uuid_array.len() == 2 && uuid_array[0] == "uuid" {
                                        port_uuids
                                            .push(uuid_array[1].as_str().unwrap().to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Now get port names
        let mut port_names = Vec::new();
        for port_uuid in port_uuids {
            let operations = json!([{
                "op": "select",
                "table": "Port",
                "where": [["_uuid", "==", ["uuid", &port_uuid]]],
                "columns": ["name"]
            }]);

            let result = self.transact(operations).await?;
            if let Some(rows) = result[0]["rows"].as_array() {
                if let Some(first_row) = rows.first() {
                    if let Some(name) = first_row["name"].as_str() {
                        port_names.push(name.to_string());
                    }
                }
            }
        }

        Ok(port_names)
    }

    /// Get bridge info
    pub async fn get_bridge_info(&self, bridge_name: &str) -> Result<String> {
        let bridge_uuid = self.find_bridge_uuid(bridge_name).await?;

        let operations = json!([{
            "op": "select",
            "table": "Bridge",
            "where": [["_uuid", "==", ["uuid", &bridge_uuid]]],
            "columns": []
        }]);

        let result = self.transact(operations).await?;
        Ok(simd_json::to_string_pretty(&result[0]["rows"][0])?)
    }
}

impl Default for OvsdbDbusClient {
    fn default() -> Self {
        Self::new()
    }
}
