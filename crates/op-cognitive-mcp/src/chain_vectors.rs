//! Snowball vector projection: chain block -> Voyage embedding -> chain
//! vector subvolume, with Qdrant filled on the replica after `btrfs receive`.
//!
//! **Strict automatic invariant:** the streaming / default `project` path must
//! **not** upsert Qdrant on the origin. Vectors land in the chain first
//! ([`StreamingSnowball::attach_vector`] / `vectors/vec-*.bin`); the automatic
//! pipeline fills `snowball_footprints` only via **`btrfs receive` →
//! [`ChainVectorIndex::ingest_received`]** (replica hook). Orthogonal paths
//! (`blob_vectors`, user_memory, RAG shuttle) are unrelated and untouched.
//!
//! Split by role:
//!
//! - **Origin, automatic** ([`ChainVectorIndex::project_pending`] with
//!   `upsert_qdrant = false`): embed pending blocks with Voyage, write raw LE
//!   f32 into the `vectors` subvolume, stop. No Qdrant write, no index
//!   watermark bump. Snapshots carry the vectors to the replica.
//!
//! - **Replica, automatic** ([`ChainVectorIndex::ingest_received`]): after
//!   incremental `btrfs receive`, upsert only vectors that arrived (watermark +
//!   gap check). No Voyage key, no re-embedding.
//!
//! - **Manual exception** (`project_pending(..., upsert_qdrant = true)` /
//!   CLI `--upsert-qdrant`, or [`ChainVectorIndex::reindex_from_chain`]):
//!   explicit direct Qdrant write that **bypasses send/receive**. Not the
//!   default streaming path.
//!
//! Qdrant remains a disposable index: rebuild from any chain (or received
//! snapshot) that already holds the vectors.

