//! Cognitive Tools for MCP
//!
//! MCP tools backed by the SQLite namespace/entry memory store.
//! Operations: store, retrieve, query, delete, list_namespaces, stats.

use crate::agent_tools::register_agent_tools;
use crate::blob_catalog_tool::register_blob_catalog_tool;
use crate::blob_vectors_tool::register_blob_vectors_tools;
use crate::memory_store::{CognitiveMemoryStore, EntryQuery, NamespaceKind};
use crate::qdrant_shuttle::QdrantSemanticShuttle;
use anyhow::Result;
use async_trait::async_trait;
use op_mcp::tool_registry::{BoxedTool, Tool, ToolRegistry};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;

/// Safe field access for simd_json values.
///
/// simd_json's `Index` impl **panics** when the key is absent, unlike `serde_json`
/// which yields `Null`. Every optional-field read must therefore go through `get`.
/// This wrapper keeps the ergonomic `field(input, "k").as_str()` shape while
/// returning a Null sentinel rather than unwinding the worker task.
///
/// Reading an absent optional argument used to panic the tokio worker and reset the
/// client connection, which surfaced as an empty HTTP response rather than an error.
pub(crate) fn field<'a>(input: &'a Value, key: &str) -> &'a Value {
    static NULL: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    input
        .get(key)
        .unwrap_or_else(|| NULL.get_or_init(|| json!(null)))
}

pub struct CognitiveToolRegistry;

impl CognitiveToolRegistry {
    pub async fn register_all(
        registry: &ToolRegistry,
        store: Arc<CognitiveMemoryStore>,
        qdrant: Option<Arc<QdrantSemanticShuttle>>,
    ) -> Result<()> {
        registry
            .register(Arc::new(MemoryTool::new(store.clone(), qdrant.clone())) as BoxedTool)
            .await?;
        registry
            .register(Arc::new(RegisterToolTool) as BoxedTool)
            .await?;
        register_agent_tools(registry).await?;
        // Provider-backed MCP tools are projected by the bridge from their
        // supervised loopback services. They must never delay construction of
        // the in-process HOT memory/workflow runtime.
        register_blob_catalog_tool(registry).await?;
        register_blob_vectors_tools(registry, qdrant).await?;
        Ok(())
    }
}

pub struct MemoryTool {
    store: Arc<CognitiveMemoryStore>,
    qdrant: Option<Arc<QdrantSemanticShuttle>>,
}

impl MemoryTool {
    pub fn new(
        store: Arc<CognitiveMemoryStore>,
        qdrant: Option<Arc<QdrantSemanticShuttle>>,
    ) -> Self {
        Self { store, qdrant }
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "cognitive_memory"
    }

    fn description(&self) -> &str {
        "Manage cognitive memory namespaces and entries. Operations: store, retrieve, query, delete, list_namespaces, stats."
    }

    fn category(&self) -> &str {
        "cognitive"
    }

