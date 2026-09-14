//! Read-only PCI inventory backed by libudev.
//!
//! Kernel PCI identity fields are udev sysattrs. Human-readable hardware
//! database names are udev properties and remain optional.

use anyhow::{Context, Result};
use async_trait::async_trait;
use op_state::{ApplyResult, Checkpoint, DiffMetadata, PluginCapabilities, StateDiff, StatePlugin};
use op_state_store::{PluginSchema, SideEffect};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue as Value;
use std::ffi::OsStr;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("x-oscal-subid" = "sch.hardware.pci.inventory@v2"))]
pub struct PciState {
    pub provider: String,
    pub mutation_supported: bool,
}

impl Default for PciState {
    fn default() -> Self {
        Self {
            provider: "libudev".into(),
            mutation_supported: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnumerateDevicesInput {
    /// Four hexadecimal digits, without the sysfs `0x` prefix.
    #[serde(default)]
    pub vendor_id: Option<String>,
    /// Six hexadecimal digits, without the sysfs `0x` prefix.
    #[serde(default)]
    pub class_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDeviceInfoInput {
    /// Canonical domain:bus:slot.function PCI address, for example 0000:00:1f.6.
    pub address: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PciDeviceRecord {
    pub address: String,
    pub vendor_id: String,
    pub device_id: String,
    pub class_code: String,
    pub subsystem_vendor_id: Option<String>,
    pub subsystem_device_id: Option<String>,
    pub revision: Option<String>,
    pub driver: Option<String>,
    pub vendor_name: Option<String>,
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EnumerateDevicesOutput {
    pub vendor_id_filter: Option<String>,
    pub class_code_filter: Option<String>,
    pub devices: Vec<PciDeviceRecord>,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetDeviceInfoOutput {
    pub address: String,
    pub found: bool,
    pub device: Option<PciDeviceRecord>,
}

#[derive(Debug, Clone)]
struct ScanRequest {
    address: Option<String>,
    vendor_id: Option<String>,
    class_code: Option<String>,
}

trait PciProvider: Send + Sync {
    fn scan(&self, request: &ScanRequest) -> Result<Vec<PciDeviceRecord>>;
}

struct UdevPciProvider;

fn normalized_hex(raw: &str, width: usize, field: &str) -> Result<String> {
    anyhow::ensure!(
        raw.len() == width && raw.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "{field} must contain exactly {width} hexadecimal digits without a 0x prefix"
    );
    Ok(raw.to_ascii_lowercase())
}

fn normalized_address(raw: &str) -> Result<String> {
    anyhow::ensure!(
        raw.is_ascii()
            && raw.len() == 12
            && raw.as_bytes()[4] == b':'
            && raw.as_bytes()[7] == b':'
            && raw.as_bytes()[10] == b'.',
        "PCI address must use exact domain:bus:slot.function syntax"
    );
    let domain = normalized_hex(&raw[0..4], 4, "PCI domain")?;
    let bus = normalized_hex(&raw[5..7], 2, "PCI bus")?;
    let slot = normalized_hex(&raw[8..10], 2, "PCI slot")?;
    let function = normalized_hex(&raw[11..12], 1, "PCI function")?;
    anyhow::ensure!(
        u8::from_str_radix(&slot, 16)? <= 0x1f,
        "PCI slot must be between 00 and 1f"
    );
    anyhow::ensure!(
        u8::from_str_radix(&function, 16)? <= 7,
        "PCI function must be between 0 and 7"
    );
    Ok(format!("{domain}:{bus}:{slot}.{function}"))
}

fn sysattr(device: &udev::Device, name: &str, width: usize) -> Result<String> {
    let value = device
        .attribute_value(name)
        .with_context(|| format!("PCI device is missing required '{name}' sysattr"))?
        .to_str()
        .with_context(|| format!("PCI '{name}' sysattr is not UTF-8"))?;
    normalized_hex(value.strip_prefix("0x").unwrap_or(value), width, name)
}

fn optional_sysattr(device: &udev::Device, name: &str, width: usize) -> Result<Option<String>> {
    device
        .attribute_value(name)
        .map(|value| {
            let value = value
                .to_str()
                .with_context(|| format!("PCI '{name}' sysattr is not UTF-8"))?;
            normalized_hex(value.strip_prefix("0x").unwrap_or(value), width, name)
        })
        .transpose()
}

fn optional_property(device: &udev::Device, name: &str) -> Option<String> {
    device
        .property_value(name)
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.is_empty())
}

fn device_record(device: &udev::Device) -> Result<PciDeviceRecord> {
    let address = device
        .sysname()
        .to_str()
        .context("PCI device sysname is not UTF-8")?;
    Ok(PciDeviceRecord {
        address: normalized_address(address)?,
        vendor_id: sysattr(device, "vendor", 4)?,
        device_id: sysattr(device, "device", 4)?,
        class_code: sysattr(device, "class", 6)?,
        subsystem_vendor_id: optional_sysattr(device, "subsystem_vendor", 4)?,
        subsystem_device_id: optional_sysattr(device, "subsystem_device", 4)?,
        revision: optional_sysattr(device, "revision", 2)?,
        driver: device
            .driver()
            .map(OsStr::to_string_lossy)
            .map(|value| value.into_owned()),
        vendor_name: optional_property(device, "ID_VENDOR_FROM_DATABASE"),
        device_name: optional_property(device, "ID_MODEL_FROM_DATABASE"),
    })
}

impl PciProvider for UdevPciProvider {
    fn scan(&self, request: &ScanRequest) -> Result<Vec<PciDeviceRecord>> {
        let mut enumerator = udev::Enumerator::new().context("create udev enumerator")?;
        enumerator
            .match_subsystem("pci")
            .context("filter udev PCI subsystem")?;
        if let Some(address) = &request.address {
            enumerator
                .match_sysname(address)
                .context("filter udev PCI address")?;
        }
        if let Some(vendor) = &request.vendor_id {
            enumerator
                .match_attribute("vendor", format!("0x{vendor}"))
                .context("filter udev PCI vendor sysattr")?;
        }
        if let Some(class) = &request.class_code {
            enumerator
                .match_attribute("class", format!("0x{class}"))
                .context("filter udev PCI class sysattr")?;
        }
        let devices = enumerator.scan_devices().context("scan udev PCI devices")?;
        let mut records = devices
            .map(|device| device_record(&device))
            .collect::<Result<Vec<_>>>()?;
        records.sort_by(|left, right| left.address.cmp(&right.address));
        Ok(records)
    }
}

fn dispatch_with_provider(
    provider: &dyn PciProvider,
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    match method {
        "enumerate_devices" => {
            let input = serde_json::from_value::<EnumerateDevicesInput>(args.clone())
                .context("invalid enumerate_devices arguments")?;
            let vendor_id = input
                .vendor_id
                .map(|value| normalized_hex(&value, 4, "vendor_id"))
                .transpose()?;
            let class_code = input
                .class_code
                .map(|value| normalized_hex(&value, 6, "class_code"))
                .transpose()?;
            let devices = provider.scan(&ScanRequest {
                address: None,
                vendor_id: vendor_id.clone(),
                class_code: class_code.clone(),
            })?;
            Ok(serde_json::to_value(EnumerateDevicesOutput {
                vendor_id_filter: vendor_id,
                class_code_filter: class_code,
                total: u64::try_from(devices.len()).context("PCI device count overflow")?,
                devices,
            })?)
        }
        "get_device_info" => {
            let input = serde_json::from_value::<GetDeviceInfoInput>(args.clone())
                .context("invalid get_device_info arguments")?;
            let address = normalized_address(&input.address)?;
            let mut devices = provider.scan(&ScanRequest {
                address: Some(address.clone()),
                vendor_id: None,
                class_code: None,
            })?;
            anyhow::ensure!(devices.len() <= 1, "udev returned duplicate PCI addresses");
            let device = devices.pop();
            Ok(serde_json::to_value(GetDeviceInfoOutput {
                address,
                found: device.is_some(),
                device,
            })?)
        }
        other => anyhow::bail!("unknown pcidecl method '{other}'"),
    }
}

pub async fn dispatch_pcidecl_method(
    method: &str,
    args: &serde_json::Value,
) -> Result<serde_json::Value> {
    let method = method.to_owned();
    let args = args.clone();
    tokio::task::spawn_blocking(move || dispatch_with_provider(&UdevPciProvider, &method, &args))
        .await
        .context("PCI udev provider task failed")?
}

pub(crate) fn pcidecl_schema() -> PluginSchema {
    let root = serde_json::to_value(schemars::schema_for!(PciState)).expect("schema serializes");
    let mut schema = super::schemars_adapter::plugin_schema_from_json(
        "pcidecl",
        "2.0.0",
        "Read-only PCI device inventory from libudev",
        &root,
    );
    schema.category = "hardware".into();
    schema.methods.insert(
        "enumerate_devices".into(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            EnumerateDevicesInput,
            EnumerateDevicesOutput,
        >(
            "enumerate_devices",
            SideEffect::Read,
            true,
            "pcidecl.read",
            "obs.hardware.pci.devices.enumerate@v2",
        ),
    );
    schema.methods.insert(
        "get_device_info".into(),
        super::plugin_scaffold_helpers::method_decl_from_schemars_with_output::<
            GetDeviceInfoInput,
            GetDeviceInfoOutput,
        >(
            "get_device_info",
            SideEffect::Read,
            true,
            "pcidecl.read",
            "obs.hardware.pci.device.info@v2",
        ),
    );
    schema.capabilities.insert(
        "pcidecl.read".into(),
        op_state_store::CapabilityDecl {
            id: "pcidecl.read".into(),
            description: "Grants: enumerate_devices, get_device_info.".into(),
        },
    );
    schema.example = Some(simd_json::json!({"provider":"libudev","mutation_supported":false}));
    schema
}

pub struct PciDeclPlugin;
impl Default for PciDeclPlugin {
    fn default() -> Self {
        Self
    }
}
impl PciDeclPlugin {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl StatePlugin for PciDeclPlugin {
    fn name(&self) -> &str {
        "pcidecl"
    }
    fn version(&self) -> &str {
        "2.0.0"
    }
    fn schema(&self) -> Option<PluginSchema> {
        Some(pcidecl_schema())
    }
    async fn calculate_diff(&self, _current: &Value, _desired: &Value) -> Result<StateDiff> {
        Ok(StateDiff {
            plugin: self.name().into(),
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
            id: uuid::Uuid::new_v4().to_string(),
            plugin: self.name().into(),
            timestamp: chrono::Utc::now().timestamp(),
            state_snapshot: simd_json::serde::to_owned_value(PciState::default())?,
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

inventory::submit! {
    crate::default_registry::PluginReg::new("pcidecl", |_ctx| std::sync::Arc::new(PciDeclPlugin::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        records: Result<Vec<PciDeviceRecord>, &'static str>,
        request: std::sync::Mutex<Option<ScanRequest>>,
    }
    impl PciProvider for Fixture {
        fn scan(&self, request: &ScanRequest) -> Result<Vec<PciDeviceRecord>> {
            *self.request.lock().unwrap() = Some(request.clone());
            self.records.clone().map_err(anyhow::Error::msg)
        }
    }
    fn record(address: &str) -> PciDeviceRecord {
        PciDeviceRecord {
            address: address.into(),
            vendor_id: "8086".into(),
            device_id: "15f3".into(),
            class_code: "020000".into(),
            subsystem_vendor_id: Some("8086".into()),
            subsystem_device_id: Some("0000".into()),
            revision: Some("03".into()),
            driver: Some("igc".into()),
            vendor_name: None,
            device_name: None,
        }
    }
    fn fixture(records: Result<Vec<PciDeviceRecord>, &'static str>) -> Fixture {
        Fixture {
            records,
            request: std::sync::Mutex::new(None),
        }
    }

    #[test]
    fn enumerate_applies_typed_normalized_filters_and_returns_records() {
        let provider = fixture(Ok(vec![record("0000:00:1f.6")]));
        let output = dispatch_with_provider(
            &provider,
            "enumerate_devices",
            &serde_json::json!({"vendor_id":"8086","class_code":"020000"}),
        )
        .unwrap();
        assert_eq!(output["total"], 1);
        assert_eq!(output["devices"][0]["driver"], "igc");
        let request = provider.request.lock().unwrap();
        assert_eq!(request.as_ref().unwrap().vendor_id.as_deref(), Some("8086"));
        assert_eq!(
            request.as_ref().unwrap().class_code.as_deref(),
            Some("020000")
        );
    }

    #[test]
    fn exact_lookup_distinguishes_missing_from_provider_error() {
        let missing = fixture(Ok(vec![]));
        let output = dispatch_with_provider(
            &missing,
            "get_device_info",
            &serde_json::json!({"address":"0000:00:1F.6"}),
        )
        .unwrap();
        assert_eq!(output["address"], "0000:00:1f.6");
        assert_eq!(output["found"], false);
        assert!(output["device"].is_null());

        let failed = fixture(Err("udev unavailable"));
        let error = dispatch_with_provider(
            &failed,
            "get_device_info",
            &serde_json::json!({"address":"0000:00:1f.6"}),
        )
        .unwrap_err();
        assert!(error.to_string().contains("udev unavailable"));
    }

    #[test]
    fn malformed_addresses_and_filters_reject_before_provider() {
        for (method, args) in [
            ("get_device_info", serde_json::json!({"address":"00:1f.6"})),
            (
                "get_device_info",
                serde_json::json!({"address":"0000:00:20.0"}),
            ),
            (
                "enumerate_devices",
                serde_json::json!({"vendor_id":"0x8086"}),
            ),
            ("enumerate_devices", serde_json::json!({"class_code":"02"})),
        ] {
            let provider = fixture(Ok(vec![]));
            assert!(dispatch_with_provider(&provider, method, &args).is_err());
            assert!(provider.request.lock().unwrap().is_none());
        }
    }

    #[test]
    fn schema_has_only_two_typed_read_methods() {
        let schema = pcidecl_schema();
        assert_eq!(schema.methods.len(), 2);
        assert!(schema.methods.contains_key("enumerate_devices"));
        assert!(schema.methods.contains_key("get_device_info"));
        let encoded = serde_json::to_string(&schema).unwrap();
        assert!(!encoded.contains("driver_override"));
        assert!(!encoded.contains("AckOutput"));
    }
}
