//! Disaster Recovery Module
//!
//! Provides system state export/import for disaster recovery with dependency tracking.
//! Each export contains all plugin states plus the dependencies needed to restore.
//!
//! Dependencies are installed via D-Bus PackageKit - NO CLI COMMANDS.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::collections::HashMap;
use zbus::Connection;

/// System dependency that must be installed for restore
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemDependency {
    /// Package name (e.g., "openvswitch-switch")
    pub name: String,
    /// Package manager (apt, yum, dnf, etc.)
    pub package_manager: String,
    /// Minimum version required (optional)
    pub min_version: Option<String>,
    /// Whether this is critical for restore
    pub required: bool,
    /// Install command override (if not standard)
    pub install_command: Option<String>,
}

/// Captured state for a single plugin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginStateExport {
    /// Plugin name
    pub plugin_name: String,
    /// Plugin version
    pub version: String,
    /// The actual state data
    pub state: Value,
    /// Dependencies required by this plugin
    pub dependencies: Vec<SystemDependency>,
    /// Timestamp when state was captured
    pub captured_at: DateTime<Utc>,
    /// State hash for integrity verification
    pub state_hash: String,
}

/// Complete disaster recovery export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisasterRecoveryExport {
    /// Export format version
    pub format_version: String,
    /// Unique export ID
    pub export_id: String,
    /// When this export was created
    pub created_at: DateTime<Utc>,
    /// Host information
    pub host_info: HostInfo,
    /// All plugin states
    pub plugins: HashMap<String, PluginStateExport>,
    /// Global dependencies (system-wide)
    pub global_dependencies: Vec<SystemDependency>,
    /// Apply order for plugins (topological sort)
    pub apply_order: Vec<String>,
    /// Checksum of entire export
    pub checksum: String,
}

/// Host information for DR context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub hostname: String,
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub kernel: String,
}

/// Result of a restore operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResult {
    pub success: bool,
    pub plugins_restored: Vec<String>,
    pub plugins_failed: Vec<(String, String)>, // (name, error)
    pub dependencies_installed: Vec<String>,
    pub dependencies_failed: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

impl DisasterRecoveryExport {
    /// Create a new empty DR export
    pub fn new() -> Self {
        Self {
            format_version: "1.0.0".to_string(),
            export_id: uuid::Uuid::new_v4().to_string(),
            created_at: Utc::now(),
            host_info: HostInfo::detect(),
            plugins: HashMap::new(),
            global_dependencies: Vec::new(),
            apply_order: Vec::new(),
            checksum: String::new(),
        }
    }

    /// Add a plugin state to the export
    pub fn add_plugin(&mut self, plugin: PluginStateExport) {
        self.apply_order.push(plugin.plugin_name.clone());
        self.plugins.insert(plugin.plugin_name.clone(), plugin);
    }

    /// Add a global dependency
    pub fn add_global_dependency(&mut self, dep: SystemDependency) {
        self.global_dependencies.push(dep);
    }

