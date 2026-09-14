//! Host runtime observations — runit services, `/proc`, `/sys`, socket probes.
//!
//! Replaces the legacy `RuntimeMirror` gRPC side door with schema-backed plugin
//! methods routed through `MutationEngine::dispatch_method_call`.

use anyhow::{Context, Result};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};

const RUNIT_ACTIVE_DIR: &str = "/etc/runit/runsvdir/default";

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct HostRuntimeState {
    #[serde(default)]
    pub last_queried_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetServiceInput {
    pub service_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListServicesInput {
    #[serde(default)]
    pub state_filter: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CheckUnixSocketsInput {
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SampleMetricsInput {
    #[serde(default)]
    pub previous_cpu_idle: Option<u64>,
    #[serde(default)]
    pub previous_cpu_total: Option<u64>,
    #[serde(default)]
    pub previous_rx: Option<u64>,
    #[serde(default)]
    pub previous_tx: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ServiceInfo {
    pub name: String,
    pub state: String,
    pub pid: u32,
    pub enabled: bool,
    pub description: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListServicesOutput {
    pub services: Vec<ServiceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SystemInfoOutput {
    pub hostname: String,
    pub kernel_version: String,
    pub uptime_seconds: u64,
    pub boot_timestamp: u64,
    pub cpu_count: u32,
    pub memory_total_bytes: u64,
    pub memory_available_bytes: u64,
    pub memory_used_bytes: u64,
    pub init_system: String,
    pub arch: String,
    pub queried_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NetworkInterfaceInfo {
    pub name: String,
    pub index: u32,
    pub mac_address: String,
    pub state: String,
    pub mtu: u32,
    #[serde(default)]
    pub ipv4_addresses: Vec<String>,
    #[serde(default)]
    pub ipv6_addresses: Vec<String>,
    #[serde(default)]
    pub rx_bytes: u64,
    #[serde(default)]
    pub tx_bytes: u64,
    #[serde(default)]
    pub rx_packets: u64,
    #[serde(default)]
    pub tx_packets: u64,
    #[serde(default)]
    pub driver: String,
    #[serde(default)]
    pub speed_mbps: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListInterfacesOutput {
    pub interfaces: Vec<NetworkInterfaceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NumaNodeInfo {
    pub node_id: u32,
    #[serde(default)]
    pub cpus: Vec<u32>,
    pub memory_total_bytes: u64,
    pub memory_free_bytes: u64,
    pub memory_used_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NumaTopologyOutput {
    pub nodes: Vec<NumaNodeInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UnixSocketStatus {
    pub path: String,
    pub exists: bool,
    pub connectable: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CheckUnixSocketsOutput {
    pub sockets: Vec<UnixSocketStatus>,
    pub queried_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MetricSample {
    pub category: String,
    pub name: String,
    pub value: f64,
    pub unit: String,
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SampleMetricsOutput {
    pub metrics: Vec<MetricSample>,
    pub cpu_idle: u64,
    pub cpu_total: u64,
    pub rx_total: u64,
    pub tx_total: u64,
    pub sampled_at: String,
}

pub struct HostRuntimePlugin;

impl Default for HostRuntimePlugin {
    fn default() -> Self {
        Self
    }
}

impl HostRuntimePlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for HostRuntimePlugin {
    fn name(&self) -> &str {
        "host_runtime"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(host_runtime_schema())
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: String::new(),
                desired_hash: String::new(),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: vec![],
            errors: vec![],
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: format!("host-runtime-{}", chrono::Utc::now().timestamp()),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::json!(null),
            backend_checkpoint: None,
        })
    }

    async fn rollback(&self, _checkpoint: &Checkpoint) -> Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> PluginCapabilities {
        PluginCapabilities {
            supports_rollback: false,
            supports_checkpoints: true,
            supports_verification: true,
            atomic_operations: false,
        }
    }
}

pub(crate) fn host_runtime_schema() -> PluginSchema {
    use super::plugin_scaffold_helpers::{method_decl_from_schemars_with_output, EmptyInput};
    use op_state_store::SideEffect;

    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "host_runtime",
        "1.0.0",
        "Host runtime observations (runit, procfs, sysfs)",
        &serde_json::to_value(schemars::schema_for!(HostRuntimeState)).unwrap(),
    );

    schema.methods.insert(
        "get_system_info".to_string(),
        method_decl_from_schemars_with_output::<EmptyInput, SystemInfoOutput>(
            "get_system_info",
            SideEffect::Read,
            true,
            "host_runtime.read",
            "obs.service.host-runtime.system-info.get@v1",
        ),
    );
    schema.methods.insert(
        "list_services".to_string(),
        method_decl_from_schemars_with_output::<ListServicesInput, ListServicesOutput>(
            "list_services",
            SideEffect::Read,
            true,
            "host_runtime.read",
            "obs.service.host-runtime.services.list@v1",
        ),
    );
    schema.methods.insert(
        "get_service".to_string(),
        method_decl_from_schemars_with_output::<GetServiceInput, ServiceInfo>(
            "get_service",
            SideEffect::Read,
            true,
            "host_runtime.read",
            "obs.service.host-runtime.service.get@v1",
        ),
    );
    schema.methods.insert(
        "list_interfaces".to_string(),
        method_decl_from_schemars_with_output::<EmptyInput, ListInterfacesOutput>(
            "list_interfaces",
            SideEffect::Read,
            true,
            "host_runtime.read",
            "obs.network.host-runtime.interfaces.list@v1",
        ),
    );
    schema.methods.insert(
        "get_numa_topology".to_string(),
        method_decl_from_schemars_with_output::<EmptyInput, NumaTopologyOutput>(
            "get_numa_topology",
            SideEffect::Read,
            true,
            "host_runtime.read",
            "obs.service.host-runtime.numa.get@v1",
        ),
    );
    schema.methods.insert(
        "check_unix_sockets".to_string(),
        method_decl_from_schemars_with_output::<CheckUnixSocketsInput, CheckUnixSocketsOutput>(
            "check_unix_sockets",
            SideEffect::Read,
            true,
            "host_runtime.read",
            "obs.service.host-runtime.unix-sockets.check@v1",
        ),
    );
    schema.methods.insert(
        "sample_metrics".to_string(),
        method_decl_from_schemars_with_output::<SampleMetricsInput, SampleMetricsOutput>(
            "sample_metrics",
            SideEffect::Read,
            true,
            "host_runtime.read",
            "obs.service.host-runtime.metrics.sample@v1",
        ),
    );

    schema.capabilities.insert(
        "host_runtime.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "host_runtime.read".to_string(),
            description: "Grants: get_system_info, list_services, get_service, list_interfaces, get_numa_topology, check_unix_sockets, sample_metrics.".to_string(),
        },
    );

    schema
}

fn valid_runit_service_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['-', '.'])
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '@'))
}

fn parse_runit_pid(status: &str) -> u32 {
    status
        .split("(pid ")
        .nth(1)
        .and_then(|tail| tail.split(')').next())
        .and_then(|pid| pid.parse().ok())
        .unwrap_or(0)
}

async fn runit_service_info(name: String, path: PathBuf) -> Result<ServiceInfo> {
    let output = tokio::process::Command::new("sv")
        .arg("status")
        .arg(&path)
        .output()
        .await
        .with_context(|| format!("sv status {name}"))?;
    let description = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let running = description.starts_with("run:");

    Ok(ServiceInfo {
        name,
        state: if running {
            "STARTED".to_string()
        } else {
            "STOPPED".to_string()
        },
        pid: parse_runit_pid(&description),
        enabled: path.exists(),
        description,
        dependencies: vec![],
    })
}

fn parse_meminfo_kb(meminfo: &str, key: &str) -> u64 {
    meminfo
        .lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1).and_then(|s| s.parse().ok()))
        .unwrap_or(0)
}

fn parse_node_meminfo_kb(meminfo: &str, key: &str) -> u64 {
    meminfo
        .lines()
        .find(|l| l.contains(key))
        .and_then(|l| {
            l.split_whitespace()
                .rev()
                .nth(1)
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(0)
}

async fn probe_unix_socket(path: &str) -> UnixSocketStatus {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return UnixSocketStatus {
            path: path.to_string(),
            exists: false,
            connectable: false,
            detail: "empty path".to_string(),
        };
    }

    let meta = tokio::fs::metadata(trimmed).await;
    let exists = meta.is_ok();
    let is_socket = meta
        .as_ref()
        .ok()
        .map(|m| m.file_type().is_socket())
        .unwrap_or(false);

    let connectable = if is_socket {
        tokio::time::timeout(
            std::time::Duration::from_millis(500),
            tokio::net::UnixStream::connect(trimmed),
        )
        .await
        .map(|r| r.is_ok())
        .unwrap_or(false)
    } else {
        false
    };

    let detail = match (exists, is_socket, connectable) {
        (false, _, _) => "missing".to_string(),
        (true, false, _) => "path exists but is not a socket".to_string(),
        (true, true, true) => "connectable".to_string(),
        (true, true, false) => "present but not accepting connections".to_string(),
    };

    UnixSocketStatus {
        path: trimmed.to_string(),
        exists,
        connectable,
        detail,
    }
}

async fn get_system_info() -> Result<SystemInfoOutput> {
    let hostname = tokio::fs::read_to_string("/etc/hostname")
        .await
        .unwrap_or_default()
        .trim()
        .to_string();
    let kernel_version = tokio::fs::read_to_string("/proc/version")
        .await
        .unwrap_or_default()
        .split_whitespace()
        .nth(2)
        .unwrap_or("")
        .to_string();
    let uptime_str = tokio::fs::read_to_string("/proc/uptime")
        .await
        .unwrap_or_default();
    let uptime_seconds = uptime_str
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0) as u64;

    let meminfo = tokio::fs::read_to_string("/proc/meminfo")
        .await
        .unwrap_or_default();
    let mem_total = parse_meminfo_kb(&meminfo, "MemTotal") * 1024;
    let mem_available = parse_meminfo_kb(&meminfo, "MemAvailable") * 1024;
    let mem_used = mem_total.saturating_sub(mem_available);

    let cpu_count = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);
    let init_system = if Path::new(RUNIT_ACTIVE_DIR).is_dir() {
        "runit"
    } else {
        "unknown"
    }
    .to_string();

    Ok(SystemInfoOutput {
        hostname,
        kernel_version,
        uptime_seconds,
        boot_timestamp: 0,
        cpu_count,
        memory_total_bytes: mem_total,
        memory_available_bytes: mem_available,
        memory_used_bytes: mem_used,
        init_system,
        arch: std::env::consts::ARCH.to_string(),
        queried_at: chrono::Utc::now().to_rfc3339(),
    })
}

async fn list_services(state_filter: &str) -> Result<ListServicesOutput> {
    let mut entries = tokio::fs::read_dir(RUNIT_ACTIVE_DIR)
        .await
        .with_context(|| format!("read {RUNIT_ACTIVE_DIR}"))?;
    let mut services = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().to_string();
        if !valid_runit_service_name(&name) {
            continue;
        }
        let info = runit_service_info(name, entry.path()).await?;
        if !state_filter.is_empty() && info.state != state_filter {
            continue;
        }
        services.push(info);
    }
    services.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(ListServicesOutput { services })
}

async fn get_service(name: &str) -> Result<ServiceInfo> {
    if !valid_runit_service_name(name) {
        anyhow::bail!("invalid runit service name");
    }
    let path = Path::new(RUNIT_ACTIVE_DIR).join(name);
    if !path.exists() {
        anyhow::bail!("runit service '{name}' is not enabled");
    }
    runit_service_info(name.to_string(), path).await
}

async fn list_interfaces() -> Result<ListInterfacesOutput> {
    let mut interfaces = Vec::new();
    let mut entries = tokio::fs::read_dir("/sys/class/net")
        .await
        .context("read /sys/class/net")?;

    while let Some(entry) = entries.next_entry().await? {
        let name = entry.file_name().to_string_lossy().to_string();
        let base = format!("/sys/class/net/{name}");

        let mac = tokio::fs::read_to_string(format!("{base}/address"))
            .await
            .unwrap_or_default()
            .trim()
            .to_string();
        let mtu: u32 = tokio::fs::read_to_string(format!("{base}/mtu"))
            .await
            .unwrap_or_default()
            .trim()
            .parse()
            .unwrap_or(0);
        let ifindex: u32 = tokio::fs::read_to_string(format!("{base}/ifindex"))
            .await
            .unwrap_or_default()
            .trim()
            .parse()
            .unwrap_or(0);
        let operstate = tokio::fs::read_to_string(format!("{base}/operstate"))
            .await
            .unwrap_or_default()
            .trim()
            .to_uppercase();

        interfaces.push(NetworkInterfaceInfo {
            name,
            index: ifindex,
            mac_address: mac,
            state: operstate,
            mtu,
            ipv4_addresses: vec![],
            ipv6_addresses: vec![],
            rx_bytes: 0,
            tx_bytes: 0,
            rx_packets: 0,
            tx_packets: 0,
            driver: String::new(),
            speed_mbps: 0,
        });
    }

    Ok(ListInterfacesOutput { interfaces })
}

async fn get_numa_topology() -> Result<NumaTopologyOutput> {
    let mut nodes = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir("/sys/devices/system/node").await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("node") {
                continue;
            }
            let node_id: u32 = name
                .strip_prefix("node")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            let meminfo_path = format!("/sys/devices/system/node/{name}/meminfo");
            let meminfo = tokio::fs::read_to_string(&meminfo_path)
                .await
                .unwrap_or_default();
            let mem_total = parse_node_meminfo_kb(&meminfo, "MemTotal") * 1024;
            let mem_free = parse_node_meminfo_kb(&meminfo, "MemFree") * 1024;

            nodes.push(NumaNodeInfo {
                node_id,
                cpus: vec![],
                memory_total_bytes: mem_total,
                memory_free_bytes: mem_free,
                memory_used_bytes: mem_total.saturating_sub(mem_free),
            });
        }
    }
    Ok(NumaTopologyOutput { nodes })
}

async fn check_unix_sockets(paths: &[String]) -> Result<CheckUnixSocketsOutput> {
    if paths.is_empty() {
        anyhow::bail!("paths must not be empty");
    }
    if paths.len() > 64 {
        anyhow::bail!("paths exceeds limit of 64");
    }
    let mut sockets = Vec::with_capacity(paths.len());
    for path in paths {
        sockets.push(probe_unix_socket(path).await);
    }
    Ok(CheckUnixSocketsOutput {
        sockets,
        queried_at: chrono::Utc::now().to_rfc3339(),
    })
}

fn root_disk_used_percent() -> Option<f64> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;

    let path = CString::new("/").ok()?;
    let mut stat = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `statvfs` writes through the out-pointer on success (rc == 0).
    let rc = unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    let stat = unsafe { stat.assume_init() };
    let total_bytes = stat.f_blocks as u64 * stat.f_frsize as u64;
    let free_bytes = stat.f_bavail as u64 * stat.f_frsize as u64;
    if total_bytes == 0 {
        return None;
    }
    Some(total_bytes.saturating_sub(free_bytes) as f64 / total_bytes as f64 * 100.0)
}

