//! Provider-agnostic embedding contracts and native request adapters.
//!
//! One nonsecret runtime configuration is persisted in RuntimeDocuments and
//! read for every canonical request. Credentials stay in the environment.

use anyhow::Result;
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;

#[path = "embedding_model_methods.rs"]
pub mod methods;
pub use methods::{
    dispatch_embedding_model_method, embedding_model_state, restored_embedding_state,
};

/// Default public Voyage AI endpoint. The only copy of this constant — consumers
/// (e.g. `op-cognitive-mcp`) resolve their call-time endpoint through
/// [`voyage_embed_params`] rather than hardcoding it themselves.
const VOYAGE_PUBLIC_URL: &str = "https://api.voyageai.com/v1/embeddings";
/// MongoDB-hosted Voyage AI endpoint (for keys prefixed with `al-`). The only
/// copy of this constant.
const VOYAGE_MONGODB_URL: &str = "https://ai.mongodb.com/v1/embeddings";

/// Top-level state for the generic embedding surface.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.embedding-model.schema@v1"))]
#[schemars(extend("x-oscal-category" = "llm"))]
pub struct EmbeddingModelState {
    /// Durable configuration revision; zero means environment bootstrap.
    #[serde(default)]
    pub config_revision: u64,
    /// Embedding provider / runtime — the only provider-aware field
    /// ("voyage", "ollama", …).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.embedding-model.provider@v1"))]
    pub provider: String,
    /// Active embedding model identifier, e.g. "voyage-4", "voyage-4-lite".
    /// Swappable; resolved from config / provider, never hardcoded.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.embedding-model.model-id@v1"))]
    pub model_id: String,
    /// Provider endpoint URL.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.embedding-model.endpoint@v1"))]
    pub endpoint: String,
    /// Shared output dimensionality of the embedding space. The voyage-4 trio
    /// all emit 1024-dim vectors into one Qdrant collection; one dim per space.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "mut.service.embedding-model.dimensions@v1"))]
    pub dimensions: u32,
    /// Configuration snapshots are "unprobed". Credential presence is not
    /// evidence that the remote embedding provider is reachable.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.embedding-model.status@v1"))]
    pub status: String,
    /// Models reported available by the provider (observed live where the
    /// provider supports enumeration; empty for cloud providers that don't).
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.embedding-model.available-models@v1"))]
    pub available_models: Vec<String>,
    /// Content digest of the active model as reported by the provider (observed).
    /// Pins the exact weights the surface is bound to for audit / provenance;
    /// empty for providers that don't expose a digest.
    #[serde(default)]
    #[schemars(extend("x-oscal-subid" = "obs.software.embedding-model.model-digest@v1"))]
    pub model_digest: String,
}

/// Interchangeable Voyage-4 embedding models — the canonical Voyage catalog.
///
/// `voyage-4-large`, `voyage-4` and `voyage-4-lite` share **one** embedding
/// space: vectors from any of the three are directly comparable, so all three
/// write to the *same* Qdrant collection at the *same* dimensionality
/// ([`VoyageModel::SHARED_EMBEDDING_DIMS`]). Switching models is a quality/cost
/// tradeoff only — it never requires re-vectorizing existing data.
///
/// This legacy catalog remains the compatibility set for the existing shared
/// collection. The generic dispatcher accepts other configured models without
/// claiming that their vectors share this collection's embedding space.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VoyageModel {
    #[serde(rename = "voyage-4-large")]
    Voyage4Large,
    #[serde(rename = "voyage-4")]
    Voyage4,
    #[serde(rename = "voyage-4-lite")]
    Voyage4Lite,
}

impl Default for VoyageModel {
    fn default() -> Self {
        // POC target; cheapest of the shared-space trio.
        VoyageModel::Voyage4Lite
    }
}

impl VoyageModel {
    /// The single shared embedding dimensionality for the interchangeable
    /// voyage-4 trio. One collection, no re-index across models.
    pub const SHARED_EMBEDDING_DIMS: u32 = 1024;

    /// The interchangeable models, all in the shared embedding space.
    pub const SHARED_SPACE: [VoyageModel; 3] = [
        VoyageModel::Voyage4Large,
        VoyageModel::Voyage4,
        VoyageModel::Voyage4Lite,
    ];

    /// Canonical Voyage API model id.
    pub fn model_id(&self) -> &'static str {
        match self {
            VoyageModel::Voyage4Large => "voyage-4-large",
            VoyageModel::Voyage4 => "voyage-4",
            VoyageModel::Voyage4Lite => "voyage-4-lite",
        }
    }

    /// Output dimensionality. Identical across the trio so their vectors fuse
    /// and compare in the same Qdrant collection.
    pub fn dimensions(&self) -> u32 {
        Self::SHARED_EMBEDDING_DIMS
    }
}

/// Secret call parameters for the legacy Voyage-specific shared collection.
/// Deliberately not serializable, schema-derived, or Debug-printable.
pub struct VoyageEmbedParams {
    pub endpoint: String,
    pub model_id: String,
    pub api_key: String,
}

/// Incompatible providers, dimensions, model spaces, and credential origins
/// are rejected instead of sending Voyage requests to a different protocol.
pub fn voyage_embed_params() -> Option<VoyageEmbedParams> {
    let key = EmbeddingModelPlugin::voyage_api_key()?;
    let state = match op_core::projection_shm::read_projection_bytes("embedding_model") {
        Some(bytes) => serde_json::from_slice::<EmbeddingModelState>(&bytes).ok()?,
        None => methods::EmbeddingConfig::from_environment().ok()?.state(0),
    };
    let model_id = legacy_voyage_model(&state)?;
    methods::validate_voyage_credential_endpoint(&state.endpoint, &key).ok()?;
    Some(VoyageEmbedParams {
        endpoint: state.endpoint,
        model_id,
        api_key: key,
    })
}

