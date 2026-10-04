//! D-Bus ↔ gRPC Bidirectional Bridge
//!
//! Provides live synchronization between D-Bus objects and gRPC services:
//! - D-Bus property changes → gRPC streaming updates
//! - gRPC mutations → D-Bus method calls / property sets
//! - D-Bus signals → gRPC server-streaming
//! - All changes flow through the event chain for audit/compliance
//!
//! Architecture:
//! ```text
//!                     ┌─────────────────┐
//!                     │   Event Chain   │ ← Source of truth
//!                     │  (audit + hash) │
//!                     └────────┬────────┘
//!                              │
//!               ┌──────────────┴──────────────┐
//!               ▼                              ▼
//!     ┌─────────────────┐            ┌─────────────────┐
//!     │     D-Bus       │◄──────────►│      gRPC       │
//!     │  (local IPC)    │            │  (remote RPC)   │
//!     └─────────────────┘            └─────────────────┘
//! ```

mod agent_runs;
mod agent_triggers;
mod antigravity_dispatch;
mod chain_vector_projector;
mod chat_response_dispatch;
mod chat_response_state;
mod chat_response_turns;
pub mod chat_service;
mod chatbot_episode_state;
mod chatbot_episode_stream;
pub mod context_bundle;
mod cron_dispatch;
mod cron_gateway;
mod cron_methods;
mod cron_observer;
mod ctl_plane_chatbot_dispatch;
mod datastore_methods;
pub mod dbus_identity;
pub mod decoy_network;
pub mod descriptor_mount;
pub mod descriptor_router;
pub mod dynamic_codec;
pub mod dynamic_reflection;
pub mod emqx_hook_provider;
mod event_chain_access;
mod freedesktop_dispatch;
mod gemma_brain_dispatch;
#[cfg(test)]
mod gemma_taxonomy_dispatch_tests;
mod generation_admission;
mod generation_jobs_dispatch;
mod generation_jobs_selection;
mod generation_jobs_stream;
mod generation_runtime;
pub mod grpc_client;
pub mod grpc_server;
pub mod grpc_web;
pub mod human_principal_dispatch;
mod identity_nextdns;
pub mod identity_sled_dispatch;
mod incus_observer;
pub mod interceptor;
mod json_render_gallery_dispatch;
mod json_schema_proto;
mod knowledge_delivery;
mod knowledge_inbox;
mod large_language_model_dispatch;
pub mod lungs_socket;
mod mcp_config_dispatch;
pub mod mcp_frontend;
pub mod mcp_policy;
mod memory_binding;
pub(crate) mod method_contract_admission;
pub(crate) mod model_providers_validation;
mod model_registry_dispatch;
mod mqtt_doorbell_relay;
pub mod mutation_engine;
pub(crate) mod native_sled_identity;
pub mod nlm_cli;
mod notebooklm_queries;
pub mod oracle_assertion;
pub mod oracle_assertion_client;
mod pam_auth;
mod request_registry;
mod responder_wake;
mod task_elevation;
mod wake_doorbell;
pub use oracle_assertion_client::DecoyAssertionClient;
mod agent_catalog_dispatch;
mod agent_workflow;
mod catalog_ordinary_profile;
pub(crate) mod chat_mode_routing;
pub(crate) mod control_plane_routing;
mod obfuscation_methods;
#[cfg(test)]
mod openflow_canonical_tests;
mod openflow_methods;
mod oscal_dispatch;
mod ovsdb_observer;
pub mod per_plugin_reflection;
pub mod plugin_grpc_gen;
pub mod plugin_object_blob;
pub mod project_dispatch;
pub mod proto_gen;
mod proxy_methods;
mod schema_integer_arguments;
pub mod schema_loader;
mod schema_renderer_dispatch;
pub mod schema_router;
mod sealed_schema_reader;
pub mod server;
pub mod shared_socket;
pub mod specialist;
mod state_stream;
mod state_tree_fold;
mod system_graph_dispatch;
mod system_graph_projection;
mod tched_catalog_binding;
pub mod tched_config_dispatch;
mod tched_config_engine;
mod tched_config_secret_policy;
mod tched_own_agent_route;
mod tched_router_admission;
mod tched_router_generation_native;
mod tched_router_legacy_dispatch;
pub mod tched_router_object_blob;
pub mod tched_router_runtime;
pub mod tracing;
mod ui_workspace_dispatch;
mod ui_workspace_stream;
mod ui_workspace_stream_selection;
mod web_ui_dispatch;
mod xray_methods;

