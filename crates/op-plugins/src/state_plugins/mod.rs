//! State plugins - each manages a domain via native protocols.
//!
//! Each plugin implements the `StatePlugin` trait from `op-state` and
//! self-registers with the loader via `inventory::submit!` (see
//! `default_registry::PluginReg`) co-located with its own definition — there is
//! no central dispatch list.
//!
//! This module is just the module tree plus the public type re-exports. Adding a
//! plugin means adding its `pub mod` here and a `submit!` in that module; it does
//! NOT mean editing a catalog.

// ---------------------------------------------------------------------------
// Module tree (plugins + shared helpers). Keep alphabetical.
// ---------------------------------------------------------------------------
pub mod adc;
pub mod agent_config;
pub mod agent_triggers;
pub mod antigravity;
pub mod antigravity_chat;
pub mod btrfs_plugin;
pub mod cognitive_agents;
pub mod cognitive_mcp;
pub mod common;
pub mod config;
pub mod cozo;
pub mod cron;
pub mod ctl_plane_chatbot;
pub mod ctl_plane_chatbot_methods;
pub mod datastore;
pub mod decoy_network;
pub mod dnsresolver;
pub mod embedding_model;
pub mod emqx;
pub mod endpoint;
pub mod fail2ban;
pub mod fail2ban_methods;
pub mod freedesktop;
pub mod full_system;
pub mod gcloud_adc;
pub mod gemma_brain;
pub mod ghostbridge;
pub mod hardware;
pub mod human_principal;
pub mod identity_sled;
pub mod incus;
pub mod incus_device;
pub mod json_render;
pub mod keypair;
pub mod keyring;
pub mod large_language_model;
pub mod large_language_model_methods;
pub mod login1;
pub mod mail_server;
pub mod mcp;
pub mod memory_methods;
pub mod memory_plugin;
pub mod mongodb_mcp;
pub mod net;
pub mod notebooklm;
pub mod oci;
pub mod openflow;
pub mod openflow_obfuscation;
pub mod oscal_subid_registry;
pub mod oscal_subid_registry_methods;
pub mod ovsdb_bridge;
pub mod packagekit;
pub mod pcidecl;
pub mod persona;
pub(crate) mod plugin_scaffold_helpers;
pub mod privacy_routes;
pub mod procfs;
pub mod project;
pub mod proxy_server;
pub mod qdrant;
pub mod rovs_commands;
pub mod rtnetlink;
pub mod runtime_documents;
pub mod schema_contract;
pub mod schema_renderer;
pub mod schema_renderer_methods;
pub mod schemars_adapter;
pub mod service;
pub mod sessdecl;
pub mod shared_unix_socket;
pub mod shared_unix_socket_methods;
pub mod snowball_methods;
pub mod snowball_plugin;
pub mod software;
pub mod tched_router;
pub mod tched_router_config_surface;
pub mod tched_router_contract;
pub mod tched_router_legacy_methods;
pub mod unix_socket;
pub mod users;
pub mod web_ui;
pub mod web_ui_methods;
pub mod wg_opdbus;
pub mod wgcf;
pub mod wireguard;
pub mod workflows_plugin;
pub mod xray;
pub mod xray_config_types;

// ---------------------------------------------------------------------------
// Public type re-exports (consumed by `crate::prelude` and other crates).
// Plugins reached only through the loader/inventory do not need a re-export.
// Keep alphabetical by type.
// ---------------------------------------------------------------------------
pub use adc::AdcPlugin;
pub use agent_config::AgentConfigPlugin;
pub use antigravity::AntigravityPlugin;
pub use antigravity_chat::AntigravityChatPlugin;
pub use btrfs_plugin::BtrfsPlugin;
pub use cognitive_mcp::CognitiveMcpPlugin;
pub use config::ConfigPlugin;
pub use cozo::CozoPlugin;
pub use cron::CronPlugin;
pub use ctl_plane_chatbot::CtlPlaneChatbotPlugin;
pub use dnsresolver::DnsResolverPlugin;
pub use emqx::EmqxPlugin;
pub use endpoint::EndpointPlugin;
pub use fail2ban::Fail2banPlugin;
pub use freedesktop::FreeDesktopPlugin;
pub use full_system::FullSystemPlugin;
pub use gcloud_adc::GcloudAdcPlugin;
pub use hardware::HardwarePlugin;
pub use incus::IncusPlugin;
pub use json_render::JsonRenderPlugin;
pub use keypair::KeypairPlugin;
pub use keyring::KeyringPlugin;
pub use login1::Login1Plugin;
pub use mail_server::MailServerPlugin;
pub use mcp::McpStatePlugin;
pub use mcp::{ExecutionResult, ToolDefinition};
pub use memory_plugin::MemoryPlugin;
pub use mongodb_mcp::MongoDbMcpPlugin;
pub use net::NetStatePlugin;
pub use openflow::OpenFlowPlugin;
pub use openflow_obfuscation::OpenFlowObfuscationPlugin;
pub use ovsdb_bridge::OvsBridgePlugin;
pub use packagekit::PackageKitPlugin;
pub use pcidecl::PciDeclPlugin;
pub use procfs::ProcfsPlugin;
pub use proxy_server::ProxyServerPlugin;
pub use qdrant::QdrantPlugin;
pub use rovs_commands::RovsCommandsPlugin;
pub use rtnetlink::RtnetlinkPlugin;
pub use schema_renderer::SchemaRendererPlugin;
pub use service::ServicePlugin;
pub use sessdecl::SessDeclPlugin;
pub use shared_unix_socket::SharedUnixSocketPlugin;
pub use software::SoftwarePlugin;
pub use tched_router::TchedRouterPlugin;
pub use unix_socket::UnixSocketPlugin;
pub use users::UsersPlugin;
pub use web_ui::WebUiPlugin;
pub use wg_opdbus::WgOpdbusPlugin;
pub use wireguard::WireGuardPlugin;
pub use workflows_plugin::WorkflowsPlugin;
