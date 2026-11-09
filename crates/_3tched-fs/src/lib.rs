//! Shared binary discovery, bounded execution, and the fused filesystem.
//!
//! The fused filesystem is the runtime mirror of the sealed blobs. A read
//! through the mount is the shared-memory object.

// The binary's FUSE engine also uses the descriptor helpers in this module.
#[allow(dead_code)]
mod fuse_buffers;
pub mod dispatch;
pub mod fuse_fs;
pub mod fuse_process;
#[cfg(feature = "ipfs")]
pub mod ipfs;
pub mod model;
pub mod namespace;
pub mod source;
pub mod store;
#[path = "threetched-surface.rs"]
pub mod threetched_surface;
mod fused_mount;

pub use fused_mount::existing_identity_mount;