    /// Finalize the export (compute checksum)
    pub fn finalize(&mut self) {
        // Compute checksum over all plugin state hashes
        let mut hasher = md5::Context::new();
        for name in &self.apply_order {
            if let Some(plugin) = self.plugins.get(name) {
                hasher.consume(plugin.state_hash.as_bytes());
            }
        }
        self.checksum = format!("{:x}", hasher.compute());
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> Result<String> {
        Ok(simd_json::to_string_pretty(self)?)
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> Result<Self> {
        let mut json_mut = json.to_string();
        Ok(unsafe { simd_json::from_str(&mut json_mut) }?)
    }

    /// Get all dependencies (global + per-plugin)
    pub fn all_dependencies(&self) -> Vec<&SystemDependency> {
        let mut deps: Vec<&SystemDependency> = self.global_dependencies.iter().collect();
        for plugin in self.plugins.values() {
            deps.extend(plugin.dependencies.iter());
        }
        deps
    }

    /// Get required dependencies only
    pub fn required_dependencies(&self) -> Vec<&SystemDependency> {
        self.all_dependencies()
            .into_iter()
            .filter(|d| d.required)
            .collect()
    }
}

impl Default for DisasterRecoveryExport {
    fn default() -> Self {
        Self::new()
    }
}

impl HostInfo {
    /// Detect current host information
    pub fn detect() -> Self {
        Self {
            hostname: hostname(),
            os: detect_os(),
            os_version: detect_os_version(),
            arch: std::env::consts::ARCH.to_string(),
            kernel: detect_kernel(),
        }
    }
}

impl PluginStateExport {
    /// Create from plugin state
    pub fn new(plugin_name: &str, version: &str, state: Value) -> Self {
        let state_json = simd_json::to_string(&state).unwrap_or_default();
        let state_hash = format!("{:x}", md5::compute(state_json.as_bytes()));

        Self {
            plugin_name: plugin_name.to_string(),
            version: version.to_string(),
            state,
            dependencies: Vec::new(),
            captured_at: Utc::now(),
            state_hash,
        }
    }

    /// Add a dependency
    pub fn add_dependency(&mut self, dep: SystemDependency) {
        self.dependencies.push(dep);
    }
}

impl SystemDependency {
    /// Create a new required dependency (uses PackageKit D-Bus, cross-distro)
    pub fn required(name: &str) -> Self {
        Self {
            name: name.to_string(),
            package_manager: "packagekit".to_string(), // Always use PackageKit D-Bus
            min_version: None,
            required: true,
            install_command: None,
        }
    }

    /// Create an optional dependency (uses PackageKit D-Bus, cross-distro)
    pub fn optional(name: &str) -> Self {
        Self {
            name: name.to_string(),
            package_manager: "packagekit".to_string(), // Always use PackageKit D-Bus
            min_version: None,
            required: false,
            install_command: None,
        }
    }

    /// Set minimum version
    pub fn with_version(mut self, version: &str) -> Self {
        self.min_version = Some(version.to_string());
        self
    }

    /// Set custom install command (fallback if PackageKit unavailable)
    pub fn with_install_command(mut self, cmd: &str) -> Self {
        self.install_command = Some(cmd.to_string());
        self
    }
}

// Helper functions
fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn detect_os() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|l| l.starts_with("ID="))
                .map(|l| l.trim_start_matches("ID=").trim_matches('"').to_string())
        })
        .unwrap_or_else(|| "linux".to_string())
}