    fn tags(&self) -> Vec<String> {
        vec![
            "memory".to_string(),
            "cognitive".to_string(),
            "storage".to_string(),
        ]
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["ensure_soul", "store", "retrieve", "query", "semantic_query", "delete", "list_namespaces", "stats"],
                    "description": "Operation to perform"
                },
                "owner": {
                    "type": "string",
                    "enum": ["chatbot", "user_container"],
                    "description": "Soul owner for ensure_soul"
                },
                "container_id": {
                    "type": "string",
                    "description": "User container identity. When namespace is omitted, memory is scoped to container:<container_id>."
                },
                "identity_id": {
                    "type": "string",
                    "description": "Canonical identity bound to a user container soul and namespace"
                },
                "wireguard_pubkey": {
                    "type": "string",
                    "description": "WireGuard public key bound to the identity, when known"
                },
                "namespace": {
                    "type": "string",
                    "description": "Namespace name (e.g. 'project:op-dbus', 'session:abc', 'agent:planner')"
                },
                "namespace_kind": {
                    "type": "string",
                    "enum": ["project", "session", "database", "workflow", "agent", "cron", "custom"],
                    "description": "Kind of namespace (used when creating)"
                },
                "key": {
                    "type": "string",
                    "description": "Entry key within namespace"
                },
                "value": {
                    "description": "Value to store (any JSON)"
                },
                "query": {
                    "type": "string",
                    "description": "Semantic query text for semantic_query"
                },
                "semantic": {
                    "type": "boolean",
                    "description": "Mirror text content into Qdrant user_memory when available"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Tags for the entry"
                },
                "key_pattern": {
                    "type": "string",
                    "description": "Substring pattern for key search (used in query)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Max results (default 50)"
                }
            },
            "required": ["operation"]
        })
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        let op = field(&input, "operation")
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing operation"))?;

        match op {
            "ensure_soul" => self.op_ensure_soul(&input).await,
            "store" => self.op_store(&input).await,
            "retrieve" => self.op_retrieve(&input).await,
            "query" => self.op_query(&input).await,
            "semantic_query" => self.op_semantic_query(&input).await,
            "delete" => self.op_delete(&input).await,
            "list_namespaces" => self.op_list_namespaces(&input).await,
            "stats" => self.op_stats().await,
            other => Err(anyhow::anyhow!("unknown operation: {}", other)),
        }
    }
}

/// Stub for the `register_tool` schema method (cognitive_mcp_schema, subid
/// mut.service.cognitive-mcp.tool.register@v1). No dynamic tool-registration
/// mechanism exists yet — this establishes the contract as a real, reachable
/// tool that returns a clear "not implemented" rather than a missing route.
/// The schema is the authority; this implementation must catch up.
pub struct RegisterToolTool;

#[async_trait]
impl Tool for RegisterToolTool {
    fn name(&self) -> &str {
        "register_tool"
    }

    fn description(&self) -> &str {
        "Register a new MCP tool at runtime. Not yet implemented."
    }

    fn category(&self) -> &str {
        "cognitive"
    }

    fn tags(&self) -> Vec<String> {
        vec!["tool".to_string(), "registry".to_string()]
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object", "additionalProperties": true})
    }

    fn required_capability(&self) -> Option<&str> {
        Some("cognitive_mcp.invoke")
    }

    fn subid(&self) -> Option<&str> {
        Some("mut.service.cognitive-mcp.tool.register@v1")
    }

    async fn execute(&self, _input: Value) -> Result<Value> {
        Err(anyhow::anyhow!(
            "register_tool is not yet implemented — no dynamic tool-registration mechanism exists"
        ))
    }
}

impl MemoryTool {
    async fn op_ensure_soul(&self, input: &Value) -> Result<Value> {
        let owner = field(input, "owner").as_str().unwrap_or("user_container");
        let container_id = field(input, "container_id").as_str();
        let identity = soul_identity(owner, input)?;
        let namespace = soul_namespace(owner, container_id)?;
        let kind = if owner == "chatbot" {
            NamespaceKind::Agent
        } else {
            NamespaceKind::Custom
        };
        let metadata = soul_metadata(owner, container_id, identity.as_deref(), input);

        self.store
            .upsert_namespace(
                &namespace,
                kind,
                Some("Durable soul state"),
                None,
                None,
                metadata,
            )
            .await?;

        let value = if field(input, "value").is_null() {
            serde_json::json!({
                "owner": owner,
                "container_id": container_id,
                "identity_id": identity,
                "wireguard_pubkey": field(input, "wireguard_pubkey").as_str(),
                "purpose": "soul"
            })
        } else {
            simd_json_to_serde(field(input, "value"))
        };
        let entry = self
            .store
            .store_entry(
                &namespace,
                "soul",
                value,
                soul_tags(owner, container_id, identity.as_deref()),
                None,
            )
            .await?;

        if owner == "user_container" {
            let namespace = memory_namespace(input)?;
            self.ensure_namespace(&namespace, Some("custom")).await?;
            self.store
                .store_entry(
                    &namespace,
                    "_identity",
                    identity_link_value(input, identity.as_deref()),
                    scoped_tags(vec!["identity".to_string()], container_id),
                    None,
                )
                .await?;
        }

        Ok(json!({
            "ok": true,
            "namespace": namespace,
            "key": entry.key,
            "id": entry.id,
            "identity_id": identity
        }))
    }

