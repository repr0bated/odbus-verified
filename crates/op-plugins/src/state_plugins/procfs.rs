//! Procfs state plugin.
//!
//! This turns procfs-derived host state into a `PluginSchema`-backed plugin so
//! JSON rendering, D-Bus projection, and tool generation all consume the same
//! schema authority instead of ad-hoc `/proc` tools.

use anyhow::{Result, bail};
use async_trait::async_trait;
use op_state::{
    ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateAction, StateDiff, StatePlugin,
};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use sysinfo::{CpuRefreshKind, Networks, Pid, ProcessRefreshKind, System};

/// Runtime procfs state, derived from `/proc` files.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(
    extend("x-oscal-subid" = "sch.software.plugin.procfs.schema@v2"),
    extend("x-immutable-paths" = [
        "/memory",
        "/loadavg",
        "/uptime",
        "/cpuinfo",
        "/stat",
        "/net_dev",
        "/mounts",
        "/kernel",
        "/vmstat",
        "/diskstats"
    ])
)]
#[schemars(extend("x-oscal-category" = "service"))]
pub struct ProcfsState {
    /// Parsed /proc/meminfo values.
    #[serde(default)]
    #[schemars(
        description = "Parsed /proc/meminfo values.",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.service.procfs.memory.query@v2")
    )]
    pub memory: MemInfo,
    /// Parsed /proc/loadavg values.
    #[serde(default)]
    #[schemars(
        description = "Parsed /proc/loadavg values.",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.service.procfs.loadavg.query@v1")
    )]
    pub loadavg: LoadAvg,
    /// Parsed /proc/uptime values.
    #[serde(default)]
    #[schemars(
        description = "Parsed /proc/uptime values.",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.service.procfs.uptime.query@v1")
    )]
    pub uptime: Uptime,
    /// Parsed CPU inventory from /proc/cpuinfo.
    #[serde(default)]
    #[schemars(
        description = "Parsed CPU inventory from /proc/cpuinfo.",
        with = "serde_json::Value",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.service.procfs.cpuinfo.query@v1")
    )]
    pub cpuinfo: Value,
    /// Parsed /proc/stat values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Parsed /proc/stat values.",
        with = "serde_json::Value",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.service.procfs.stat.query@v1")
    )]
    pub stat: Option<Value>,
    /// Parsed /proc/net/dev counters.
    #[serde(default)]
    #[schemars(
        description = "Parsed /proc/net/dev counters.",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.service.procfs.net-dev.query@v1")
    )]
    pub net_dev: NetDev,
    /// Parsed /proc/mounts entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Parsed /proc/mounts entries.",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.service.procfs.mounts.query@v1")
    )]
    pub mounts: Option<Vec<MountEntry>>,
    /// Kernel version from /proc/version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Kernel version from /proc/version.",
        with = "serde_json::Value",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.service.procfs.kernel.query@v1")
    )]
    pub kernel: Option<Value>,
    /// Parsed /proc/vmstat values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Parsed /proc/vmstat values.",
        with = "serde_json::Value",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.service.procfs.vmstat.query@v1")
    )]
    pub vmstat: Option<Value>,
    /// Parsed /proc/diskstats rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(
        description = "Parsed /proc/diskstats rows.",
        with = "serde_json::Value",
        extend("readOnly" = true),
        extend("x-oscal-subid" = "obs.service.procfs.diskstats.query@v1")
    )]
    pub diskstats: Option<Value>,
}

/// Memory counters reported by the same native snapshot as method output.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MemInfo {
    pub total_kb: u64,
    pub available_kb: u64,
    pub used_kb: u64,
}

/// Load averages reported by the native system information provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LoadAvg {
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
}

/// Host uptime reported by the native system information provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Uptime {
    pub uptime_secs: u64,
}

/// Cumulative counters supported by `sysinfo` for one network interface.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NetDevInterface {
    pub interface: String,
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub rx_errors: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
    pub tx_errors: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct NetDev {
    pub interfaces: Vec<NetDevInterface>,
}

