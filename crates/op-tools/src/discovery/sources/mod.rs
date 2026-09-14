//! Discovery Sources
//!
//! Implementations of ToolDiscoverySource for various backends:
//! - D-Bus runtime introspection
//! - Plugin registry scanning
//! - Agent registry scanning

mod agent;
mod dbus;
mod plugin;

pub use agent::AgentDiscoverySource;
pub use dbus::DbusDiscoverySource;
pub use plugin::PluginDiscoverySource;
