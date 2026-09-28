use anyhow::{Context, Result, ensure};
use op_state_store::{FieldType, PluginSchema};
use qdrant_client::qdrant::{
    Condition, Filter, PointStruct, QueryPointsBuilder, RetrievedPoint, ScoredPoint,
    ScrollPointsBuilder, UpsertPointsBuilder,
};
use qdrant_client::{Payload, Qdrant};
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

const DEFAULT_QDRANT_URL: &str = "http://127.0.0.1:6334";
const DEFAULT_COLLECTION_NAME: &str = "ctl_plane_reasoning_episodes";
const DEFAULT_USER_MEMORY_COLLECTION: &str = "user_memory";
const DEFAULT_BLOB_VECTORS_COLLECTION: &str = "blob_vectors";
const DEFAULT_TRACE_LIMIT: u32 = 5;
const DEFAULT_VOYAGE_API_URL: &str = "https://api.voyageai.com/v1/embeddings";
const DEFAULT_VOYAGE_MONGODB_API_URL: &str = "https://ai.mongodb.com/v1/embeddings";
const DEFAULT_VOYAGE_QUERY_MODEL: &str = "voyage-4";
const DEFAULT_VOYAGE_OUTPUT_DIMENSION: u32 = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionTraceContext {
    pub wireguard_pubkey: String,
    pub mutation_index: u64,
    pub genesis: String,
    pub trace_id: String,
}

pub struct QdrantSemanticShuttle {
    client: Qdrant,
    collection_name: String,
    user_memory_collection: String,
    blob_vectors_collection: String,
    voyage_client: VoyageClient,
}

impl QdrantSemanticShuttle {
    /// Initializes the Qdrant gRPC client used by the Accountability Loop.
    pub async fn new() -> Result<Self> {
        let qdrant_url =
            std::env::var("COGNITIVE_MCP_QDRANT_URL").unwrap_or_else(|_| DEFAULT_QDRANT_URL.into());
        let collection_name = std::env::var("COGNITIVE_MCP_QDRANT_COLLECTION")
            .unwrap_or_else(|_| DEFAULT_COLLECTION_NAME.into());
        let user_memory_collection = std::env::var("COGNITIVE_MCP_USER_MEMORY_COLLECTION")
            .unwrap_or_else(|_| DEFAULT_USER_MEMORY_COLLECTION.into());
        let blob_vectors_collection = std::env::var("COGNITIVE_MCP_BLOB_VECTORS_COLLECTION")
            .unwrap_or_else(|_| DEFAULT_BLOB_VECTORS_COLLECTION.into());
        let voyage_client = VoyageClient::from_env()?;

        Self::new_with_clients(
            &qdrant_url,
            collection_name,
            user_memory_collection,
            blob_vectors_collection,
            voyage_client,
        )
        .await
    }

    async fn new_with_clients(
        qdrant_url: &str,
        collection_name: impl Into<String>,
        user_memory_collection: impl Into<String>,
        blob_vectors_collection: impl Into<String>,
        voyage_client: VoyageClient,
    ) -> Result<Self> {
        let collection_name = collection_name.into();
        let user_memory_collection = user_memory_collection.into();
        let blob_vectors_collection = blob_vectors_collection.into();
        let client = Qdrant::from_url(qdrant_url)
            .build()
            .with_context(|| format!("failed to build Qdrant client for {qdrant_url}"))?;

        tokio::time::timeout(std::time::Duration::from_secs(5), client.health_check())
            .await
            .with_context(|| format!("Qdrant health check timed out after 5s at {qdrant_url}"))?
            .with_context(|| {
                format!("failed to reach Qdrant gRPC health endpoint at {qdrant_url}")
            })?;

        tracing::info!(
            qdrant_url,
            collection = %collection_name,
            user_memory_collection = %user_memory_collection,
            blob_vectors_collection = %blob_vectors_collection,
            "Qdrant Semantic Shuttle linked to the gRPC interface"
        );

        Ok(Self {
            client,
            collection_name,
            user_memory_collection,
            blob_vectors_collection,
            voyage_client,
        })
    }

