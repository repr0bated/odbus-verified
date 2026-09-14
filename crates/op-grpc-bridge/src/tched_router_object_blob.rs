//! 3tched Router D-Bus + gRPC object blob prototype.
//!
//! This is the copy-forward shape for coupling D-Bus identity, plugin schema
//! identity, and tonic reflection identity without making D-Bus passthrough part
//! of the new public API surface.

use crate::plugin_object_blob::{blobify_plugin_schema, PluginObjectBlob};
use op_state_store::PluginSchema;

const TCHED_ROUTER_PLUGIN_ID: &str = "tched_router";

/// Immutable bridge-local identity for the 3tched Router object projection.
///
/// `PluginObjectBlob` now lives in the `op-blob` crate, so the constructors
/// are free functions (inherent impls on a foreign type are not allowed).
pub type TchedRouterObjectBlob = PluginObjectBlob;

/// Build the immutable object blob from the plugin-owned 3tched Router schema.
pub fn from_plugin_schema() -> TchedRouterObjectBlob {
    from_schema(op_plugins::state_plugins::tched_router::tched_router_plugin_schema())
}

pub fn from_schema(schema: PluginSchema) -> TchedRouterObjectBlob {
    blobify_plugin_schema(TCHED_ROUTER_PLUGIN_ID, schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tched_router_blob_couples_dbus_schema_and_grpc_identity() {
        let blob = from_plugin_schema();

        assert_eq!(blob.manifest.plugin_id, "tched_router");
        assert_eq!(
            blob.manifest.dbus.object_path,
            "/org/opdbus/v1/plugins/tched_router"
        );
        assert_eq!(blob.manifest.blob_type, "active_reflection");
        assert!(!blob.schema_json.is_empty());
        assert_eq!(blob.manifest.schema_hash.len(), 64);
        assert!(!blob.descriptor_set.is_empty());
        assert!(blob
            .manifest
            .methods
            .iter()
            .any(|method| method.schema_name == "GetState"));
        assert!(blob
            .manifest
            .methods
            .iter()
            .any(|method| method.grpc_name == "GetState"));
        assert!(blob
            .manifest
            .methods
            .iter()
            .all(|method| !method.input_message.is_empty()));
        assert!(blob
            .manifest
            .methods
            .iter()
            .all(|method| !method.grpc_path.is_empty()));
        // Per-method typed services are populated.
        assert!(!blob.manifest.grpc.services.is_empty());
    }
}
