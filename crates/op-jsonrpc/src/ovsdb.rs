//! OVSDB JSON-RPC client for Open vSwitch integration
//!
//! Direct JSON-RPC client for /var/run/openvswitch/db.sock

use anyhow::{Context, Result};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tracing::{debug, info};

/// OVSDB JSON-RPC client
pub struct OvsdbDbusClient {
    socket_path: String,
    timeout: Duration,
}

impl OvsdbDbusClient {
    /// Create a new OVSDB client with default socket path
    pub fn new() -> Self {
        Self {
            socket_path: "/var/run/openvswitch/db.sock".to_string(),
            timeout: Duration::from_secs(30),
        }
    }

    /// Create with a custom socket path
    pub fn with_socket(socket_path: impl Into<String>) -> Self {
        Self {
            socket_path: socket_path.into(),
            timeout: Duration::from_secs(30),
        }
    }

    /// Set timeout for RPC calls
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Send a JSON-RPC request and get response
    async fn rpc_call(&self, method: &str, params: Value) -> Result<Value> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .context("Failed to connect to OVSDB socket")?;

        let request = json!({
            "method": method,
            "params": params,
            "id": 0
        });

        let request_str = simd_json::to_string(&request)?;
        debug!("OVSDB request: {}", request_str);

        stream.write_all(request_str.as_bytes()).await?;
        stream.write_all(b"\n").await?;
        // Signal request completion. OVSDB may not newline-terminate responses, so
        // line-oriented reads can block until timeout.
        stream.shutdown().await?;

        let mut response_bytes = Vec::new();
        tokio::time::timeout(self.timeout, stream.read_to_end(&mut response_bytes))
            .await
            .context("OVSDB response timeout")??;

        if response_bytes.is_empty() {
            return Err(anyhow::anyhow!("OVSDB returned empty response"));
        }

        let response_text =
            String::from_utf8(response_bytes).context("OVSDB response was not valid UTF-8")?;
        debug!("OVSDB response: {}", response_text.trim());
        let response: Value = Self::parse_json_response(&response_text)?;

        if let Some(error) = response.get("error") {
            if !error.is_null() {
                return Err(anyhow::anyhow!("OVSDB error: {}", error));
            }
        }

