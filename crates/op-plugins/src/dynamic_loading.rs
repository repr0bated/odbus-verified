//! Dynamic Loading Plugin - Manages tool loading and caching
//!
//! This plugin provides dynamic tool loading capabilities with intelligent caching
//! and execution-aware loading decisions.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::plugin::{Plugin, PluginCapabilities, PluginContext, PluginMetadata};
use crate::state::{ChangeOperation, DesiredState, StateChange, ValidationResult};

/// Dynamic Loading Plugin Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicLoadingConfig {
    /// Maximum cache size
    pub cache_size: usize,
    /// Critical tools that should always be loaded
    pub critical_tools: Vec<String>,
    /// Loading strategy (smart, aggressive, conservative)
    pub strategy: String,
}

/// Cache statistics for monitoring
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheStatistics {
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub load_time_ms: u64,
    pub evictions: u64,
    pub current_size: usize,
}

/// Dynamic Loading Plugin
pub struct DynamicLoadingPlugin {
    name: String,
    config: Arc<RwLock<DynamicLoadingConfig>>,
    state: Arc<RwLock<Value>>,
    desired: Arc<RwLock<DesiredState>>,
    stats: Arc<RwLock<CacheStatistics>>,
    storage_path: PathBuf,
    numa_node: Option<u32>,
}

impl DynamicLoadingPlugin {
    pub fn new() -> Self {
        Self {
            name: "dynamic_loading".to_string(),
            config: Arc::new(RwLock::new(DynamicLoadingConfig {
                cache_size: 100,
                critical_tools: vec![
                    "dbus_".to_string(),
                    "systemd_".to_string(),
                    "ovs_".to_string(),
                    "agent_".to_string(),
                ],
                strategy: "smart".to_string(),
            })),
            state: Arc::new(RwLock::new(json!({
                "cache_size": 100,
                "hit_rate": 0.0,
                "load_time_avg_ms": 0,
                "active_tools": 0,
                "storage_path": "/var/lib/op-dbus/plugins/dynamic_loading",
                "numa_node": 0
            }))),
            desired: Arc::new(RwLock::new(DesiredState::default())),
            stats: Arc::new(RwLock::new(CacheStatistics::default())),
            storage_path: PathBuf::from("/var/lib/op-dbus/plugins/dynamic_loading"),
            numa_node: None,
        }
    }

    /// Get current cache statistics
    pub async fn get_cache_stats(&self) -> Result<CacheStatistics> {
        Ok(self.stats.read().await.clone())
    }

    /// Update cache statistics
    pub async fn update_cache_stats(&self, hit: bool, load_time_ms: u64) -> Result<()> {
        let mut stats = self.stats.write().await;
        if hit {
            stats.cache_hits += 1;
        } else {
            stats.cache_misses += 1;
        }
        stats.load_time_ms += load_time_ms;
        stats.current_size = stats.current_size.min(self.config.read().await.cache_size);

        // Update state with current statistics
        let hit_rate = if stats.cache_hits + stats.cache_misses > 0 {
            stats.cache_hits as f64 / (stats.cache_hits + stats.cache_misses) as f64
        } else {
            0.0
        };

        let mut state = self.state.write().await;
        *state = json!({
            "cache_size": self.config.read().await.cache_size,
            "hit_rate": hit_rate,
            "load_time_avg_ms": stats.load_time_ms / (stats.cache_hits + stats.cache_misses).max(1),
            "active_tools": stats.current_size,
            "cache_hits": stats.cache_hits,
            "cache_misses": stats.cache_misses,
            "evictions": stats.evictions
        });

        Ok(())
    }

    /// Configure dynamic loading
    pub async fn configure(&self, config: DynamicLoadingConfig) -> Result<()> {
        *self.config.write().await = config;
        Ok(())
    }

    /// Get current configuration
    pub async fn get_config(&self) -> Result<DynamicLoadingConfig> {
        Ok(self.config.read().await.clone())
    }
    /// Ensure plugin storage directory exists
    async fn ensure_btrfs_subvolume(&self) -> Result<()> {
        // Check if the path is on a btrfs filesystem by reading mountinfo
        let path_str = self.storage_path.to_string_lossy();
        let mounts = tokio::fs::read_to_string("/proc/self/mountinfo")
            .await
            .unwrap_or_default();

        let is_btrfs = mounts.lines().any(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() > 8 {
                let fstype = parts.get(8).unwrap_or(&"");
                let mountpoint = parts.get(4).unwrap_or(&"");
                *fstype == "btrfs" && path_str.starts_with(mountpoint)
            } else {
                false
            }
        });

        if is_btrfs {
            tracing::info!("Storage path is on BTRFS: {}", self.storage_path.display());
        }

        // Ensure directory exists (regular directory ops regardless of filesystem)
        tokio::fs::create_dir_all(&self.storage_path).await?;

