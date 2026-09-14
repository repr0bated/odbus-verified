//! Client of the cognitive-mcp owner over the session-bus plugin socket.
//!
//! Production memory/soul/namespace traffic goes through
//! `org.opdbus.v1.PluginV1.Call` on `/org/opdbus/v1/plugins/cognitive_mcp`
//! (destination `org.opdbus.v1.plugins`), matching `op-mcp`'s
//! `cognitive_bridge`. Cognitive-mcp is the sole CozoDB owner; this crate
//! must not open RocksDB on the serve path.
//!
//! In-process `CognitiveMemoryStore` / `SoulMemoryStore` exist only so unit
//! and integration tests can drive the gRPC service impls against in-memory
//! Cozo.

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use op_cognitive_mcp::memory_store::{
    CognitiveMemoryStore, EntryQuery, MemoryEntry as StoreEntry, NamespaceKind,
};
use op_cognitive_mcp::soul_memory::{
    AgentNamespaceBinding, SoulMemory as StoreSoul, SoulMemoryStore, SoulUpdate,
};
use serde_json::{json, Value};
use std::sync::Arc;

/// Well-known session bus. Same default as `op-mcp::cognitive_bridge`.
pub const DEFAULT_SESSION_BUS_ADDRESS: &str = "unix:path=/run/opdbus/session-bus.sock";
const BRIDGE_BUS_NAME: &str = "org.opdbus.v1.plugins";
const PLUGIN_INTERFACE: &str = "org.opdbus.v1.PluginV1";
const COGNITIVE_OBJECT_PATH: &str = "/org/opdbus/v1/plugins/cognitive_mcp";

/// Reserved namespaces used to persist assistant soul/bindings through the
/// existing `memory_*` plugin methods (those relations are not separate
/// schema methods).
const SOUL_NAMESPACE: &str = "assistant:soul";
const BINDING_NAMESPACE: &str = "assistant:binding";
const META_KEY: &str = "_meta";

/// Session-bus address used to reach the cognitive_mcp plugin.
pub fn default_cognitive_bus_address() -> String {
    std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .or_else(|_| std::env::var("COGNITIVE_MCP_BUS_ADDRESS"))
        .unwrap_or_else(|_| DEFAULT_SESSION_BUS_ADDRESS.to_string())
}

/// D-Bus client for `PluginV1.Call` on the cognitive_mcp plugin.
#[derive(Clone)]
pub struct CognitivePluginClient {
    connection: zbus::Connection,
}

impl CognitivePluginClient {
    pub async fn connect(bus_address: &str) -> Result<Self> {
        let connection = zbus::connection::Builder::address(bus_address)
            .with_context(|| format!("invalid session bus address: {bus_address}"))?
            .build()
            .await
            .with_context(|| format!("connecting to session bus at {bus_address}"))?;
        Ok(Self { connection })
    }

    pub async fn connect_default() -> Result<Self> {
        Self::connect(&default_cognitive_bus_address()).await
    }

    /// Invoke one schema method. Unwraps the bridge accountability envelope
    /// and the MCP `content[0].text` wrapper used by the cognitive loopback.
    async fn call_method(&self, method: &str, args: &Value) -> Result<Value> {
        let json_args = serde_json::to_string(args)?;
        let reply = self
            .connection
            .call_method(
                Some(BRIDGE_BUS_NAME),
                COGNITIVE_OBJECT_PATH,
                Some(PLUGIN_INTERFACE),
                "Call",
                &(method, json_args.as_str()),
            )
            .await
            .with_context(|| format!("PluginV1.Call({method}) on cognitive_mcp"))?;

        let body: String = reply
            .body()
            .deserialize()
            .context("cognitive_mcp PluginV1.Call returned a non-string body")?;
        unwrap_call_payload(method, &body)
    }

    async fn memory_store(
        &self,
        namespace: &str,
        key: &str,
        value: Value,
        tags: Vec<String>,
        namespace_kind: Option<&str>,
    ) -> Result<Value> {
        let mut args = json!({
            "namespace": namespace,
            "key": key,
            "value": value,
            "tags": tags,
        });
        if let Some(kind) = namespace_kind {
            args["namespace_kind"] = json!(kind);
        }
        self.call_method("memory_store", &args).await
    }