// Re-export main types
pub use grpc_client::{
    GhostbridgeCallMetadata, GrpcClientPool, RemoteEndpoint, RemoteOperationClient,
};
pub use grpc_server::{OperationGrpcServer, PluginSchemaProvider, run_grpc_server};
pub use interceptor::ghostbridge_interceptor;
pub use mutation_engine::{ChangeSource, ChangeType, MutationEngine, StateChange};
pub use per_plugin_reflection::{
    MethodReflectionMeta, PerMethodReflectionConfig, PerMethodReflectionRegistry,
    generate_all_method_protos_for_plugin, generate_reflection_file_descriptor_set,
    to_method_service_name,
};
pub use plugin_grpc_gen::{
    MethodServiceLifecycleEvent, MethodServiceRegistry, PerMethodGrpcServices,
    generate_method_file_descriptor, generate_method_proto, generate_plugin_method_protos,
};
pub use proto_gen::{ProtoGenConfig, ProtoGenerator};
pub use server::{PemIdentity, ServerConfig, run_tched_router_server, serve_upgradeable_tcp};
// Object blob artifacts (schema-coupled D-Bus + gRPC reflection units),
// backed by the op-blob crate.
pub use plugin_object_blob::{BlobMethod, DbusObjectIdentity, PluginObjectBlob};
pub use tched_router_object_blob::TchedRouterObjectBlob;

/// Generated protobuf types — one sub-module per domain proto.
/// All are compiled into the combined operation_descriptor.bin for reflection.
pub mod proto {
    // Core: StateSync, PluginService, EventChainService, OvsdbMirror, RuntimeMirror
    tonic::include_proto!("operation.v1");

    pub mod mail {
        tonic::include_proto!("operation.mail.v1");
    }
    pub mod privacy {
        tonic::include_proto!("operation.privacy.v1");
    }
    pub mod registration {
        tonic::include_proto!("operation.registration.v1");
    }
    pub mod registry {
        tonic::include_proto!("operation.registry.v1");
    }

    /// EMQX ExHook v3 broker callback service (EMQX 5.9+).
    pub mod emqx_exhook {
        tonic::include_proto!("emqx.exhook.v3");
    }

    /// 3tched Router plugin schema gRPC service (GetSchema / WatchSchema).
    pub mod tched_router {
        tonic::include_proto!("tched_router");
    }

    /// ChatService — operator-to-system chat interface (delegator, forced tool calling).
    pub mod chat {
        tonic::include_proto!("op_chat.chat");
    }

    /// Build-time generated PluginSchema method services.
    pub mod plugin_methods {
        tonic::include_proto!("operation.plugin.v1");
    }

    /// Combined FileDescriptorSet covering all domain protos (including chat).
    /// Served by tonic-reflection so clients can discover every service.
    pub const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("operation_descriptor");
}

// Contract-pinned test names: the validation contract's Tool lines invoke
// these with `--exact <name>` at the crate root, so they must live here
// rather than inside a module (a module prefix would break the exact match).
// Each is a thin wrapper around the real implementation in
// `human_principal_dispatch::tests`.
#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn set_alias_cross_state_collision() {
    crate::human_principal_dispatch::tests::set_alias_cross_state_collision_impl().await;
}

#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn register_key_unwritable_cozo_fails_clean() {
    crate::human_principal_dispatch::tests::register_key_unwritable_cozo_fails_clean_impl().await;
}

#[cfg(test)]
#[tokio::test(flavor = "multi_thread")]
async fn registry_mutations_are_audit_recorded() {
    crate::human_principal_dispatch::tests::registry_mutations_are_audit_recorded_impl().await;
}