fn legacy_voyage_model(state: &EmbeddingModelState) -> Option<String> {
    if state.provider != "voyage" || state.dimensions != VoyageModel::SHARED_EMBEDDING_DIMS {
        return None;
    }
    let model_id = if state.model_id.is_empty() {
        VoyageModel::default().model_id().to_string()
    } else {
        state.model_id.clone()
    };
    if !VoyageModel::SHARED_SPACE
        .iter()
        .any(|model| model.model_id() == model_id)
    {
        return None;
    }
    Some(model_id)
}

pub struct EmbeddingModelPlugin;
impl Default for EmbeddingModelPlugin {
    fn default() -> Self {
        Self
    }
}

impl EmbeddingModelPlugin {
    pub fn new() -> Self {
        Self
    }
    fn voyage_api_key() -> Option<String> {
        [
            "COGNITIVE_MCP_VOYAGE_API_KEY",
            "VOYAGE_API_KEY",
            "VOYAGE_API_KEY_RUST",
        ]
        .iter()
        .find_map(|key| {
            std::env::var(key)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
    }
    fn voyage_url_for_key(key: &str) -> String {
        if key.trim().starts_with("al-") {
            VOYAGE_MONGODB_URL
        } else {
            VOYAGE_PUBLIC_URL
        }
        .to_string()
    }
}

#[async_trait]
impl StatePlugin for EmbeddingModelPlugin {
    fn name(&self) -> &str {
        "embedding_model"
    }
    fn version(&self) -> &str {
        "2.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = embedding_model_schema();
        super::common::oscal::ensure_category_metadata_fields(&mut schema);
        Some(schema)
    }
    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "schema-declared".to_string(),
                desired_hash: "schema-declared".to_string(),
            },
        })
    }
    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        anyhow::bail!("embedding configuration changes require set_model or update_config")
    }
    async fn verify_state(&self, desired: &Value) -> Result<bool> {
        let actual = simd_json::serde::to_owned_value(embedding_model_state().await?)?;
        Ok(&actual == desired)
    }
    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(embedding_model_state().await?)?,
            backend_checkpoint: None,
        })
    }
    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        anyhow::bail!("embedding configuration rollback requires an explicit update_config")
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

/// Every input/output is plugin-owned and seals with this versioned contract.
pub(crate) fn embedding_model_schema() -> PluginSchema {
    use super::plugin_scaffold_helpers::method_decl_from_schemars_with_output;
    use methods::*;
    use op_state_store::SideEffect;
    let root = serde_json::to_value(schemars::schema_for!(EmbeddingModelState))
        .expect("embedding state schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "embedding_model",
        "2.0.0",
        "Native embeddings through a configured Voyage, Ollama, or OpenAI-compatible provider",
        &root,
    );
    macro_rules! method {
        ($name:literal, $input:ty, $output:ty, $effect:ident, $cap:literal, $subid:literal) => {
            schema.methods.insert(
                $name.into(),
                method_decl_from_schemars_with_output::<$input, $output>(
                    $name,
                    SideEffect::$effect,
                    true,
                    $cap,
                    $subid,
                ),
            );
        };
    }
    method!(
        "embed",
        EmbedInput,
        EmbedOutput,
        Mutation,
        "embedding.invoke",
        "mut.service.embedding.embed@v1"
    );
    method!(
        "list_models",
        EmptyInput,
        ListModelsOutput,
        Read,
        "embedding.read",
        "obs.service.embedding.model.list@v1"
    );
    method!(
        "get_config",
        EmptyInput,
        ConfigOutput,
        Read,
        "embedding.read",
        "obs.service.embedding.config.get@v1"
    );
    method!(
        "set_model",
        SetModelInput,
        ConfigOutput,
        Mutation,
        "embedding.invoke",
        "mut.service.embedding.model.set@v1"
    );
    method!(
        "update_config",
        UpdateConfigInput,
        ConfigOutput,
        Mutation,
        "embedding.invoke",
        "mut.service.embedding.config.update@v1"
    );
    for (id, description) in [
        (
            "embedding.invoke",
            "Embed texts and change the nonsecret embedding selection",
        ),
        (
            "embedding.read",
            "Read embedding configuration and provider model metadata",
        ),
    ] {
        schema.capabilities.insert(
            id.into(),
            op_state_store::CapabilityDecl {
                id: id.into(),
                description: description.into(),
            },
        );
    }
    schema
}
inventory::submit! {
    crate::default_registry::PluginReg::new("embedding_model", |_ctx| std::sync::Arc::new(EmbeddingModelPlugin::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_voyage_consumer_rejects_other_protocols_and_vector_spaces() {
        let mut state = EmbeddingModelState {
            provider: "voyage".into(),
            model_id: "voyage-4-lite".into(),
            dimensions: 1024,
            ..Default::default()
        };
        assert_eq!(
            legacy_voyage_model(&state).as_deref(),
            Some("voyage-4-lite")
        );
        for provider in ["ollama", "openai", "openai_compatible", "unknown"] {
            state.provider = provider.into();
            assert!(legacy_voyage_model(&state).is_none());
        }
        state.provider = "voyage".into();
        state.dimensions = 512;
        assert!(legacy_voyage_model(&state).is_none());
        state.dimensions = 1024;
        state.model_id = "different-vector-space".into();
        assert!(legacy_voyage_model(&state).is_none());
    }
}