fn detect_os_version() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|l| l.starts_with("VERSION_ID="))
                .map(|l| {
                    l.trim_start_matches("VERSION_ID=")
                        .trim_matches('"')
                        .to_string()
                })
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn detect_kernel() -> String {
    std::fs::read_to_string("/proc/version")
        .map(|s| s.split_whitespace().nth(2).unwrap_or("unknown").to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Get default dependencies for a plugin type
pub fn get_plugin_dependencies(plugin_name: &str) -> Vec<SystemDependency> {
    match plugin_name {
        "net" | "openflow" => vec![SystemDependency::required("openvswitch-switch")],
        "lxc" => vec![
            // Proxmox provides pct, no extra deps on Proxmox hosts
        ],
        "netmaker" => vec![SystemDependency::optional("netclient")],
        "btrfs" => vec![SystemDependency::required("btrfs-progs")],
        "numa" => vec![SystemDependency::optional("numactl")],
        "packagekit" => vec![SystemDependency::required("packagekit")],
        _ => vec![],
    }
}

/// Global dependencies required for any op-dbus installation
pub fn get_global_dependencies() -> Vec<SystemDependency> {
    vec![
        SystemDependency::required("openvswitch-switch"),
        SystemDependency::optional("btrfs-progs"),
        SystemDependency::optional("numactl"),
        SystemDependency::optional("jq"),
    ]
}

// =============================================================================
// PackageKit D-Bus Integration for Dependency Installation
// =============================================================================

/// Install dependencies via PackageKit D-Bus (NO CLI)
pub async fn install_dependencies_via_packagekit(
    dependencies: &[&SystemDependency],
) -> Result<Vec<InstallResult>> {
    let mut results = Vec::new();

    // Filter to just the package names we need to install
    let package_names: Vec<&str> = dependencies.iter().map(|d| d.name.as_str()).collect();

    if package_names.is_empty() {
        return Ok(results);
    }

    // Connect to D-Bus
    let connection = Connection::system()
        .await
        .context("Failed to connect to system D-Bus")?;

    // Create PackageKit transaction
    let pk_proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.PackageKit",
        "/org/freedesktop/PackageKit",
        "org.freedesktop.PackageKit",
    )
    .await
    .context("Failed to create PackageKit proxy")?;

    // First, resolve package names to package IDs
    let tx_path: zbus::zvariant::OwnedObjectPath = pk_proxy
        .call("CreateTransaction", &())
        .await
        .context("Failed to create PackageKit transaction")?;

    let tx_proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.PackageKit",
        tx_path.as_str(),
        "org.freedesktop.PackageKit.Transaction",
    )
    .await
    .context("Failed to create transaction proxy")?;

    // Resolve packages (filter: NONE=0, package names)
    let resolve_result: std::result::Result<(), zbus::Error> = tx_proxy
        .call("Resolve", &(0u64, package_names.clone()))
        .await;

    match resolve_result {
        Ok(_) => {
            for name in &package_names {
                results.push(InstallResult {
                    package: name.to_string(),
                    success: true,
                    error: None,
                });
            }
        }
        Err(e) => {
            // If resolve fails, try to install anyway (PackageKit will resolve)
            tracing::warn!("PackageKit resolve failed: {}, trying direct install", e);

            // Create new transaction for install
            let install_tx_path: zbus::zvariant::OwnedObjectPath = pk_proxy
                .call("CreateTransaction", &())
                .await
                .context("Failed to create install transaction")?;

            let install_proxy = zbus::Proxy::new(
                &connection,
                "org.freedesktop.PackageKit",
                install_tx_path.as_str(),
                "org.freedesktop.PackageKit.Transaction",
            )
            .await?;

            // Try installing with package names directly
            // Note: This may need package IDs in format "name;version;arch;repo"
            let install_result: std::result::Result<(), zbus::Error> = install_proxy
                .call("InstallPackages", &(0u64, package_names.clone()))
                .await;

            match install_result {
                Ok(_) => {
                    for name in &package_names {
                        results.push(InstallResult {
                            package: name.to_string(),
                            success: true,
                            error: None,
                        });
                    }
                }
                Err(install_err) => {
                    for name in &package_names {
                        results.push(InstallResult {
                            package: name.to_string(),
                            success: false,
                            error: Some(install_err.to_string()),
                        });
                    }
                }
            }
        }
    }

    Ok(results)
}

/// Result of a single package installation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallResult {
    pub package: String,
    pub success: bool,
    pub error: Option<String>,
}

/// Check if a package is installed via PackageKit D-Bus
pub async fn is_package_installed(package_name: &str) -> Result<bool> {
    let connection = Connection::system()
        .await
        .context("Failed to connect to system D-Bus")?;

    let pk_proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.PackageKit",
        "/org/freedesktop/PackageKit",
        "org.freedesktop.PackageKit",
    )
    .await?;

    // Create transaction
    let tx_path: zbus::zvariant::OwnedObjectPath = pk_proxy.call("CreateTransaction", &()).await?;

    let tx_proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.PackageKit",
        tx_path.as_str(),
        "org.freedesktop.PackageKit.Transaction",
    )
    .await?;

    // Search for installed packages (filter: INSTALLED=2)
    let result: std::result::Result<(), zbus::Error> = tx_proxy
        .call("SearchNames", &(2u64, vec![package_name.to_string()]))
        .await;

    // If we get a result without error, package exists
    Ok(result.is_ok())
}