    /// Reads the explicitly selected identity session.
    pub fn current_trace_context(&self) -> Result<SessionTraceContext> {
        let identity = op_identity::configured_identity_session()
            .context("no unambiguous identity session is configured for semantic tracing")?;
        let genesis = identity
            .genesis
            .clone()
            .filter(|value| !value.is_empty())
            .context("configured identity session has no genesis")?;

        Ok(SessionTraceContext {
            wireguard_pubkey: identity.wireguard_pubkey,
            mutation_index: identity.mutation_index,
            genesis,
            trace_id: identity.trace_id,
        })
    }

    /// Renders the active appended PluginSchema into deterministic retrieval text.
    pub fn current_schema_embedding_text(&self) -> Result<String> {
        let schema = read_plugin_schema()?;
        Ok(render_schema_embedding_text(&schema))
    }

    /// Fetches the exact session episodes currently associated with the active trace.
    pub async fn stream_semantic_trace(&self) -> Result<Vec<RetrievedPoint>> {
        self.fetch_trace_episodes(DEFAULT_TRACE_LIMIT).await
    }

    pub async fn fetch_trace_episodes(&self, limit: u32) -> Result<Vec<RetrievedPoint>> {
        let trace = self.current_trace_context()?;
        let response = self
            .client
            .scroll(
                ScrollPointsBuilder::new(self.collection_name.clone())
                    .filter(Filter::must([Condition::matches(
                        "trace_id",
                        trace.trace_id.clone(),
                    )]))
                    .limit(limit)
                    .with_payload(true)
                    .with_vectors(true),
            )
            .await
            .with_context(|| {
                format!(
                    "failed to query Qdrant collection {} for trace {}",
                    self.collection_name, trace.trace_id
                )
            })?;

        tracing::info!(
            trace_id = %trace.trace_id,
            mutation_index = trace.mutation_index,
            matches = response.result.len(),
            "Accountability Loop fetched semantic trace episodes"
        );

        Ok(response.result)
    }

    /// Performs semantic retrieval within the active trace using a schema-derived Voyage query.
    pub async fn search_semantic_trace(&self, limit: u64) -> Result<Vec<ScoredPoint>> {
        let (trace, schema_query_text) = self.active_schema_query_text()?;
        let limit = if limit == 0 {
            u64::from(DEFAULT_TRACE_LIMIT)
        } else {
            limit
        };

        let embedding = self
            .voyage_client
            .embed_query(&schema_query_text)
            .await
            .context("failed to embed active shared-memory schema with Voyage")?;

        let response = self
            .client
            .query(
                QueryPointsBuilder::new(self.collection_name.clone())
                    .query(embedding)
                    .filter(Filter::must([Condition::matches(
                        "trace_id",
                        trace.trace_id.clone(),
                    )]))
                    .limit(limit)
                    .with_payload(true),
            )
            .await
            .with_context(|| {
                format!(
                    "failed semantic query against Qdrant collection {} for trace {}",
                    self.collection_name, trace.trace_id
                )
            })?;

        tracing::info!(
            trace_id = %trace.trace_id,
            mutation_index = trace.mutation_index,
            schema_name = %extract_schema_title(&schema_query_text),
            matches = response.result.len(),
            model = %self.voyage_client.model,
            "Accountability Loop fetched semantic matches from the shared-memory schema projection"
        );

        Ok(response.result)
    }

    // ── User Memory Methods ──────────────────────────────────────────────

    /// Embed text as a query vector (for semantic search)
    pub async fn embed_query_text(&self, text: &str) -> Result<Vec<f32>> {
        self.voyage_client.embed_query(text).await
    }

    /// Embed text as a document vector (for storage in Qdrant)
    pub async fn embed_document(&self, text: &str) -> Result<Vec<f32>> {
        self.voyage_client.embed_document(text).await
    }