    async fn ensure_namespace(&self, name: &str, kind_str: Option<&str>) -> Result<()> {
        let kind = kind_str
            .and_then(|s| s.parse::<NamespaceKind>().ok())
            .unwrap_or_else(|| {
                if name.starts_with("project:") {
                    NamespaceKind::Project
                } else if name.starts_with("session:") {
                    NamespaceKind::Session
                } else if name.starts_with("agent:") {
                    NamespaceKind::Agent
                } else if name.starts_with("cron:") {
                    NamespaceKind::Cron
                } else if name.starts_with("workflow:") {
                    NamespaceKind::Workflow
                } else if name.starts_with("db:") {
                    NamespaceKind::Database
                } else {
                    NamespaceKind::Custom
                }
            });

        if self.store.get_namespace_by_name(name).await?.is_none() {
            self.store
                .upsert_namespace(name, kind, None, None, None, serde_json::json!({}))
                .await?;
        }
        Ok(())
    }

    async fn op_store(&self, input: &Value) -> Result<Value> {
        let namespace = memory_namespace(input)?;
        let key = field(input, "key")
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing key"))?;
        let value = simd_json_to_serde(field(input, "value"));
        let tags: Vec<String> = field(input, "tags")
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let tags = scoped_tags(tags, field(input, "container_id").as_str());

        self.ensure_namespace(&namespace, field(input, "namespace_kind").as_str())
            .await?;

        if field(input, "container_id").as_str().is_some()
            && self
                .store
                .retrieve_entry(&namespace, "_identity")
                .await?
                .is_none()
        {
            let identity = soul_identity("user_container", input)?;
            self.store
                .store_entry(
                    &namespace,
                    "_identity",
                    identity_link_value(input, identity.as_deref()),
                    scoped_tags(
                        vec!["identity".to_string()],
                        field(input, "container_id").as_str(),
                    ),
                    None,
                )
                .await?;
        }

        let entry = self
            .store
            .store_entry(&namespace, key, value.clone(), tags, None)
            .await?;
        let semantic_mirrored = self
            .mirror_semantic_memory(input, &namespace, key, &value)
            .await?;

        Ok(json!({
            "ok": true,
            "id": entry.id,
            "namespace": namespace,
            "key": key,
            "semantic_mirrored": semantic_mirrored
        }))
    }

    async fn op_retrieve(&self, input: &Value) -> Result<Value> {
        let namespace = memory_namespace(input)?;
        let key = field(input, "key")
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing key"))?;

        match self.store.retrieve_entry(&namespace, key).await? {
            Some(e) => {
                let val = serde_to_simd_json(e.value);
                Ok(json!({
                    "found": true,
                    "id": e.id,
                    "namespace": namespace,
                    "key": e.key,
                    "value": val,
                    "tags": e.tags,
                    "access_count": e.access_count,
                    "updated_at": e.updated_at.to_rfc3339()
                }))
            }
            None => Ok(json!({ "found": false, "namespace": namespace, "key": key })),
        }
    }

