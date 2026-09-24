//! op-snowball: Streaming snowball with Btrfs subvolumes for timing, vectors,
//! and state
//!
//! This crate provides:
//! - Streaming snowball for audit trails
//! - Plugin footprints for change tracking
//! - Btrfs subvolumes for timing, vectors, and state
//! - Automatic snapshots with configurable intervals
//! - Rolling retention policies
//! - btrfs send/receive for replication

#![deny(rustdoc::broken_intra_doc_links)]

pub mod btrfs_delta;
pub mod btrfs_numa_integration;
pub mod footprint;
pub mod replication;
pub mod retention;
pub mod snapshot;
pub mod snowball;

// Re-export main types
pub use btrfs_delta::{find_new_since, generation, received_uuid, FindNewDelta};
pub use btrfs_numa_integration::OptimizedSnowball;
pub use footprint::{BlockEvent, PluginFootprint};
pub use retention::RetentionPolicy;
pub use snapshot::SnapshotInterval;
pub use snowball::{
    decode_vector, encode_vector, parse_block_number, parse_vector_block_number, ChainBlockRef,
    SnowballTransferReport, StreamingSnowball, SNAPSHOT_LABELS,
};

/// Prelude for convenient imports
pub mod prelude {
    pub use super::footprint::{BlockEvent, PluginFootprint};
    pub use super::retention::RetentionPolicy;
    pub use super::snapshot::SnapshotInterval;
    pub use super::snowball::StreamingSnowball;
}
