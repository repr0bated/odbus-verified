//! Plugin object blob support — re-exported from the canonical `op-blob`
//! crate (drop-in replacement for the first-pass implementation that lived
//! here).
//!
//! `op-blob` adds, beyond the original API surface:
//! - `PluginObjectBlob::seal()` / `from_sealed()` — deterministic zero-copy
//!   sealed byte image (`op_blob::sealed::BlobRef` for borrowed reads)
//! - `op_blob::BlobStore` — per-object shm persistence at
//!   `/dev/shm/opdbus/plugin-blobs/<id>.<hash16>.blob`
//! - `op_blob::identity` — WireGuard-keypair account identity with an
//!   account-persistent session, stamped into blobs via
//!   `blobify_plugin_schema_with_identity`
//! - `op_blob::btrfs` — seal a blob store into a mountable btrfs image

pub use op_blob::{
    blobify_plugin_schema, blobify_plugin_schema_with_identity, BlobMethod, DbusObjectIdentity,
    GrpcObjectIdentity, PluginObjectBlob,
};