    /// Upsert a memory entry into the user_memory collection
    ///
    /// Payload includes `container_id` and `entry_key` for scoped retrieval.
    pub async fn upsert_user_memory(
        &self,
        point_id: impl Into<String>,
        vector: Vec<f32>,
        container_id: &str,
        entry_key: &str,
        content: &str,
    ) -> Result<()> {
        let payload: Payload = serde_json::json!({
            "container_id": container_id,
            "entry_key": entry_key,
            "content": content,
        })
        .try_into()
        .context("failed to build user_memory payload")?;

        let point = PointStruct::new(point_id.into(), vector, payload);

        self.client
            .upsert_points(UpsertPointsBuilder::new(
                self.user_memory_collection.clone(),
                vec![point],
            ))
            .await
            .with_context(|| {
                format!(
                    "failed to upsert user_memory point into collection {}",
                    self.user_memory_collection
                )
            })?;

        tracing::info!(
            collection = %self.user_memory_collection,
            container_id = %container_id,
            entry_key = %entry_key,
            "User memory upserted to Qdrant"
        );

        Ok(())
    }

    /// Semantic search over user_memory scoped to a container_id
    pub async fn search_user_memory(
        &self,
        query_embedding: Vec<f32>,
        container_id: &str,
        limit: u64,
    ) -> Result<Vec<ScoredPoint>> {
        let response = self
            .client
            .query(
                QueryPointsBuilder::new(self.user_memory_collection.clone())
                    .query(query_embedding)
                    .filter(Filter::must([Condition::matches(
                        "container_id",
                        container_id.to_string(),
                    )]))
                    .limit(limit)
                    .with_payload(true),
            )
            .await
            .with_context(|| {
                format!(
                    "failed semantic query against user_memory collection {} for container {}",
                    self.user_memory_collection, container_id
                )
            })?;

        tracing::info!(
            collection = %self.user_memory_collection,
            container_id = %container_id,
            matches = response.result.len(),
            "User memory semantic search completed"
        );

        Ok(response.result)
    }

    // ── Blob Vectors ──────────────────────────────────────────────────────

    /// Rebuilds the `blob_vectors` collection wholesale: embeds every active
    /// plugin's current schema text via Voyage and upserts all points. No
    /// staleness check — the corpus is small enough (<2MB total) that a
    /// wholesale replace-on-refresh is simpler and cheaper than tracking
    /// per-plugin schema hashes.
    pub async fn refresh_blob_vectors(&self) -> Result<RefreshBlobVectorsSummary> {
        let texts = all_blob_embedding_texts()?;
        let mut points = Vec::with_capacity(texts.len());

        for (plugin_id, text) in &texts {
            let vector = self
                .embed_document(text)
                .await
                .with_context(|| format!("failed to embed schema text for plugin '{plugin_id}'"))?;

            let payload: Payload = serde_json::json!({
                "plugin_id": plugin_id,
                "text": text,
            })
            .try_into()
            .context("failed to build blob_vectors payload")?;

            points.push(PointStruct::new(
                plugin_id_to_uuid(plugin_id).to_string(),
                vector,
                payload,
            ));
        }

        let embedded = points.len();
        self.client
            .upsert_points(UpsertPointsBuilder::new(
                self.blob_vectors_collection.clone(),
                points,
            ))
            .await
            .with_context(|| {
                format!(
                    "failed to upsert {embedded} points into collection '{}'",
                    self.blob_vectors_collection
                )
            })?;

        tracing::info!(
            collection = %self.blob_vectors_collection,
            embedded,
            "blob_vectors collection refreshed"
        );

        Ok(RefreshBlobVectorsSummary {
            embedded,
            collection: self.blob_vectors_collection.clone(),
        })
    }