async fn sample_metrics(input: SampleMetricsInput) -> Result<SampleMetricsOutput> {
    let mut metrics = Vec::new();

    let meminfo = tokio::fs::read_to_string("/proc/meminfo")
        .await
        .unwrap_or_default();
    let total = parse_meminfo_kb(&meminfo, "MemTotal") * 1024;
    let available = parse_meminfo_kb(&meminfo, "MemAvailable") * 1024;
    if total > 0 {
        let used_percent = total.saturating_sub(available) as f64 / total as f64 * 100.0;
        metrics.push(MetricSample {
            category: "memory".to_string(),
            name: "used_percent".to_string(),
            value: used_percent,
            unit: "percent".to_string(),
            labels: Default::default(),
        });
    }

    let stat = tokio::fs::read_to_string("/proc/stat")
        .await
        .unwrap_or_default();
    let mut cpu_idle = 0u64;
    let mut cpu_total = 0u64;
    if let Some(fields) = stat
        .lines()
        .next()
        .filter(|l| l.starts_with("cpu "))
        .map(|l| {
            l.split_whitespace()
                .skip(1)
                .filter_map(|f| f.parse::<u64>().ok())
                .collect::<Vec<_>>()
        })
    {
        if fields.len() >= 4 {
            let idle = fields[3] + fields.get(4).copied().unwrap_or(0);
            cpu_idle = idle;
            cpu_total = fields.iter().sum();
            if let (Some(prev_idle), Some(prev_total)) =
                (input.previous_cpu_idle, input.previous_cpu_total)
            {
                let delta_total = cpu_total.saturating_sub(prev_total);
                let delta_idle = cpu_idle.saturating_sub(prev_idle);
                if delta_total > 0 {
                    let used_percent =
                        delta_total.saturating_sub(delta_idle) as f64 / delta_total as f64 * 100.0;
                    metrics.push(MetricSample {
                        category: "cpu".to_string(),
                        name: "used_percent".to_string(),
                        value: used_percent,
                        unit: "percent".to_string(),
                        labels: Default::default(),
                    });
                }
            }
        }
    }

    if let Some(used_percent) = root_disk_used_percent() {
        metrics.push(MetricSample {
            category: "disk".to_string(),
            name: "used_percent".to_string(),
            value: used_percent,
            unit: "percent".to_string(),
            labels: Default::default(),
        });
    }

    let mut rx_total = 0u64;
    let mut tx_total = 0u64;
    if let Ok(net) = tokio::fs::read_to_string("/proc/net/dev").await {
        for line in net.lines().skip(2) {
            if let Some((iface, stats)) = line.split_once(':') {
                if iface.trim() == "lo" {
                    continue;
                }
                let vals: Vec<u64> = stats
                    .split_whitespace()
                    .map(|v| v.parse::<u64>().unwrap_or(0))
                    .collect();
                if vals.len() >= 9 {
                    rx_total += vals[0];
                    tx_total += vals[8];
                }
            }
        }
    }
    if let (Some(prev_rx), Some(prev_tx)) = (input.previous_rx, input.previous_tx) {
        let rx_rate = rx_total.saturating_sub(prev_rx) as f64;
        let tx_rate = tx_total.saturating_sub(prev_tx) as f64;
        let mbps = (rx_rate + tx_rate) * 8.0 / 1_000_000.0;
        metrics.push(MetricSample {
            category: "network".to_string(),
            name: "throughput_mbps".to_string(),
            value: mbps,
            unit: "mbps".to_string(),
            labels: Default::default(),
        });
    }

    Ok(SampleMetricsOutput {
        metrics,
        cpu_idle,
        cpu_total,
        rx_total,
        tx_total,
        sampled_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub async fn dispatch_host_runtime_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    match method {
        "get_system_info" => Ok(serde_json::to_value(get_system_info().await?)?),
        "list_services" => {
            let input: ListServicesInput =
                serde_json::from_value(args.clone()).unwrap_or(ListServicesInput {
                    state_filter: String::new(),
                });
            Ok(serde_json::to_value(
                list_services(&input.state_filter).await?,
            )?)
        }
        "get_service" => {
            let input: GetServiceInput = serde_json::from_value(args.clone())
                .map_err(|e| anyhow::anyhow!("invalid get_service args: {e}"))?;
            Ok(serde_json::to_value(
                get_service(&input.service_name).await?,
            )?)
        }
        "list_interfaces" => Ok(serde_json::to_value(list_interfaces().await?)?),
        "get_numa_topology" => Ok(serde_json::to_value(get_numa_topology().await?)?),
        "check_unix_sockets" => {
            let input: CheckUnixSocketsInput = serde_json::from_value(args.clone())
                .map_err(|e| anyhow::anyhow!("invalid check_unix_sockets args: {e}"))?;
            Ok(serde_json::to_value(
                check_unix_sockets(&input.paths).await?,
            )?)
        }
        "sample_metrics" => {
            let input: SampleMetricsInput =
                serde_json::from_value(args.clone()).unwrap_or(SampleMetricsInput {
                    previous_cpu_idle: None,
                    previous_cpu_total: None,
                    previous_rx: None,
                    previous_tx: None,
                });
            Ok(serde_json::to_value(sample_metrics(input).await?)?)
        }
        other => Err(anyhow::anyhow!("unknown host_runtime method: {other}")),
    }
}

inventory::submit! {
    crate::default_registry::PluginReg::new("host_runtime", |_ctx| std::sync::Arc::new(HostRuntimePlugin::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_exposes_dispatched_methods() {
        let schema = host_runtime_schema();
        for method in [
            "get_system_info",
            "list_services",
            "get_service",
            "list_interfaces",
            "get_numa_topology",
            "check_unix_sockets",
            "sample_metrics",
        ] {
            assert!(schema.methods.contains_key(method), "missing {method}");
        }
    }
}
