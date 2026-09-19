#![recursion_limit = "512"]
#![deny(rustdoc::broken_intra_doc_links)]
#![allow(
    dead_code,
    unused_imports,
    unused_variables,
    deprecated,
    clippy::derivable_impls,
    clippy::collapsible_if,
    clippy::large_enum_variant,
    clippy::let_and_return,
    clippy::new_without_default,
    clippy::vec_init_then_push,
    clippy::needless_update,
    clippy::needless_borrows_for_generic_args,
    clippy::op_ref,
    clippy::empty_line_after_outer_attr
)]

//! op-plugins: Plugin system with state management and snowball footprints
//!
//! Features:
//! - Plugin trait with desired state management
//! - State plugins for network, LXC, systemd, OpenFlow, etc.
//! - BTRFS subvolume storage per plugin
//! - Automatic hash footprints for snowball audit trail
//! - Auto-creation of missing plugins
//! - Lifecycle hooks
//! - Canonical plugin-document persistence into the schema catalog

pub mod auto_create;
pub mod builtin;
pub mod canonical;
pub mod chat;
pub mod dynamic_loading;
pub mod plugin;
pub mod registry;
pub mod service_def;
pub mod state;

// State plugins - each manages a specific domain
pub mod default_registry;
pub mod state_plugins;

pub use auto_create::AutoPlugin;
pub use canonical as plugin_paths;
pub use default_registry::{DefaultPluginRegistry, PluginRegistryConfig};
pub use plugin::{Plugin, PluginCapabilities, PluginContext, PluginMetadata};
pub use registry::PluginRegistry as PluginCatalog;
pub use registry::{PluginRecord, PluginRegistry};
pub use state::{ChangeOperation, DesiredState, StateChange, ValidationResult};

// ── Canonical PluginSchema authority surface ───────────────────────────────
// "The plugin is the schema." The schema *type* physically lives in the lowest
// crate (op-state-store, zero workspace deps) to avoid dependency cycles, but
// op-plugins is the single authoritative *import surface*: every consumer
// (cognitive-mcp, projection, web, gRPC) imports schema types from `op_plugins`.
// op-state-store is retired as a public schema surface — code that still (or
// mistakenly) reaches for the old path can resolve through this alias instead.
pub use op_state_store::plugin_schema::{
    PluginSchemaBuilder, ValidationResult as SchemaValidationResult,
};
pub use op_state_store::{Constraint, FieldSchema, FieldType, PluginSchema, ReadOnlyCondition};

/// Canonical input/contract schema for the cognitive-mcp gateway, including the
/// `code_search` / `code_context` / `code_index` tool inputs (each carrying its
/// OSCAL subid). This is the authoritative accessor; tools derive their
/// `input_schema()` from this via `PluginSchema::field_input_schema(...)`.
pub fn cognitive_mcp_plugin_schema() -> PluginSchema {
    state_plugins::cognitive_mcp::cognitive_mcp_schema()
}

/// Deterministically materialize the initial D-Bus projection state from a
/// plugin schema. This is the bootstrap counterpart to the MutationEngine's
/// normal projection writes: it is used only when no live projection exists.
pub fn projection_seed_state_from_schema(schema: &PluginSchema) -> simd_json::OwnedValue {
    state_plugins::plugin_scaffold_helpers::materialize_state_from_schema(schema)
}

// Re-export chat types
pub use chat::{
    ChatMessage, ChatRequest, ChatResponse, ChatRole, ExecutionStatus, TokenUsage, ToolCall,
};

/// Prelude for convenient imports
pub mod prelude {
    pub use super::auto_create::AutoPlugin;
    pub use super::registry::PluginRegistry as PluginCatalog;
    pub use super::registry::PluginRegistry;
    pub use super::state::{ChangeOperation, DesiredState, StateChange, ValidationResult};

    // Re-export state plugins
    pub use super::dynamic_loading::DynamicLoadingPlugin;
    pub use super::state_plugins::{
        AdcPlugin, AgentConfigPlugin, AntigravityChatPlugin, AntigravityPlugin, BtrfsPlugin,
        CognitiveMcpPlugin, ConfigPlugin, CronPlugin, CtlPlaneChatbotPlugin, DnsResolverPlugin,
        EndpointPlugin, ExecutionResult, Fail2banPlugin, FreeDesktopPlugin, FullSystemPlugin,
        GcloudAdcPlugin, HardwarePlugin, IncusPlugin, KeypairPlugin, KeyringPlugin, Login1Plugin,
        MailServerPlugin, McpStatePlugin, MemoryPlugin, NetStatePlugin, OpenFlowObfuscationPlugin,
        OpenFlowPlugin, OvsBridgePlugin, PackageKitPlugin, PciDeclPlugin, ProcfsPlugin,
        ProxyServerPlugin, RovsCommandsPlugin, RtnetlinkPlugin, SchemaRendererPlugin,
        ServicePlugin, SessDeclPlugin, SoftwarePlugin, TchedRouterPlugin, ToolDefinition,
        UnixSocketPlugin, UsersPlugin, WebUiPlugin, WgOpdbusPlugin, WireGuardPlugin,
        WorkflowsPlugin,
    };
}
pub mod state_publisher;
