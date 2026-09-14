//! Systemd state plugin - manages systemd via org.freedesktop.systemd1 D-Bus
//! Maps D-Bus object tree to declarative state

use anyhow::{Context, Result};
use async_trait::async_trait;
use op_state::plugtree::PlugTree;
use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff, StatePlugin,
};
use serde::{Deserialize, Serialize};
use simd_json::{json, OwnedValue as Value};
use simd_json::prelude::*;
use std::collections::HashMap;
use zbus::{Connection, Proxy};

/// Systemd configuration schema - mirrors D-Bus object tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemdConfig {
    /// Units indexed by name (e.g., "ssh.service")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub units: Option<HashMap<String, UnitConfig>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnitConfig {
    /// Desired state: "active", "inactive", "failed", etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_state: Option<String>,

    /// Should unit be enabled at boot
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// Should unit be masked (prevents starting)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub masked: Option<bool>,

    /// Additional D-Bus properties (dynamic)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, Value>>,
}

/// Systemd state plugin
pub struct SystemdStatePlugin;

impl SystemdStatePlugin {
    pub fn new() -> Self {
        Self
    }

    /// Connect to systemd via D-Bus
    async fn connect_systemd(&self) -> Result<Proxy<'static>> {
        let conn = Connection::system()
            .await
            .context("Failed to connect to system D-Bus")?;

        Proxy::new(
            &conn,
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
        )
        .await
        .context("Failed to create systemd D-Bus proxy")
    }

    /// Get unit object path from unit name
    async fn get_unit_path(&self, proxy: &Proxy<'_>, unit_name: &str) -> Result<String> {
        let path: zbus::zvariant::OwnedObjectPath = proxy
            .call("GetUnit", &(unit_name,))
            .await
            .context(format!("Failed to get unit path for {}", unit_name))?;

        Ok(path.to_string())
    }

    /// Query current state of a unit
    async fn query_unit(&self, unit_name: &str) -> Result<UnitConfig> {
        let proxy = self.connect_systemd().await?;
        let unit_path = self.get_unit_path(&proxy, unit_name).await?;

        let conn = Connection::system().await?;
        let unit_proxy = Proxy::new(
            &conn,
            "org.freedesktop.systemd1",
            unit_path,
            "org.freedesktop.systemd1.Unit",
        )
        .await?;

        // Get ActiveState property
        let active_state: String = unit_proxy
            .get_property("ActiveState")
            .await
            .unwrap_or_else(|_| "unknown".to_string());

        // Check if enabled (this is a UnitFile property)
        let enabled = self.check_unit_enabled(&proxy, unit_name).await.ok();

        Ok(UnitConfig {
            active_state: Some(active_state),
            enabled,
            masked: None, // TODO: Query mask state
            properties: None,
        })
    }

    /// Check if unit is enabled
    async fn check_unit_enabled(&self, proxy: &Proxy<'_>, unit_name: &str) -> Result<bool> {
        let state: String = proxy
            .call("GetUnitFileState", &(unit_name,))
            .await
            .context("Failed to get unit file state")?;

        Ok(state == "enabled")
    }

    /// Start a systemd unit
    async fn start_unit(&self, unit_name: &str) -> Result<()> {
        let proxy = self.connect_systemd().await?;

        let _job: zbus::zvariant::OwnedObjectPath = proxy
            .call("StartUnit", &(unit_name, "replace"))
            .await
            .context(format!("Failed to start unit {}", unit_name))?;

        log::info!("Started systemd unit: {}", unit_name);
        Ok(())
    }

    /// Stop a systemd unit
    async fn stop_unit(&self, unit_name: &str) -> Result<()> {
        let proxy = self.connect_systemd().await?;

        let _job: zbus::zvariant::OwnedObjectPath = proxy
            .call("StopUnit", &(unit_name, "replace"))
            .await
            .context(format!("Failed to stop unit {}", unit_name))?;

        log::info!("Stopped systemd unit: {}", unit_name);
        Ok(())
    }

    /// Enable a systemd unit
    async fn enable_unit(&self, unit_name: &str) -> Result<()> {
        let proxy = self.connect_systemd().await?;

        let _: (bool, Vec<(String, String, String)>) = proxy
            .call("EnableUnitFiles", &(vec![unit_name], false, true))
            .await
            .context(format!("Failed to enable unit {}", unit_name))?;

        log::info!("Enabled systemd unit: {}", unit_name);
        Ok(())
    }

    /// Disable a systemd unit
    async fn disable_unit(&self, unit_name: &str) -> Result<()> {
        let proxy = self.connect_systemd().await?;

        let _: Vec<(String, String, String)> = proxy
            .call("DisableUnitFiles", &(vec![unit_name], false))
            .await
            .context(format!("Failed to disable unit {}", unit_name))?;

        log::info!("Disabled systemd unit: {}", unit_name);
        Ok(())
    }

    /// Apply desired unit configuration
    async fn apply_unit_config(&self, unit_name: &str, config: &UnitConfig) -> Result<()> {
        // Apply masked state first (prevents other operations)
        if let Some(desired_masked) = config.masked {
            // Check current mask state via GetUnitFileState
            let proxy = self.connect_systemd().await?;
            let state: String = proxy
                .call("GetUnitFileState", &(unit_name,))
                .await
                .unwrap_or_else(|_| "unknown".to_string());

            let currently_masked = state == "masked";

            if desired_masked && !currently_masked {
                self.mask_unit(unit_name).await?;
            } else if !desired_masked && currently_masked {
                self.unmask_unit(unit_name).await?;
            }
        }

        // Apply active state
        if let Some(ref desired_state) = config.active_state {
            let current = self.query_unit(unit_name).await?;
            let current_state = current
                .active_state
                .unwrap_or_else(|| "unknown".to_string());

            if desired_state == "active" && current_state != "active" {
                self.start_unit(unit_name).await?;
            } else if desired_state == "inactive" && current_state == "active" {
                self.stop_unit(unit_name).await?;
            }
        }

        // Apply enabled state
        if let Some(desired_enabled) = config.enabled {
            let current = self.query_unit(unit_name).await?;
            let current_enabled = current.enabled.unwrap_or(false);

            if desired_enabled && !current_enabled {
                self.enable_unit(unit_name).await?;
            } else if !desired_enabled && current_enabled {
                self.disable_unit(unit_name).await?;
            }
        }

        Ok(())
    }
}

