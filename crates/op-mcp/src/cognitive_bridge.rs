//! Bridge-backed tool executor: the fan-in proxy's engine.
//!
//! Sources its tool list and dispatches every call through `op-grpc-bridge` rather
//! than opening the cognitive store directly. That matters for three reasons, each
//! measured rather than assumed:
//!
//! 1. **One CozoDB writer.** `CognitiveMcpServer::new` opens a persistent CozoDB, and
//!    every MCP client that spawns `op-cognitive-mcp --stdio` races for the file lock
//!    — the loser dies with `Resource temporarily unavailable`. Routing through the
//!    bridge leaves exactly one owner.
//!
//! 2. **Enforcement.** The direct stdio path bypasses the method gate, argument
//!    validation, capability check and event chain. Every call made here passes all
//!    four and lands in the event chain with an `event_id`.
//!
//! 3. **One authenticated caller.** This process holds the identity, so individual
//!    MCP clients carry no credential material and capability can be scoped per
//!    client without touching the sealed schema.
//!
//! Transport is D-Bus `org.opdbus.v1.PluginV1.Call` on the `cognitive_mcp` plugin —
//! the only interface and bus name in play. `list_tools` populates the registry;
//! `invoke_tool` dispatches.
//!
//! OSCAL subid: exp.service.cognitive-mcp.fanin-proxy@v1

use crate::server::{ToolExecutor, ToolInfo};
use anyhow::{anyhow, Context, Result};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use tokio::sync::RwLock;

/// Bus name owned by op-grpc-bridge. The only legal well-known name.
const BRIDGE_BUS_NAME: &str = "org.opdbus.v1.plugins";
/// The single canonical plugin interface.
const PLUGIN_INTERFACE: &str = "org.opdbus.v1.PluginV1";
/// Object path for the cognitive_mcp plugin.
const COGNITIVE_OBJECT_PATH: &str = "/org/opdbus/v1/plugins/cognitive_mcp";

/// Session bus address; matches the bridge's own default.
fn session_bus_address() -> String {
    std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .unwrap_or_else(|_| "unix:path=/run/opdbus/session-bus.sock".to_string())
}

pub struct BridgeToolExecutor {
    connection: zbus::Connection,
    /// Cached tool list. The registry is populated at runtime on the cognitive side,
    /// so this is refreshed on demand rather than assumed static.
    tools: RwLock<Vec<ToolInfo>>,
}

impl BridgeToolExecutor {
    pub async fn connect() -> Result<Self> {
        let address = session_bus_address();
        let connection = zbus::connection::Builder::address(address.as_str())
            .with_context(|| format!("invalid session bus address: {address}"))?
            .build()
            .await
            .with_context(|| format!("connecting to session bus at {address}"))?;

        let executor = Self {
            connection,
            tools: RwLock::new(Vec::new()),
        };
        executor.refresh().await?;
        Ok(executor)
    }

    /// Invoke one schema method on the cognitive_mcp plugin through the bridge.
    ///
    /// Returns the `result` payload, unwrapping the bridge's accountability envelope
    /// (`success`, `event_id`, `event_hash`, `result`). A missing `result` is an
    /// error rather than an empty success, so a silent contract change surfaces.
    async fn call_method(&self, method: &str, json_args: &str) -> Result<Value> {
        let reply = self
            .connection
            .call_method(
                Some(BRIDGE_BUS_NAME),
                COGNITIVE_OBJECT_PATH,
                Some(PLUGIN_INTERFACE),
                "Call",
                &(method, json_args),
            )
            .await
            .with_context(|| format!("bridge PluginV1.Call({method})"))?;

        let body: String = reply
            .body()
            .deserialize()
            .context("bridge returned a non-string body")?;

        let mut body_bytes = body.clone().into_bytes();
        let envelope: Value = simd_json::to_owned_value(&mut body_bytes)
            .map_err(|e| anyhow!("bridge returned invalid JSON for {method}: {e}: {body}"))?;

        envelope
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow!("bridge envelope for {method} carried no 'result': {body}"))
    }

    /// Re-read the tool list from the live registry via the bridge.
    async fn refresh(&self) -> Result<usize> {
        // list_tools takes no arguments; the schema types it as an empty object, so
        // `{}` validates where a bare `null` previously did not.
        let result = self.call_method("list_tools", "{}").await?;

        let entries = result
            .get("tools")
            .and_then(|v: &Value| v.as_array())
            .ok_or_else(|| anyhow!("list_tools result had no 'tools' array: {result}"))?;

        let tools: Vec<ToolInfo> = entries
            .iter()
            .filter_map(|entry| {
                let name = entry
                    .get("name")
                    .and_then(|v: &Value| v.as_str())?
                    .to_string();
                Some(ToolInfo {
                    name,
                    description: entry
                        .get("description")
                        .and_then(|v: &Value| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    // Tools whose schema the cognitive side omits still need a valid
                    // object schema, or MCP clients reject the tool outright.
                    input_schema: entry
                        .get("inputSchema")
                        .cloned()
                        .unwrap_or_else(|| json!({"type": "object"})),
                    annotations: None,
                })
            })
            .collect();

        let count = tools.len();
        *self.tools.write().await = tools;
        Ok(count)
    }
}