    async fn memory_retrieve(&self, namespace: &str, key: &str) -> Result<Value> {
        self.call_method(
            "memory_retrieve",
            &json!({ "namespace": namespace, "key": key }),
        )
        .await
    }

    async fn memory_query(
        &self,
        namespace: Option<&str>,
        key_pattern: Option<&str>,
        tags: Option<&[String]>,
        limit: Option<i64>,
    ) -> Result<Value> {
        let mut args = json!({});
        if let Some(ns) = namespace {
            args["namespace"] = json!(ns);
        }
        if let Some(pat) = key_pattern {
            args["key_pattern"] = json!(pat);
        }
        if let Some(t) = tags {
            args["tags"] = json!(t);
        }
        if let Some(lim) = limit {
            args["limit"] = json!(lim);
        }
        self.call_method("memory_query", &args).await
    }

    async fn memory_delete(&self, namespace: &str, key: &str) -> Result<Value> {
        self.call_method(
            "memory_delete",
            &json!({ "namespace": namespace, "key": key }),
        )
        .await
    }
}

/// Memory CRUD used by `MemoryServiceImpl` / `NamespaceMemoryServiceImpl`.
pub(crate) enum MemoryBackend {
    Local(Arc<CognitiveMemoryStore>),
    Remote(Arc<CognitivePluginClient>),
}

impl MemoryBackend {
    pub(crate) async fn query_entries(&self, q: EntryQuery) -> Result<Vec<StoreEntry>> {
        match self {
            Self::Local(store) => store.query_entries(q).await,
            Self::Remote(client) => remote_query_entries(client, q).await,
        }
    }

    pub(crate) async fn retrieve_entry(
        &self,
        namespace: &str,
        key: &str,
    ) -> Result<Option<StoreEntry>> {
        match self {
            Self::Local(store) => store.retrieve_entry(namespace, key).await,
            Self::Remote(client) => remote_retrieve_entry(client, namespace, key).await,
        }
    }

    pub(crate) async fn store_entry(
        &self,
        namespace: &str,
        key: &str,
        value: Value,
        tags: Vec<String>,
    ) -> Result<()> {
        match self {
            Self::Local(store) => {
                store.store_entry(namespace, key, value, tags, None).await?;
                Ok(())
            }
            Self::Remote(client) => {
                client
                    .memory_store(namespace, key, value, tags, None)
                    .await?;
                Ok(())
            }
        }
    }

    pub(crate) async fn delete_entry(&self, namespace: &str, key: &str) -> Result<bool> {
        match self {
            Self::Local(store) => store.delete_entry(namespace, key).await,
            Self::Remote(client) => {
                let result = client.memory_delete(namespace, key).await?;
                Ok(result.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
            }
        }
    }

    pub(crate) async fn ensure_namespace(&self, name: &str, kind: NamespaceKind) -> Result<()> {
        match self {
            Self::Local(store) => local_ensure_namespace(store, name, kind).await,
            Self::Remote(client) => {
                client
                    .memory_store(
                        name,
                        META_KEY,
                        json!({ "ensured": true, "kind": kind.to_string() }),
                        vec!["meta".into()],
                        Some(&kind.to_string()),
                    )
                    .await?;
                Ok(())
            }
        }
    }

    pub(crate) async fn delete_namespace(&self, name: &str) -> Result<bool> {
        match self {
            Self::Local(store) => store.delete_namespace(name).await,
            Self::Remote(client) => {
                let entries = remote_query_entries(
                    client,
                    EntryQuery {
                        namespace_id: Some(name.into()),
                        ..Default::default()
                    },
                )
                .await?;
                if entries.is_empty() {
                    let _ = client.memory_delete(name, META_KEY).await?;
                    return Ok(false);
                }
                for entry in &entries {
                    let _ = client.memory_delete(name, &entry.key).await?;
                }
                let _ = client.memory_delete(name, META_KEY).await?;
                Ok(true)
            }
        }
    }
}

/// Soul + agent-namespace bindings used by `SoulServiceImpl` /
/// `NamespaceMemoryServiceImpl`.
pub(crate) enum SoulBackend {
    Local(Arc<SoulMemoryStore>),
    Remote(Arc<CognitivePluginClient>),
}

impl SoulBackend {
    pub(crate) async fn get_soul(&self, agent_id: &str) -> Result<Option<StoreSoul>> {
        match self {
            Self::Local(store) => store.get_soul(agent_id).await,
            Self::Remote(client) => remote_get_json(client, SOUL_NAMESPACE, agent_id)
                .await
                .and_then(|v| v.map(value_to_soul).transpose()),
        }
    }

