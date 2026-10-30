//! # op-blob — schema-driven plugin object blobs
//!
//! Implements the pipeline defined in
//! `.kiro/specs/schemars-to-reflection-plugin-pipeline/` and the
//! schema-coupled-plugin-blob-reflection whitepaper:
//!
//! ```text
//! Rust state/input/output structs (#[derive(schemars::JsonSchema)])
//!   -> schemars JSON Schema 2020-12
//!   -> adapter::plugin_schema_from_json
//!   -> PluginSchema                      <- SINGLE SOURCE OF TRUTH
//!   -> blob::blobify
//!   -> PluginObjectBlob                  <- sealed projection: schema + metadata
//!        schema_json + schema_hash
//!        D-Bus identity (/org/opdbus/v1/plugins/<name>)
//!        gRPC identity + FileDescriptorSet (typed, per-method)
//!        method manifest (subid, capability, side_effect, idempotent)
//!   -> catalog::ActiveReflectionCatalog  <- /dev/shm/opdbus/plugin-blobs/<id>.<hash>.blob
//!   -> btrfs::seal_blobs_into_btrfs_image <- btrfs filesystem in the blob artifact
//! ```
//!
//! Non-negotiable rules carried over from the whitepaper:
//! 1. `PluginSchema` is the source of truth. The blob is a frozen projection,
//!    never a second source.
//! 2. The plugin owns its schema; schemars is the derivation tool, not the
//!    published contract.
//! 3. Blobs are per-object artifacts (one plugin, one blob, one schema hash),
//!    replacing the monolithic `/dev/shm/live-schema.json` runtime shape.
//! 4. Reflection advertises only active blobs; descriptors are synthesized
//!    from `MethodDecl.args/returns` — no phantom services.
//! 5. Zero-copy: a sealed blob is a sectioned byte image; `BlobRef` hands out
//!    borrowed slices (schema JSON, manifest, descriptor bytes) without
//!    re-parsing or re-allocating on the read path.
//!
//! Divergences from the full workspace (documented, deliberate):
//! - The local `PluginSchema` is a reduced WORKING VIEW used only for
//!   descriptor/manifest synthesis. Real plugins are sealed through
//!   `blobify_plugin_schema`, which seals the FULL canonical
//!   `op_state_store::PluginSchema` JSON (signals, guarantees, category, …);
//!   read it back with `BlobRef::state_store_schema`.
//! - Plugin descriptor bytes are hand-encoded protobuf (`descriptor.rs`).
//!   Standard JSON well-known descriptors come from prost-reflect's built-ins.

pub mod adapter;
pub mod blob;
pub mod btrfs;
pub mod catalog;
pub mod demo;
pub mod descriptor;
pub mod identity;
pub mod json_schema_proto;
pub mod schema;
pub mod subid;

pub use adapter::{
    AckOutput, EmptyInput, apply_state_defaults, method_decl_from_schemars_with_output,
    plugin_schema_from_json, schema_diffs,
};
pub use blob::{
    BlobManifest, BlobRef, DbusIdentity, GrpcIdentity, MethodManifest, PluginObjectBlob, blobify,
    blobify_with_identity, canonical_plugin_id, try_blobify, try_blobify_with_identity,
};
pub use catalog::{ActiveReflectionCatalog, BlobStore};
pub use identity::{BlobIdentity, WgKeypair};
pub use schema::{Constraint, FieldSchema, FieldType, MethodDecl, PluginSchema, SideEffect};
pub use subid::validate_subid;

pub type BlobMethod = MethodManifest;
pub type DbusObjectIdentity = DbusIdentity;
pub type GrpcObjectIdentity = GrpcIdentity;

/// Convenience producer for trusted schemas. Panics on invalid schemas;
/// publication code must use `try_blobify_plugin_schema` to report errors.
pub fn blobify_plugin_schema(
    plugin_id: &str,
    schema: op_state_store::PluginSchema,
) -> PluginObjectBlob {
    blobify_plugin_schema_with_identity(plugin_id, schema, None)
}

/// Seal a full workspace schema without panicking on ambiguous field aliases.
pub fn try_blobify_plugin_schema(
    plugin_id: &str,
    schema: op_state_store::PluginSchema,
) -> Result<PluginObjectBlob, String> {
    try_blobify_plugin_schema_with_identity(plugin_id, schema, None)
}

/// Seal the FULL canonical workspace schema. The sealed SCHEMA_JSON section
/// (and the schema hash) is the complete `op_state_store::PluginSchema` —
/// signals, guarantees, category and all. The reduced local `PluginSchema` is
/// derived from the same canonical value purely as the working view for
/// descriptor/manifest synthesis; it is never what gets sealed.
///
/// Requires a trusted, validated schema; panics on invalid input. Publication
/// boundaries must use `try_blobify_plugin_schema_with_identity` instead.
pub fn blobify_plugin_schema_with_identity(
    plugin_id: &str,
    schema: op_state_store::PluginSchema,
    identity: Option<BlobIdentity>,
) -> PluginObjectBlob {
    try_blobify_plugin_schema_with_identity(plugin_id, schema, identity)
        .expect("trusted workspace PluginSchema is valid for blob synthesis")
}

/// Fallible full-schema producer. No catalog is opened or mutated here.
pub fn try_blobify_plugin_schema_with_identity(
    plugin_id: &str,
    mut schema: op_state_store::PluginSchema,
    identity: Option<BlobIdentity>,
) -> Result<PluginObjectBlob, String> {
    schema.name = blob::canonical_plugin_id(plugin_id);
    let canonical_value = blob::canonicalize_json_object_keys(
        serde_json::to_value(&schema)
            .map_err(|error| format!("plugin {plugin_id:?} schema serialization: {error}"))?,
    );
    // Explicitly sort object keys because serde_json's `preserve_order`
    // feature may be enabled elsewhere in the workspace.
    let canonical_json = serde_json::to_string(&canonical_value)
        .map_err(|error| format!("plugin {plugin_id:?} canonical serialization: {error}"))?;
    let working: PluginSchema = serde_json::from_value(canonical_value)
        .map_err(|error| format!("plugin {plugin_id:?} descriptor working view: {error}"))?;
    blob::try_blobify_canonical(&working, canonical_json, identity)
}