#[async_trait::async_trait]
impl ToolExecutor for BridgeToolExecutor {
    async fn list_tools(&self) -> Result<Vec<ToolInfo>> {
        {
            let cached = self.tools.read().await;
            if !cached.is_empty() {
                return Ok(cached.clone());
            }
        }
        self.refresh().await?;
        Ok(self.tools.read().await.clone())
    }

    async fn execute_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        let args = json!({ "tool_name": name, "arguments": arguments });
        self.call_method("invoke_tool", &simd_json::to_string(&args)?)
            .await
    }

    async fn get_tool_schema(&self, name: &str) -> Result<Option<Value>> {
        Ok(self
            .list_tools()
            .await?
            .into_iter()
            .find(|tool| tool.name == name)
            .map(|tool| tool.input_schema))
    }

    async fn search_tools(&self, query: &str, limit: usize) -> Result<Vec<ToolInfo>> {
        let needle = query.to_lowercase();
        Ok(self
            .list_tools()
            .await?
            .into_iter()
            .filter(|tool| {
                tool.name.to_lowercase().contains(&needle)
                    || tool.description.to_lowercase().contains(&needle)
            })
            .take(limit)
            .collect())
    }
}

// ---------------------------------------------------------------------------
// MergedToolExecutor: one socket, all tools
// ---------------------------------------------------------------------------

/// Combines cognitive tools (via bridge) with op-tools builtins into a single
/// executor. This is the "one socket to serve all" — clients see one flat tool
/// list containing both the 400+ cognitive/agent tools and the 155 system tools.
///
/// Dispatch priority: cognitive (bridge) tools win on name collision because they
/// go through the enforcement chain. op-tools builtins run locally (no bridge hop).
pub struct MergedToolExecutor {
    bridge: BridgeToolExecutor,
    local_registry: std::sync::Arc<op_tools::ToolRegistry>,
}

impl MergedToolExecutor {
    pub async fn connect() -> Result<Self> {
        let bridge = BridgeToolExecutor::connect().await?;
        let local_registry = std::sync::Arc::new(op_tools::ToolRegistry::new());
        op_tools::register_builtin_tools(&local_registry)
            .await
            .map_err(|e| anyhow!("failed to register op-tools builtins: {e}"))?;
        Ok(Self {
            bridge,
            local_registry,
        })
    }

    /// Names of tools served by the bridge (cognitive side).
    async fn bridge_tool_names(&self) -> std::collections::HashSet<String> {
        self.bridge
            .list_tools()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|t| t.name)
            .collect()
    }
}

#[async_trait::async_trait]
impl ToolExecutor for MergedToolExecutor {
    async fn list_tools(&self) -> Result<Vec<ToolInfo>> {
        let mut tools = self.bridge.list_tools().await?;
        let bridge_names: std::collections::HashSet<String> =
            tools.iter().map(|t| t.name.clone()).collect();

        // Append local op-tools that don't collide with bridge tools.
        let local_defs = self.local_registry.list().await;
        for def in local_defs {
            if !bridge_names.contains(&def.name) {
                tools.push(ToolInfo {
                    name: def.name,
                    description: def.description,
                    input_schema: def.input_schema,
                    annotations: None,
                });
            }
        }
        Ok(tools)
    }

    async fn execute_tool(&self, name: &str, arguments: Value) -> Result<Value> {
        // Bridge tools take priority (enforced path).
        let bridge_names = self.bridge_tool_names().await;
        if bridge_names.contains(name) {
            return self.bridge.execute_tool(name, arguments).await;
        }

        // Fall through to local op-tools registry.
        let tool = self
            .local_registry
            .get(name)
            .await
            .ok_or_else(|| anyhow!("tool not found: {name}"))?;
        tool.execute(arguments).await
    }

    async fn get_tool_schema(&self, name: &str) -> Result<Option<Value>> {
        // Check bridge first.
        if let Some(schema) = self.bridge.get_tool_schema(name).await? {
            return Ok(Some(schema));
        }
        // Then local.
        Ok(self
            .local_registry
            .get_definition(name)
            .await
            .map(|d| d.input_schema))
    }

    async fn search_tools(&self, query: &str, limit: usize) -> Result<Vec<ToolInfo>> {
        let needle = query.to_lowercase();
        Ok(self
            .list_tools()
            .await?
            .into_iter()
            .filter(|tool| {
                tool.name.to_lowercase().contains(&needle)
                    || tool.description.to_lowercase().contains(&needle)
            })
            .take(limit)
            .collect())
    }
}
