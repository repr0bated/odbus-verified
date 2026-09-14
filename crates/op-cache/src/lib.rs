//! op-cache: BTRFS-based caching with NUMA awareness and agent orchestration.
//!
//! Features:
//! - BTRFS subvolume cache management
//! - NUMA-aware memory allocation
//! - Snapshot management for rollback
//! - Agent registry with capabilities
//! - Workstack orchestration with caching
//! - Pattern tracking for workstack promotion
//! - gRPC services

pub mod agent;
pub mod agent_registry;
pub mod btrfs_cache;
pub mod capability_resolver;
pub mod numa;
pub mod orchestrator;
pub mod pattern_tracker;
pub mod snapshot_manager;
pub mod workflow_cache;
pub mod workflow_executor;
pub mod workflow_tracker;
pub mod workstack_cache;

pub mod grpc;

pub use agent::{Agent, AgentRegistry, Capability, Priority};
pub use btrfs_cache::BtrfsCache;
pub use numa::{NumaMemoryPolicy, NumaNode, NumaOptimizer, NumaTopology};
pub use orchestrator::Orchestrator;
pub use pattern_tracker::PatternTracker;
pub use snapshot_manager::SnapshotManager;
pub use workstack_cache::WorkstackCache;

pub mod proto {
    tonic::include_proto!("op_cache");

    /// Combined FileDescriptorSet for reflection.
    pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("cache_descriptor");
}

/// Prelude for convenient imports.
pub mod prelude {
    pub use super::agent::{Agent, AgentRegistry, Capability, Priority};
    pub use super::btrfs_cache::BtrfsCache;
    pub use super::numa::{NumaMemoryPolicy, NumaNode, NumaOptimizer, NumaTopology};
    pub use super::orchestrator::Orchestrator;
    pub use super::pattern_tracker::PatternTracker;
    pub use super::snapshot_manager::SnapshotManager;
    pub use super::workstack_cache::WorkstackCache;
}
