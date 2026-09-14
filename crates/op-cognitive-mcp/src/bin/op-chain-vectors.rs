//! Operator CLI for the snowball vector pipeline (`snowball_footprints` only).
//!
//! Strict automatic Qdrant policy applies **only** to this collection — not to
//! `blob_vectors`, user_memory, or RAG shuttle traffic.
//!
//! Origin host (has a Voyage key):
//! ```sh
//! op-chain-vectors status
//! op-chain-vectors project --limit 50              # embed → btrfs vectors only (no Qdrant)
//! op-chain-vectors project --limit 50 --upsert-qdrant  # manual: also write Qdrant directly
//! op-chain-vectors replicate --snapshot /prepared/golden-ro --host offsite.example --remote-path /prepared/receive
//! ```
//!
//! Replica host (no Voyage key needed):
//! ```sh
//! op-chain-vectors ingest                  # Qdrant upsert after btrfs receive (automatic path)
//! ```
//!
//! Configuration is env-driven and shared with the rest of the workspace:
//! `OPDBUS_SNOWBALL_PATH`, `OPDBUS_QDRANT_URL` (or `COGNITIVE_MCP_QDRANT_URL`),
//! `OPDBUS_QDRANT_SNOWBALL_COLLECTION`, `OPDBUS_QDRANT_SNOWBALL_DIM`.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use op_cognitive_mcp::ChainVectorIndex;
use op_snowball::replication::{transfer_prepared_snapshot, PreparedSnapshotTransfer};
use op_snowball::{btrfs_delta, StreamingSnowball};
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(
    name = "op-chain-vectors",
    about = "Embed Snowball block vectors, index them in Qdrant, and transfer prepared filesystem snapshots"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Report chain, vector-coverage and replication state without changing anything.
    Status,
    /// Origin: embed pending blocks into the chain `vectors` data directory.
    ///
    /// Default does **not** write Qdrant (`snowball_footprints` is filled by
    /// replica `ingest` after `btrfs receive`). Pass `--upsert-qdrant` for a
    /// manual direct index write that bypasses send/receive.
    Project {
        /// Cap this pass (default: all pending blocks).
        #[arg(long)]
        limit: Option<usize>,
        /// Manual exception: also upsert `snowball_footprints` on this host.
        /// Does not affect other Qdrant collections.
        #[arg(long, default_value_t = false)]
        upsert_qdrant: bool,
    },
    /// Manual: rebuild `snowball_footprints` from chain vectors (no Voyage, no receive).
    Reindex,
    /// Replica: index the vectors that arrived since the last indexed block.
    Ingest {
        /// Override the persisted watermark (0 reindexes everything present).
        #[arg(long)]
        since_block: Option<u64>,
    },
    /// Create a logical recovery snapshot of the three chain data directories.
    Snapshot,
    /// Transfer one explicitly prepared read-only Btrfs snapshot; create no snapshots.
    Replicate {
        /// Absolute path of the prepared read-only filesystem snapshot.
        #[arg(long)]
        snapshot: PathBuf,
        /// Optional read-only parent on the same filesystem; its unchanged copy
        /// must already exist on the receiver for an incremental transfer.
        #[arg(long)]
        parent: Option<PathBuf>,
        /// Target host, e.g. `opc@oracle-host`.
        #[arg(long)]
        host: String,
        /// Existing absolute Btrfs directory on the target that receives this snapshot.
        #[arg(long)]
        remote_path: String,
        /// Optional absolute remote executable, run as `<program> <received-path>
        /// <stream-uuid>` only after the entire prepared snapshot is received.
        /// Legacy hooks expecting a logical Snowball counter are incompatible.
        #[arg(long)]
        on_receive: Option<String>,
        /// Transfer timeout, also applied separately to the optional arrival hook.
        #[arg(long, default_value_t = 3600)]
        timeout_secs: u64,
    },
    /// Origin: semantic search over indexed blocks.
    Search {
        query: String,
        #[arg(long, default_value_t = 5)]
        limit: u64,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.cmd {
        Cmd::Status => status().await,
        Cmd::Project {
            limit,
            upsert_qdrant,
        } => {
            let index = ChainVectorIndex::open().await?;
            let summary = index.project_pending(limit, upsert_qdrant).await?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
            Ok(())
        }
        Cmd::Reindex => {
            let index = ChainVectorIndex::open_replica().await?;
            let summary = index.reindex_from_chain().await?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
            Ok(())
        }
        Cmd::Ingest { since_block } => {
            let index = ChainVectorIndex::open_replica().await?;
            let summary = index.ingest_received(since_block).await?;
            println!("{}", serde_json::to_string_pretty(&summary)?);
            Ok(())
        }
        Cmd::Snapshot => {
            let chain = open_chain().await?;
            println!("{}", chain.create_snapshot().await?);
            Ok(())
        }
        Cmd::Replicate {
            snapshot,
            parent,
            host,
            remote_path,
            on_receive,
            timeout_secs,
        } => {
            let report = transfer_prepared_snapshot(PreparedSnapshotTransfer {
                snapshot,
                parent,
                host,
                remote_directory: remote_path,
                on_receive,
                timeout: Duration::from_secs(timeout_secs),
            })
            .await?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            if let Some(error) = report.arrival_hook_error {
                anyhow::bail!("snapshot transfer completed; arrival hook failed: {error}");
            }
            Ok(())
        }
        Cmd::Search { query, limit } => {
            let index = ChainVectorIndex::open().await?;
            for point in index.search(&query, limit).await? {
                let block = point
                    .payload
                    .get("block_num")
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                let plugin = point
                    .payload
                    .get("plugin_id")
                    .and_then(|v| v.as_str())
                    .cloned()
                    .unwrap_or_default();
                let action = point
                    .payload
                    .get("action")
                    .and_then(|v| v.as_str())
                    .cloned()
                    .unwrap_or_default();
                println!("{:.4}  block {block}  {plugin}/{action}", point.score);
            }
            Ok(())
        }
    }
}

async fn open_chain() -> Result<StreamingSnowball> {
    let path = std::env::var("OPDBUS_SNOWBALL_PATH")
        .unwrap_or_else(|_| "/var/lib/opdbus/snowball".to_string());
    StreamingSnowball::new(&path)
        .await
        .with_context(|| format!("failed to open chain at {path}"))
}

async fn status() -> Result<()> {
    let chain = open_chain().await?;
    let blocks = chain.blocks().await?;
    let with_vectors = blocks.iter().filter(|block| block.has_vector).count();
    let vectors_subvol = chain.vector_subvolume_path();

    println!("chain:            {}", chain.base_path().display());
    println!("blocks:           {}", blocks.len());
    println!("with vectors:     {with_vectors}");
    println!("missing vectors:  {}", blocks.len() - with_vectors);

    match btrfs_delta::generation(vectors_subvol).await {
        Ok(gen) => println!("vectors gen:      {gen}"),
        Err(err) => println!("vectors gen:      unavailable ({err})"),
    }
    match btrfs_delta::received_uuid(vectors_subvol).await {
        Ok(Some(uuid)) => println!("role:             replica (received {uuid})"),
        Ok(None) => println!("role:             origin (locally produced)"),
        Err(err) => println!("role:             unknown ({err})"),
    }

    if let Some(last) = blocks.last() {
        println!(
            "latest block:     {} {}/{}",
            last.block_num, last.category, last.action
        );
    }

    // Index state needs Qdrant; report the chain half regardless of whether it
    // is reachable, so `status` stays useful during an outage.
    match ChainVectorIndex::open_replica().await {
        Ok(index) => {
            println!(
                "indexed through:  block {}",
                index.read_indexed_block().await?
            );
            println!("collection:       {}", index.collection());
        }
        Err(err) => println!("index:            unavailable ({err:#})"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replicate_requires_explicit_snapshot_and_preserves_optional_parent() {
        assert!(Cli::try_parse_from([
            "op-chain-vectors",
            "replicate",
            "--host",
            "backup",
            "--remote-path",
            "/receive",
        ])
        .is_err());
        let cli = Cli::try_parse_from([
            "op-chain-vectors",
            "replicate",
            "--snapshot",
            "/prepared/current",
            "--parent",
            "/prepared/previous",
            "--host",
            "backup",
            "--remote-path",
            "/receive",
        ])
        .unwrap();
        match cli.cmd {
            Cmd::Replicate {
                snapshot,
                parent,
                timeout_secs,
                ..
            } => {
                assert_eq!(snapshot, PathBuf::from("/prepared/current"));
                assert_eq!(parent, Some(PathBuf::from("/prepared/previous")));
                assert_eq!(timeout_secs, 3600);
            }
            _ => panic!("expected replicate command"),
        }
    }
}
