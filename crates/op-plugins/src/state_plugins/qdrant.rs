//! Qdrant vector search engine plugin — GB.Qdrant.
//!
//! Schema derived from Qdrant's official OpenAPI spec
//! (qdrant/qdrant/docs/redoc/master/openapi.json). Fields mirror the
//! `CollectionInfo`, `CollectionConfig`, `VectorParams`, `HnswConfig`,
//! `OptimizersConfig`, and `CollectionStatus` schemas defined there.

use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
use anyhow::{Context, Result};
use async_trait::async_trait;
use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff, StatePlugin,
};
use op_state_store::{PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::time::Duration;

// =============================================================================
// PLUGIN ENTRY: identity and typed schema seed
// =============================================================================

const PLUGIN_NAME: &str = "qdrant";
const PLUGIN_VERSION: &str = "1.1.0";
const PLUGIN_CATEGORY: &str = "data";
const PLUGIN_DESCRIPTION: &str =
    "Qdrant vector search engine — collections, vector config, HNSW, optimizers";
const PLUGIN_DISPLAY_NAME: &str = "GB.Qdrant";

const DEFAULT_QDRANT_HTTP_ENDPOINT: &str = "http://127.0.0.1:6333";
const DEFAULT_QDRANT_GRPC_ENDPOINT: &str = "http://127.0.0.1:6334";

// ── State structs with schemars::JsonSchema ──────────────────────────────────

/// Qdrant collection info — mirrors `CollectionInfo` from the OpenAPI spec.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QdrantCollectionInfo {
    pub name: String,
    pub status: String,
    pub optimizer_status: String,
    #[serde(default)]
    pub points_count: Option<u64>,
    #[serde(default)]
    pub indexed_vectors_count: Option<u64>,
    pub segments_count: u64,
    pub config: QdrantCollectionConfig,
    pub payload_schema: serde_json::Value,
}

/// Mirrors `CollectionConfig` from the OpenAPI spec.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QdrantCollectionConfig {
    pub params: QdrantCollectionParams,
    pub hnsw_config: QdrantHnswConfig,
    pub optimizer_config: QdrantOptimizersConfig,
    #[serde(default)]
    pub quantization_config: Option<String>,
    #[serde(default)]
    pub wal_config: Option<serde_json::Value>,
    #[serde(default)]
    pub strict_mode_config: Option<serde_json::Value>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

/// Mirrors `CollectionParams` + `VectorsConfig` from the OpenAPI spec.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QdrantCollectionParams {
    pub vectors: QdrantVectorParams,
    #[serde(default)]
    pub shard_number: Option<u32>,
    #[serde(default)]
    pub replication_factor: Option<u32>,
    #[serde(default)]
    pub write_consistency_factor: Option<u32>,
    #[serde(default)]
    pub read_fan_out_factor: Option<u32>,
    #[serde(default)]
    pub on_disk_payload: Option<bool>,
    #[serde(default)]
    pub sparse_vectors: Option<serde_json::Value>,
}

/// Mirrors `VectorParams` from the OpenAPI spec.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QdrantVectorParams {
    pub size: u64,
    pub distance: String,
    #[serde(default)]
    pub hnsw_config: Option<serde_json::Value>,
    #[serde(default)]
    pub quantization_config: Option<String>,
    #[serde(default)]
    pub on_disk: Option<bool>,
    #[serde(default)]
    pub datatype: Option<String>,
    #[serde(default)]
    pub multivector_config: Option<serde_json::Value>,
}

/// Mirrors `HnswConfig` from the OpenAPI spec.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QdrantHnswConfig {
    pub m: u32,
    pub ef_construct: u32,
    pub full_scan_threshold: u32,
    pub max_indexing_threads: u32,
    #[serde(default)]
    pub on_disk: Option<bool>,
    #[serde(default)]
    pub payload_m: Option<u32>,
    #[serde(default)]
    pub m0: Option<u32>,
}

/// Mirrors `OptimizersConfig` from the OpenAPI spec.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QdrantOptimizersConfig {
    pub deleted_threshold: f64,
    pub vacuum_min_vector_number: u64,
    pub default_segment_number: u32,
    pub max_segment_size: u64,
    pub flush_interval_sec: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub indexing_threshold: Option<u64>,
}

