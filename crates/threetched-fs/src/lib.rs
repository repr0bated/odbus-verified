//! Shared binary discovery and bounded execution, without the FUSE engine.

// The binary's FUSE engine also uses the descriptor helpers in this module.
#[allow(dead_code)]
mod fuse_buffers;
pub mod fuse_process;
#[path = "threetched-surface.rs"]
pub mod threetched_surface;