    /// Semantic search over the `blob_vectors` collection. No scoping filter —
    /// unlike user_memory, this is public catalog data, not per-container.
    pub async fn search_blob_vectors(&self, query: &str, limit: u64) -> Result<Vec<ScoredPoint>> {
        let embedding = self
            .embed_query_text(query)
            .await
            .context("failed to embed query for blob_vectors search")?;

        let response = self
            .client
            .query(
                QueryPointsBuilder::new(self.blob_vectors_collection.clone())
                    .query(embedding)
                    .limit(limit)
                    .with_payload(true),
            )
            .await
            .with_context(|| {
                format!(
                    "failed semantic query against collection '{}'",
                    self.blob_vectors_collection
                )
            })?;

        tracing::info!(
            collection = %self.blob_vectors_collection,
            matches = response.result.len(),
            "blob_vectors search completed"
        );

        Ok(response.result)
    }

    fn active_schema_query_text(&self) -> Result<(SessionTraceContext, String)> {
        let trace = self.current_trace_context()?;
        let schema = read_plugin_schema()?;
        Ok((trace, render_schema_embedding_text(&schema)))
    }
}

struct VoyageClient {
    client: Client,
    api_url: String,
    api_key: String,
    model: String,
    output_dimension: u32,
}

impl VoyageClient {
    fn from_env() -> Result<Self> {
        let api_key = std::env::var("COGNITIVE_MCP_VOYAGE_API_KEY")
            .or_else(|_| std::env::var("VOYAGE_API_KEY"))
            .or_else(|_| std::env::var("VOYAGE_API_KEY_RUST"))
            .or_else(|_| voyage_key_from_file().ok_or(std::env::VarError::NotPresent))
            .context(
                "missing Voyage API key: set COGNITIVE_MCP_VOYAGE_API_KEY, VOYAGE_API_KEY, \
                 VOYAGE_API_KEY_RUST, or COGNITIVE_MCP_VOYAGE_KEY_FILE",
            )?;
        let api_url = std::env::var("COGNITIVE_MCP_VOYAGE_API_URL")
            .unwrap_or_else(|_| voyage_url_for_key(&api_key).into());
        let model = std::env::var("COGNITIVE_MCP_VOYAGE_QUERY_MODEL")
            .unwrap_or_else(|_| DEFAULT_VOYAGE_QUERY_MODEL.into());
        let output_dimension = std::env::var("COGNITIVE_MCP_VOYAGE_OUTPUT_DIMENSION")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(DEFAULT_VOYAGE_OUTPUT_DIMENSION);

        Ok(Self {
            client: Client::new(),
            api_url,
            api_key,
            model,
            output_dimension,
        })
    }

    async fn embed_query(&self, input: &str) -> Result<Vec<f32>> {
        self.embed(input, "query").await
    }

    async fn embed_document(&self, input: &str) -> Result<Vec<f32>> {
        self.embed(input, "document").await
    }

    async fn embed(&self, input: &str, input_type: &str) -> Result<Vec<f32>> {
        let body = VoyageEmbeddingRequest {
            input,
            model: &self.model,
            input_type,
            truncation: true,
            output_dimension: self.output_dimension,
            output_dtype: "float",
        };

        let response = self
            .client
            .post(&self.api_url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("failed to call Voyage embeddings API at {}", self.api_url))?
            .error_for_status()
            .context("Voyage embeddings API returned an error status")?;

        let response_json = response
            .json::<Value>()
            .await
            .context("failed to decode Voyage embeddings response")?;

        extract_embedding(&response_json)
    }
}

fn voyage_key_from_file() -> Option<String> {
    let path = std::env::var("COGNITIVE_MCP_VOYAGE_KEY_FILE")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| Path::new(&home).join(".ssh/mongo-voyage"))
        })?;

    let contents = std::fs::read_to_string(&path).ok()?;
    let key = contents
        .lines()
        .map(str::trim)
        .find(|line| {
            !line.is_empty()
                && !line.starts_with('#')
                && !line.starts_with("mdb_sa_id_")
                && !line.starts_with("mdb_sa_sk_")
                && (line.starts_with("al-") || line.starts_with("pa-"))
        })?
        .to_string();

    tracing::info!(path = %path.display(), "Loaded Voyage API key from file");
    Some(key)
}