    async fn op_query(&self, input: &Value) -> Result<Value> {
        let q = EntryQuery {
            namespace_id: optional_memory_namespace(input)?,
            key_pattern: field(input, "key_pattern").as_str().map(String::from),
            tags: field(input, "tags").as_array().map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            }),
            limit: field(input, "limit").as_i64(),
            offset: None,
        };

        let entries = self.store.query_entries(q).await?;
        let count = entries.len();
        let items: Vec<Value> = entries
            .into_iter()
            .map(|e| {
                json!({
                    "id": e.id,
                    "namespace_id": e.namespace_id,
                    "key": e.key,
                    "tags": e.tags,
                    "access_count": e.access_count,
                    "updated_at": e.updated_at.to_rfc3339()
                })
            })
            .collect();

        Ok(json!({ "count": count, "entries": items }))
    }

    async fn op_semantic_query(&self, input: &Value) -> Result<Value> {
        let qdrant = self.qdrant.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "semantic memory unavailable: Qdrant Semantic Shuttle is not configured"
            )
        })?;
        let container_id = field(input, "container_id")
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing container_id"))?;
        let query = field(input, "query")
            .as_str()
            .or_else(|| field(input, "key_pattern").as_str())
            .ok_or_else(|| anyhow::anyhow!("missing query"))?;
        let limit = field(input, "limit").as_i64().unwrap_or(10).max(1) as u64;

        let embedding = qdrant.embed_query_text(query).await?;
        let results = qdrant
            .search_user_memory(embedding, container_id, limit)
            .await?;
        let count = results.len();
        let items: Vec<Value> = results
            .into_iter()
            .map(|point| {
                json!({
                    "id": format!("{:?}", point.id),
                    "score": point.score,
                    "payload": format!("{:?}", point.payload)
                })
            })
            .collect();

        Ok(json!({
            "count": count,
            "container_id": container_id,
            "results": items
        }))
    }

    async fn op_delete(&self, input: &Value) -> Result<Value> {
        let namespace = memory_namespace(input)?;
        let key = field(input, "key")
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing key"))?;

        let deleted = self.store.delete_entry(&namespace, key).await?;
        Ok(json!({ "ok": deleted, "namespace": namespace, "key": key }))
    }

    async fn op_list_namespaces(&self, input: &Value) -> Result<Value> {
        // `namespace_kind` is optional. `input[key]` on simd_json's OwnedValue panics
        // when the key is absent (unlike serde_json, which yields Null), so this must
        // go through `get`. Calling list_namespaces without a kind filter is the
        // common case and previously panicked the worker task.
        let kind = input
            .get("namespace_kind")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<NamespaceKind>().ok());

        let namespaces = self.store.list_namespaces(kind).await?;
        let count = namespaces.len();
        let items: Vec<Value> = namespaces
            .into_iter()
            .map(|ns| {
                json!({
                    "id": ns.id,
                    "name": ns.name,
                    "kind": ns.kind.to_string(),
                    "description": ns.description,
                    "linked_task_id": ns.linked_task_id,
                    "linked_cron": ns.linked_cron
                })
            })
            .collect();

        Ok(json!({ "count": count, "namespaces": items }))
    }

    async fn op_stats(&self) -> Result<Value> {
        let stats = self.store.get_stats().await?;
        Ok(json!({
            "total_namespaces": stats.total_namespaces,
            "total_entries": stats.total_entries,
            "entries_by_kind": stats.entries_by_kind
        }))
    }

    async fn mirror_semantic_memory(
        &self,
        input: &Value,
        namespace: &str,
        key: &str,
        value: &serde_json::Value,
    ) -> Result<bool> {
        if !field(input, "semantic").as_bool().unwrap_or(false) {
            return Ok(false);
        }

        let Some(qdrant) = &self.qdrant else {
            tracing::warn!(
                namespace = namespace,
                key = key,
                "semantic memory requested but Qdrant Semantic Shuttle is unavailable"
            );
            return Ok(false);
        };

        let Some(container_id) = field(input, "container_id").as_str() else {
            tracing::warn!(
                namespace = namespace,
                key = key,
                "semantic memory requested without container_id"
            );
            return Ok(false);
        };

        let content = memory_content(value);
        if content.trim().is_empty() {
            return Ok(false);
        }

        let vector = qdrant.embed_document(&content).await?;
        qdrant
            .upsert_user_memory(
                uuid::Uuid::new_v4().to_string(),
                vector,
                container_id,
                key,
                &content,
            )
            .await?;
        Ok(true)
    }
}

