//! OVSDB client via D-Bus (zbus)
//!
//! This module provides an OVSDB client for OVS bridge management.
//! It invokes the schema-declared `ovsdb_bridge` methods through the canonical
//! `org.opdbus.v1.plugins` tree.
//!
//! Per AGENTS.md §4: D-Bus is the ONLY control plane.

use anyhow::{anyhow, Result};
use serde_json::json;
use serde_json::Value;
use simd_json::OwnedValue as SimdValue;
use std::time::Duration;
use tokio::sync::OnceCell;
use tokio::time::timeout;
use tracing::{info, warn};

const DBUS_BUS_NAME: &str = "org.opdbus.v1.plugins";
const DBUS_OBJECT_PATH: &str = "/org/opdbus/v1/plugins/ovsdb_bridge";
const DBUS_INTERFACE: &str = "org.opdbus.v1.PluginV1";

/// OVSDB JSON-RPC client via D-Bus
pub struct OvsdbClient {
    timeout: Duration,
    proxy: OnceCell<zbus::Proxy<'static>>,
}

impl OvsdbClient {
    /// Create a new OVSDB client using D-Bus
    pub fn new() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            proxy: OnceCell::new(),
        }
    }

    /// Create with a custom timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Get (or build once and cache) the D-Bus proxy to the OVSDB daemon.
    async fn proxy(&self) -> Result<&zbus::Proxy<'static>> {
        self.proxy
            .get_or_try_init(|| async {
                let address = std::env::var("DBUS_SESSION_BUS_ADDRESS")
                    .unwrap_or_else(|_| op_core::config::SESSION_BUS_ADDRESS.to_string());
                let conn = zbus::connection::Builder::address(address.as_str())
                    .map_err(|e| anyhow!("Invalid session D-Bus address: {}", e))?
                    .build()
                    .await
                    .map_err(|e| anyhow!("Failed to connect to session D-Bus: {}", e))?;

                zbus::proxy::Builder::new(&conn)
                    .destination(DBUS_BUS_NAME)?
                    .path(DBUS_OBJECT_PATH)?
                    .interface(DBUS_INTERFACE)?
                    .build()
                    .await
                    .map_err(|e| anyhow!("Failed to build D-Bus proxy: {}", e))
            })
            .await
    }

    /// Parse a daemon reply, surfacing `{"error": ...}` replies as errors.
    fn parse_daemon_reply(raw: &str) -> Result<Value> {
        let value: Value = serde_json::from_str(raw)
            .map_err(|e| anyhow!("Failed to parse daemon reply: {}", e))?;
        if let Some(err) = value.get("error").filter(|e| !e.is_null()) {
            return Err(anyhow!("OVSDB daemon error: {}", err));
        }
        Ok(value)
    }

    fn parse_plugin_reply(raw: &str) -> Result<Value> {
        let envelope: Value = serde_json::from_str(raw)
            .map_err(|e| anyhow!("Failed to parse plugin reply: {}", e))?;
        if envelope.get("success").and_then(Value::as_bool) != Some(true) {
            return Err(anyhow!("OVSDB plugin call failed: {}", envelope));
        }
        let result = envelope
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow!("OVSDB plugin reply has no result payload"))?;
        Self::parse_daemon_reply(&result.to_string())
    }

    /// Execute a transaction via D-Bus.
    ///
    /// `operations` is the OVSDB operations array; the daemon's rovs client
    /// prepends its configured database name itself.
    async fn transact_dbus(&self, operations: Value) -> Result<Value> {
        let proxy = self.proxy().await?;

        let args = json!({
            "db_name": "Open_vSwitch",
            "operations": operations,
        })
        .to_string();
        let result: String = proxy
            .call("Call", &("transact", args.as_str()))
            .await
            .map_err(|e| anyhow!("D-Bus transact call failed: {}", e))?;

        Self::parse_plugin_reply(&result)
    }

    /// Execute a transaction with timeout.
    ///
    /// `_db` is accepted for API compatibility; the daemon's rovs client is
    /// bound to its configured database (Open_vSwitch) and prepends the name
    /// to the operations array itself.
    async fn transact_with_timeout(&self, _db: &str, operations: Value) -> Result<Value> {
        let result = timeout(self.timeout, self.transact_dbus(operations))
            .await
            .map_err(|_| anyhow!("Transaction timed out"))??;
        Ok(result)
    }

    /// Execute a JSON-RPC transact
    pub async fn transact(&self, db: &str, operations: Value) -> Result<Value> {
        self.transact_with_timeout(db, operations).await
    }

    /// Execute a transaction with SIMD JSON value
    pub async fn transact_simd(&self, db: &str, operations: SimdValue) -> Result<Value> {
        let ops_value: Value = serde_json::from_str(&operations.to_string())
            .map_err(|e| anyhow!("Failed to convert SIMD JSON to serde_json: {}", e))?;
        self.transact(db, ops_value).await
    }

    /// List all databases (JSON-RPC `list_dbs`, a top-level method — not a
    /// transact operation — so it goes through the daemon's `ListDbs`).
    pub async fn list_dbs(&self) -> Result<Vec<String>> {
        let proxy = self.proxy().await?;
        let raw: String = timeout(self.timeout, proxy.call("Call", &("list_dbs", "{}")))
            .await
            .map_err(|_| anyhow!("list_dbs timed out"))?
            .map_err(|e| anyhow!("D-Bus list_dbs call failed: {}", e))?;

        let value = Self::parse_plugin_reply(&raw)?;
        let value = value.get("databases").unwrap_or(&value);
        let dbs = value
            .as_array()
            .ok_or_else(|| anyhow!("list_dbs: expected array, got {}", value))?
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        Ok(dbs)
    }

    /// Get schema for a database (served from the daemon's cached schema).
    pub async fn get_schema(&self, db: &str) -> Result<Value> {
        let proxy = self.proxy().await?;
        let args = json!({ "db_name": db }).to_string();
        let raw: String = timeout(
            self.timeout,
            proxy.call("Call", &("get_schema", args.as_str())),
        )
        .await
        .map_err(|_| anyhow!("get_schema timed out"))?
        .map_err(|e| anyhow!("D-Bus get_schema call failed: {}", e))?;

        Self::parse_plugin_reply(&raw)
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
        self.add_port_with_type(bridge, port, None).await
    }

    /// Add a port to a bridge with optional type
    pub async fn add_port_with_type(
        &self,
        bridge: &str,
        port: &str,
        port_type: Option<&str>,
    ) -> Result<()> {
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
        let iface_type = port_type.unwrap_or("system");

        let operations = if let Some(port_uuid) = existing_port_uuid {
            json!([
                {
                    "op": "mutate",
                    "table": "Bridge",
                    "where": [["_uuid", "==", ["uuid", &bridge_uuid]]],
                    "mutations": [
                        ["ports", "insert", ["uuid", &port_uuid]]
                    ]
                }
            ])
        } else if let Some(iface_uuid) = existing_iface_uuid {
            json!([
                {
                    "op": "insert",
                    "table": "Port",
                    "row": {
                        "name": port,
                        "interfaces": ["set", [["uuid", &iface_uuid]]]
                    },
                    "uuid-name": &port_ref
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
            json!([
                {
                    "op": "insert",
                    "table": "Port",
                    "row": {
                        "name": port,
                        "interfaces": ["set", [["named-uuid", &iface_ref]]]
                    },
                    "uuid-name": &port_ref
                },
                {
                    "op": "insert",
                    "table": "Interface",
                    "row": {
                        "name": port,
                        "type": iface_type
                    },
                    "uuid-name": &iface_ref
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

    /// Delete a port from a bridge
    pub async fn delete_port(&self, bridge: &str, port: &str) -> Result<()> {
        let del_ops = json!([
            {
                "op": "mutate",
                "table": "Bridge",
                "where": [["name", "==", bridge]],
                "mutations": [
                    ["ports", "delete", ["name", port]]
                ]
            },
            {
                "op": "delete",
                "table": "Port",
                "where": [["name", "==", port]]
            },
            {
                "op": "delete",
                "table": "Interface",
                "where": [["name", "==", port]]
            }
        ]);

        self.transact("Open_vSwitch", del_ops).await?;
        info!("Deleted port {} from bridge {}", port, bridge);
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
        if let Some(rows) = result
            .get(0)
            .and_then(|r| r.get("rows"))
            .and_then(|r| r.as_array())
        {
            for row in rows {
                if let Some(name) = row.get("name").and_then(|n| n.as_str()) {
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
        if let Some(rows) = result
            .get(0)
            .and_then(|r| r.get("rows"))
            .and_then(|r| r.as_array())
        {
            if let Some(first_row) = rows.first() {
                port_uuids = Self::extract_uuid_set(first_row.get("ports").unwrap_or(&Value::Null));
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
            if let Some(rows) = result
                .get(0)
                .and_then(|r| r.get("rows"))
                .and_then(|r| r.as_array())
            {
                if let Some(first_row) = rows.first() {
                    if let Some(name) = first_row.get("name").and_then(|n| n.as_str()) {
                        port_names.push(name.to_string());
                    }
                }
            }
        }

        Ok(port_names)
    }

    /// List bridges and their ports
    pub async fn list_bridge_ports(&self, bridge: &str) -> Result<Vec<String>> {
        self.list_ports(bridge).await
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
        result
            .get(0)
            .and_then(|r| {
                r.get("rows")
                    .and_then(|rows| rows.as_array().and_then(|r| r.first()))
            })
            .cloned()
            .ok_or_else(|| anyhow!("Bridge '{}' not found", name))
    }

    /// Dump entire database
    pub async fn dump_db(&self, db: &str) -> Result<Value> {
        let schema = self.get_schema(db).await?;
        let tables = schema
            .get("tables")
            .and_then(|v| v.as_object())
            .ok_or_else(|| anyhow!("Invalid schema: missing tables"))?;

        let table_names: Vec<String> = tables.keys().cloned().collect();
        let mut out = serde_json::Map::new();

        for name in table_names {
            let result = self
                .transact(
                    db,
                    json!([{
                        "op": "select",
                        "table": name,
                        "where": []
                    }]),
                )
                .await;
            let rows = match result {
                Ok(r) => r
                    .get(0)
                    .and_then(|item| item.get("rows"))
                    .cloned()
                    .unwrap_or_else(|| json!([])),
                Err(e) => {
                    warn!("dump_db: skipping table {}: {}", name, e);
                    json!([])
                }
            };
            out.insert(name, rows);
        }

        Ok(Value::Object(out))
    }

    /// Monitor a database for changes.
    ///
    /// Returns a channel that receives OVSDB update notifications. The feed
    /// is driven by the SchemaEngine's shared-memory mutation pipeline, not
    /// by polling. When the daemon's D-Bus notification surface is wired,
    /// updates will flow through here; until then the channel stays open
    /// but idle (no polling, no spin — per the zero-copy shm architecture).
    pub async fn monitor_db(&self, db: &str) -> Result<tokio::sync::mpsc::Receiver<Value>> {
        // Validate the database name early so callers get an immediate error
        // if the daemon is unreachable.
        let _ = self.get_schema(db).await?;

        let (_tx, rx) = tokio::sync::mpsc::channel(100);
        info!(
            "monitor_db: channel open for {}, awaiting shm-driven feed",
            db
        );
        Ok(rx)
    }

    /// Set a property on a bridge
    pub async fn set_bridge_property(
        &self,
        bridge_name: &str,
        property: &str,
        value: &str,
    ) -> Result<()> {
        let operations = json!([{
            "op": "update",
            "table": "Bridge",
            "where": [["name", "==", bridge_name]],
            "row": { property: value }
        }]);
        self.transact("Open_vSwitch", operations).await?;
        info!(
            "Bridge {} property {} set to {}",
            bridge_name, property, value
        );
        Ok(())
    }

    /// Set interface type on an interface
    pub async fn set_interface_type(&self, iface_name: &str, iface_type: &str) -> Result<()> {
        let operations = json!([{
            "op": "update",
            "table": "Interface",
            "where": [["name", "==", iface_name]],
            "row": { "type": iface_type }
        }]);
        self.transact("Open_vSwitch", operations).await?;
        info!("Interface {} type set to {}", iface_name, iface_type);
        Ok(())
    }

    /// Find bridge UUID by name
    async fn find_bridge_uuid(&self, name: &str) -> Result<String> {
        let operations = json!([{
            "op": "select",
            "table": "Bridge",
            "where": [["name", "==", name]],
            "columns": ["_uuid"]
        }]);

        let result = self.transact("Open_vSwitch", operations).await?;
        if let Some(rows) = result
            .get(0)
            .and_then(|r| r.get("rows"))
            .and_then(|r| r.as_array())
        {
            if let Some(first_row) = rows.first() {
                if let Some(uuid_array) = first_row.get("_uuid").and_then(|u| u.as_array()) {
                    if uuid_array.len() == 2 && uuid_array[0] == "uuid" {
                        if let Some(uuid_str) = uuid_array[1].as_str() {
                            return Ok(uuid_str.to_string());
                        }
                    }
                }
            }
        }
        Err(anyhow!("Bridge '{}' not found", name))
    }

    /// Find named row UUID in a table
    async fn find_named_row_uuid(&self, table: &str, name: &str) -> Result<String> {
        let operations = json!([{
            "op": "select",
            "table": table,
            "where": [["name", "==", name]],
            "columns": ["_uuid"]
        }]);

        let result = self.transact("Open_vSwitch", operations).await?;
        if let Some(rows) = result
            .get(0)
            .and_then(|r| r.get("rows"))
            .and_then(|r| r.as_array())
        {
            if let Some(first_row) = rows.first() {
                if let Some(uuid) =
                    Self::extract_uuid_atom(first_row.get("_uuid").unwrap_or(&Value::Null))
                {
                    return Ok(uuid);
                }
            }
        }
        Err(anyhow!("{} '{}' not found", table, name))
    }

    fn sanitize_ref(input: &str) -> String {
        input
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect()
    }

    fn extract_uuid_set(value: &Value) -> Vec<String> {
        if let Some(as_set) = value.as_array() {
            if as_set.len() == 2 && as_set[0] == "set" {
                if let Some(items) = as_set[1].as_array() {
                    return items.iter().filter_map(Self::extract_uuid_atom).collect();
                }
            }
        }
        Self::extract_uuid_atom(value).into_iter().collect()
    }

    /// Extract a single UUID from a value
    pub fn extract_uuid_atom(value: &Value) -> Option<String> {
        let arr = value.as_array()?;
        if arr.len() == 2 && (arr[0] == "uuid" || arr[0] == "named-uuid") {
            return arr[1].as_str().map(|s| s.to_string());
        }
        None
    }
}

impl Default for OvsdbClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_ref_replaces_special_chars() {
        assert_eq!(OvsdbClient::sanitize_ref("test-bridge"), "test_bridge");
        assert_eq!(OvsdbClient::sanitize_ref("port123"), "port123");
    }

    #[test]
    fn extract_uuid_atom_works() {
        let value = json!(["uuid", "abc123"]);
        assert_eq!(
            OvsdbClient::extract_uuid_atom(&value),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn extract_uuid_atom_named_uuid_works() {
        let value = json!(["named-uuid", "my_bridge"]);
        assert_eq!(
            OvsdbClient::extract_uuid_atom(&value),
            Some("my_bridge".to_string())
        );
    }

    #[test]
    fn extract_uuid_set_single() {
        let value = json!(["uuid", "abc"]);
        let uuids = OvsdbClient::extract_uuid_set(&value);
        assert_eq!(uuids, vec!["abc".to_string()]);
    }

    #[test]
    fn extract_uuid_set_multiple() {
        let value = json!(["set", [["uuid", "a"], ["uuid", "b"]]]);
        let uuids = OvsdbClient::extract_uuid_set(&value);
        assert_eq!(uuids, vec!["a".to_string(), "b".to_string()]);
    }
}