fn voyage_url_for_key(key: &str) -> &'static str {
    if key.trim().starts_with("al-") {
        DEFAULT_VOYAGE_MONGODB_API_URL
    } else {
        DEFAULT_VOYAGE_API_URL
    }
}

#[derive(Serialize)]
struct VoyageEmbeddingRequest<'a> {
    input: &'a str,
    model: &'a str,
    input_type: &'a str,
    truncation: bool,
    output_dimension: u32,
    output_dtype: &'a str,
}

fn read_plugin_schema() -> Result<PluginSchema> {
    // The sealed blob IS the plugin: the active session schema is read from
    // the plugin's own blob in the SHM catalog. `OP_ACTIVE_SCHEMA_PLUGIN`
    // selects which plugin anchors the session's retrieval text.
    let plugin_id = std::env::var("OP_ACTIVE_SCHEMA_PLUGIN")
        .unwrap_or_else(|_| "ctl_plane_chatbot".to_string());
    if let Some(schema) = op_blob::catalog::read_plugin_schema_shm(&plugin_id) {
        return Ok(schema);
    }
    anyhow::bail!("sealed plugin blob '{plugin_id}' is unavailable")
}

/// Returns (plugin_id, embedding_text) for every active plugin in the SHM
/// blob catalog. The multi-plugin counterpart to `current_schema_embedding_text`,
/// which covers the explicitly selected active plugin.
fn all_blob_embedding_texts() -> Result<Vec<(String, String)>> {
    let ids = op_blob::catalog::read_manifest_plugin_ids_shm()
        .context("SHM blob manifest is unavailable")?;
    Ok(ids
        .into_iter()
        .filter_map(|id| {
            op_blob::catalog::read_plugin_schema_shm(&id)
                .map(|schema| (id, render_schema_embedding_text(&schema)))
        })
        .collect())
}

/// Deterministic UUID v5 from a plugin id, so `refresh_blob_vectors` upserts
/// idempotently in place instead of accumulating duplicate points.
fn plugin_id_to_uuid(plugin_id: &str) -> uuid::Uuid {
    // Project-local namespace for blob_vectors point ids. Fixed and arbitrary
    // (generated once) — must never change, or a refresh would orphan every
    // existing point instead of overwriting it.
    const BLOB_VECTORS_NAMESPACE: uuid::Uuid =
        uuid::Uuid::from_u128(0x8f2b_6c4a_0d1e_4a3f_9b7c_2e5d_1a6f_8c3b);
    uuid::Uuid::new_v5(&BLOB_VECTORS_NAMESPACE, plugin_id.as_bytes())
}

#[derive(Debug, Serialize)]
pub struct RefreshBlobVectorsSummary {
    pub embedded: usize,
    pub collection: String,
}

fn render_schema_embedding_text(schema: &PluginSchema) -> String {
    let mut lines = vec![
        format!("schema_name: {}", schema.name),
        format!("schema_category: {}", schema.category),
        format!("schema_version: {}", schema.version),
        format!("schema_description: {}", schema.description.trim()),
    ];

    let mut tags = schema.tags.clone();
    tags.sort();
    if !tags.is_empty() {
        lines.push(format!("schema_tags: {}", tags.join(", ")));
    }

    let mut immutable_paths = schema.immutable_paths.clone();
    immutable_paths.sort();
    if !immutable_paths.is_empty() {
        lines.push(format!("immutable_paths: {}", immutable_paths.join(", ")));
    }

    let mut dependencies = schema.dependencies.clone();
    dependencies.sort();
    if !dependencies.is_empty() {
        lines.push(format!("dependencies: {}", dependencies.join(", ")));
    }

    let mut field_names = schema.fields.keys().cloned().collect::<Vec<_>>();
    field_names.sort();

    for field_name in field_names {
        let Some(field_schema) = schema.fields.get(&field_name) else {
            continue;
        };

        lines.push(format!(
            "field {}: type={}, required={}, read_only={}, description={}",
            field_name,
            render_field_type(&field_schema.field_type),
            field_schema.required,
            field_schema.read_only,
            field_schema.description.trim()
        ));

        if let Some(condition) = &field_schema.read_only_when {
            lines.push(format!(
                "field {} read_only_when: {}={}",
                field_name, condition.property, condition.value
            ));
        }

        if !field_schema.constraints.is_empty() {
            let constraints = field_schema
                .constraints
                .iter()
                .map(render_constraint)
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("field {} constraints: {}", field_name, constraints));
        }
    }

    lines.join("\n")
}

