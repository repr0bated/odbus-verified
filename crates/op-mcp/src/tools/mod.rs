//! Built-in Tools - All Loaded at Startup
//!
//! This module contains all tool implementations organized by category.
//! All tools are registered at startup and never evicted.

pub mod response;
pub mod filesystem;
pub mod shell;
pub mod system;
pub mod systemd;
pub mod ovs;
pub mod plugin;
pub mod qdrant;

use crate::tool_registry::{BoxedTool, ToolRegistry};
use anyhow::Result;
use std::sync::Arc;
use tracing::info;

/// Load ALL tools into registry at startup
/// 
/// This is called once when the server starts.
/// All tools remain loaded for the lifetime of the server.
pub async fn load_all_tools(registry: &ToolRegistry) -> Result<usize> {
    let mut count = 0;

    // Response tools (always needed)
    info!("Loading response tools...");
    count += response::register_all(registry).await?;

    // Filesystem tools
    info!("Loading filesystem tools...");
    count += filesystem::register_all(registry).await?;

    // Shell tools
    info!("Loading shell tools...");
    count += shell::register_all(registry).await?;

    // System tools
    info!("Loading system tools...");
    count += system::register_all(registry).await?;

    // Systemd tools (D-Bus)
    info!("Loading systemd tools...");
    count += systemd::register_all(registry).await?;

    // OVS tools
    info!("Loading OVS tools...");
    count += ovs::register_all(registry).await?;

    // Plugin state tools
    info!("Loading plugin state tools...");
    count += plugin::register_all(registry).await?;

    // Qdrant vector search tools
    info!("Loading Qdrant tools...");
    count += qdrant::register_all(registry).await?;

    info!("✅ Loaded {} tools total (no eviction)", count);
    Ok(count)
}
