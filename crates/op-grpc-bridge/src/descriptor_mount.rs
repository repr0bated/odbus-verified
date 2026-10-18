//! Descriptor-mounted route table for sealed blob plugins.
//!
//! This module builds and maintains a runtime routing table from sealed blob
//! descriptors, enabling plugin methods to be called without recompiling the bridge.

use std::collections::HashMap;
use std::sync::Arc;

use prost_reflect::{DescriptorPool, MessageDescriptor};
use serde_json::Value as JsonValue;

use op_state_store::SideEffect;

use crate::proto::FILE_DESCRIPTOR_SET;

/// A sealed method ready to be mounted as a callable gRPC route.
///
/// Holds all information needed to route, validate, and dispatch a call
/// to a method in a sealed blob without needing the method's type signature.
#[derive(Clone)]
pub struct MountedMethod {
    /// Plugin ID from the sealed blob.
    pub plugin_id: String,

    /// Sealed schema document for argument validation.
    pub sealed_schema: Arc<JsonValue>,

    /// The method name (e.g., "start_proxy").
    pub method_name: String,

    /// gRPC service name (e.g., "operation.method.proxy_server.start_proxy").
    pub grpc_service: String,

    /// gRPC method name (e.g., "StartProxy").
    pub grpc_method: String,

    /// Input message descriptor (for decoding request frames).
    pub input_descriptor: MessageDescriptor,

    /// Output message descriptor (for encoding response frames).
    pub output_descriptor: MessageDescriptor,

    /// Required capability to call this method.
    pub required_capability: String,

    /// Whether the method has side effects.
    pub side_effect: SideEffect,

    /// Whether the method is idempotent.
    pub idempotent: bool,
}

/// Mount table for descriptor-driven routes.
///
/// Built once per catalog sync alongside the reflection index, with atomic
/// swaps so callers see consistent tables.
pub struct DescriptorMountTable {
    /// Catalog manifest hash this table was built from.
    /// Used to detect stale tables and trigger resync.
    pub catalog_hash: String,

    /// Descriptor pool containing all blob descriptors + well-known types.
    /// Shared across all mounted methods in this table.
    pub pool: DescriptorPool,

    /// Routes keyed by gRPC path (e.g., "/operation.method.proxy_server.start_proxy/StartProxy").
    /// Multiple methods may attempt the same path; first by plugin_id wins.
    pub routes: HashMap<String, MountedMethod>,
}

impl DescriptorMountTable {
    /// Create an empty mount table.
    pub fn empty() -> Self {
        // Create pool from well-known types descriptor set
        let pool = DescriptorPool::decode(FILE_DESCRIPTOR_SET).unwrap_or_else(|_| {
            // Fallback: empty routes, descriptor lookups will fail gracefully
            DescriptorPool::default()
        });

        Self {
            catalog_hash: String::new(),
            pool,
            routes: HashMap::new(),
        }
    }

    /// Look up a mounted method by gRPC path.
    pub fn lookup(&self, grpc_path: &str) -> Option<&MountedMethod> {
        self.routes.get(grpc_path)
    }

    /// Get the set of advertised gRPC services.
    pub fn advertised_services(&self) -> Vec<String> {
        let mut services = self
            .routes
            .values()
            .map(|method| method.grpc_service.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        services.sort();
        services
    }

    /// Build a mount table from the sealed blob catalog.
    ///
    /// Reads the manifest-pinned catalog, creates entries for each sealed method,
    /// and validates that all entries are mountable (unary, correct path format, etc).
    pub async fn build(_catalog_dir: &str, catalog_hash: &str) -> Self {
        // Phase 2A stub: full implementation follows
        // TODO: Read ManifestPinnedCatalog
        // TODO: Validate each blob's descriptors
        // TODO: Create MountedMethod entries
        // TODO: Check for path collisions
        // TODO: Return populated table

        // For now, return empty table (tests will drive full implementation)
        Self {
            catalog_hash: catalog_hash.to_string(),
            pool: DescriptorPool::decode(FILE_DESCRIPTOR_SET)
                .unwrap_or_else(|_| DescriptorPool::default()),
            routes: HashMap::new(),
        }
    }
}
