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
//! capture, generated per-plugin protos, chat-router legacy,
//! and the ~140 dispatch modules. Old files remain on disk unreferenced;
//! remove them with `git rm` when convenient — they no longer compile
//! into this crate (see `autobins`/`autotests` in Cargo.toml).

pub mod admission;
pub mod catalog;
pub(crate) mod chat_link;
#[path = "chat_response_turns.rs"]
pub(crate) mod chat_response_turns;
#[path = "chat_router_agent_route.rs"]
pub(crate) mod chat_router_agent_route;
#[path = "chat_router_catalog_binding.rs"]
pub(crate) mod chat_router_catalog_binding;
#[path = "chat_router_config_dispatch.rs"]
pub(crate) mod chat_router_config_dispatch;
#[path = "chat_router_config_secret_policy.rs"]
pub(crate) mod chat_router_config_secret_policy;
#[path = "chat_router_generation.rs"]
pub(crate) mod chat_router_generation;
#[path = "chat_router_runtime.rs"]
pub(crate) mod chat_router_runtime;
pub mod cockpit;
pub mod core;
#[path = "cron_gateway.rs"]
pub(crate) mod cron_gateway;
#[path = "cron_methods.rs"]
pub(crate) mod cron_methods;
#[path = "decoy_network.rs"]
pub(crate) mod decoy_network;
pub mod grant_document;
pub mod grpc_client;
pub mod human_principal_registry;
pub mod identity_binding;
#[path = "identity_nextdns.rs"]
pub(crate) mod identity_nextdns;
pub mod identity_receiver;
#[path = "method_contract_admission.rs"]
pub(crate) mod method_contract_admission;
#[path = "model_providers_validation.rs"]
pub(crate) mod model_providers_validation;
#[path = "model_registry_dispatch.rs"]
pub(crate) mod model_registry_dispatch;
pub mod mqtt_doorbell_relay;
#[path = "nlm_cli.rs"]
pub(crate) mod nlm_cli;
#[path = "notebooklm_queries.rs"]
pub(crate) mod notebooklm_queries;
#[path = "obfuscation_methods.rs"]
pub(crate) mod obfuscation_methods;
#[path = "openflow_methods.rs"]
pub(crate) mod openflow_methods;
#[path = "openflow_permit.rs"]
pub(crate) mod openflow_permit;
pub mod oracle_assertion;
#[allow(dead_code)] // Receiver is not mounted until all identity paths are wired.
pub(crate) mod oracle_receiver;
#[cfg(test)]
mod oracle_test_support;
#[path = "pam_auth.rs"]
pub(crate) mod pam_auth;
#[path = "proxy_methods.rs"]
pub(crate) mod proxy_methods;
#[path = "request_registry.rs"]
pub(crate) mod request_registry;
pub mod router;
#[path = "runtime.rs"]
pub(crate) mod runtime;
#[path = "runtime_bootstrap.rs"]
pub(crate) mod runtime_bootstrap;
#[path = "runtime_chat.rs"]
pub(crate) mod runtime_chat;
#[path = "runtime_cognitive.rs"]
pub(crate) mod runtime_cognitive;
#[path = "runtime_creation.rs"]
pub(crate) mod runtime_creation;
#[path = "runtime_identity.rs"]
pub(crate) mod runtime_identity;
#[path = "runtime_identity_owner.rs"]
pub(crate) mod runtime_identity_owner;
#[path = "runtime_lungs.rs"]
pub(crate) mod runtime_lungs;
#[path = "runtime_misc.rs"]
pub(crate) mod runtime_misc;
#[path = "runtime_mqtt.rs"]
pub(crate) mod runtime_mqtt;
#[path = "runtime_network.rs"]
pub(crate) mod runtime_network;
#[path = "runtime_platform.rs"]
pub(crate) mod runtime_platform;
#[path = "runtime_workspace.rs"]
pub(crate) mod runtime_workspace;
#[path = "sealed_schema_reader.rs"]
pub(crate) mod sealed_schema_reader;
pub mod server;
pub mod service;
pub mod sid1_checks;
#[allow(dead_code)] // The reader is not mounted until request admission is wired.
pub(crate) mod state_reader;
#[path = "task_elevation.rs"]
pub(crate) mod task_elevation;
pub mod tls_door;
#[path = "xray_methods.rs"]
pub(crate) mod xray_methods;

pub use runtime_bootstrap::run_from_env;

/// Generated protobuf types for `package operation.v1`.
pub mod proto {
    tonic::include_proto!("operation.v1");

    /// Combined descriptor set for gRPC reflection.
    pub const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("operation_descriptor");
}

pub use catalog::{Catalog, CatalogError, PluginEntry, PluginMethod};
pub use core::{BridgeCore, Journal, StateStore};
pub use grpc_client::{
    GhostbridgeCallMetadata, GrpcClientPool, RemoteEndpoint, RemoteOperationClient,
};
pub use router::{Handler, Router, RouterError};
pub use server::serve_with_listener;
pub use tls_door::{PemIdentity, serve_upgradeable_tcp};