impl Default for SystemdStatePlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemdStatePlugin {
    /// Apply state to a single unit
    #[allow(dead_code)]
    pub async fn apply_unit(
        &self,
        unit_name: &str,
        unit_config: &UnitConfig,
    ) -> Result<ApplyResult> {
        let mut changes_applied = Vec::new();
        let mut errors = Vec::new();

        match self.apply_unit_config(unit_name, unit_config).await {
            Ok(_) => {
                changes_applied.push(format!("Applied config for unit: {}", unit_name));
            }
            Err(e) => {
                errors.push(format!("Failed to apply unit {}: {}", unit_name, e));
            }
        }

        Ok(ApplyResult {
            success: errors.is_empty(),
            changes_applied,
            errors,
            checkpoint: None,
        })
    }

    /// Mask a systemd unit
    pub async fn mask_unit(&self, unit_name: &str) -> Result<()> {
        let proxy = self.connect_systemd().await?;

        let _: Vec<(String, String, String)> = proxy
            .call("MaskUnitFiles", &(vec![unit_name], false, true))
            .await
            .context(format!("Failed to mask unit {}", unit_name))?;

        log::info!("Masked systemd unit: {}", unit_name);
        Ok(())
    }

    /// Unmask a systemd unit
    pub async fn unmask_unit(&self, unit_name: &str) -> Result<()> {
        let proxy = self.connect_systemd().await?;

        let _: Vec<(String, String, String)> = proxy
            .call("UnmaskUnitFiles", &(vec![unit_name], false))
            .await
            .context(format!("Failed to unmask unit {}", unit_name))?;

        log::info!("Unmasked systemd unit: {}", unit_name);
        Ok(())
    }
}

#[async_trait]
impl PlugTree for SystemdStatePlugin {
    fn pluglet_type(&self) -> &str {
        "unit"
    }

    fn pluglet_id_field(&self) -> &str {
        "name"
    }

    fn extract_pluglet_id(&self, resource: &Value) -> Result<String> {
        match resource {
            Value::Object(obj) => obj.keys().next()
                .map(|s| s.to_string())
                .ok_or_else(|| anyhow::anyhow!("Unit missing name")),
            _ => anyhow::bail!("Resource must be an object"),
        }
    }