fn render_field_type(field_type: &FieldType) -> String {
    match field_type {
        FieldType::String => "string".to_string(),
        FieldType::Integer => "integer".to_string(),
        FieldType::Float => "float".to_string(),
        FieldType::Boolean => "boolean".to_string(),
        FieldType::Array(inner) => format!("array<{}>", render_field_type(inner)),
        FieldType::Object(fields) => {
            let mut names = fields.keys().cloned().collect::<Vec<_>>();
            names.sort();
            format!("object<{}>", names.join("|"))
        }
        FieldType::Enum(values) => format!("enum<{}>", values.join("|")),
        FieldType::OneOf(branches) => format!(
            "one_of<{}>",
            branches
                .iter()
                .map(render_field_type)
                .collect::<Vec<_>>()
                .join("|")
        ),
        FieldType::Map(value) => format!("map<string,{}>", render_field_type(value)),
        FieldType::JsonSchema(_) => "json_schema".to_string(),
        FieldType::Any => "any".to_string(),
    }
}

fn render_constraint(constraint: &op_state_store::Constraint) -> String {
    match constraint {
        op_state_store::Constraint::Min { value } => format!("min={value}"),
        op_state_store::Constraint::Max { value } => format!("max={value}"),
        op_state_store::Constraint::Pattern { regex } => format!("pattern={regex}"),
        op_state_store::Constraint::OneOf { values } => {
            format!(
                "one_of={}",
                serde_json::to_string(values).unwrap_or_default()
            )
        }
        op_state_store::Constraint::RequiresField { field } => format!("requires_field={field}"),
        op_state_store::Constraint::Custom { validator } => format!("custom={validator}"),
    }
}

fn extract_schema_title(schema_query_text: &str) -> &str {
    schema_query_text
        .lines()
        .find_map(|line| line.strip_prefix("schema_name: "))
        .unwrap_or("unknown")
}

fn extract_embedding(response_json: &Value) -> Result<Vec<f32>> {
    let Some(embedding_values) = response_json
        .get("data")
        .and_then(Value::as_array)
        .and_then(|entries| entries.first())
        .and_then(|entry| entry.get("embedding"))
        .and_then(Value::as_array)
        .or_else(|| {
            response_json
                .get("embeddings")
                .and_then(Value::as_array)
                .and_then(|entries| entries.first())
                .and_then(Value::as_array)
        })
    else {
        return Err(anyhow::anyhow!(
            "Voyage embeddings response did not contain a usable embedding"
        ));
    };

    let mut embedding = Vec::with_capacity(embedding_values.len());
    for value in embedding_values {
        let number = value
            .as_f64()
            .context("Voyage embedding contained a non-numeric value")?;
        embedding.push(number as f32);
    }

    ensure!(!embedding.is_empty(), "Voyage embedding response was empty");
    Ok(embedding)
}

#[cfg(test)]
mod tests {
    use super::*;
    use op_state_store::{Constraint, FieldSchema, ReadOnlyCondition};
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn plugin_id_to_uuid_is_stable() {
        assert_eq!(
            plugin_id_to_uuid("zeroclaw"),
            plugin_id_to_uuid("zeroclaw"),
            "same plugin id must produce the same UUID every time"
        );
        assert_ne!(
            plugin_id_to_uuid("zeroclaw"),
            plugin_id_to_uuid("antigravity"),
            "different plugin ids must not collide"
        );
    }