    pub(crate) async fn upsert_soul(
        &self,
        agent_id: &str,
        update: SoulUpdate,
    ) -> Result<StoreSoul> {
        match self {
            Self::Local(store) => store.upsert_soul(agent_id, update).await,
            Self::Remote(client) => {
                let existing = self.get_soul(agent_id).await?;
                let now = Utc::now();
                let soul = StoreSoul {
                    agent_id: agent_id.to_string(),
                    identity: update
                        .identity
                        .or_else(|| existing.as_ref().map(|s| s.identity.clone()))
                        .unwrap_or_default(),
                    personality: update
                        .personality
                        .or_else(|| existing.as_ref().map(|s| s.personality.clone()))
                        .unwrap_or_default(),
                    traits: update
                        .traits
                        .or_else(|| existing.as_ref().map(|s| s.traits.clone()))
                        .unwrap_or(Value::Object(Default::default())),
                    version: existing.as_ref().map(|s| s.version + 1).unwrap_or(1),
                    created_at: existing.as_ref().map(|s| s.created_at).unwrap_or(now),
                    updated_at: now,
                };
                client
                    .memory_store(
                        SOUL_NAMESPACE,
                        agent_id,
                        serde_json::to_value(&soul)?,
                        vec!["soul".into()],
                        Some("agent"),
                    )
                    .await?;
                Ok(soul)
            }
        }
    }

    pub(crate) async fn delete_soul(&self, agent_id: &str) -> Result<()> {
        match self {
            Self::Local(store) => {
                store.delete_soul(agent_id).await?;
                Ok(())
            }
            Self::Remote(client) => {
                client.memory_delete(SOUL_NAMESPACE, agent_id).await?;
                Ok(())
            }
        }
    }

    pub(crate) async fn list_souls(&self) -> Result<Vec<StoreSoul>> {
        match self {
            Self::Local(store) => store.list_souls().await,
            Self::Remote(client) => {
                let entries = remote_query_entries(
                    client,
                    EntryQuery {
                        namespace_id: Some(SOUL_NAMESPACE.into()),
                        ..Default::default()
                    },
                )
                .await?;
                let mut out = Vec::new();
                for entry in entries {
                    if entry.key == META_KEY {
                        continue;
                    }
                    out.push(value_to_soul(entry.value)?);
                }
                Ok(out)
            }
        }
    }

    pub(crate) async fn get_binding(
        &self,
        agent_id: &str,
    ) -> Result<Option<AgentNamespaceBinding>> {
        match self {
            Self::Local(store) => store.get_binding(agent_id).await,
            Self::Remote(client) => remote_get_json(client, BINDING_NAMESPACE, agent_id)
                .await
                .and_then(|v| v.map(value_to_binding).transpose()),
        }
    }

    pub(crate) async fn bind_namespace(
        &self,
        agent_id: &str,
        namespace: &str,
    ) -> Result<AgentNamespaceBinding> {
        match self {
            Self::Local(store) => store.bind_namespace(agent_id, namespace).await,
            Self::Remote(client) => {
                let existing = self.get_binding(agent_id).await?;
                let now = Utc::now();
                let binding = AgentNamespaceBinding {
                    agent_id: agent_id.to_string(),
                    namespace: namespace.to_string(),
                    created_at: existing.as_ref().map(|b| b.created_at).unwrap_or(now),
                    updated_at: now,
                };
                client
                    .memory_store(
                        BINDING_NAMESPACE,
                        agent_id,
                        serde_json::to_value(&binding)?,
                        vec!["binding".into()],
                        Some("agent"),
                    )
                    .await?;
                Ok(binding)
            }
        }
    }

