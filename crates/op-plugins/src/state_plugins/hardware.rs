use anyhow::{bail, Result};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::PluginSchema;
use serde::{Deserialize, Serialize};
use simd_json::prelude::*;
use simd_json::OwnedValue as Value;
use sysinfo::{Disks, System};

/// Hardware inventory snapshot.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[schemars(extend("x-oscal-subid" = "sch.software.plugin.hardware.schema@v1"))]
#[schemars(extend("x-oscal-category" = "hardware"))]
pub struct HardwareState {
    /// CPU information.
    #[serde(default)]
    #[schemars(
        description = "CPU information",
        extend("x-oscal-subid" = "exp.service.hardware.cpu.render@v1")
    )]
    pub cpu: CpuInfo,

    /// Memory information.
    #[serde(default)]
    #[schemars(
        description = "Memory information",
        extend("x-oscal-subid" = "exp.service.hardware.memory.render@v1")
    )]
    pub memory: MemoryInfo,

    /// Disk inventory.
    #[serde(default)]
    #[schemars(
        description = "Disk inventory",
        extend("x-oscal-subid" = "exp.service.hardware.disks.render@v1")
    )]
    pub disks: Vec<DiskInfo>,
}

/// CPU information.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
pub struct CpuInfo {
    /// CPU model name.
    #[schemars(
        description = "CPU model name",
        example = &"AMD Ryzen 9 7950X",
        extend("x-oscal-subid" = "exp.service.hardware.cpu.model.declare@v1")
    )]
    pub model: Option<String>,

    /// Number of CPU cores.
    #[schemars(
        description = "Number of CPU cores",
        example = 16,
        extend("x-oscal-subid" = "exp.service.hardware.cpu.cores.declare@v1")
    )]
    pub cores: usize,
}

/// Memory information.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
pub struct MemoryInfo {
    /// Total memory in kilobytes.
    #[schemars(
        description = "Total memory in kilobytes",
        example = 33554432,
        extend("x-oscal-subid" = "exp.service.hardware.memory.total-kb.declare@v1")
    )]
    pub total_kb: u64,

    /// Available memory in kilobytes.
    #[schemars(
        description = "Available memory in kilobytes",
        example = 16777216,
        extend("x-oscal-subid" = "exp.service.hardware.memory.available-kb.declare@v1")
    )]
    pub available_kb: u64,
}

