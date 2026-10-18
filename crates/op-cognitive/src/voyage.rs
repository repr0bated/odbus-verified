use anyhow::{ensure, Context, Result};
use futures::StreamExt;
use op_plugins::state_plugins::embedding_model::methods::validate_voyage_credential_endpoint;
use op_plugins::state_plugins::embedding_model::{voyage_embed_params, VoyageModel};
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Voyage AI client for text embeddings
pub struct VoyageClient {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl VoyageClient {
    /// The active model id, for logging/tracing only.
    pub fn model(&self) -> &str {
        &self.model
    }
}

#[derive(Serialize)]
struct EmbeddingRequest<'a> {
    input: Vec<&'a str>,
    model: &'a str,
    input_type: Option<&'a str>,
    /// Pinned to the shared-space dimension so callers always land in the same
    /// Qdrant collection regardless of which model in the trio is active —
    /// without this, Voyage returns each model's native (differing) dimension.
    truncation: bool,
    output_dimension: u32,
}

#[derive(Deserialize)]
struct EmbeddingData {
    index: usize,
    embedding: Vec<f32>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

impl VoyageClient {
    /// Construct directly from resolved call-time parameters (endpoint,
    /// model, key) — the primary path, used when the `embedding_model`
    /// plugin's config is available.
    pub fn from_params(endpoint: String, api_key: String, model: String) -> Result<Self> {
        validate_voyage_credential_endpoint(&endpoint, &api_key)?;
        Ok(Self {
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(30))
                .connect_timeout(std::time::Duration::from_secs(5))
                .no_proxy()
                .build()?,
            api_key,
            model,
            base_url: endpoint,
        })
    }

    /// Create a new Voyage client by resolving config through the
    /// `embedding_model` plugin (projection, falling back to its own env-var
    /// reads — see [`op_plugins::state_plugins::embedding_model::voyage_embed_params`]).
    pub fn new() -> Result<Self> {
        let params = voyage_embed_params().context(
            "Voyage credentials are missing or embedding_model's current \
             provider, endpoint, dimensions, or vector space is incompatible",
        )?;
        Self::from_params(params.endpoint, params.api_key, params.model_id)
    }

    /// Embed text using Voyage API
    pub async fn embed(&self, text: &str, input_type: Option<&str>) -> Result<Vec<f32>> {
        validate_voyage_credential_endpoint(&self.base_url, &self.api_key)?;
        let req = EmbeddingRequest {
            input: vec![text],
            model: &self.model,
            input_type,
            truncation: true,
            output_dimension: VoyageModel::SHARED_EMBEDDING_DIMS,
        };

        let response = self
            .client
            .post(&self.base_url)
            .bearer_auth(&self.api_key)
            .json(&req)
            .send()
            .await
            .context("Failed to send Voyage API request")?
            .error_for_status()
            .context("Voyage API returned error status")?;
        ensure!(
            response.status().is_success(),
            "Voyage API returned a redirect"
        );
        const MAX_BYTES: usize = 1024 * 1024;
        ensure!(
            response
                .content_length()
                .is_none_or(|size| size <= MAX_BYTES as u64),
            "Voyage response is too large"
        );
        let mut bytes = Vec::new();
        let mut chunks = response.bytes_stream();
        while let Some(chunk) = chunks.next().await {
            let chunk = chunk.context("Voyage response body failed")?;
            ensure!(
                bytes.len().saturating_add(chunk.len()) <= MAX_BYTES,
                "Voyage response is too large"
            );
            bytes.extend_from_slice(&chunk);
        }
        let resp: EmbeddingResponse =
            serde_json::from_slice(&bytes).context("Failed to parse Voyage API response")?;
        ensure!(
            resp.data.len() == 1 && resp.data[0].index == 0,
            "Voyage returned an invalid embedding cardinality or index"
        );
        ensure!(
            resp.data[0].embedding.len() == VoyageModel::SHARED_EMBEDDING_DIMS as usize
                && resp.data[0]
                    .embedding
                    .iter()
                    .all(|number| number.is_finite()),
            "Voyage returned incompatible embedding dimensions or numeric values"
        );

        resp.data
            .into_iter()
            .next()
            .map(|d| d.embedding)
            .context("Voyage API returned no embeddings")
    }
}
