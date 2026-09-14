//! State management - declarative plugin system
#[cfg(any(feature = "mcp", feature = "web"))]
pub mod authority;
pub mod auto_plugin;
pub mod crypto;
pub mod dbus_plugin_base;
pub mod dbus_server;
pub mod plugin;
pub mod plugin_workflow;
pub mod plugins;
pub mod plugtree;
pub mod schema_validator;
