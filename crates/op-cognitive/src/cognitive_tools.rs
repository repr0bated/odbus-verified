//! Cognitive Tools for MCP
//!
//! MCP tools backed by the Cozo namespace/entry memory store.
//! Operations: store, retrieve, query, delete, list_namespaces, stats.

use crate::agent_tools::register_agent_tools;
use crate::blob_catalog_tool::register_blob_catalog_tool;
use crate::blob_vectors_tool::register_blob_vectors_tools;
use crate::memory_store::{CognitiveMemoryStore, EntryQuery, NamespaceKind};
use crate::qdrant_shuttle::QdrantSemanticShuttle;
use anyhow::{Context, Result};
use async_trait::async_trait;
use op_mcp::tool_registry::{BoxedTool, Tool, ToolRegistry};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::sync::Arc;

// One bounded process-wide owner, including separately constructed MemoryTools.
// This serializes memory operations without retaining payloads or keyed locks.
static MEMORY_OPERATIONS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
            .register(Arc::new(GeminiQueryTool {
                fallback: crate::gemini_fallback::GeminiFallback::new(),
            }) as BoxedTool)
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

/// Explicit provider operation; never selects the chatbot's default route.
pub struct GeminiQueryTool {
    fallback: crate::gemini_fallback::GeminiFallback,
}

#[async_trait]
impl Tool for GeminiQueryTool {
    fn name(&self) -> &str {
        "gemini_query"
    }
    fn description(&self) -> &str {
        "Explicit Gemini query or bounded deep research using configured provider credentials."
    }
    fn category(&self) -> &str {
        "cognitive"
    }
    fn tags(&self) -> Vec<String> {
        vec!["query".into(), "research".into()]
    }
    fn input_schema(&self) -> Value {
        simd_json::serde::to_owned_value(
            &op_plugins::cognitive_plugin_schema().methods["gemini_query"].args,
        )
        .expect("plugin-owned Gemini input schema serializes")
    }
    fn required_capability(&self) -> Option<&str> {
        Some("cognitive.invoke")
    }
    fn subid(&self) -> Option<&str> {
        Some("mut.service.gemini.query@v1")
    }

    async fn execute(&self, input: Value) -> Result<Value> {
        use op_plugins::state_plugins::cognitive::{
            GeminiMode, GeminiQueryOutput, GeminiQueryRequest,
        };
        let request: GeminiQueryRequest = serde_json::from_value(simd_json_to_serde(&input))
            .context("invalid Gemini query request")?;
        anyhow::ensure!(
            !request.query.trim().is_empty(),
            "Gemini query must not be empty"
        );
        anyhow::ensure!(
            (1..=5).contains(&request.depth),
            "research depth must be 1-5"
        );
        let output = tokio::time::timeout(std::time::Duration::from_secs(25), async {
            let context = request.context.as_deref().filter(|s| !s.is_empty());
            match request.mode {
                GeminiMode::Query => {
                    let result = self.fallback.gemini_query(&request.query, context).await?;
                    Ok::<_, anyhow::Error>(GeminiQueryOutput {
                        response: result.answer,
                        citations: result.citations.into_iter().map(|c| c.source).collect(),
                    })
                }
                GeminiMode::DeepResearch => {
                    let result = self
                        .fallback
                        .deep_research(&request.query, context, request.depth.into())
                        .await?;
                    anyhow::ensure!(
                        result.sections.len() == usize::from(request.depth),
                        "deep research incomplete"
                    );
                    let response = result
                        .sections
                        .iter()
                        .map(|s| format!("{}\n{}", s.title, s.content))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    Ok(GeminiQueryOutput {
                        response,
                        citations: result
                            .sections
                            .into_iter()
                            .flat_map(|s| s.citations)
                            .map(|c| c.source)
                            .collect(),
                    })
                }
            }
        })
        .await
        .context("Gemini operation exceeded its 25-second deadline; no complete result")??;
        Ok(serde_to_simd_json(serde_json::to_value(output)?))
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
        let _guard = MEMORY_OPERATIONS.lock().await;
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

        // Clear every prior mirror, including legacy random UUIDs, before
        // replacing authoritative content, even for nonsemantic/empty writes.
        // Failure leaves the old Cozo row available for a retry.
        if let Some(qdrant) = &self.qdrant {
            qdrant.delete_user_memory(&namespace, key).await?;
        }
        let entry = self
            .store
            .store_entry(&namespace, key, value.clone(), tags, None)
            .await?;
        let semantic_mirrored = self
            .mirror_semantic_memory(input, &namespace, key, &entry.id, &value)
            .await
            .context("Cozo memory committed but semantic mirror failed; retry store")?;

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
        let mut items = Vec::new();
        let mut seen_keys = std::collections::HashSet::new();
        for point in results {
            let payload = serde_json::to_value(&point.payload)?;
            let Some(key) = payload.get("entry_key").and_then(|v| v.as_str()) else {
                continue;
            };
            if payload.get("container_id").and_then(|v| v.as_str()) != Some(container_id) {
                continue;
            }
            let Some(entry) = self.store.retrieve_entry(container_id, key).await? else {
                continue;
            };
            let point_uuid = point.id.as_ref().and_then(|id| match &id.point_id_options {
                Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(id)) => {
                    Some(id.as_str())
                }
                _ => None,
            });
            // Cozo is authoritative. Current legacy random-ID mirrors remain
            // readable; collapse duplicates within this bounded result batch.
            if !semantic_matches_entry(point_uuid, &payload, container_id, &entry) {
                continue;
            }
            if !seen_keys.insert(key.to_string()) {
                continue;
            }
            items.push(json!({
                "id": format!("{:?}", point.id),
                "score": point.score,
                "payload": serde_to_simd_json(serde_json::to_value(&point.payload)?)
            }));
        }
        let count = items.len();

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