        Ok(response["result"].clone())
    }

    fn parse_json_response(response_text: &str) -> Result<Value> {
        let trimmed = response_text.trim();
        if trimmed.is_empty() {
            return Err(anyhow::anyhow!("OVSDB response contained only whitespace"));
        }

        // First try parsing the full payload.
        let mut payload = trimmed.to_string();
        if let Ok(value) = unsafe { simd_json::from_str::<Value>(payload.as_mut_str()) } {
            return Ok(value);
        }

        // Some servers can emit multiple lines; fall back to the last valid JSON line.
        for line in trimmed.lines().rev() {
            let candidate = line.trim();
            if candidate.is_empty() {
                continue;
            }

            let mut owned = candidate.to_string();
            if let Ok(value) = unsafe { simd_json::from_str::<Value>(owned.as_mut_str()) } {
                return Ok(value);
            }
        }

        Err(anyhow::anyhow!(
            "Failed to parse OVSDB JSON response payload"
        ))
    }

    /// List all databases
    pub async fn list_dbs(&self) -> Result<Vec<String>> {
        let result = self.rpc_call("list_dbs", json!([])).await?;
        Ok(simd_json::serde::from_owned_value(result)?)
    }

    /// Get schema for a database
    pub async fn get_schema(&self, db: &str) -> Result<Value> {
        self.rpc_call("get_schema", json!([db])).await
    }

    /// Execute a transaction
    pub async fn transact(&self, db: &str, operations: Value) -> Result<Value> {
        let mut params = vec![json!(db)];
        if let Some(ops_array) = operations.as_array() {
            for op in ops_array {
                params.push(op.clone());
            }
        }
        let result = self.rpc_call("transact", json!(params)).await?;

        // OVSDB can return per-operation errors inside the result array.
        if let Some(results) = result.as_array() {
            for (idx, op_result) in results.iter().enumerate() {
                if let Some(error) = op_result.get("error").and_then(|e| e.as_str()) {
                    let details = op_result
                        .get("details")
                        .and_then(|d| d.as_str())
                        .unwrap_or("no details");
                    return Err(anyhow::anyhow!(
                        "OVSDB operation {} failed: {} ({})",
                        idx,
                        error,
                        details
                    ));
                }
            }
        }

        Ok(result)
    }

    /// Create a bridge
    pub async fn create_bridge(&self, name: &str) -> Result<()> {
        let safe_name = Self::sanitize_ref(name);
        let bridge_uuid = format!("bridge_{}", safe_name);
        let port_uuid = format!("port_{}", safe_name);
        let iface_uuid = format!("iface_{}", safe_name);

        let operations = json!([
            {
                "op": "insert",
                "table": "Bridge",
                "row": {
                    "name": name,
                    "ports": ["set", [["named-uuid", port_uuid]]]
                },
                "uuid-name": bridge_uuid
            },
            {
                "op": "insert",
                "table": "Port",
                "row": {
                    "name": name,
                    "interfaces": ["set", [["named-uuid", iface_uuid]]]
                },
                "uuid-name": port_uuid
            },
            {
                "op": "insert",
                "table": "Interface",
                "row": {
                    "name": name,
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

        self.transact("Open_vSwitch", operations).await?;
        info!("Created OVS bridge: {}", name);
        Ok(())
    }

    /// Delete a bridge
    pub async fn delete_bridge(&self, name: &str) -> Result<()> {
        let bridge_uuid = self.find_bridge_uuid(name).await?;

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

        self.transact("Open_vSwitch", operations).await?;
        info!("Deleted OVS bridge: {}", name);
        Ok(())
    }

    /// Add a port to a bridge
    pub async fn add_port(&self, bridge: &str, port: &str) -> Result<()> {
        let bridge_uuid = self.find_bridge_uuid(bridge).await?;
        let existing_ports = self.list_ports(bridge).await.unwrap_or_default();
        if existing_ports.iter().any(|p| p == port) {
            info!("Port {} already attached to bridge {}", port, bridge);
            return Ok(());
        }

        let existing_port_uuid = self.find_named_row_uuid("Port", port).await.ok();
        let existing_iface_uuid = self.find_named_row_uuid("Interface", port).await.ok();
        let safe_port = Self::sanitize_ref(port);
        let port_ref = format!("port_{}", safe_port);
        let iface_ref = format!("iface_{}", safe_port);

        let operations = if let Some(port_uuid) = existing_port_uuid {
            // Port exists but is not attached to this bridge yet.
            json!([
                {
                    "op": "mutate",
                    "table": "Bridge",
                    "where": [["_uuid", "==", ["uuid", &bridge_uuid]]],
                    "mutations": [
                        ["ports", "insert", ["set", [["uuid", &port_uuid]]]]
                    ]
                }
            ])
        } else if let Some(iface_uuid) = existing_iface_uuid {
            // Interface exists; create only Port row and attach it.
            json!([
                {
                    "op": "insert",
                    "table": "Port",
                    "row": {
                        "name": port,
                        "interfaces": ["set", [["uuid", &iface_uuid]]]
                    },
                    "uuid-name": port_ref
                },
                {
                    "op": "mutate",
                    "table": "Bridge",
                    "where": [["_uuid", "==", ["uuid", &bridge_uuid]]],
                    "mutations": [
                        ["ports", "insert", ["set", [["named-uuid", &port_ref]]]]
                    ]
                }
            ])
        } else {
            // Fresh system port.
            json!([
                {
                    "op": "insert",
                    "table": "Port",
                    "row": {
                        "name": port,
                        "interfaces": ["set", [["named-uuid", &iface_ref]]]
                    },
                    "uuid-name": port_ref
                },
                {
                    "op": "insert",
                    "table": "Interface",
                    "row": {
                        "name": port,
                        "type": "system"
                    },
                    "uuid-name": iface_ref
                },
                {
                    "op": "mutate",
                    "table": "Bridge",
                    "where": [["_uuid", "==", ["uuid", &bridge_uuid]]],
                    "mutations": [
                        ["ports", "insert", ["set", [["named-uuid", &port_ref]]]]
                    ]
                }
            ])
        };

        self.transact("Open_vSwitch", operations).await?;
        info!("Added port {} to bridge {}", port, bridge);
        Ok(())
    }

    /// List all bridges
    pub async fn list_bridges(&self) -> Result<Vec<String>> {
        let operations = json!([{
            "op": "select",
            "table": "Bridge",
            "where": [],
            "columns": ["name"]
        }]);

        let result = self.transact("Open_vSwitch", operations).await?;

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

    /// List ports on a bridge
    pub async fn list_ports(&self, bridge: &str) -> Result<Vec<String>> {
        let bridge_uuid = self.find_bridge_uuid(bridge).await?;

        let operations = json!([{
            "op": "select",
            "table": "Bridge",
            "where": [["_uuid", "==", ["uuid", &bridge_uuid]]],
            "columns": ["ports"]
        }]);

        let result = self.transact("Open_vSwitch", operations).await?;

        let mut port_uuids = Vec::new();
        if let Some(rows) = result[0]["rows"].as_array() {
            if let Some(first_row) = rows.first() {
                port_uuids = Self::extract_uuid_set(&first_row["ports"]);
            }
        }

        // Get port names
        let mut port_names = Vec::new();
        for port_uuid in port_uuids {
            let ops = json!([{
                "op": "select",
                "table": "Port",
                "where": [["_uuid", "==", ["uuid", &port_uuid]]],
                "columns": ["name"]
            }]);

            let result = self.transact("Open_vSwitch", ops).await?;
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

    /// Check if a bridge exists
    pub async fn bridge_exists(&self, name: &str) -> Result<bool> {
        match self.find_bridge_uuid(name).await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Get bridge info
    pub async fn get_bridge_info(&self, name: &str) -> Result<Value> {
        let bridge_uuid = self.find_bridge_uuid(name).await?;

        let operations = json!([{
            "op": "select",
            "table": "Bridge",
            "where": [["_uuid", "==", ["uuid", &bridge_uuid]]],
            "columns": []
        }]);

        let result = self.transact("Open_vSwitch", operations).await?;
        Ok(result[0]["rows"][0].clone())
    }

    /// Dump entire database
    pub async fn dump_db(&self, db: &str) -> Result<Value> {
        let schema = self.get_schema(db).await?;
        let tables = schema
            .get("tables")
            .and_then(|v| v.as_object())
            .ok_or_else(|| anyhow::anyhow!("Invalid schema: missing tables"))?;

        let table_names: Vec<String> = tables.keys().cloned().collect();
        let mut out = simd_json::value::owned::Object::new();

        for name in table_names {
            // Select each table independently so one failure doesn't abort the whole dump.
            let result = self
                .rpc_call(
                    "transact",
                    json!([db, {"op": "select", "table": name, "where": []}]),
                )
                .await;
            let rows = match result {
                Ok(r) => r
                    .as_array()
                    .and_then(|a| a.first())
                    .and_then(|r| r.get("rows"))
                    .cloned()
                    .unwrap_or_else(|| json!([])),
                Err(e) => {
                    tracing::warn!("dump_db: skipping table {}: {}", name, e);
                    json!([])
                }
            };
            out.insert(name, rows);
        }

        Ok(Value::Object(Box::new(out)))
    }

    /// Monitor a database for changes
    pub async fn monitor_db(&self, db: &str) -> Result<tokio::sync::mpsc::Receiver<Value>> {
        let mut stream = UnixStream::connect(&self.socket_path)
            .await
            .context("Failed to connect to OVSDB socket for monitoring")?;

        let schema = self.get_schema(db).await?;
        let tables = schema
            .get("tables")
            .and_then(|v| v.as_object())
            .ok_or_else(|| anyhow::anyhow!("Invalid schema: missing tables"))?;

        let mut monitor_requests = simd_json::value::owned::Object::new();
        for (name, _) in tables {
            monitor_requests.insert(
                name.clone(),
                json!({
                    "columns": [], // All columns
                    "select": {
                        "initial": true,
                        "insert": true,
                        "delete": true,
                        "modify": true
                    }
                }),
            );
        }

        let request = json!({
            "method": "monitor",
            "params": [db, null, Value::Object(Box::new(monitor_requests))],
            "id": "monitor"
        });

        let request_str = simd_json::to_string(&request)?;
        stream.write_all(request_str.as_bytes()).await?;
        stream.write_all(b"\n").await?;

        let (tx, rx) = tokio::sync::mpsc::channel(100);

        tokio::spawn(async move {
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            while let Ok(n) = reader.read_line(&mut line).await {
                if n == 0 {
                    break;
                }

                let mut line_clone = line.clone();
                if let Ok(update) = unsafe { simd_json::from_str::<Value>(line_clone.as_mut_str()) }
                {
                    if let Some(method) = update.get("method").and_then(|m| m.as_str()) {
                        if method == "update" && tx.send(update).await.is_err() {
                            break;
                        }
                    }
                }
                line.clear();
            }
        });

        Ok(rx)
    }

    /// Find bridge UUID by name
    pub async fn find_bridge_uuid(&self, name: &str) -> Result<String> {
        let operations = json!([{
            "op": "select",
            "table": "Bridge",
            "where": [["name", "==", name]],
            "columns": ["_uuid"]
        }]);

        let result = self.transact("Open_vSwitch", operations).await?;
        if let Some(rows) = result[0]["rows"].as_array() {
            if let Some(first_row) = rows.first() {
                if let Some(uuid_array) = first_row["_uuid"].as_array() {
                    if uuid_array.len() == 2 && uuid_array[0] == "uuid" {
                        return Ok(uuid_array[1].as_str().unwrap().to_string());
                    }
                }
            }
        }
        Err(anyhow::anyhow!("Bridge '{}' not found", name))
    }

    /// List ports on a bridge by name
    pub async fn list_bridge_ports(&self, bridge_name: &str) -> Result<Vec<String>> {
        let bridge_uuid = self.find_bridge_uuid(bridge_name).await?;

        let operations = json!([{
            "op": "select",
            "table": "Bridge",
            "where": [["_uuid", "==", ["uuid", &bridge_uuid]]],
            "columns": ["ports"]
        }]);

        let result = self.transact("Open_vSwitch", operations).await?;

        let mut port_uuids = Vec::new();
        if let Some(rows) = result[0]["rows"].as_array() {
            if let Some(first_row) = rows.first() {
                port_uuids = Self::extract_uuid_set(&first_row["ports"]);
            }
        }

        let mut port_names = Vec::new();
        for port_uuid in port_uuids {
            let ops = json!([{
                "op": "select",
                "table": "Port",
                "where": [["_uuid", "==", ["uuid", &port_uuid]]],
                "columns": ["name"]
            }]);

            let result = self.transact("Open_vSwitch", ops).await?;
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

    fn sanitize_ref(input: &str) -> String {
        input
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect()
    }

    fn extract_uuid_set(value: &Value) -> Vec<String> {
        // RFC7047 allows set columns to be encoded either as ["set", [...]]
        // or directly as a single atom (e.g. ["uuid", "..."]).
        if let Some(as_set) = value.as_array() {
            if as_set.len() == 2 && as_set[0] == "set" {
                if let Some(items) = as_set[1].as_array() {
                    return items
                        .iter()
                        .filter_map(Self::extract_uuid_atom)
                        .collect::<Vec<_>>();
                }
            }
        }
        Self::extract_uuid_atom(value).into_iter().collect()
    }

    pub fn extract_uuid_atom(value: &Value) -> Option<String> {
        let arr = value.as_array()?;
        if arr.len() == 2 && (arr[0] == "uuid" || arr[0] == "named-uuid") {
            return arr[1].as_str().map(|s| s.to_string());
        }
        None
    }

    async fn find_named_row_uuid(&self, table: &str, name: &str) -> Result<String> {
        let operations = json!([{
            "op": "select",
            "table": table,
            "where": [["name", "==", name]],
            "columns": ["_uuid"]
        }]);

        let result = self.transact("Open_vSwitch", operations).await?;
        if let Some(rows) = result[0]["rows"].as_array() {
            if let Some(first_row) = rows.first() {
                if let Some(uuid) = Self::extract_uuid_atom(&first_row["_uuid"]) {
                    return Ok(uuid);
                }
            }
        }
        Err(anyhow::anyhow!("{} '{}' not found", table, name))
    }
}

impl Default for OvsdbDbusClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio::net::UnixListener;

    #[test]
    fn parse_json_response_accepts_plain_json() {
        let parsed = OvsdbDbusClient::parse_json_response(
            r#"{"result":["Open_vSwitch"],"error":null,"id":0}"#,
        )
        .expect("parse response");
        assert_eq!(parsed["id"], 0);
        assert_eq!(parsed["result"][0], "Open_vSwitch");
    }

    #[test]
    fn parse_json_response_falls_back_to_last_valid_line() {
        let parsed = OvsdbDbusClient::parse_json_response(
            "noise line\n{\"result\":[\"Open_vSwitch\"],\"error\":null,\"id\":0}\n",
        )
        .expect("parse response");
        assert_eq!(parsed["result"][0], "Open_vSwitch");
    }

    #[test]
    fn extract_uuid_set_supports_singleton_atom() {
        let value = json!(["uuid", "abc"]);
        let uuids = OvsdbDbusClient::extract_uuid_set(&value);
        assert_eq!(uuids, vec!["abc".to_string()]);
    }

    #[test]
    fn extract_uuid_set_supports_set_encoding() {
        let value = json!(["set", [["uuid", "a"], ["uuid", "b"]]]);
        let uuids = OvsdbDbusClient::extract_uuid_set(&value);
        assert_eq!(uuids, vec!["a".to_string(), "b".to_string()]);
    }

    #[tokio::test]
    async fn rpc_call_handles_response_without_trailing_newline() {
        let socket_path = unique_test_socket_path();
        let _ = std::fs::remove_file(&socket_path);

        let listener = UnixListener::bind(&socket_path).expect("bind unix listener");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut request_buf = [0_u8; 1024];
            let _ = socket.read(&mut request_buf).await.expect("read request");

            let response = r#"{"result":["Open_vSwitch","_Server"],"error":null,"id":0}"#;
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write response");
            socket.shutdown().await.expect("shutdown socket");
        });

        let client = OvsdbDbusClient::with_socket(socket_path.to_string_lossy().to_string())
            .with_timeout(Duration::from_secs(2));
        let dbs = client.list_dbs().await.expect("list dbs");
        assert_eq!(dbs, vec!["Open_vSwitch".to_string(), "_Server".to_string()]);

        server.await.expect("server task");
        let _ = std::fs::remove_file(&socket_path);
    }

    fn unique_test_socket_path() -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("op-jsonrpc-ovsdb-{}.sock", uuid::Uuid::new_v4()));
        path
    }
}
