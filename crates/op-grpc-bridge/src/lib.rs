//! op-grpc-bridge — fresh, catalog-driven D-Bus <-> gRPC bridge.
//!
//! Design (from the wire contract in `proto/operation.proto` plus the
//! plugin-catalog shape used by the SHM snapshots):
//!
//! - The **catalog** is the route source of truth: one entry per plugin
//!   (`id`, `schema_hash`, declared `methods`, optional JSON `schema`).
//! - The **catalog hash** stamped on every state frame is blake3 over the
//!   sorted `(plugin_id, schema_hash)` pairs, exactly as `StateChange`
//!   documents, so frames are self-describing without polling.
//! - The **router** maps `(plugin_id, method)` to a synchronous handler.
//!   Handlers are pure value-in/value-out; async or D-Bus-backed behavior
//!   plugs in behind `BridgeCore` call sites, not inside the router.
//! - The **core** owns the state store and an append-only hash-chained
//!   journal. Every accepted mutation appends one journal event and
//!   publishes one `StateChange` frame. `VerifyChain` recomputes the
//!   chain, so audit needs no external store.
//! - The **service** layer is a thin tonic translation of the three
//!   proto services onto the core. `EventChainService` proof/snapshot
//!   RPCs are honestly `UNIMPLEMENTED`; everything else is real.
//!
//! - OvsdbMirror, RuntimeMirror, ComponentRegistry, MailService, PrivacyNetworkService, DbusPassthrough — dropped, surface already covered by plugin state/catalog.
//! - HookProvider — dropped from bridge, hooks move to the EMQX plugin (user-defined category field, separate work).
//! - RegistrationService — dropped, super old, fresh start later.
//!
//! Deliberately gone (the old pollution): build-time live-host schema
//! capture, generated per-plugin protos, chat-router/tched-router legacy,
//! and the ~140 dispatch modules. Old files remain on disk unreferenced;
//! remove them with `git rm` when convenient — they no longer compile
//! into this crate (see `autobins`/`autotests` in Cargo.toml).

pub mod catalog;
pub mod core;
pub mod router;
pub mod server;
pub mod service;

/// Generated protobuf types for `package operation.v1`.
pub mod proto {
    tonic::include_proto!("operation.v1");

    /// Combined descriptor set for gRPC reflection.
    pub const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("operation_descriptor");
}

pub use catalog::{Catalog, CatalogError, PluginEntry, PluginMethod};
pub use core::{BridgeCore, Journal, StateStore};
pub use router::{Handler, Router, RouterError};
pub use server::serve;