// Contract-pinned oracle_assertion test names at the crate root.
#[cfg(test)]
mod oracle_assertion_crate_tests {
    #[tokio::test(flavor = "multi_thread")]
    async fn rejection_variants_map_to_unauthenticated_tags() {
        crate::oracle_assertion::tests::rejection_variants_map_to_unauthenticated_tags_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn replay_cache_keyed_by_nonce_not_wire_bytes() {
        crate::oracle_assertion::tests::replay_cache_keyed_by_nonce_not_wire_bytes_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn nonce_not_consumed_when_principal_check_fails() {
        crate::oracle_assertion::tests::nonce_not_consumed_when_principal_check_fails_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn corrupted_store_rejects_unknown_decoy_key_at_validate() {
        crate::oracle_assertion::tests::corrupted_store_rejects_unknown_decoy_key_at_validate_impl(
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn validate_rejects_inverted_lifetime() {
        crate::oracle_assertion::tests::validate_rejects_inverted_lifetime_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn leeway_equality_edges_are_exact() {
        crate::oracle_assertion::tests::leeway_equality_edges_are_exact_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn inverted_lifetime_fires_at_parse_step() {
        crate::oracle_assertion::tests::inverted_lifetime_fires_at_parse_step_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_trust_store_rejects_all() {
        crate::oracle_assertion::tests::empty_trust_store_rejects_all_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn replay_purge_edge_equals_acceptance_edge() {
        crate::oracle_assertion::tests::replay_purge_edge_equals_acceptance_edge_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn cross_principal_assertion_ip_swap_matrix() {
        crate::oracle_assertion::tests::cross_principal_assertion_ip_swap_matrix_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn replay_cache_keyed_globally_by_nonce() {
        crate::oracle_assertion::tests::replay_cache_keyed_globally_by_nonce_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn trust_store_rotation_is_load_once() {
        crate::oracle_assertion::tests::trust_store_rotation_is_load_once_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn validator_state_is_per_serving_instance() {
        crate::oracle_assertion::tests::validator_state_is_per_serving_instance_impl().await;
    }
}

// Contract-pinned interceptor + gate wiring tests at the crate root.
#[cfg(test)]
mod interceptor_crate_tests {
    #[tokio::test(flavor = "multi_thread")]
    async fn assertion_present_valid_inserts_human_principal_identity() {
        crate::interceptor::tests::assertion_present_valid_inserts_human_principal_identity_impl()
            .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn assertion_present_invalid_returns_unauthenticated() {
        crate::interceptor::tests::assertion_present_invalid_returns_unauthenticated_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn assertion_present_legacy_headers_not_consulted() {
        crate::interceptor::tests::assertion_present_legacy_headers_not_consulted_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn assertion_absent_rejects_legacy_headers() {
        crate::interceptor::tests::assertion_absent_rejects_legacy_headers_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn duplicate_assertion_metadata_values_reject_malformed() {
        crate::interceptor::tests::duplicate_assertion_metadata_values_reject_malformed_impl()
            .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn assertion_without_connect_info_rejects_missing_connect_info() {
        crate::interceptor::tests::assertion_without_connect_info_rejects_missing_connect_info_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sid1_parked_inserts_human_principal_identity() {
        crate::interceptor::tests::sid1_parked_inserts_human_principal_identity_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sid1_and_oia1_together_rejected() {
        crate::interceptor::tests::sid1_and_oia1_together_rejected_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn sid1_requires_current_principal_registration() {
        crate::interceptor::tests::sid1_requires_current_principal_registration_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn human_principal_grant_allows_capability_gate() {
        crate::interceptor::tests::human_principal_grant_allows_capability_gate_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn human_principal_missing_grant_denies_capability_gate() {
        crate::interceptor::tests::human_principal_missing_grant_denies_capability_gate_impl()
            .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn human_identity_shadows_ghostbridge_for_capability_gate() {
        crate::interceptor::tests::human_identity_shadows_ghostbridge_for_capability_gate_impl()
            .await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn registration_interceptor_factory_respects_bootstrap() {
        crate::interceptor::tests::registration_interceptor_factory_respects_bootstrap_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn validator_per_instance_interceptor_isolation() {
        crate::interceptor::tests::validator_per_instance_interceptor_isolation_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn bridge_capability_identity_prefers_human() {
        crate::interceptor::tests::bridge_capability_identity_prefers_human_impl().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn assertion_bad_signature_does_not_insert_ghostbridge() {
        crate::interceptor::tests::assertion_bad_signature_does_not_insert_ghostbridge_impl().await;
    }
}