fn simd_json_to_serde(v: &Value) -> serde_json::Value {
    let s = simd_json::to_string(v).unwrap_or_default();
    serde_json::from_str(&s).unwrap_or(serde_json::Value::Null)
}

fn serde_to_simd_json(v: serde_json::Value) -> Value {
    let s = serde_json::to_string(&v).unwrap_or_default();
    let mut buf = s.into_bytes();
    simd_json::from_slice(&mut buf).unwrap_or(Value::Static(simd_json::StaticNode::Null))
}

fn memory_namespace(input: &Value) -> Result<String> {
    optional_memory_namespace(input)?
        .ok_or_else(|| anyhow::anyhow!("missing namespace or container_id"))
}

fn optional_memory_namespace(input: &Value) -> Result<Option<String>> {
    if let Some(namespace) = field(input, "namespace").as_str() {
        return Ok(Some(namespace.to_string()));
    }
    Ok(field(input, "container_id")
        .as_str()
        .map(container_namespace))
}

fn container_namespace(container_id: &str) -> String {
    format!("container:{}", container_id)
}

fn soul_namespace(owner: &str, container_id: Option<&str>) -> Result<String> {
    match owner {
        "chatbot" => Ok("soul:chatbot".to_string()),
        "user_container" => {
            let container_id =
                container_id.ok_or_else(|| anyhow::anyhow!("missing container_id"))?;
            Ok(format!("soul:user-container:{container_id}"))
        }
        other => Err(anyhow::anyhow!("unknown soul owner: {}", other)),
    }
}

fn soul_metadata(
    owner: &str,
    container_id: Option<&str>,
    identity_id: Option<&str>,
    input: &Value,
) -> serde_json::Value {
    serde_json::json!({
        "owner": owner,
        "container_id": container_id,
        "identity_id": identity_id,
        "wireguard_pubkey": field(input, "wireguard_pubkey").as_str(),
        "subid": if owner == "chatbot" {
            "src.software.chatbot-soul.persist@v1"
        } else {
            "src.software.user-container-soul.persist@v1"
        }
    })
}

fn soul_tags(owner: &str, container_id: Option<&str>, identity_id: Option<&str>) -> Vec<String> {
    let mut tags = vec!["soul".to_string(), owner.to_string()];
    if let Some(container_id) = container_id {
        tags.push(format!("container:{container_id}"));
    }
    if let Some(identity_id) = identity_id {
        tags.push(format!("identity:{identity_id}"));
    }
    tags
}

fn scoped_tags(mut tags: Vec<String>, container_id: Option<&str>) -> Vec<String> {
    if let Some(container_id) = container_id {
        let tag = format!("container:{container_id}");
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

fn memory_content(value: &serde_json::Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_default())
}

fn soul_identity(owner: &str, input: &Value) -> Result<Option<String>> {
    let identity = field(input, "identity_id").as_str().map(ToOwned::to_owned);
    if owner == "user_container" && identity.is_none() {
        return Err(anyhow::anyhow!(
            "missing identity_id for user_container soul"
        ));
    }
    Ok(identity)
}

fn identity_link_value(input: &Value, identity_id: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "identity_id": identity_id,
        "container_id": field(input, "container_id").as_str(),
        "wireguard_pubkey": field(input, "wireguard_pubkey").as_str(),
        "soul_namespace": field(input, "container_id")
            .as_str()
            .map(|container_id| format!("soul:user-container:{container_id}")),
        "subid": "src.software.user-container-memory.identity-link@v1"
    })
}