use anyhow::{Context, Result};
use op_snowball::btrfs_delta;
use op_snowball::{ChainBlockRef, StreamingSnowball};
use qdrant_client::qdrant::{
    CreateCollectionBuilder, Distance, GetPointsBuilder, PointId, PointStruct, QueryPointsBuilder,
    ScoredPoint, UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::{Payload, Qdrant, QdrantError};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::voyage::VoyageClient;

const DEFAULT_CHAIN_PATH: &str = "/var/lib/opdbus/snowball";
const DEFAULT_COLLECTION: &str = "snowball_footprints";
const DEFAULT_DIMS: u64 = 1024;

/// Namespace for deriving deterministic Qdrant point ids from block hashes.
/// Fixed forever: changing it orphans every previously indexed point instead of
/// overwriting it.
const BLOCK_POINT_NAMESPACE: uuid::Uuid =
    uuid::Uuid::from_u128(0x6d1f_28c7_4b93_4f0a_9e57_31c8_ab4d_7e62);

/// File holding the highest block number whose vector has been indexed.
const INDEXED_BLOCK_FILE: &str = ".vector-index-block";

pub struct ChainVectorIndex {
    chain: StreamingSnowball,
    chain_path: PathBuf,
    qdrant: Qdrant,
    collection: String,
    dims: u64,
    /// `None` on a replica: ingesting received vectors needs no embedder.
    voyage: Option<VoyageClient>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ProjectionSummary {
    pub embedded: usize,
    pub already_present: usize,
    /// Points written directly to Qdrant on this pass.
    ///
    /// Always `0` for the automatic project path. Non-zero only when the
    /// explicit manual `--upsert-qdrant` / `upsert_qdrant: true` exception is set
    /// (bypasses send/receive).
    pub upserted_to_qdrant: usize,
    pub collection: String,
}

/// Whether an origin `project` call may write Qdrant directly.
///
/// Automatic pipeline always returns `false`. Only the explicit manual
/// exception (`--upsert-qdrant`) returns `true`.
pub fn project_writes_qdrant(upsert_qdrant: bool) -> bool {
    upsert_qdrant
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct IngestSummary {
    pub upserted: usize,
    pub skipped: usize,
    /// Highest block number already indexed before this pass.
    pub from_block: u64,
    /// Highest block number indexed after it — the persisted watermark.
    pub to_block: u64,
    /// btrfs generation of the vector subvolume, for observability only.
    pub generation: u64,
    pub collection: String,
}

impl ChainVectorIndex {
    /// Origin side: embeds into the chain (and optionally Qdrant when the
    /// caller passes the manual upsert flag). Requires a Voyage key.
    pub async fn open() -> Result<Self> {
        let voyage = VoyageClient::new().context(
            "snowball vector projection needs a Voyage key (see embedding_model plugin)",
        )?;
        Self::build(Some(voyage)).await
    }

    /// Replica / automatic index path: indexes vectors that arrived in a btrfs
    /// stream. No embedder, so it works on a host that has no Voyage credentials
    /// at all.
    pub async fn open_replica() -> Result<Self> {
        Self::build(None).await
    }

    async fn build(voyage: Option<VoyageClient>) -> Result<Self> {
        let chain_path = PathBuf::from(
            std::env::var("OPDBUS_SNOWBALL_PATH")
                .unwrap_or_else(|_| DEFAULT_CHAIN_PATH.to_string()),
        );
        let url = qdrant_url();
        let collection = std::env::var("OPDBUS_QDRANT_SNOWBALL_COLLECTION")
            .unwrap_or_else(|_| DEFAULT_COLLECTION.to_string());
        let dims = std::env::var("OPDBUS_QDRANT_SNOWBALL_DIM")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_DIMS);

        let chain = StreamingSnowball::new(&chain_path)
            .await
            .with_context(|| format!("failed to open chain at {}", chain_path.display()))?;

        let qdrant = Qdrant::from_url(&url)
            .build()
            .with_context(|| format!("failed to build Qdrant client for {url}"))?;
        qdrant
            .health_check()
            .await
            .with_context(|| format!("Qdrant unreachable at {url}"))?;

        let index = Self {
            chain,
            chain_path,
            qdrant,
            collection,
            dims,
            voyage,
        };
        index.ensure_collection().await?;
        Ok(index)
    }

    /// Create the collection on first use. Idempotent, and tolerant of a
    /// concurrent creator.
    async fn ensure_collection(&self) -> Result<()> {
        if self
            .qdrant
            .collection_exists(&self.collection)
            .await
            .with_context(|| format!("failed to probe collection '{}'", self.collection))?
        {
            return Ok(());
        }

        match self
            .qdrant
            .create_collection(
                CreateCollectionBuilder::new(&self.collection)
                    .vectors_config(VectorParamsBuilder::new(self.dims, Distance::Cosine)),
            )
            .await
        {
            Ok(_) => {
                tracing::info!(
                    collection = %self.collection,
                    dims = self.dims,
                    "created Qdrant collection for snowball vectors"
                );
                Ok(())
            }
            Err(err) if is_already_exists(&err) => Ok(()),
            Err(err) => Err(anyhow::Error::from(err)
                .context(format!("failed to create collection '{}'", self.collection))),
        }
    }

    /// Embed every block that has no vector yet and write it into the chain.
    ///
    /// **Automatic default** (`upsert_qdrant = false`): Voyage + `attach_vector`
    /// only. Does **not** upsert Qdrant and does **not** advance the Qdrant
    /// watermark — the replica's `ingest_received` owns that after `btrfs
    /// receive`.
    ///
    /// **Manual exception** (`upsert_qdrant = true`): also upserts Qdrant on
    /// this host and advances the watermark. That bypasses send/receive; use
    /// only via an explicit CLI flag / CallMethod, never as the streaming
    /// default.
    ///
    /// `limit` caps one pass so a large backlog can be worked through in
    /// bounded chunks.
    pub async fn project_pending(
        &self,
        limit: Option<usize>,
        upsert_qdrant: bool,
    ) -> Result<ProjectionSummary> {
        let voyage = self
            .voyage
            .as_ref()
            .context("project_pending requires an embedder; this index was opened as a replica")?;
        let write_qdrant = project_writes_qdrant(upsert_qdrant);

        let all = self.chain.blocks().await?;
        let already_present = all.iter().filter(|block| block.has_vector).count();
        let pending: Vec<ChainBlockRef> = all
            .into_iter()
            .filter(|block| !block.has_vector)
            .take(limit.unwrap_or(usize::MAX))
            .collect();

        let mut embedded = 0usize;
        let mut upserted_to_qdrant = 0usize;
        for block in &pending {
            let vector = voyage
                .embed(&block.embedding_text(), Some("document"))
                .await
                .with_context(|| format!("failed to embed block {}", block.block_num))?;
            let vector = self.fit_dims(vector, block.block_num)?;

            // Chain first: the vector is durable state; automatic Qdrant is
            // derived only after receive → ingest on the replica.
            self.chain.attach_vector(block.block_num, &vector).await?;
            if write_qdrant {
                self.upsert_block(block, vector).await?;
                upserted_to_qdrant += 1;
            }
            embedded += 1;
        }

        // Watermark tracks Qdrant coverage, not chain vector coverage. Bump it
        // only when this pass actually wrote the index (manual exception).
        if write_qdrant {
            if let Some(highest) = pending.iter().map(|block| block.block_num).max() {
                let watermark = self.read_indexed_block().await?.max(highest);
                self.write_indexed_block(watermark).await?;
            }
        }

        tracing::info!(
            collection = %self.collection,
            embedded,
            already_present,
            upserted_to_qdrant,
            write_qdrant,
            "snowball vector projection pass complete"
        );

        Ok(ProjectionSummary {
            embedded,
            already_present,
            upserted_to_qdrant,
            collection: self.collection.clone(),
        })
    }

    /// Manual: rebuild the index from vectors already in the chain, without
    /// embedding and **without** waiting for `btrfs receive`.
    ///
    /// Disaster-recovery / operator catch-up on a host that already has chain
    /// vectors on disk. This is **not** the automatic replication path —
    /// prefer `ingest_received` after receive when filling the replica index.
    /// No Voyage key and no re-embedding spend.
    pub async fn reindex_from_chain(&self) -> Result<IngestSummary> {
        let blocks = self.chain.blocks().await?;
        let mut upserted = 0usize;
        let mut skipped = 0usize;

        for block in &blocks {
            if !block.has_vector {
                skipped += 1;
                continue;
            }
            match self.chain.read_vector(block.block_num).await? {
                Some(vector) => {
                    let vector = self.fit_dims(vector, block.block_num)?;
                    self.upsert_block(block, vector).await?;
                    upserted += 1;
                }
                None => skipped += 1,
            }
        }

        let checkpoint = blocks
            .iter()
            .filter(|block| block.has_vector)
            .map(|block| block.block_num)
            .max()
            .unwrap_or(0);
        self.write_indexed_block(checkpoint).await?;

        let generation = btrfs_delta::generation(self.chain.vector_subvolume_path())
            .await
            .unwrap_or(0);

        tracing::info!(
            collection = %self.collection,
            upserted,
            skipped,
            generation,
            "rebuilt snowball vector index from the chain"
        );

        Ok(IngestSummary {
            upserted,
            skipped,
            from_block: 0,
            to_block: checkpoint,
            generation,
            collection: self.collection.clone(),
        })
    }

    /// Replica side: index the vectors that arrived, and only those.
    ///
    /// The delta is the chain's own append-only block numbering, not a btrfs
    /// generation. Generations cannot answer this across a replication series:
    /// each incremental `btrfs receive` materialises a *new* subvolume whose
    /// entire tree carries that receive's transaction id, so `find-new` reports
    /// every file as new even when the stream was 123 bytes.
    ///
    /// Backfilled vectors (a vector attached to an older block after the
    /// watermark moved past it) and points lost from the disposable index are
    /// caught by a gap check: whenever the watermark leaves any vector
    /// unaccounted for, one batched retrieve per 256 blocks confirms what is
    /// actually present and re-upserts only the difference.
    ///
    /// Pass `None` to resume from the persisted watermark (`0` on first run,
    /// which indexes everything present).
    pub async fn ingest_received(&self, from_block: Option<u64>) -> Result<IngestSummary> {
        let watermark = match from_block {
            Some(block) => block,
            None => self.read_indexed_block().await?,
        };

        let blocks = self.chain.blocks().await?;
        let with_vectors: Vec<&ChainBlockRef> =
            blocks.iter().filter(|block| block.has_vector).collect();

        let mut pending: Vec<u64> = with_vectors
            .iter()
            .filter(|block| block.block_num > watermark)
            .map(|block| block.block_num)
            .collect();

        if pending.len() < with_vectors.len() {
            pending.extend(self.missing_from_index(&with_vectors).await?);
            pending.sort_unstable();
            pending.dedup();
        }

        let mut upserted = 0usize;
        let mut skipped = 0usize;
        for block_num in &pending {
            let Some(vector) = self.chain.read_vector(*block_num).await? else {
                skipped += 1;
                continue;
            };
            let vector = self.fit_dims(vector, *block_num)?;

            match blocks.iter().find(|block| block.block_num == *block_num) {
                Some(block) => self.upsert_block(block, vector).await?,
                // Vector present without its timing record (partial or
                // out-of-order receive). Index it by block number so a later
                // pass can enrich it rather than dropping it.
                None => self.upsert_orphan_vector(*block_num, vector).await?,
            }
            upserted += 1;
        }

        let checkpoint = with_vectors
            .iter()
            .map(|block| block.block_num)
            .max()
            .unwrap_or(watermark)
            .max(watermark);
        self.write_indexed_block(checkpoint).await?;

        let generation = btrfs_delta::generation(self.chain.vector_subvolume_path())
            .await
            .unwrap_or(0);

        tracing::info!(
            collection = %self.collection,
            upserted,
            skipped,
            from_block = watermark,
            to_block = checkpoint,
            generation,
            "indexed received snowball vectors"
        );

        Ok(IngestSummary {
            upserted,
            skipped,
            from_block: watermark,
            to_block: checkpoint,
            generation,
            collection: self.collection.clone(),
        })
    }

    /// Block numbers whose point is absent from the index, asked in one batched
    /// retrieve per chunk rather than by scrolling the whole collection.
    async fn missing_from_index(&self, blocks: &[&ChainBlockRef]) -> Result<Vec<u64>> {
        const CHUNK: usize = 256;
        let mut missing = Vec::new();

        for chunk in blocks.chunks(CHUNK) {
            let ids: Vec<PointId> = chunk
                .iter()
                .map(|block| point_id(block.block_num).to_string().into())
                .collect();
            let found = self
                .qdrant
                .get_points(
                    GetPointsBuilder::new(&self.collection, ids)
                        .with_payload(false)
                        .with_vectors(false),
                )
                .await
                .with_context(|| format!("failed to probe '{}' for gaps", self.collection))?;

            let present: HashSet<String> = found
                .result
                .into_iter()
                .filter_map(|point| {
                    point.id.and_then(|id| match id.point_id_options {
                        Some(qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u)) => Some(u),
                        Some(qdrant_client::qdrant::point_id::PointIdOptions::Num(n)) => {
                            Some(n.to_string())
                        }
                        None => None,
                    })
                })
                .collect();
            for block in chunk {
                let id_str = point_id(block.block_num).to_string();
                if !present.contains(&id_str) {
                    missing.push(block.block_num);
                }
            }
        }

        Ok(missing)
    }

    /// Semantic search over indexed blocks. Origin-side only (needs a query
    /// embedding).
    pub async fn search(&self, query: &str, limit: u64) -> Result<Vec<ScoredPoint>> {
        let voyage = self
            .voyage
            .as_ref()
            .context("search requires an embedder; this index was opened as a replica")?;
        let embedding = voyage
            .embed(query, Some("query"))
            .await
            .context("failed to embed search query")?;
        let embedding = self.fit_dims(embedding, 0)?;

        let response = self
            .qdrant
            .query(
                QueryPointsBuilder::new(&self.collection)
                    .query(embedding)
                    .limit(limit)
                    .with_payload(true),
            )
            .await
            .with_context(|| format!("query against '{}' failed", self.collection))?;
        Ok(response.result)
    }

    /// Whether this chain arrived through `btrfs receive` (i.e. we are the
    /// replication target) rather than being produced locally.
    pub async fn is_replica(&self) -> Result<bool> {
        Ok(
            btrfs_delta::received_uuid(self.chain.vector_subvolume_path())
                .await?
                .is_some(),
        )
    }

    pub fn collection(&self) -> &str {
        &self.collection
    }

    pub fn chain_path(&self) -> &Path {
        &self.chain_path
    }

    async fn upsert_block(&self, block: &ChainBlockRef, vector: Vec<f32>) -> Result<()> {
        let payload: Payload = serde_json::json!({
            "block_num": block.block_num,
            // Snowball carries the authoritative ChainEvent hash verbatim.
            // The compatibility input digest is provenance copied from the
            // payload; neither value is re-hashed by the vector projection.
            "event_hash": block.hash,
            "input_patch_hash": block
                .field("data.payload.input_patch_hash")
                .unwrap_or_default(),
            "plugin_id": block.category,
            "action": block.action,
            "timestamp": block.timestamp,
            "text": block.embedding_text(),
        })
        .try_into()
        .context("failed to build snowball vector payload")?;

        let point = PointStruct::new(point_id(block.block_num).to_string(), vector, payload);
        self.qdrant
            .upsert_points(UpsertPointsBuilder::new(&self.collection, vec![point]))
            .await
            .with_context(|| {
                format!(
                    "failed to upsert block {} into '{}'",
                    block.block_num, self.collection
                )
            })?;
        Ok(())
    }

    async fn upsert_orphan_vector(&self, block_num: u64, vector: Vec<f32>) -> Result<()> {
        let payload: Payload = serde_json::json!({
            "block_num": block_num,
            "timing_record_missing": true,
        })
        .try_into()
        .context("failed to build orphan vector payload")?;

        let point = PointStruct::new(point_id(block_num).to_string(), vector, payload);
        self.qdrant
            .upsert_points(UpsertPointsBuilder::new(&self.collection, vec![point]))
            .await
            .with_context(|| format!("failed to upsert orphan vector for block {block_num}"))?;
        Ok(())
    }

    /// Match the collection width. Voyage's voyage-4 family is Matryoshka, so a
    /// longer vector truncates into the same space; Qdrant's Cosine distance
    /// normalizes, so no renormalization is needed after truncation. A *short*
    /// vector is a real mismatch and is refused rather than zero-padded.
    fn fit_dims(&self, mut vector: Vec<f32>, block_num: u64) -> Result<Vec<f32>> {
        let want = self.dims as usize;
        if vector.len() > want {
            vector.truncate(want);
        }
        anyhow::ensure!(
            vector.len() == want,
            "embedding for block {block_num} has {} dims, collection '{}' expects {want}",
            vector.len(),
            self.collection
        );
        Ok(vector)
    }

    fn indexed_block_path(&self) -> PathBuf {
        self.chain_path.join(INDEXED_BLOCK_FILE)
    }

    /// Highest block number whose vector is in the index.
    pub async fn read_indexed_block(&self) -> Result<u64> {
        match tokio::fs::read_to_string(self.indexed_block_path()).await {
            Ok(text) => Ok(text.trim().parse().unwrap_or(0)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(err) => Err(anyhow::Error::from(err).context("failed to read index watermark")),
        }
    }

    async fn write_indexed_block(&self, block_num: u64) -> Result<()> {
        tokio::fs::write(self.indexed_block_path(), format!("{block_num}\n"))
            .await
            .context("failed to persist index watermark")
    }
}

/// Deterministic point id for a block.
///
/// Keyed on the block number, which is the block's identity on disk and is
/// replicated verbatim, so re-indexing (locally or on a replica) overwrites in
/// place. It deliberately does *not* key on `BlockEvent.hash`: that field holds
/// the input-patch hash, which is identical for every repeat of the same call
/// and would collapse many blocks into one point.
fn point_id(block_num: u64) -> uuid::Uuid {
    uuid::Uuid::new_v5(
        &BLOCK_POINT_NAMESPACE,
        format!("block:{block_num}").as_bytes(),
    )
}

fn qdrant_url() -> String {
    std::env::var("OPDBUS_QDRANT_URL")
        .or_else(|_| std::env::var("COGNITIVE_MCP_QDRANT_URL"))
        .unwrap_or_else(|_| "http://127.0.0.1:6334".to_string())
}

fn is_already_exists(err: &QdrantError) -> bool {
    err.to_string().to_lowercase().contains("already exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_namespace_is_stable_and_distinct() {
        let a = uuid::Uuid::new_v5(&BLOCK_POINT_NAMESPACE, b"hash-a");
        let b = uuid::Uuid::new_v5(&BLOCK_POINT_NAMESPACE, b"hash-b");
        assert_eq!(a, uuid::Uuid::new_v5(&BLOCK_POINT_NAMESPACE, b"hash-a"));
        assert_ne!(a, b);
    }

    /// `QdrantError::ResponseError` holds qdrant-client's tonic `Status`, which
    /// is a different type from the `tonic::Status` this crate's own services
    /// use. The alias is what makes the two nameable in one file.
    #[test]
    fn already_exists_detection_matches_qdrant_wording() {
        assert!(super::is_already_exists(&QdrantError::ResponseError {
            status: tonic::Status::already_exists("Collection `x` already exists!"),
        }));
        assert!(!super::is_already_exists(&QdrantError::ResponseError {
            status: tonic::Status::internal("some other failure"),
        }));
    }

    #[test]
    fn automatic_project_path_does_not_write_qdrant() {
        // Strict automatic: default project must not upsert Qdrant on origin.
        assert!(!project_writes_qdrant(false));
        // Manual exception only: explicit --upsert-qdrant / CallMethod flag.
        assert!(project_writes_qdrant(true));
    }

    #[test]
    fn projection_summary_default_reports_zero_direct_upserts() {
        let summary = ProjectionSummary {
            embedded: 3,
            already_present: 40,
            upserted_to_qdrant: 0,
            collection: DEFAULT_COLLECTION.to_string(),
        };
        assert_eq!(summary.upserted_to_qdrant, 0);
        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["upserted_to_qdrant"], 0);
    }
}
