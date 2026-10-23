//! Receive hook for vector-dump replication: index received vectors and chain links.
//!
//! Invoked as `exec <program> <received-path> <stream-uuid>` (no subcommand).
//! Set OPDBUS_CHAIN_VECTOR_CHECKPOINT to an operator-owned writable file outside
//! the received read-only snapshot. Its parent must already exist. Use a fresh
//! checkpoint per received snapshot; existing indexed points are gap-checked.

use anyhow::{Context, Result};
use op_cognitive::ChainVectorIndex;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let received_path = std::env::args()
        .nth(1)
        .context("missing received path argument")?;
    let stream_uuid = std::env::args()
        .nth(2)
        .context("missing stream uuid argument")?;
    let stream_uuid = uuid::Uuid::parse_str(&stream_uuid).context("invalid stream uuid")?;

    let received = PathBuf::from(&received_path);
    let checkpoint = std::env::var("OPDBUS_CHAIN_VECTOR_CHECKPOINT")
        .context("OPDBUS_CHAIN_VECTOR_CHECKPOINT must name a writable checkpoint outside the received snapshot")?;
    let index = ChainVectorIndex::open_received_root_with_checkpoint(
        &received, &PathBuf::from(checkpoint), &stream_uuid.to_string(),
    ).await?;
    index.ingest_received(None).await?;
    Ok(())
}