        Ok(())
    }

    /// Get BTRFS filesystem information via sysfs
    pub async fn get_btrfs_info(&self) -> Result<Value> {
        let path_str = self.storage_path.to_string_lossy();
        let mounts = tokio::fs::read_to_string("/proc/self/mountinfo")
            .await
            .unwrap_or_default();

        let mut info = json!({
            "subvolume": self.storage_path.display().to_string(),
            "info": "Not on a BTRFS filesystem"
        });

        for line in mounts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() > 8 && parts[8] == "btrfs" {
                let mountpoint = parts.get(4).unwrap_or(&"").to_string();
                if path_str.starts_with(&mountpoint) {
                    // Read btrfs sysfs info if available
                    let mut btrfs_sysfs = json!({
                        "mountpoint": mountpoint,
                        "device": parts.get(3).unwrap_or(&"").to_string(),
                        "filesystem": "btrfs"
                    });

                    // Try to read btrfs UUID from sysfs
                    if let Ok(entries) = tokio::fs::read_dir("/sys/fs/btrfs").await {
                        let mut uuids = Vec::new();
                        let mut dir = entries;
                        while let Ok(Some(entry)) = dir.next_entry().await {
                            let name = entry.file_name().to_string_lossy().to_string();
                            if !name.starts_with('.') {
                                uuids.push(name);
                            }
                        }
                        if !uuids.is_empty() {
                            btrfs_sysfs["uuids"] = json!(uuids);
                        }
                    }

                    info = btrfs_sysfs;
                    break;
                }
            }
        }

        Ok(info)
    }
}

#[async_trait]
impl Plugin for DynamicLoadingPlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "Dynamic tool loading with intelligent caching and execution tracking"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    async fn get_state(&self) -> Result<Value> {
        Ok(self.state.read().await.clone())
    }

    async fn get_desired_state(&self) -> Result<DesiredState> {
        Ok(self.desired.read().await.clone())
    }

    async fn set_desired_state(&self, desired: DesiredState) -> Result<()> {
        *self.desired.write().await = desired;
        Ok(())
    }

    async fn apply_state(&self) -> Result<Vec<StateChange>> {
        let desired = self.desired.read().await;
        let mut current = self.state.write().await;

        // Apply configuration changes
        if let Some(config) = desired.state.get("config") {
            let new_config: DynamicLoadingConfig =
                simd_json::serde::from_owned_value(config.clone())?;
            *self.config.write().await = new_config;
        }

        // Update state to match desired
        *current = desired.state.clone();

        Ok(vec![StateChange::new(
            ChangeOperation::Update,
            self.name.clone(),
            None,
            None,
            "Dynamic loading configuration applied",
        )])
    }

    async fn diff(&self) -> Result<Vec<StateChange>> {
        let desired = self.desired.read().await;
        let current = self.state.read().await;

        let mut changes = Vec::new();

        // Check if cache size needs adjustment
        if let (Some(desired_size), Some(current_size)) = (
            desired.state.get("cache_size").and_then(|v| v.as_u64()),
            current.get("cache_size").and_then(|v| v.as_u64()),
        ) {
            if desired_size != current_size {
                changes.push(StateChange::update(
                    self.name.clone(),
                    json!(current_size),
                    json!(desired_size),
                    format!("Cache size change: {} -> {}", current_size, desired_size),
                ));
            }
        }

        Ok(changes)
    }

    async fn validate(&self, config: &Value) -> Result<ValidationResult> {
        if let Some(cache_size) = config.get("cache_size") {
            if let Some(size) = cache_size.as_u64() {
                if !(10..=10000).contains(&size) {
                    return Ok(ValidationResult::failure(
                        "Cache size must be between 10 and 10000",
                    ));
                }
            }
        }
        Ok(ValidationResult::success())
    }

    async fn initialize(&mut self, context: PluginContext) -> Result<()> {
        // Store the plugin context
        self.storage_path = context.storage_path;
        self.numa_node = context.numa_node;

        // Update state with storage information
        let mut state = self.state.write().await;
        state["storage_path"] = simd_json::json!(self.storage_path.to_string_lossy());
        state["numa_node"] = simd_json::json!(self.numa_node);

        // Ensure BTRFS subvolume exists
        self.ensure_btrfs_subvolume().await?;

        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities::default()
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: self.name.clone(),
            version: self.version().to_string(),
            description: self.description().to_string(),
            author: Some("OP-DBUS Team".to_string()),
            license: Some("MIT".to_string()),
            dependencies: vec![
                "op-dynamic-loader".to_string(),
                "op-execution-tracker".to_string(),
            ],
            dbus_services: vec![],
            object_schemas: std::collections::HashMap::new(),
            feature_schemas: Vec::new(),
        }
    }

    #[allow(deprecated)]
    async fn handle_command(&self, command: &str, args: Value) -> Result<Value> {
        match command {
            "get_stats" => {
                let stats = self.get_cache_stats().await?;
                Ok(simd_json::serde::to_owned_value(stats)?)
            }
            "configure" => {
                let config: DynamicLoadingConfig = simd_json::serde::from_owned_value(args)?;
                self.configure(config).await?;
                Ok(json!({"status": "configured"}))
            }
            "get_config" => {
                let config = self.get_config().await?;
                Ok(simd_json::serde::to_owned_value(config)?)
            }
            "get_btrfs_info" => {
                let info = self.get_btrfs_info().await?;
                Ok(info)
            }
            "ensure_btrfs" => {
                self.ensure_btrfs_subvolume().await?;
                Ok(json!({"status": "btrfs_subvolume_ensured"}))
            }
            _ => Err(anyhow::anyhow!(
                "Command '{}' not supported by plugin '{}'",
                command,
                self.name()
            )),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn state_hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.name().as_bytes());
        hasher.update(self.version().as_bytes());
        hasher.update(self.state.blocking_read().to_string().as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

impl Default for DynamicLoadingPlugin {
    fn default() -> Self {
        Self::new()
    }
}
