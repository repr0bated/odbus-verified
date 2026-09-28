//! OVSDB access for the workspace.
//!
//! `OvsdbDbusClient` (name kept for call-site compatibility) talks to
//! `ovsdb-server` natively via the vendor `rovs_ovsdb::Client` — no D-Bus hop.
//! The dead raw JSON-RPC D-Bus proxy was removed; compatibility callers enter
//! through the schema-declared `ovsdb_bridge` object on the canonical plugin
//! tree, while the audited bridge backend uses this native client.
//!
//! NOTE: the OpenFlow passthrough half of this module (`RovsOpenFlow`,
//! `openflow_proxy`, `ensure_proxies`) was removed along with
//! `op-openvswitch-daemon` (deprecated, purged — see CLAUDE.md). OpenFlow
//! control now runs entirely through `op-network::openflow::OpenFlowClient`
//! (direct TCP, no D-Bus hop) and the passive `op-of-controller` service in
//! `crates/op-network/src/controller.rs` — the same pattern OVSDB now follows.

use anyhow::{Context, Result};
use std::{collections::HashMap, sync::Arc};

#[path = "ovsdb_monitor.rs"]
pub mod monitor;

#[cfg(test)]
#[path = "rovs_proxy_native_tests.rs"]
mod native_tests;

// ── OvsdbDbusClient ─────────────────────────────────────────────────────────

/// Default OVSDB socket address, in `rovs_transport::Address` syntax, matching
/// `ovs_capabilities.rs`'s `ovsdb_socket_path`.
const DEFAULT_OVSDB_ADDR: &str = "unix:/var/run/openvswitch/db.sock";

/// High-level OVSDB client — wraps the real vendor `rovs_ovsdb::Client`
/// (crates.io `rovs-ovsdb`, already a workspace dependency and already used
/// natively — no D-Bus hop — by `op-network::openflow` via `rovs_openflow`/
/// `rovs_transport`). No separate D-Bus passthrough daemon exists for OVSDB;
/// one here would (and did, before this fix) target a service that was never
/// built. `rovs_ovsdb::Client` needs `&mut self`, so the connection is held
/// behind a mutex and shared/reconnected lazily.
#[derive(Clone, Default)]
pub struct OvsdbDbusClient {
    client: Arc<tokio::sync::Mutex<Option<rovs_ovsdb::Client>>>,
    locks: Arc<tokio::sync::Mutex<HashMap<(String, String), LockSession>>>,
    monitors: Arc<tokio::sync::Mutex<HashMap<(String, String), monitor::MonitorSubscription>>>,
}

struct LockSession {
    connection: rovs_jsonrpc::Connection,
    initially_locked: bool,
}

pub struct ProtocolMonitorStart {
    pub initial: Arc<monitor::ProtocolSnapshot>,
    pub receiver: tokio::sync::watch::Receiver<monitor::ProtocolObservation>,
}

#[derive(Debug, Clone)]
pub struct LockReceipt {
    pub lock_id: String,
    pub locked: bool,
    pub queued: bool,
}

#[derive(Debug, Clone)]
pub struct UnlockReceipt {
    pub lock_id: String,
    pub unlocked: bool,
    pub acquired_while_queued: bool,
    pub stolen_before_unlock: bool,
}

const RPC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const MAX_RETAINED_LOCKS: usize = 64;
const MAX_RETAINED_MONITORS: usize = 64;

fn validate_session_key(owner: &str, id: &str) -> Result<()> {
    anyhow::ensure!(
        !owner.is_empty() && owner.len() <= 256,
        "invalid OVSDB caller scope"
    );
    anyhow::ensure!(
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')),
        "invalid OVSDB session identifier"
    );
    Ok(())
}

/// Lock, steal and unlock take an RFC 7047 §3.1 `<id>`: a JSON string
/// matching `[_a-zA-Z][_a-zA-Z0-9]*`. ovsdb-server rejects anything else
/// with "lock request params must be <id>", so refuse it here with the reason.
/// Monitor ids are any JSON value and keep the permissive session-key rule.
fn validate_lock_key(owner: &str, lock_id: &str) -> Result<()> {
    validate_session_key(owner, lock_id)?;
    let mut bytes = lock_id.bytes();
    let valid_first = bytes
        .next()
        .is_some_and(|byte| byte == b'_' || byte.is_ascii_alphabetic());
    anyhow::ensure!(
        valid_first && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric()),
        "invalid OVSDB lock identifier {lock_id:?}: must match [_a-zA-Z][_a-zA-Z0-9]* (RFC 7047)"
    );
    Ok(())
}

fn retained_key(owner: &str, id: &str) -> (String, String) {
    (owner.to_owned(), id.to_owned())
}