    #[test]
    fn should_extract_embedding_from_openai_style_data_payload() {
        let embedding = extract_embedding(&json!({
            "data": [{
                "embedding": [0.25, -0.5, 1.5]
            }]
        }))
        .unwrap();

        assert_eq!(embedding, vec![0.25, -0.5, 1.5]);
    }

    #[test]
    fn should_extract_embedding_from_embeddings_array_payload() {
        let embedding = extract_embedding(&json!({
            "embeddings": [[0.1, 0.2, 0.3]]
        }))
        .unwrap();

        assert_eq!(embedding, vec![0.1, 0.2, 0.3]);
    }

    #[test]
    fn should_render_schema_embedding_text_deterministically() {
        let mut nested_fields = HashMap::new();
        nested_fields.insert(
            "beta".to_string(),
            FieldSchema {
                field_type: FieldType::Boolean,
                required: false,
                description: String::new(),
                default: None,
                example: None,
                constraints: vec![],
                read_only: false,
                read_only_when: None,
            },
        );
        nested_fields.insert(
            "alpha".to_string(),
            FieldSchema {
                field_type: FieldType::String,
                required: false,
                description: String::new(),
                default: None,
                example: None,
                constraints: vec![],
                read_only: false,
                read_only_when: None,
            },
        );

        let mut fields = HashMap::new();
        fields.insert(
            "outcome_class".to_string(),
            FieldSchema {
                field_type: FieldType::Enum(vec!["deny".into(), "allow".into()]),
                required: true,
                description: "Outcome bucket".into(),
                default: None,
                example: None,
                constraints: vec![Constraint::OneOf {
                    values: vec![simd_json::json!("allow"), simd_json::json!("deny")],
                }],
                read_only: false,
                read_only_when: Some(ReadOnlyCondition {
                    property: "sealed".into(),
                    value: "true".into(),
                }),
            },
        );
        fields.insert(
            "tools_consulted".to_string(),
            FieldSchema {
                field_type: FieldType::Array(Box::new(FieldType::Object(nested_fields))),
                required: false,
                description: "Tools touched by the episode".into(),
                default: None,
                example: None,
                constraints: vec![Constraint::Min { value: 1.0 }],
                read_only: true,
                read_only_when: None,
            },
        );

        let schema = PluginSchema {
            name: "ctl-plane-chatbot".into(),
            category: "accountability".into(),
            version: "1.0.0".into(),
            description: "Human reviewable reasoning episodes".into(),
            display_name: None,
            fields,
            public_state: Default::default(),
            field_writes: std::collections::HashMap::new(),
            dependencies: vec!["op-grpc-bridge".into(), "op-state-store".into()],
            example: None,
            immutable_paths: vec!["/episode_id".into()],
            tags: vec!["audit".into(), "pii".into()],
            dialect: op_state_store::DEFAULT_SCHEMA_DIALECT.into(),
            mutation_index: Some(7),
            subids: std::collections::HashMap::new(),
            org: None,
            methods: std::collections::HashMap::new(),
            capabilities: std::collections::HashMap::new(),
            capability_grants: std::collections::HashMap::new(),
            signals: vec![],
            guarantees: op_state_store::PluginCapabilities::default(),
        };

        let rendered = render_schema_embedding_text(&schema);

        assert!(rendered.contains("schema_name: ctl-plane-chatbot"));
        assert!(rendered.contains("schema_category: accountability"));
        assert!(rendered.contains("schema_tags: audit, pii"));
        assert!(rendered.contains("immutable_paths: /episode_id"));
        assert!(rendered.contains("dependencies: op-grpc-bridge, op-state-store"));
        assert!(rendered.contains(
            "field outcome_class: type=enum<deny|allow>, required=true, read_only=false, description=Outcome bucket"
        ));
        assert!(rendered.contains("field outcome_class read_only_when: sealed=true"));
        assert!(rendered.contains("field tools_consulted: type=array<object<alpha|beta>>"));
    }
}