/// Disk information.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DiskInfo {
    /// Disk device name.
    #[schemars(
        description = "Disk device name",
        example = &"nvme0n1",
        extend("x-oscal-subid" = "exp.service.hardware.disk.name.declare@v1")
    )]
    pub name: String,

    /// Capacity of the mounted filesystem in bytes.
    #[schemars(
        description = "Mounted filesystem capacity in bytes",
        example = 1000204886016_i64,
        extend("x-oscal-subid" = "exp.service.hardware.disk.capacity-bytes.declare@v1")
    )]
    pub capacity_bytes: u64,

    /// Currently available space on the mounted filesystem in bytes.
    pub available_bytes: u64,

    /// Mount point, if any.
    #[serde(default)]
    #[schemars(
        description = "Mount point, if any",
        example = &"/",
        extend("x-oscal-subid" = "exp.service.hardware.disk.mountpoint.declare@v1")
    )]
    pub mountpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ScanHardwareOutput {
    pub observed_at: String,
    pub inventory: HardwareState,
    /// `observed` means this request took a fresh native snapshot; it does not
    /// claim a kernel hotplug rescan or a mutation.
    pub disposition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RefreshInventoryOutput {
    pub observed_at: String,
    pub inventory: HardwareState,
    /// `observed` means this request took a fresh native snapshot; it does not
    /// claim a kernel hotplug rescan or a mutation.
    pub disposition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ListDevicesOutput {
    pub observed_at: String,
    pub devices: Vec<DiskInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetDeviceInput {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetDeviceOutput {
    pub observed_at: String,
    pub device: Option<DiskInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GetStatsOutput {
    pub observed_at: String,
    pub cpu: CpuInfo,
    pub memory: MemoryInfo,
    pub disk_count: usize,
}

/// Method output paired with the inventory captured by the same native
/// snapshot. The caller commits `state` before returning `output`.
#[derive(Debug, Clone)]
pub struct HardwareObservedDispatch {
    pub output: serde_json::Value,
    pub state: HardwareState,
}

pub struct HardwarePlugin;

impl Default for HardwarePlugin {
    fn default() -> Self {
        Self
    }
}

impl HardwarePlugin {
    pub fn new() -> Self {
        Self
    }

    fn snapshot() -> Result<(String, HardwareState)> {
        let mut system = System::new();
        system.refresh_memory();
        system.refresh_cpu();
        validate_native_inventory(
            system.total_memory(),
            system.available_memory(),
            system.cpus().len(),
        )?;
        let cpu = system.cpus().first();
        let disks = Disks::new_with_refreshed_list();
        let inventory = HardwareState {
            cpu: CpuInfo {
                model: cpu.and_then(|cpu| nonempty(cpu.brand())),
                cores: system.cpus().len(),
            },
            memory: MemoryInfo {
                total_kb: bytes_to_kib(system.total_memory()),
                available_kb: bytes_to_kib(system.available_memory()),
            },
            disks: disks
                .list()
                .iter()
                .map(|disk| DiskInfo {
                    name: disk.name().to_string_lossy().into_owned(),
                    capacity_bytes: disk.total_space(),
                    available_bytes: disk.available_space(),
                    mountpoint: nonempty(&disk.mount_point().to_string_lossy()),
                })
                .collect(),
        };
        Ok((chrono::Utc::now().to_rfc3339(), inventory))
    }

    pub fn dispatch_hardware_method(
        method: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        Ok(Self::dispatch_hardware_method_observed(method, args)?.output)
    }

    pub fn dispatch_hardware_method_observed(
        method: &str,
        args: &serde_json::Value,
    ) -> Result<HardwareObservedDispatch> {
        let (observed_at, inventory) = Self::snapshot()?;
        Self::dispatch_snapshot_observed(method, args, observed_at, inventory)
    }

    fn dispatch_snapshot_observed(
        method: &str,
        args: &serde_json::Value,
        observed_at: String,
        inventory: HardwareState,
    ) -> Result<HardwareObservedDispatch> {
        validate_hardware_state(&inventory)?;
        let output = Self::dispatch_snapshot(method, args, observed_at, inventory.clone())?;
        Ok(HardwareObservedDispatch {
            output,
            state: inventory,
        })
    }

    fn dispatch_snapshot(
        method: &str,
        args: &serde_json::Value,
        observed_at: String,
        inventory: HardwareState,
    ) -> Result<serde_json::Value> {
        match method {
            "list_devices" => Ok(serde_json::to_value(ListDevicesOutput {
                observed_at,
                devices: inventory.disks.clone(),
            })?),
            "get_device" => {
                let input: GetDeviceInput = serde_json::from_value(args.clone())?;
                if input.name.trim().is_empty() {
                    bail!("device name must not be empty");
                }
                Ok(serde_json::to_value(GetDeviceOutput {
                    observed_at,
                    device: inventory
                        .disks
                        .iter()
                        .find(|disk| disk.name == input.name)
                        .cloned(),
                })?)
            }
            "get_stats" => Ok(serde_json::to_value(GetStatsOutput {
                observed_at,
                cpu: inventory.cpu,
                memory: inventory.memory,
                disk_count: inventory.disks.len(),
            })?),
            "scan_hardware" => Ok(serde_json::to_value(ScanHardwareOutput {
                observed_at,
                inventory,
                disposition: "observed".to_string(),
            })?),
            "refresh_inventory" => Ok(serde_json::to_value(RefreshInventoryOutput {
                observed_at,
                inventory,
                disposition: "observed".to_string(),
            })?),
            _ => bail!("unsupported hardware method: {method}"),
        }
    }
}

fn validate_native_inventory(
    total_memory_bytes: u64,
    available_memory_bytes: u64,
    cpu_count: usize,
) -> Result<()> {
    if total_memory_bytes == 0 {
        bail!("native hardware snapshot reported zero total memory");
    }
    if available_memory_bytes > total_memory_bytes {
        bail!("native hardware snapshot reported available memory greater than total memory");
    }
    if cpu_count == 0 {
        bail!("native hardware snapshot reported no CPUs");
    }
    Ok(())
}

fn validate_hardware_state(inventory: &HardwareState) -> Result<()> {
    if inventory.memory.total_kb == 0 {
        bail!("hardware observation reported zero total memory");
    }
    if inventory.memory.available_kb > inventory.memory.total_kb {
        bail!("hardware observation reported available memory greater than total memory");
    }
    if inventory.cpu.cores == 0 {
        bail!("hardware observation reported no CPU cores");
    }
    Ok(())
}

fn nonempty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn bytes_to_kib(bytes: u64) -> u64 {
    bytes / 1024
}

#[async_trait]
impl StatePlugin for HardwarePlugin {
    fn name(&self) -> &str {
        "hardware"
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn schema(&self) -> Option<op_state_store::PluginSchema> {
        Some(hardware_schema())
    }

    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().to_string(),
            actions: vec![],
            metadata: DiffMetadata {
                timestamp: chrono::Utc::now().timestamp(),
                current_hash: "unknown".to_string(),
                desired_hash: "unknown".to_string(),
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
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().to_string(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: Value::null(),
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

/// Derived `hardware` schema from the `HardwareState` struct.
pub fn hardware_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(HardwareState))
        .expect("schemars schema serializes to JSON");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "hardware",
        "1.0.0",
        "Hardware inventory snapshot",
        &root,
    );

    // Add methods
    use super::plugin_scaffold_helpers::{method_decl_from_schemars_with_output, EmptyInput};
    use op_state_store::SideEffect;

    schema.methods.insert(
        "list_devices".to_string(),
        method_decl_from_schemars_with_output::<EmptyInput, ListDevicesOutput>(
            "list_devices",
            SideEffect::Read,
            true,
            "hardware.read",
            "obs.service.hardware.device.list@v1",
        ),
    );
    schema.methods.insert(
        "get_device".to_string(),
        method_decl_from_schemars_with_output::<GetDeviceInput, GetDeviceOutput>(
            "get_device",
            SideEffect::Read,
            true,
            "hardware.read",
            "obs.service.hardware.device.get@v1",
        ),
    );
    schema.methods.insert(
        "scan_hardware".to_string(),
        method_decl_from_schemars_with_output::<EmptyInput, ScanHardwareOutput>(
            "scan_hardware",
            SideEffect::Read,
            true,
            "hardware.read",
            "obs.service.hardware.scan@v1",
        ),
    );
    schema.methods.insert(
        "get_stats".to_string(),
        method_decl_from_schemars_with_output::<EmptyInput, GetStatsOutput>(
            "get_stats",
            SideEffect::Read,
            true,
            "hardware.read",
            "obs.service.hardware.stats@v1",
        ),
    );
    schema.methods.insert(
        "refresh_inventory".to_string(),
        method_decl_from_schemars_with_output::<EmptyInput, RefreshInventoryOutput>(
            "refresh_inventory",
            SideEffect::Read,
            true,
            "hardware.read",
            "obs.service.hardware.refresh@v1",
        ),
    );

    schema.capabilities.insert(
        "hardware.read".to_string(),
        op_state_store::CapabilityDecl {
            id: "hardware.read".to_string(),
            description:
                "Grants: list_devices, get_device, scan_hardware, get_stats, refresh_inventory."
                    .to_string(),
        },
    );
    // Publish inventory without mount paths. Array-item rules retain disk
    // identity and capacity fields while withholding other filesystem detail.
    schema.public_state.paths = vec![
        "/cpu/cores".to_string(),
        "/memory".to_string(),
        "/disks/*/name".to_string(),
        "/disks/*/capacity_bytes".to_string(),
        "/disks/*/available_bytes".to_string(),
    ];
    schema
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> HardwareState {
        HardwareState {
            cpu: CpuInfo {
                model: Some("fixture-cpu".into()),
                cores: 4,
            },
            memory: MemoryInfo {
                total_kb: 1024,
                available_kb: 512,
            },
            disks: vec![DiskInfo {
                name: "fixture0".into(),
                capacity_bytes: 4096,
                available_bytes: 1024,
                mountpoint: Some("/mnt".into()),
            }],
        }
    }

    #[test]
    fn all_inventory_methods_use_typed_fixture_and_filter_devices() {
        let state = fixture();
        let list = HardwarePlugin::dispatch_snapshot(
            "list_devices",
            &serde_json::json!({}),
            "t".into(),
            state.clone(),
        )
        .unwrap();
        assert_eq!(list["devices"][0]["name"], "fixture0");
        let one = HardwarePlugin::dispatch_snapshot(
            "get_device",
            &serde_json::json!({"name":"fixture0"}),
            "t".into(),
            state.clone(),
        )
        .unwrap();
        assert_eq!(one["device"]["capacity_bytes"], 4096);
        let missing = HardwarePlugin::dispatch_snapshot(
            "get_device",
            &serde_json::json!({"name":"missing"}),
            "t".into(),
            state.clone(),
        )
        .unwrap();
        assert!(missing["device"].is_null());
        let stats = HardwarePlugin::dispatch_snapshot(
            "get_stats",
            &serde_json::json!({}),
            "t".into(),
            state.clone(),
        )
        .unwrap();
        assert_eq!(stats["cpu"]["cores"], 4);
        for method in ["scan_hardware", "refresh_inventory"] {
            let output = HardwarePlugin::dispatch_snapshot(
                method,
                &serde_json::json!({}),
                "t".into(),
                state.clone(),
            )
            .unwrap();
            assert_eq!(output["disposition"], "observed");
            assert_eq!(output["inventory"]["cpu"]["model"], "fixture-cpu");
        }
        assert!(HardwarePlugin::dispatch_snapshot(
            "get_device",
            &serde_json::json!({"name":" "}),
            "t".into(),
            state,
        )
        .is_err());
    }

    #[test]
    fn observed_dispatch_pairs_output_with_the_same_complete_inventory() {
        let receipt = HardwarePlugin::dispatch_snapshot_observed(
            "get_stats",
            &serde_json::json!({}),
            "t".into(),
            fixture(),
        )
        .unwrap();
        assert_eq!(receipt.output["cpu"]["cores"], 4);
        assert_eq!(receipt.state.cpu.cores, 4);
        assert_eq!(receipt.state.disks[0].name, "fixture0");
    }

    #[test]
    fn invalid_native_inventory_is_rejected_before_publication() {
        assert!(validate_native_inventory(0, 0, 1).is_err());
        assert!(validate_native_inventory(1024, 1025, 1).is_err());
        assert!(validate_native_inventory(1024, 0, 0).is_err());
        assert!(validate_native_inventory(1024, 0, 1).is_ok());

        let mut invalid = fixture();
        invalid.cpu.cores = 0;
        assert!(HardwarePlugin::dispatch_snapshot_observed(
            "get_stats",
            &serde_json::json!({}),
            "t".into(),
            invalid,
        )
        .is_err());

        let mut invalid = fixture();
        invalid.memory.available_kb = invalid.memory.total_kb + 1;
        assert!(HardwarePlugin::dispatch_snapshot_observed(
            "get_stats",
            &serde_json::json!({}),
            "t".into(),
            invalid,
        )
        .is_err());
    }

    #[test]
    fn default_and_actual_inventory_satisfy_the_public_policy() {
        for state in [HardwareState::default(), fixture()] {
            let state = simd_json::serde::to_owned_value(&state).unwrap();
            let projected = hardware_schema()
                .project_public_state(&state)
                .unwrap()
                .expect("hardware has an explicit public policy");
            let projected = serde_json::to_value(projected).unwrap();
            assert!(projected["cpu"].get("model").is_none());
            if let Some(disk) = projected["disks"]
                .as_array()
                .and_then(|disks| disks.first())
            {
                assert!(disk.get("mountpoint").is_none());
                assert!(disk.get("name").is_some());
                assert!(disk.get("capacity_bytes").is_some());
                assert!(disk.get("available_bytes").is_some());
            }
        }
    }

    #[test]
    fn schema_method_keys_match_declared_names() {
        let schema = hardware_schema();
        assert_eq!(schema.methods.len(), 5);
        for (key, method) in &schema.methods {
            assert_eq!(key, &method.name);
        }
    }

    #[test]
    fn native_byte_counts_are_converted_to_kibibytes() {
        assert_eq!(bytes_to_kib(2049), 2);
    }

    /// Every `x-oscal-subid` annotation in the derived schema must be a valid
    /// OSCAL subid according to the canonical taxonomy.
    #[test]
    fn all_subids_are_valid() {
        let raw = serde_json::to_value(schemars::schema_for!(HardwareState)).unwrap();
        let mut subids = Vec::new();
        collect_subids(&raw, &mut subids);
        assert!(!subids.is_empty(), "expected at least one subid");
        for subid in subids {
            crate::state_plugins::common::oscal::validate_subid(&subid)
                .unwrap_or_else(|e| panic!("invalid subid {subid}: {e}"));
        }
    }

    fn collect_subids(value: &serde_json::Value, out: &mut Vec<String>) {
        if let serde_json::Value::Object(map) = value {
            if let Some(subid) = map.get("x-oscal-subid").and_then(|v| v.as_str()) {
                out.push(subid.to_string());
            }
            for v in map.values() {
                collect_subids(v, out);
            }
        } else if let serde_json::Value::Array(arr) = value {
            for v in arr {
                collect_subids(v, out);
            }
        }
    }
}

// Self-registration: the plugin registry discovers this via inventory
// (single source of the catalog; no central dispatch list).
inventory::submit! {
    crate::default_registry::PluginReg::new("hardware", |_ctx| std::sync::Arc::new(HardwarePlugin::new()))
}