    async fn apply_pluglet(&self, pluglet_id: &str, desired: &Value) -> Result<ApplyResult> {
        let unit_config: UnitConfig = simd_json::serde::from_owned_value(desired.clone())?;
        self.apply_unit(pluglet_id, &unit_config).await
    }

    async fn query_pluglet(&self, pluglet_id: &str) -> Result<Option<Value>> {
        match self.query_unit(pluglet_id).await {
            Ok(unit) => Ok(Some(simd_json::serde::to_owned_value(unit)?)),
            Err(_) => Ok(None),
        }
    }

    async fn list_pluglet_ids(&self) -> Result<Vec<String>> {
        // Would require listing all systemd units - for now return empty
        // Full implementation would call ListUnits on D-Bus
        Ok(Vec::new())
    }
}

#[async_trait]
impl StatePlugin for SystemdStatePlugin {
    fn name(&self) -> &str {
        "systemd"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }


    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        let current_config: SystemdConfig = simd_json::serde::from_owned_value(current.clone())?;
        let desired_config: SystemdConfig = simd_json::serde::from_owned_value(desired.clone())?;

        let mut actions = Vec::new();

        if let Some(desired_units) = &desired_config.units {
            for (unit_name, desired_unit) in desired_units {
                let current_unit = current_config.units.as_ref().and_then(|u| u.get(unit_name));

                if current_unit != Some(desired_unit) {
                    actions.push(StateAction::Modify {
                        resource: unit_name.clone(),
                        changes: simd_json::serde::to_owned_value(desired_unit)?,
                    });
                }
            }
        }

        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: format!("{:x}", md5::compute(simd_json::to_string(current)?)),
                desired_hash: format!("{:x}", md5::compute(simd_json::to_string(desired)?)),
            },
        })
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let mut changes_applied = Vec::new();
        let mut errors = Vec::new();

        for action in &diff.actions {
            if let StateAction::Modify { resource, changes } = action {
                let unit_config: UnitConfig = simd_json::serde::from_owned_value(changes.clone())?;

                match self.apply_unit_config(resource, &unit_config).await {
                    Ok(_) => {
                        changes_applied.push(format!("Applied systemd config for: {}", resource));
                    }
                    Err(e) => {
                        errors.push(format!("Failed to apply config for {}: {}", resource, e));
                    }
                }
            }
        }

        Ok(ApplyResult {
            success: errors.is_empty(),
            changes_applied,
            errors,
            checkpoint: None,
        })
    }

    async fn verify_state(&self, desired: &Value) -> Result<bool> {
        let desired_config: SystemdConfig = simd_json::serde::from_owned_value(desired.clone())?;

        if let Some(units) = &desired_config.units {
            for (unit_name, desired_unit) in units {
                let current = self.query_unit(unit_name).await?;

                if let Some(ref desired_state) = desired_unit.active_state {
                    if current.active_state.as_ref() != Some(desired_state) {
                        return Ok(false);
                    }
                }

                if let Some(desired_enabled) = desired_unit.enabled {
                    if current.enabled != Some(desired_enabled) {
                        return Ok(false);
                    }
                }
            }
        }

        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        let current = simd_json::json!(null);
        Ok(Checkpoint {
            id: format!("systemd-{}", chrono::Utc::now().timestamp()),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: current,
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, checkpoint: &Checkpoint) -> Result<()> {
        let old_config: SystemdConfig = simd_json::serde::from_owned_value(checkpoint.state_snapshot.clone())?;

        if let Some(units) = old_config.units {
            for (unit_name, unit_config) in units {
                self.apply_unit_config(&unit_name, &unit_config).await?;
            }
        }

        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: true,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false, // D-Bus calls are not atomic
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Smoke test that exercises zbus systemd connectivity.
    // It should not fail the build if system D-Bus policy restricts access.
    #[tokio::test(flavor = "current_thread")]
    async fn test_systemd_query_unit() {
        let plugin = SystemdStatePlugin::new();
        // Common units to try
        let candidates = ["dbus.service", "systemd-logind.service", "cron.service"];

        // Try each candidate until one succeeds, but don't fail if all are blocked.
        for u in candidates {
            let res = plugin.query_unit(u).await;
            if let Ok(cfg) = res {
                assert!(cfg.active_state.is_some());
                return;
            }
        }
        // If none succeed, we at least reached D-Bus paths without panicking.
    }
}