    pub(crate) async fn clear_binding(&self, agent_id: &str) -> Result<bool> {
        match self {
            Self::Local(store) => store.clear_binding(agent_id).await,
            Self::Remote(client) => {
                let result = client.memory_delete(BINDING_NAMESPACE, agent_id).await?;
                Ok(result.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
            }
        }
    }

    pub(crate) async fn list_bindings(&self) -> Result<Vec<AgentNamespaceBinding>> {
        match self {
            Self::Local(store) => store.list_bindings().await,
            Self::Remote(client) => {
                let entries = remote_query_entries(
                    client,
                    EntryQuery {
                        namespace_id: Some(BINDING_NAMESPACE.into()),
                        ..Default::default()
                    },
                )
                .await?;
                let mut out = Vec::new();
                for entry in entries {
                    if entry.key == META_KEY {
                        continue;
                    }
                    out.push(value_to_binding(entry.value)?);
                }
                Ok(out)
            }
        }
    }
}

pub(crate) async fn local_ensure_namespace(
    store: &CognitiveMemoryStore,
    name: &str,
    kind: NamespaceKind,
) -> Result<()> {
    if store.get_namespace_by_name(name).await?.is_some() {
        return Ok(());
    }
    store
        .upsert_namespace(name, kind, None, None, None, Value::Null)
        .await?;
    Ok(())
}

async fn remote_retrieve_entry(
    client: &CognitivePluginClient,
    namespace: &str,
    key: &str,
) -> Result<Option<StoreEntry>> {
    let result = client.memory_retrieve(namespace, key).await?;
    if result.get("found").and_then(|v| v.as_bool()) == Some(false) {
        return Ok(None);
    }
    if result.get("found").and_then(|v| v.as_bool()) != Some(true)
        && result.get("key").and_then(|v| v.as_str()).is_none()
    {
        return Ok(None);
    }
    Ok(Some(json_to_entry(namespace, &result)))
}

async fn remote_query_entries(
    client: &CognitivePluginClient,
    q: EntryQuery,
) -> Result<Vec<StoreEntry>> {
    let limit = match (q.limit, q.offset) {
        (Some(lim), Some(off)) => Some(lim + off),
        (Some(lim), None) => Some(lim),
        (None, Some(off)) => Some(off + 100),
        (None, None) => None,
    };
    let result = client
        .memory_query(
            q.namespace_id.as_deref(),
            q.key_pattern.as_deref(),
            q.tags.as_deref(),
            limit,
        )
        .await?;
    let raw = result
        .get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let offset = q.offset.unwrap_or(0).max(0) as usize;
    let take = q.limit.unwrap_or(i64::MAX).max(0) as usize;

    let mut out = Vec::new();
    for item in raw.into_iter().skip(offset).take(take) {
        let ns = item
            .get("namespace_id")
            .or_else(|| item.get("namespace"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| q.namespace_id.clone())
            .unwrap_or_default();
        let key = item
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if key.is_empty() || key == META_KEY {
            continue;
        }
        if item.get("value").is_some() {
            out.push(json_to_entry(&ns, &item));
            continue;
        }
        match remote_retrieve_entry(client, &ns, &key).await? {
            Some(entry) => out.push(entry),
            None => out.push(json_to_entry(&ns, &item)),
        }
    }
    Ok(out)
}

async fn remote_get_json(
    client: &CognitivePluginClient,
    namespace: &str,
    key: &str,
) -> Result<Option<Value>> {
    Ok(remote_retrieve_entry(client, namespace, key)
        .await?
        .map(|e| e.value))
}

fn json_to_entry(fallback_ns: &str, v: &Value) -> StoreEntry {
    let updated = parse_ts(
        v.get("updated_at")
            .and_then(|t| t.as_str())
            .unwrap_or_default(),
    );
    let created = v
        .get("created_at")
        .and_then(|t| t.as_str())
        .map(parse_ts)
        .unwrap_or(updated);
    let namespace = v
        .get("namespace")
        .or_else(|| v.get("namespace_id"))
        .and_then(|n| n.as_str())
        .unwrap_or(fallback_ns)
        .to_string();
    let tags = v
        .get("tags")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    StoreEntry {
        id: v
            .get("id")
            .and_then(|i| i.as_str())
            .unwrap_or_default()
            .to_string(),
        namespace_id: namespace,
        key: v
            .get("key")
            .and_then(|k| k.as_str())
            .unwrap_or_default()
            .to_string(),
        value: v.get("value").cloned().unwrap_or(Value::Null),
        tags,
        created_at: created,
        updated_at: updated,
        expires_at: None,
        access_count: v.get("access_count").and_then(|n| n.as_i64()).unwrap_or(0),
        last_accessed: updated,
    }
}

fn value_to_soul(value: Value) -> Result<StoreSoul> {
    serde_json::from_value(value).context("decode soul memory JSON")
}

fn value_to_binding(value: Value) -> Result<AgentNamespaceBinding> {
    serde_json::from_value(value).context("decode namespace binding JSON")
}

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

/// Unwrap PluginV1 + MCP wrappers down to the tool payload.
pub(crate) fn unwrap_call_payload(method: &str, body: &str) -> Result<Value> {
    let envelope: Value = serde_json::from_str(body)
        .with_context(|| format!("cognitive_mcp {method} returned non-JSON: {body}"))?;
    if envelope.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let msg = envelope
            .get("error")
            .and_then(|e| e.as_str())
            .or_else(|| envelope.get("message").and_then(|m| m.as_str()))
            .unwrap_or("cognitive_mcp call failed");
        return Err(anyhow!("{method}: {msg}"));
    }
    let result = envelope
        .get("result")
        .cloned()
        .ok_or_else(|| anyhow!("bridge envelope for {method} carried no 'result': {body}"))?;
    unwrap_mcp_content(method, result)
}

fn unwrap_mcp_content(method: &str, result: Value) -> Result<Value> {
    if result.get("isError").and_then(|v| v.as_bool()) == Some(true) {
        let msg = result
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|item| item.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("cognitive tool returned isError");
        return Err(anyhow!("{method}: {msg}"));
    }
    if let Some(text) = result
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|item| item.get("text"))
        .and_then(|t| t.as_str())
    {
        if let Ok(parsed) = serde_json::from_str::<Value>(text) {
            if let Some(inner) = parsed.get("result").cloned() {
                return Ok(inner);
            }
            return Ok(parsed);
        }
        return Err(anyhow!("{method}: MCP content was not JSON: {text}"));
    }
    if let Some(inner) = result.get("result").cloned() {
        return Ok(inner);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwraps_plugin_envelope_and_mcp_content() {
        let tool = json!({"ok": true, "key": "k1", "namespace": "demo"});
        let body = json!({
            "success": true,
            "event_id": 1,
            "event_hash": "abc",
            "plugin_id": "cognitive_mcp",
            "method": "memory_store",
            "result": {
                "content": [{ "type": "text", "text": tool.to_string() }],
                "isError": false
            }
        })
        .to_string();
        let out = unwrap_call_payload("memory_store", &body).unwrap();
        assert_eq!(out["key"], "k1");
        assert_eq!(out["ok"], true);
    }

    #[test]
    fn unwraps_direct_tool_result() {
        let body = json!({
            "success": true,
            "result": { "found": true, "key": "k1", "value": "hello" }
        })
        .to_string();
        let out = unwrap_call_payload("memory_retrieve", &body).unwrap();
        assert_eq!(out["found"], true);
        assert_eq!(out["value"], "hello");
    }

    #[test]
    fn mcp_is_error_becomes_err() {
        let body = json!({
            "success": true,
            "result": {
                "content": [{ "type": "text", "text": "Error: missing key" }],
                "isError": true
            }
        })
        .to_string();
        let err = unwrap_call_payload("memory_retrieve", &body).unwrap_err();
        assert!(err.to_string().contains("missing key"));
    }

    #[test]
    fn default_bus_address_is_session_socket() {
        // Env may or may not be set in the test process; the fallback is the
        // canonical session-bus UDS used by the bridge and cognitive_bridge.
        let addr = DEFAULT_SESSION_BUS_ADDRESS;
        assert_eq!(addr, "unix:path=/run/opdbus/session-bus.sock");
    }
}