        if let Some(qdrant) = &self.qdrant {
            // Also clean orphaned mirrors when Cozo has no matching row.
            qdrant.delete_user_memory(&namespace, key).await?;
        } else {
            tracing::warn!(
                "Qdrant unconfigured: deleting Cozo memory only; remote cleanup unverified"
            );
        }
        let deleted = self.store.delete_entry(&namespace, key).await.context(
            "semantic cleanup completed or unavailable but Cozo deletion failed; retry delete",
        )?;
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
        entry_id: &str,
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
            .upsert_user_memory(entry_id.to_string(), vector, container_id, key, &content)
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

fn semantic_matches_entry(
    _point_id: Option<&str>,
    payload: &serde_json::Value,
    namespace: &str,
    entry: &crate::memory_store::MemoryEntry,
) -> bool {
    entry.namespace_id == namespace
        && payload.get("container_id").and_then(|v| v.as_str()) == Some(namespace)
        && payload.get("entry_key").and_then(|v| v.as_str()) == Some(entry.key.as_str())
        && payload.get("content").and_then(|v| v.as_str())
            == Some(memory_content(&entry.value).as_str())
}

#[cfg(test)]
mod semantic_lifecycle_tests {
    use super::*;

    #[tokio::test]
    async fn gemini_rejects_empty_query_and_unbounded_depth_before_provider_access() {
        let tool = GeminiQueryTool {
            fallback: crate::gemini_fallback::GeminiFallback::new(),
        };
        for input in [
            json!({"query":" ","depth":3}),
            json!({"query":"valid","depth":6}),
        ] {
            assert!(tool.execute(input).await.is_err());
        }
        let schema = tool.input_schema();
        assert!(field(&schema, "properties").get("query").is_some());
        assert_eq!(tool.required_capability(), Some("cognitive.invoke"));
    }

    #[test]
    fn qdrant_payload_serializes_as_native_values() {
        let payload: qdrant_client::Payload = serde_json::json!({
            "container_id":"session/a/personal", "entry_key":"k", "content":"current"
        })
        .try_into()
        .unwrap();
        let point = qdrant_client::qdrant::PointStruct::new(
            uuid::Uuid::new_v4().to_string(),
            vec![1.0],
            payload,
        );
        assert_eq!(
            serde_json::to_value(&point.payload).unwrap(),
            serde_json::json!({
                "container_id":"session/a/personal", "entry_key":"k", "content":"current"
            })
        );
    }

    #[tokio::test]
    async fn semantic_results_require_current_authoritative_identity_and_content() {
        let store = CognitiveMemoryStore::new(Arc::new(
            op_cozo_store::CozoGraphShuttle::new_in_memory().unwrap(),
        ))
        .await
        .unwrap();
        store
            .upsert_namespace(
                "session/a/personal",
                NamespaceKind::Custom,
                None,
                None,
                None,
                serde_json::json!({}),
            )
            .await
            .unwrap();
        let old = store
            .store_entry(
                "session/a/personal",
                "k",
                serde_json::json!("old"),
                vec![],
                None,
            )
            .await
            .unwrap();
        let payload =
            serde_json::json!({"container_id":old.namespace_id,"entry_key":"k","content":"old"});
        assert!(semantic_matches_entry(
            Some(&old.id),
            &payload,
            "session/a/personal",
            &old
        ));
        assert!(semantic_matches_entry(
            Some("legacy-random-id"),
            &payload,
            "session/a/personal",
            &old
        ));
        assert!(!semantic_matches_entry(
            Some(&old.id),
            &payload,
            "session/b/personal",
            &old
        ));
        let current = store
            .store_entry(
                "session/a/personal",
                "k",
                serde_json::json!("new"),
                vec![],
                None,
            )
            .await
            .unwrap();
        assert_eq!(old.id, current.id);
        assert!(!semantic_matches_entry(
            Some(&old.id),
            &payload,
            "session/a/personal",
            &current
        ));
        store.delete_entry("session/a/personal", "k").await.unwrap();
        assert!(store
            .retrieve_entry("session/a/personal", "k")
            .await
            .unwrap()
            .is_none());
    }
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
