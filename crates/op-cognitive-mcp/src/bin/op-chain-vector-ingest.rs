//! Receive hook for vector-dump replication: index received vectors and chain links.
//!
//! Invoked as `exec <program> <received-path> <stream-uuid>` (no subcommand).

use anyhow::{Context, Result};
use op_cognitive_mcp::ChainVectorIndex;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let received_path = std::env::args()
        .nth(1)
        .context("missing received path argument")?;
    let stream_uuid = std::env::args()
        .nth(2)
        .context("missing stream uuid argument")?;
    anyhow::ensure!(!stream_uuid.is_empty(), "stream uuid must be non-empty");

    let received = PathBuf::from(&received_path);
    let root = received
        .parent()
        .context("received path has no parent directory")?;

    let index = ChainVectorIndex::open_received_root(root).await?;
    index.ingest_received(None).await?;
    Ok(())
}