impl OvsdbDbusClient {
    /// Kept sync to preserve the existing construction call sites; the vendor
    /// client connects lazily on first use.
    pub fn new() -> Self {
        Self {
            client: Arc::new(tokio::sync::Mutex::new(None)),
            locks: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            monitors: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Run `f` against a connected client, reconnecting once on failure (the
    /// socket may have been idle-closed by `ovsdb-server` between calls).
    async fn with_client<T, F>(&self, f: F) -> Result<T>
    where
        F: for<'a> Fn(
            &'a mut rovs_ovsdb::Client,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<T>> + Send + 'a>,
        >,
    {
        let addr =
            std::env::var("OVSDB_SOCKET_ADDR").unwrap_or_else(|_| DEFAULT_OVSDB_ADDR.to_string());
        let mut guard = self.client.lock().await;
        if guard.is_none() {
            let client = rovs_ovsdb::Client::connect(&addr)
                .await
                .with_context(|| format!("failed to connect to OVSDB at {addr}"))?;
            *guard = Some(client);
        }
        match f(guard.as_mut().expect("just ensured Some")).await {
            Ok(val) => Ok(val),
            Err(_first_err) => {
                // Reconnect once and retry — covers idle-closed sockets.
                let client = rovs_ovsdb::Client::connect(&addr)
                    .await
                    .with_context(|| format!("failed to reconnect to OVSDB at {addr}"))?;
                *guard = Some(client);
                f(guard.as_mut().expect("just ensured Some")).await
            }
        }
    }

    // ── Internal: build & send a transact ───────────────────────────────

    async fn transact_one(&self, op: serde_json::Value) -> Result<serde_json::Value> {
        // rovs_ovsdb::Client::transact() prepends the configured database name
        // itself, so operations here are passed WITHOUT it (unlike the raw
        // JSON-RPC ["Open_vSwitch", op, ...] wire shape).
        let result = self
            .with_client(move |client| {
                let op = op.clone();
                Box::pin(async move {
                    client
                        .transact(serde_json::Value::Array(vec![op]))
                        .await
                        .context("OVSDB transact failed")
                })
            })
            .await?;
        // RFC 7047: `result` is an array of one entry per input operation, in
        // order. Callers of transact_one all expect the single op's result
        // object directly (e.g. `result.get("rows")`), so unwrap here once
        // rather than in every caller.
        Self::check_errors(&result)?;
        Ok(result
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }

    async fn transact_many(&self, ops: Vec<serde_json::Value>) -> Result<serde_json::Value> {
        self.with_client(move |client| {
            let ops = ops.clone();
            Box::pin(async move {
                client
                    .transact(serde_json::Value::Array(ops))
                    .await
                    .context("OVSDB transact failed")
            })
        })
        .await
    }

    // ── Read helpers ──────────────────────────────────────────────────────

    /// Return `true` if the daemon (and OVSDB) is reachable.
    pub async fn list_dbs(&self) -> Result<Vec<String>> {
        let val = self
            .with_client(|client| {
                Box::pin(async move { client.list_dbs().await.context("OVSDB list_dbs failed") })
            })
            .await?;
        Ok(val)
    }

    /// Return the schema cached by the connected vendor client.
    pub async fn get_schema(&self) -> Result<serde_json::Value> {
        self.with_client(|client| {
            Box::pin(async move {
                let schema = client
                    .schema()
                    .ok_or_else(|| anyhow::anyhow!("OVSDB schema is not loaded"))?;
                serde_json::to_value(schema).context("serialize OVSDB schema")
            })
        })
        .await
    }

    /// Return `true` if a bridge with the given name exists.
    pub async fn bridge_exists(&self, bridge_name: &str) -> Result<bool> {
        let result = self
            .transact_one(serde_json::json!({
                "op": "select",
                "table": "Bridge",
                "where": [["name", "==", bridge_name]],
                "columns": ["_uuid"]
            }))
            .await?;
        Ok(result
            .get("rows")
            .and_then(|r| r.as_array())
            .map(|a| !a.is_empty())
            .unwrap_or(false))
    }

    /// Return the names of all bridges.
    pub async fn list_bridges(&self) -> Result<Vec<String>> {
        let result = self
            .transact_one(serde_json::json!({
                "op": "select",
                "table": "Bridge",
                "where": [],
                "columns": ["name"]
            }))
            .await?;
        Ok(result
            .get("rows")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|r| r.get("name").and_then(|v| v.as_str()).map(String::from))
            .collect())
    }

    /// Return the names of all ports on a bridge.
    pub async fn list_bridge_ports(&self, bridge_name: &str) -> Result<Vec<String>> {
        let bridge_result = self
            .transact_one(serde_json::json!({
                "op": "select",
                "table": "Bridge",
                "where": [["name", "==", bridge_name]],
                "columns": ["ports"]
            }))
            .await?;
        let bridge_rows = bridge_result.get("rows").and_then(|r| r.as_array());
        let port_uuids: Vec<String> = match bridge_rows {
            Some(rows) if !rows.is_empty() => {
                let mut uuids = Vec::new();
                if let Some(ports) = rows[0].get("ports") {
                    Self::collect_uuids(ports, &mut uuids);
                }
                uuids
            }
            _ => return Ok(Vec::new()),
        };

        let mut names = Vec::new();
        for uuid in port_uuids {
            let result = self
                .transact_one(serde_json::json!({
                    "op": "select",
                    "table": "Port",
                    "where": [["_uuid", "==", ["uuid", uuid]]],
                    "columns": ["name"]
                }))
                .await?;
            if let Some(row) = result
                .get("rows")
                .and_then(|r| r.as_array())
                .and_then(|a| a.first())
                && let Some(n) = row.get("name").and_then(|v| v.as_str())
            {
                names.push(n.to_string());
            }
        }
        Ok(names)
    }

    /// Return the raw JSON row for a bridge.
    pub async fn get_bridge_info(&self, bridge_name: &str) -> Result<String> {
        let result = self
            .transact_one(serde_json::json!({
                "op": "select",
                "table": "Bridge",
                "where": [["name", "==", bridge_name]],
                "columns": []
            }))
            .await?;
        Ok(serde_json::to_string_pretty(&result)?)
    }

    // ── Mutation helpers ──────────────────────────────────────────────────

    /// Create a bridge if it does not exist.
    pub async fn create_bridge(&self, bridge_name: &str) -> Result<()> {
        if self.bridge_exists(bridge_name).await? {
            log::info!("Bridge {} already exists, skipping creation", bridge_name);
            return Ok(());
        }
        let ops = vec![
            serde_json::json!({
                "op": "insert",
                "table": "Bridge",
                "row": { "name": bridge_name, "stp_enable": false },
                "uuid-name": "new_bridge"
            }),
            serde_json::json!({
                "op": "mutate",
                "table": "Open_vSwitch",
                "where": [],
                "mutations": [["bridges", "insert", ["named-uuid", "new_bridge"]]]
            }),
        ];
        let result = self.transact_many(ops).await?;
        Self::check_errors(&result)?;
        log::info!("Bridge {} created via D-Bus daemon", bridge_name);
        Ok(())
    }

    /// Delete a bridge and its ports/interfaces.
    pub async fn delete_bridge(&self, bridge_name: &str) -> Result<()> {
        let bridge_result = self
            .transact_one(serde_json::json!({
                "op": "select",
                "table": "Bridge",
                "where": [["name", "==", bridge_name]],
                "columns": ["_uuid", "ports"]
            }))
            .await?;
        let bridge_rows = bridge_result.get("rows").and_then(|r| r.as_array());
        let (bridge_uuid, bridge_row) = match bridge_rows {
            Some(rows) if !rows.is_empty() => {
                let uuid = rows[0]
                    .get("_uuid")
                    .and_then(|u| u.as_array())
                    .and_then(|a| a.get(1))
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .ok_or_else(|| anyhow::anyhow!("bridge UUID not found"))?;
                (uuid, &rows[0])
            }
            _ => return Err(anyhow::anyhow!("Bridge '{}' not found", bridge_name)),
        };

        let mut port_uuids: Vec<String> = Vec::new();
        if let Some(ports) = bridge_row.get("ports") {
            Self::collect_uuids(ports, &mut port_uuids);
        }

        let mut iface_uuids: Vec<String> = Vec::new();
        for port_uuid in &port_uuids {
            let port_result = self
                .transact_one(serde_json::json!({
                    "op": "select",
                    "table": "Port",
                    "where": [["_uuid", "==", ["uuid", port_uuid.clone()]]],
                    "columns": ["interfaces"]
                }))
                .await?;
            if let Some(rows) = port_result.get("rows").and_then(|r| r.as_array())
                && let Some(row) = rows.first()
                && let Some(ifaces) = row.get("interfaces")
            {
                Self::collect_uuids(ifaces, &mut iface_uuids);
            }
        }

        let mut ops = vec![
            serde_json::json!({
                "op": "mutate",
                "table": "Open_vSwitch",
                "where": [],
                "mutations": [["bridges", "delete", ["uuid", bridge_uuid.clone()]]]
            }),
            serde_json::json!({
                "op": "delete",
                "table": "Bridge",
                "where": [["_uuid", "==", ["uuid", bridge_uuid]]]
            }),
        ];
        for port_uuid in &port_uuids {
            ops.push(serde_json::json!({
                "op": "delete",
                "table": "Port",
                "where": [["_uuid", "==", ["uuid", port_uuid]]]
            }));
        }
        for iface_uuid in &iface_uuids {
            ops.push(serde_json::json!({
                "op": "delete",
                "table": "Interface",
                "where": [["_uuid", "==", ["uuid", iface_uuid]]]
            }));
        }

        let result = self.transact_many(ops).await?;
        Self::check_errors(&result)?;
        log::info!("Bridge {} deleted via D-Bus daemon", bridge_name);
        Ok(())
    }

    /// Add a system port to a bridge.
    pub async fn add_port(&self, bridge_name: &str, port_name: &str) -> Result<()> {
        let ops = vec![
            serde_json::json!({
                "op": "insert",
                "table": "Interface",
                "row": { "name": port_name, "type": "system" },
                "uuid-name": "new_iface"
            }),
            serde_json::json!({
                "op": "insert",
                "table": "Port",
                "row": {
                    "name": port_name,
                    "interfaces": ["set", [["named-uuid", "new_iface"]]]
                },
                "uuid-name": "new_port"
            }),
            serde_json::json!({
                "op": "mutate",
                "table": "Bridge",
                "where": [["name", "==", bridge_name]],
                "mutations": [["ports", "insert", ["named-uuid", "new_port"]]]
            }),
        ];
        let result = self.transact_many(ops).await?;
        Self::check_errors(&result)?;
        log::info!(
            "Port {} added to bridge {} via D-Bus daemon",
            port_name,
            bridge_name
        );
        Ok(())
    }

    // ── Private helpers ───────────────────────────────────────────────────

    /// Recursively collect UUID strings from an OVSDB set value.
    fn collect_uuids(value: &serde_json::Value, out: &mut Vec<String>) {
        if let Some(arr) = value.as_array()
            && arr.len() == 2
        {
            if arr[0] == "uuid" {
                if let Some(s) = arr[1].as_str() {
                    out.push(s.to_string());
                }
            } else if arr[0] == "set"
                && let Some(items) = arr[1].as_array()
            {
                for item in items {
                    Self::collect_uuids(item, out);
                }
            }
        }
    }

    /// Check a transact result for per-operation error objects.
    fn check_errors(result: &serde_json::Value) -> Result<()> {
        if let Some(results) = result.as_array() {
            for (i, op_result) in results.iter().enumerate() {
                if let Some(error) = op_result.get("error")
                    && !error.is_null()
                {
                    let details = op_result
                        .get("details")
                        .and_then(|d| d.as_str())
                        .unwrap_or("no details");
                    return Err(anyhow::anyhow!(
                        "OVSDB operation {} failed: {} ({})",
                        i,
                        error,
                        details
                    ));
                }
            }
        }
        Ok(())
    }

    /// Set the `type` column on an Interface row (e.g. "internal", "system").
    pub async fn set_interface_type(&self, iface_name: &str, iface_type: &str) -> Result<()> {
        let ops = vec![serde_json::json!({
            "op": "update",
            "table": "Interface",
            "where": [["name", "==", iface_name]],
            "row": { "type": iface_type }
        })];
        let result = self.transact_many(ops).await?;
        Self::check_errors(&result)?;
        log::info!(
            "Interface {} type set to {} via D-Bus daemon",
            iface_name,
            iface_type
        );
        Ok(())
    }

    /// Set a bridge property (e.g., "datapath_type", "fail_mode").
    pub async fn set_bridge_property(
        &self,
        bridge_name: &str,
        property: &str,
        value: &str,
    ) -> Result<()> {
        let ops = vec![serde_json::json!({
            "op": "update",
            "table": "Bridge",
            "where": [["name", "==", bridge_name]],
            "row": { property: value }
        })];
        let result = self.transact_many(ops).await?;
        Self::check_errors(&result)?;
        log::info!(
            "Bridge {} property {} set to {} via D-Bus daemon",
            bridge_name,
            property,
            value
        );
        Ok(())
    }

    /// Delete a port from a bridge and remove associated Interface.
    pub async fn delete_port(&self, bridge_name: &str, port_name: &str) -> Result<()> {
        // First get the port UUID and its interface UUID
        let port_result = self
            .transact_one(serde_json::json!({
                "op": "select",
                "table": "Port",
                "where": [["name", "==", port_name]],
                "columns": ["_uuid", "interfaces"]
            }))
            .await?;

        let port_uuid = port_result
            .get("rows")
            .and_then(|r| r.as_array())
            .and_then(|a| a.first())
            .and_then(|row| row.get("_uuid"))
            .and_then(|u| u.as_array())
            .and_then(|a| a.get(1))
            .and_then(|u| u.as_str())
            .map(String::from)
            .ok_or_else(|| anyhow::anyhow!("Port '{}' not found", port_name))?;

        // Get interface UUIDs from port
        let mut iface_uuids: Vec<String> = Vec::new();
        if let Some(rows) = port_result.get("rows").and_then(|r| r.as_array())
            && let Some(row) = rows.first()
            && let Some(ifaces) = row.get("interfaces")
        {
            Self::collect_uuids(ifaces, &mut iface_uuids);
        }

        // Build delete operations: remove from bridge, delete port, delete interfaces
        let mut ops = vec![
            serde_json::json!({
                "op": "mutate",
                "table": "Bridge",
                "where": [["name", "==", bridge_name]],
                "mutations": [["ports", "delete", ["uuid", port_uuid.clone()]]]
            }),
            serde_json::json!({
                "op": "delete",
                "table": "Port",
                "where": [["_uuid", "==", ["uuid", port_uuid]]]
            }),
        ];

        for iface_uuid in iface_uuids {
            ops.push(serde_json::json!({
                "op": "delete",
                "table": "Interface",
                "where": [["_uuid", "==", ["uuid", iface_uuid]]]
            }));
        }

        let result = self.transact_many(ops).await?;
        Self::check_errors(&result)?;
        log::info!(
            "Port {} deleted from bridge {} via D-Bus daemon",
            port_name,
            bridge_name
        );
        Ok(())
    }

    /// Add a port with specific type to a bridge.
    pub async fn add_port_with_type(
        &self,
        bridge_name: &str,
        port_name: &str,
        port_type: Option<&str>,
    ) -> Result<()> {
        let ops = match port_type {
            Some(ptype) => vec![
                serde_json::json!({
                    "op": "insert",
                    "table": "Interface",
                    "row": { "name": port_name, "type": ptype },
                    "uuid-name": "new_iface"
                }),
                serde_json::json!({
                    "op": "insert",
                    "table": "Port",
                    "row": {
                        "name": port_name,
                        "interfaces": ["set", [["named-uuid", "new_iface"]]]
                    },
                    "uuid-name": "new_port"
                }),
                serde_json::json!({
                    "op": "mutate",
                    "table": "Bridge",
                    "where": [["name", "==", bridge_name]],
                    "mutations": [["ports", "insert", ["named-uuid", "new_port"]]]
                }),
            ],
            None => vec![
                serde_json::json!({
                    "op": "insert",
                    "table": "Interface",
                    "row": { "name": port_name },
                    "uuid-name": "new_iface"
                }),
                serde_json::json!({
                    "op": "insert",
                    "table": "Port",
                    "row": {
                        "name": port_name,
                        "interfaces": ["set", [["named-uuid", "new_iface"]]]
                    },
                    "uuid-name": "new_port"
                }),
                serde_json::json!({
                    "op": "mutate",
                    "table": "Bridge",
                    "where": [["name", "==", bridge_name]],
                    "mutations": [["ports", "insert", ["named-uuid", "new_port"]]]
                }),
            ],
        };

        let result = self.transact_many(ops).await?;
        Self::check_errors(&result)?;
        log::info!(
            "Port {} added to bridge {} via D-Bus daemon",
            port_name,
            bridge_name
        );
        Ok(())
    }

    /// Add a port while preserving the schema's distinct port/interface names
    /// and optional VLAN tag.
    pub async fn add_port_with_options(
        &self,
        bridge_name: &str,
        port_name: &str,
        interface_name: Option<&str>,
        tag: Option<i32>,
    ) -> Result<()> {
        let interface_name = interface_name.unwrap_or(port_name);
        let mut port_row = serde_json::json!({
            "name": port_name,
            "interfaces": ["set", [["named-uuid", "new_iface"]]]
        });
        if let Some(tag) = tag {
            anyhow::ensure!((0..=4095).contains(&tag), "OVS VLAN tag must be 0..=4095");
            port_row["tag"] = serde_json::json!(tag);
        }
        let result = self
            .transact_many(vec![
                serde_json::json!({
                    "op": "insert",
                    "table": "Interface",
                    "row": { "name": interface_name, "type": "system" },
                    "uuid-name": "new_iface"
                }),
                serde_json::json!({
                    "op": "insert",
                    "table": "Port",
                    "row": port_row,
                    "uuid-name": "new_port"
                }),
                serde_json::json!({
                    "op": "mutate",
                    "table": "Bridge",
                    "where": [["name", "==", bridge_name]],
                    "mutations": [["ports", "insert", ["named-uuid", "new_port"]]]
                }),
            ])
            .await?;
        Self::check_errors(&result)?;
        Ok(())
    }

    /// Set the boolean STP column on a bridge through a checked transaction.
    pub async fn set_bridge_stp(&self, bridge_name: &str, enabled: bool) -> Result<()> {
        let result = self
            .transact_many(vec![serde_json::json!({
                "op": "update",
                "table": "Bridge",
                "where": [["name", "==", bridge_name]],
                "row": { "stp_enable": enabled }
            })])
            .await?;
        Self::check_errors(&result)
    }

    /// Ensure an OVS internal Port/Interface pair exists on `bridge_name`.
    ///
    /// Existing ports are repaired to `type=internal`; new rows and their
    /// Bridge attachment are created in one RFC 7047 transaction.
    pub async fn ensure_internal_port(&self, bridge_name: &str, port_name: &str) -> Result<bool> {
        if !self.bridge_exists(bridge_name).await? {
            return Err(anyhow::anyhow!("Bridge '{}' not found", bridge_name));
        }
        if self
            .list_bridge_ports(bridge_name)
            .await?
            .iter()
            .any(|existing| existing == port_name)
        {
            self.set_interface_type(port_name, "internal").await?;
            log::info!(
                "Internal port {} already attached to bridge {}; type reconciled",
                port_name,
                bridge_name
            );
            return Ok(false);
        }

        self.add_port_with_type(bridge_name, port_name, Some("internal"))
            .await?;
        Ok(true)
    }

    /// Dump the contents of an OVSDB database as JSON.
    /// Returns a JSON object with database contents.
    pub async fn dump_db(&self, database: &str) -> Result<serde_json::Value> {
        // Query all tables in the database
        let _tables_result = self
            .transact_one(serde_json::json!({
                "op": "select",
                "table": "Open_vSwitch",
                "where": [],
                "columns": []
            }))
            .await?;

        // Build a dump structure with all tables
        let mut dump = serde_json::json!({
            "database": database,
            "tables": {}
        });

        // Get list of tables from the database schema
        let schema_result = self
            .transact_one(serde_json::json!({
                "op": "get_schema",
                "id": "dump"
            }))
            .await?;

        if let Some(tables) = schema_result.get("tables").and_then(|t| t.as_object()) {
            for table_name in tables.keys() {
                let table_data = self
                    .transact_one(serde_json::json!({
                        "op": "select",
                        "table": table_name,
                        "where": [],
                        "columns": []
                    }))
                    .await?;

                if let Some(rows) = table_data.get("rows") {
                    dump["tables"][table_name] = rows.clone();
                }
            }
        }

        Ok(dump)
    }

    /// Observe selected OVS topology columns through a dedicated native
    /// connection. Every observation is a complete IDL snapshot; coalesced
    /// notifications never require replaying table deltas. No mutation retry
    /// helper is used and dropping all receivers closes the connection.
    pub async fn monitor_db(
        &self,
        database: &str,
    ) -> Result<tokio::sync::watch::Receiver<monitor::MonitorObservation>> {
        let address =
            std::env::var("OVSDB_SOCKET_ADDR").unwrap_or_else(|_| DEFAULT_OVSDB_ADDR.to_owned());
        monitor::subscribe(&address, database).await
    }

    /// Issue one native OVSDB JSON-RPC request on a short-lived connection.
    /// This is reserved for RFC 7047 methods which are not transactions.
    pub async fn native_rpc(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let address =
            std::env::var("OVSDB_SOCKET_ADDR").unwrap_or_else(|_| DEFAULT_OVSDB_ADDR.to_owned());
        let address: rovs_transport::Address = address.parse().context("invalid OVSDB address")?;
        let stream = rovs_transport::Stream::connect(&address)
            .await
            .context("connect to OVSDB native RPC endpoint")?;
        let mut connection = rovs_jsonrpc::Connection::new(stream);
        tokio::time::timeout(RPC_TIMEOUT, connection.transact(method, params))
            .await
            .context("OVSDB native RPC timed out")?
            .map_err(anyhow::Error::from)
            .with_context(|| format!("OVSDB {method} request failed"))
    }

    /// Acquire and retain an RFC 7047 lock on a dedicated connection.
    pub async fn lock(&self, owner: &str, lock_id: &str) -> Result<LockReceipt> {
        validate_lock_key(owner, lock_id)?;
        let key = retained_key(owner, lock_id);
        {
            let locks = self.locks.lock().await;
            anyhow::ensure!(
                !locks.contains_key(&key),
                "OVSDB lock/steal must alternate with unlock"
            );
            anyhow::ensure!(
                locks.len() < MAX_RETAINED_LOCKS,
                "OVSDB retained lock limit reached"
            );
        }
        let address =
            std::env::var("OVSDB_SOCKET_ADDR").unwrap_or_else(|_| DEFAULT_OVSDB_ADDR.to_owned());
        let address: rovs_transport::Address = address.parse().context("invalid OVSDB address")?;
        let stream = tokio::time::timeout(RPC_TIMEOUT, rovs_transport::Stream::connect(&address))
            .await
            .context("connect to OVSDB lock endpoint timed out")?
            .context("connect to OVSDB lock endpoint")?;
        let mut connection = rovs_jsonrpc::Connection::new(stream);
        let result = tokio::time::timeout(
            RPC_TIMEOUT,
            connection.transact("lock", serde_json::json!([lock_id])),
        )
        .await
        .context("OVSDB lock request timed out")?
        .map_err(anyhow::Error::from)
        .context("OVSDB lock request failed")?;
        let locked = result
            .get("locked")
            .and_then(serde_json::Value::as_bool)
            .context("OVSDB lock response omitted locked")?;
        let mut locks = self.locks.lock().await;
        anyhow::ensure!(
            !locks.contains_key(&key),
            "concurrent OVSDB lock request used the same owner and ID"
        );
        anyhow::ensure!(
            locks.len() < MAX_RETAINED_LOCKS,
            "OVSDB retained lock limit reached"
        );
        locks.insert(
            key,
            LockSession {
                connection,
                initially_locked: locked,
            },
        );
        Ok(LockReceipt {
            lock_id: lock_id.to_owned(),
            locked,
            queued: !locked,
        })
    }

    /// Steal a lock and retain the new owner connection.
    pub async fn steal(&self, owner: &str, lock_id: &str) -> Result<LockReceipt> {
        validate_lock_key(owner, lock_id)?;
        let key = retained_key(owner, lock_id);
        {
            let locks = self.locks.lock().await;
            anyhow::ensure!(
                !locks.contains_key(&key),
                "OVSDB lock/steal must alternate with unlock"
            );
            anyhow::ensure!(
                locks.len() < MAX_RETAINED_LOCKS,
                "OVSDB retained lock limit reached"
            );
        }
        let address =
            std::env::var("OVSDB_SOCKET_ADDR").unwrap_or_else(|_| DEFAULT_OVSDB_ADDR.to_owned());
        let address: rovs_transport::Address = address.parse().context("invalid OVSDB address")?;
        let stream = tokio::time::timeout(RPC_TIMEOUT, rovs_transport::Stream::connect(&address))
            .await
            .context("connect to OVSDB steal endpoint timed out")?
            .context("connect to OVSDB steal endpoint")?;
        let mut connection = rovs_jsonrpc::Connection::new(stream);
        let result = tokio::time::timeout(
            RPC_TIMEOUT,
            connection.transact("steal", serde_json::json!([lock_id])),
        )
        .await
        .context("OVSDB steal request timed out")?
        .map_err(anyhow::Error::from)
        .context("OVSDB steal request failed")?;
        anyhow::ensure!(
            result.get("locked").and_then(serde_json::Value::as_bool) == Some(true),
            "OVSDB steal did not grant the lock"
        );
        let mut locks = self.locks.lock().await;
        anyhow::ensure!(
            !locks.contains_key(&key),
            "concurrent OVSDB steal used the same owner and ID"
        );
        anyhow::ensure!(
            locks.len() < MAX_RETAINED_LOCKS,
            "OVSDB retained lock limit reached"
        );
        locks.insert(
            key,
            LockSession {
                connection,
                initially_locked: true,
            },
        );
        Ok(LockReceipt {
            lock_id: lock_id.to_owned(),
            locked: true,
            queued: false,
        })
    }

    /// Release a retained RFC 7047 lock connection.
    pub async fn unlock(&self, owner: &str, lock_id: &str) -> Result<UnlockReceipt> {
        validate_lock_key(owner, lock_id)?;
        let key = retained_key(owner, lock_id);
        let mut session = self
            .locks
            .lock()
            .await
            .remove(&key)
            .ok_or_else(|| anyhow::anyhow!("OVSDB lock '{lock_id}' is not held"))?;
        let result = tokio::time::timeout(
            RPC_TIMEOUT,
            session
                .connection
                .transact("unlock", serde_json::json!([lock_id])),
        )
        .await
        .context("OVSDB unlock request timed out")?
        .map_err(anyhow::Error::from)
        .context("OVSDB unlock request failed")?;
        anyhow::ensure!(
            result.as_object().is_some_and(serde_json::Map::is_empty),
            "OVSDB unlock returned an invalid result"
        );
        let notifications = session.connection.drain_notifications().collect::<Vec<_>>();
        anyhow::ensure!(
            notifications.len() <= 8,
            "too many OVSDB lock notifications"
        );
        let mut acquired_while_queued = false;
        let mut stolen_before_unlock = false;
        for notification in notifications {
            anyhow::ensure!(
                notification.id.is_none(),
                "OVSDB lock notification unexpectedly had an ID"
            );
            anyhow::ensure!(
                notification.params == serde_json::json!([lock_id]),
                "OVSDB lock notification named a different lock"
            );
            match notification.method.as_str() {
                "locked" => acquired_while_queued = true,
                "stolen" => stolen_before_unlock = true,
                _ => anyhow::bail!("unexpected OVSDB lock notification"),
            }
        }
        // A queued lock may be unlocked before it is granted; RFC 7047 defines
        // that as cancellation of the pending request. If it was granted, the
        // server must send `locked` before this unlock reply and we report it.
        let _owned_before_unlock = session.initially_locked || acquired_while_queued;
        Ok(UnlockReceipt {
            lock_id: lock_id.to_owned(),
            unlocked: true,
            acquired_while_queued,
            stolen_before_unlock,
        })
    }

    /// Start a caller-selected protocol monitor and retain the native session.
    pub async fn start_protocol_monitor(
        &self,
        owner: &str,
        database: &str,
        monitor_id: &str,
        method: &str,
        requests: serde_json::Value,
    ) -> Result<ProtocolMonitorStart> {
        validate_session_key(owner, monitor_id)?;
        let key = retained_key(owner, monitor_id);
        {
            let mut monitors = self.monitors.lock().await;
            monitors.retain(|_, subscription| subscription.is_active());
            anyhow::ensure!(
                !monitors.contains_key(&key),
                "OVSDB monitor ID is already active for this caller"
            );
            anyhow::ensure!(
                monitors.len() < MAX_RETAINED_MONITORS,
                "OVSDB retained monitor limit reached"
            );
        }
        let address =
            std::env::var("OVSDB_SOCKET_ADDR").unwrap_or_else(|_| DEFAULT_OVSDB_ADDR.to_owned());
        let subscription =
            monitor::subscribe_protocol(&address, database, monitor_id, method, requests).await?;
        let initial = match subscription.receiver.borrow().clone() {
            monitor::ProtocolObservation::Snapshot(snapshot) => snapshot,
            monitor::ProtocolObservation::Unavailable(reason) => {
                return Err(anyhow::anyhow!("OVSDB monitor unavailable: {reason}"));
            }
        };
        let receiver = subscription.receiver.clone();
        let mut monitors = self.monitors.lock().await;
        anyhow::ensure!(
            !monitors.contains_key(&key),
            "concurrent OVSDB monitor used the same caller and ID"
        );
        anyhow::ensure!(
            monitors.len() < MAX_RETAINED_MONITORS,
            "OVSDB retained monitor limit reached"
        );
        monitors.insert(key, subscription);
        Ok(ProtocolMonitorStart { initial, receiver })
    }

    /// Cancel a retained monitor through monitor_cancel on its owning session.
    pub async fn cancel_monitor(&self, owner: &str, monitor_id: &str) -> Result<serde_json::Value> {
        validate_session_key(owner, monitor_id)?;
        let subscription = self
            .monitors
            .lock()
            .await
            .remove(&retained_key(owner, monitor_id))
            .ok_or_else(|| {
                anyhow::anyhow!("OVSDB monitor '{monitor_id}' is not active for this caller")
            })?;
        subscription.cancel().await
    }

    /// Compatibility shim for plugins that pass `simd_json::OwnedValue`.
    /// Converts to `serde_json::Value` and routes through the D-Bus daemon.
    pub async fn transact_simd(
        &self,
        operations: simd_json::OwnedValue,
    ) -> Result<serde_json::Value> {
        let text = simd_json::to_string(&operations)
            .context("failed to serialize simd_json operations to JSON text")?;
        let converted: serde_json::Value = serde_json::from_str(&text)
            .context("failed to deserialize simd_json operations as serde_json::Value")?;
        self.transact_many(converted.as_array().cloned().unwrap_or_default())
            .await
    }

    // ── Datapath-safe controller attach (OVS async flow contract) ─────────────

    /// See [`crate::datapath_safe::ensure_fallback_normal`].
    pub async fn ensure_fallback_normal(&self, bridge: &str) -> Result<()> {
        crate::datapath_safe::ensure_fallback_normal(bridge).await
    }

    /// See [`crate::datapath_safe::set_fail_mode`].
    pub async fn set_fail_mode(&self, bridge: &str, mode: &str) -> Result<()> {
        crate::datapath_safe::set_fail_mode(bridge, mode).await
    }

    /// See [`crate::datapath_safe::del_controller`].
    pub async fn del_controller(&self, bridge: &str) -> Result<()> {
        crate::datapath_safe::del_controller(bridge).await
    }

    /// See [`crate::datapath_safe::set_controller`].
    pub async fn set_controller(&self, bridge: &str, endpoint: &str) -> Result<()> {
        crate::datapath_safe::set_controller(bridge, endpoint).await
    }

    /// See [`crate::datapath_safe::get_datapath_health`].
    pub async fn get_datapath_health(
        &self,
        bridge: &str,
    ) -> Result<crate::datapath_safe::DatapathHealth> {
        crate::datapath_safe::get_datapath_health(bridge).await
    }

    /// See [`crate::datapath_safe::attach_controller_safe`].
    pub async fn attach_controller_safe(
        &self,
        bridge: &str,
        endpoint: &str,
    ) -> Result<crate::datapath_safe::DatapathHealth> {
        crate::datapath_safe::attach_controller_safe(bridge, endpoint).await
    }
}