/// One mounted filesystem. This remains part of the projected state contract;
/// the retained method surface does not synthesize it from unsupported data.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MountEntry {
    pub device: String,
    pub mountpoint: String,
    pub fstype: String,
    pub options: String,
}

pub struct ProcfsPlugin;

impl ProcfsPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ProcfsPlugin {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StatePlugin for ProcfsPlugin {
    fn name(&self) -> &str {
        "procfs"
    }

    fn version(&self) -> &str {
        "2.0.0"
    }

    fn is_available(&self) -> bool {
        true
    }

    fn unavailable_reason(&self) -> String {
        "sysinfo native host snapshot unavailable".to_string()
    }

    fn schema(&self) -> Option<PluginSchema> {
        Some(procfs_schema())
    }

    async fn calculate_diff(&self, current: &Value, desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![StateAction::NoOp {
                resource: "procfs".to_string(),
            }],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: format!("{:x}", md5::compute(simd_json::to_string(current)?)),
                desired_hash: format!("{:x}", md5::compute(simd_json::to_string(desired)?)),
            },
        })
    }

    async fn apply_state(&self, _diff: &StateDiff) -> Result<ApplyResult> {
        Ok(ApplyResult {
            success: true,
            changes_applied: Vec::new(),
            errors: Vec::new(),
            checkpoint: None,
        })
    }

    async fn verify_state(&self, _desired: &Value) -> Result<bool> {
        Ok(true)
    }

    async fn create_checkpoint(&self) -> Result<Checkpoint> {
        Ok(Checkpoint {
            id: format!("procfs-{}", chrono::Utc::now().timestamp()),
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
            atomic_operations: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ProcessSnapshot {
    pub pid: i32,
    pub name: String,
    pub memory_bytes: u64,
    pub virtual_memory_bytes: u64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListProcessesOutput {
    pub observed_at: String,
    pub processes: Vec<ProcessSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetProcessInfoOutput {
    pub observed_at: String,
    pub process: Option<ProcessSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetMeminfoOutput {
    pub observed_at: String,
    pub total_kb: u64,
    pub available_kb: u64,
    pub used_kb: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CpuSnapshot {
    pub name: String,
    pub brand: Option<String>,
    pub frequency_mhz: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetCpuinfoOutput {
    pub observed_at: String,
    pub cpus: Vec<CpuSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetLoadavgOutput {
    pub observed_at: String,
    pub load1: f64,
    pub load5: f64,
    pub load15: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetUptimeOutput {
    pub observed_at: String,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetNetDevOutput {
    pub observed_at: String,
    pub interfaces: Vec<NetDevInterface>,
}

/// Method output paired with the complete state observed by the same native
/// snapshot. The caller commits `state` before returning `output`.
#[derive(Debug, Clone)]
pub struct ProcfsObservedDispatch {
    pub output: serde_json::Value,
    pub state: ProcfsState,
}

#[derive(Debug, Clone)]
struct ProcfsSnapshot {
    observed_at: String,
    processes: Vec<ProcessSnapshot>,
    total_memory_bytes: u64,
    available_memory_bytes: u64,
    cpus: Vec<CpuSnapshot>,
    loadavg: LoadAvg,
    uptime_secs: u64,
    interfaces: Vec<NetDevInterface>,
}

fn native_snapshot() -> Result<ProcfsSnapshot> {
    let mut system = System::new();
    system.refresh_memory();
    system.refresh_cpu_specifics(CpuRefreshKind::new().with_frequency());
    system.refresh_processes_specifics(ProcessRefreshKind::new().with_memory());
    let networks = Networks::new_with_refreshed_list();
    snapshot_from_native(
        chrono::Utc::now().to_rfc3339(),
        &system,
        &networks,
        System::load_average(),
        System::uptime(),
    )
}

fn snapshot_from_native(
    observed_at: String,
    system: &System,
    networks: &Networks,
    loadavg: sysinfo::LoadAvg,
    uptime_secs: u64,
) -> Result<ProcfsSnapshot> {
    let mut processes = system
        .processes()
        .iter()
        .map(|(pid, process)| process_snapshot(*pid, process))
        .collect::<Result<Vec<_>>>()?;
    processes.sort_unstable_by_key(|process| process.pid);

    let cpus = system
        .cpus()
        .iter()
        .map(|cpu| CpuSnapshot {
            name: cpu.name().to_string(),
            brand: nonempty(cpu.brand()),
            frequency_mhz: (cpu.frequency() > 0).then_some(cpu.frequency()),
        })
        .collect();

    let mut interfaces = networks
        .iter()
        .map(|(name, network)| NetDevInterface {
            interface: name.clone(),
            rx_bytes: network.total_received(),
            rx_packets: network.total_packets_received(),
            rx_errors: network.total_errors_on_received(),
            tx_bytes: network.total_transmitted(),
            tx_packets: network.total_packets_transmitted(),
            tx_errors: network.total_errors_on_transmitted(),
        })
        .collect::<Vec<_>>();
    interfaces.sort_unstable_by(|left, right| left.interface.cmp(&right.interface));

    Ok(ProcfsSnapshot {
        observed_at,
        processes,
        total_memory_bytes: system.total_memory(),
        available_memory_bytes: system.available_memory(),
        cpus,
        loadavg: LoadAvg {
            load1: loadavg.one,
            load5: loadavg.five,
            load15: loadavg.fifteen,
        },
        uptime_secs,
        interfaces,
    })
}

fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn process_snapshot(pid: Pid, process: &sysinfo::Process) -> Result<ProcessSnapshot> {
    Ok(ProcessSnapshot {
        pid: i32::try_from(pid.as_u32()).map_err(|_| anyhow::anyhow!("pid exceeds i32 range"))?,
        name: process.name().to_string(),
        memory_bytes: process.memory(),
        virtual_memory_bytes: process.virtual_memory(),
        status: process.status().to_string(),
    })
}

fn bytes_to_kib(bytes: u64) -> u64 {
    bytes / 1024
}

pub fn dispatch_procfs_method(method: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
    Ok(dispatch_procfs_method_observed(method, args)?.output)
}

pub fn dispatch_procfs_method_observed(
    method: &str,
    args: &serde_json::Value,
) -> Result<ProcfsObservedDispatch> {
    dispatch_procfs_snapshot_observed(method, args, native_snapshot()?)
}

fn dispatch_procfs_snapshot(
    method: &str,
    args: &serde_json::Value,
    snapshot: ProcfsSnapshot,
) -> Result<serde_json::Value> {
    Ok(dispatch_procfs_snapshot_observed(method, args, snapshot)?.output)
}

fn dispatch_procfs_snapshot_observed(
    method: &str,
    args: &serde_json::Value,
    snapshot: ProcfsSnapshot,
) -> Result<ProcfsObservedDispatch> {
    validate_procfs_snapshot(&snapshot)?;
    let state = procfs_state_from_snapshot(&snapshot)?;
    let ProcfsSnapshot {
        observed_at,
        processes,
        total_memory_bytes,
        available_memory_bytes,
        cpus,
        loadavg,
        uptime_secs,
        interfaces,
    } = snapshot;

    let output = match method {
        "list_processes" => Ok::<_, anyhow::Error>(serde_json::to_value(ListProcessesOutput {
            observed_at,
            processes,
        })?),
        "get_process_info" => {
            let input: ProcessInfoInput = serde_json::from_value(args.clone())?;
            if input.pid < 0 {
                bail!("pid must be non-negative");
            }
            Ok(serde_json::to_value(GetProcessInfoOutput {
                observed_at,
                process: processes
                    .into_iter()
                    .find(|process| process.pid == input.pid),
            })?)
        }
        "get_meminfo" => Ok(serde_json::to_value(GetMeminfoOutput {
            observed_at,
            total_kb: bytes_to_kib(total_memory_bytes),
            available_kb: bytes_to_kib(available_memory_bytes),
            used_kb: bytes_to_kib(total_memory_bytes.saturating_sub(available_memory_bytes)),
        })?),
        "get_cpuinfo" => Ok(serde_json::to_value(GetCpuinfoOutput {
            observed_at,
            cpus,
        })?),
        "get_loadavg" => Ok(serde_json::to_value(GetLoadavgOutput {
            observed_at,
            load1: loadavg.load1,
            load5: loadavg.load5,
            load15: loadavg.load15,
        })?),
        "get_uptime" => Ok(serde_json::to_value(GetUptimeOutput {
            observed_at,
            uptime_secs,
        })?),
        "get_net_dev" => Ok(serde_json::to_value(GetNetDevOutput {
            observed_at,
            interfaces,
        })?),
        _ => bail!("unsupported procfs method: {method}"),
    }?;
    Ok(ProcfsObservedDispatch { output, state })
}

fn validate_procfs_snapshot(snapshot: &ProcfsSnapshot) -> Result<()> {
    if snapshot.total_memory_bytes == 0 {
        bail!("native procfs snapshot reported zero total memory");
    }
    if snapshot.available_memory_bytes > snapshot.total_memory_bytes {
        bail!("native procfs snapshot reported available memory greater than total memory");
    }
    if snapshot.cpus.is_empty() {
        bail!("native procfs snapshot reported no CPUs");
    }
    Ok(())
}

fn procfs_state_from_snapshot(snapshot: &ProcfsSnapshot) -> Result<ProcfsState> {
    let total_kb = bytes_to_kib(snapshot.total_memory_bytes);
    let available_kb = bytes_to_kib(snapshot.available_memory_bytes);
    Ok(ProcfsState {
        memory: MemInfo {
            total_kb,
            available_kb,
            used_kb: bytes_to_kib(
                snapshot
                    .total_memory_bytes
                    .saturating_sub(snapshot.available_memory_bytes),
            ),
        },
        loadavg: snapshot.loadavg.clone(),
        uptime: Uptime {
            uptime_secs: snapshot.uptime_secs,
        },
        cpuinfo: simd_json::serde::to_owned_value(&snapshot.cpus)?,
        stat: None,
        net_dev: NetDev {
            interfaces: snapshot.interfaces.clone(),
        },
        mounts: None,
        kernel: None,
        vmstat: None,
        diskstats: None,
    })
}

/// Procfs schema derived from the typed [`ProcfsState`] struct via schemars.
pub(crate) fn procfs_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(ProcfsState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "procfs",
        "2.0.0",
        "Read-only procfs host state projected through PluginSchema.",
        &root,
    );

    schema.methods.insert(
        "list_processes".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            super::plugin_scaffold_helpers::EmptyInput,
            ListProcessesOutput,
        >(
            "list_processes",
            op_state_store::SideEffect::Read,
            true,
            "procfs.read",
            "obs.service.procfs.process.list@v1",
        ),
    );
    schema.methods.insert(
        "get_process_info".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            ProcessInfoInput,
            GetProcessInfoOutput,
        >(
            "get_process_info",
            op_state_store::SideEffect::Read,
            true,
            "procfs.read",
            "obs.service.procfs.process.get@v1",
        ),
    );
    schema.methods.insert(
        "get_meminfo".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            super::plugin_scaffold_helpers::EmptyInput,
            GetMeminfoOutput,
        >(
            "get_meminfo",
            op_state_store::SideEffect::Read,
            true,
            "procfs.read",
            "obs.service.procfs.meminfo.get@v1",
        ),
    );
    schema.methods.insert(
        "get_cpuinfo".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            super::plugin_scaffold_helpers::EmptyInput,
            GetCpuinfoOutput,
        >(
            "get_cpuinfo",
            op_state_store::SideEffect::Read,
            true,
            "procfs.read",
            "obs.service.procfs.cpuinfo.get@v1",
        ),
    );
    schema.methods.insert(
        "get_loadavg".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            super::plugin_scaffold_helpers::EmptyInput,
            GetLoadavgOutput,
        >(
            "get_loadavg",
            op_state_store::SideEffect::Read,
            true,
            "procfs.read",
            "obs.service.procfs.loadavg.get@v1",
        ),
    );
    schema.methods.insert(
        "get_uptime".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            super::plugin_scaffold_helpers::EmptyInput,
            GetUptimeOutput,
        >(
            "get_uptime",
            op_state_store::SideEffect::Read,
            true,
            "procfs.read",
            "obs.service.procfs.uptime.get@v1",
        ),
    );
    schema.methods.insert(
        "get_net_dev".to_string(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            super::plugin_scaffold_helpers::EmptyInput,
            GetNetDevOutput,
        >(
            "get_net_dev",
            op_state_store::SideEffect::Read,
            true,
            "procfs.read",
            "obs.service.procfs.net-dev.get@v1",
        ),
    );

    schema.capabilities.insert(
        "procfs.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "procfs.read".to_string(),
            description:
                "Grants: list_processes, get_process_info, get_meminfo, get_cpuinfo, get_loadavg, get_uptime, get_net_dev."
                    .to_string(),
        },
    );

    // Shared readers receive only bounded, typed host telemetry. Opaque
    // procfs documents, mount paths, process arguments, and open-ended maps
    // remain internal.
    schema.public_state.paths = vec![
        "/memory".to_string(),
        "/loadavg".to_string(),
        "/uptime".to_string(),
        "/net_dev".to_string(),
    ];

    schema
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ProcessInfoInput {
    pub pid: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_plugins::common::oscal::validate_subid;
    use serde_json::Value as JVal;

    fn fixture() -> ProcfsSnapshot {
        ProcfsSnapshot {
            observed_at: "2026-09-13T00:00:00Z".to_string(),
            processes: vec![ProcessSnapshot {
                pid: 42,
                name: "fixture-process".to_string(),
                memory_bytes: 4096,
                virtual_memory_bytes: 8192,
                status: "Run".to_string(),
            }],
            total_memory_bytes: 8193,
            available_memory_bytes: 2048,
            cpus: vec![CpuSnapshot {
                name: "cpu0".to_string(),
                brand: Some("fixture-cpu".to_string()),
                frequency_mhz: Some(3200),
            }],
            loadavg: LoadAvg {
                load1: 1.25,
                load5: 0.75,
                load15: 0.5,
            },
            uptime_secs: 1234,
            interfaces: vec![NetDevInterface {
                interface: "eth0".to_string(),
                rx_bytes: 10,
                rx_packets: 2,
                rx_errors: 1,
                tx_bytes: 20,
                tx_packets: 4,
                tx_errors: 3,
            }],
        }
    }

    fn collect_subids(value: &JVal, out: &mut Vec<String>) {
        if let Some(obj) = value.as_object() {
            if let Some(JVal::String(subid)) = obj.get("x-oscal-subid") {
                out.push(subid.clone());
            }
            for v in obj.values() {
                collect_subids(v, out);
            }
        }
        if let Some(arr) = value.as_array() {
            for v in arr {
                collect_subids(v, out);
            }
        }
    }

    #[test]
    fn all_subids_are_valid() {
        let root = serde_json::to_value(schemars::schema_for!(ProcfsState))
            .expect("schemars schema serializes to JSON");
        let mut subids = Vec::new();
        collect_subids(&root, &mut subids);
        assert!(!subids.is_empty(), "expected at least one subid");
        for subid in subids {
            assert!(validate_subid(&subid).is_ok(), "invalid subid: {subid}");
        }
    }

    #[test]
    fn all_seven_methods_dispatch_from_one_typed_fixture() {
        let empty = serde_json::json!({});

        let processes = dispatch_procfs_snapshot("list_processes", &empty, fixture()).unwrap();
        assert_eq!(processes["processes"][0]["pid"], 42);
        assert!(processes["processes"][0].get("command").is_none());

        let process = dispatch_procfs_snapshot(
            "get_process_info",
            &serde_json::json!({"pid": 42}),
            fixture(),
        )
        .unwrap();
        assert_eq!(process["process"]["name"], "fixture-process");

        let memory = dispatch_procfs_snapshot("get_meminfo", &empty, fixture()).unwrap();
        assert_eq!(memory["total_kb"], 8);
        assert_eq!(memory["available_kb"], 2);
        assert_eq!(memory["used_kb"], 6);

        let cpu = dispatch_procfs_snapshot("get_cpuinfo", &empty, fixture()).unwrap();
        assert_eq!(cpu["cpus"][0]["frequency_mhz"], 3200);

        let load = dispatch_procfs_snapshot("get_loadavg", &empty, fixture()).unwrap();
        assert_eq!(load["load1"], 1.25);

        let uptime = dispatch_procfs_snapshot("get_uptime", &empty, fixture()).unwrap();
        assert_eq!(uptime["uptime_secs"], 1234);

        let network = dispatch_procfs_snapshot("get_net_dev", &empty, fixture()).unwrap();
        assert_eq!(network["interfaces"][0]["rx_errors"], 1);
        assert!(network["interfaces"][0].get("rx_drop").is_none());
    }

    #[test]
    fn observed_dispatch_keeps_method_output_separate_from_plugin_state() {
        let receipt =
            dispatch_procfs_snapshot_observed("get_meminfo", &serde_json::json!({}), fixture())
                .unwrap();
        assert_eq!(receipt.output["total_kb"], 8);
        assert_eq!(receipt.state.memory.total_kb, 8);
        assert_eq!(receipt.state.memory.available_kb, 2);
        assert_eq!(receipt.state.memory.used_kb, 6);
        assert_eq!(receipt.state.net_dev.interfaces[0].interface, "eth0");

        let serialized = serde_json::to_value(&receipt.state).unwrap();
        for unobserved in ["stat", "mounts", "kernel", "vmstat", "diskstats"] {
            assert!(
                serialized.get(unobserved).is_none(),
                "{unobserved} must not be fabricated by the sysinfo snapshot"
            );
        }
        assert!(serialized.get("observed_at").is_none());
        assert!(serialized.get("processes").is_none());
    }

    #[test]
    fn actual_observed_state_projects_only_declared_public_telemetry() {
        let state = procfs_state_from_snapshot(&fixture()).unwrap();
        let state = simd_json::serde::to_owned_value(&state).unwrap();
        let projected = procfs_schema()
            .project_public_state(&state)
            .unwrap()
            .expect("procfs has an explicit public policy");
        let projected = serde_json::to_value(projected).unwrap();

        assert_eq!(projected["memory"]["total_kb"], 8);
        assert_eq!(projected["loadavg"]["load1"], 1.25);
        assert_eq!(projected["uptime"]["uptime_secs"], 1234);
        assert_eq!(projected["net_dev"]["interfaces"][0]["interface"], "eth0");
        assert!(projected.get("cpuinfo").is_none());
        assert!(projected.get("stat").is_none());
        assert!(projected.get("mounts").is_none());
    }

    #[test]
    fn process_lookup_rejects_invalid_pid_and_represents_missing_process() {
        assert!(
            dispatch_procfs_snapshot(
                "get_process_info",
                &serde_json::json!({"pid": -1}),
                fixture(),
            )
            .is_err()
        );
        assert!(
            dispatch_procfs_snapshot("get_process_info", &serde_json::json!({}), fixture(),)
                .is_err()
        );

        let missing = dispatch_procfs_snapshot(
            "get_process_info",
            &serde_json::json!({"pid": 404}),
            fixture(),
        )
        .unwrap();
        assert!(missing["process"].is_null());
    }

    #[test]
    fn invalid_native_snapshots_are_rejected_before_publication() {
        let mut zero_total = fixture();
        zero_total.total_memory_bytes = 0;
        assert!(
            dispatch_procfs_snapshot_observed("get_meminfo", &serde_json::json!({}), zero_total,)
                .is_err()
        );

        let mut impossible_available = fixture();
        impossible_available.available_memory_bytes = impossible_available.total_memory_bytes + 1;
        assert!(
            dispatch_procfs_snapshot_observed(
                "get_meminfo",
                &serde_json::json!({}),
                impossible_available,
            )
            .is_err()
        );

        let mut no_cpus = fixture();
        no_cpus.cpus.clear();
        assert!(
            dispatch_procfs_snapshot_observed("get_cpuinfo", &serde_json::json!({}), no_cpus,)
                .is_err()
        );

        let mut zero_available_and_frequency = fixture();
        zero_available_and_frequency.available_memory_bytes = 0;
        zero_available_and_frequency.cpus[0].frequency_mhz = None;
        assert!(
            dispatch_procfs_snapshot_observed(
                "get_cpuinfo",
                &serde_json::json!({}),
                zero_available_and_frequency,
            )
            .is_ok()
        );
    }

    #[test]
    fn schema_method_keys_match_declared_names() {
        let schema = procfs_schema();
        assert_eq!(schema.methods.len(), 7);
        for (key, method) in &schema.methods {
            assert_eq!(key, &method.name);
        }
    }

    #[test]
    fn native_bytes_are_converted_to_kibibytes_at_the_output_boundary() {
        assert_eq!(bytes_to_kib(2049), 2);
    }

    #[test]
    fn plugin_and_schema_versions_match_v2_memory_contract() {
        assert_eq!(ProcfsPlugin::new().version(), "2.0.0");
        assert_eq!(procfs_schema().version, "2.0.0");
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("procfs", |_ctx| std::sync::Arc::new(ProcfsPlugin::new()))
}

#[cfg(test)]
mod typed_field_tests {
    use super::procfs_schema;
    use op_state_store::FieldType;

    /// Untyped fields are why procfs rendered as `cpuinfo: any`, generated ten
    /// `google.protobuf.Value`s, and materialised an all-null projection.
    #[test]
    fn retyped_fields_are_no_longer_any() {
        let schema = procfs_schema();
        for name in ["loadavg", "uptime", "memory", "net_dev", "mounts"] {
            let f = schema.fields.get(name).expect("field present");
            assert!(
                !matches!(f.field_type, FieldType::Any),
                "{name} is still FieldType::Any"
            );
        }
    }

    #[test]
    fn loadavg_exposes_its_real_subfields() {
        let schema = procfs_schema();
        let FieldType::Object(fields) = &schema.fields["loadavg"].field_type else {
            panic!(
                "loadavg should be an object, got {:?}",
                schema.fields["loadavg"].field_type
            );
        };
        for (name, expected) in [
            ("load1", FieldType::Float),
            ("load5", FieldType::Float),
            ("load15", FieldType::Float),
        ] {
            let got = &fields
                .get(name)
                .unwrap_or_else(|| panic!("{name} missing"))
                .field_type;
            assert_eq!(*got, expected, "{name} typed as {got:?}");
        }
    }

    #[test]
    fn uptime_exposes_the_supported_integer_seconds() {
        let schema = procfs_schema();
        let FieldType::Object(fields) = &schema.fields["uptime"].field_type else {
            panic!("uptime should be an object");
        };
        assert_eq!(fields["uptime_secs"].field_type, FieldType::Integer);
        assert_eq!(fields.len(), 1);
    }
}

#[cfg(test)]
mod collection_field_tests {
    use super::procfs_schema;
    use op_state_store::FieldType;

    #[test]
    fn mounts_is_an_array_of_typed_rows() {
        let schema = procfs_schema();
        let FieldType::Array(item) = &schema.fields["mounts"].field_type else {
            panic!(
                "mounts should be an array, got {:?}",
                schema.fields["mounts"].field_type
            );
        };
        let FieldType::Object(cols) = item.as_ref() else {
            panic!("mount rows should be objects, got {item:?}");
        };
        for c in ["device", "mountpoint", "fstype", "options"] {
            assert_eq!(cols[c].field_type, FieldType::String, "{c}");
        }
    }

    #[test]
    fn net_dev_counters_are_integers_not_strings() {
        // num_or_str typed these by whatever the value happened to look like;
        // a counter that reads 0 must not be a different type from one at 10^9.
        let schema = procfs_schema();
        let FieldType::Object(top) = &schema.fields["net_dev"].field_type else {
            panic!("net_dev should be an object");
        };
        let FieldType::Array(item) = &top["interfaces"].field_type else {
            panic!("interfaces should be an array");
        };
        let FieldType::Object(cols) = item.as_ref() else {
            panic!("interface rows should be objects");
        };
        assert_eq!(cols["interface"].field_type, FieldType::String);
        for c in [
            "rx_bytes",
            "rx_packets",
            "rx_errors",
            "tx_bytes",
            "tx_packets",
            "tx_errors",
        ] {
            assert_eq!(cols[c].field_type, FieldType::Integer, "{c}");
        }
    }
}
