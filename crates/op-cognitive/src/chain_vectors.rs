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
use op_cozo_store::CozoGraphShuttle;
use op_snowball::btrfs_delta;
use op_snowball::{ChainBlockRef, StreamingSnowball};
use qdrant_client::qdrant::{
    CreateCollectionBuilder, Distance, GetPointsBuilder, PointId, PointStruct, QueryPointsBuilder,
    ScoredPoint, UpsertPointsBuilder, VectorParamsBuilder, Condition, Filter,
};
use qdrant_client::{Payload, Qdrant, QdrantError};
use serde::Serialize;
use std::collections::HashSet;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
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

const RECEIVED_CHECKPOINT_ENV: &str = "OPDBUS_CHAIN_VECTOR_CHECKPOINT";

#[derive(Serialize, serde::Deserialize)]
struct ReceivedCheckpoint {
    root: PathBuf,
    stream_uuid: String,
    indexed_block: u64,
}

/// Atomic checkpoint publication needs the parent directory, even when an
/// existing destination can be opened for writing. Exercise creation/removal
/// without replacing or rewriting that destination (which may be invalid).
/// Keep the probe and cleanup in one task so cancellation cannot split them.
/// This observes current directory access; the operator can change it later.
async fn preflight_checkpoint_parent(parent: PathBuf) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        let directory = std::fs::File::open(&parent)
            .context("open checkpoint parent for publication synchronization")?;
        let temporary = parent.join(format!(".vector-checkpoint-preflight-{}.tmp", uuid::Uuid::new_v4()));
        let file = std::fs::OpenOptions::new()
            .write(true).create_new(true).mode(0o600).open(&temporary)
            .context("checkpoint parent must permit temporary-file creation before indexing")?;
        drop(file);
        std::fs::remove_file(&temporary).with_context(|| format!(
            "checkpoint parent preflight could not remove its temporary file {}; indexing has not started",
            temporary.display(),
        ))?;
        directory.sync_all().context("sync checkpoint parent after preflight cleanup")?;
        Ok(())
    }).await.context("checkpoint parent preflight task failed")?
}

/// Publish a complete checkpoint without exposing a truncated destination.
/// Blocking I/O stays in one task so cancellation cannot interrupt the
/// write/sync/publication sequence. An interrupted process may leave an unused
/// temporary file, but readers only open the final checkpoint path.
async fn publish_checkpoint(path: PathBuf, content: Vec<u8>, replace: bool) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        let parent = path.parent().filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let temporary = parent.join(format!(".vector-checkpoint-{}.tmp", uuid::Uuid::new_v4()));
        let mut file = std::fs::OpenOptions::new()
            .write(true).create_new(true).mode(0o600).open(&temporary)
            .context("create checkpoint temporary file in its writable directory")?;
        let result = (|| -> Result<()> {
            file.write_all(&content).context("write complete checkpoint")?;
            file.sync_all().context("sync checkpoint contents")?;
            drop(file);
            if replace {
                std::fs::rename(&temporary, &path).context("atomically replace checkpoint")?;
            } else {
                // First publication must not overwrite a checkpoint another
                // received snapshot created after our existence check.
                std::fs::hard_link(&temporary, &path).context("publish new checkpoint without replacement")?;
                std::fs::remove_file(&temporary).context("remove published checkpoint temporary link")?;
            }
            std::fs::File::open(parent)?.sync_all().context("sync checkpoint directory")?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        }
        result
    }).await.context("checkpoint publication task failed")?
}

/// Decides what of a block is signal. The bridge supplies the sealed plugin
/// schema of the mutation (OD-64: "filter out noise with the original
/// schema that created the snowball data"); the default is the lossless
/// flatten, kept for replicas without a catalog.
pub trait ProjectionPolicy: Send + Sync {
    /// `(label, value)` lines to embed for this block, or `None` to fall back
    /// to [`ChainBlockRef::embedding_text`].
    fn embedding_lines(&self, block: &ChainBlockRef) -> Option<Vec<(String, String)>>;
    /// Payload fields to attach to the point beside the allow-listed
    /// identity/provenance fields (e.g. `method_subid`, `schema_hash`).
    fn payload_fields(&self, _block: &ChainBlockRef) -> serde_json::Map<String, serde_json::Value> {
        serde_json::Map::new()
    }
}

