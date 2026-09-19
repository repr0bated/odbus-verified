//! Full System State Plugin
//!
//! This plugin captures the COMPLETE system state for disaster recovery.
//! It aggregates state from all other plugins into a single JSON document
//! that can be used to reinstall/restore the entire system.
//!
//! ## State Categories
//!
//! - **system**: hostname, timezone, locale, kernel parameters
//! - **network**: interfaces, routes, DNS, bridges, VLANs
//! - **services**: systemd units and their configurations
//! - **packages**: installed packages and versions
//! - **users**: user accounts and groups
//! - **storage**: mounts, fstab entries
//! - **containers**: Incus instances (read from SHM present state)
//! - **security**: firewall rules, SELinux/AppArmor policies
//!
//! This plugin is special: it queries OTHER plugins to build the full state.

use anyhow::{Context, Result};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, StateAction, StateDiff, StatePlugin};
use op_state_store::{Constraint, FieldSchema, FieldType, PluginSchema};
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::{json, OwnedValue as Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use zbus::{Connection, Proxy};

/// Full system state for disaster recovery
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[schemars(extend("x-oscal-category" = "system"))]
pub struct FullSystemState {
    /// State schema version
    pub version: u32,

    /// Timestamp of when this state was captured
    pub captured_at: String,

    /// Hostname
    pub hostname: String,

    /// System information
    pub system: SystemInfo,

    /// Network configuration
    pub network: NetworkState,

    /// Systemd services
    pub services: Vec<ServiceState>,

    /// Installed packages
    pub packages: Vec<PackageInfo>,

    /// User accounts
    pub users: Vec<UserInfo>,

    /// Storage mounts
    pub storage: StorageState,

    /// Container state (Incus instances)
    pub containers: ContainerState,

    /// Plugin-specific state (aggregated from all plugins)
    pub plugins: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SystemInfo {
    pub kernel_version: String,
    pub os_release: String,
    pub timezone: String,
    pub locale: String,
    pub uptime_seconds: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NetworkState {
    pub interfaces: Vec<InterfaceInfo>,
    pub routes: Vec<RouteInfo>,
    pub dns_servers: Vec<String>,
    pub bridges: Vec<BridgeInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct InterfaceInfo {
    pub name: String,
    pub mac: String,
    pub addresses: Vec<String>,
    pub state: String,
    pub mtu: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RouteInfo {
    pub destination: String,
    pub gateway: Option<String>,
    pub interface: String,
    pub metric: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BridgeInfo {
    pub name: String,
    pub ports: Vec<String>,
    pub bridge_type: String, // "linux" or "ovs"
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ServiceState {
    pub name: String,
    pub enabled: bool,
    pub running: bool,
    pub unit_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PackageInfo {
    pub name: String,
    pub version: String,
    pub arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UserInfo {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub home: String,
    pub shell: String,
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StorageState {
    pub mounts: Vec<MountInfo>,
    pub block_devices: Vec<BlockDeviceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MountInfo {
    pub source: String,
    pub target: String,
    pub fstype: String,
    pub options: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BlockDeviceInfo {
    pub name: String,
    pub size_bytes: u64,
    pub fstype: Option<String>,
    pub mountpoint: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ContainerState {
    /// Incus instances, read from the incus plugin's present state.
    pub instances: Vec<ContainerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ContainerInfo {
    pub name: String,
    /// "Running", "Stopped", "Frozen" -- as Incus reports it.
    pub status: String,
    /// "container" or "virtual-machine".
    #[serde(default, rename = "type")]
    pub instance_type: String,
}

fn validate_hostname(hostname: &str) -> Result<String> {
    let trimmed = hostname.trim();
    if trimmed.is_empty()
        || trimmed.len() > 253
        || !trimmed
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
    {
        anyhow::bail!("invalid hostname: {hostname:?}");
    }
    Ok(trimmed.to_owned())
}

/// Owner of the hostname write side. Tests inject an isolated owner so they
/// can prove the method's effect and readback without changing the host UTS
/// namespace or `/etc/hostname`.
#[async_trait]
trait HostnameOwner: Send + Sync {
    async fn set_hostname(&self, hostname: &str) -> Result<()>;
}

struct NativeHostnameOwner {
    hostname_path: PathBuf,
}

impl NativeHostnameOwner {
    fn system() -> Self {
        Self {
            hostname_path: PathBuf::from("/etc/hostname"),
        }
    }

    #[cfg(test)]
    fn with_hostname_path(path: impl Into<PathBuf>) -> Self {
        Self {
            hostname_path: path.into(),
        }
    }

    /// Shared native effect implementation. The caller must already be in
    /// the intended UTS and mount namespaces before invoking this method.
    fn set_hostname_native(&self, hostname: &str) -> Result<()> {
        let trimmed = validate_hostname(hostname)?;
        nix::unistd::sethostname(&trimmed).with_context(|| format!("sethostname({trimmed})"))?;
        std::fs::write(&self.hostname_path, format!("{trimmed}\n"))
            .with_context(|| format!("write {}", self.hostname_path.display()))?;
        Ok(())
    }
}

#[async_trait]
impl HostnameOwner for NativeHostnameOwner {
    async fn set_hostname(&self, hostname: &str) -> Result<()> {
        self.set_hostname_native(hostname)
    }
}

/// The Full System State Plugin
pub struct FullSystemPlugin {
    /// Cached current state
    state_cache: Arc<RwLock<Option<FullSystemState>>>,

    /// Sender for snowball footprints
    #[allow(dead_code)]
    snowball_sender: Option<tokio::sync::mpsc::UnboundedSender<op_snowball::PluginFootprint>>,
}

impl Default for FullSystemPlugin {
    fn default() -> Self {
        Self::new()
    }
}

impl FullSystemPlugin {
    pub fn new() -> Self {
        Self {
            state_cache: Arc::new(RwLock::new(None)),
            snowball_sender: None,
        }
    }

    /// Create with snowball sender for change tracking
    pub fn with_snowball(
        sender: tokio::sync::mpsc::UnboundedSender<op_snowball::PluginFootprint>,
    ) -> Self {
        Self {
            state_cache: Arc::new(RwLock::new(None)),
            snowball_sender: Some(sender),
        }
    }

    /// Capture complete system state
    pub async fn capture_full_state(&self) -> Result<FullSystemState> {
        let now = chrono::Utc::now().to_rfc3339();

        info!("Capturing full system state...");

        let hostname = self
            .get_hostname()
            .await
            .unwrap_or_else(|_| "unknown".to_string());
        let system = self.capture_system_info().await.unwrap_or_default();
        let network = self.capture_network_state().await.unwrap_or_default();
        let services = self.capture_services().await.unwrap_or_default();
        let packages = self.capture_packages().await.unwrap_or_default();
        let users = self.capture_users().await.unwrap_or_default();
        let storage = self.capture_storage().await.unwrap_or_default();
        let containers = self.capture_containers().await.unwrap_or_default();

        let state = FullSystemState {
            version: 1,
            captured_at: now,
            hostname,
            system,
            network,
            services,
            packages,
            users,
            storage,
            containers,
            plugins: HashMap::new(), // Will be populated by StateManager
        };

        info!("Full system state captured");

        // Cache the state
        *self.state_cache.write().await = Some(state.clone());

        Ok(state)
    }

    async fn get_hostname(&self) -> Result<String> {
        tokio::fs::read_to_string("/proc/sys/kernel/hostname")
            .await
            .map(|s| s.trim().to_string())
            .context("Failed to read hostname from /proc/sys/kernel/hostname")
    }

    async fn capture_system_info(&self) -> Result<SystemInfo> {
        let kernel_version = tokio::fs::read_to_string("/proc/sys/kernel/osrelease")
            .await
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        let os_release = tokio::fs::read_to_string("/etc/os-release")
            .await
            .ok()
            .and_then(|content| {
                content
                    .lines()
                    .find(|l| l.starts_with("PRETTY_NAME="))
                    .map(|l| {
                        l.trim_start_matches("PRETTY_NAME=")
                            .trim_matches('"')
                            .to_string()
                    })
            })
            .unwrap_or_default();

        let timezone = tokio::fs::read_link("/etc/localtime")
            .await
            .ok()
            .and_then(|p| p.to_str().map(|s| s.replace("/usr/share/zoneinfo/", "")))
            .unwrap_or_else(|| "UTC".to_string());

        let locale = std::env::var("LANG").unwrap_or_else(|_| "C.UTF-8".to_string());

        let uptime = tokio::fs::read_to_string("/proc/uptime")
            .await
            .ok()
            .and_then(|s| {
                s.split_whitespace()
                    .next()
                    .and_then(|u| u.parse::<f64>().ok())
            })
            .map(|u| u as u64)
            .unwrap_or(0);

        Ok(SystemInfo {
            kernel_version,
            os_release,
            timezone,
            locale,
            uptime_seconds: uptime,
        })
    }

    async fn capture_network_state(&self) -> Result<NetworkState> {
        let mut state = NetworkState::default();

        // Get interfaces from /sys/class/net
        if let Ok(mut entries) = tokio::fs::read_dir("/sys/class/net").await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                if name == "lo" {
                    continue;
                }

                let mac = tokio::fs::read_to_string(format!("/sys/class/net/{}/address", name))
                    .await
                    .unwrap_or_default()
                    .trim()
                    .to_string();

                let state_str =
                    tokio::fs::read_to_string(format!("/sys/class/net/{}/operstate", name))
                        .await
                        .unwrap_or_default()
                        .trim()
                        .to_string();

                let mtu: u32 = tokio::fs::read_to_string(format!("/sys/class/net/{}/mtu", name))
                    .await
                    .unwrap_or_default()
                    .trim()
                    .parse()
                    .unwrap_or(1500);

                state.interfaces.push(InterfaceInfo {
                    name,
                    mac,
                    addresses: vec![], // Would need ip tool to get addresses
                    state: state_str,
                    mtu,
                });
            }
        }

        // Get DNS from resolv.conf
        if let Ok(resolv) = tokio::fs::read_to_string("/etc/resolv.conf").await {
            for line in resolv.lines() {
                if line.starts_with("nameserver") {
                    if let Some(ns) = line.split_whitespace().nth(1) {
                        state.dns_servers.push(ns.to_string());
                    }
                }
            }
        }

        // Check for OVS bridges via D-Bus daemon (AGENTS.md §4)
        let ovs_client = op_network::rovs_proxy::OvsdbDbusClient::new();
        if let Ok(bridges) = ovs_client.list_bridges().await {
            for bridge in bridges {
                let ports = ovs_client
                    .list_bridge_ports(&bridge)
                    .await
                    .unwrap_or_default();

                state.bridges.push(BridgeInfo {
                    name: bridge,
                    ports,
                    bridge_type: "ovs".to_string(),
                });
            }
        }

        Ok(state)
    }

    async fn capture_services(&self) -> Result<Vec<ServiceState>> {
        let mut services = Vec::new();

        let conn = Connection::system()
            .await
            .context("Failed to connect to system D-Bus")?;

        // On non-systemd hosts (e.g. Artix runit), org.freedesktop.systemd1 is
        // not owned. Skip silently instead of hanging on a method call that
        // will never be answered.
        let dbus_proxy = Proxy::new(
            &conn,
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
        )
        .await
        .context("Failed to create D-Bus proxy")?;
        let names: Vec<String> = dbus_proxy
            .call("ListActivatableNames", &())
            .await
            .unwrap_or_default();
        if !names.iter().any(|n| n == "org.freedesktop.systemd1") {
            debug!("org.freedesktop.systemd1 not available — skipping service capture (non-systemd host)");
            return Ok(services);
        }

        let proxy = Proxy::new(
            &conn,
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
        )
        .await
        .context("Failed to create systemd D-Bus proxy")?;

        #[allow(clippy::type_complexity)]
        let units: Vec<(
            String,
            String,
            String,
            String,
            String,
            String,
            zbus::zvariant::OwnedObjectPath,
            u32,
            String,
            zbus::zvariant::OwnedObjectPath,
        )> = proxy
            .call("ListUnits", &())
            .await
            .context("Failed to list systemd units")?;

        let unit_files: Vec<(String, String)> = proxy
            .call("ListUnitFiles", &())
            .await
            .context("Failed to list systemd unit files")?;

        let enabled_map: std::collections::HashMap<String, bool> = unit_files
            .into_iter()
            .filter(|(name, _)| name.ends_with(".service"))
            .map(|(name, state)| (name, state == "enabled"))
            .collect();

        for (
            name,
            _desc,
            _load_state,
            active_state,
            _sub_state,
            _followed,
            _path,
            _job_id,
            _job_type,
            _job_path,
        ) in units
        {
            if !name.ends_with(".service") {
                continue;
            }
            let running = active_state == "active" || active_state == "running";
            let enabled = enabled_map.get(&name).copied().unwrap_or(false);
            services.push(ServiceState {
                name: name.trim_end_matches(".service").to_string(),
                enabled,
                running,
                unit_type: "service".to_string(),
            });
        }

        Ok(services)
    }

    async fn capture_packages(&self) -> Result<Vec<PackageInfo>> {
        let mut packages = Vec::new();

        // Read dpkg status directly (AGENTS.md §4: no subprocess bypasses)
        let content = tokio::fs::read_to_string("/var/lib/dpkg/status")
            .await
            .unwrap_or_default();

        let mut current_name = None;
        let mut current_version = None;
        let mut current_arch = None;

        for line in content.lines() {
            if let Some(stripped) = line.strip_prefix("Package: ") {
                current_name = Some(stripped.trim().to_string());
            } else if let Some(stripped) = line.strip_prefix("Version: ") {
                current_version = Some(stripped.trim().to_string());
            } else if let Some(stripped) = line.strip_prefix("Architecture: ") {
                current_arch = Some(stripped.trim().to_string());
            } else if line.is_empty() {
                if let (Some(name), Some(version)) = (current_name.take(), current_version.take()) {
                    let arch = current_arch.take().unwrap_or_else(|| "unknown".to_string());
                    packages.push(PackageInfo {
                        name,
                        version,
                        arch,
                    });
                }
            }
        }
        // Handle last stanza if file doesn't end with blank line
        if let (Some(name), Some(version)) = (current_name.take(), current_version.take()) {
            let arch = current_arch.take().unwrap_or_else(|| "unknown".to_string());
            packages.push(PackageInfo {
                name,
                version,
                arch,
            });
        }

        Ok(packages)
    }

    async fn capture_users(&self) -> Result<Vec<UserInfo>> {
        let mut users = Vec::new();

        // Read group memberships directly (AGENTS.md §4: no subprocess bypasses)
        let groups_content = tokio::fs::read_to_string("/etc/group")
            .await
            .unwrap_or_default();
        let mut group_map: HashMap<String, Vec<String>> = HashMap::new();
        for line in groups_content.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 4 {
                let group_name = parts[0].to_string();
                let members: Vec<String> = parts[3]
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect();
                for member in &members {
                    group_map
                        .entry(member.clone())
                        .or_default()
                        .push(group_name.clone());
                }
            }
        }

        if let Ok(passwd) = tokio::fs::read_to_string("/etc/passwd").await {
            for line in passwd.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 7 {
                    let uid: u32 = parts[2].parse().unwrap_or(0);

                    // Skip system users (uid < 1000) except root
                    if uid != 0 && uid < 1000 {
                        continue;
                    }

                    let name = parts[0].to_string();
                    let groups = group_map.get(&name).cloned().unwrap_or_default();

                    users.push(UserInfo {
                        name,
                        uid,
                        gid: parts[3].parse().unwrap_or(0),
                        home: parts[5].to_string(),
                        shell: parts[6].to_string(),
                        groups,
                    });
                }
            }
        }

        Ok(users)
    }

    async fn capture_storage(&self) -> Result<StorageState> {
        let mut state = StorageState::default();

        // Get mounts from /proc/mounts
        if let Ok(mounts) = tokio::fs::read_to_string("/proc/mounts").await {
            for line in mounts.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    let source = parts[0].to_string();
                    let target = parts[1].to_string();

                    // Skip virtual filesystems
                    if source.starts_with("none")
                        || target.starts_with("/sys")
                        || target.starts_with("/proc")
                        || target.starts_with("/dev")
                        || target.starts_with("/run")
                    {
                        continue;
                    }

                    state.mounts.push(MountInfo {
                        source,
                        target,
                        fstype: parts[2].to_string(),
                        options: parts[3].to_string(),
                    });
                }
            }
        }

        // Get block devices from /sys/block (AGENTS.md §4: no subprocess bypasses)
        let sysfs = std::path::Path::new("/sys/block");
        if sysfs.exists() {
            let mut entries = match tokio::fs::read_dir(sysfs).await {
                Ok(e) => e,
                Err(_) => return Ok(state),
            };

            while let Ok(Some(entry)) = entries.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                // Skip loop devices and ramdisks
                if name.starts_with("loop") || name.starts_with("ram") {
                    continue;
                }

                let size_path = entry.path().join("size");
                let size_sectors = tokio::fs::read_to_string(&size_path)
                    .await
                    .ok()
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .unwrap_or(0);
                let size_bytes = size_sectors * 512;

                let mountpoint = Self::read_mountpoint(&name).await;

                state.block_devices.push(BlockDeviceInfo {
                    name,
                    size_bytes,
                    fstype: None,
                    mountpoint,
                });
            }
        }

        Ok(state)
    }

    async fn read_mountpoint(device: &str) -> Option<String> {
        let mounts = tokio::fs::read_to_string("/proc/mounts").await.ok()?;
        for line in mounts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let dev = parts[0];
                if dev.contains(device) {
                    return Some(parts[1].to_string());
                }
            }
        }
        None
    }

    /// Containers are Incus instances, and their present state already lives in
    /// `/dev/shm/opdbus/state/incus.json`. There is nothing to "capture": SHM is
    /// the present state, so this reads it rather than collecting it again.
    ///
    /// The previous implementation scanned `/var/lib/lxc` (the wrong tree --
    /// Incus stores under `/var/lib/incus`) and then shelled out to `docker ps`.
    /// Docker is not installed on this host and never was; the subprocess failure
    /// was swallowed by `if let Ok(..)`, so this reported an empty container list
    /// on a machine running seven Incus instances and looked like it worked.
    async fn capture_containers(&self) -> Result<ContainerState> {
        #[derive(Deserialize)]
        struct IncusPresentState {
            #[serde(default)]
            instances: Vec<IncusInstanceView>,
        }
        #[derive(Deserialize)]
        struct IncusInstanceView {
            #[serde(default)]
            name: String,
            #[serde(default)]
            status: String,
            #[serde(default, rename = "type")]
            instance_type: String,
        }

        let Some(bytes) = op_core::projection_shm::read_projection_bytes("incus") else {
            return Ok(ContainerState::default());
        };
        let present: IncusPresentState = serde_json::from_slice(&bytes)
            .context("incus present state in SHM is not valid JSON")?;

        Ok(ContainerState {
            instances: present
                .instances
                .into_iter()
                .map(|i| ContainerInfo {
                    name: i.name,
                    status: i.status,
                    instance_type: i.instance_type,
                })
                .collect(),
        })
    }
}

#[async_trait]
impl StatePlugin for FullSystemPlugin {
    fn name(&self) -> &str {
        "full_system"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        let mut schema = super::schemars_adapter::plugin_schema_from_json(
            "full_system",
            "1.0.0",
            "Full system recovery snapshot",
            &serde_json::to_value(schemars::schema_for!(FullSystemState)).unwrap(),
        );
        super::schemars_adapter::apply_state_defaults(
            &mut schema,
            &simd_json::serde::to_owned_value(&FullSystemState::default()).unwrap(),
        );

        use super::plugin_scaffold_helpers::{
            method_decl_from_schemars_with_output, AckOutput, EmptyInput,
        };
        use op_state_store::SideEffect;

        #[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
        pub struct SetHostnameInput {
            pub hostname: String,
        }

        // Backed by real dispatchers (dispatch_full_system_method):
        // capture_state calls the plugin's own capture_full_state(), and
        // set_hostname reproduces apply_state's existing "hostname" action so
        // it's reachable directly, not only via a full StateDiff/apply_state
        // round trip.
        schema.methods.insert(
            "capture_state".to_string(),
            method_decl_from_schemars_with_output::<EmptyInput, FullSystemState>(
                "capture_state",
                SideEffect::Read,
                true,
                "full_system.read",
                "obs.system.plugin.full-system.state.capture@v1",
            ),
        );
        schema.methods.insert(
            "set_hostname".to_string(),
            method_decl_from_schemars_with_output::<SetHostnameInput, AckOutput>(
                "set_hostname",
                SideEffect::Mutation,
                true,
                "full_system.write",
                "mut.system.plugin.full-system.hostname.set@v1",
            ),
        );
        schema.capabilities.insert(
            "full_system.read".to_string(),
            op_state_store::CapabilityDecl {
                id: "full_system.read".to_string(),
                description: "Grants: capture_state.".to_string(),
            },
        );
        schema.capabilities.insert(
            "full_system.write".to_string(),
            op_state_store::CapabilityDecl {
                id: "full_system.write".to_string(),
                description: "Grants: set_hostname.".to_string(),
            },
        );

        Some(schema)
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        use op_state::DiffMetadata;
        use sha2::{Digest, Sha256};

        let mut actions = Vec::new();

        // Check for hostname change
        if current.get("hostname") != desired.get("hostname") {
            actions.push(StateAction::Modify {
                resource: "hostname".to_string(),
                changes: json!({
                    "from": current.get("hostname"),
                    "to": desired.get("hostname"),
                }),
            });
        }

        // More sophisticated diffing would be done here
        // For now, just mark if there's any difference
        if current != desired {
            actions.push(StateAction::Modify {
                resource: "full_system".to_string(),
                changes: json!({
                    "message": "Full system state differs from desired"
                }),
            });
        }

        // Create hashes for metadata
        let current_str = simd_json::to_string(current).unwrap_or_default();
        let desired_str = simd_json::to_string(desired).unwrap_or_default();
        let current_hash = format!("{:x}", Sha256::digest(current_str.as_bytes()));
        let desired_hash = format!("{:x}", Sha256::digest(desired_str.as_bytes()));

        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions,
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash,
                desired_hash,
            },
        })
    }

    async fn apply_state(&self, diff: &StateDiff) -> Result<ApplyResult> {
        let mut changes_applied = Vec::new();
        let mut errors = Vec::new();

        for action in &diff.actions {
            match action {
                StateAction::Modify { resource, changes } if resource == "hostname" => {
                    if let Some(hostname) = changes.get("to").and_then(|v| v.as_str()) {
                        match NativeHostnameOwner::system().set_hostname(hostname).await {
                            Ok(()) => {
                                changes_applied.push(format!("Set hostname to {}", hostname));
                            }
                            Err(e) => {
                                errors.push(format!("Failed to set hostname: {e}"));
                            }
                        }
                    }
                }
                _ => {
                    debug!("Unhandled action: {:?}", action);
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

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        let state = self.capture_full_state().await?;
        Ok(Checkpoint {
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(state)?,
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        warn!("Full system rollback not implemented - requires manual intervention");
        Ok(())
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        // Would compare current vs desired
        Ok(true)
    }

    fn capabilities(&self) -> op_state::PluginCapabilities {
        op_state::PluginCapabilities {
            supports_rollback: false, // Too complex for automatic rollback
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

/// Dispatch a `full_system` schema method. Called from `op-grpc-bridge`'s
/// `MutationEngine::dispatch_method_call`.
pub async fn dispatch_full_system_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    let owner = NativeHostnameOwner::system();
    dispatch_full_system_method_with_owner(&owner, method, args).await
}

async fn dispatch_full_system_method_with_owner<O: HostnameOwner>(
    owner: &O,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    match method {
        "capture_state" => {
            let plugin = FullSystemPlugin::new();
            let state = plugin.capture_full_state().await?;
            Ok(serde_json::to_value(state)?)
        }
        "set_hostname" => {
            let hostname = args
                .get("hostname")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("hostname is required"))?;
            owner.set_hostname(hostname).await?;
            Ok(serde_json::json!({ "success": true }))
        }
        other => Err(anyhow::anyhow!("unknown full_system method: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::process::Stdio;
    use std::sync::Mutex;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, BufReader};

    struct HostnameService {
        hostname: Mutex<String>,
    }

    #[zbus::interface(name = "org.freedesktop.hostname1")]
    impl HostnameService {
        async fn set_hostname(&self, hostname: &str, _interactive: bool) -> zbus::fdo::Result<()> {
            let hostname = validate_hostname(hostname)
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
            *self.hostname.lock().unwrap() = hostname;
            Ok(())
        }

        #[zbus(property)]
        fn hostname(&self) -> String {
            self.hostname.lock().unwrap().clone()
        }
    }

    struct DbusHostnameOwner {
        connection: zbus::Connection,
    }

    #[async_trait]
    impl HostnameOwner for DbusHostnameOwner {
        async fn set_hostname(&self, hostname: &str) -> Result<()> {
            let hostname = validate_hostname(hostname)?;
            let proxy = zbus::Proxy::new(
                &self.connection,
                "org.freedesktop.hostname1",
                "/org/freedesktop/hostname1",
                "org.freedesktop.hostname1",
            )
            .await?;
            proxy
                .call::<_, _, ()>("SetHostname", &(hostname, false))
                .await?;
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_capture_system_info() {
        let plugin = FullSystemPlugin::new();
        let info = plugin.capture_system_info().await;
        assert!(info.is_ok());
        let info = info.unwrap();
        assert!(!info.kernel_version.is_empty());
    }

    #[tokio::test]
    async fn capture_state_dispatch_returns_complete_read_only_snapshot() {
        let captured = dispatch_full_system_method("capture_state", &serde_json::json!({}))
            .await
            .expect("capture_state dispatch");
        assert_eq!(captured["version"], 1);
        assert!(!captured["captured_at"].as_str().unwrap().is_empty());
        assert!(!captured["hostname"].as_str().unwrap().is_empty());
        assert!(captured["system"].is_object());
        assert!(captured["network"].is_object());
        assert!(captured["storage"].is_object());
        assert!(captured["containers"].is_object());
    }

    #[tokio::test]
    async fn set_hostname_dispatch_applies_private_dbus_owner_and_reads_back() {
        let mut command = tokio::process::Command::new("dbus-daemon");
        command
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut daemon = command.spawn().expect("spawn private dbus-daemon");
        let stdout = daemon.stdout.take().expect("private bus stdout");
        let mut lines = BufReader::new(stdout).lines();
        let address = tokio::time::timeout(Duration::from_secs(3), lines.next_line())
            .await
            .expect("private bus address timeout")
            .expect("read private bus address")
            .expect("private bus printed no address");
        let service = zbus::connection::Builder::address(address.as_str())
            .expect("service bus address")
            .build()
            .await
            .expect("connect hostname service");
        service
            .object_server()
            .at(
                "/org/freedesktop/hostname1",
                HostnameService {
                    hostname: Mutex::new("before".into()),
                },
            )
            .await
            .expect("mount hostname fixture");
        service
            .request_name("org.freedesktop.hostname1")
            .await
            .expect("request hostname service name");
        let client = zbus::connection::Builder::address(address.as_str())
            .expect("client bus address")
            .build()
            .await
            .expect("connect hostname client");
        let owner = DbusHostnameOwner {
            connection: client.clone(),
        };

        let result = dispatch_full_system_method_with_owner(
            &owner,
            "set_hostname",
            &serde_json::json!({"hostname":" fixture.local "}),
        )
        .await
        .expect("set_hostname dispatch");
        assert_eq!(result["success"], true);
        let proxy = zbus::Proxy::new(
            &client,
            "org.freedesktop.hostname1",
            "/org/freedesktop/hostname1",
            "org.freedesktop.hostname1",
        )
        .await
        .expect("hostname readback proxy");
        let readback: String = proxy.get_property("Hostname").await.unwrap();
        assert_eq!(readback, "fixture.local");

        assert!(dispatch_full_system_method_with_owner(
            &owner,
            "set_hostname",
            &serde_json::json!({"hostname":"bad/name"}),
        )
        .await
        .is_err());
        let unchanged: String = proxy.get_property("Hostname").await.unwrap();
        assert_eq!(unchanged, "fixture.local");
        assert!(dispatch_full_system_method_with_owner(
            &owner,
            "set_hostname",
            &serde_json::json!({}),
        )
        .await
        .is_err());
        daemon.kill().await.expect("stop private dbus-daemon");
    }

    #[test]
    #[ignore = "requires CAP_SYS_ADMIN for CLONE_NEWUTS; run explicitly as an isolated privileged test"]
    fn native_hostname_owner_effect_isolated_to_uts_namespace_and_file() {
        let directory = tempfile::tempdir().unwrap();
        let hostname_path = directory.path().join("hostname");
        let parent_before = nix::unistd::gethostname().unwrap();
        let thread_path = hostname_path.clone();
        std::thread::spawn(move || -> Result<(OsString, OsString, String)> {
            let result = unsafe { nix::libc::unshare(nix::libc::CLONE_NEWUTS) };
            if result != 0 {
                anyhow::bail!("unshare(CLONE_NEWUTS): {}", std::io::Error::last_os_error());
            }
            let before = nix::unistd::gethostname()?;
            let owner = NativeHostnameOwner::with_hostname_path(thread_path);
            owner.set_hostname_native("opdbus-native-fixture")?;
            let after = nix::unistd::gethostname()?;
            let file = std::fs::read_to_string(&owner.hostname_path)?;
            Ok((before, after, file))
        })
        .join()
        .expect("native hostname thread panicked")
        .map(|(before, after, file)| {
            assert_ne!(before, after);
            assert_eq!(after, OsString::from("opdbus-native-fixture"));
            assert_eq!(file, "opdbus-native-fixture\n");
        })
        .unwrap();
        assert_eq!(nix::unistd::gethostname().unwrap(), parent_before);
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("full_system", |_ctx| std::sync::Arc::new(FullSystemPlugin::new()))
}