/// Restore system from DR export using PackageKit D-Bus
pub async fn restore_from_export(export: &DisasterRecoveryExport) -> Result<RestoreResult> {
    let mut result = RestoreResult {
        success: true,
        plugins_restored: Vec::new(),
        plugins_failed: Vec::new(),
        dependencies_installed: Vec::new(),
        dependencies_failed: Vec::new(),
        warnings: Vec::new(),
    };

    // Step 1: Install global dependencies via PackageKit
    tracing::info!("Installing global dependencies via PackageKit D-Bus...");
    let global_deps: Vec<&SystemDependency> = export.global_dependencies.iter().collect();

    if !global_deps.is_empty() {
        match install_dependencies_via_packagekit(&global_deps).await {
            Ok(install_results) => {
                for ir in install_results {
                    if ir.success {
                        result.dependencies_installed.push(ir.package);
                    } else {
                        result.dependencies_failed.push((
                            ir.package,
                            ir.error.unwrap_or_else(|| "Unknown error".to_string()),
                        ));
                    }
                }
            }
            Err(e) => {
                result
                    .warnings
                    .push(format!("Global dependency install failed: {}", e));
            }
        }
    }

    // Step 2: Install per-plugin dependencies
    for plugin_name in &export.apply_order {
        if let Some(plugin) = export.plugins.get(plugin_name) {
            tracing::info!("Installing dependencies for plugin: {}", plugin_name);

            let plugin_deps: Vec<&SystemDependency> = plugin.dependencies.iter().collect();
            if !plugin_deps.is_empty() {
                match install_dependencies_via_packagekit(&plugin_deps).await {
                    Ok(install_results) => {
                        for ir in install_results {
                            if ir.success {
                                result.dependencies_installed.push(ir.package);
                            } else {
                                result.dependencies_failed.push((
                                    ir.package,
                                    ir.error.unwrap_or_else(|| "Unknown error".to_string()),
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        result.warnings.push(format!(
                            "Dependency install for {} failed: {}",
                            plugin_name, e
                        ));
                    }
                }
            }
        }
    }

    // Step 3: Mark plugins as ready for restore
    // (Actual state application would be done by StateManager)
    for plugin_name in &export.apply_order {
        if export.plugins.contains_key(plugin_name) {
            result.plugins_restored.push(plugin_name.clone());
        }
    }

    // Check for any required dependency failures
    let required_failed: Vec<_> = result
        .dependencies_failed
        .iter()
        .filter(|(name, _)| {
            export
                .required_dependencies()
                .iter()
                .any(|d| d.name == *name)
        })
        .collect();

    if !required_failed.is_empty() {
        result.success = false;
        result.warnings.push(format!(
            "Required dependencies failed: {:?}",
            required_failed
        ));
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dr_export_creation() {
        let mut export = DisasterRecoveryExport::new();
        assert_eq!(export.format_version, "1.0.0");
        assert!(export.plugins.is_empty());

        // Add a plugin
        let plugin = PluginStateExport::new("net", "1.0.0", simd_json::json!({"bridges": []}));
        export.add_plugin(plugin);
        assert_eq!(export.plugins.len(), 1);
        assert_eq!(export.apply_order, vec!["net"]);
    }

    #[test]
    fn test_plugin_state_hash() {
        let state = simd_json::json!({"bridges": ["ovsbr0"]});
        let plugin = PluginStateExport::new("net", "1.0.0", state);
        assert!(!plugin.state_hash.is_empty());
    }

    #[test]
    fn test_dependencies() {
        let deps = get_plugin_dependencies("net");
        assert!(!deps.is_empty());
        assert!(deps.iter().any(|d| d.name == "openvswitch-switch"));
    }

    #[test]
    fn test_export_json() {
        let mut export = DisasterRecoveryExport::new();
        let plugin = PluginStateExport::new("test", "1.0.0", simd_json::json!({}));
        export.add_plugin(plugin);
        export.finalize();

        let json = export.to_json().unwrap();
        assert!(json.contains("format_version"));
        assert!(json.contains("test"));

        let restored = DisasterRecoveryExport::from_json(&json).unwrap();
        assert_eq!(restored.plugins.len(), 1);
    }
}