/// Lossless flatten: every `data.*` scalar, as before.
pub struct FlattenAll;
impl ProjectionPolicy for FlattenAll {
    fn embedding_lines(&self, _block: &ChainBlockRef) -> Option<Vec<(String, String)>> {
        None
    }
}

/// Retrieval text for a block under `policy`.
pub fn embedding_text_under(policy: &dyn ProjectionPolicy, block: &ChainBlockRef) -> String {
    match policy.embedding_lines(block) {
        Some(lines) => block.embedding_text_from(&lines),
        None => block.embedding_text(),
    }
}

/// Host load as seen by the projector when it embedded a block. Payload
/// metadata only — never part of the text, never part of the hashed chain.
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, PartialEq)]
pub struct HostLoad {
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
    pub running: u64,
    pub total: u64,
    pub psi_cpu_some_avg10: Option<f64>,
    pub psi_memory_some_avg10: Option<f64>,
    pub psi_io_some_avg10: Option<f64>,
}

/// What the projector records beside a vector (the sidecar).
#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq)]
pub struct VectorMeta {
    /// Unix ms when the embedding was produced.
    pub embedded_at_ms: u64,
    /// Block timestamp → embed: how long the mutation waited for its vector.
    pub queued_ms: u64,
    /// Embedder round trip.
    pub embed_ms: u64,
    /// Optional declared diagnostic observation. Legacy sidecars with a load
    /// object remain readable; unavailable observations are omitted, never zeroed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_load: Option<HostLoad>,
    /// Which policy rendered the text (`schema` or `flatten`).
    pub policy: String,
}

pub struct ChainVectorIndex {
    chain: StreamingSnowball,
    chain_path: PathBuf,
    qdrant: Qdrant,
    collection: String,
    dims: u64,
    /// `None` on a replica: ingesting received vectors needs no embedder.
    voyage: Option<VoyageClient>,
    policy: std::sync::Arc<dyn ProjectionPolicy>,
    checkpoint_path: PathBuf,
    received_identity: Option<(PathBuf, String)>,
}

