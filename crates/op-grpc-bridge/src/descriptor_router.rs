//! Router for descriptor-mounted gRPC methods.
//!
//! Intercepts unmounted request paths and routes them through sealed blob
//! descriptors at runtime without rebuild.
//!
//! NOTE: Full implementation deferred to Step 2 Phase 2C.
//! This is a placeholder for module compilation.

use crate::descriptor_mount::DescriptorMountTable;

/// Service that routes unhandled gRPC paths through descriptor-mounted methods.
///
/// Mounted as the fallback service after compiled routes. For each request:
/// 1. Look up the path in the mount table
/// 2. If found, decode/dispatch through the mounted method
/// 3. If not found, return UNIMPLEMENTED
///
/// Full implementation will handle:
/// - Dynamic message decoding using descriptors
/// - Calling call_plugin_method_dynamic dispatch core
/// - Response encoding and gRPC framing
/// - Proper error handling and status codes
pub struct DescriptorRouter {
    mount_table: std::sync::Arc<std::sync::RwLock<std::sync::Arc<DescriptorMountTable>>>,
}

impl DescriptorRouter {
    /// Create a router backed by a mount table.
    pub fn new(
        mount_table: std::sync::Arc<std::sync::RwLock<std::sync::Arc<DescriptorMountTable>>>,
    ) -> Self {
        Self { mount_table }
    }

    /// Look up a mounted method by gRPC path.
    pub fn lookup(&self, path: &str) -> Option<()> {
        let table = self.mount_table.read().ok()?;
        table.lookup(path).map(|_| ())
    }
}