/// Top-level Qdrant plugin state.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[schemars(extend("x-oscal-category" = "data"))]
pub struct QdrantState {
    pub version: String,
    pub title: String,
    pub commit: String,
    pub http_endpoint: String,
    pub grpc_endpoint: String,
    pub collections: Vec<QdrantCollectionInfo>,
    #[serde(default)]
    pub cluster_status: Option<serde_json::Value>,
    #[serde(default)]
    pub telemetry: Option<serde_json::Value>,
}

// ── Method input types ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateCollectionInput {
    pub name: String,
    pub vectors_config: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeleteCollectionInput {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListCollectionsInput {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpsertPointsInput {
    pub collection_name: String,
    pub points: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchPointsInput {
    pub collection_name: String,
    pub vector: Vec<f32>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeletePointsInput {
    pub collection_name: String,
    pub points: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScrollPointsInput {
    pub collection_name: String,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateSnapshotInput {
    pub collection_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListSnapshotsInput {
    pub collection_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreatePayloadIndexInput {
    pub collection_name: String,
    pub field_name: String,
    pub field_schema: serde_json::Value,
}

// ── Method output types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListCollectionsOutput {
    pub collections: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchPointsOutput {
    pub results: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScrollPointsOutput {
    pub points: Vec<serde_json::Value>,
    pub next_page_offset: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListSnapshotsOutput {
    pub snapshots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CollectionMutationOutput {
    pub status: String,
    pub result: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OperationMutationOutput {
    pub status: String,
    pub operation_id: u64,
    pub operation_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotDescription {
    pub name: String,
    pub size: u64,
    pub creation_time: String,
    #[serde(default)]
    pub checksum: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateSnapshotOutput {
    pub status: String,
    pub snapshot: SnapshotDescription,
}

struct QdrantHttpClient {
    client: reqwest::Client,
    base: String,
    api_key: Option<String>,
}

impl QdrantHttpClient {
    fn new() -> Result<Self> {
        Self::with_base(QdrantPlugin::http_endpoint())
    }

    fn with_base(endpoint: String) -> Result<Self> {
        let base = endpoint.trim_end_matches('/').to_string();
        let parsed = reqwest::Url::parse(&base).context("invalid QDRANT_HTTP_URL")?;
        anyhow::ensure!(
            matches!(parsed.scheme(), "http" | "https"),
            "QDRANT_HTTP_URL must use http or https"
        );
        anyhow::ensure!(
            parsed.username().is_empty() && parsed.password().is_none(),
            "QDRANT_HTTP_URL must not contain userinfo"
        );
        anyhow::ensure!(
            parsed.query().is_none() && parsed.fragment().is_none(),
            "QDRANT_HTTP_URL must not contain query or fragment"
        );
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .context("build Qdrant HTTP client")?,
            base,
            api_key: std::env::var("QDRANT_API_KEY")
                .ok()
                .filter(|v| !v.is_empty()),
        })
    }

    fn collection_segment(name: &str) -> Result<&str> {
        anyhow::ensure!(
            !name.is_empty() && name != "." && name != "..",
            "collection name must be a concrete segment"
        );
        anyhow::ensure!(name.len() <= 255, "collection name is too long");
        anyhow::ensure!(
            name.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b)),
            "collection name contains unsafe path characters"
        );
        Ok(name)
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
        wait: bool,
    ) -> Result<serde_json::Value> {
        let mut url = format!("{}{}", self.base, path);
        if wait {
            url.push_str(if path.contains('?') {
                "&wait=true"
            } else {
                "?wait=true"
            });
        }
        let mut request = self
            .client
            .request(method, url)
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(key) = &self.api_key {
            request = request.header("api-key", key);
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.context("Qdrant HTTP request")?;
        let status = response.status();
        let payload: serde_json::Value = response.json().await.context("decode Qdrant response")?;
        anyhow::ensure!(
            status.is_success(),
            "Qdrant returned HTTP {}: {}",
            status,
            payload
        );
        anyhow::ensure!(
            payload.get("status").and_then(|v| v.as_str()) == Some("ok"),
            "Qdrant returned unsuccessful response: {}",
            payload
        );
        Ok(payload)
    }

    fn collection_mutation(payload: serde_json::Value) -> Result<serde_json::Value> {
        let result = payload
            .get("result")
            .context("Qdrant collection mutation omitted result")?
            .as_bool()
            .context("Qdrant collection mutation result is not boolean")?;
        Ok(serde_json::to_value(CollectionMutationOutput {
            status: payload["status"]
                .as_str()
                .context("Qdrant response omitted status")?
                .to_string(),
            result,
        })?)
    }

    fn operation_mutation(payload: serde_json::Value) -> Result<serde_json::Value> {
        let result = payload
            .get("result")
            .context("Qdrant operation omitted result")?;
        Ok(serde_json::to_value(OperationMutationOutput {
            status: payload["status"]
                .as_str()
                .context("Qdrant response omitted status")?
                .to_string(),
            operation_id: result["operation_id"]
                .as_u64()
                .context("Qdrant operation omitted operation_id")?,
            operation_status: result["status"]
                .as_str()
                .context("Qdrant operation omitted status")?
                .to_string(),
        })?)
    }

    fn snapshot_mutation(payload: serde_json::Value) -> Result<serde_json::Value> {
        let snapshot: SnapshotDescription = serde_json::from_value(
            payload
                .get("result")
                .cloned()
                .context("Qdrant snapshot omitted result")?,
        )
        .context("invalid Qdrant snapshot description")?;
        Ok(serde_json::to_value(CreateSnapshotOutput {
            status: payload["status"]
                .as_str()
                .context("Qdrant response omitted status")?
                .to_string(),
            snapshot,
        })?)
    }
}

pub async fn dispatch_qdrant_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    let client = QdrantHttpClient::new()?;
    dispatch_qdrant_method_with_client(&client, method, args).await
}

async fn dispatch_qdrant_method_with_client(
    client: &QdrantHttpClient,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    match method {
        "create_collection" => {
            let i: CreateCollectionInput = serde_json::from_value(args.clone())
                .context("invalid create_collection arguments")?;
            let name = QdrantHttpClient::collection_segment(&i.name)?;
            QdrantHttpClient::collection_mutation(
                client
                    .request(
                        reqwest::Method::PUT,
                        &format!("/collections/{name}"),
                        Some(serde_json::json!({"vectors": i.vectors_config})),
                        true,
                    )
                    .await?,
            )
        }
        "delete_collection" => {
            let i: DeleteCollectionInput = serde_json::from_value(args.clone())
                .context("invalid delete_collection arguments")?;
            let name = QdrantHttpClient::collection_segment(&i.name)?;
            QdrantHttpClient::collection_mutation(
                client
                    .request(
                        reqwest::Method::DELETE,
                        &format!("/collections/{name}"),
                        None,
                        true,
                    )
                    .await?,
            )
        }
        "list_collections" => {
            let p = client
                .request(reqwest::Method::GET, "/collections", None, false)
                .await?;
            let collections = p["result"]["collections"]
                .as_array()
                .context("Qdrant collections result is not an array")?
                .iter()
                .map(|v| {
                    v["name"]
                        .as_str()
                        .map(str::to_string)
                        .context("Qdrant collection item omitted name")
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(serde_json::to_value(ListCollectionsOutput { collections })?)
        }
        "upsert_points" => {
            let i: UpsertPointsInput =
                serde_json::from_value(args.clone()).context("invalid upsert_points arguments")?;
            let name = QdrantHttpClient::collection_segment(&i.collection_name)?;
            QdrantHttpClient::operation_mutation(
                client
                    .request(
                        reqwest::Method::PUT,
                        &format!("/collections/{name}/points"),
                        Some(serde_json::json!({"points": i.points})),
                        true,
                    )
                    .await?,
            )
        }
        "search_points" => {
            let i: SearchPointsInput =
                serde_json::from_value(args.clone()).context("invalid search_points arguments")?;
            let name = QdrantHttpClient::collection_segment(&i.collection_name)?;
            let p = client
                .request(
                    reqwest::Method::POST,
                    &format!("/collections/{name}/points/search"),
                    Some(serde_json::json!({"vector": i.vector, "limit": i.limit})),
                    false,
                )
                .await?;
            Ok(serde_json::to_value(SearchPointsOutput {
                results: p["result"]
                    .as_array()
                    .context("Qdrant search result is not an array")?
                    .clone(),
            })?)
        }
        "delete_points" => {
            let i: DeletePointsInput =
                serde_json::from_value(args.clone()).context("invalid delete_points arguments")?;
            let name = QdrantHttpClient::collection_segment(&i.collection_name)?;
            QdrantHttpClient::operation_mutation(
                client
                    .request(
                        reqwest::Method::POST,
                        &format!("/collections/{name}/points/delete"),
                        Some(serde_json::json!({"points": i.points})),
                        true,
                    )
                    .await?,
            )
        }
        "scroll_points" => {
            let i: ScrollPointsInput =
                serde_json::from_value(args.clone()).context("invalid scroll_points arguments")?;
            let name = QdrantHttpClient::collection_segment(&i.collection_name)?;
            let p = client
                .request(
                    reqwest::Method::POST,
                    &format!("/collections/{name}/points/scroll"),
                    Some(serde_json::json!({"limit": i.limit})),
                    false,
                )
                .await?;
            let r = &p["result"];
            Ok(serde_json::to_value(ScrollPointsOutput {
                points: r["points"]
                    .as_array()
                    .cloned()
                    .context("Qdrant scroll points result is not an array")?,
                next_page_offset: r.get("next_page_offset").cloned().filter(|v| !v.is_null()),
            })?)
        }
        "create_snapshot" => {
            let i: CreateSnapshotInput = serde_json::from_value(args.clone())
                .context("invalid create_snapshot arguments")?;
            let name = QdrantHttpClient::collection_segment(&i.collection_name)?;
            QdrantHttpClient::snapshot_mutation(
                client
                    .request(
                        reqwest::Method::POST,
                        &format!("/collections/{name}/snapshots"),
                        None,
                        true,
                    )
                    .await?,
            )
        }
        "list_snapshots" => {
            let i: ListSnapshotsInput =
                serde_json::from_value(args.clone()).context("invalid list_snapshots arguments")?;
            let name = QdrantHttpClient::collection_segment(&i.collection_name)?;
            let p = client
                .request(
                    reqwest::Method::GET,
                    &format!("/collections/{name}/snapshots"),
                    None,
                    false,
                )
                .await?;
            let snapshots = p["result"]
                .as_array()
                .context("Qdrant snapshots result is not an array")?
                .iter()
                .map(|v| {
                    v["name"]
                        .as_str()
                        .map(str::to_string)
                        .context("Qdrant collection item omitted name")
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(serde_json::to_value(ListSnapshotsOutput { snapshots })?)
        }
        "create_payload_index" => {
            let i: CreatePayloadIndexInput = serde_json::from_value(args.clone())
                .context("invalid create_payload_index arguments")?;
            let name = QdrantHttpClient::collection_segment(&i.collection_name)?;
            QdrantHttpClient::operation_mutation(client.request(reqwest::Method::PUT, &format!("/collections/{name}/index"), Some(serde_json::json!({"field_name": i.field_name, "field_schema": i.field_schema})), true).await?)
        }
        _ => anyhow::bail!(
            "unknown Qdrant method '{}'; no canonical backend dispatch",
            method
        ),
    }
}

// =============================================================================
// PLUGIN BODY: D-Bus-backed behavior only
// =============================================================================

pub struct QdrantPlugin;

impl Default for QdrantPlugin {
    fn default() -> Self {
        Self
    }
}

impl QdrantPlugin {
    pub fn new() -> Self {
        Self
    }

    fn http_endpoint() -> String {
        std::env::var("QDRANT_HTTP_URL")
            .unwrap_or_else(|_| DEFAULT_QDRANT_HTTP_ENDPOINT.to_string())
    }

    fn grpc_endpoint() -> String {
        std::env::var("QDRANT_GRPC_URL")
            .unwrap_or_else(|_| DEFAULT_QDRANT_GRPC_ENDPOINT.to_string())
    }

    pub(crate) fn exemplar_state() -> QdrantState {
        QdrantState {
            version: "1.18.2".to_string(),
            title: "qdrant - vector search engine".to_string(),
            commit: String::new(),
            http_endpoint: Self::http_endpoint(),
            grpc_endpoint: Self::grpc_endpoint(),
            collections: vec![QdrantCollectionInfo {
                name: "example_collection".to_string(),
                status: "green".to_string(),
                optimizer_status: "ok".to_string(),
                points_count: Some(0),
                indexed_vectors_count: Some(0),
                segments_count: 0,
                config: QdrantCollectionConfig {
                    params: QdrantCollectionParams {
                        vectors: QdrantVectorParams {
                            size: 1024,
                            distance: "Cosine".to_string(),
                            hnsw_config: None,
                            quantization_config: None,
                            on_disk: Some(false),
                            datatype: Some("float32".to_string()),
                            multivector_config: None,
                        },
                        shard_number: Some(1),
                        replication_factor: Some(1),
                        write_consistency_factor: Some(1),
                        read_fan_out_factor: None,
                        on_disk_payload: Some(false),
                        sparse_vectors: None,
                    },
                    hnsw_config: QdrantHnswConfig {
                        m: 16,
                        ef_construct: 100,
                        full_scan_threshold: 10000,
                        max_indexing_threads: 0,
                        on_disk: Some(false),
                        payload_m: None,
                        m0: None,
                    },
                    optimizer_config: QdrantOptimizersConfig {
                        deleted_threshold: 0.2,
                        vacuum_min_vector_number: 1000,
                        default_segment_number: 0,
                        max_segment_size: 200_000,
                        flush_interval_sec: 10,
                        indexing_threshold: Some(20000),
                    },
                    quantization_config: None,
                    wal_config: None,
                    strict_mode_config: None,
                    metadata: None,
                },
                payload_schema: serde_json::json!({}),
            }],
            cluster_status: None,
            telemetry: None,
        }
    }
}

#[async_trait]
impl StatePlugin for QdrantPlugin {
    fn name(&self) -> &str {
        PLUGIN_NAME
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = qdrant_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: PLUGIN_NAME.to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: PLUGIN_NAME.to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::json!(null),
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

// =============================================================================
// PLUGIN EXIT: publish the single PluginSchema contract
// =============================================================================

pub(crate) fn qdrant_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(QdrantState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        PLUGIN_NAME,
        PLUGIN_VERSION,
        PLUGIN_DESCRIPTION,
        &root,
    );
    schema.category = PLUGIN_CATEGORY.to_string();
    if let Ok(defaults) = simd_json::serde::to_owned_value(QdrantPlugin::exemplar_state()) {
        super::schemars_adapter::apply_state_defaults(&mut schema, &defaults);
    }
    schema.display_name = Some(PLUGIN_DISPLAY_NAME.to_string());
    schema.subids.insert(
        "__schema__".to_string(),
        "sch.software.plugin.qdrant.schema@v1".to_string(),
    );
    schema.subids.insert(
        "version".to_string(),
        "obs.software.plugin.qdrant.version@v1".to_string(),
    );
    schema.subids.insert(
        "title".to_string(),
        "obs.software.plugin.qdrant.title@v1".to_string(),
    );
    schema.subids.insert(
        "commit".to_string(),
        "obs.software.plugin.qdrant.commit@v1".to_string(),
    );
    schema.subids.insert(
        "http_endpoint".to_string(),
        "src.software.plugin.qdrant.http-endpoint@v1".to_string(),
    );
    schema.subids.insert(
        "grpc_endpoint".to_string(),
        "src.software.plugin.qdrant.grpc-endpoint@v1".to_string(),
    );
    schema.subids.insert(
        "collections".to_string(),
        "obs.software.plugin.qdrant.collections@v1".to_string(),
    );
    schema.subids.insert(
        "cluster_status".to_string(),
        "obs.software.plugin.qdrant.cluster-status@v1".to_string(),
    );
    schema.subids.insert(
        "telemetry".to_string(),
        "obs.software.plugin.qdrant.telemetry@v1".to_string(),
    );

    schema.methods.insert(
        "create_collection".to_string(),
        method_decl_from_schemars_with_output::<CreateCollectionInput, CollectionMutationOutput>(
            "create_collection",
            SideEffect::Mutation,
            false,
            "cap.data.qdrant.collection.create@v1",
            "mut.data.qdrant.collection.create@v1",
        ),
    );
    schema.methods.insert(
        "delete_collection".to_string(),
        method_decl_from_schemars_with_output::<DeleteCollectionInput, CollectionMutationOutput>(
            "delete_collection",
            SideEffect::Mutation,
            false,
            "cap.data.qdrant.collection.delete@v1",
            "mut.data.qdrant.collection.delete@v1",
        ),
    );
    schema.methods.insert(
        "list_collections".to_string(),
        method_decl_from_schemars_with_output::<ListCollectionsInput, ListCollectionsOutput>(
            "list_collections",
            SideEffect::Read,
            true,
            "cap.data.qdrant.collection.list@v1",
            "obs.data.qdrant.collection.list@v1",
        ),
    );
    schema.methods.insert(
        "upsert_points".to_string(),
        method_decl_from_schemars_with_output::<UpsertPointsInput, OperationMutationOutput>(
            "upsert_points",
            SideEffect::Mutation,
            false,
            "cap.data.qdrant.point.upsert@v1",
            "mut.data.qdrant.point.upsert@v1",
        ),
    );
    schema.methods.insert(
        "search_points".to_string(),
        method_decl_from_schemars_with_output::<SearchPointsInput, SearchPointsOutput>(
            "search_points",
            SideEffect::Read,
            true,
            "cap.data.qdrant.point.search@v1",
            "obs.data.qdrant.point.search@v1",
        ),
    );
    schema.methods.insert(
        "delete_points".to_string(),
        method_decl_from_schemars_with_output::<DeletePointsInput, OperationMutationOutput>(
            "delete_points",
            SideEffect::Mutation,
            false,
            "cap.data.qdrant.point.delete@v1",
            "mut.data.qdrant.point.delete@v1",
        ),
    );
    schema.methods.insert(
        "scroll_points".to_string(),
        method_decl_from_schemars_with_output::<ScrollPointsInput, ScrollPointsOutput>(
            "scroll_points",
            SideEffect::Read,
            true,
            "cap.data.qdrant.point.scroll@v1",
            "obs.data.qdrant.point.scroll@v1",
        ),
    );
    schema.methods.insert(
        "create_snapshot".to_string(),
        method_decl_from_schemars_with_output::<CreateSnapshotInput, CreateSnapshotOutput>(
            "create_snapshot",
            SideEffect::Mutation,
            false,
            "cap.data.qdrant.snapshot.create@v1",
            "mut.data.qdrant.snapshot.create@v1",
        ),
    );
    schema.methods.insert(
        "list_snapshots".to_string(),
        method_decl_from_schemars_with_output::<ListSnapshotsInput, ListSnapshotsOutput>(
            "list_snapshots",
            SideEffect::Read,
            true,
            "cap.data.qdrant.snapshot.list@v1",
            "obs.data.qdrant.snapshot.list@v1",
        ),
    );
    schema.methods.insert(
        "create_payload_index".to_string(),
        method_decl_from_schemars_with_output::<CreatePayloadIndexInput, OperationMutationOutput>(
            "create_payload_index",
            SideEffect::Mutation,
            false,
            "cap.data.qdrant.payload-index.create@v1",
            "mut.data.qdrant.payload-index.create@v1",
        ),
    );

    schema.capabilities.insert(
        "cap.data.qdrant.collection.create@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.qdrant.collection.create@v1".to_string(),
            description: "Grants: create_collection.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.data.qdrant.collection.delete@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.qdrant.collection.delete@v1".to_string(),
            description: "Grants: delete_collection.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.data.qdrant.collection.list@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.qdrant.collection.list@v1".to_string(),
            description: "Grants: list_collections.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.data.qdrant.point.upsert@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.qdrant.point.upsert@v1".to_string(),
            description: "Grants: upsert_points.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.data.qdrant.point.search@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.qdrant.point.search@v1".to_string(),
            description: "Grants: search_points.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.data.qdrant.point.delete@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.qdrant.point.delete@v1".to_string(),
            description: "Grants: delete_points.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.data.qdrant.point.scroll@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.qdrant.point.scroll@v1".to_string(),
            description: "Grants: scroll_points.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.data.qdrant.snapshot.create@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.qdrant.snapshot.create@v1".to_string(),
            description: "Grants: create_snapshot.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.data.qdrant.snapshot.list@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.qdrant.snapshot.list@v1".to_string(),
            description: "Grants: list_snapshots.".to_string(),
        },
    );
    schema.capabilities.insert(
        "cap.data.qdrant.payload-index.create@v1".to_string(),
        op_state_store::CapabilityDecl {
            id: "cap.data.qdrant.payload-index.create@v1".to_string(),
            description: "Grants: create_payload_index.".to_string(),
        },
    );

    schema
}

inventory::submit! {
    crate::default_registry::PluginReg::new(PLUGIN_NAME, |_ctx| std::sync::Arc::new(QdrantPlugin::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn invoke(method: &str, args: serde_json::Value) -> Result<serde_json::Value> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            let mut request = [0u8; 8192];
            let n = stream.read(&mut request).await?;
            let request = String::from_utf8_lossy(&request[..n]);
            let body = if request.contains("/points/search") {
                serde_json::json!({"result": [], "status": "ok"})
            } else if request.contains("/points/scroll") {
                serde_json::json!({"result": {"points": [], "next_page_offset": null}, "status": "ok"})
            } else if request.contains("/snapshots") && request.starts_with("POST") {
                serde_json::json!({"result": {"name":"demo-2026-01-01T00:00:00Z", "size":0, "creation_time":"2026-01-01T00:00:00Z", "checksum":null}, "status": "ok"})
            } else if request.contains("/points") || request.contains("/index") {
                serde_json::json!({"result": {"operation_id": 7, "status": "completed"}, "status": "ok"})
            } else if request.contains("/snapshots") && request.starts_with("GET") {
                serde_json::json!({"result": [], "status": "ok"})
            } else if request.starts_with("GET /collections ") {
                serde_json::json!({"result": {"collections": [{"name": "demo"}]}, "status": "ok"})
            } else {
                serde_json::json!({"result": true, "status": "ok"})
            };
            let encoded = body.to_string();
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", encoded.len(), encoded);
            stream.write_all(response.as_bytes()).await?;
            anyhow::Ok(())
        });
        let client = QdrantHttpClient::with_base(format!("http://{}", address))?;
        let result = dispatch_qdrant_method_with_client(&client, method, &args).await;
        server.await??;
        result
    }

    #[tokio::test]
    async fn all_declared_methods_use_typed_http_contracts() -> Result<()> {
        let calls = [
            (
                "create_collection",
                serde_json::json!({"name":"demo","vectors_config":{"size":4,"distance":"Cosine"}}),
            ),
            ("delete_collection", serde_json::json!({"name":"demo"})),
            ("list_collections", serde_json::json!({})),
            (
                "upsert_points",
                serde_json::json!({"collection_name":"demo","points":[]}),
            ),
            (
                "search_points",
                serde_json::json!({"collection_name":"demo","vector":[0.1],"limit":2}),
            ),
            (
                "delete_points",
                serde_json::json!({"collection_name":"demo","points":[]}),
            ),
            (
                "scroll_points",
                serde_json::json!({"collection_name":"demo","limit":2}),
            ),
            (
                "create_snapshot",
                serde_json::json!({"collection_name":"demo"}),
            ),
            (
                "list_snapshots",
                serde_json::json!({"collection_name":"demo"}),
            ),
            (
                "create_payload_index",
                serde_json::json!({"collection_name":"demo","field_name":"tenant","field_schema":"keyword"}),
            ),
        ];
        for (method, args) in calls {
            let output = invoke(method, args).await?;
            assert!(
                output.get("success").is_none(),
                "{method} must not use AckOutput"
            );
        }
        Ok(())
    }

    #[test]
    fn collection_names_cannot_escape_http_paths() {
        assert!(QdrantHttpClient::collection_segment("demo").is_ok());
        assert!(QdrantHttpClient::collection_segment(".").is_err());
        assert!(QdrantHttpClient::collection_segment("..").is_err());
        assert!(QdrantHttpClient::collection_segment("../secrets").is_err());
        assert!(QdrantHttpClient::collection_segment("demo/name").is_err());
    }

    #[test]
    fn endpoint_rejects_credentials_and_redirect_targets() {
        assert!(QdrantHttpClient::with_base("http://user:pass@127.0.0.1:6333".into()).is_err());
        assert!(QdrantHttpClient::with_base("http://127.0.0.1:6333/?token=x".into()).is_err());
        assert!(QdrantHttpClient::with_base("http://127.0.0.1:6333/#fragment".into()).is_err());
    }
}