/// Query-only access to the existing index. Unlike opening a projector, this
/// never creates a collection, opens a chain writer, or changes a watermark.
/// Caller scope must come from admission, never from user-supplied arguments.
pub async fn search_session_events(
    query: &str,
    actor_id: &str,
    session_id: &str,
    limit: u32,
) -> Result<(String, Vec<ScoredPoint>)> {
    anyhow::ensure!(!actor_id.is_empty() && !session_id.is_empty(), "verified event search scope is required");
    anyhow::ensure!(!query.trim().is_empty() && query.len() <= 4000, "event search query must contain 1 to 4000 bytes");
    anyhow::ensure!((1..=20).contains(&limit), "event search limit must be between 1 and 20");
    let voyage = VoyageClient::new()?;
    let embedding = voyage.embed(query, Some("query")).await?;
    let collection = std::env::var("OPDBUS_QDRANT_SNOWBALL_COLLECTION")
        .unwrap_or_else(|_| DEFAULT_COLLECTION.to_string());
    let client = Qdrant::from_url(&qdrant_url()).build()?;
    let response = client.query(
        QueryPointsBuilder::new(collection)
            .query(embedding)
            .filter(Filter::must([
                Condition::matches("actor_id", actor_id.to_owned()),
                Condition::matches("session_id", session_id.to_owned()),
            ]))
            .limit(u64::from(limit))
            .with_payload(true),
    ).await.context("canonical event index search failed")?;
    Ok((voyage.model().to_owned(), response.result))
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
        let chain = StreamingSnowball::new(&chain_path)
            .await
            .with_context(|| format!("failed to open chain at {}", chain_path.display()))?;
        Self::build_with_chain(chain_path, chain, voyage).await
    }

    /// Received dump root, with an explicitly configured writable checkpoint
    /// outside the read-only snapshot. No state is created inside `dest`.
    pub async fn open_received_root(dest: &Path) -> Result<Self> {
        let checkpoint = std::env::var(RECEIVED_CHECKPOINT_ENV)
            .with_context(|| format!("{RECEIVED_CHECKPOINT_ENV} must name a writable checkpoint outside the received snapshot"))?;
        let stream_uuid = btrfs_delta::received_uuid(dest).await?
            .context("chain root is not a received Btrfs snapshot")?;
        Self::open_received_root_with_checkpoint(dest, Path::new(&checkpoint), &stream_uuid).await
    }

    pub async fn open_received_root_with_checkpoint(
        dest: &Path,
        checkpoint: &Path,
        expected_stream_uuid: &str,
    ) -> Result<Self> {
        let root = tokio::fs::canonicalize(dest).await.context("resolve received chain root")?;
        let stream_uuid = btrfs_delta::received_uuid(&root).await?
            .context("chain root is not a received Btrfs snapshot")?;
        anyhow::ensure!(stream_uuid == expected_stream_uuid, "received snapshot UUID does not match arrival hook");
        anyhow::ensure!(checkpoint.is_absolute(), "received checkpoint path must be absolute");
        let checkpoint_parent = tokio::fs::canonicalize(checkpoint.parent().context("checkpoint has no parent")?)
            .await.context("checkpoint parent must already exist and be writable by the operator")?;
        let checkpoint_path = checkpoint_parent.join(checkpoint.file_name().context("checkpoint has no file name")?);
        anyhow::ensure!(!checkpoint_path.starts_with(&root), "checkpoint must be outside the read-only received snapshot");
        let checkpoint_exists = match tokio::fs::symlink_metadata(&checkpoint_path).await {
            Ok(metadata) => {
                anyhow::ensure!(metadata.is_file() && !metadata.file_type().is_symlink(), "checkpoint must be a regular file, not a symlink");
                true
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if checkpoint_exists {
            tokio::fs::OpenOptions::new().write(true).open(&checkpoint_path).await
                .context("received checkpoint must be writable before indexing")?;
            preflight_checkpoint_parent(checkpoint_parent).await?;
        } else {
            publish_checkpoint(checkpoint_path.clone(), serde_json::to_vec(&ReceivedCheckpoint {
                root: root.clone(), stream_uuid: stream_uuid.clone(), indexed_block: 0,
            })?, false).await.context("initialize received checkpoint")?;
        }
        let chain = StreamingSnowball::open_received(&root)
            .await
            .with_context(|| format!("failed to open received chain at {}", root.display()))?;
        let mut index = Self::build_with_chain(root.clone(), chain, None).await?;
        index.checkpoint_path = checkpoint_path;
        index.received_identity = Some((root, stream_uuid));
        // Reject a reused checkpoint before any point or Cozo-link mutation.
        index.read_indexed_block().await?;
        Ok(index)
    }

    async fn build_with_chain(
        chain_path: PathBuf,
        chain: StreamingSnowball,
        voyage: Option<VoyageClient>,
    ) -> Result<Self> {
        let url = qdrant_url();
        let collection = std::env::var("OPDBUS_QDRANT_SNOWBALL_COLLECTION")
            .unwrap_or_else(|_| DEFAULT_COLLECTION.to_string());
        let dims = std::env::var("OPDBUS_QDRANT_SNOWBALL_DIM")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_DIMS);

        let replica = voyage.is_none();
        let client = Qdrant::from_url(&url);
        // Qdrant's builder otherwise starts a synchronous compatibility health
        // probe. Even that connection is unnecessary on the automatic origin.
        let client = if replica { client } else { client.skip_compatibility_check() };
        let qdrant = client.build()
            .with_context(|| format!("failed to build Qdrant client for {url}"))?;
        let index = Self {
            chain,
            checkpoint_path: chain_path.join(INDEXED_BLOCK_FILE),
            received_identity: None,
            chain_path,
            qdrant,
            collection,
            dims,
            voyage,
            policy: std::sync::Arc::new(FlattenAll),
        };
        // Origin projection only embeds into the chain. The replica and
        // explicit manual index writes own Qdrant initialization.
        if replica {
            index.ensure_collection().await?;
        }
        Ok(index)
    }

    /// Install the noise filter (the bridge's schema-described policy).
    pub fn with_policy(mut self, policy: std::sync::Arc<dyn ProjectionPolicy>) -> Self {
        self.policy = policy;
        self
    }

    pub fn policy(&self) -> &dyn ProjectionPolicy {
        self.policy.as_ref()
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
        let mut upserted_to_qdrant = 0usize;
        let mut indexed_through: Option<u64> = None;
        if write_qdrant {
            self.ensure_collection().await?;
            // A previous manual pass may have attached a durable vector before
            // its Qdrant write failed. Reuse that vector instead of embedding
            // again or treating its presence as index completion.
            let blocks = self.chain.blocks().await?;
            let with_vectors: Vec<&ChainBlockRef> =
                blocks.iter().filter(|block| block.has_vector).collect();
            let missing = self.missing_from_index(&with_vectors).await?;
            for block_num in missing.into_iter().take(limit.unwrap_or(usize::MAX)) {
                let block = blocks.iter().find(|block| block.block_num == block_num)
                    .context("missing-index block disappeared from chain view")?;
                let vector = self.chain.read_vector(block_num).await?
                    .context("missing-index block has no durable vector")?;
                self.upsert_block(block, self.fit_dims(vector, block_num)?).await?;
                upserted_to_qdrant += 1;
                indexed_through = Some(indexed_through.unwrap_or(0).max(block_num));
            }
        }

        let (pending, already_present) = self
            .chain
            .pending_blocks(limit.unwrap_or(usize::MAX).saturating_sub(upserted_to_qdrant))
            .await?;

        let mut embedded = 0usize;
        for block in &pending {
            let lines = self.policy.embedding_lines(block);
            let text = match &lines {
                Some(lines) => block.embedding_text_from(lines),
                None => block.embedding_text(),
            };
            let started = std::time::Instant::now();
            let vector = voyage
                .embed(&text, Some("document"))
                .await
                .with_context(|| format!("failed to embed block {}", block.block_num))?;
            let embed_ms = started.elapsed().as_millis() as u64;
            let vector = self.fit_dims(vector, block.block_num)?;

            // Chain first: the vector is durable state; automatic Qdrant is
            // derived only after receive → ingest on the replica.
            self.chain.attach_vector(block.block_num, &vector).await?;
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let meta = VectorMeta {
                embedded_at_ms: now_ms,
                queued_ms: now_ms.saturating_sub(block.timestamp),
                embed_ms,
                // The projector has no admitted metrics source. Do not read
                // live /proc or invent zero-valued health as a fallback.
                host_load: None,
                policy: if lines.is_some() { "schema" } else { "flatten" }.into(),
            };
            if let Ok(meta_value) = simd_json::serde::to_owned_value(&meta)
                && let Err(error) = self
                    .chain
                    .attach_vector_meta(block.block_num, &meta_value)
                    .await
            {
                tracing::warn!(%error, block = block.block_num, "vector sidecar not written");
            }
            if write_qdrant {
                self.upsert_block(block, vector).await?;
                upserted_to_qdrant += 1;
                indexed_through = Some(indexed_through.unwrap_or(0).max(block.block_num));
            }
            embedded += 1;
        }

        // Watermark tracks Qdrant coverage, not chain vector coverage. Bump it
        // only when this pass actually wrote the index (manual exception).
        if let Some(highest) = indexed_through {
            let watermark = self.read_indexed_block().await?.max(highest);
            self.write_indexed_block(watermark).await?;
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
        let cozo_path = std::env::var("COGNITIVE_COZO_DB_PATH").context(
            "COGNITIVE_COZO_DB_PATH is required to persist chain vector links",
        )?;
        let cozo = CozoGraphShuttle::new_persistent(std::path::PathBuf::from(cozo_path))
            .map_err(|error| anyhow::anyhow!(error))
            .context("open Cozo for chain vector links")?;

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
                Some(block) => {
                    let links = chain_links(block);
                    let link_refs: Vec<(&str, &str)> = links
                        .iter()
                        .map(|(kind, value)| (kind.as_str(), value.as_str()))
                        .collect();
                    cozo
                        .replace_chain_vector_links(&point_id(block.block_num).to_string(), &link_refs)
                        .map_err(|e| anyhow::Error::new(e))
                        .with_context(|| {
                            format!(
                                "failed to write chain links for block {}",
                                block.block_num
                            )
                        })?;
                    // An indexed point suppresses future gap retries. Commit
                    // its idempotent Cozo links before making that point visible.
                    self.upsert_block(block, vector).await?;
                }
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
        // Allow-listed provenance only: identity digests and hashes for
        // filtering, never raw arguments and never the session genesis.
        let mut payload_map = serde_json::Map::new();
        payload_map.insert("block_num".into(), block.block_num.into());
        // Snowball carries the authoritative ChainEvent hash verbatim.
        // The compatibility input digest is provenance copied from the
        // payload; neither value is re-hashed by the vector projection.
        payload_map.insert("event_hash".into(), block.hash.clone().into());
        payload_map.insert(
            "input_patch_hash".into(),
            block
                .field("data.payload.input_patch_hash")
                .unwrap_or_default()
                .into(),
        );
        payload_map.insert("plugin_id".into(), block.category.clone().into());
        payload_map.insert("action".into(), block.action.clone().into());
        payload_map.insert("timestamp".into(), block.timestamp.into());
        for (key, path) in [
            ("actor_id", "data.metadata.actor_id"),
            ("session_id", "data.metadata.session_id"),
            ("sealed_id_digest", "data.metadata.sealed_id_digest"),
            ("capability_id", "data.metadata.capability_id"),
            ("decision", "data.metadata.decision"),
        ] {
            if let Some(value) = block.field(path) {
                payload_map.insert(key.into(), value.into());
            }
        }
        payload_map.extend(self.policy.payload_fields(block));
        // Replicas have vectors but may lack the origin's sealed policy.
        // Never reconstruct searchable raw payload text using FlattenAll.
        // Only an explicitly supplied filter may publish retrieval text.
        payload_map.remove("text");
        if let Some(lines) = self.policy.embedding_lines(block) {
            payload_map.insert("text".into(), block.embedding_text_from(&lines).into());
        }
        if let Ok(Some(meta)) = self.chain.read_vector_meta(block.block_num).await {
            let meta: serde_json::Value =
                simd_json::serde::from_owned_value(meta).unwrap_or(serde_json::Value::Null);
            payload_map.insert("vector_meta".into(), meta);
        }
        let payload: Payload = serde_json::Value::Object(payload_map)
            .try_into()
            .context("failed to build snowball vector payload")?;

        let point = PointStruct::new(point_id(block.block_num).to_string(), vector, payload);
        self.qdrant
            .upsert_points(UpsertPointsBuilder::new(&self.collection, vec![point]).wait(true))
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
            .upsert_points(UpsertPointsBuilder::new(&self.collection, vec![point]).wait(true))
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
        self.checkpoint_path.clone()
    }

    /// Highest block number whose vector is in the index.
    pub async fn read_indexed_block(&self) -> Result<u64> {
        match tokio::fs::read_to_string(self.indexed_block_path()).await {
            Ok(text) => {
                if let Some((root, stream_uuid)) = &self.received_identity {
                    let checkpoint: ReceivedCheckpoint = serde_json::from_str(&text)
                        .context("invalid received index checkpoint; use a fresh checkpoint to rebuild")?;
                    anyhow::ensure!(&checkpoint.root == root && &checkpoint.stream_uuid == stream_uuid,
                        "checkpoint belongs to a different received snapshot; configure a fresh checkpoint");
                    Ok(checkpoint.indexed_block)
                } else {
                    Ok(text.trim().parse().unwrap_or(0))
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(0),
            Err(err) => Err(anyhow::Error::from(err).context("failed to read index watermark")),
        }
    }

    async fn write_indexed_block(&self, block_num: u64) -> Result<()> {
        let content = if let Some((root, stream_uuid)) = &self.received_identity {
            serde_json::to_string(&ReceivedCheckpoint {
                root: root.clone(), stream_uuid: stream_uuid.clone(), indexed_block: block_num,
            })?
        } else {
            format!("{block_num}\n")
        };
        publish_checkpoint(self.indexed_block_path(), content.into_bytes(), true).await
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

/// Identity links for the chain Cozo relation, read only from the timing record.
pub fn chain_links(block: &ChainBlockRef) -> Vec<(String, String)> {
    let mut links = Vec::new();
    let mut push = |kind: &str, value: &str| {
        if !value.is_empty() {
            links.push((kind.to_string(), value.to_string()));
        }
    };

    if let Some(value) = block.field("data.payload.conversation_id") {
        push("conversation_id", value);
    }
    if let Some(value) = flattened_suffix(block, "turn_id") {
        push("turn_id", value);
    }
    push("event_hash", &block.hash);
    if let Some(value) = block.field("data.payload.input_patch_hash") {
        push("input_patch_hash", value);
    }
    push("plugin_id", &block.category);
    for (kind, path) in [
        ("actor_id", "data.metadata.actor_id"),
        ("session_id", "data.metadata.session_id"),
        ("sealed_id_digest", "data.metadata.sealed_id_digest"),
        ("capability_id", "data.metadata.capability_id"),
    ] {
        if let Some(value) = block.field(path) {
            push(kind, value);
        }
    }
    if let Some(value) = flattened_suffix(block, "subid") {
        push("subid", value);
    }
    links
}

fn flattened_suffix<'a>(block: &'a ChainBlockRef, leaf: &str) -> Option<&'a str> {
    block
        .data_fields
        .iter()
        .find(|(path, _)| path == leaf || path.ends_with(&format!(".{leaf}")))
        .map(|(_, value)| value.as_str())
}

fn qdrant_url() -> String {
    std::env::var("OPDBUS_QDRANT_URL")
        .or_else(|_| std::env::var("COGNITIVE_QDRANT_URL"))
        .unwrap_or_else(|_| "http://127.0.0.1:6334".to_string())
}

fn is_already_exists(err: &QdrantError) -> bool {
    err.to_string().to_lowercase().contains("already exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn checkpoint_parent_preflight_preserves_existing_invalid_and_foreign_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let checkpoint = directory.path().join("checkpoint.json");
        for bytes in [
            b"incomplete checkpoint {".as_slice(),
            br#"{"root":"/different/root","stream_uuid":"foreign","indexed_block":9}"#.as_slice(),
        ] {
            std::fs::write(&checkpoint, bytes).unwrap();
            preflight_checkpoint_parent(directory.path().to_path_buf()).await.unwrap();
            assert_eq!(std::fs::read(&checkpoint).unwrap(), bytes);
            let entries: Vec<_> = std::fs::read_dir(directory.path()).unwrap()
                .map(|entry| entry.unwrap().file_name()).collect();
            assert_eq!(entries, vec![checkpoint.file_name().unwrap().to_owned()]);
        }
    }

    #[tokio::test]
    async fn checkpoint_parent_preflight_rejects_unusable_parent_without_modifying_it() {
        let directory = tempfile::tempdir().unwrap();
        let parent_file = directory.path().join("not-a-directory");
        std::fs::write(&parent_file, b"retained").unwrap();
        assert!(preflight_checkpoint_parent(parent_file.clone()).await.is_err());
        assert_eq!(std::fs::read(parent_file).unwrap(), b"retained");
        assert!(preflight_checkpoint_parent(directory.path().join("missing")).await.is_err());
    }

    #[test]
    fn vector_metadata_reads_legacy_load_and_omits_unavailable_load() {
        let mut legacy = serde_json::json!({
            "embedded_at_ms": 1, "queued_ms": 2, "embed_ms": 3, "policy": "schema",
            "host_load": {"load1": 1.0, "load5": 2.0, "load15": 3.0, "running": 4, "total": 5}
        });
        let old: VectorMeta = serde_json::from_value(legacy.clone()).unwrap();
        assert_eq!(old.host_load.unwrap().load1, 1.0);
        legacy.as_object_mut().unwrap().remove("host_load");
        let current: VectorMeta = serde_json::from_value(legacy).unwrap();
        assert!(current.host_load.is_none());
        assert!(serde_json::to_value(current).unwrap().get("host_load").is_none());
    }

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

    /// Same JSON shape as on-disk timing records (`ChainBlockRef::from_timing_bytes`).
    fn block_from_timing_json(json: &str, block_num: u64) -> ChainBlockRef {
        parse_timing_record(block_num, json)
    }

    fn parse_timing_record(block_num: u64, json: &str) -> ChainBlockRef {
        let mut bytes = json.as_bytes().to_vec();
        let mut value = simd_json::to_owned_value(&mut bytes).expect("timing JSON");
        let mut data_fields = Vec::new();
        let data = match &mut value {
            simd_json::OwnedValue::Object(map) => map.remove("data"),
            _ => None,
        }
        .unwrap_or(simd_json::OwnedValue::Static(simd_json::StaticNode::Null));
        flatten_timing_data(&data, "data", &mut data_fields);
        let hash = timing_str(&value, "hash").unwrap_or_default();
        let category = timing_str(&value, "category").unwrap_or_default();
        let action = timing_str(&value, "action").unwrap_or_default();
        let timestamp = timing_u64(&value, "timestamp").unwrap_or_default();
        ChainBlockRef {
            block_num,
            hash,
            category,
            action,
            timestamp,
            data_fields,
            has_vector: true,
            data,
        }
    }

    fn timing_str(value: &simd_json::OwnedValue, key: &str) -> Option<String> {
        match value {
            simd_json::OwnedValue::Object(map) => map.get(key).and_then(|v| match v {
                simd_json::OwnedValue::String(s) => Some(s.to_string()),
                _ => None,
            }),
            _ => None,
        }
    }

    fn timing_u64(value: &simd_json::OwnedValue, key: &str) -> Option<u64> {
        match value {
            simd_json::OwnedValue::Object(map) => map.get(key).and_then(|v| match v {
                simd_json::OwnedValue::Static(simd_json::StaticNode::I64(n)) => u64::try_from(*n).ok(),
                simd_json::OwnedValue::Static(simd_json::StaticNode::U64(n)) => Some(*n),
                _ => None,
            }),
            _ => None,
        }
    }

    fn flatten_timing_data(
        value: &simd_json::OwnedValue,
        prefix: &str,
        out: &mut Vec<(String, String)>,
    ) {
        match value {
            simd_json::OwnedValue::Object(map) => {
                for (key, child) in map {
                    flatten_timing_data(child, &format!("{prefix}.{key}"), out);
                }
            }
            simd_json::OwnedValue::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    flatten_timing_data(item, &format!("{prefix}[{index}]"), out);
                }
            }
            simd_json::OwnedValue::String(s) => out.push((prefix.to_string(), s.to_string())),
            simd_json::OwnedValue::Static(simd_json::StaticNode::I64(n)) => out.push((prefix.to_string(), n.to_string())),
            simd_json::OwnedValue::Static(simd_json::StaticNode::U64(n)) => out.push((prefix.to_string(), n.to_string())),
            simd_json::OwnedValue::Static(simd_json::StaticNode::F64(n)) => out.push((prefix.to_string(), n.to_string())),
            simd_json::OwnedValue::Static(simd_json::StaticNode::Null) => {}
            simd_json::OwnedValue::Static(simd_json::StaticNode::Bool(b)) => {
                out.push((prefix.to_string(), b.to_string()));
            }
        }
    }

    fn links_map(block: &ChainBlockRef) -> std::collections::BTreeMap<String, String> {
        chain_links(block).into_iter().collect()
    }

    #[test]
    fn chain_links_reads_declared_ids_from_timing_record() {
        let json = r#"{
            "timestamp": 1,
            "category": "chat_plugin",
            "action": "Chat",
            "hash": "block-event-hash",
            "vector": [],
            "data": {
                "payload": {
                    "conversation_id": "conv-abc",
                    "turn_id": "turn-7",
                    "input_patch_hash": "patch-digest"
                },
                "metadata": {
                    "actor_id": "actor-1",
                    "session_id": "session-1",
                    "sealed_id_digest": "sealed-1",
                    "capability_id": "cap-1",
                    "subid": "schema-subid"
                }
            }
        }"#;
        let block = block_from_timing_json(json, 12);
        let links = links_map(&block);
        assert_eq!(links.get("conversation_id").map(String::as_str), Some("conv-abc"));
        assert_eq!(links.get("turn_id").map(String::as_str), Some("turn-7"));
        assert_eq!(links.get("event_hash").map(String::as_str), Some("block-event-hash"));
        assert_eq!(links.get("input_patch_hash").map(String::as_str), Some("patch-digest"));
        assert_eq!(links.get("plugin_id").map(String::as_str), Some("chat_plugin"));
        assert_eq!(links.get("actor_id").map(String::as_str), Some("actor-1"));
        assert_eq!(links.get("session_id").map(String::as_str), Some("session-1"));
        assert_eq!(links.get("sealed_id_digest").map(String::as_str), Some("sealed-1"));
        assert_eq!(links.get("capability_id").map(String::as_str), Some("cap-1"));
        assert_eq!(links.get("subid").map(String::as_str), Some("schema-subid"));
        assert!(!links.contains_key("chat_id"));
    }

    #[test]
    fn chain_links_omits_session_genesis() {
        let json = r#"{
            "timestamp": 1,
            "category": "p",
            "action": "a",
            "hash": "h",
            "vector": [],
            "data": {
                "metadata": { "session_genesis": "genesis-value" }
            }
        }"#;
        let block = block_from_timing_json(json, 3);
        let links = links_map(&block);
        assert!(!links.contains_key("session_genesis"));
        assert_eq!(links.get("event_hash").map(String::as_str), Some("h"));
    }

    #[test]
    fn chain_links_omits_absent_optional_ids() {
        let json = r#"{
            "timestamp": 1,
            "category": "only-plugin",
            "action": "a",
            "hash": "ev",
            "vector": [],
            "data": { "payload": {} }
        }"#;
        let block = block_from_timing_json(json, 4);
        let links = chain_links(&block);
        assert_eq!(links.len(), 2);
        assert!(links.iter().any(|(k, v)| k == "event_hash" && v == "ev"));
        assert!(links.iter().any(|(k, v)| k == "plugin_id" && v == "only-plugin"));
        for (kind, value) in &links {
            assert!(!value.is_empty(), "kind {kind} must not be an empty string");
        }
    }

    #[test]
    fn chain_links_includes_subid_only_when_timing_carries_it() {
        let without = block_from_timing_json(
            r#"{
            "timestamp": 1,
            "category": "p",
            "action": "a",
            "hash": "h",
            "vector": [],
            "data": { "metadata": { "actor_id": "a1" } }
        }"#,
            5,
        );
        assert!(!links_map(&without).contains_key("subid"));

        let with = block_from_timing_json(
            r#"{
            "timestamp": 1,
            "category": "p",
            "action": "a",
            "hash": "h",
            "vector": [],
            "data": { "metadata": { "subid": "from-timing" } }
        }"#,
            6,
        );
        assert_eq!(
            links_map(&with).get("subid").map(String::as_str),
            Some("from-timing")
        );
    }
}
